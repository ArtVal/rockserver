//! Native device-control WebSocket ingress and its bounded v1 handshake.

use axum::{
    extract::{
        State, WebSocketUpgrade,
        ws::{CloseFrame, Message, WebSocket},
    },
    http::HeaderMap,
    response::Response,
};
use serde_json::json;
use uuid::Uuid;

use crate::{
    device_control::{
        CommandAccepted, CommandResult, DeviceCommand, DeviceControlScope, DeviceId,
        DeviceManifest, DeviceRole, DeviceStateSnapshot, RevisionOrder, revision_order,
    },
    device_control_auth::{DeviceControlAuthenticationError, DeviceControlPrincipal},
    device_control_command::CommandRouter,
    device_control_presence::{
        ConnectionRegistration, ConnectionRegistry, DisconnectReason, control_shutdown_subscriber,
    },
    device_control_state::StateHub,
};

use super::{
    control_auth::authenticate_control_ingress,
    state::AppState,
    transport::{error_response, request_id, retry_after, unauthorized_response, with_request_id},
};

#[path = "control/protocol.rs"]
mod protocol;
#[path = "control/state.rs"]
mod state;

use protocol::*;
use state::*;

/// Shared process-local dependencies for one authenticated control socket.
struct ControlRuntime {
    registry: ConnectionRegistry,
    state_hub: StateHub,
    store: Option<std::sync::Arc<dyn crate::device_control::DeviceControlStore>>,
    commands: CommandRouter,
    timing: super::state::ControlTiming,
    directory_state: AppState,
}

/// Authenticates then upgrades the canonical device-control WebSocket endpoint.
pub(super) async fn connect(
    State(state): State<AppState>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let request_id = request_id(&headers);
    let Some(resolver) = state.control_session_resolver.as_ref() else {
        return retry_after(
            error_response(
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "control_unavailable",
                "Device control is temporarily unavailable.",
                &request_id,
                json!({}),
            ),
            1,
        );
    };
    let principal = match authenticate_control_ingress(&headers, resolver.as_ref()).await {
        Ok(principal) => principal,
        Err(DeviceControlAuthenticationError::InvalidCredential) => {
            return unauthorized_response(&request_id);
        }
        Err(DeviceControlAuthenticationError::Unavailable) => {
            return retry_after(
                error_response(
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    "control_auth_unavailable",
                    "Device control authentication is temporarily unavailable.",
                    &request_id,
                    json!({}),
                ),
                1,
            );
        }
    };
    let registry = state.control_registry.clone();
    let state_hub = state.control_state_hub.clone();
    let store = state.control_store.clone();
    let timing = state.control_timing;
    let commands = state.control_commands.clone();
    with_request_id(
        upgrade
            .max_message_size(MAX_FRAME_BYTES + 1)
            .on_upgrade(move |socket| {
                run(
                    socket,
                    principal,
                    ControlRuntime {
                        registry,
                        state_hub,
                        store,
                        commands,
                        timing,
                        directory_state: state,
                    },
                )
            }),
        &request_id,
    )
}

