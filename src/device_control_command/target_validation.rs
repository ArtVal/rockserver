//! Target manifest and device capability validation for incoming commands.

use crate::device_control::{
    ActuatorAction, CATALOG_STATION_SOURCE, CommandBody, DIRECT_STATION_SOURCE, DeviceCapability,
    DeviceCommand, DeviceManifest, DeviceRole, Presentation, StreamSource, SurfaceKind, ViewKind,
    VolumeCommand,
};

use super::router::CommandError;

/// Validates that the target device's declared manifest supports executing the requested command.
pub fn validate_target(
    command: &DeviceCommand,
    manifest: &DeviceManifest,
) -> Result<(), CommandError> {
    match &command.body {
        CommandBody::PlayStation { .. }
        | CommandBody::PlayStream {
            source: StreamSource::RockserverCatalog,
            ..
        } => require_role_capability(manifest, DeviceRole::Player, |capability| {
            matches!(capability, DeviceCapability::Station { sources }
                if sources.iter().any(|source| source == CATALOG_STATION_SOURCE))
        }),
        CommandBody::PlayStream {
            source: StreamSource::DirectStream,
            ..
        } => require_role_capability(manifest, DeviceRole::Player, |capability| {
            matches!(capability, DeviceCapability::Station { sources }
                if sources.iter().any(|source| source == DIRECT_STATION_SOURCE))
        }),
        CommandBody::Playback { action } => require_role_capability(
            manifest,
            DeviceRole::Player,
            |capability| matches!(capability, DeviceCapability::Playback { actions } if actions.contains(action)),
        ),
        CommandBody::Volume { command } => {
            require_role_capability(manifest, DeviceRole::Player, |capability| {
                matches!(capability, DeviceCapability::Volume { mute, .. } if match command {
                    VolumeCommand::SetMute { .. } => *mute,
                    VolumeCommand::SetLevel { .. }
                    | VolumeCommand::Change { .. } => true,
                })
            })
        }
        CommandBody::Display { presentation } => {
            let Some(surface_id) = &command.target.surface_id else {
                return Err(CommandError {
                    code: "invalid_payload",
                });
            };
            let surface = manifest
                .surfaces
                .iter()
                .find(|surface| &surface.surface_id == surface_id)
                .ok_or(CommandError {
                    code: "capability_not_supported",
                })?;
            if surface.kind != SurfaceKind::Display {
                return Err(CommandError {
                    code: "capability_not_supported",
                });
            }
            let supported = manifest.capabilities.items.iter().any(|capability| matches!(capability, DeviceCapability::Display { views, max_items, max_text_length } if display_supported(presentation, views, *max_items, *max_text_length)));
            (manifest.roles.contains(&DeviceRole::DisplaySurface) && supported)
                .then_some(())
                .ok_or(CommandError {
                    code: "capability_not_supported",
                })
        }
        CommandBody::Actuator { action, value } => {
            let Some(entity_id) = &command.target.entity_id else {
                return Err(CommandError {
                    code: "invalid_payload",
                });
            };
            let entity = manifest
                .entities
                .iter()
                .find(|entity| &entity.entity_id == entity_id)
                .ok_or(CommandError {
                    code: "capability_not_supported",
                })?;
            let action_supported = manifest.capabilities.items.iter().any(|capability| matches!(capability, DeviceCapability::Actuator { commands } if commands.contains(action)));
            if !manifest.roles.contains(&DeviceRole::Actuator)
                || !entity.controllable
                || !action_supported
            {
                return Err(CommandError {
                    code: "capability_not_supported",
                });
            }
            if let (ActuatorAction::SetValue, Some(value)) = (action, value)
                && (entity.minimum.is_some_and(|minimum| *value < minimum)
                    || entity.maximum.is_some_and(|maximum| *value > maximum))
            {
                return Err(CommandError {
                    code: "invalid_payload",
                });
            }
            Ok(())
        }
        CommandBody::Unknown { .. } => Err(CommandError {
            code: "unsupported_command",
        }),
    }
}

/// Verifies that a manifest declares the required role and at least one matching capability.
pub fn require_role_capability(
    manifest: &DeviceManifest,
    role: DeviceRole,
    capability: impl Fn(&DeviceCapability) -> bool,
) -> Result<(), CommandError> {
    (manifest.roles.contains(&role) && manifest.capabilities.items.iter().any(capability))
        .then_some(())
        .ok_or(CommandError {
            code: "capability_not_supported",
        })
}

/// Checks whether a display presentation is supported by the device's display capabilities.
pub fn display_supported(
    presentation: &Presentation,
    views: &[ViewKind],
    max_items: u8,
    max_text_length: u16,
) -> bool {
    match presentation {
        Presentation::Text { text } => text.len() <= usize::from(max_text_length),
        Presentation::NowPlaying { .. } => views.contains(&ViewKind::NowPlaying),
        Presentation::SensorGrid { items, .. } => {
            views.contains(&ViewKind::SensorGrid) && items.len() <= usize::from(max_items)
        }
    }
}
