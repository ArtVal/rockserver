//! OpenAPI voice stream protocol, frames, limits, and runtime consistency checks.

use serde_json::{Value as JsonValue, json};
use serde_yaml::Value;

use super::common::{OPENAPI, assert_component_invalid, assert_component_valid};

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
