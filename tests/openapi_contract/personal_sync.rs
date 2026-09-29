//! OpenAPI personal data sync contract and schema validation checks.

use serde_json::{Value as JsonValue, json};
use serde_yaml::Value;

use super::common::{OPENAPI, assert_component_invalid, assert_component_valid};

#[test]
fn personal_sync_contract_is_bounded_and_device_session_gated() {
    let document: JsonValue = serde_yaml::from_str(OPENAPI)
        .map(|value: Value| serde_json::to_value(value).expect("OpenAPI must convert to JSON"))
        .expect("OpenAPI YAML must parse");

    let sync = document["paths"]
        .get("/api/v1/sync")
        .and_then(|path| path.get("post"))
        .expect("personal sync must define POST");
    assert_eq!(
        sync.get("x-rockserver-status").and_then(JsonValue::as_str),
        Some("implemented"),
        "personal sync is implemented since RM-012-A"
    );
    let security = sync
        .get("security")
        .and_then(JsonValue::as_array)
        .expect("personal sync must declare security");
    assert_eq!(
        security.len(),
        1,
        "personal sync must have one auth alternative"
    );
    assert!(
        security[0]
            .as_object()
            .expect("security alternative must be a mapping")
            .contains_key("RockserverBearer"),
        "personal sync must accept only the native-session bearer"
    );

    for (name, expected) in [
        ("max_favourites_per_account", 500),
        ("max_history_per_account", 500),
        ("max_batch_items_per_collection", 300),
        ("history_retention_days", 90),
        ("tombstone_retention_days", 90),
        ("deleted_account_purge_days", 30),
        ("max_request_body_bytes", 262_144),
        ("max_metadata_bytes", 4_096),
    ] {
        assert_eq!(
            sync.get("x-personal-data-policy")
                .and_then(|policy| policy.get(name))
                .and_then(JsonValue::as_i64),
            Some(expected),
            "unexpected personal-data policy value for {name}"
        );
    }

    assert_component_valid(
        &document,
        "PersonalSyncRequest",
        &json!({
            "since_revision": 41,
            "favourites": {
                "upserts": [{
                    "record_id": "0f0e0d0c-0b0a-49f8-8a7b-6c5d4e3f2a1b",
                    "station_id": "station-rock-001",
                    "added_at": "2026-09-28T10:00:00Z",
                    "updated_at": "2026-09-28T10:00:00Z"
                }],
                "deletes": [{"record_id": "0f0e0d0c-0b0a-49f8-8a7b-6c5d4e3f2a1c",
                             "updated_at": "2026-09-28T10:05:00Z"}]
            },
            "history": {
                "upserts": [{
                    "record_id": "0f0e0d0c-0b0a-49f8-8a7b-6c5d4e3f2a1d",
                    "station_id": "station-jazz-002",
                    "started_at": "2026-09-28T09:00:00Z",
                    "last_played_at": "2026-09-28T09:30:00Z",
                    "ended_at": "2026-09-28T09:31:00Z",
                    "play_duration_ms": 1_800_000,
                    "metadata": {"lastKnownName": "Jazz FM"},
                    "updated_at": "2026-09-28T10:00:00Z"
                }]
            }
        }),
    );
    assert_component_valid(&document, "PersonalSyncRequest", &json!({}));
    assert_component_invalid(
        &document,
        "PersonalSyncRequest",
        &json!({"unknown": true}),
        "sync requests must stay closed",
    );
    assert_component_invalid(
        &document,
        "FavouriteChanges",
        &json!({"upserts": [{
            "record_id": "0f0e0d0c-0b0a-49f8-8a7b-6c5d4e3f2a1b",
            "station_id": "station-rock-001",
            "added_at": "2026-09-28T10:00:00Z",
            "updated_at": "2026-09-28T10:00:00Z"
        }], "extra": []}),
        "collection changes must reject unknown fields",
    );
    assert_component_invalid(
        &document,
        "HistoryUpsert",
        &json!({
            "record_id": "0f0e0d0c-0b0a-49f8-8a7b-6c5d4e3f2a1d",
            "station_id": "station-jazz-002",
            "started_at": "2026-09-28T09:00:00Z",
            "last_played_at": "2026-09-28T09:30:00Z",
            "play_duration_ms": -1,
            "updated_at": "2026-09-28T10:00:00Z"
        }),
        "playback durations must be non-negative",
    );
    assert_component_invalid(
        &document,
        "RecordDelete",
        &json!({"record_id": "0f0e0d0c-0b0a-49f8-8a7b-6c5d4e3f2a1c"}),
        "deletes must carry the tombstone instant",
    );
    assert_component_valid(
        &document,
        "PersonalSyncResponse",
        &json!({
            "server_revision": 57,
            "server_time": "2026-09-28T10:05:00Z",
            "favourites": {"records": [{
                "record_id": "0f0e0d0c-0b0a-49f8-8a7b-6c5d4e3f2a1b",
                "station_id": "station-rock-001",
                "added_at": "2026-09-28T10:00:00Z",
                "updated_at": "2026-09-28T10:00:00Z"
            }]},
            "history": {"records": [{
                "record_id": "0f0e0d0c-0b0a-49f8-8a7b-6c5d4e3f2a1d",
                "deleted_at": "2026-09-28T10:04:00Z",
                "started_at": "2026-09-28T09:00:00Z",
                "last_played_at": "2026-09-28T09:30:00Z",
                "updated_at": "2026-09-28T10:04:00Z"
            }]}
        }),
    );
    assert_component_invalid(
        &document,
        "PersonalSyncResponse",
        &json!({
            "server_revision": 57,
            "server_time": "2026-09-28T10:05:00Z",
            "favourites": {"records": []},
            "history": {"records": []},
            "extra": true
        }),
        "sync responses must stay closed",
    );
}
