//! Contract checks for authenticated audio relay behavior.

use super::common::OPENAPI;
use serde_yaml::Value;

#[test]
fn relay_contract_has_session_security_and_icy_responses() {
    let doc: Value = serde_yaml::from_str(OPENAPI).unwrap();
    let operation = &doc["paths"]["/api/v1/stations/{station_id}/stream"]["get"];
    let security = operation["security"].as_sequence().unwrap();
    assert_eq!(security.len(), 2);
    assert!(
        security
            .iter()
            .any(|entry| entry.get("RockserverBrowserCookie").is_some())
    );
    assert!(
        security
            .iter()
            .any(|entry| entry.get("RockserverBearer").is_some())
    );
    assert!(
        operation["parameters"]
            .as_sequence()
            .unwrap()
            .iter()
            .any(|parameter| parameter["name"] == "Icy-MetaData")
    );
    for status in ["200", "400", "401", "404", "429", "502", "503"] {
        assert!(
            operation["responses"].get(status).is_some(),
            "missing {status}"
        );
    }
    assert!(
        operation["responses"]["200"]["headers"]
            .get("icy-metaint")
            .is_some()
    );
}

#[test]
fn metadata_contract_has_security_states_and_sse_reconnect_policy() {
    let doc: Value = serde_yaml::from_str(OPENAPI).unwrap();
    for path in ["now-playing", "events"] {
        let operation = &doc["paths"][format!("/api/v1/stations/{{station_id}}/{path}")]["get"];
        let security = operation["security"].as_sequence().unwrap();
        assert!(
            security
                .iter()
                .any(|entry| entry.get("RockserverBrowserCookie").is_some())
        );
        assert!(
            security
                .iter()
                .any(|entry| entry.get("RockserverBearer").is_some())
        );
        for status in ["200", "400", "401", "404", "503"] {
            assert!(
                operation["responses"].get(status).is_some(),
                "{path}: {status}"
            );
        }
    }
    let schema = &doc["components"]["schemas"]["StationNowPlaying"];
    for property in ["stationId", "rawTitle", "updatedAt", "state"] {
        assert!(
            schema["required"]
                .as_sequence()
                .unwrap()
                .iter()
                .any(|item| item == property)
        );
    }
    assert_eq!(
        schema["properties"]["state"]["enum"]
            .as_sequence()
            .unwrap()
            .len(),
        3
    );
    let events = &doc["paths"]["/api/v1/stations/{station_id}/events"]["get"];
    assert!(
        events["responses"]["200"]["content"]
            .get("text/event-stream")
            .is_some()
    );
    assert!(events["responses"].get("429").is_some());
}
