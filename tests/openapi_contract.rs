//! Structural checks for the contract-first OpenAPI document and golden protocol fixtures.

use serde_json::{Value as JsonValue, json};
use serde_yaml::Value;
use std::{collections::BTreeSet, fs, path::Path};

const OPENAPI: &str = include_str!("../api/openapi.yaml");
const FIXTURE_DIRECTORY: &str = "tests/fixtures/device-control/v1";

/// Describes one raw golden fixture and the OpenAPI component that validates it.
struct FixtureSpec {
    file: &'static str,
    schema: &'static str,
    valid: bool,
}

const DEVICE_CONTROL_FIXTURES: &[FixtureSpec] = &[
    FixtureSpec {
        file: "hello-client.json",
        schema: "ProtocolHelloMessage",
        valid: true,
    },
    FixtureSpec {
        file: "welcome-server.json",
        schema: "ProtocolWelcomeMessage",
        valid: true,
    },
    FixtureSpec {
        file: "rockcast-register-client.json",
        schema: "DeviceRegisterMessage",
        valid: true,
    },
    FixtureSpec {
        file: "rockcast-registered-server.json",
        schema: "DeviceRegisteredMessage",
        valid: true,
    },
    FixtureSpec {
        file: "esp32-register-client.json",
        schema: "DeviceRegisterMessage",
        valid: true,
    },
    FixtureSpec {
        file: "esp32-manifest-client.json",
        schema: "DeviceManifestMessage",
        valid: true,
    },
    FixtureSpec {
        file: "esp32-state-full-client.json",
        schema: "DeviceStateFullMessage",
        valid: true,
    },
    FixtureSpec {
        file: "esp32-state-delta-client.json",
        schema: "DeviceStateDeltaMessage",
        valid: true,
    },
    FixtureSpec {
        file: "esp32-temperature-state-client.json",
        schema: "EntityStateMessage",
        valid: true,
    },
    FixtureSpec {
        file: "esp32-humidity-state-client.json",
        schema: "EntityStateMessage",
        valid: true,
    },
    FixtureSpec {
        file: "directory-snapshot-server.json",
        schema: "DirectorySnapshotMessage",
        valid: true,
    },
    FixtureSpec {
        file: "directory-upsert-server.json",
        schema: "DirectoryUpsertMessage",
        valid: true,
    },
    FixtureSpec {
        file: "device-catalog-response.json",
        schema: "DeviceCatalogPage",
        valid: true,
    },
    FixtureSpec {
        file: "ha-normalized-entity-directory-entry.json",
        schema: "EntityDirectoryEntry",
        valid: true,
    },
    FixtureSpec {
        file: "ha-normalized-entity-state.json",
        schema: "EntityStateSnapshot",
        valid: true,
    },
    FixtureSpec {
        file: "display-sensor-grid-command-server.json",
        schema: "DeviceCommandMessage",
        valid: true,
    },
    FixtureSpec {
        file: "station-command-server.json",
        schema: "DeviceCommandMessage",
        valid: true,
    },
    FixtureSpec {
        file: "station-command-received-server.json",
        schema: "CommandReceivedMessage",
        valid: true,
    },
    FixtureSpec {
        file: "station-command-accepted-rockcast.json",
        schema: "CommandAcceptedMessage",
        valid: true,
    },
    FixtureSpec {
        file: "station-command-succeeded-rockcast.json",
        schema: "CommandResultMessage",
        valid: true,
    },
    FixtureSpec {
        file: "station-command-failed-rockcast.json",
        schema: "CommandResultMessage",
        valid: true,
    },
    FixtureSpec {
        file: "unknown-capability.json",
        schema: "DeviceCapability",
        valid: true,
    },
    FixtureSpec {
        file: "unknown-command-client.json",
        schema: "DeviceCommandMessage",
        valid: true,
    },
    FixtureSpec {
        file: "unknown-command-error-server.json",
        schema: "ProtocolErrorMessage",
        valid: true,
    },
    FixtureSpec {
        file: "stale-manifest-client.json",
        schema: "DeviceManifestMessage",
        valid: true,
    },
    FixtureSpec {
        file: "stale-manifest-error-server.json",
        schema: "ProtocolErrorMessage",
        valid: true,
    },
    FixtureSpec {
        file: "stale-device-state-client.json",
        schema: "DeviceStateFullMessage",
        valid: true,
    },
    FixtureSpec {
        file: "stale-device-state-error-server.json",
        schema: "ProtocolErrorMessage",
        valid: true,
    },
    FixtureSpec {
        file: "stale-entity-state-client.json",
        schema: "EntityStateMessage",
        valid: true,
    },
    FixtureSpec {
        file: "stale-entity-state-error-server.json",
        schema: "ProtocolErrorMessage",
        valid: true,
    },
    FixtureSpec {
        file: "invalid-sensor-unit-value-client.json",
        schema: "EntityStateMessage",
        valid: true,
    },
    FixtureSpec {
        file: "invalid-sensor-unit-value-error-server.json",
        schema: "ProtocolErrorMessage",
        valid: true,
    },
    FixtureSpec {
        file: "duplicate-command-received-server.json",
        schema: "CommandReceivedMessage",
        valid: true,
    },
    FixtureSpec {
        file: "duplicate-command-result-server.json",
        schema: "CommandResultMessage",
        valid: true,
    },
    FixtureSpec {
        file: "offline-target-command-client.json",
        schema: "DeviceCommandMessage",
        valid: true,
    },
    FixtureSpec {
        file: "offline-target-error-server.json",
        schema: "ProtocolErrorMessage",
        valid: true,
    },
    FixtureSpec {
        file: "missing-surface-command-server.json",
        schema: "DeviceCommandMessage",
        valid: true,
    },
    FixtureSpec {
        file: "missing-surface-error-server.json",
        schema: "ProtocolErrorMessage",
        valid: true,
    },
    FixtureSpec {
        file: "invalid-frame-missing-message-id.json",
        schema: "ControlMessageEnvelope",
        valid: false,
    },
];

