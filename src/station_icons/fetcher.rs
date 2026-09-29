//! External icon fetching and HTML favicon candidate discovery.
//!
//! Provides bounded HTTP downloading with SSRF mitigation, redirect destination validation,
//! and HTML icon link resolution.

use std::{net::IpAddr, time::Duration};

use async_trait::async_trait;
use reqwest::redirect::Policy;
use url::Url;

use super::domain::{
    IconSourceFetcher, IconStorageError, IconValidationError, MAX_SOURCE_BYTES, PreparedIcon,
    prepare_icon,
};

/// Maximum homepage HTML bytes inspected while resolving a favicon candidate.
///
/// One MiB: production sampling showed a third of failing homepages simply
/// exceeded the earlier 256 KiB bound while being otherwise valid.
const MAX_HOMEPAGE_BYTES: usize = 1024 * 1024;
/// Upper bound on the candidates inspected for one homepage.
const MAX_ICON_CANDIDATES: usize = 4;

/// Fetches external icons only after validating the URL and every redirect destination.
#[derive(Clone, Debug)]
pub struct SafeIconFetcher {
    client: reqwest::Client,
}

impl SafeIconFetcher {
    /// Creates a fetcher with bounded request time and no implicit redirects.
    ///
    /// A browser-like User-Agent is sent because many station sites block empty
    /// or tool-like agents outright; production sampling showed that a third of
    /// otherwise-failing homepages answer normally with one.
    pub fn new() -> Result<Self, IconStorageError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(Policy::none())
            .user_agent("Mozilla/5.0 (compatible; RockServer station-icon fetcher)")
            .build()
            .map_err(|_| IconStorageError::Unavailable)?;
        Ok(Self { client })
    }

    /// Downloads at most `limit` bytes from a validated publicly routable URL.
    ///
    /// Every redirect destination is re-validated against the same SSRF policy before the
    /// request is sent. An unsuccessful HTTP status classifies as `Format` because that URL
    /// is unusable, while transport failures classify as `Decode` so callers can retry.
    async fn fetch_bounded(
        &self,
        source: &str,
        limit: usize,
    ) -> Result<Vec<u8>, IconValidationError> {
        self.fetch_bounded_opt(source, limit, false).await
    }

    /// Downloads a bounded body; `truncate_oversize` stops reading at the limit instead of
    /// failing, which suits HTML inspection where the declared icon links sit in the document
    /// head and a heavy page must not hide them behind a size rejection.
    async fn fetch_bounded_opt(
        &self,
        source: &str,
        limit: usize,
        truncate_oversize: bool,
    ) -> Result<Vec<u8>, IconValidationError> {
        let mut url = Url::parse(source).map_err(|_| IconValidationError::Format)?;
        for _ in 0..=5 {
            validate_public_url(&url).await?;
            let mut response = self
                .client
                .get(url.clone())
                .send()
                .await
                .map_err(|_| IconValidationError::Decode)?;
            if response.status().is_redirection() {
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or(IconValidationError::Format)?;
                url = url
                    .join(location)
                    .map_err(|_| IconValidationError::Format)?;
                continue;
            }
            if !response.status().is_success() {
                return Err(IconValidationError::Format);
            }
            if !truncate_oversize
                && response
                    .content_length()
                    .is_some_and(|size| size as usize > limit)
            {
                return Err(IconValidationError::Size);
            }
            let mut body = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| IconValidationError::Decode)?
            {
                if body.len() + chunk.len() > limit {
                    if truncate_oversize {
                        break;
                    }
                    return Err(IconValidationError::Size);
                }
                body.extend_from_slice(&chunk);
            }
            return Ok(body);
        }
        Err(IconValidationError::Format)
    }
}

#[async_trait]
impl IconSourceFetcher for SafeIconFetcher {
    /// Downloads at most two MiB from a publicly routable HTTP(S) URL and normalizes it.
    async fn fetch_icon(&self, source: &str) -> Result<PreparedIcon, IconValidationError> {
        prepare_icon(&self.fetch_bounded(source, MAX_SOURCE_BYTES).await?)
    }

    /// Resolves the ordered favicon candidates for a validated station homepage.
    ///
    /// Returns every declared HTTP(S) `<link rel="...icon...">` target in document order
    /// followed by the homepage-root `/favicon.ico` fallback, deduplicated: production
    /// sampling showed both a deep-page relative link dying while the root icon lives and
    /// a first declared candidate being unusable, so the worker tries them in order.
    async fn discover_homepage_icons(
        &self,
        homepage: &str,
    ) -> Result<Vec<String>, IconValidationError> {
        // Heavy pages are truncated to the inspected prefix instead of failing: the declared
        // icon links live in the document head, and production sampling showed ~17% of
        // remaining permanent errors were pages heavier than the whole-body bound.
        let body = self
            .fetch_bounded_opt(homepage, MAX_HOMEPAGE_BYTES, true)
            .await?;
        let base = Url::parse(homepage).map_err(|_| IconValidationError::Format)?;
        Ok(homepage_icon_candidates(
            &String::from_utf8_lossy(&body),
            &base,
        ))
    }
}

