//! Focused regression tests for this private domain facade.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use time::OffsetDateTime;
use tokio::sync::mpsc;
use uuid::Uuid;

use super::*;
use crate::{
    device_control::{
        CommandAccepted, CommandBody, CommandId, CommandReservation, CommandResult, CommandStatus,
        DeviceCapabilities, DeviceCapability, DeviceCommand, DeviceControlScope,
        DeviceControlStore, DeviceId, DeviceManifest, DeviceRole, DeviceStateSnapshot, Entity,
        EntityState, StoreError, StoreOutcome, StreamSource, Surface,
    },
    device_control_presence::{ConnectionRegistry, OutboundFrame},
    search::RepositoryError,
};

type StoredCommand = (Uuid, DeviceId, CommandReservation, Option<CommandResult>);

/// Deterministic station catalog fake; stream URLs are inert test placeholders only.
struct FakeCatalog {
    stations: HashMap<String, ResolvedStation>,
    unavailable: bool,
}

impl FakeCatalog {
    fn with_stations(stations: &[(&str, &str, &str)]) -> Arc<Self> {
        Arc::new(Self {
            stations: stations
                .iter()
                .map(|(id, name, url)| {
                    (
                        (*id).to_owned(),
                        ResolvedStation {
                            stream_uri: (*url).to_owned(),
                            presentation: crate::device_control::StationPresentation {
                                name: (*name).to_owned(),
                                icon_url: None,
                            },
                        },
                    )
                })
                .collect(),
            unavailable: false,
        })
    }
}

#[async_trait]
impl StationCatalog for FakeCatalog {
    async fn station(&self, station_id: &str) -> Result<Option<ResolvedStation>, RepositoryError> {
        if self.unavailable {
            return Err(RepositoryError::new(
                "station catalog fixture",
                std::io::Error::other("fixture catalog failure"),
            ));
        }
        Ok(self.stations.get(station_id).cloned())
    }
}

#[derive(Default)]
struct MemoryStore {
    commands: Mutex<HashMap<CommandId, StoredCommand>>,
    manifests: Mutex<HashMap<(Uuid, DeviceId), DeviceManifest>>,
}

