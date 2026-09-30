//! Integration coverage for the device-session catalog routes (RS-2).
//!
//! Both routes are exercised through the full axum router with a deterministic fake
//! native-session resolver; no real account store, provider, or network is involved.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tower::ServiceExt;
use uuid::Uuid;

use rockserver::auth::{
    ActiveSession, NativeSessionLookupError, NativeSessionResolver, SecretHash,
};
use rockserver::http::router_with_search_service_and_native_session_resolver;
use rockserver::search::{
    Embedding, InMemoryStationRepository, RankedStation, RepositoryError, SearchConstraints,
    SearchQuery, SearchService, Station, StationHealth, StationRepository,
};

const BROWSE_PATH: &str = "/api/v1/device-control/catalog/stations";
const SEARCH_PATH: &str = "/api/v1/device-control/catalog/search";
const FIRST_DEVICE_TOKEN: &str = "first-device-native-session";
const SECOND_DEVICE_TOKEN: &str = "second-device-native-session";

/// Deterministic native-session resolver keyed by SHA-256 of the raw bearer value.
struct FakeResolver {
    sessions: Vec<(SecretHash, ActiveSession)>,
    unavailable: bool,
}

impl FakeResolver {
    /// Creates a resolver that authenticates `token` as the given device.
    fn for_device(token: &str, device_id: Uuid) -> Self {
        Self {
            sessions: vec![(
                hash_of(token),
                ActiveSession {
                    session_id: Uuid::new_v4(),
                    user_id: Uuid::new_v4(),
                    device_id,
                },
            )],
            unavailable: false,
        }
    }

    /// Creates a resolver that answers every lookup with "no active session".
    fn rejecting() -> Self {
        Self {
            sessions: Vec::new(),
            unavailable: false,
        }
    }

    /// Creates a resolver whose session storage cannot be consulted.
    fn unavailable() -> Self {
        Self {
            sessions: Vec::new(),
            unavailable: true,
        }
    }
}

fn hash_of(token: &str) -> SecretHash {
    let mut digest = [0_u8; 32];
    digest.copy_from_slice(&Sha256::digest(token.as_bytes()));
    SecretHash::new(digest)
}

#[async_trait::async_trait]
impl NativeSessionResolver for FakeResolver {
    async fn resolve_active_native_session(
        &self,
        access_hash: &SecretHash,
    ) -> Result<Option<ActiveSession>, NativeSessionLookupError> {
        if self.unavailable {
            return Err(NativeSessionLookupError);
        }
        Ok(self
            .sessions
            .iter()
            .find(|(hash, _)| hash == access_hash)
            .map(|(_, session)| *session))
    }
}

/// Repository fixture whose stations deliberately carry unusable stream URLs.
///
/// Device-facing pages must still return the station metadata: playback is resolved
/// server-side and never reads a catalog URL from the device.
struct FixedStreamRepository {
    stations: Vec<Station>,
    search_delay: Option<Duration>,
}

#[async_trait::async_trait]
impl StationRepository for FixedStreamRepository {
    async fn search(
        &self,
        query: &SearchQuery,
        constraints: &SearchConstraints,
        _embedding: Option<&Embedding>,
    ) -> Result<Vec<RankedStation>, RepositoryError> {
        if let Some(delay) = self.search_delay {
            tokio::time::sleep(delay).await;
        }
        Ok(self
            .stations
            .iter()
            .filter(|station| {
                query
                    .terms
                    .iter()
                    .any(|term| station.name.to_lowercase().contains(term))
                    || station
                        .tags
                        .iter()
                        .any(|tag| query.tags.iter().any(|wanted| wanted == tag))
            })
            .take(constraints.limit)
            .map(|station| RankedStation {
                station: station.clone(),
                score: 1.0,
                reason: "fixture match".to_owned(),
                total_matches: None,
            })
            .collect())
    }

    async fn check_readiness(&self) -> Result<(), RepositoryError> {
        Ok(())
    }