/// Loads a raw fixture from the canonical v1 directory.
fn load_fixture(file: &str) -> JsonValue {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(FIXTURE_DIRECTORY)
        .join(file);
    serde_json::from_slice(
        &fs::read(&path)
            .unwrap_or_else(|error| panic!("fixture {} must exist: {error}", path.display())),
    )
    .unwrap_or_else(|error| panic!("fixture {} must be JSON: {error}", path.display()))
}

/// Builds a JSON Schema 2020-12 root that resolves local OpenAPI component references.
fn fixture_schema(document: &JsonValue, component: &str) -> JsonValue {
    assert!(
        document["components"]["schemas"].get(component).is_some(),
        "fixture schema {component} must be an OpenAPI component"
    );
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$ref": format!("#/components/schemas/{component}"),
        "components": document["components"].clone(),
    })
}

/// Validates an inline instance against an OpenAPI component and collects human-readable errors.
fn component_errors(document: &JsonValue, component: &str, instance: &JsonValue) -> Vec<String> {
    let schema = fixture_schema(document, component);
    let validator = jsonschema::validator_for(&schema)
        .unwrap_or_else(|error| panic!("{component} schema must compile: {error}"));
    validator
        .iter_errors(instance)
        .map(|error| error.to_string())
        .collect()
}

/// Asserts that an inline instance satisfies an OpenAPI component schema.
fn assert_component_valid(document: &JsonValue, component: &str, instance: &JsonValue) {
    let errors = component_errors(document, component, instance);
    assert!(
        errors.is_empty(),
        "{component} must accept the instance; errors: {errors:?}"
    );
}

/// Asserts that an inline instance violates an OpenAPI component schema.
fn assert_component_invalid(
    document: &JsonValue,
    component: &str,
    instance: &JsonValue,
    context: &str,
) {
    let errors = component_errors(document, component, instance);
    assert!(
        !errors.is_empty(),
        "{component} must reject the instance ({context}): {instance}"
    );
}

/// Reads a nested JSON value and fails with the fixture path when it is absent.
fn fixture_at<'a>(fixture: &'a JsonValue, pointer: &str) -> &'a JsonValue {
    fixture
        .pointer(pointer)
        .unwrap_or_else(|| panic!("fixture must contain {pointer}"))
}

/// Reads a slash-delimited mapping path from a YAML value.
fn value_at<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('/').try_fold(root, |value, key| value.get(key))
}

/// Collects local references and operation identifiers from an arbitrary YAML subtree.
fn collect_contract_links(
    value: &Value,
    references: &mut Vec<String>,
    operation_ids: &mut Vec<String>,
) {
    match value {
        Value::Mapping(mapping) => {
            for (key, child) in mapping {
                if key.as_str() == Some("$ref") {
                    references.push(
                        child
                            .as_str()
                            .expect("OpenAPI $ref values must be strings")
                            .to_owned(),
                    );
                } else if key.as_str() == Some("operationId") {
                    operation_ids.push(
                        child
                            .as_str()
                            .expect("OpenAPI operationId values must be strings")
                            .to_owned(),
                    );
                }
                collect_contract_links(child, references, operation_ids);
            }
        }
        Value::Sequence(sequence) => {
            for child in sequence {
                collect_contract_links(child, references, operation_ids);
            }
        }
        _ => {}
    }
}

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