#[async_trait]
impl DeviceControlStore for MemoryStore {
    async fn apply_manifest(
        &self,
        user: Uuid,
        device: DeviceId,
        manifest: DeviceManifest,
    ) -> Result<StoreOutcome, StoreError> {
        self.manifests
            .lock()
            .expect("test mutex")
            .insert((user, device), manifest);
        Ok(StoreOutcome::Accepted)
    }
    async fn load_manifest(
        &self,
        user: Uuid,
        device: DeviceId,
    ) -> Result<Option<DeviceManifest>, StoreError> {
        Ok(self
            .manifests
            .lock()
            .expect("test mutex")
            .get(&(user, device))
            .cloned())
    }
    async fn list_entities(&self, _: Uuid, _: DeviceId) -> Result<Vec<Entity>, StoreError> {
        Ok(Vec::new())
    }
    async fn list_surfaces(&self, _: Uuid, _: DeviceId) -> Result<Vec<Surface>, StoreError> {
        Ok(Vec::new())
    }
    async fn list_capabilities(
        &self,
        _: Uuid,
        _: DeviceId,
    ) -> Result<Vec<DeviceCapability>, StoreError> {
        Ok(Vec::new())
    }
    async fn store_device_state(
        &self,
        _: Uuid,
        _: DeviceId,
        _: DeviceStateSnapshot,
    ) -> Result<StoreOutcome, StoreError> {
        Ok(StoreOutcome::Accepted)
    }
    async fn load_device_state(
        &self,
        _: Uuid,
        _: DeviceId,
    ) -> Result<Option<DeviceStateSnapshot>, StoreError> {
        Ok(None)
    }
    async fn store_entity_state(
        &self,
        _: Uuid,
        _: DeviceId,
        _: EntityState,
    ) -> Result<StoreOutcome, StoreError> {
        Ok(StoreOutcome::Accepted)
    }
    async fn load_entity_state(
        &self,
        _: Uuid,
        _: DeviceId,
        _: &str,
    ) -> Result<Option<EntityState>, StoreError> {
        Ok(None)
    }
    async fn reserve_command(
        &self,
        user: Uuid,
        device: DeviceId,
        request: CommandReservation,
    ) -> Result<StoreOutcome, StoreError> {
        let mut commands = self.commands.lock().expect("test mutex");
        match commands.get(&request.command.command_id) {
            Some((stored_user, stored_device, stored, _))
                if *stored_user == user
                    && *stored_device == device
                    && stored.fingerprint == request.fingerprint =>
            {
                Ok(StoreOutcome::Replay)
            }
            Some(_) => Ok(StoreOutcome::Conflict),
            None => {
                commands.insert(request.command.command_id, (user, device, request, None));
                Ok(StoreOutcome::Accepted)
            }
        }
    }
    async fn load_command(
        &self,
        user: Uuid,
        device: DeviceId,
        command_id: CommandId,
    ) -> Result<Option<crate::device_control::CommandLifecycle>, StoreError> {
        Ok(self
            .commands
            .lock()
            .expect("test mutex")
            .get(&command_id)
            .filter(|(stored_user, stored_device, _, _)| {
                *stored_user == user && *stored_device == device
            })
            .map(
                |(_, _, request, result)| crate::device_control::CommandLifecycle {
                    command: request.command.clone(),
                    result: result.clone(),
                },
            ))
    }
    async fn complete_command(
        &self,
        user: Uuid,
        device: DeviceId,
        result: CommandResult,
    ) -> Result<StoreOutcome, StoreError> {
        let mut commands = self.commands.lock().expect("test mutex");
        let Some((stored_user, stored_device, _, prior)) = commands.get_mut(&result.command_id)
        else {
            return Ok(StoreOutcome::NotOwned);
        };
        if *stored_user != user || *stored_device != device {
            return Ok(StoreOutcome::NotOwned);
        }
        if let Some(prior) = prior {
            return Ok(if *prior == result {
                StoreOutcome::Replay
            } else {
                StoreOutcome::Conflict
            });
        }
        *prior = Some(result);
        Ok(StoreOutcome::Accepted)
    }
    async fn prune_commands(&self, _: u32) -> Result<u64, StoreError> {
        Ok(0)
    }
}

fn manifest(roles: Vec<DeviceRole>, capabilities: Vec<DeviceCapability>) -> DeviceManifest {
    DeviceManifest {
        manifest_revision: 1,
        roles,
        capabilities: DeviceCapabilities {
            revision: 1,
            items: capabilities,
        },
        entities: Vec::new(),
        surfaces: Vec::new(),
    }
}

fn register(
    registry: &ConnectionRegistry,
    owner: Uuid,
    device: Uuid,
    manifest: DeviceManifest,
    scopes: Vec<DeviceControlScope>,
) -> (Uuid, mpsc::Receiver<OutboundFrame>) {
    let connection = Uuid::new_v4();
    let (replacement, _) = registry.replacement_channel();
    let (outbound, receiver) = registry.outbound_channel();
    registry.register(
        crate::device_control_presence::ConnectionRegistration {
            user_id: owner,
            device_id: device,
            connection_id: connection,
            replacement,
            outbound,
            manifest,
            scopes,
        },
        std::time::Instant::now(),
    );
    (connection, receiver)
}

fn command(target: Uuid) -> DeviceCommand {
    DeviceCommand {
        command_id: CommandId(Uuid::new_v4()),
        target: crate::device_control::CommandTarget {
            device_id: DeviceId(target),
            entity_id: None,
            surface_id: None,
        },
        deadline_at: None,
        body: CommandBody::PlayStation {
            station_id: "calm-jazz".into(),
        },
    }
}

/// Router with the canonical resolved calm-jazz stream available for resolution.
fn station_router() -> CommandRouter {
    CommandRouter::default().with_station_catalog(FakeCatalog::with_stations(&[(
        "calm-jazz",
        "Calm Jazz",
        "https://streams.example.com/calm-jazz.mp3",
    )]))
}

