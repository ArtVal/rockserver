//! Private state-admission helpers for the device-control WebSocket lifecycle.

use uuid::Uuid;

use crate::{
    device_control::{
        DeviceId, DeviceRole, DeviceRuntimeState, DeviceStateDelta, DeviceStateSnapshot,
        EntityState, RevisionOrder, revision_order,
    },
    device_control_state::StateHub,
};

/// Rejects impossible or deliberately empty complete runtime observations.
pub(super) fn valid_snapshot(snapshot: &DeviceStateSnapshot) -> bool {
    snapshot.state_revision > 0
        && (snapshot.state.playback.is_some()
            || snapshot.state.volume.is_some()
            || snapshot.state.display.is_some())
        && valid_track_title(&snapshot.state)
}

/// Rejects a delta that cannot represent the next monotonic device observation.
pub(super) fn valid_delta(delta: &DeviceStateDelta) -> bool {
    delta.base_revision > 0
        && delta.state_revision > delta.base_revision
        && (delta.changes.playback.is_some()
            || delta.changes.volume.is_some()
            || delta.changes.display.is_some())
        && valid_track_title(&delta.changes)
}

/// Keeps optional metadata within the published protocol's text bound.
fn valid_track_title(state: &DeviceRuntimeState) -> bool {
    state
        .playback
        .as_ref()
        .and_then(|playback| playback.track_title.as_deref())
        .is_none_or(|title| {
            let length = title.chars().count();
            (1..=256).contains(&length) && !title.chars().any(char::is_control)
        })
}

/// Applies only fields explicitly included by a typed state delta.
pub(super) fn merge_state(
    current: DeviceRuntimeState,
    changes: DeviceRuntimeState,
) -> DeviceRuntimeState {
    DeviceRuntimeState {
        playback: changes.playback.or(current.playback),
        volume: changes.volume.or(current.volume),
        display: changes.display.or(current.display),
    }
}

/// Publishes a full snapshot when its revision is a legal successor or a forward resync.
///
/// A complete snapshot is the protocol's resync primitive: the device may publish new
/// revisions while its socket is down, so with no base requirement a gap can only be a
/// forward revision that legitimately overwrites the older projection. Rejecting it would
/// demand an impossible `accepted + 1` successor and drop the device forever. Deltas keep
/// the strictly ordered `base_revision` semantics and never take this path.
pub(super) fn accept_snapshot(
    hub: &StateHub,
    user_id: Uuid,
    device_id: Uuid,
    snapshot: DeviceStateSnapshot,
) -> RevisionOrder {
    let device_id = DeviceId(device_id);
    let result = match hub.device_state(user_id, device_id) {
        Some(current) => revision_order(
            current.state_revision,
            &current.state,
            snapshot.state_revision,
            &snapshot.state,
            None,
        ),
        None => RevisionOrder::Next,
    };
    match result {
        RevisionOrder::Next | RevisionOrder::Gap => {
            hub.publish_device_state(user_id, device_id, snapshot);
            RevisionOrder::Next
        }
        order => order,
    }
}

/// Classifies a typed entity observation against the account-scoped latest observation.
pub(super) fn entity_revision(
    hub: &StateHub,
    user_id: Uuid,
    device_id: DeviceId,
    incoming: &EntityState,
) -> RevisionOrder {
    match hub.entity_state(user_id, device_id, &incoming.entity_id) {
        Some(accepted) => revision_order(
            accepted.entity_revision,
            &accepted,
            incoming.entity_revision,
            incoming,
            None,
        ),
        None => RevisionOrder::Next,
    }
}

/// A controller-only device publishes no runtime facts and thus skips the full-state requirement.
pub(super) fn requires_full_state(roles: &[DeviceRole]) -> bool {
    roles.iter().any(|role| *role != DeviceRole::Controller)
}

/// A stale full snapshot never mutates persistence, but it proves the reconnecting player still
/// has a valid base state and may continue sending heartbeats.
pub(super) fn accepted_full_snapshot(
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
mod track_title_tests {
    use super::valid_track_title;
    use crate::device_control::{DeviceRuntimeState, PlaybackState};

    #[test]
    fn rejects_empty_and_overlong_track_metadata() {
        let mut state = DeviceRuntimeState {
            playback: Some(PlaybackState {
                status: "playing".into(),
                station_id: Some("id".into()),
                track_title: Some("Artist - Track".into()),
            }),
            ..Default::default()
        };
        assert!(valid_track_title(&state));
        state.playback.as_mut().unwrap().track_title = Some(String::new());
        assert!(!valid_track_title(&state));
        state.playback.as_mut().unwrap().track_title = Some("x".repeat(257));
        assert!(!valid_track_title(&state));
    }
}
