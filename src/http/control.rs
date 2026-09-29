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
        DeviceManifest, DeviceStateSnapshot, RevisionOrder, revision_order,
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
#[cfg(test)]
#[path = "control/tests.rs"]
mod tests;

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
