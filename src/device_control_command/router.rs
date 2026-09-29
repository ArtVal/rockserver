//! Command routing, lifecycle tracking, idempotency, and timeouts.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use sha2::{Digest, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

use crate::{
    device_control::{
        CommandAccepted, CommandBody, CommandId, CommandReceived, CommandReservation,
        CommandResult, CommandStatus, DeviceCommand, DeviceControlScope, DeviceControlStore,
        DeviceId, DeviceRole, DomainError, StoreOutcome, StreamSource, Timestamp,
    },
    device_control_presence::{ConnectionRegistry, OutboundFrame},
};

use super::{
    catalog::{ResolutionFailure, StationCatalog, resolve_station},
    target_validation::validate_target,
};

const MAX_CONNECTION_IN_FLIGHT: usize = 16;
const MAX_TARGET_IN_FLIGHT: usize = 8;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_TIMEOUT: Duration = Duration::from_secs(30);

/// Safe protocol outcome sent only to the originating active controller connection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandError {
    /// Stable non-enumerating error code.
    pub code: &'static str,
}

#[derive(Clone)]
struct InFlight {
    owner_id: Uuid,
    controller_device_id: Uuid,
    controller_connection_id: Uuid,
    target_device_id: DeviceId,
    target_connection_id: Uuid,
    request_id: String,
}

/// Server-side command router shared by all authenticated control WebSocket sessions.
#[derive(Clone, Default)]
pub struct CommandRouter {
    state: Arc<Mutex<HashMap<CommandId, InFlight>>>,
    catalog: Option<Arc<dyn StationCatalog>>,
}

impl CommandRouter {
    /// Attaches the station catalog used to resolve `station.play_station` commands.
    ///
    /// A router without a catalog rejects station commands with `persistence_unavailable`
    /// before any lifecycle record is created.
    pub fn with_station_catalog(mut self, catalog: Arc<dyn StationCatalog>) -> Self {
        self.catalog = Some(catalog);
        self
    }