#[tokio::test]
async fn command_is_delivered_once_and_only_terminal_target_result_completes_it() {
    let registry = ConnectionRegistry::default();
    let router = station_router();
    let store: Arc<dyn DeviceControlStore> = Arc::new(MemoryStore::default());
    let owner = Uuid::new_v4();
    let controller = Uuid::new_v4();
    let target = Uuid::new_v4();
    let (controller_connection, mut controller_messages) = register(
        &registry,
        owner,
        controller,
        manifest(vec![DeviceRole::Controller], Vec::new()),
        vec![DeviceControlScope::MediaControl],
    );
    let (target_connection, mut target_messages) = register(
        &registry,
        owner,
        target,
        manifest(
            vec![DeviceRole::Player],
            vec![DeviceCapability::Station {
                sources: vec![crate::device_control::CATALOG_STATION_SOURCE.into()],
            }],
        ),
        Vec::new(),
    );
    store
        .apply_manifest(
            owner,
            DeviceId(target),
            registry.active_for(owner, target).unwrap().manifest,
        )
        .await
        .unwrap();
    let command = command(target);
    router
        .submit(
            &registry,
            Some(&store),
            owner,
            controller,
            controller_connection,
            Uuid::new_v4().to_string(),
            command.clone(),
        )
        .await
        .unwrap();
    assert_eq!(
        controller_messages.recv().await.unwrap().kind,
        "command.received"
    );
    // The target — never the controller — receives the server-resolved play_stream body
    // under the original command_id, with the resolved station echoed.
    let delivered = target_messages.recv().await.unwrap();
    assert_eq!(delivered.kind, "device.command");
    assert_eq!(
        delivered.payload["command_id"],
        serde_json::to_value(command.command_id).unwrap()
    );
    assert_eq!(delivered.payload["body"]["name"], "station.play_stream");
    assert_eq!(
        delivered.payload["body"]["source"],
        crate::device_control::CATALOG_STATION_SOURCE
    );
    assert_eq!(delivered.payload["body"]["station_id"], "calm-jazz");
    assert_eq!(
        delivered.payload["body"]["station"],
        serde_json::json!({"name":"Calm Jazz","icon_url":null})
    );
    assert_eq!(
        delivered.payload["body"]["stream_uri"],
        "https://streams.example.com/calm-jazz.mp3"
    );
    router
        .accepted(
            &registry,
            owner,
            target,
            target_connection,
            CommandAccepted {
                command_id: command.command_id,
                accepted_at: timestamp(OffsetDateTime::now_utc()),
            },
        )
        .unwrap();
    assert_eq!(
        controller_messages.recv().await.unwrap().kind,
        "command.accepted"
    );
    router
        .result(
            &registry,
            Some(&store),
            owner,
            target,
            target_connection,
            CommandResult {
                command_id: command.command_id,
                status: CommandStatus::Succeeded,
                completed_at: timestamp(OffsetDateTime::now_utc()),
                error: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        controller_messages.recv().await.unwrap().kind,
        "command.result"
    );
    router
        .result(
            &registry,
            Some(&store),
            owner,
            target,
            target_connection,
            CommandResult {
                command_id: command.command_id,
                status: CommandStatus::Succeeded,
                completed_at: timestamp(OffsetDateTime::now_utc()),
                error: None,
            },
        )
        .await
        .unwrap_err();
    router
        .submit(
            &registry,
            Some(&store),
            owner,
            controller,
            controller_connection,
            Uuid::new_v4().to_string(),
            command,
        )
        .await
        .unwrap();
    assert_eq!(
        controller_messages.recv().await.unwrap().kind,
        "command.received"
    );
    assert_eq!(
        controller_messages.recv().await.unwrap().kind,
        "command.result"
    );
    assert!(target_messages.try_recv().is_err());
}

#[tokio::test]
async fn spoofed_or_offline_targets_are_never_delivered() {
    let registry = ConnectionRegistry::default();
    let router = station_router();
    let store: Arc<dyn DeviceControlStore> = Arc::new(MemoryStore::default());
    let owner = Uuid::new_v4();
    let controller = Uuid::new_v4();
    let target = Uuid::new_v4();
    let (controller_connection, mut controller_messages) = register(
        &registry,
        owner,
        controller,
        manifest(vec![DeviceRole::Controller], Vec::new()),
        vec![DeviceControlScope::MediaControl],
    );
    let (target_connection, mut target_messages) = register(
        &registry,
        owner,
        target,
        manifest(
            vec![DeviceRole::Player],
            vec![DeviceCapability::Station {
                sources: vec![crate::device_control::CATALOG_STATION_SOURCE.into()],
            }],
        ),
        Vec::new(),
    );
    store
        .apply_manifest(
            owner,
            DeviceId(target),
            registry.active_for(owner, target).unwrap().manifest,
        )
        .await
        .unwrap();
    let sent = command(target);
    router
        .submit(
            &registry,
            Some(&store),
            owner,
            controller,
            controller_connection,
            Uuid::new_v4().to_string(),
            sent.clone(),
        )
        .await
        .unwrap();
    let _ = controller_messages.recv().await;
    let _ = target_messages.recv().await;
    assert_eq!(
        router
            .accepted(
                &registry,
                owner,
                Uuid::new_v4(),
                target_connection,
                CommandAccepted {
                    command_id: sent.command_id,
                    accepted_at: timestamp(OffsetDateTime::now_utc())
                }
            )
            .unwrap_err()
            .code,
        "forbidden"
    );
    registry.disconnect(
        target,
        target_connection,
        crate::device_control_presence::DisconnectReason::TransportLost,
        std::time::Instant::now(),
    );
    router
        .disconnected(&registry, Some(&store), owner, target, target_connection)
        .await;
    assert_eq!(
        router
            .submit(
                &registry,
                Some(&store),
                owner,
                controller,
                controller_connection,
                Uuid::new_v4().to_string(),
                command(target)
            )
            .await
            .unwrap_err()
            .code,
        "target_offline"
    );
}

/// Registers a controller and a player that advertises exactly the given station sources.
///
/// Returns the controller device, its connection and frames, then the target connection,
/// target frames, and the target device.
#[allow(clippy::type_complexity)]
async fn registered_pair(
    registry: &ConnectionRegistry,
    owner: Uuid,
    sources: Vec<String>,
) -> (
    Uuid,
    Uuid,
    mpsc::Receiver<OutboundFrame>,
    Uuid,
    mpsc::Receiver<OutboundFrame>,
    Uuid,
) {
    let controller = Uuid::new_v4();
    let target = Uuid::new_v4();
    let (controller_connection, controller_messages) = register(
        registry,
        owner,
        controller,
        manifest(vec![DeviceRole::Controller], Vec::new()),
        vec![DeviceControlScope::MediaControl],
    );
    let (target_connection, target_messages) = register(
        registry,
        owner,
        target,
        manifest(
            vec![DeviceRole::Player],
            vec![DeviceCapability::Station { sources }],
        ),
        Vec::new(),
    );
    (
        controller,
        controller_connection,
        controller_messages,
        target_connection,
        target_messages,
        target,
    )
}

/// Reads one controller frame and returns its serialized payload for leak assertions.
async fn controller_frame(receiver: &mut mpsc::Receiver<OutboundFrame>) -> String {
    let frame = receiver.recv().await.expect("controller frame must arrive");
    serde_json::to_string(&frame.payload).expect("controller payload serializes")
}

#[tokio::test]
async fn deterministic_resolution_failures_terminate_without_dispatch_or_uri_leak() {
    let registry = ConnectionRegistry::default();
    let router = CommandRouter::default().with_station_catalog(FakeCatalog::with_stations(&[
        ("station-empty-001", "Empty", ""),
        ("station-broken-002", "Broken", "not a valid url"),
        (
            "station-private-003",
            "Private",
            "https://192.0.2.10/never-echo-this.mp3",
        ),
        (
            "station-loopback-004",
            "Loopback",
            "http://127.0.0.1:8000/live",
        ),
    ]));
    let store: Arc<dyn DeviceControlStore> = Arc::new(MemoryStore::default());
    let owner = Uuid::new_v4();
    let (
        controller,
        controller_connection,
        mut controller_messages,
        _connection,
        mut target_messages,
        target,
    ) = registered_pair(
        &registry,
        owner,
        vec![crate::device_control::CATALOG_STATION_SOURCE.into()],
    )
    .await;
    store
        .apply_manifest(
            owner,
            DeviceId(target),
            registry.active_for(owner, target).unwrap().manifest,
        )
        .await
        .unwrap();

    for station_id in [
        "station-unknown-000",
        "station-empty-001",
        "station-broken-002",
        "station-private-003",
        "station-loopback-004",
    ] {
        let mut sent = command(target);
        sent.body = CommandBody::PlayStation {
            station_id: station_id.into(),
        };
        router
            .submit(
                &registry,
                Some(&store),
                owner,
                controller,
                controller_connection,
                Uuid::new_v4().to_string(),
                sent.clone(),
            )
            .await
            .unwrap();
        let received = controller_frame(&mut controller_messages).await;
        assert!(received.contains("command"), "receipt precedes the failure");
        let result = controller_frame(&mut controller_messages).await;
        assert!(result.contains("\"failed\""), "{station_id} must fail");
        assert!(
            result.contains("invalid_payload"),
            "{station_id} is deterministic"
        );
        assert!(
            !result.contains("stream_uri") && !result.contains("never-echo-this"),
            "terminal results never carry a stream URI"
        );
        assert!(
            target_messages.try_recv().is_err(),
            "an unresolvable station is never dispatched"
        );

        // The same controller request replays the stored failure without re-resolution.
        router
            .submit(
                &registry,
                Some(&store),
                owner,
                controller,
                controller_connection,
                Uuid::new_v4().to_string(),
                sent,
            )
            .await
            .unwrap();
        let replay = controller_frame(&mut controller_messages).await;
        assert!(
            replay.contains("\"duplicate\":true"),
            "{station_id} replays"
        );
        assert!(
            controller_frame(&mut controller_messages)
                .await
                .contains("invalid_payload")
        );
        assert!(
            target_messages.try_recv().is_err(),
            "replays never re-dispatch"
        );
    }
}

#[tokio::test]
async fn transient_catalog_failure_completes_with_retryable_terminal_result() {
    let registry = ConnectionRegistry::default();
    let router = CommandRouter::default().with_station_catalog(Arc::new(FakeCatalog {
        stations: HashMap::new(),
        unavailable: true,
    }));
    let store: Arc<dyn DeviceControlStore> = Arc::new(MemoryStore::default());
    let owner = Uuid::new_v4();
    let (
        controller,
        controller_connection,
        mut controller_messages,
        _connection,
        mut target_messages,
        target,
    ) = registered_pair(
        &registry,
        owner,
        vec![crate::device_control::CATALOG_STATION_SOURCE.into()],
    )
    .await;
    store
        .apply_manifest(
            owner,
            DeviceId(target),
            registry.active_for(owner, target).unwrap().manifest,
        )
        .await
        .unwrap();
    let request_id = "rs-5-terminal-request".to_owned();
    let sent = command(target);
    router
        .submit(
            &registry,
            Some(&store),
            owner,
            controller,
            controller_connection,
            request_id.clone(),
            sent.clone(),
        )
        .await
        .unwrap();
    assert!(
        controller_frame(&mut controller_messages)
            .await
            .contains("command")
    );
    let result = controller_frame(&mut controller_messages).await;
    assert!(result.contains("persistence_unavailable"));
    let result: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        result["command_id"],
        serde_json::to_value(sent.command_id).unwrap()
    );
    assert_eq!(result["error"]["request_id"], request_id);
    assert_eq!(result["error"]["details"], serde_json::json!({}));
    assert!(target_messages.try_recv().is_err(), "nothing is dispatched");
}

