use std::env;

use rockserver::{
    auth::OwnedDevice,
    device_control::{
        CommandId, CommandReservation, CommandResult, CommandStatus, DeviceCommand,
        DeviceControlStore, DeviceId, DeviceManifest, DeviceStateSnapshot, DomainError,
        EntityState, StoreOutcome, Timestamp,
    },
    persistence::{PostgresAccountStore, PostgresDeviceControlStore},
};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

/// Exercises migration 0021, active-owner filtering, manifest revisions, and tombstoned projections.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn postgres_device_control_manifest_is_owner_scoped_and_tombstoned() {
    let database_url = env::var("TEST_DATABASE_URL")
        .expect("set TEST_DATABASE_URL to an isolated PostgreSQL database");
    let accounts = PostgresAccountStore::connect(&database_url).await.unwrap();
    let store = PostgresDeviceControlStore::connect(&database_url)
        .await
        .unwrap();
    let owner = Uuid::new_v4();
    let foreign = Uuid::new_v4();
    let device = Uuid::new_v4();
    accounts.create_user(owner).await.unwrap();
    accounts.create_user(foreign).await.unwrap();
    assert!(
        accounts
            .create_device(&OwnedDevice {
                id: device,
                user_id: owner,
                device_display_name: "control fixture".into(),
                device_type: "test".into(),
                created_at: "2026-09-02T00:00:00Z".into(),
                last_seen_at: None
            })
            .await
            .unwrap()
    );
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../fixtures/device-control/v1/esp32-manifest-client.json"
    ))
    .unwrap();
    let mut manifest: DeviceManifest =
        serde_json::from_value(fixture["payload"]["manifest"].clone()).unwrap();
    assert_eq!(
        store
            .apply_manifest(owner, DeviceId(device), manifest.clone())
            .await
            .unwrap(),
        StoreOutcome::Accepted
    );
    assert_eq!(
        store
            .apply_manifest(owner, DeviceId(device), manifest.clone())
            .await
            .unwrap(),
        StoreOutcome::Replay
    );
    assert_eq!(
        store
            .apply_manifest(foreign, DeviceId(device), manifest.clone())
            .await
            .unwrap(),
        StoreOutcome::NotOwned
    );
    assert_eq!(
        store
            .list_entities(owner, DeviceId(device))
            .await
            .unwrap()
            .len(),
        manifest.entities.len()
    );
    let state_fixture: Value = serde_json::from_str(include_str!(
        "../fixtures/device-control/v1/esp32-state-full-client.json"
    ))
    .unwrap();
    let state: DeviceStateSnapshot =
        serde_json::from_value(state_fixture["payload"]["snapshot"].clone()).unwrap();
    assert_eq!(
        store
            .store_device_state(owner, DeviceId(device), state.clone())
            .await
            .unwrap(),
        StoreOutcome::Accepted
    );
    assert_eq!(
        store
            .store_device_state(owner, DeviceId(device), state.clone())
            .await
            .unwrap(),
        StoreOutcome::Replay
    );
    let mut stale_state = state.clone();
    stale_state.state_revision -= 1;
    assert_eq!(
        store
            .store_device_state(owner, DeviceId(device), stale_state)
            .await
            .unwrap(),
        StoreOutcome::Stale
    );
    let mut conflicting_state = state.clone();
    conflicting_state.state.playback.as_mut().unwrap().status = "playing".into();
    assert_eq!(
        store
            .store_device_state(owner, DeviceId(device), conflicting_state)
            .await
            .unwrap(),
        StoreOutcome::Conflict
    );
    // A full state snapshot is itself the resync primitive: a forward revision gap (facts
    // published while disconnected) overwrites the stored projection instead of demanding
    // an `accepted + 1` successor the device's monotonic counter cannot produce.
    let mut gap_state = state.clone();
    gap_state.state_revision += 2;
    assert_eq!(
        store
            .store_device_state(owner, DeviceId(device), gap_state.clone())
            .await
            .unwrap(),
        StoreOutcome::Accepted
    );
    assert_eq!(
        store
            .load_device_state(owner, DeviceId(device))
            .await
            .unwrap()
            .unwrap()
            .state_revision,
        gap_state.state_revision
    );
    let entity_fixture: Value = serde_json::from_str(include_str!(
        "../fixtures/device-control/v1/esp32-temperature-state-client.json"
    ))
    .unwrap();
    let entity_state: EntityState =
        serde_json::from_value(entity_fixture["payload"]["state"].clone()).unwrap();
    assert_eq!(
        store
            .store_entity_state(owner, DeviceId(device), entity_state.clone())
            .await
            .unwrap(),
        StoreOutcome::Accepted
    );
    assert_eq!(
        store
            .store_entity_state(owner, DeviceId(device), entity_state.clone())
            .await
            .unwrap(),
        StoreOutcome::Replay
    );
    let mut stale_entity = entity_state.clone();
    stale_entity.entity_revision -= 1;
    assert_eq!(
        store
            .store_entity_state(owner, DeviceId(device), stale_entity)
            .await
            .unwrap(),
        StoreOutcome::Stale
    );
    assert!(
        store
            .load_entity_state(owner, DeviceId(device), &entity_state.entity_id)
            .await
            .unwrap()
            .is_some()
    );
    let command_fixture: Value = serde_json::from_str(include_str!(
        "../fixtures/device-control/v1/display-sensor-grid-command-server.json"
    ))
    .unwrap();
    let mut command: DeviceCommand =
        serde_json::from_value(command_fixture["payload"].clone()).unwrap();
    command.command_id = CommandId(Uuid::new_v4());
    command.target.device_id = DeviceId(device);
    let reservation = CommandReservation {
        command: command.clone(),
        fingerprint: [7; 32],
        deadline_at: Timestamp::parse("2030-01-01T00:00:00Z").unwrap(),
    };
    assert_eq!(
        store
            .reserve_command(owner, DeviceId(device), reservation.clone())
            .await
            .unwrap(),
        StoreOutcome::Accepted
    );
    assert_eq!(
        store
            .reserve_command(owner, DeviceId(device), reservation.clone())
            .await
            .unwrap(),
        StoreOutcome::Replay
    );
    let mut conflicting = reservation.clone();
    conflicting.fingerprint = [8; 32];
    assert_eq!(
        store
            .reserve_command(owner, DeviceId(device), conflicting)
            .await
            .unwrap(),
        StoreOutcome::Conflict
    );
    let result = CommandResult {
        command_id: command.command_id,
        status: CommandStatus::Succeeded,
        completed_at: Timestamp::parse("2026-09-02T12:03:00Z").unwrap(),
        error: None,
    };
    assert_eq!(
        store
            .complete_command(owner, DeviceId(device), result.clone())
            .await
            .unwrap(),
        StoreOutcome::Accepted
    );
    assert_eq!(
        store
            .complete_command(owner, DeviceId(device), result)
            .await
            .unwrap(),
        StoreOutcome::Replay
    );
    let conflicting_result = CommandResult {
        command_id: command.command_id,
        status: CommandStatus::Failed,
        completed_at: Timestamp::parse("2026-09-02T12:03:01Z").unwrap(),
        error: Some(DomainError {
            code: "failed".into(),
            message: "safe test error".into(),
            request_id: "postgres-test".into(),
            details: Default::default(),
        }),
    };
    assert_eq!(
        store
            .complete_command(owner, DeviceId(device), conflicting_result)
            .await
            .unwrap(),
        StoreOutcome::Conflict
    );
    assert!(
        store
            .load_command(owner, DeviceId(device), command.command_id)
            .await
            .unwrap()
            .unwrap()
            .result
            .is_some()
    );
    let pool = PgPool::connect(&database_url).await.unwrap();
    assert!(sqlx::query("INSERT INTO device_control_manifests(device_id,revision,payload) VALUES($1,0,'{}'::jsonb)")
        .bind(Uuid::new_v4()).execute(&pool).await.is_err());
    sqlx::query("UPDATE device_control_commands SET completed_at=now()-interval '25 hours' WHERE command_id=$1")
        .bind(command.command_id.0).execute(&pool).await.unwrap();
    pool.close().await;
    assert_eq!(store.prune_commands(1).await.unwrap(), 1);
    assert!(
        store
            .load_command(owner, DeviceId(device), command.command_id)
            .await
            .unwrap()
            .is_none()
    );
    let original_entities = manifest.entities.clone();
    manifest.manifest_revision += 1;
    manifest.entities.clear();
    assert_eq!(
        store
            .apply_manifest(owner, DeviceId(device), manifest.clone())
            .await
            .unwrap(),
        StoreOutcome::Accepted
    );
    assert!(
        store
            .list_entities(owner, DeviceId(device))
            .await
            .unwrap()
            .is_empty()
    );
    manifest.manifest_revision += 1;
    manifest.entities = original_entities;
    assert_eq!(
        store
            .apply_manifest(owner, DeviceId(device), manifest)
            .await
            .unwrap(),
        StoreOutcome::Accepted
    );
    assert_eq!(
        store
            .list_entities(owner, DeviceId(device))
            .await
            .unwrap()
            .len(),
        2
    );
    assert!(accounts.revoke_owned_device(owner, device).await.unwrap());
    assert!(
        store
            .load_manifest(owner, DeviceId(device))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .store_device_state(owner, DeviceId(device), state)
            .await
            .unwrap(),
        StoreOutcome::NotOwned
    );
    store.close().await;
    accounts.close().await;
}