/// Builds a homepage's ordered favicon candidates: declared icon links, then `/favicon.ico`.
pub(super) fn homepage_icon_candidates(html: &str, base: &Url) -> Vec<String> {
    let mut candidates = icon_links(html, base, MAX_ICON_CANDIDATES);
    let mut fallback = base
        .join("/favicon.ico")
        .expect("an absolute HTTP(S) base always joins a root-relative path");
    fallback.set_fragment(None);
    let fallback = fallback.to_string();
    if !candidates.contains(&fallback) {
        candidates.push(fallback);
    }
    candidates
}

/// Extracts up to `max` distinct HTTP(S) icon links declared by bounded HTML, in document
/// order, resolved against the base.
///
/// Accepts `rel` tokens `icon` and `apple-touch-icon` (this naturally excludes the SVG-only
/// `mask-icon` token) and skips `data:` targets, which cannot be fetched like a URL.
pub(super) fn icon_links(html: &str, base: &Url, max: usize) -> Vec<String> {
    let mut links: Vec<String> = Vec::new();
    if max == 0 {
        return links;
    }
    let lowered = html.to_ascii_lowercase();
    let mut cursor = 0;
    while let Some(found) = lowered[cursor..].find("<link") {
        let tag_start = cursor + found;
        cursor = tag_start + "<link".len();
        // `<link` must end the tag name so prefixes like `<linker` are not matched.
        let after_name = lowered.as_bytes().get(cursor).copied();
        let name_terminated = after_name.is_none()
            || matches!(after_name, Some(b'>') | Some(b'/'))
            || after_name.is_some_and(|byte| byte.is_ascii_whitespace());
        if !name_terminated {
            continue;
        }
        let Some(tag_end) = lowered[cursor..].find('>') else {
            break;
        };
        let tag_end = cursor + tag_end;
        let attributes = parse_link_attributes(&html[tag_start..=tag_end]);
        let Some((_, rel)) = attributes.iter().find(|(name, _)| name == "rel") else {
            cursor = tag_end + 1;
            continue;
        };
        let is_icon_rel = rel
            .to_ascii_lowercase()
            .split_ascii_whitespace()
            .any(|token| matches!(token, "icon" | "apple-touch-icon"));
        if is_icon_rel
            && let Some((_, href)) = attributes.iter().find(|(name, _)| name == "href")
            && let Some(resolved) = resolve_icon_href(&unescape_href(href), base)
            && !links.contains(&resolved)
        {
            links.push(resolved);
            if links.len() == max {
                return links;
            }
        }
        cursor = tag_end + 1;
    }
    links
}

/// Resolves one link target to an absolute fragment-free HTTP(S) URL.
fn resolve_icon_href(href: &str, base: &Url) -> Option<String> {
    if href.is_empty() || href.starts_with("data:") {
        return None;
    }
    let mut url = base.join(href).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    url.set_fragment(None);
    Some(url.to_string())
}

/// Parses one `<link>` tag's attributes; names are lowercased, values are kept verbatim.
///
/// Handles double-quoted, single-quoted and unquoted values; slice offsets only ever stop on
/// ASCII delimiters, so slicing never splits a multi-byte character.
fn parse_link_attributes(tag: &str) -> Vec<(String, String)> {
    let bytes = tag.as_bytes();
    let mut attributes = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        while index < bytes.len() && (bytes[index].is_ascii_whitespace() || bytes[index] == b'/') {
            index += 1;
        }
        if index >= bytes.len() {
            break;
        }
        let name_start = index;
        while index < bytes.len() && bytes[index] != b'=' && !bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let name = tag[name_start..index].to_ascii_lowercase();
        let mut value = String::new();
        if index < bytes.len() && bytes[index] == b'=' {
            index += 1;
            if matches!(bytes.get(index), Some(b'"') | Some(b'\'')) {
                let quote = bytes[index];
                index += 1;
                let value_start = index;
                while index < bytes.len() && bytes[index] != quote {
                    index += 1;
                }
                value = tag[value_start..index].to_owned();
                if index < bytes.len() {
                    index += 1;
                }
            } else {
                let value_start = index;
                while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
                    index += 1;
                }
                value = tag[value_start..index].to_owned();
            }
        }
        attributes.push((name, value));
    }
    attributes
}

/// Decodes the small set of character entities valid inside an attribute value.
fn unescape_href(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

async fn validate_public_url(url: &Url) -> Result<(), IconValidationError> {
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(IconValidationError::Format);
    }
    let host = url.host_str().expect("host was checked");
    if host.eq_ignore_ascii_case("localhost") {
        return Err(IconValidationError::Format);
    }
    let addresses = tokio::net::lookup_host((host, url.port_or_known_default().unwrap_or(80)))
        .await
        .map_err(|_| IconValidationError::Format)?;
    if addresses
        .into_iter()
        .any(|address| !public_ip(address.ip()))
    {
        return Err(IconValidationError::Format);
    }
    Ok(())
}

fn public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            !address.is_private()
                && !address.is_loopback()
                && !address.is_link_local()
                && !address.is_broadcast()
                && !address.is_unspecified()
                && !address.is_documentation()
        }
        IpAddr::V6(address) => {
            !address.is_loopback()
                && !address.is_unspecified()
                && !address.is_unique_local()
                && !address.is_unicast_link_local()
        }
    }
}