    /// Admits one command after authentication, owner, scope, manifest and capacity checks.
    #[allow(clippy::too_many_arguments)]
    pub async fn submit(
        &self,
        registry: &ConnectionRegistry,
        store: Option<&Arc<dyn DeviceControlStore>>,
        owner_id: Uuid,
        controller_device_id: Uuid,
        controller_connection_id: Uuid,
        request_id: String,
        mut command: DeviceCommand,
    ) -> Result<(), CommandError> {
        let now = OffsetDateTime::now_utc();
        let received_at = timestamp(now);
        let controller = registry
            .active_for(owner_id, controller_device_id)
            .filter(|active| active.connection_id == controller_connection_id)
            .ok_or(CommandError { code: "forbidden" })?;
        if !controller.manifest.roles.contains(&DeviceRole::Controller) {
            return Err(CommandError { code: "forbidden" });
        }
        command
            .validate_at(&received_at)
            .map_err(validation_error)?;
        command.executable().map_err(validation_error)?;
        // Only the router itself may originate the server-resolved stream variant; the frozen
        // contract rejects a controller-supplied source=rockserver_catalog as invalid_payload.
        if matches!(
            &command.body,
            CommandBody::PlayStream {
                source: StreamSource::RockserverCatalog,
                ..
            }
        ) {
            return Err(CommandError {
                code: "invalid_payload",
            });
        }
        let required_scope = required_scope(&command.body);
        if !controller.scopes.contains(&required_scope) {
            return Err(CommandError { code: "forbidden" });
        }
        let Some(store) = store else {
            return Err(CommandError {
                code: "persistence_unavailable",
            });
        };
        let target = match registry.active_for(owner_id, command.target.device_id.0) {
            Some(target) => target,
            None => match store
                .load_manifest(owner_id, command.target.device_id)
                .await
            {
                Ok(Some(_)) => {
                    return Err(CommandError {
                        code: "target_offline",
                    });
                }
                Ok(None) => return Err(CommandError { code: "forbidden" }),
                Err(_) => {
                    return Err(CommandError {
                        code: "persistence_unavailable",
                    });
                }
            },
        };
        validate_target(&command, &target.manifest)?;
        // Station resolution needs the catalog boundary; fail before any lifecycle record
        // exists so a misconfigured router stays retryable without durable noise.
        if matches!(command.body, CommandBody::PlayStation { .. }) && self.catalog.is_none() {
            return Err(CommandError {
                code: "persistence_unavailable",
            });
        }
        // The fingerprint represents the client-visible validated request. The server-selected
        // default deadline is intentionally excluded so a retry with the same command ID replays.
        let fingerprint = fingerprint(&command).map_err(|_| CommandError {
            code: "invalid_payload",
        })?;
        let deadline = command
            .deadline_at
            .as_ref()
            .map(Timestamp::instant)
            .unwrap_or(now + time::Duration::seconds(DEFAULT_TIMEOUT.as_secs() as i64));
        if deadline <= now || deadline > now + time::Duration::seconds(MAX_TIMEOUT.as_secs() as i64)
        {
            return Err(CommandError {
                code: "command_timeout",
            });
        }
        command.deadline_at = Some(timestamp(deadline));
        let reservation = CommandReservation {
            command: command.clone(),
            fingerprint,
            deadline_at: command
                .deadline_at
                .clone()
                .expect("command deadline assigned"),
        };
        let reserve = store
            .reserve_command(owner_id, command.target.device_id, reservation)
            .await
            .map_err(|_| CommandError {
                code: "persistence_unavailable",
            })?;
        if reserve == StoreOutcome::Conflict {
            return Err(CommandError {
                code: "duplicate_command",
            });
        }
        if reserve != StoreOutcome::Accepted && reserve != StoreOutcome::Replay {
            return Err(CommandError { code: "forbidden" });
        }
        if reserve == StoreOutcome::Replay {
            replay(
                store.as_ref(),
                registry,
                owner_id,
                controller_device_id,
                controller_connection_id,
                command.target.device_id,
                command.command_id,
            )
            .await?;
            return Ok(());
        }
        let over_capacity = {
            let state = self
                .state
                .lock()
                .expect("command router mutex is not poisoned");
            state
                .values()
                .filter(|item| item.controller_connection_id == controller_connection_id)
                .count()
                >= MAX_CONNECTION_IN_FLIGHT
                || state
                    .values()
                    .filter(|item| item.target_device_id == command.target.device_id)
                    .count()
                    >= MAX_TARGET_IN_FLIGHT
        };
        if over_capacity {
            let _ = complete(
                store.as_ref(),
                owner_id,
                command.target.device_id,
                terminal(&command.command_id, &request_id, "too_many_in_flight"),
            )
            .await;
            return Err(CommandError {
                code: "too_many_in_flight",
            });
        }
        let in_flight = InFlight {
            owner_id,
            controller_device_id,
            controller_connection_id,
            target_device_id: command.target.device_id,
            target_connection_id: target.connection_id,
            request_id: request_id.clone(),
        };
        self.state
            .lock()
            .expect("command router mutex is not poisoned")
            .insert(command.command_id, in_flight);
        send(
            registry,
            owner_id,
            controller_device_id,
            controller_connection_id,
            "command.received",
            &CommandReceived {
                command_id: command.command_id,
                received_at,
                duplicate: false,
            },
        )?;
        // Server-side station resolution: the controller-visible lifecycle and idempotency
        // fingerprint were established above from the original play_station command, and the
        // resolved play_stream body replaces it only for the dispatch to the target. The
        // stream URI therefore never enters persistence, logs, or controller frames.
        if let CommandBody::PlayStation { station_id } = command.body.clone() {
            let catalog = match self.catalog.clone() {
                Some(catalog) => catalog,
                None => {
                    self.finish(
                        registry,
                        store.as_ref(),
                        command.command_id,
                        terminal(&command.command_id, &request_id, "persistence_unavailable"),
                    )
                    .await;
                    return Ok(());
                }
            };
            match resolve_station(catalog.as_ref(), &station_id).await {
                Ok(station) => {
                    command.body = CommandBody::PlayStream {
                        source: StreamSource::RockserverCatalog,
                        station_id: Some(station_id),
                        station: Some(station.presentation),
                        stream_uri: station.stream_uri,
                    };
                }
                Err(failure) => {
                    let (code, message) = match failure {
                        ResolutionFailure::Invalid(message) => ("invalid_payload", message),
                        ResolutionFailure::Unavailable => {
                            ("persistence_unavailable", "Station catalog is unavailable.")
                        }
                    };
                    self.finish(
                        registry,
                        store.as_ref(),
                        command.command_id,
                        terminal_message(&command.command_id, &request_id, code, message),
                    )
                    .await;
                    return Ok(());
                }
            }
        }
        if send(
            registry,
            owner_id,
            command.target.device_id.0,
            target.connection_id,
            "device.command",
            &command,
        )
        .is_err()
        {
            self.finish(
                registry,
                store.as_ref(),
                command.command_id,
                terminal(&command.command_id, &request_id, "target_offline"),
            )
            .await;
            return Ok(());
        }
        let router = self.clone();
        let registry = registry.clone();
        let store = Arc::clone(store);
        tokio::spawn(async move {
            let remaining = std::time::Duration::try_from(deadline - OffsetDateTime::now_utc())
                .unwrap_or_default();
            tokio::time::sleep_until(tokio::time::Instant::from_std(
                std::time::Instant::now() + remaining,
            ))
            .await;
            router
                .finish(
                    &registry,
                    store.as_ref(),
                    command.command_id,
                    terminal(&command.command_id, &request_id, "command_timeout"),
                )
                .await;
        });
        Ok(())
    }