    async fn list_public(
        &self,
        after_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Station>, RepositoryError> {
        Ok(self
            .stations
            .iter()
            .filter(|station| after_id.is_none_or(|after| station.id.as_str() > after))
            .take(limit)
            .cloned()
            .collect())
    }
}

fn station(id: &str, name: &str, stream_url: &str, tags: &[&str]) -> Station {
    Station {
        id: id.to_owned(),
        name: name.to_owned(),
        stream_url: stream_url.to_owned(),
        homepage_url: None,
        favicon_url: None,
        tags: tags.iter().map(|tag| tag.to_string()).collect(),
        language: Some("en".to_owned()),
        country_code: Some("GB".to_owned()),
        codec: Some("MP3".to_owned()),
        bitrate_kbps: Some(128),
        health: StationHealth::Healthy,
    }
}

fn app_with_built_in_catalog(resolver: Arc<FakeResolver>) -> Router {
    router_with_search_service_and_native_session_resolver(
        SearchService::new(Arc::new(
            InMemoryStationRepository::with_builtin_catalog().expect("builtin catalog must load"),
        )),
        Duration::from_secs(5),
        resolver,
    )
}

fn app_with_fixed_streams(resolver: Arc<FakeResolver>, timeout: Duration) -> Router {
    router_with_search_service_and_native_session_resolver(
        SearchService::new(Arc::new(FixedStreamRepository {
            stations: vec![
                station(
                    "station-broken-001",
                    "Broken URL Rock",
                    "not a valid url",
                    &["rock"],
                ),
                station(
                    "station-legacy-002",
                    "Legacy Scheme Jazz",
                    "ftp://legacy.example/stream",
                    &["jazz"],
                ),
            ],
            search_delay: None,
        })),
        timeout,
        resolver,
    )
}

async fn get(app: Router, uri: &str, token: Option<&str>) -> (StatusCode, HeaderMap, Value) {
    let mut request = Request::get(uri);
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let response = app
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    (status, headers, body)
}

fn station_ids(page: &Value) -> Vec<&str> {
    page["stations"]
        .as_array()
        .expect("device pages must list stations")
        .iter()
        .map(|station| station["id"].as_str().expect("station ids must be strings"))
        .collect()
}

#[tokio::test]
async fn browse_returns_one_bounded_stream_free_page() {
    let app = app_with_built_in_catalog(Arc::new(FakeResolver::for_device(
        FIRST_DEVICE_TOKEN,
        Uuid::new_v4(),
    )));

    let (status, headers, page) = get(app, BROWSE_PATH, Some(FIRST_DEVICE_TOKEN)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert!(headers.contains_key("x-request-id"));
    assert!(page["request_id"].as_str().is_some_and(|id| !id.is_empty()));
    let stations = page["stations"].as_array().unwrap();
    assert_eq!(
        stations.len(),
        20,
        "the default page must be exactly 20 of 41 stations"
    );
    for station in stations {
        for forbidden in ["stream_url", "score", "reason", "provider_id"] {
            assert!(
                station.get(forbidden).is_none(),
                "device pages must never expose {forbidden}"
            );
        }
        assert!(station["id"].as_str().is_some_and(|id| !id.is_empty()));
        assert!(
            station["name"]
                .as_str()
                .is_some_and(|name| !name.is_empty())
        );
        assert!(station["health"].as_str().is_some());
    }
    let next_cursor = page["next_cursor"]
        .as_str()
        .expect("a full page must offer a cursor");
    assert!(
        next_cursor.len() <= 512
            && next_cursor
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    );
    assert_eq!(
        next_cursor,
        stations.last().unwrap()["id"].as_str().unwrap()
    );
}

#[tokio::test]
async fn browse_paginates_strictly_after_the_stable_cursor() {
    let app = app_with_built_in_catalog(Arc::new(FakeResolver::for_device(
        FIRST_DEVICE_TOKEN,
        Uuid::new_v4(),
    )));

    let mut seen: Vec<String> = Vec::new();
    let mut cursor: Option<String> = None;
    let mut terminated = false;
    for _ in 0..30 {
        let uri = match &cursor {
            Some(cursor) => format!("{BROWSE_PATH}?limit=5&cursor={cursor}"),
            None => format!("{BROWSE_PATH}?limit=5"),
        };
        let (status, _, page) = get(app.clone(), &uri, Some(FIRST_DEVICE_TOKEN)).await;
        assert_eq!(status, StatusCode::OK);
        for id in station_ids(&page) {
            assert!(
                !seen.iter().any(|seen_id| seen_id == id),
                "no station may repeat"
            );
            if let Some(last) = cursor.as_ref() {
                assert!(
                    id > last.as_str(),
                    "pages must start strictly after the cursor"
                );
            }
            seen.push(id.to_owned());
        }
        match page["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => {
                terminated = true;
                break;
            }
        }
    }
    assert_eq!(
        seen.len(),
        41,
        "walking the cursor must cover the whole catalog"
    );
    assert!(terminated, "the walk must terminate with a null cursor");
}

#[tokio::test]
async fn browse_rejects_unknown_and_oversized_parameters() {
    let app = app_with_built_in_catalog(Arc::new(FakeResolver::for_device(
        FIRST_DEVICE_TOKEN,
        Uuid::new_v4(),
    )));

    for uri in [
        format!("{BROWSE_PATH}?limit=0"),
        format!("{BROWSE_PATH}?limit=21"),
        format!("{BROWSE_PATH}?limit=-1"),
        format!("{BROWSE_PATH}?limit=abc"),
        format!("{BROWSE_PATH}?cursor="),
        format!("{BROWSE_PATH}?cursor=has%20space"),
        format!("{BROWSE_PATH}?cursor=bad%24symbol"),
        format!("{BROWSE_PATH}?cursor={}", "a".repeat(513)),
        format!("{BROWSE_PATH}?unknown=1"),
    ] {
        let (status, _, body) = get(app.clone(), &uri, Some(FIRST_DEVICE_TOKEN)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "invalid uri: {uri}");
        assert_eq!(body["code"], "malformed_request");
        assert!(body["request_id"].as_str().is_some_and(|id| !id.is_empty()));
    }

    // A maximum-length well-formed cursor stays accepted even when it matches no station.
    let (status, _, page) = get(
        app,
        &format!("{BROWSE_PATH}?cursor={}", "z".repeat(512)),
        Some(FIRST_DEVICE_TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["stations"], serde_json::json!([]));
    assert!(page["next_cursor"].is_null());
}

#[tokio::test]
async fn browse_requires_an_active_device_session() {
    let app = app_with_built_in_catalog(Arc::new(FakeResolver::rejecting()));

    let (status, headers, body) = get(app.clone(), BROWSE_PATH, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(headers[header::WWW_AUTHENTICATE], "Bearer");
    assert_eq!(body["code"], "authentication_required");

    // An expired or revoked session resolves to no session and must fail identically.
    let (status, _, body) = get(app, BROWSE_PATH, Some("expired-or-revoked-token")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "authentication_required");
}

#[tokio::test]
async fn browse_reports_unavailable_authentication_as_retryable() {
    let app = app_with_built_in_catalog(Arc::new(FakeResolver::unavailable()));

    let (status, headers, body) = get(app, BROWSE_PATH, Some(FIRST_DEVICE_TOKEN)).await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["code"], "control_auth_unavailable");
    assert!(headers.contains_key(header::RETRY_AFTER));
}

#[tokio::test]
async fn browse_serves_metadata_even_when_stream_urls_are_unusable() {
    let app = app_with_fixed_streams(
        Arc::new(FakeResolver::for_device(FIRST_DEVICE_TOKEN, Uuid::new_v4())),
        Duration::from_secs(5),
    );

    let (status, _, page) = get(app, BROWSE_PATH, Some(FIRST_DEVICE_TOKEN)).await;

    assert_eq!(status, StatusCode::OK);
    let ids = station_ids(&page);
    assert_eq!(ids, vec!["station-broken-001", "station-legacy-002"]);
    for station in page["stations"].as_array().unwrap() {
        assert!(station.get("stream_url").is_none());
        assert_eq!(station["health"], "healthy");
    }
    assert!(page["next_cursor"].is_null(), "a short page ends the walk");
}

#[tokio::test]
async fn search_returns_one_ranked_stream_free_page() {
    let app = app_with_built_in_catalog(Arc::new(FakeResolver::for_device(
        FIRST_DEVICE_TOKEN,
        Uuid::new_v4(),
    )));

    let (status, headers, page) = get(
        app,
        &format!("{SEARCH_PATH}?q=rock"),
        Some(FIRST_DEVICE_TOKEN),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert!(
        page.get("next_cursor").is_none(),
        "ranked search is not cursorable"
    );
    let stations = page["stations"].as_array().unwrap();
    assert!(!stations.is_empty());
    assert!(stations.len() <= 20);
    for station in stations {
        for forbidden in ["stream_url", "score", "reason", "provider_id"] {
            assert!(
                station.get(forbidden).is_none(),
                "search must never expose {forbidden}"
            );
        }
    }
}

#[tokio::test]
async fn search_rejects_unknown_oversized_and_whitespace_queries() {
    let app = app_with_built_in_catalog(Arc::new(FakeResolver::for_device(
        FIRST_DEVICE_TOKEN,
        Uuid::new_v4(),
    )));

    for uri in [
        SEARCH_PATH.to_owned(),
        format!("{SEARCH_PATH}?q=%20%20%20"),
        format!("{SEARCH_PATH}?q={}", "r".repeat(129)),
        format!("{SEARCH_PATH}?q=rock&limit=0"),
        format!("{SEARCH_PATH}?q=rock&limit=21"),
        format!("{SEARCH_PATH}?q=rock&locale=not%20a%20locale"),
        format!("{SEARCH_PATH}?q=rock&unknown=1"),
    ] {
        let (status, _, body) = get(app.clone(), &uri, Some(FIRST_DEVICE_TOKEN)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "invalid uri: {uri}");
        assert_eq!(body["code"], "malformed_request");
        assert!(body["request_id"].as_str().is_some_and(|id| !id.is_empty()));
    }

    let (status, _, page) = get(
        app,
        &format!("{SEARCH_PATH}?q={}&limit=20", "r".repeat(128)),
        Some(FIRST_DEVICE_TOKEN),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a 128-character query stays accepted"
    );
    assert!(page["stations"].as_array().unwrap().len() <= 20);
}

#[tokio::test]
async fn search_requires_an_active_device_session() {
    let app = app_with_built_in_catalog(Arc::new(FakeResolver::rejecting()));

    let (status, _, body) = get(app.clone(), &format!("{SEARCH_PATH}?q=rock"), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "authentication_required");

    let (status, _, _) = get(app, &format!("{SEARCH_PATH}?q=rock"), Some("unknown-token")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn search_maps_repository_timeouts_to_a_retryable_unavailable_response() {
    let resolver = Arc::new(FakeResolver::for_device(FIRST_DEVICE_TOKEN, Uuid::new_v4()));
    let app = router_with_search_service_and_native_session_resolver(
        SearchService::new(Arc::new(FixedStreamRepository {
            stations: vec![station(
                "station-slow-001",
                "Slow Rock",
                "https://ok.example/s",
                &["rock"],
            )],
            search_delay: Some(Duration::from_millis(200)),
        })),
        Duration::from_millis(10),
        resolver,
    );

    let (status, _, body) = get(
        app,
        &format!("{SEARCH_PATH}?q=rock"),
        Some(FIRST_DEVICE_TOKEN),
    )
    .await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["code"], "service_unavailable");
    assert_eq!(body["details"]["timeout_ms"], 10);
}

#[tokio::test]
async fn browse_rate_limit_is_per_device() {
    let first_device = Uuid::new_v4();
    let second_device = Uuid::new_v4();
    let mut resolver = FakeResolver::for_device(FIRST_DEVICE_TOKEN, first_device);
    resolver.sessions.push((
        hash_of(SECOND_DEVICE_TOKEN),
        ActiveSession {
            session_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            device_id: second_device,
        },
    ));
    let app = app_with_built_in_catalog(Arc::new(resolver));

    for _ in 0..20 {
        let (status, _, _) = get(app.clone(), BROWSE_PATH, Some(FIRST_DEVICE_TOKEN)).await;
        assert_eq!(status, StatusCode::OK);
    }
    let (status, headers, body) = get(app.clone(), BROWSE_PATH, Some(FIRST_DEVICE_TOKEN)).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["code"], "rate_limited");
    assert_eq!(body["details"]["limit_scope"], "device");
    assert!(headers.contains_key(header::RETRY_AFTER));

    // The chatty first device must not consume the second device's quota.
    let (status, _, _) = get(app, BROWSE_PATH, Some(SECOND_DEVICE_TOKEN)).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn search_rate_limit_matches_the_public_search_quota() {
    let app = app_with_built_in_catalog(Arc::new(FakeResolver::for_device(
        FIRST_DEVICE_TOKEN,
        Uuid::new_v4(),
    )));

    for _ in 0..10 {
        let (status, _, _) = get(
            app.clone(),
            &format!("{SEARCH_PATH}?q=rock"),
            Some(FIRST_DEVICE_TOKEN),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }
    let (status, _, body) = get(
        app,
        &format!("{SEARCH_PATH}?q=rock"),
        Some(FIRST_DEVICE_TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["details"]["limit_scope"], "device");
}
