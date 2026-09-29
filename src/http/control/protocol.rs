//! Private v1 WebSocket wire DTOs and bounded frame helpers.

use std::time::Duration;

use axum::extract::ws::{CloseFrame, Message, WebSocket};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

use crate::device_control::{
    DeviceControlScope, DeviceManifest, DeviceStateDelta, DeviceStateSnapshot, EntityState,
};

pub(super) const MAX_FRAME_BYTES: usize = 65_536;
const MAX_PAYLOAD_BYTES: usize = 61_440;

/// Typed client envelope shared by the v1 messages accepted in this lifecycle stage.
#[derive(Deserialize)]
pub(super) struct ClientEnvelope<T> {
    pub(super) protocol_version: u8,
    pub(super) message_id: Uuid,
    #[serde(rename = "type")]
    pub(super) kind: String,
    pub(super) sent_at: String,
    pub(super) payload: T,
}

/// Typed server envelope shared by v1 replies emitted in this lifecycle stage.
#[derive(Serialize)]
struct ServerEnvelope<'a, T> {
    protocol_version: u8,
    message_id: Uuid,
    #[serde(rename = "type")]
    kind: &'a str,
    sent_at: String,
    payload: T,
}

/// Client-supported major protocol versions.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HelloPayload {
    pub(super) supported_protocol_versions: Vec<u8>,
}

/// Bounded registration metadata and the first typed manifest declaration.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RegisterPayload {
    pub(super) device_type: String,
    pub(super) app_version: String,
    pub(super) manifest: DeviceManifest,
}

/// Typed manifest replacement after registration.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ManifestPayload {
    pub(super) manifest: DeviceManifest,
}

/// Required full runtime-state observation after every register or reconnect.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FullStatePayload {
    pub(super) snapshot: DeviceStateSnapshot,
}

/// Ordered partial runtime-state observation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StateDeltaPayload {
    pub(super) delta: DeviceStateDelta,
}

/// Typed latest entity observation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EntityStatePayload {
    pub(super) state: EntityState,
}

/// Explicit request to replace a state stream with a full snapshot.
#[derive(Serialize)]
pub(super) struct ResyncRequestedPayload {
    pub(super) kind: &'static str,
    pub(super) reason: &'static str,
}

/// Initial controller directory snapshot delivered on the authenticated control socket.
#[derive(Serialize)]
pub(super) struct DirectorySnapshotPayload {
    pub(super) event_id: Uuid,
    pub(super) directory: super::super::directory::DirectoryDto,
}

/// One replacement directory entry delivered after an owned control transition.
#[derive(Serialize)]
pub(super) struct DirectoryUpsertPayload {
    pub(super) event_id: Uuid,
    pub(super) directory_revision: u64,
    pub(super) device: super::super::directory::DirectoryDeviceDto,
}

/// Server-owned control-plane limits sent after version negotiation.
#[derive(Serialize)]
pub(super) struct DeviceControlLimits {
    max_json_frame_bytes: u32,
    max_payload_bytes: u32,
    max_capabilities: u8,
    max_entities: u16,
    max_surfaces: u8,
    max_directory_devices: u8,
    heartbeat_interval_seconds: u8,
    offline_ttl_seconds: u8,
    max_in_flight_per_connection: u8,
    max_in_flight_per_target_device: u8,
    default_command_timeout_seconds: u8,
    max_command_timeout_seconds: u8,
    command_idempotency_window_seconds: u32,
    max_stream_redirects: u8,
}

/// Version-negotiation reply.
#[derive(Serialize)]
pub(super) struct WelcomePayload {
    pub(super) selected_protocol_version: u8,
    pub(super) connection_id: Uuid,
    pub(super) registration_deadline_seconds: u8,
    pub(super) limits: DeviceControlLimits,
}

/// Registration acknowledgement with only server-derived identity fields.
#[derive(Serialize)]
pub(super) struct RegisteredPayload {
    pub(super) connection_id: Uuid,
    pub(super) authenticated_user_id: Uuid,
    pub(super) authenticated_device_id: Uuid,
    pub(super) granted_scopes: Vec<&'static str>,
    pub(super) heartbeat_interval_seconds: u8,
    pub(super) offline_ttl_seconds: u8,
    pub(super) require_full_state: bool,
}

/// Client heartbeat payload.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HeartbeatPayload {
    pub(super) sequence: u64,
}

/// Server heartbeat acknowledgement.
#[derive(Serialize)]
pub(super) struct HeartbeatAckPayload {
    pub(super) sequence: u64,
    pub(super) server_time: String,
}

/// Safe protocol error body.
#[derive(Serialize)]
pub(super) struct ProtocolErrorPayload<'a> {
    pub(super) error: ProtocolError<'a>,
}

/// Safe fixed error details that never include credentials or raw parser errors.
#[derive(Serialize)]
pub(super) struct ProtocolError<'a> {
    pub(super) code: &'a str,
    pub(super) message: &'a str,
    pub(super) request_id: String,
    pub(super) details: std::collections::BTreeMap<String, String>,
}

pub(super) enum ProtocolParseError {
    Invalid,
    TooLarge,
}