#[tokio::test]
async fn controller_supplied_play_stream_variants_are_gated() {
    let registry = ConnectionRegistry::default();
    let router = station_router();
    let store: Arc<dyn DeviceControlStore> = Arc::new(MemoryStore::default());
    let owner = Uuid::new_v4();
    let (
        controller,
        controller_connection,
        mut controller_messages,
        _connection,
        mut target_messages,
        target,
    ) = registered_pair(
        &registry,
        owner,
        vec![
            crate::device_control::CATALOG_STATION_SOURCE.into(),
            crate::device_control::DIRECT_STATION_SOURCE.into(),
        ],
    )
    .await;
    store
        .apply_manifest(
            owner,
            DeviceId(target),
            registry.active_for(owner, target).unwrap().manifest,
        )
        .await
        .unwrap();

    // The server-resolved variant is router-only: a controller sending it is rejected outright.
    let mut spoofed = command(target);
    spoofed.body = CommandBody::PlayStream {
        source: StreamSource::RockserverCatalog,
        station_id: Some("calm-jazz".into()),
        station: Some(crate::device_control::StationPresentation {
            name: "Spoofed".into(),
            icon_url: None,
        }),
        stream_uri: "https://streams.example.com/spoofed.mp3".into(),
    };
    assert_eq!(
        router
            .submit(
                &registry,
                Some(&store),
                owner,
                controller,
                controller_connection,
                Uuid::new_v4().to_string(),
                spoofed,
            )
            .await
            .unwrap_err()
            .code,
        "invalid_payload"
    );
    assert!(controller_messages.try_recv().is_err());
    assert!(target_messages.try_recv().is_err());

    // A forbidden direct destination is rejected before any lifecycle exists.
    let mut forbidden = command(target);
    forbidden.body = CommandBody::PlayStream {
        source: StreamSource::DirectStream,
        station_id: None,
        station: None,
        stream_uri: "https://10.0.0.5/private.mp3".into(),
    };
    assert_eq!(
        router
            .submit(
                &registry,
                Some(&store),
                owner,
                controller,
                controller_connection,
                Uuid::new_v4().to_string(),
                forbidden,
            )
            .await
            .unwrap_err()
            .code,
        "invalid_payload"
    );

    // A well-formed direct stream is dispatched unchanged to the advertising target.
    let mut direct = command(target);
    direct.body = CommandBody::PlayStream {
        source: StreamSource::DirectStream,
        station_id: None,
        station: None,
        stream_uri: "https://streams.example.com/direct.mp3".into(),
    };
    router
        .submit(
            &registry,
            Some(&store),
            owner,
            controller,
            controller_connection,
            Uuid::new_v4().to_string(),
            direct.clone(),
        )
        .await
        .unwrap();
    assert!(
        controller_frame(&mut controller_messages)
            .await
            .contains("command")
    );
    let delivered = target_messages
        .recv()
        .await
        .expect("direct stream is dispatched");
    assert_eq!(
        delivered.payload["command_id"],
        serde_json::to_value(direct.command_id).unwrap()
    );
    assert_eq!(
        delivered.payload["body"]["source"],
        crate::device_control::DIRECT_STATION_SOURCE
    );
    assert!(delivered.payload["body"].get("station_id").is_none());
}

