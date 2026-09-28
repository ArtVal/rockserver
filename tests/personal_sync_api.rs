//! HTTP behavior of `POST /api/v1/sync` against a deterministic in-memory personal store.

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower::ServiceExt;
use uuid::Uuid;

use rockserver::auth::{
    ActiveSession, NativeSessionLookupError, NativeSessionResolver, SecretHash,
};
use rockserver::http::router_with_personal_data_and_native_session_resolver;
use rockserver::personal_data::{InMemoryPersonalDataStore, PersonalDataStore};
use rockserver::search::{InMemoryStationRepository, SearchService};

/// One account, two paired devices; both devices sync the same personal profile.
const USER_ID: Uuid = Uuid::from_u128(0xC000_0000_0000_0000_0000_0000_0000_0001);
const DEVICE_ONE: Uuid = Uuid::from_u128(0xC000_0000_0000_0000_0000_0000_0000_0002);
const DEVICE_TWO: Uuid = Uuid::from_u128(0xC000_0000_0000_0000_0000_0000_0000_0003);
const TOKEN_ONE: &str = "personal-sync-token-one";
const TOKEN_TWO: &str = "personal-sync-token-two";

struct FakeResolver {
    sessions: Vec<(SecretHash, ActiveSession)>,
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
        Ok(self
            .sessions
            .iter()
            .find(|(hash, _)| hash == access_hash)
            .map(|(_, session)| *session))
    }
}

/// Builds the app with the shared in-memory store so both devices observe one profile.
fn app(store: std::sync::Arc<InMemoryPersonalDataStore>) -> axum::Router {
    let resolver = FakeResolver {
        sessions: vec![
            (
                hash_of(TOKEN_ONE),
                ActiveSession {
                    session_id: Uuid::new_v4(),
                    user_id: USER_ID,
                    device_id: DEVICE_ONE,
                },
            ),
            (
                hash_of(TOKEN_TWO),
                ActiveSession {
                    session_id: Uuid::new_v4(),
                    user_id: USER_ID,
                    device_id: DEVICE_TWO,
                },
            ),
        ],
    };
    router_with_personal_data_and_native_session_resolver(
        SearchService::new(std::sync::Arc::new(
            InMemoryStationRepository::with_builtin_catalog().unwrap(),
        )),
        std::time::Duration::from_secs(5),
        std::sync::Arc::new(resolver),
        store,
    )
}

fn now_rfc3339(offset_seconds: i64) -> String {
    let instant = time::OffsetDateTime::now_utc() + time::Duration::seconds(offset_seconds);
    instant
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap()
}

async fn sync(app: &axum::Router, token: &str, body: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/sync")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let payload: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
            .unwrap_or(Value::Null);
    (status, payload)
}

fn favourite(record_id: Uuid, station: &str, updated: String) -> Value {
    json!({
        "record_id": record_id.to_string(),
        "station_id": station,
        "added_at": updated,
        "updated_at": updated,
    })
}

#[tokio::test]
async fn sync_requires_a_native_device_session() {
    let app = app(std::sync::Arc::new(InMemoryPersonalDataStore::default()));
    let (missing, _) = sync(&app, "", json!({})).await;
    assert_eq!(missing, StatusCode::UNAUTHORIZED);
    let (invalid, payload) = sync(&app, "not-a-session", json!({})).await;
    assert_eq!(invalid, StatusCode::UNAUTHORIZED);
    assert_eq!(payload["code"], "authentication_required");
}