pub(super) async fn receive_message<T>(
    socket: &mut WebSocket,
    timeout: Duration,
    expected_kind: &str,
) -> Result<ClientEnvelope<T>, ProtocolParseError>
where
    T: DeserializeOwned + Serialize,
{
    match tokio::time::timeout(timeout, socket.recv()).await {
        Ok(Some(Ok(Message::Text(text)))) => parse_message(&text, expected_kind),
        _ => Err(ProtocolParseError::Invalid),
    }
}

fn parse_message<T>(
    text: &str,
    expected_kind: &str,
) -> Result<ClientEnvelope<T>, ProtocolParseError>
where
    T: DeserializeOwned + Serialize,
{
    if text.len() > MAX_FRAME_BYTES {
        return Err(ProtocolParseError::TooLarge);
    }
    let message: ClientEnvelope<T> =
        serde_json::from_str(text).map_err(|_| ProtocolParseError::Invalid)?;
    if message.protocol_version != 1
        || message.kind != expected_kind
        || OffsetDateTime::parse(&message.sent_at, &Rfc3339).is_err()
        || serde_json::to_vec(&message.payload)
            .map_err(|_| ProtocolParseError::Invalid)?
            .len()
            > MAX_PAYLOAD_BYTES
    {
        return Err(ProtocolParseError::Invalid);
    }
    let _ = message.message_id;
    Ok(message)
}

/// Parses a bounded, versioned envelope before dispatching to a typed payload DTO.
pub(super) fn parse_any(text: &str) -> Result<ClientEnvelope<Value>, ProtocolParseError> {
    if text.len() > MAX_FRAME_BYTES {
        return Err(ProtocolParseError::TooLarge);
    }
    let message: ClientEnvelope<Value> =
        serde_json::from_str(text).map_err(|_| ProtocolParseError::Invalid)?;
    if message.protocol_version != 1
        || message.kind.len() > 64
        || OffsetDateTime::parse(&message.sent_at, &Rfc3339).is_err()
        || serde_json::to_vec(&message.payload)
            .map_err(|_| ProtocolParseError::Invalid)?
            .len()
            > MAX_PAYLOAD_BYTES
    {
        return Err(ProtocolParseError::Invalid);
    }
    Ok(message)
}

pub(super) fn valid_hello(payload: &HelloPayload) -> bool {
    (1..=4).contains(&payload.supported_protocol_versions.len())
        && payload.supported_protocol_versions.contains(&1)
}

pub(super) fn valid_registration(payload: &RegisterPayload) -> bool {
    !payload.device_type.is_empty()
        && payload.device_type.len() <= 64
        && !payload.app_version.is_empty()
        && payload.app_version.len() <= 64
}

pub(super) fn now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .expect("RFC3339 formatter is valid")
}

pub(super) fn limits() -> DeviceControlLimits {
    DeviceControlLimits {
        max_json_frame_bytes: 65_536,
        max_payload_bytes: 61_440,
        max_capabilities: 32,
        max_entities: 256,
        max_surfaces: 16,
        max_directory_devices: 50,
        heartbeat_interval_seconds: 20,
        offline_ttl_seconds: 60,
        max_in_flight_per_connection: 16,
        max_in_flight_per_target_device: 8,
        default_command_timeout_seconds: 10,
        max_command_timeout_seconds: 30,
        command_idempotency_window_seconds: 86_400,
        max_stream_redirects: 5,
    }
}

pub(super) async fn send_envelope<T: Serialize>(
    socket: &mut WebSocket,
    kind: &str,
    payload: T,
) -> Result<(), ()> {
    let message = ServerEnvelope {
        protocol_version: 1,
        message_id: Uuid::new_v4(),
        kind,
        sent_at: now(),
        payload,
    };
    socket
        .send(Message::Text(
            serde_json::to_string(&message)
                .expect("device-control server DTO serializes")
                .into(),
        ))
        .await
        .map_err(|_| ())
}

pub(super) async fn protocol_close(
    socket: &mut WebSocket,
    code: &str,
    close_code: u16,
) -> Result<(), ()> {
    let _ = send_envelope(
        socket,
        "protocol.error",
        ProtocolErrorPayload {
            error: ProtocolError {
                code,
                message: "Device-control protocol error.",
                request_id: Uuid::new_v4().to_string(),
                details: Default::default(),
            },
        },
    )
    .await;
    socket
        .send(Message::Close(Some(CloseFrame {
            code: close_code,
            reason: code.into(),
        })))
        .await
        .map_err(|_| ())
}

/// Maps an internal control scope to its wire representation.
pub(super) fn scope_name(scope: &DeviceControlScope) -> &'static str {
    match scope {
        DeviceControlScope::DirectoryRead => "device.directory.read",
        DeviceControlScope::PresenceRead => "device.presence.read",
        DeviceControlScope::EntityStateRead => "entity.state.read",
        DeviceControlScope::MediaControl => "media.control",
        DeviceControlScope::DisplayControl => "display.control",
        DeviceControlScope::ActuatorControl => "actuator.control",
    }
}

/// Sends a formatted error envelope over the control socket when a command fails.
pub(super) async fn send_command_error(
    socket: &mut WebSocket,
    code: &'static str,
) -> Result<(), ()> {
    send_envelope(
        socket,
        "protocol.error",
        ProtocolErrorPayload {
            error: ProtocolError {
                code,
                message: "Device command was rejected.",
                request_id: Uuid::new_v4().to_string(),
                details: Default::default(),
            },
        },
    )
    .await
}