#[tokio::test]
async fn play_station_requires_catalog_wiring_and_a_catalog_source_target() {
    let registry = ConnectionRegistry::default();
    let router = station_router();
    let store: Arc<dyn DeviceControlStore> = Arc::new(MemoryStore::default());
    let owner = Uuid::new_v4();
    let (controller, controller_connection, _messages, _connection, _target_messages, direct_only) =
        registered_pair(
            &registry,
            owner,
            vec![crate::device_control::DIRECT_STATION_SOURCE.into()],
        )
        .await;
    let (_controller, _connection, _messages, _connection2, _target_messages, catalog_target) =
        registered_pair(
            &registry,
            owner,
            vec![crate::device_control::CATALOG_STATION_SOURCE.into()],
        )
        .await;
    store
        .apply_manifest(
            owner,
            DeviceId(direct_only),
            registry.active_for(owner, direct_only).unwrap().manifest,
        )
        .await
        .unwrap();
    // A direct-stream-only player cannot receive catalog-resolved playback.
    assert_eq!(
        router
            .submit(
                &registry,
                Some(&store),
                owner,
                controller,
                controller_connection,
                Uuid::new_v4().to_string(),
                command(direct_only),
            )
            .await
            .unwrap_err()
            .code,
        "capability_not_supported"
    );
    // A router without the catalog boundary stays retryable without creating lifecycle.
    assert_eq!(
        CommandRouter::default()
            .submit(
                &registry,
                Some(&store),
                owner,
                controller,
                controller_connection,
                Uuid::new_v4().to_string(),
                command(catalog_target),
            )
            .await
            .unwrap_err()
            .code,
        "persistence_unavailable"
    );
}