#[test]
fn device_control_v1_is_bounded_and_fully_linked() {
    let document: Value = serde_yaml::from_str(OPENAPI).expect("OpenAPI YAML must parse");
    let paths = document
        .get("paths")
        .expect("OpenAPI paths must be declared");

    for (path, status) in [
        ("/api/v1/device-control/directory", "implemented"),
        ("/api/v1/devices/connect", "implemented"),
    ] {
        let operation = paths
            .get(path)
            .and_then(|path_item| path_item.get("get"))
            .unwrap_or_else(|| panic!("{path} must define GET"));
        assert_eq!(
            operation.get("x-rockserver-status").and_then(Value::as_str),
            Some(status),
            "{path} must have the expected implementation status"
        );
        let security = operation
            .get("security")
            .and_then(Value::as_sequence)
            .expect("device-control operations must declare security");
        assert_eq!(security.len(), 1, "{path} must have one auth alternative");
        let schemes = security[0]
            .as_mapping()
            .expect("security alternative must be a mapping");
        assert_eq!(schemes.len(), 1, "{path} must accept only one scheme");
        assert!(
            schemes.contains_key(Value::String("RockserverBearer".to_owned())),
            "{path} must accept only the native-session bearer"
        );
    }

    let connect = paths
        .get("/api/v1/devices/connect")
        .and_then(|path| path.get("get"))
        .expect("device-control connect operation must exist");
    for direction in ["x-websocket-client-messages", "x-websocket-server-messages"] {
        let messages = connect
            .get(direction)
            .and_then(|message_set| message_set.get("oneOf"))
            .and_then(Value::as_sequence)
            .unwrap_or_else(|| panic!("{direction} must be a oneOf message set"));
        assert!(!messages.is_empty(), "{direction} must not be empty");
    }
    let policy = connect
        .get("x-device-control-policy")
        .expect("device-control limits must be machine-readable");
    for (name, expected) in [
        ("protocol_major", 1),
        ("max_json_frame_bytes", 65_536),
        ("max_payload_bytes", 61_440),
        ("heartbeat_interval_seconds", 20),
        ("offline_ttl_seconds", 60),
        ("registration_deadline_seconds", 10),
        ("command_idempotency_window_seconds", 86_400),
    ] {
        assert_eq!(
            policy.get(name).and_then(Value::as_i64),
            Some(expected),
            "unexpected device-control policy value for {name}"
        );
    }

    let register_properties = value_at(
        &document,
        "components/schemas/DeviceRegisterPayload/properties",
    )
    .and_then(Value::as_mapping)
    .expect("device registration fields must be declared");
    for forbidden in ["user_id", "device_id", "device_secret", "access_token"] {
        assert!(
            !register_properties.contains_key(Value::String(forbidden.to_owned())),
            "registration must not accept {forbidden}"
        );
    }
    let directory_properties = value_at(
        &document,
        "components/schemas/DeviceControlDirectoryEntry/properties",
    )
    .and_then(Value::as_mapping)
    .expect("directory fields must be declared");
    for forbidden in [
        "user_id",
        "device_secret",
        "access_token",
        "credential_id",
        "provider_native_id",
    ] {
        assert!(
            !directory_properties.contains_key(Value::String(forbidden.to_owned())),
            "directory must not expose {forbidden}"
        );
    }
    let runtime_state = directory_properties
        .get(Value::String("runtime_state".to_owned()))
        .expect("directory entries must declare the owner-scoped runtime_state projection");
    assert_eq!(
        runtime_state.get("$ref").and_then(Value::as_str),
        Some("#/components/schemas/DeviceStateSnapshot"),
        "runtime_state must reuse the revisioned DeviceStateSnapshot without duplication"
    );
    let directory_entry_required = value_at(
        &document,
        "components/schemas/DeviceControlDirectoryEntry/required",
    )
    .and_then(Value::as_sequence)
    .expect("directory entry requirements must be declared");
    assert!(
        !directory_entry_required.contains(&Value::String("runtime_state".to_owned())),
        "runtime_state must stay optional for targets that never published state"
    );

    let mut references = Vec::new();
    let mut operation_ids = Vec::new();
    collect_contract_links(&document, &mut references, &mut operation_ids);
    for reference in references {
        let local_path = reference
            .strip_prefix("#/")
            .unwrap_or_else(|| panic!("only local OpenAPI references are allowed: {reference}"));
        assert!(
            value_at(&document, local_path).is_some(),
            "dangling OpenAPI reference {reference}"
        );
    }
    for schema_name in ["DeviceCapability", "DeviceCommandBody"] {
        let mapping = value_at(
            &document,
            &format!("components/schemas/{schema_name}/discriminator/mapping"),
        )
        .and_then(Value::as_mapping)
        .unwrap_or_else(|| panic!("{schema_name} must declare a discriminator mapping"));
        for reference in mapping.values() {
            let reference = reference
                .as_str()
                .expect("discriminator targets must be strings");
            let local_path = reference
                .strip_prefix("#/")
                .expect("discriminator targets must be local references");
            assert!(
                value_at(&document, local_path).is_some(),
                "dangling discriminator target {reference}"
            );
        }
    }
    let unique_operation_ids: BTreeSet<_> = operation_ids.iter().collect();
    assert_eq!(
        unique_operation_ids.len(),
        operation_ids.len(),
        "operationId values must be unique"
    );
}

