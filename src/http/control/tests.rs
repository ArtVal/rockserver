use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use axum::Router;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use uuid::Uuid;

use super::*;
use crate::{
    auth::{ActiveSession, NativeSessionLookupError, NativeSessionResolver, SecretHash},
    device_control::{DeviceRole, DeviceRuntimeState, DeviceStateSnapshot},
    http::endpoints::{RouterBuilder, state::ControlTiming},
    search::{InMemoryStationRepository, SearchService},
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
    RouterBuilder::default()
        .with_search_service(search_service)
        .with_api_bearer_token("unrelated")
        .with_control_registry(registry)
        .with_control_state_hub(hub)
        .with_control_session_resolver(resolver)
        .with_control_timing(ControlTiming {
            registration_deadline: Duration::from_millis(100),
            offline_ttl: ttl,
        })
        .build()
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
    stream
        .write_all(
            format!(
                "GET /api/v1/devices/connect HTTP/1.1\r\nHost: {address}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nAuthorization: Bearer {token}\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
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
async fn registered_player(address: SocketAddr, token: &str, revision: u64) -> (TcpStream, Value) {
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
