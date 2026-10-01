use super::*;
use crate::{
    auth::{ActiveSession, NativeSessionLookupError, NativeSessionResolver, SecretHash},
    search::{
        Embedding, RankedStation, RepositoryError, SearchConstraints, SearchQuery, Station,
        StationHealth, StationRepository,
    },
};
use async_trait::async_trait;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tower::ServiceExt;
use uuid::Uuid;

#[path = "tests/metadata.rs"]
mod metadata;

struct Catalog(String);

#[async_trait]
impl StationRepository for Catalog {
    async fn search(
        &self,
        _: &SearchQuery,
        _: &SearchConstraints,
        _: Option<&Embedding>,
    ) -> Result<Vec<RankedStation>, RepositoryError> {
        Ok(vec![])
    }
    async fn check_readiness(&self) -> Result<(), RepositoryError> {
        Ok(())
    }
    async fn get_public(&self, id: &str) -> Result<Option<Station>, RepositoryError> {
        Ok((id == "station").then(|| Station {
            id: id.into(),
            name: "Test".into(),
            stream_url: self.0.clone(),
            homepage_url: None,
            favicon_url: None,
            tags: vec![],
            language: None,
            country_code: None,
            codec: None,
            bitrate_kbps: None,
            health: StationHealth::Healthy,
        }))
    }
}

struct Session;

#[async_trait]
impl NativeSessionResolver for Session {
    async fn resolve_active_native_session(
        &self,
        _: &SecretHash,
    ) -> Result<Option<ActiveSession>, NativeSessionLookupError> {
        Ok(Some(ActiveSession {
            session_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            device_id: Uuid::new_v4(),
        }))
    }
}

fn build_relay_router(url: String) -> (axum::Router, RelayState) {
    let mut state = super::super::builder::RouterBuilder::new()
        .with_repository(Arc::new(Catalog(url)))
        .with_control_session_resolver(Arc::new(Session))
        .build_state();
    state.relay.allow_loopback = true;
    let relay = state.relay.clone();
    (super::super::routes::build_router(state), relay)
}

async fn upstream(headers: &str, body: &'static [u8], hold: bool) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    let address = listener.local_addr().unwrap();
    let header_text = headers.to_owned();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            seen.fetch_add(1, Ordering::SeqCst);
            let mut request = [0; 2048];
            let _ = socket.read(&mut request).await;
            let _ = socket.write_all(header_text.as_bytes()).await;
            for chunk in body.chunks(3) {
                if socket.write_all(chunk).await.is_err() {
                    break;
                }
                tokio::task::yield_now().await;
            }
            if hold {
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }
    });
    (format!("http://{address}/radio"), count)
}

async fn get(router: axum::Router, path: &str, icy: bool, authorized: bool) -> Response {
    let mut builder = Request::get(path);
    if authorized {
        builder = builder.header(header::AUTHORIZATION, "Bearer native");
    }
    if icy {
        builder = builder.header("Icy-MetaData", "1");
    }
    router
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn rejects_unauthorized_and_client_url_before_upstream() {
    let (url, connections) = upstream("HTTP/1.1 200 OK\r\n\r\n", b"audio", false).await;
    let (router, _) = build_relay_router(url);
    assert_eq!(
        get(
            router.clone(),
            "/api/v1/stations/station/stream",
            false,
            false
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        get(
            router,
            "/api/v1/stations/station/stream?url=http://example.org/",
            false,
            true
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(connections.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn icy_is_forwarded_or_removed_and_title_is_saved_across_reads() {
    let mut body = b"ABCD".to_vec();
    body.push(2);
    body.extend_from_slice(b"StreamTitle='Song';");
    body.resize(4 + 1 + 32, 0);
    body.extend_from_slice(b"EFGH");
    body.push(0);
    let body = Box::leak(body.into_boxed_slice());
    let (url, _) = upstream("HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nicy-metaint: 4\r\nConnection: close\r\n\r\n", body, false).await;
    let (router, relay) = build_relay_router(url);
    let response = get(
        router.clone(),
        "/api/v1/stations/station/stream",
        true,
        true,
    )
    .await;
    assert_eq!(response.headers()["icy-metaint"], "4");
    assert_eq!(
        to_bytes(response.into_body(), 1000).await.unwrap().as_ref(),
        body
    );
    let response = get(router, "/api/v1/stations/station/stream", false, true).await;
    assert!(response.headers().get("icy-metaint").is_none());
    assert_eq!(
        to_bytes(response.into_body(), 1000).await.unwrap().as_ref(),
        b"ABCDEFGH"
    );
    assert_eq!(relay.titles.lock().unwrap()["station"].raw_title, "Song");
}

#[tokio::test]
async fn missing_and_broken_icy_keep_audio_and_disconnect_releases_slot() {
    let (url, _) = upstream(
        "HTTP/1.1 200 OK\r\nContent-Type: audio/aac\r\n\r\n",
        b"audio",
        false,
    )
    .await;
    let (router, _) = build_relay_router(url);
    let response = get(router, "/api/v1/stations/station/stream", false, true).await;
    assert_eq!(
        to_bytes(response.into_body(), 1000).await.unwrap().as_ref(),
        b"audio"
    );

    let mut broken = b"ABCD\x01".to_vec();
    broken.extend_from_slice(b"invalid-title");
    broken.resize(4 + 1 + 16, 0);
    broken.extend_from_slice(b"EFGH\x00");
    let broken = Box::leak(broken.into_boxed_slice());
    let (url, _) = upstream("HTTP/1.1 200 OK\r\nicy-metaint: 4\r\n\r\n", broken, false).await;
    let (router, relay) = build_relay_router(url);
    let response = get(router, "/api/v1/stations/station/stream", false, true).await;
    assert_eq!(
        to_bytes(response.into_body(), 1000).await.unwrap().as_ref(),
        b"ABCDEFGH"
    );
    assert!(relay.titles.lock().unwrap().is_empty());

    let (url, _) = upstream("HTTP/1.1 200 OK\r\n\r\n", b"audio", true).await;
    let (router, relay) = build_relay_router(url);
    let response = get(router, "/api/v1/stations/station/stream", false, true).await;
    drop(response);
    for _ in 0..20 {
        if relay.slots.available_permits() == MAX_CONNECTIONS {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("relay slot was not released after disconnect");
}

#[tokio::test]
async fn blocks_private_destinations_and_redirects() {
    let (router, _) = build_relay_router("http://169.254.169.254/latest/meta-data".into());
    assert_eq!(
        get(router, "/api/v1/stations/station/stream", false, true)
            .await
            .status(),
        StatusCode::BAD_GATEWAY
    );
    let (url, _) = upstream(
        "HTTP/1.1 302 Found\r\nLocation: http://169.254.169.254/latest/meta-data\r\n\r\n",
        b"",
        false,
    )
    .await;
    let (router, _) = build_relay_router(url);
    assert_eq!(
        get(router, "/api/v1/stations/station/stream", false, true)
            .await
            .status(),
        StatusCode::BAD_GATEWAY
    );
}
