//! OpenAPI device-control v1 bounding, linking, and golden fixture validation checks.

use std::{collections::BTreeSet, fs, path::Path};

use serde_json::{Value as JsonValue, json};
use serde_yaml::Value;

use super::common::{
    DEVICE_CONTROL_FIXTURES, FIXTURE_DIRECTORY, OPENAPI, collect_contract_links, fixture_at,
    fixture_schema, load_fixture, value_at,
};

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