#[test]
fn device_control_v1_golden_fixtures_match_schemas_and_flows() {
    let document: JsonValue = serde_yaml::from_str(OPENAPI)
        .map(|value: Value| serde_json::to_value(value).expect("OpenAPI must convert to JSON"))
        .expect("OpenAPI YAML must parse");
    let fixture_directory = Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_DIRECTORY);
    let registered: BTreeSet<_> = DEVICE_CONTROL_FIXTURES
        .iter()
        .map(|fixture| fixture.file.to_owned())
        .collect();
    let discovered: BTreeSet<_> = fs::read_dir(&fixture_directory)
        .expect("fixture directory must exist")
        .map(|entry| entry.expect("fixture directory entry must be readable"))
        .filter_map(|entry| {
            (entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                == Some("json"))
            .then(|| {
                entry
                    .file_name()
                    .into_string()
                    .expect("fixture names must be UTF-8")
            })
        })
        .collect();
    assert_eq!(
        discovered, registered,
        "every JSON fixture must be registered exactly once"
    );

    for fixture in DEVICE_CONTROL_FIXTURES {
        let schema = fixture_schema(&document, fixture.schema);
        let validator = jsonschema::validator_for(&schema)
            .unwrap_or_else(|error| panic!("{} schema must compile: {error}", fixture.schema));
        let instance = load_fixture(fixture.file);
        let errors: Vec<_> = validator.iter_errors(&instance).collect();
        assert_eq!(
            errors.is_empty(),
            fixture.valid,
            "{} must be schema-{}; errors: {:?}",
            fixture.file,
            if fixture.valid { "valid" } else { "invalid" },
            errors
        );
    }

    let message_ids: BTreeSet<_> = DEVICE_CONTROL_FIXTURES
        .iter()
        .filter(|fixture| fixture.schema.ends_with("Message"))
        .map(|fixture| {
            fixture_at(&load_fixture(fixture.file), "/message_id")
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(
        message_ids.len(),
        34,
        "message IDs must be distinct across the v1 flows"
    );

    let full = load_fixture("esp32-state-full-client.json");
    let delta = load_fixture("esp32-state-delta-client.json");
    assert_eq!(
        fixture_at(&delta, "/payload/delta/base_revision"),
        fixture_at(&full, "/payload/snapshot/state_revision")
    );
    assert_eq!(
        fixture_at(&delta, "/payload/delta/state_revision").as_i64(),
        fixture_at(&delta, "/payload/delta/base_revision")
            .as_i64()
            .map(|revision| revision + 1)
    );
    assert!(
        fixture_at(
            &load_fixture("stale-manifest-client.json"),
            "/payload/manifest/manifest_revision"
        )
        .as_i64()
            < fixture_at(
                &load_fixture("esp32-manifest-client.json"),
                "/payload/manifest/manifest_revision"
            )
            .as_i64(),
        "stale fixture must carry a lower manifest revision"
    );
    assert!(
        fixture_at(
            &load_fixture("stale-device-state-client.json"),
            "/payload/snapshot/state_revision"
        )
        .as_i64()
            < fixture_at(&full, "/payload/snapshot/state_revision").as_i64(),
        "stale device-state fixture must carry a lower revision"
    );
    assert!(
        fixture_at(
            &load_fixture("stale-entity-state-client.json"),
            "/payload/state/entity_revision"
        )
        .as_i64()
            < fixture_at(
                &load_fixture("esp32-temperature-state-client.json"),
                "/payload/state/entity_revision"
            )
            .as_i64(),
        "stale entity-state fixture must carry a lower revision"
    );

    let command = load_fixture("station-command-server.json");
    let command_id = fixture_at(&command, "/payload/command_id");
    assert!(
        fixture_at(&command, "/payload/target/device_id").is_string(),
        "commands need an explicit target"
    );
    for file in [
        "station-command-received-server.json",
        "station-command-accepted-rockcast.json",
        "station-command-succeeded-rockcast.json",
        "duplicate-command-received-server.json",
        "duplicate-command-result-server.json",
    ] {
        assert_eq!(
            fixture_at(&load_fixture(file), "/payload/command_id"),
            command_id,
            "{file} must correlate to the station command"
        );
    }
    assert_eq!(
        fixture_at(
            &load_fixture("station-command-received-server.json"),
            "/payload/duplicate"
        ),
        false
    );
    assert_eq!(
        fixture_at(
            &load_fixture("duplicate-command-received-server.json"),
            "/payload/duplicate"
        ),
        true
    );
    assert_eq!(
        fixture_at(
            &load_fixture("station-command-succeeded-rockcast.json"),
            "/payload/status"
        ),
        "succeeded"
    );
    assert_eq!(
        fixture_at(
            &load_fixture("station-command-succeeded-rockcast.json"),
            "/payload/error"
        ),
        &JsonValue::Null
    );
    assert_eq!(
        fixture_at(
            &load_fixture("duplicate-command-result-server.json"),
            "/payload/completed_at"
        ),
        fixture_at(
            &load_fixture("station-command-succeeded-rockcast.json"),
            "/payload/completed_at"
        )
    );
    assert_eq!(
        fixture_at(
            &load_fixture("station-command-failed-rockcast.json"),
            "/payload/status"
        ),
        "failed"
    );
    assert!(
        fixture_at(
            &load_fixture("station-command-failed-rockcast.json"),
            "/payload/error"
        )
        .is_object()
    );

    for (request, reply, code) in [
        (
            "unknown-command-client.json",
            "unknown-command-error-server.json",
            "unsupported_command",
        ),
        (
            "stale-manifest-client.json",
            "stale-manifest-error-server.json",
            "stale_revision",
        ),
        (
            "stale-device-state-client.json",
            "stale-device-state-error-server.json",
            "stale_revision",
        ),
        (
            "stale-entity-state-client.json",
            "stale-entity-state-error-server.json",
            "stale_revision",
        ),
        (
            "invalid-sensor-unit-value-client.json",
            "invalid-sensor-unit-value-error-server.json",
            "invalid_payload",
        ),
        (
            "offline-target-command-client.json",
            "offline-target-error-server.json",
            "target_offline",
        ),
        (
            "missing-surface-command-server.json",
            "missing-surface-error-server.json",
            "capability_not_supported",
        ),
    ] {
        let request_fixture = load_fixture(request);
        let request_message_id = fixture_at(&request_fixture, "/message_id");
        let error = load_fixture(reply);
        assert_eq!(
            fixture_at(&error, "/payload/in_reply_to_message_id"),
            request_message_id,
            "{reply} must reply to {request}"
        );
        assert_eq!(
            fixture_at(&error, "/payload/error/code"),
            code,
            "{reply} must report {code}"
        );
    }
    let invalid_sensor = load_fixture("invalid-sensor-unit-value-client.json");
    assert_eq!(fixture_at(&invalid_sensor, "/payload/state/unit"), "°C");
    assert!(
        fixture_at(&invalid_sensor, "/payload/state/value").is_string(),
        "the invalid unit/value fixture must require semantic rejection"
    );

    let registration = load_fixture("rockcast-register-client.json");
    for forbidden in [
        "user_id",
        "device_id",
        "actor",
        "device_secret",
        "access_token",
    ] {
        assert!(
            registration["payload"].get(forbidden).is_none(),
            "registration must not assert {forbidden}"
        );
    }
    let ha = load_fixture("ha-normalized-entity-directory-entry.json");
    assert!(
        ha.get("device_id").is_none() && ha.get("provider_native_id").is_none(),
        "HA projection must not masquerade as a paired device or expose provider IDs"
    );

    // RS-8: the owner-scoped runtime_state projection rides the ordinary directory snapshot
    // and upsert. A target without published state keeps the field absent, and a later
    // upsert carries a strictly higher device state revision than the earlier snapshot.
    let directory_snapshot = load_fixture("directory-snapshot-server.json");
    let devices = directory_snapshot
        .pointer("/payload/directory/devices")
        .and_then(JsonValue::as_array)
        .expect("directory snapshot fixture must list devices");
    let rockcast = devices
        .iter()
        .find(|device| {
            device["device_type"] == json!("rockcast") && device["runtime_state"].is_object()
        })
        .expect("one entry must demonstrate the runtime_state projection");
    assert!(
        devices
            .iter()
            .any(|device| device.get("runtime_state").is_none()),
        "one entry must demonstrate absent runtime_state for a target without published state"
    );
    let upsert = load_fixture("directory-upsert-server.json");
    assert_eq!(
        upsert.pointer("/payload/device/device_id"),
        rockcast.get("device_id"),
        "the upsert fixture must project the same device as the snapshot fixture"
    );
    assert!(
        upsert
            .pointer("/payload/device/runtime_state/state_revision")
            .and_then(JsonValue::as_i64)
            .expect("upsert runtime_state must be revisioned")
            > rockcast
                .pointer("/runtime_state/state_revision")
                .and_then(JsonValue::as_i64)
                .expect("snapshot runtime_state must be revisioned"),
        "the upsert must advance the device's monotonic state_revision over the snapshot"
    );
}

#[test]
fn device_catalog_contract_is_bounded_and_stream_free() {
    let document: JsonValue = serde_yaml::from_str(OPENAPI)
        .map(|value: Value| serde_json::to_value(value).expect("OpenAPI must convert to JSON"))
        .expect("OpenAPI YAML must parse");

    for path in [
        "/api/v1/device-control/catalog/stations",
        "/api/v1/device-control/catalog/search",
    ] {
        let operation = document["paths"]
            .get(path)
            .and_then(|path_item| path_item.get("get"))
            .unwrap_or_else(|| panic!("{path} must define GET"));
        assert_eq!(
            operation
                .get("x-rockserver-status")
                .and_then(JsonValue::as_str),
            Some("implemented"),
            "{path} must be implemented since RS-2"
        );
        let security = operation
            .get("security")
            .and_then(JsonValue::as_array)
            .expect("device catalog operations must declare security");
        assert_eq!(security.len(), 1, "{path} must have one auth alternative");
        assert!(
            security[0]
                .as_object()
                .expect("security alternative must be a mapping")
                .contains_key("RockserverBearer"),
            "{path} must accept only the native-session bearer"
        );
    }

    let browse_parameters = document["paths"]
        .get("/api/v1/device-control/catalog/stations")
        .and_then(|item| item.pointer("/get/parameters"))
        .and_then(JsonValue::as_array)
        .expect("browse must declare parameters");
    let browse_limit = browse_parameters
        .iter()
        .find(|parameter| parameter.get("name").and_then(JsonValue::as_str) == Some("limit"))
        .expect("browse must bound its page size");
    assert_eq!(
        browse_limit
            .pointer("/schema/maximum")
            .and_then(JsonValue::as_i64),
        Some(20),
        "browse pages must be at most 20 stations"
    );
    let cursor = browse_parameters
        .iter()
        .find(|parameter| parameter.get("name").and_then(JsonValue::as_str) == Some("cursor"))
        .expect("browse must declare its cursor");
    assert_eq!(
        cursor
            .pointer("/schema/maxLength")
            .and_then(JsonValue::as_i64),
        Some(512)
    );

    let search_parameters = document["paths"]
        .get("/api/v1/device-control/catalog/search")
        .and_then(|item| item.pointer("/get/parameters"))
        .and_then(JsonValue::as_array)
        .expect("search must declare parameters");
    assert!(
        !search_parameters
            .iter()
            .any(|parameter| parameter.get("name").and_then(JsonValue::as_str) == Some("cursor")),
        "ranked search must not be cursorable"
    );
    let query = search_parameters
        .iter()
        .find(|parameter| parameter.get("name").and_then(JsonValue::as_str) == Some("q"))
        .expect("search must declare q");
    assert_eq!(
        query.get("required").and_then(JsonValue::as_bool),
        Some(true)
    );
    assert_eq!(
        query
            .pointer("/schema/maxLength")
            .and_then(JsonValue::as_i64),
        Some(128),
        "search queries must be at most 128 characters"
    );

    let station_properties = document
        .pointer("/components/schemas/DeviceStationDto/properties")
        .and_then(|properties| properties.as_object())
        .expect("DeviceStationDto must declare properties");
    assert_eq!(
        document
            .pointer("/components/schemas/DeviceStationDto/additionalProperties")
            .and_then(JsonValue::as_bool),
        Some(false),
        "DeviceStationDto must stay closed"
    );
    for forbidden in ["stream_url", "score", "reason", "provider_id"] {
        assert!(
            !station_properties.contains_key(forbidden),
            "DeviceStationDto must never expose {forbidden}"
        );
    }
    for page in ["DeviceCatalogPage", "DeviceSearchPage"] {
        assert_eq!(
            document
                .pointer(&format!(
                    "/components/schemas/{page}/properties/stations/maxItems"
                ))
                .and_then(JsonValue::as_i64),
            Some(20),
            "{page} must cap pages at 20 stations"
        );
    }
    assert!(
        !document
            .pointer("/components/schemas/DeviceSearchPage/properties")
            .and_then(JsonValue::as_object)
            .expect("DeviceSearchPage must declare properties")
            .contains_key("next_cursor"),
        "ranked search pages must not carry a cursor"
    );

    let station = json!({
        "id": "station-rock-001",
        "name": "Highway Rock",
        "homepage_url": null,
        "tags": ["rock"],
        "language": "en",
        "country_code": "GB",
        "codec": "AAC",
        "bitrate_kbps": 128,
        "health": "healthy"
    });
    assert_component_valid(&document, "DeviceStationDto", &station);
    let mut leaked = station.clone();
    leaked["stream_url"] = json!("https://streams.example.com/highway-rock.aac");
    assert_component_invalid(
        &document,
        "DeviceStationDto",
        &leaked,
        "station metadata must never carry a stream URL",
    );

    let mut oversized_page = json!({
        "request_id": "req_01TOOBIG",
        "stations": [],
        "next_cursor": null
    });
    let oversized_stations: Vec<_> = (0..21).map(|_| station.clone()).collect();
    oversized_page["stations"] = json!(oversized_stations);
    assert_component_invalid(
        &document,
        "DeviceCatalogPage",
        &oversized_page,
        "device catalog pages must be at most 20 stations",
    );
    assert_component_invalid(
        &document,
        "DeviceCatalogPage",
        &json!({
            "request_id": "req_01BADCURSOR",
            "stations": [],
            "next_cursor": "not a cursor $%"
        }),
        "cursors must be bounded ASCII stable station IDs",
    );

    let catalog_fixture = load_fixture("device-catalog-response.json");
    assert!(
        catalog_fixture["stations"]
            .as_array()
            .expect("catalog fixture must list stations")
            .iter()
            .all(|station| station.get("stream_url").is_none()),
        "the canonical device catalog fixture must not carry stream URLs"
    );
}

#[test]
fn station_play_stream_and_volume_bounds_are_enforceable() {
    let document: JsonValue = serde_yaml::from_str(OPENAPI)
        .map(|value: Value| serde_json::to_value(value).expect("OpenAPI must convert to JSON"))
        .expect("OpenAPI YAML must parse");

    assert_component_valid(
        &document,
        "StationCommand",
        &json!({"name": "station.play_station", "station_id": "station.jazz_fixture"}),
    );
    assert_component_valid(
        &document,
        "StationCommand",
        &json!({
            "name": "station.play_stream",
            "source": "rockserver_catalog",
            "station_id": "station.jazz_fixture",
            "station": {"name": "Fixture Jazz", "icon_url": null},
            "stream_uri": "https://streams.example.com/quiet-jazz.mp3"
        }),
    );
    assert_component_valid(
        &document,
        "StationCommand",
        &json!({
            "name": "station.play_stream",
            "source": "rockserver_catalog",
            "station_id": "station.jazz_fixture",
            "station": {"name": "Fixture Jazz", "icon_url": "https://icons.example.com/jazz.png"},
            "stream_uri": "https://streams.example.com/quiet-jazz.mp3"
        }),
    );
    assert_component_valid(
        &document,
        "StationCommand",
        &json!({
            "name": "station.play_stream",
            "source": "direct_stream",
            "stream_uri": "https://streams.example.com/highway-rock.aac"
        }),
    );
    assert_component_invalid(
        &document,
        "StationCommand",
        &json!({
            "name": "station.play_stream",
            "source": "rockserver_catalog",
            "station_id": "station.jazz_fixture",
            "stream_uri": "https://streams.example.com/quiet-jazz.mp3"
        }),
        "the server-resolved variant must include the target presentation",
    );
    assert_component_invalid(
        &document,
        "StationCommand",
        &json!({
            "name": "station.play_stream",
            "source": "rockserver_catalog",
            "station_id": "station.jazz_fixture",
            "station": {"name": "Fixture Jazz"},
            "stream_uri": "https://streams.example.com/quiet-jazz.mp3"
        }),
        "the target presentation must explicitly carry nullable icon_url",
    );
    assert_component_invalid(
        &document,
        "StationCommand",
        &json!({
            "name": "station.play_stream",
            "source": "rockserver_catalog",
            "station": {"name": "Fixture Jazz", "icon_url": null},
            "stream_uri": "https://streams.example.com/quiet-jazz.mp3"
        }),
        "the server-resolved variant must echo the resolved station_id",
    );
    assert_component_invalid(
        &document,
        "StationCommand",
        &json!({
            "name": "station.play_stream",
            "source": "direct_stream",
            "station_id": "station.jazz_fixture",
            "stream_uri": "https://streams.example.com/highway-rock.aac"
        }),
        "the direct_stream variant must not claim a catalog station",
    );
    assert_component_invalid(
        &document,
        "StationCommand",
        &json!({
            "name": "station.play_stream",
            "source": "rockserver_catalog",
            "station_id": "station.jazz_fixture",
            "stream_uri": "ftp://streams.example.com/quiet-jazz.mp3"
        }),
        "stream URIs must be absolute HTTP(S)",
    );
    assert_component_invalid(
        &document,
        "StationCommand",
        &json!({
            "name": "station.play_stream",
            "source": "rockserver_catalog",
            "station_id": "station.jazz_fixture",
            "stream_uri": format!("https://streams.example.com/{}.mp3", "a".repeat(2048))
        }),
        "stream URIs must be at most 2048 characters",
    );

    assert_eq!(
        document
            .pointer("/components/schemas/StationStreamUri/x-rockserver-status")
            .and_then(JsonValue::as_str),
        Some("implemented"),
        "the server-resolved play_stream dispatch is implemented since RS-3"
    );
    assert!(
        document
            .pointer("/components/schemas/VoiceStreamCancel/x-rockserver-status")
            .is_none(),
        "the voice cancel frame is implemented by RS-4"
    );

    let honest_volume = json!({
        "name": "media.volume",
        "version": 1,
        "minimum": 0,
        "maximum": 100,
        "step": 1,
        "mute": true
    });
    assert_component_valid(&document, "VolumeCapability", &honest_volume);
    for mutation in [
        json!({"minimum": -1, "maximum": 100, "step": 1}),
        json!({"minimum": 0, "maximum": 101, "step": 1}),
        json!({"minimum": 0, "maximum": 100, "step": 0}),
        json!({"minimum": 0, "maximum": 100, "step": 101}),
    ] {
        let mut broken = honest_volume.clone();
        for (field, value) in mutation.as_object().expect("mutation is an object") {
            broken[field] = value.clone();
        }
        assert_component_invalid(
            &document,
            "VolumeCapability",
            &broken,
            "Rockmobile aborts the whole snapshot on out-of-bounds volume",
        );
    }
    assert_component_valid(
        &document,
        "VolumeCommand",
        &json!({"name": "volume.set_volume", "level": 100}),
    );
    for level in [-1, 101] {
        assert_component_invalid(
            &document,
            "VolumeCommand",
            &json!({"name": "volume.set_volume", "level": level}),
            "volume levels must stay within 0..=100",
        );
    }

    let display_views = document
        .pointer("/components/schemas/DisplayCapability/properties/views/items/enum")
        .and_then(JsonValue::as_array)
        .expect("display views must stay a closed enum");
    assert_eq!(
        display_views,
        &vec![json!("text"), json!("now_playing"), json!("sensor_grid")],
        "no remote presentation types may be added beyond the frozen three"
    );
    assert_component_invalid(
        &document,
        "DisplayCapability",
        &json!({
            "name": "display.presentation",
            "version": 1,
            "views": ["station_list", "search", "loading", "offline", "playback_error"],
            "max_items": 8,
            "max_text_length": 128
        }),
        "local UI states must stay schema-invalid as presentation views",
    );
}

#[test]
fn voice_stream_contract_matches_runtime_and_implemented_device_flow() {
    let document: JsonValue = serde_yaml::from_str(OPENAPI)
        .map(|value: Value| serde_json::to_value(value).expect("OpenAPI must convert to JSON"))
        .expect("OpenAPI YAML must parse");
    let voice_stream = document["paths"]
        .get("/api/v1/voice/stream")
        .and_then(|item| item.get("get"))
        .expect("canonical voice stream path must define GET");
    let limits = voice_stream
        .get("x-voice-stream-limits")
        .expect("voice stream limits must be machine-readable");
    for (name, expected) in [
        ("sample_rate_hz", 16_000),
        ("channels", 1),
        ("max_chunk_bytes", 32_768),
        ("max_session_bytes", 2_097_152),
        ("max_session_audio_seconds", 60),
        ("idle_timeout_seconds", 10),
        ("wall_timeout_seconds", 75),
        ("provider_timeout_seconds", 15),
        ("search_timeout_ms", 5_000),
    ] {
        assert_eq!(
            limits.get(name).and_then(JsonValue::as_i64),
            Some(expected),
            "voice stream limit {name} must match the enforced runtime"
        );
    }
    assert_eq!(
        limits.get("audio_format").and_then(JsonValue::as_str),
        Some("pcm_s16le")
    );
    let client_messages = voice_stream
        .get("x-websocket-client-messages")
        .and_then(|messages| messages.get("oneOf"))
        .and_then(JsonValue::as_array)
        .expect("voice stream must declare client messages");
    assert!(
        client_messages.iter().any(|message| {
            message
                .get("$ref")
                .and_then(JsonValue::as_str)
                .is_some_and(|reference| reference.ends_with("VoiceStreamCancel"))
        }),
        "the implemented cancel frame must be part of the client message set"
    );
    assert!(
        document["components"]["schemas"]["VoiceStreamCancel"]
            .get("x-rockserver-status")
            .is_none(),
        "implemented cancel must not remain planned"
    );
    assert_component_valid(
        &document,
        "VoiceStreamStart",
        &json!({
            "type": "start",
            "sample_rate_hz": 16000,
            "locale": "en-US",
            "surface_id": "voice.main",
            "limit": 10
        }),
    );
    for sample_rate in [8000, 24_000, 44_100] {
        assert_component_invalid(
            &document,
            "VoiceStreamStart",
            &json!({"type": "start", "sample_rate_hz": sample_rate}),
            "only 16 kHz mono pcm_s16le is accepted",
        );
    }
    assert_component_invalid(
        &document,
        "VoiceStreamStart",
        &json!({"type": "start", "sample_rate_hz": 16000, "limit": 50}),
        "the stream path caps the result limit at 10",
    );
    assert_component_valid(&document, "VoiceStreamCancel", &json!({"type": "cancel"}));
    assert_component_invalid(
        &document,
        "VoiceStreamCancel",
        &json!({"type": "cancel", "reason": "user bailed"}),
        "the cancel frame must stay closed",
    );
    assert_component_invalid(
        &document,
        "VoiceStreamCancel",
        &json!({"type": "abort"}),
        "only the exact cancel frame is recognised",
    );
    assert_component_valid(
        &document,
        "VoiceStreamReady",
        &json!({
            "type": "ready",
            "request_id": "req_01VOICE",
            "audio_format": "pcm_s16le",
            "sample_rate_hz": 16000,
            "source_device_id": "40000000-0000-4000-8000-000000000002",
            "surface_id": "voice.main"
        }),
    );
    assert_component_valid(
        &document,
        "VoiceDeviceCommandResult",
        &json!({
            "type": "result",
            "request_id": "req_01VOICE",
            "status": "succeeded"
        }),
    );
    assert_component_valid(
        &document,
        "VoiceStreamError",
        &json!({
            "type": "error",
            "code": "clarification_required",
            "message": "The voice command needs clarification.",
            "request_id": "req_01VOICE",
            "details": {}
        }),
    );
    assert_component_invalid(
        &document,
        "VoiceStreamError",
        &json!({
            "type": "error",
            "code": "unfrozen_error",
            "message": "No.",
            "request_id": "req_01VOICE",
            "details": {}
        }),
        "voice errors must use the frozen code vocabulary",
    );
}

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

#[tokio::test]
async fn device_catalog_paths_are_registered_and_require_a_native_device_session() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    struct EmptyResolver;
    #[async_trait::async_trait]
    impl rockserver::auth::NativeSessionResolver for EmptyResolver {
        async fn resolve_active_native_session(
            &self,
            _access_hash: &rockserver::auth::SecretHash,
        ) -> Result<
            Option<rockserver::auth::ActiveSession>,
            rockserver::auth::NativeSessionLookupError,
        > {
            Ok(None)
        }
    }

    let app = rockserver::http::router_with_search_service_and_native_session_resolver(
        rockserver::search::SearchService::new(std::sync::Arc::new(
            rockserver::search::InMemoryStationRepository::with_builtin_catalog().unwrap(),
        )),
        std::time::Duration::from_secs(5),
        std::sync::Arc::new(EmptyResolver),
    );
    for path in [
        "/api/v1/device-control/catalog/stations",
        "/api/v1/device-control/catalog/search?q=jazz",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::get(path)
                    .header(
                        "authorization",
                        format!("Bearer {}", rockserver::http::TEST_API_BEARER_TOKEN),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::UNAUTHORIZED,
            "{path} is registered since RS-2 and must require a valid device session"
        );
    }
}

#[tokio::test]
async fn search_endpoint_is_registered() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

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

    assert_eq!(response.status(), axum::http::StatusCode::OK);
}