    /// Records a target acknowledgement only for the exact authenticated delivery generation.
    pub fn accepted(
        &self,
        registry: &ConnectionRegistry,
        owner_id: Uuid,
        device_id: Uuid,
        connection_id: Uuid,
        mut accepted: CommandAccepted,
    ) -> Result<(), CommandError> {
        let state = self
            .state
            .lock()
            .expect("command router mutex is not poisoned");
        let item = state.get(&accepted.command_id).ok_or(CommandError {
            code: "duplicate_command",
        })?;
        if item.owner_id != owner_id
            || item.target_device_id.0 != device_id
            || item.target_connection_id != connection_id
        {
            return Err(CommandError { code: "forbidden" });
        }
        accepted.accepted_at = timestamp(OffsetDateTime::now_utc());
        send(
            registry,
            owner_id,
            item.controller_device_id,
            item.controller_connection_id,
            "command.accepted",
            &accepted,
        )
    }

    /// Persists and forwards exactly one valid target terminal result.
    pub async fn result(
        &self,
        registry: &ConnectionRegistry,
        store: Option<&Arc<dyn DeviceControlStore>>,
        owner_id: Uuid,
        device_id: Uuid,
        connection_id: Uuid,
        mut result: CommandResult,
    ) -> Result<(), CommandError> {
        result.validate().map_err(validation_error)?;
        let item = self
            .state
            .lock()
            .expect("command router mutex is not poisoned")
            .get(&result.command_id)
            .cloned()
            .ok_or(CommandError {
                code: "duplicate_command",
            })?;
        if item.owner_id != owner_id
            || item.target_device_id.0 != device_id
            || item.target_connection_id != connection_id
        {
            return Err(CommandError { code: "forbidden" });
        }
        result.completed_at = timestamp(OffsetDateTime::now_utc());
        let Some(store) = store else {
            return Err(CommandError {
                code: "persistence_unavailable",
            });
        };
        self.finish(registry, store.as_ref(), result.command_id, result)
            .await;
        Ok(())
    }

    /// Fails only commands delivered to a disconnected exact generation; controller loss does not reroute work.
    pub async fn disconnected(
        &self,
        registry: &ConnectionRegistry,
        store: Option<&Arc<dyn DeviceControlStore>>,
        owner_id: Uuid,
        device_id: Uuid,
        connection_id: Uuid,
    ) {
        let Some(store) = store else {
            return;
        };
        let commands: Vec<_> = self
            .state
            .lock()
            .expect("command router mutex is not poisoned")
            .iter()
            .filter_map(|(command_id, item)| {
                (item.owner_id == owner_id
                    && item.target_device_id.0 == device_id
                    && item.target_connection_id == connection_id)
                    .then_some((*command_id, item.request_id.clone()))
            })
            .collect();
        for (command_id, request_id) in commands {
            self.finish(
                registry,
                store.as_ref(),
                command_id,
                terminal(&command_id, &request_id, "target_offline"),
            )
            .await;
        }
    }