async fn run(mut socket: WebSocket, principal: DeviceControlPrincipal, runtime: ControlRuntime) {
    let ControlRuntime {
        registry,
        state_hub,
        store,
        commands,
        timing,
        directory_state,
    } = runtime;
    let connection_id = Uuid::new_v4();
    match receive_message::<HelloPayload>(
        &mut socket,
        timing.registration_deadline,
        "protocol.hello",
    )
    .await
    {
        Ok(envelope) if valid_hello(&envelope.payload) => {}
        Ok(_) => {
            let _ = protocol_close(&mut socket, "unsupported_protocol_version", 1002).await;
            return;
        }
        Err(ProtocolParseError::TooLarge) => {
            let _ = protocol_close(&mut socket, "frame_too_large", 1009).await;
            return;
        }
        Err(ProtocolParseError::Invalid) => {
            let _ = protocol_close(&mut socket, "invalid_message", 1007).await;
            return;
        }
    }
    if send_envelope(
        &mut socket,
        "protocol.welcome",
        WelcomePayload {
            selected_protocol_version: 1,
            connection_id,
            registration_deadline_seconds: 10,
            limits: limits(),
        },
    )
    .await
    .is_err()
    {
        return;
    }
    state_hub.publish_presence(principal.user_id, DeviceId(principal.device_id), true);
    let register = match receive_message::<RegisterPayload>(
        &mut socket,
        timing.registration_deadline,
        "device.register",
    )
    .await
    {
        Ok(envelope) => envelope,
        Err(ProtocolParseError::TooLarge) => {
            let _ = protocol_close(&mut socket, "frame_too_large", 1009).await;
            return;
        }
        _ => {
            let _ = protocol_close(&mut socket, "registration_required", 1008).await;
            return;
        }
    };
    if !valid_registration(&register.payload) || register.payload.manifest.validate().is_err() {
        let _ = protocol_close(&mut socket, "invalid_message", 1007).await;
        return;
    }
    if let Some(store) = &store
        && !matches!(
            store
                .apply_manifest(
                    principal.user_id,
                    DeviceId(principal.device_id),
                    register.payload.manifest.clone()
                )
                .await,
            Ok(crate::device_control::StoreOutcome::Accepted
                | crate::device_control::StoreOutcome::Replay)
        )
    {
        let _ = protocol_close(&mut socket, "registration_rejected", 1008).await;
        return;
    }
    let (replacement, mut replaced) = registry.replacement_channel();
    let (outbound, mut outbound_messages) = registry.outbound_channel();
    let granted_scopes = granted_scopes(&register.payload.manifest);
    let replaced_previous = registry.register(
        ConnectionRegistration {
            user_id: principal.user_id,
            device_id: principal.device_id,
            connection_id,
            replacement,
            outbound,
            manifest: register.payload.manifest.clone(),
            scopes: granted_scopes.clone(),
        },
        std::time::Instant::now(),
    );
    if let Some((replaced_owner, replaced_connection)) = replaced_previous {
        commands
            .disconnected(
                &registry,
                store.as_ref(),
                replaced_owner,
                principal.device_id,
                replaced_connection,
            )
            .await;
    }
    state_hub.publish_manifest(principal.user_id, DeviceId(principal.device_id));
    if send_envelope(
        &mut socket,
        "device.registered",
        RegisteredPayload {
            connection_id,
            authenticated_user_id: principal.user_id,
            authenticated_device_id: principal.device_id,
            granted_scopes: granted_scopes.iter().map(scope_name).collect(),
            heartbeat_interval_seconds: 20,
            offline_ttl_seconds: 60,
            require_full_state: true,
        },
    )
    .await
    .is_err()
    {
        registry.disconnect(
            principal.device_id,
            connection_id,
            DisconnectReason::TransportLost,
            std::time::Instant::now(),
        );
        return;
    }
    let directory_enabled = granted_scopes.contains(&DeviceControlScope::DirectoryRead);
    let mut directory_events = state_hub.subscribe(principal.user_id);
    if directory_enabled {
        let cursor = state_hub.cursor(principal.user_id);
        let snapshot = match super::directory::snapshot(
            &directory_state,
            principal.user_id,
            &granted_scopes,
            &super::directory::DirectoryFilters::default(),
            cursor,
        )
        .await
        {
            Ok(snapshot) => snapshot,
            Err(()) => {
                let _ = protocol_close(&mut socket, "directory_unavailable", 1011).await;
                return;
            }
        };
        if send_envelope(
            &mut socket,
            "directory.snapshot",
            DirectorySnapshotPayload {
                event_id: Uuid::new_v4(),
                directory: snapshot,
            },
        )
        .await
        .is_err()
        {
            return;
        }
    }
    let mut reason = DisconnectReason::TransportLost;
    let mut last_seen = tokio::time::Instant::now();
    let mut shutdown = control_shutdown_subscriber();
    let mut manifest = register.payload.manifest;
    // A controller-only device publishes no runtime facts. A player/controller hybrid still must
    // establish a full state before heartbeats, exactly like every other fact-publishing device.
    let mut needs_full_state = requires_full_state(&manifest.roles);
    loop {
        tokio::select! {
            _ = shutdown.recv() => { let _ = socket.send(Message::Close(Some(CloseFrame { code: 1001, reason: "server_shutdown".into() }))).await; break; }
            _ = tokio::time::sleep_until(last_seen + timing.offline_ttl) => { reason = DisconnectReason::HeartbeatExpired; let _ = protocol_close(&mut socket, "heartbeat_expired", 1008).await; break; }
            replacement_reason = replaced.recv() => { reason = replacement_reason.unwrap_or(DisconnectReason::TransportLost); let _ = socket.send(Message::Close(Some(CloseFrame { code: 4001, reason: "replaced".into() }))).await; break; }
            outbound = outbound_messages.recv() => match outbound {
                Some(outbound) => if send_envelope(&mut socket, outbound.kind, outbound.payload).await.is_err() { break; },
                None => break,
            },
            event = directory_events.recv(), if directory_enabled => match event {
                Ok(event) => {
                    let device_id = match event {
                        crate::device_control_state::StateEvent::Manifest { device_id }
                        | crate::device_control_state::StateEvent::Presence { device_id, .. }
                        | crate::device_control_state::StateEvent::DeviceState { device_id, .. }
                        | crate::device_control_state::StateEvent::EntityState { device_id, .. } => device_id,
                    };
                    let cursor = state_hub.cursor(principal.user_id);
                    match super::directory::snapshot(&directory_state, principal.user_id, &granted_scopes, &super::directory::DirectoryFilters::default(), cursor).await {
                        Ok(snapshot) => if let Some(device) = snapshot.devices.into_iter().find(|device| device.device_id == device_id)
                            && send_envelope(&mut socket, "directory.upsert", DirectoryUpsertPayload { event_id: Uuid::new_v4(), directory_revision: cursor.max(1), device }).await.is_err() { break; },
                        Err(()) => { let _ = protocol_close(&mut socket, "directory_unavailable", 1011).await; break; }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => { let _ = protocol_close(&mut socket, "directory_resync_required", 1008).await; break; }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            },
            message = socket.recv() => match message {
                Some(Ok(Message::Text(text))) => match parse_any(&text) {
                    Ok(envelope) => match envelope.kind.as_str() {
                        "device.heartbeat" => match serde_json::from_value::<HeartbeatPayload>(envelope.payload) {
                            Ok(payload) => {
                                if needs_full_state {
                                    let _ = protocol_close(&mut socket, "full_state_required", 1008).await;
                                    break;
                                }
                                last_seen = tokio::time::Instant::now();
                                if !registry.heartbeat(principal.device_id, connection_id, std::time::Instant::now()) { reason = DisconnectReason::Replaced; break; }
                                if send_envelope(&mut socket, "device.heartbeat_ack", HeartbeatAckPayload { sequence: payload.sequence, server_time: now() }).await.is_err() { break; }
                            }
                            Err(_) => { let _ = protocol_close(&mut socket, "invalid_message", 1007).await; break; }
                        },
                        "device.manifest" => match serde_json::from_value::<ManifestPayload>(envelope.payload) {
                            Ok(payload) if payload.manifest.validate().is_ok() => {
                                let persisted = match &store { Some(store) => matches!(store.apply_manifest(principal.user_id, DeviceId(principal.device_id), payload.manifest.clone()).await, Ok(crate::device_control::StoreOutcome::Accepted | crate::device_control::StoreOutcome::Replay)), None => true };
                                if persisted { manifest = payload.manifest; let _ = registry.update_manifest(principal.device_id, connection_id, manifest.clone()); state_hub.publish_manifest(principal.user_id, DeviceId(principal.device_id)); } else { let _ = protocol_close(&mut socket, "manifest_rejected", 1008).await; break; }
                            }
                            _ => { let _ = protocol_close(&mut socket, "invalid_message", 1007).await; break; }
                        },
                        "device.state_full" => match serde_json::from_value::<FullStatePayload>(envelope.payload) {
                            Ok(payload) if valid_snapshot(&payload.snapshot) => {
                                let persisted = match &store {
                                    Some(store) => accepted_full_snapshot(store.store_device_state(principal.user_id, DeviceId(principal.device_id), payload.snapshot.clone()).await),
                                    None => true,
                                };
                                if !persisted { needs_full_state = true; let _ = send_envelope(&mut socket, "device.resync_requested", ResyncRequestedPayload { kind: "device_state", reason: "revision_gap" }).await; continue; }
                                match accept_snapshot(&state_hub, principal.user_id, principal.device_id, payload.snapshot) {
                                    // A stale full snapshot never replaces the latest server projection, but it is
                                    // still a valid reconnect handshake. Blocking its heartbeat would drop an
                                    // otherwise healthy player every heartbeat interval.
                                    RevisionOrder::Next | RevisionOrder::Replay | RevisionOrder::Stale => needs_full_state = false,
                                    RevisionOrder::Conflict | RevisionOrder::Gap => { needs_full_state = true; let _ = send_envelope(&mut socket, "device.resync_requested", ResyncRequestedPayload { kind: "device_state", reason: "revision_gap" }).await; }
                                }
                            }
                            _ => { let _ = protocol_close(&mut socket, "invalid_message", 1007).await; break; }
                        },
                        "device.state_delta" => match serde_json::from_value::<StateDeltaPayload>(envelope.payload) {
                            Ok(payload) if !needs_full_state && valid_delta(&payload.delta) => {
                                let Some(current) = state_hub.device_state(principal.user_id, DeviceId(principal.device_id)) else { needs_full_state = true; let _ = send_envelope(&mut socket, "device.resync_requested", ResyncRequestedPayload { kind: "device_state", reason: "missing_base" }).await; continue; };
                                let merged = DeviceStateSnapshot { state_revision: payload.delta.state_revision, observed_at: payload.delta.observed_at, received_at: None, state: merge_state(current.state.clone(), payload.delta.changes) };
                                match revision_order(current.state_revision, &current.state, merged.state_revision, &merged.state, Some(payload.delta.base_revision)) {
                                    RevisionOrder::Next => {
                                        let persisted = match &store {
                                            Some(store) => matches!(store.store_device_state(principal.user_id, DeviceId(principal.device_id), merged.clone()).await, Ok(crate::device_control::StoreOutcome::Accepted | crate::device_control::StoreOutcome::Replay)),
                                            None => true,
                                        };
                                        if persisted { state_hub.publish_device_state(principal.user_id, DeviceId(principal.device_id), merged) } else { needs_full_state = true; let _ = send_envelope(&mut socket, "device.resync_requested", ResyncRequestedPayload { kind: "device_state", reason: "revision_gap" }).await; }
                                    },
                                    RevisionOrder::Replay | RevisionOrder::Stale => {},
                                    RevisionOrder::Conflict | RevisionOrder::Gap => { needs_full_state = true; let _ = send_envelope(&mut socket, "device.resync_requested", ResyncRequestedPayload { kind: "device_state", reason: "revision_gap" }).await; }
                                }
                            }
                            Ok(_) => { needs_full_state = true; let _ = send_envelope(&mut socket, "device.resync_requested", ResyncRequestedPayload { kind: "device_state", reason: "missing_base" }).await; }
                            Err(_) => { let _ = protocol_close(&mut socket, "invalid_message", 1007).await; break; }
                        },
                        "entity.state" => match serde_json::from_value::<EntityStatePayload>(envelope.payload) {
                            Ok(mut payload) if manifest.entities.iter().any(|entity| payload.state.validate_for(entity).is_ok()) => {
                                payload.state.freshness = Some(payload.state.freshness_at(&crate::device_control::Timestamp::parse(now()).expect("server time")));
                                match entity_revision(&state_hub, principal.user_id, DeviceId(principal.device_id), &payload.state) {
                                    RevisionOrder::Next => {
                                        match &store {
                                            Some(store) => match store.store_entity_state(principal.user_id, DeviceId(principal.device_id), payload.state.clone()).await {
                                                Ok(crate::device_control::StoreOutcome::Accepted | crate::device_control::StoreOutcome::Replay) => state_hub.publish_entity_state(principal.user_id, DeviceId(principal.device_id), payload.state),
                                                Ok(crate::device_control::StoreOutcome::Stale) => {},
                                                _ => { let _ = send_envelope(&mut socket, "device.resync_requested", ResyncRequestedPayload { kind: "entity_state", reason: "revision_gap" }).await; }
                                            },
                                            None => state_hub.publish_entity_state(principal.user_id, DeviceId(principal.device_id), payload.state),
                                        }
                                    }
                                    RevisionOrder::Replay | RevisionOrder::Stale => {},
                                    RevisionOrder::Conflict | RevisionOrder::Gap => { let _ = send_envelope(&mut socket, "device.resync_requested", ResyncRequestedPayload { kind: "entity_state", reason: "revision_gap" }).await; }
                                }
                            }
                            _ => { let _ = protocol_close(&mut socket, "invalid_message", 1007).await; break; }
                        },
                        "device.command" => match serde_json::from_value::<DeviceCommand>(envelope.payload) {
                            Ok(command) => if let Err(error) = commands.submit(&registry, store.as_ref(), principal.user_id, principal.device_id, connection_id, envelope.message_id.to_string(), command).await { let _ = send_command_error(&mut socket, error.code).await; },
                            Err(_) => { let _ = send_command_error(&mut socket, "invalid_payload").await; }
                        },
                        "command.accepted" => match serde_json::from_value::<CommandAccepted>(envelope.payload) {
                            Ok(accepted) => if let Err(error) = commands.accepted(&registry, principal.user_id, principal.device_id, connection_id, accepted) { let _ = send_command_error(&mut socket, error.code).await; },
                            Err(_) => { let _ = send_command_error(&mut socket, "invalid_payload").await; }
                        },
                        "command.result" => match serde_json::from_value::<CommandResult>(envelope.payload) {
                            Ok(result) => if let Err(error) = commands.result(&registry, store.as_ref(), principal.user_id, principal.device_id, connection_id, result).await { let _ = send_command_error(&mut socket, error.code).await; },
                            Err(_) => { let _ = send_command_error(&mut socket, "invalid_payload").await; }
                        },
                        _ => { let _ = protocol_close(&mut socket, "invalid_message", 1007).await; break; }
                    },
                    Err(ProtocolParseError::TooLarge) => { let _ = protocol_close(&mut socket, "frame_too_large", 1009).await; break; }
                    Err(ProtocolParseError::Invalid) => { let _ = protocol_close(&mut socket, "invalid_message", 1007).await; break; }
                },
                Some(Ok(Message::Close(_))) => { reason = DisconnectReason::GracefulDisconnect; break; }
                Some(Ok(Message::Binary(_))) => { let _ = protocol_close(&mut socket, "invalid_message", 1003).await; break; }
                Some(Ok(_)) => {},
                Some(Err(_)) | None => break,
            }
        }
    }
    if registry.disconnect(
        principal.device_id,
        connection_id,
        reason,
        std::time::Instant::now(),
    ) {
        commands
            .disconnected(
                &registry,
                store.as_ref(),
                principal.user_id,
                principal.device_id,
                connection_id,
            )
            .await;
        state_hub.publish_presence(principal.user_id, DeviceId(principal.device_id), false);
    }
}

fn granted_scopes(manifest: &DeviceManifest) -> Vec<DeviceControlScope> {
    super::directory::granted_scopes(manifest)
}

fn requires_full_state(roles: &[DeviceRole]) -> bool {
    roles.iter().any(|role| *role != DeviceRole::Controller)
}

fn scope_name(scope: &DeviceControlScope) -> &'static str {
    match scope {
        DeviceControlScope::DirectoryRead => "device.directory.read",
        DeviceControlScope::PresenceRead => "device.presence.read",
        DeviceControlScope::EntityStateRead => "entity.state.read",
        DeviceControlScope::MediaControl => "media.control",
        DeviceControlScope::DisplayControl => "display.control",
        DeviceControlScope::ActuatorControl => "actuator.control",
    }
}

async fn send_command_error(socket: &mut WebSocket, code: &'static str) -> Result<(), ()> {
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

/// A stale full snapshot never mutates persistence, but it proves the reconnecting player still
/// has a valid base state and may continue sending heartbeats.
fn accepted_full_snapshot(
    outcome: Result<crate::device_control::StoreOutcome, crate::device_control::StoreError>,
) -> bool {
    matches!(
        outcome,
        Ok(crate::device_control::StoreOutcome::Accepted
            | crate::device_control::StoreOutcome::Replay
            | crate::device_control::StoreOutcome::Stale)
    )
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        net::SocketAddr,
        sync::{Arc, Mutex, OnceLock},
        time::Duration,
    };

    use axum::Router;
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
    };

    use super::*;
    use crate::{
        auth::{ActiveSession, NativeSessionLookupError, NativeSessionResolver, SecretHash},
        device_control::{DeviceRuntimeState, DeviceStateSnapshot},
        http::endpoints::{
            build_router,
            state::{AppState, ControlTiming, PublicLimitState},
        },
        search::{InMemoryStationRepository, SearchService},
        voice::{SpeechRecognizers, UnavailableSpeechRecognizer},
    };

    static TRANSPORT_TEST_GATE: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

    #[test]
    fn controller_only_connections_do_not_require_a_runtime_snapshot() {
        assert!(!requires_full_state(&[DeviceRole::Controller]));
        assert!(requires_full_state(&[DeviceRole::Player]));
        assert!(requires_full_state(&[
            DeviceRole::Controller,
            DeviceRole::Player
        ]));
    }

    #[test]
    fn stale_persisted_full_snapshot_is_a_valid_reconnect_handshake() {
        assert!(accepted_full_snapshot(Ok(
            crate::device_control::StoreOutcome::Stale
        )));
    }

    #[derive(Default)]
    struct FakeResolver {
        sessions: Mutex<HashMap<Vec<u8>, ActiveSession>>,
        unavailable: bool,
    }

    #[async_trait::async_trait]
    impl NativeSessionResolver for FakeResolver {
        async fn resolve_active_native_session(
            &self,
            hash: &SecretHash,
        ) -> Result<Option<ActiveSession>, NativeSessionLookupError> {
            if self.unavailable {
                return Err(NativeSessionLookupError);
            }
            Ok(self
                .sessions
                .lock()
                .expect("test resolver mutex")
                .get(hash.as_bytes())
                .copied())
        }
    }

    fn resolver(token: &str, session: ActiveSession) -> Arc<FakeResolver> {
        let resolver = Arc::new(FakeResolver::default());
        resolver
            .sessions
            .lock()
            .expect("test resolver mutex")
            .insert(Sha256::digest(token.as_bytes()).to_vec(), session);
        resolver
    }

    fn app(
        resolver: Arc<dyn NativeSessionResolver>,
        registry: ConnectionRegistry,
        ttl: Duration,
        hub: crate::device_control_state::StateHub,
    ) -> Router {
        let search_service = SearchService::new(Arc::new(
            InMemoryStationRepository::with_builtin_catalog().unwrap(),
        ));
        build_router(AppState {
            search_service: search_service.clone(),
            speech_recognizers: SpeechRecognizers::same(Arc::new(UnavailableSpeechRecognizer)),
            voice_command_interpreter: Arc::new(crate::voice::DeterministicCommandInterpreter),
            voice_command_timeout: Duration::from_secs(5),
            api_bearer_token: "unrelated".to_owned(),
            account_store: None,
            admin_store: None,
            trusted_proxy_token: None,
            local_admin_origin: None,
            public_limits: Arc::new(Mutex::new(PublicLimitState::default())),
            control_registry: registry,
            control_commands: crate::device_control_command::CommandRouter::default()
                .with_station_catalog(Arc::new(search_service)),
            control_state_hub: hub,
            control_store: None,
            control_session_resolver: Some(resolver),
            personal_store: None,
            icon_import: None,
            yandex_home: None,
            control_timing: ControlTiming {
                registration_deadline: Duration::from_millis(100),
                offline_ttl: ttl,
            },
        })
    }

    async fn server(app: Router) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (address, task)
    }

    async fn upgrade(address: SocketAddr, token: &str) -> (TcpStream, String) {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(format!("GET /api/v1/devices/connect HTTP/1.1\r\nHost: {address}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nAuthorization: Bearer {token}\r\n\r\n").as_bytes()).await.unwrap();
        let mut response = Vec::new();
        loop {
            let byte = stream.read_u8().await.unwrap();
            response.push(byte);
            if response.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        (stream, String::from_utf8(response).unwrap())
    }

    async fn write_frame(stream: &mut TcpStream, opcode: u8, payload: &[u8]) {
        let mut frame = vec![0x80 | opcode];
        match payload.len() {
            length @ 0..=125 => frame.push(0x80 | length as u8),
            length if length <= usize::from(u16::MAX) => {
                frame.push(0x80 | 126);
                frame.extend_from_slice(&(length as u16).to_be_bytes());
            }
            length => {
                frame.push(0x80 | 127);
                frame.extend_from_slice(&(length as u64).to_be_bytes());
            }
        }
        let mask = [1_u8, 2, 3, 4];
        frame.extend_from_slice(&mask);
        frame.extend(
            payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % 4]),
        );
        stream.write_all(&frame).await.unwrap();
    }

    async fn read_frame(stream: &mut TcpStream) -> (u8, Vec<u8>) {
        let opcode = stream.read_u8().await.unwrap() & 0x0f;
        let second = stream.read_u8().await.unwrap();
        assert_eq!(second & 0x80, 0);
        let length = match second & 0x7f {
            value @ 0..=125 => usize::from(value),
            126 => usize::from(stream.read_u16().await.unwrap()),
            127 => usize::try_from(stream.read_u64().await.unwrap()).unwrap(),
            _ => unreachable!(),
        };
        let mut payload = vec![0; length];
        stream.read_exact(&mut payload).await.unwrap();
        (opcode, payload)
    }

    fn envelope(kind: &str, payload: Value) -> String {
        json!({"protocol_version":1,"message_id":Uuid::new_v4(),"type":kind,"sent_at":"2026-09-02T12:00:00Z","payload":payload}).to_string()
    }

    async fn registered(address: SocketAddr, token: &str) -> (TcpStream, Value) {
        registered_player(address, token, 1).await
    }

    /// Registers a fact-publishing player and submits a full snapshot at `revision`.
    ///
    /// The snapshot is sent before any heartbeat so the reconnect handshake exercises
    /// the full-state admission gate rather than the controller-only exemption.
    async fn registered_player(
        address: SocketAddr,
        token: &str,
        revision: u64,
    ) -> (TcpStream, Value) {
        let (mut stream, response) = upgrade(address, token).await;
        assert!(response.starts_with("HTTP/1.1 101"));
        write_frame(
            &mut stream,
            1,
            envelope("protocol.hello", json!({"supported_protocol_versions":[1]})).as_bytes(),
        )
        .await;
        let (opcode, welcome) = read_frame(&mut stream).await;
        assert_eq!(opcode, 1);
        assert_eq!(
            serde_json::from_slice::<Value>(&welcome).unwrap()["type"],
            "protocol.welcome"
        );
        write_frame(
            &mut stream,
            1,
            envelope(
                "device.register",
                json!({"device_type":"rockcast","app_version":"test","manifest":{"manifest_revision":1,"roles":["player"],"capabilities":{"revision":1,"items":[]},"entities":[],"surfaces":[]}}),
            )
            .as_bytes(),
        )
        .await;
        let (_, registered) = read_frame(&mut stream).await;
        let registered: Value = serde_json::from_slice(&registered).unwrap();
        write_frame(
            &mut stream,
            1,
            envelope("device.state_full", json!({"snapshot":{"state_revision":revision,"observed_at":"2026-09-02T12:00:00Z","state":{"playback":{"status":"idle","station_id":null}}}})).as_bytes(),
        )
        .await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(registered["type"], "device.registered");
        (stream, registered)
    }

    async fn wait_until(mut predicate: impl FnMut() -> bool) {
        for _ in 0..50 {
            if predicate() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(predicate(), "condition was not met");
    }

    #[tokio::test]
    async fn authentication_happens_before_upgrade_and_registered_identity_is_server_derived() {
        let _gate = TRANSPORT_TEST_GATE
            .get_or_init(|| tokio::sync::Mutex::new(()))
            .lock()
            .await;
        let principal = ActiveSession {
            session_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            device_id: Uuid::new_v4(),
        };
        let registry = ConnectionRegistry::default();
        let resolver = resolver("native", principal);
        let (address, task) = server(app(
            resolver,
            registry.clone(),
            Duration::from_secs(1),
            Default::default(),
        ))
        .await;
        let (_, denied) = upgrade(address, "invalid").await;
        assert!(denied.starts_with("HTTP/1.1 401"));
        let (stream, response) = registered(address, "native").await;
        assert_eq!(
            response["payload"]["authenticated_user_id"],
            principal.user_id.to_string()
        );
        assert_eq!(
            response["payload"]["authenticated_device_id"],
            principal.device_id.to_string()
        );
        drop(stream);
        wait_until(|| registry.snapshot_for(principal.user_id).is_empty()).await;
        task.abort();

        let unavailable = Arc::new(FakeResolver {
            sessions: Mutex::new(HashMap::new()),
            unavailable: true,
        });
        let (address, task) = server(app(
            unavailable,
            ConnectionRegistry::default(),
            Duration::from_secs(1),
            Default::default(),
        ))
        .await;
        let (_, unavailable) = upgrade(address, "native").await;
        assert!(unavailable.starts_with("HTTP/1.1 503"));
        task.abort();
    }

    #[tokio::test]
    async fn heartbeat_reconnect_transport_loss_and_ttl_have_one_active_offline_transition() {
        let _gate = TRANSPORT_TEST_GATE
            .get_or_init(|| tokio::sync::Mutex::new(()))
            .lock()
            .await;
        let principal = ActiveSession {
            session_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            device_id: Uuid::new_v4(),
        };
        let registry = ConnectionRegistry::default();
        let resolver = resolver("native", principal);
        let (address, task) = server(app(
            resolver,
            registry.clone(),
            Duration::from_millis(70),
            Default::default(),
        ))
        .await;
        let (mut old, old_registered) = registered(address, "native").await;
        let old_id = old_registered["payload"]["connection_id"]
            .as_str()
            .unwrap()
            .to_owned();
        write_frame(
            &mut old,
            1,
            envelope("device.heartbeat", json!({"sequence":7})).as_bytes(),
        )
        .await;
        let (_, ack) = read_frame(&mut old).await;
        assert_eq!(
            serde_json::from_slice::<Value>(&ack).unwrap()["payload"]["sequence"],
            7
        );
        let (new, new_registered) = registered(address, "native").await;
        assert_ne!(
            old_id,
            new_registered["payload"]["connection_id"].as_str().unwrap()
        );
        drop(old);
        assert_eq!(registry.snapshot_for(principal.user_id).len(), 1);
        drop(new);
        wait_until(|| registry.snapshot_for(principal.user_id).is_empty()).await;
        assert_eq!(
            registry
                .events_for(principal.user_id)
                .iter()
                .filter(|event| !event.online)
                .count(),
            1
        );
        let (mut graceful, _) = registered(address, "native").await;
        write_frame(&mut graceful, 8, &[]).await;
        wait_until(|| registry.snapshot_for(principal.user_id).is_empty()).await;
        assert_eq!(
            registry
                .events_for(principal.user_id)
                .last()
                .unwrap()
                .reason,
            Some(DisconnectReason::GracefulDisconnect)
        );
        let (_ttl_stream, _) = registered(address, "native").await;
        wait_until(|| registry.snapshot_for(principal.user_id).is_empty()).await;
        assert_eq!(
            registry
                .events_for(principal.user_id)
                .last()
                .unwrap()
                .reason,
            Some(DisconnectReason::HeartbeatExpired)
        );
        task.abort();
    }

    #[tokio::test]
    async fn forward_full_state_revision_on_reconnect_keeps_the_player_online() {
        let _gate = TRANSPORT_TEST_GATE
            .get_or_init(|| tokio::sync::Mutex::new(()))
            .lock()
            .await;
        let principal = ActiveSession {
            session_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            device_id: Uuid::new_v4(),
        };
        let registry = ConnectionRegistry::default();
        let hub = StateHub::default();
        let resolver = resolver("native", principal);
        let (address, task) = server(app(
            resolver,
            registry.clone(),
            Duration::from_secs(5),
            hub.clone(),
        ))
        .await;
        let (mut first, _) = registered_player(address, "native", 1).await;
        write_frame(
            &mut first,
            1,
            envelope("device.heartbeat", json!({"sequence":1})).as_bytes(),
        )
        .await;
        let (_, ack) = read_frame(&mut first).await;
        assert_eq!(
            serde_json::from_slice::<Value>(&ack).unwrap()["type"],
            "device.heartbeat_ack"
        );
        drop(first);
        wait_until(|| registry.snapshot_for(principal.user_id).is_empty()).await;
        // While disconnected the player kept publishing facts, so its monotonic counter
        // legitimately runs ahead of the server projection. Replaying the same +1-bound
        // successor would be impossible; the full snapshot must overwrite instead.
        let (mut second, second_registered) = registered_player(address, "native", 19).await;
        assert_eq!(
            hub.device_state(principal.user_id, DeviceId(principal.device_id))
                .expect("forward snapshot overwrites the projection")
                .state_revision,
            19
        );
        write_frame(
            &mut second,
            1,
            envelope("device.heartbeat", json!({"sequence":0})).as_bytes(),
        )
        .await;
        let (opcode, ack) = read_frame(&mut second).await;
        assert_eq!(opcode, 1);
        assert_eq!(
            serde_json::from_slice::<Value>(&ack).unwrap()["type"],
            "device.heartbeat_ack"
        );
        assert_eq!(
            registry.snapshot_for(principal.user_id).len(),
            1,
            "the forward-revision connection stays the single active generation"
        );
        let _ = second_registered;
        drop(second);
        task.abort();
    }

    #[tokio::test]
    async fn wrong_first_frame_binary_and_registration_timeout_close_without_presence() {
        let _gate = TRANSPORT_TEST_GATE
            .get_or_init(|| tokio::sync::Mutex::new(()))
            .lock()
            .await;
        let principal = ActiveSession {
            session_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            device_id: Uuid::new_v4(),
        };
        let registry = ConnectionRegistry::default();
        let resolver = resolver("native", principal);
        let (address, task) = server(app(
            resolver,
            registry.clone(),
            Duration::from_secs(1),
            Default::default(),
        ))
        .await;
        let (mut wrong, _) = upgrade(address, "native").await;
        write_frame(
            &mut wrong,
            1,
            envelope("device.register", json!({})).as_bytes(),
        )
        .await;
        assert_eq!(read_frame(&mut wrong).await.0, 1);
        let (mut binary, _) = upgrade(address, "native").await;
        write_frame(&mut binary, 2, b"binary").await;
        assert_eq!(read_frame(&mut binary).await.0, 1);
        let (mut oversized, _) = upgrade(address, "native").await;
        write_frame(&mut oversized, 1, &vec![b'x'; MAX_FRAME_BYTES + 1]).await;
        let (_, error) = read_frame(&mut oversized).await;
        assert_eq!(
            serde_json::from_slice::<Value>(&error).unwrap()["payload"]["error"]["code"],
            "frame_too_large"
        );
        let (mut identity, _) = upgrade(address, "native").await;
        write_frame(
            &mut identity,
            1,
            envelope("protocol.hello", json!({"supported_protocol_versions":[1]})).as_bytes(),
        )
        .await;
        let _ = read_frame(&mut identity).await;
        write_frame(
            &mut identity,
            1,
            envelope(
                "device.register",
                json!({"device_type":"rockcast","app_version":"test","manifest":{},"device_id":Uuid::new_v4()}),
            )
            .as_bytes(),
        )
        .await;
        assert_eq!(read_frame(&mut identity).await.0, 1);
        let (_timeout, _) = upgrade(address, "native").await;
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(registry.snapshot_for(principal.user_id).is_empty());
        task.abort();
    }

    #[tokio::test]
    async fn server_shutdown_closes_registered_connections_and_cleans_presence() {
        let _gate = TRANSPORT_TEST_GATE
            .get_or_init(|| tokio::sync::Mutex::new(()))
            .lock()
            .await;
        let principal = ActiveSession {
            session_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            device_id: Uuid::new_v4(),
        };
        let registry = ConnectionRegistry::default();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(crate::serve(
            listener,
            app(
                resolver("native", principal),
                registry.clone(),
                Duration::from_secs(1),
                Default::default(),
            ),
            async move {
                let _ = shutdown_rx.await;
            },
        ));
        let (mut stream, _) = registered(address, "native").await;
        shutdown_tx.send(()).unwrap();
        assert_eq!(read_frame(&mut stream).await.0, 8);
        wait_until(|| registry.snapshot_for(principal.user_id).is_empty()).await;
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    #[test]
    fn full_state_replay_is_idempotent_and_forward_revision_resyncs() {
        let hub = StateHub::default();
        let user = Uuid::new_v4();
        let device = Uuid::new_v4();
        let snapshot = DeviceStateSnapshot {
            state_revision: 1,
            observed_at: crate::device_control::Timestamp::parse("2026-09-03T00:00:00Z").unwrap(),
            received_at: None,
            state: DeviceRuntimeState {
                playback: Some(crate::device_control::PlaybackState {
                    status: "idle".into(),
                    station_id: None,
                    track_title: None,
                }),
                volume: None,
                display: None,
            },
        };
        assert_eq!(
            accept_snapshot(&hub, user, device, snapshot.clone()),
            RevisionOrder::Next
        );
        assert_eq!(
            accept_snapshot(&hub, user, device, snapshot.clone()),
            RevisionOrder::Replay
        );
        // A device may publish revisions while disconnected; the reconnecting full
        // snapshot runs ahead of the projection and overwrites it as a resync.
        let forward = DeviceStateSnapshot {
            state_revision: 3,
            ..snapshot.clone()
        };
        assert_eq!(
            accept_snapshot(&hub, user, device, forward),
            RevisionOrder::Next
        );
        assert_eq!(
            hub.device_state(user, crate::device_control::DeviceId(device))
                .expect("forward resync is published")
                .state_revision,
            3
        );
        let stale = DeviceStateSnapshot {
            state_revision: 2,
            ..snapshot.clone()
        };
        assert_eq!(
            accept_snapshot(&hub, user, device, stale),
            RevisionOrder::Stale
        );
        assert_eq!(
            hub.device_state(user, crate::device_control::DeviceId(device))
                .expect("a stale revision never resurrects the directory projection")
                .state_revision,
            3
        );
        let conflict = DeviceStateSnapshot {
            state_revision: 3,
            state: DeviceRuntimeState {
                playback: Some(crate::device_control::PlaybackState {
                    status: "playing".into(),
                    station_id: None,
                    track_title: None,
                }),
                volume: None,
                display: None,
            },
            ..snapshot
        };
        assert_eq!(
            accept_snapshot(&hub, user, device, conflict),
            RevisionOrder::Conflict
        );
        assert_eq!(
            hub.device_state(user, crate::device_control::DeviceId(device))
                .expect("a conflicting revision never mutates the directory projection")
                .state_revision,
            3
        );
    }
}
