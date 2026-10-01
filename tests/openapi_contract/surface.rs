//! General OpenAPI contract surface, path, schema, and route registration checks.

use axum::{body::Body, http::Request, http::StatusCode};
use serde_yaml::Value;
use tower::ServiceExt;

use super::common::{OPENAPI, value_at};

#[test]
fn openapi_contract_is_parseable_and_has_required_surface() {
    let document: Value = serde_yaml::from_str(OPENAPI).expect("OpenAPI YAML must parse");

    let version = value_at(&document, "openapi")
        .and_then(Value::as_str)
        .expect("OpenAPI version must be a string");
    assert!(version.starts_with("3."), "expected OpenAPI 3.x");

    for path in [
        "/health/live",
        "/health/ready",
        "/api/v1/admin/auth/login",
        "/api/v1/admin/auth/refresh",
        "/api/v1/admin/auth/logout",
        "/api/v1/admin/session",
        "/api/v1/admin/devices",
        "/api/v1/admin/audit",
        "/api/v1/admin/stations",
        "/api/v1/admin/icons/import",
        "/api/v1/admin/icons/import/{job_id}",
        "/api/v1/admin/stations/{station_id}/icon",
        "/api/v1/stations/{station_id}/icon",
        "/api/v1/stations/{station_id}/stream",
        "/api/v1/stations/{station_id}/now-playing",
        "/api/v1/stations/{station_id}/events",
        "/api/v1/auth/device-session",
        "/api/v1/auth/browser-logout",
        "/api/v1/browser/account",
        "/api/v1/account/profile",
        "/api/v1/account",
        "/api/v1/devices",
        "/api/v1/devices/{device_id}",
        "/api/v1/sync",
        "/api/v1/browser/devices/{device_id}",
        "/api/v1/search",
        "/api/v1/voice/command",
        "/api/v1/voice/stream",
    ] {
        assert!(
            value_at(&document, "paths")
                .and_then(|paths| paths.get(path))
                .is_some(),
            "missing required path {path}"
        );
    }

    assert!(
        value_at(&document, "paths")
            .and_then(|paths| paths.get("/api/v1/search"))
            .and_then(|search| search.get("post"))
            .is_some(),
        "search path must define POST"
    );
    let icon_import = value_at(&document, "paths")
        .and_then(|paths| paths.get("/api/v1/admin/icons/import"))
        .expect("station icon import path must exist");
    assert!(icon_import.get("get").is_some());
    assert!(icon_import.get("post").is_some());
    assert!(
        icon_import
            .get("post")
            .and_then(|operation| operation.get("security"))
            .is_some(),
        "icon import start must require the administrator bearer"
    );
    let icon_delivery = value_at(&document, "paths")
        .and_then(|paths| paths.get("/api/v1/stations/{station_id}/icon"))
        .and_then(|path| path.get("get"))
        .expect("station icon delivery must define GET");
    assert!(
        icon_delivery
            .get("responses")
            .and_then(|responses| responses.get("304"))
            .is_some(),
        "station icon delivery must document ETag revalidation"
    );
    let manual_icon = value_at(&document, "paths")
        .and_then(|paths| paths.get("/api/v1/admin/stations/{station_id}/icon"))
        .expect("manual station icon path must exist");
    assert!(manual_icon.get("put").is_some());
    assert!(manual_icon.get("delete").is_some());
    let completion_operation = value_at(&document, "paths")
        .and_then(|paths| paths.get("/api/v1/pairing-requests/{request_id}/complete"))
        .and_then(|path| path.get("post"))
        .expect("pairing completion must define POST");
    let completion = completion_operation
        .get("requestBody")
        .and_then(|body| body.get("content"))
        .and_then(|content| content.get("application/json"))
        .and_then(|json| json.get("schema"))
        .expect("pairing completion must define a JSON schema");
    let required = completion
        .get("required")
        .and_then(Value::as_sequence)
        .expect("pairing completion must require its desktop proof");
    assert_eq!(required, &vec![Value::String("desktop_token".to_owned())]);
    for status in ["200", "202", "401", "409", "410", "503"] {
        assert!(
            completion_operation
                .get("responses")
                .and_then(|responses| responses.get(status))
                .is_some(),
            "pairing completion must document {status}"
        );
    }
    assert!(
        completion
            .get("properties")
            .and_then(|properties| properties.get("user_id"))
            .is_none(),
        "pairing completion must derive the owner server-side"
    );
    let voice = value_at(&document, "paths")
        .and_then(|paths| paths.get("/api/v1/voice/command"))
        .and_then(|command| command.get("post"))
        .expect("canonical voice-command path must define POST");
    for status in ["200", "400", "413", "422", "500", "504"] {
        assert!(
            voice
                .get("responses")
                .and_then(|responses| responses.get(status))
                .is_some(),
            "voice command must document {status}"
        );
    }
    for path in [
        "/api/v1/search",
        "/api/v1/voice/command",
        "/api/v1/voice/stream",
    ] {
        assert!(
            value_at(&document, "paths")
                .and_then(|paths| paths.get(path))
                .and_then(Value::as_mapping)
                .and_then(|operations| operations.values().next())
                .and_then(|operation| operation.get("security"))
                .is_none(),
            "{path} must remain anonymous"
        );
    }
    assert!(
        value_at(&document, "components/securitySchemes/RockCastBearer").is_some(),
        "the RockCast Bearer scheme must be declared"
    );
    for path in [
        "/api/v1/admin/auth/refresh",
        "/api/v1/admin/auth/logout",
        "/api/v1/admin/session",
        "/api/v1/admin/devices",
        "/api/v1/admin/audit",
        "/api/v1/admin/stations",
    ] {
        assert!(
            value_at(&document, "paths")
                .and_then(|paths| paths.get(path))
                .and_then(Value::as_mapping)
                .and_then(|operations| operations.values().next())
                .and_then(|operation| operation.get("security"))
                .is_some(),
            "{path} must retain the separate admin Bearer boundary"
        );
    }
    let voice_stream = value_at(&document, "paths")
        .and_then(|paths| paths.get("/api/v1/voice/stream"))
        .and_then(|stream| stream.get("get"))
        .expect("canonical voice stream path must define GET upgrade");
    assert!(
        voice_stream.get("x-websocket-client-messages").is_some()
            && voice_stream.get("x-websocket-server-messages").is_some(),
        "voice stream must document both WebSocket message directions"
    );
    assert!(
        value_at(&document, "paths")
            .and_then(|paths| paths.get("/health/ready"))
            .and_then(|ready| ready.get("get"))
            .and_then(|get| get.get("responses"))
            .and_then(|responses| responses.get("503"))
            .is_some(),
        "readiness must document PostgreSQL unavailability"
    );

    let schemas = value_at(&document, "components/schemas")
        .and_then(Value::as_mapping)
        .expect("components.schemas must be a mapping");
    for schema in [
        "SearchRequest",
        "SearchResponse",
        "VoiceCommandRequest",
        "VoiceCommandResponse",
        "VoiceStreamStart",
        "VoiceStreamCommit",
        "VoiceStreamCancel",
        "VoiceStreamReady",
        "VoiceStreamTranscript",
        "VoiceStreamResult",
        "VoiceDeviceCommandResult",
        "VoiceStreamErrorCode",
        "VoiceStreamError",
        "NormalizedQuery",
        "StationResult",
        "ErrorResponse",
        "DeviceSessionRequest",
        "DeviceSession",
        "AccountProfile",
        "DeviceList",
        "BrowserAccount",
        "BrowserDevice",
        "RenameDeviceRequest",
        "CreatedPairingRequest",
        "PairingPreview",
        "PersonalSyncRequest",
        "FavouriteChanges",
        "HistoryChanges",
        "FavouriteUpsert",
        "HistoryUpsert",
        "RecordDelete",
        "PersonalSyncResponse",
        "PersonalFavouriteRecord",
        "PersonalHistoryRecord",
    ] {
        assert!(
            schemas.contains_key(Value::String(schema.to_owned())),
            "missing required schema {schema}"
        );
    }
    assert!(
        value_at(&document, "paths")
            .and_then(|paths| paths.get("/api/v1/auth/browser-session"))
            .and_then(|path| path.get("post"))
            .is_some(),
        "browser session CSRF refresh must be documented"
    );
    let browser_device = schemas
        .get(Value::String("BrowserDevice".to_owned()))
        .expect("browser device schema must exist");
    let browser_properties = browser_device
        .get("properties")
        .and_then(Value::as_mapping)
        .expect("browser device fields must be declared");
    for secret in ["credential_id", "access_token", "device_secret", "user_id"] {
        assert!(
            !browser_properties.contains_key(Value::String(secret.to_owned())),
            "browser device must not expose {secret}"
        );
    }
    let preview = schemas
        .get(Value::String("PairingPreview".to_owned()))
        .expect("pairing preview schema must exist");
    let preview_properties = preview
        .get("properties")
        .and_then(Value::as_mapping)
        .expect("pairing preview properties must be declared");
    for field in [
        "device_display_name",
        "device_type",
        "short_code",
        "verification_phrase",
        "expires_at",
        "status",
    ] {
        assert!(
            preview_properties.contains_key(Value::String(field.to_owned())),
            "pairing preview must expose {field}"
        );
    }
    for secret in [
        "desktop_token",
        "approval_secret",
        "credential_id",
        "device_secret",
    ] {
        assert!(
            !preview_properties.contains_key(Value::String(secret.to_owned())),
            "pairing preview must not expose {secret}"
        );
    }
}

#[tokio::test]
async fn search_endpoint_is_registered() {
    let response = rockserver::http::router()
        .oneshot(
            Request::post("/api/v1/search")
                .header("content-type", "application/json")
                .header(
                    "authorization",
                    format!("Bearer {}", rockserver::http::TEST_API_BEARER_TOKEN),
                )
                .body(Body::from(r#"{"query":"jazz"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_ne!(response.status(), StatusCode::NOT_FOUND);
}
