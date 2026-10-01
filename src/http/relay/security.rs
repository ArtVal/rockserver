//! Destination validation and pinned DNS resolution for radio upstreams.

use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};

use reqwest::{Client, Url};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

// Reject non-routable and transition ranges before any socket is opened.
fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            !(v.is_private()
                || v.is_loopback()
                || v.is_link_local()
                || v.is_broadcast()
                || v.is_documentation()
                || v.is_unspecified()
                || v.is_multicast()
                || v.octets()[0] == 0
                || v.octets()[0] >= 224
                || v.octets()[0] == 100 && (64..=127).contains(&v.octets()[1])
                || v.octets()[0] == 169 && v.octets()[1] == 254
                || v.octets()[0] == 192 && v.octets()[1] == 0
                || v.octets()[0] == 198 && (18..=19).contains(&v.octets()[1]))
        }
        IpAddr::V6(v) => {
            if let Some(mapped) = v.to_ipv4_mapped() {
                return public_ip(IpAddr::V4(mapped));
            }
            !(v.is_loopback()
                || v.is_unspecified()
                || v.is_multicast()
                || v.is_unique_local()
                || v.is_unicast_link_local()
                || (v.segments()[0] & 0xffc0) == 0xfe80
                || v.segments()[0] == 0x64 && v.segments()[1] == 0xff9b
                || v.segments()[0] == 0x2002
                || v.segments()[0] == 0x2001 && v.segments()[1] == 0x0db8)
        }
    }
}

/// Validates every resolved address and pins the HTTP client to one checked destination.
pub(super) async fn pinned_client(url: &Url, allow_loopback: bool) -> Result<Client, ()> {
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(());
    }
    let host = url.host_str().ok_or(())?;
    let port = url.port_or_known_default().ok_or(())?;
    let addresses: Vec<SocketAddr> =
        tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::lookup_host((host, port)))
            .await
            .map_err(|_| ())?
            .map_err(|_| ())?
            .collect();
    if addresses.is_empty()
        || addresses.iter().any(|address| {
            !(public_ip(address.ip()) || allow_loopback && address.ip().is_loopback())
        })
    {
        return Err(());
    }
    Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT)
        .resolve(host, addresses[0])
        .build()
        .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    #[test]
    fn rejects_internal_addresses() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "169.254.169.254",
            "100.64.0.1",
            "192.0.0.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!public_ip(ip.parse().unwrap()), "{ip}");
        }
        assert!(public_ip(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
    }
}
