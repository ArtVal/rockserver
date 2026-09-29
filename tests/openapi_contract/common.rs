//! Shared OpenAPI fixture specifications, schemas, and validation helpers.

use std::{fs, path::Path};

use serde_json::{Value as JsonValue, json};
use serde_yaml::Value;

pub const OPENAPI: &str = include_str!("../../api/openapi.yaml");
pub const FIXTURE_DIRECTORY: &str = "tests/fixtures/device-control/v1";

/// Describes one raw golden fixture and the OpenAPI component that validates it.
pub struct FixtureSpec {
    pub file: &'static str,
    pub schema: &'static str,
    pub valid: bool,
}

pub const DEVICE_CONTROL_FIXTURES: &[FixtureSpec] = &[
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
pub fn load_fixture(file: &str) -> JsonValue {
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
pub fn fixture_schema(document: &JsonValue, component: &str) -> JsonValue {
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
pub fn component_errors(
    document: &JsonValue,
    component: &str,
    instance: &JsonValue,
) -> Vec<String> {
    let schema = fixture_schema(document, component);
    let validator = jsonschema::validator_for(&schema)
        .unwrap_or_else(|error| panic!("{component} schema must compile: {error}"));
    validator
        .iter_errors(instance)
        .map(|error| error.to_string())
        .collect()
}

/// Asserts that an inline instance satisfies an OpenAPI component schema.
pub fn assert_component_valid(document: &JsonValue, component: &str, instance: &JsonValue) {
    let errors = component_errors(document, component, instance);
    assert!(
        errors.is_empty(),
        "{component} must accept the instance; errors: {errors:?}"
    );
}

/// Asserts that an inline instance violates an OpenAPI component schema.
pub fn assert_component_invalid(
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
pub fn fixture_at<'a>(fixture: &'a JsonValue, pointer: &str) -> &'a JsonValue {
    fixture
        .pointer(pointer)
        .unwrap_or_else(|| panic!("fixture must contain {pointer}"))
}

/// Reads a slash-delimited mapping path from a YAML value.
pub fn value_at<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('/').try_fold(root, |value, key| value.get(key))
}

/// Collects local references and operation identifiers from an arbitrary YAML subtree.
pub fn collect_contract_links(
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