#[tokio::test]
async fn first_sync_pushes_and_returns_the_full_snapshot() {
    let app = app(std::sync::Arc::new(InMemoryPersonalDataStore::default()));
    let (status, payload) = sync(
        &app,
        TOKEN_ONE,
        json!({
            "favourites": {
                "upserts": [favourite(Uuid::new_v4(), "station-rock-001", now_rfc3339(0))],
            },
            "history": {
                "upserts": [{
                    "record_id": Uuid::new_v4().to_string(),
                    "station_id": "station-jazz-002",
                    "started_at": now_rfc3339(-600),
                    "last_played_at": now_rfc3339(-300),
                    "play_duration_ms": 120_000,
                    "metadata": {"lastKnownName": "Jazz FM"},
                    "updated_at": now_rfc3339(0),
                }],
            },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        payload["favourites"]["records"].as_array().unwrap().len(),
        1
    );
    assert_eq!(payload["history"]["records"].as_array().unwrap().len(), 1);
    assert!(payload["server_revision"].as_u64().unwrap() > 0);
    assert!(!payload["server_time"].as_str().unwrap().is_empty());
    assert_eq!(
        payload["history"]["records"][0]["metadata"]["lastKnownName"],
        "Jazz FM"
    );
}

#[tokio::test]
async fn the_second_device_pulls_the_first_device_push() {
    let store = std::sync::Arc::new(InMemoryPersonalDataStore::default());
    let app = app(store);
    let record_id = Uuid::new_v4();
    let (status, pushed) = sync(
        &app,
        TOKEN_ONE,
        json!({"favourites": {"upserts": [favourite(record_id, "station-rock-001", now_rfc3339(0))]}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let cursor = pushed["server_revision"].as_u64().unwrap();

    // Cursors are per device: the second device has seen nothing yet, so it pulls the
    // full snapshot and must observe the first device's push.
    let (_, pulled) = sync(&app, TOKEN_TWO, json!({})).await;
    assert_eq!(pulled["favourites"]["records"].as_array().unwrap().len(), 1);
    assert!(
        pulled["server_revision"].as_u64().unwrap() >= cursor,
        "the snapshot must bring the second device to at least the pushed revision"
    );

    // A later pull from the second device's own cursor returns nothing new.
    let own_cursor = pulled["server_revision"].as_u64().unwrap();
    let (_, quiet) = sync(&app, TOKEN_TWO, json!({"since_revision": own_cursor})).await;
    assert_eq!(quiet["favourites"]["records"].as_array().unwrap().len(), 0);
    assert_eq!(
        pulled["favourites"]["records"][0]["record_id"],
        record_id.to_string()
    );
}

#[tokio::test]
async fn stale_pushes_lose_and_are_echoed_back() {
    let store = std::sync::Arc::new(InMemoryPersonalDataStore::default());
    let app = app(store);
    let record_id = Uuid::new_v4();
    let (first_status, first) = sync(
        &app,
        TOKEN_ONE,
        json!({"favourites": {"upserts": [favourite(record_id, "station-new", now_rfc3339(0))]}}),
    )
    .await;
    assert_eq!(first_status, StatusCode::OK);
    let cursor = first["server_revision"].as_u64().unwrap();

    let (_, replayed) = sync(
        &app,
        TOKEN_TWO,
        json!({
            "since_revision": cursor,
            "favourites": {"upserts": [favourite(record_id, "station-old", now_rfc3339(-3600))]},
        }),
    )
    .await;
    let records = replayed["favourites"]["records"].as_array().unwrap();
    assert_eq!(records.len(), 1, "the pushed loser must be echoed");
    assert_eq!(records[0]["station_id"], "station-new");
    assert_eq!(
        replayed["server_revision"].as_u64().unwrap(),
        cursor,
        "an echoed loser must not advance the cursor"
    );
}

#[tokio::test]
async fn deletions_propagate_as_tombstones_and_reject_stale_resurrections() {
    let store = std::sync::Arc::new(InMemoryPersonalDataStore::default());
    let app = app(store);
    let record_id = Uuid::new_v4();
    sync(
        &app,
        TOKEN_ONE,
        json!({"favourites": {"upserts": [favourite(record_id, "station-rock-001", now_rfc3339(0))]}}),
    )
    .await;

    let (_, deleted) = sync(
        &app,
        TOKEN_ONE,
        json!({
            "favourites": {"deletes": [{"record_id": record_id.to_string(), "updated_at": now_rfc3339(60)}]},
        }),
    )
    .await;
    let tombstone = &deleted["favourites"]["records"][0];
    assert_eq!(tombstone["record_id"], record_id.to_string());
    assert!(
        tombstone["deleted_at"].is_string(),
        "the deletion must propagate as a tombstone"
    );

    // The second device learns the deletion, then replays its stale local copy.
    let cursor = deleted["server_revision"].as_u64().unwrap();
    let (_, stale) = sync(
        &app,
        TOKEN_TWO,
        json!({
            "since_revision": cursor,
            "favourites": {"upserts": [favourite(record_id, "station-rock-001", now_rfc3339(-3600))]},
        }),
    )
    .await;
    assert!(
        stale["favourites"]["records"][0]["deleted_at"].is_string(),
        "a stale upsert must not resurrect a deleted record"
    );
}

#[tokio::test]
async fn malformed_batches_fail_with_the_public_error_contract() {
    let app = app(std::sync::Arc::new(InMemoryPersonalDataStore::default()));
    let (bad_record, payload) = sync(
        &app,
        TOKEN_ONE,
        json!({
            "favourites": {"upserts": [{
                "record_id": "not-a-uuid",
                "station_id": "station-rock-001",
                "added_at": now_rfc3339(0),
                "updated_at": now_rfc3339(0),
            }]},
        }),
    )
    .await;
    assert_eq!(bad_record, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(payload["code"], "validation_failed");

    let (unknown_field, _) = sync(&app, TOKEN_ONE, json!({"unknown": true})).await;
    assert_eq!(unknown_field, StatusCode::UNPROCESSABLE_ENTITY);

    let (bad_history_order, payload) = sync(
        &app,
        TOKEN_ONE,
        json!({
            "history": {"upserts": [{
                "record_id": Uuid::new_v4().to_string(),
                "station_id": "station-jazz-002",
                "started_at": now_rfc3339(0),
                "last_played_at": now_rfc3339(-600),
                "updated_at": now_rfc3339(0),
            }]},
        }),
    )
    .await;
    assert_eq!(bad_history_order, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(payload["details"]["field"], "history.last_played_at");
}

#[tokio::test]
async fn successful_syncs_are_not_cacheable_and_carry_a_request_id() {
    let app = app(std::sync::Arc::new(InMemoryPersonalDataStore::default()));
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/sync")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN_ONE}"))
                .header("x-request-id", "personal-sync-req-1")
                .body(Body::from(json!({}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(response.headers()["x-request-id"], "personal-sync-req-1");
}

#[tokio::test]
async fn store_backed_router_reports_sync_unavailability_without_a_store() {
    // The plain offline router has no personal store; the endpoint must fail closed.
    let app = rockserver::http::router();
    let response = app
        .oneshot(
            Request::post("/api/v1/sync")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN_ONE}"))
                .body(Body::from(json!({}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn the_in_memory_store_matches_the_shared_delta_selection() {
    // Compile-time guard: the HTTP fixture store implements the same trait the production
    // router consumes, so HTTP tests cannot drift from the persistence contract.
    let store: std::sync::Arc<dyn PersonalDataStore> =
        std::sync::Arc::new(InMemoryPersonalDataStore::default());
    let outcome = store
        .synchronize(USER_ID, Default::default())
        .await
        .unwrap();
    assert_eq!(outcome.server_revision, 0);
}