    /// Terminates an in-flight command exactly once and never lets a stale timeout overwrite a result.
    async fn finish(
        &self,
        registry: &ConnectionRegistry,
        store: &dyn DeviceControlStore,
        command_id: CommandId,
        result: CommandResult,
    ) {
        let item = self
            .state
            .lock()
            .expect("command router mutex is not poisoned")
            .remove(&command_id);
        let Some(item) = item else {
            return;
        };
        if complete(store, item.owner_id, item.target_device_id, result.clone()).await
            == StoreOutcome::Accepted
        {
            let _ = send(
                registry,
                item.owner_id,
                item.controller_device_id,
                item.controller_connection_id,
                "command.result",
                &result,
            );
        }
    }
}

fn required_scope(body: &CommandBody) -> DeviceControlScope {
    match body {
        CommandBody::Display { .. } => DeviceControlScope::DisplayControl,
        CommandBody::Actuator { .. } => DeviceControlScope::ActuatorControl,
        _ => DeviceControlScope::MediaControl,
    }
}

async fn replay(
    store: &dyn DeviceControlStore,
    registry: &ConnectionRegistry,
    owner_id: Uuid,
    controller_device_id: Uuid,
    controller_connection_id: Uuid,
    target_device_id: DeviceId,
    command_id: CommandId,
) -> Result<(), CommandError> {
    let lifecycle = store
        .load_command(owner_id, target_device_id, command_id)
        .await
        .map_err(|_| CommandError {
            code: "persistence_unavailable",
        })?;
    send(
        registry,
        owner_id,
        controller_device_id,
        controller_connection_id,
        "command.received",
        &CommandReceived {
            command_id,
            received_at: timestamp(OffsetDateTime::now_utc()),
            duplicate: true,
        },
    )?;
    if let Some(result) = lifecycle.and_then(|lifecycle| lifecycle.result) {
        send(
            registry,
            owner_id,
            controller_device_id,
            controller_connection_id,
            "command.result",
            &result,
        )?;
    }
    Ok(())
}

async fn complete(
    store: &dyn DeviceControlStore,
    owner_id: Uuid,
    target_device_id: DeviceId,
    result: CommandResult,
) -> StoreOutcome {
    store
        .complete_command(owner_id, target_device_id, result)
        .await
        .unwrap_or(StoreOutcome::Conflict)
}

fn send<T: serde::Serialize>(
    registry: &ConnectionRegistry,
    owner_id: Uuid,
    device_id: Uuid,
    connection_id: Uuid,
    kind: &'static str,
    payload: &T,
) -> Result<(), CommandError> {
    registry
        .send_to(
            owner_id,
            device_id,
            connection_id,
            OutboundFrame {
                kind,
                payload: serde_json::to_value(payload).map_err(|_| CommandError {
                    code: "invalid_payload",
                })?,
            },
        )
        .then_some(())
        .ok_or(CommandError {
            code: "target_offline",
        })
}

fn fingerprint(command: &DeviceCommand) -> Result<[u8; 32], serde_json::Error> {
    Ok(Sha256::digest(serde_json::to_vec(command)?).into())
}

fn terminal(command_id: &CommandId, request_id: &str, code: &'static str) -> CommandResult {
    terminal_message(
        command_id,
        request_id,
        code,
        "Device command did not complete.",
    )
}

/// Builds a terminal failure with a fixed message; the message must never echo a stream URI.
fn terminal_message(
    command_id: &CommandId,
    request_id: &str,
    code: &'static str,
    message: &'static str,
) -> CommandResult {
    CommandResult {
        command_id: *command_id,
        status: CommandStatus::Failed,
        completed_at: timestamp(OffsetDateTime::now_utc()),
        error: Some(DomainError {
            code: code.into(),
            message: message.into(),
            request_id: request_id.into(),
            details: Default::default(),
        }),
    }
}

pub(crate) fn timestamp(value: OffsetDateTime) -> Timestamp {
    Timestamp::parse(value.format(&Rfc3339).expect("RFC3339 format is valid"))
        .expect("server timestamp is valid")
}

fn validation_error(error: crate::device_control::ValidationError) -> CommandError {
    CommandError { code: error.code() }
}
