//! Integration tests for personal data synchronization, last-writer-wins conflict resolution,
//! cursor-based changesets, retention sweeps, and account purge cascades against PostgreSQL.

use std::env;

use rockserver::persistence::{PostgresAccountStore, PostgresPersonalDataStore};
use rockserver::personal_data::{
    CollectionChanges, FavouriteUpsert, HistoryUpsert, PersonalDataStore, RecordDelete,
    SyncRequest, Timestamp as PersonalTimestamp,
};
use uuid::Uuid;

/// Exercises migration 0025: last-writer-wins merges, per-account cursor deltas, tombstone
/// propagation, retention expiry, and deleted-account purge against real PostgreSQL.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn postgres_personal_data_sync_is_last_writer_wins_and_cursor_scoped() {
    let database_url = env::var("TEST_DATABASE_URL")
        .expect("set TEST_DATABASE_URL to an isolated PostgreSQL database");
    let accounts = PostgresAccountStore::connect(&database_url).await.unwrap();
    let store = PostgresPersonalDataStore::connect(&database_url)
        .await
        .unwrap();
    let owner = Uuid::new_v4();
    let foreign = Uuid::new_v4();
    accounts.create_user(owner).await.unwrap();
    accounts.create_user(foreign).await.unwrap();

    let now = time::OffsetDateTime::now_utc();
    let stamp = |offset: time::Duration| {
        PersonalTimestamp::parse(
            (now + offset)
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
        )
        .unwrap()
    };
    let favourite_record = Uuid::new_v4();
    let history_record = Uuid::new_v4();

    // First device pushes both collections.
    let pushed = store
        .synchronize(
            owner,
            SyncRequest {
                since_revision: 0,
                favourites: CollectionChanges {
                    upserts: vec![FavouriteUpsert {
                        record_id: favourite_record,
                        station_id: "station-rock-001".into(),
                        added_at: stamp(time::Duration::ZERO),
                        updated_at: stamp(time::Duration::ZERO),
                    }],
                    deletes: vec![],
                },
                history: CollectionChanges {
                    upserts: vec![HistoryUpsert {
                        record_id: history_record,
                        station_id: "station-jazz-002".into(),
                        started_at: stamp(time::Duration::minutes(-30)),
                        last_played_at: stamp(time::Duration::minutes(-5)),
                        ended_at: None,
                        play_duration_ms: Some(1_500_000),
                        metadata: Some(serde_json::json!({"lastKnownName": "Jazz FM"})),
                        updated_at: stamp(time::Duration::ZERO),
                    }],
                    deletes: vec![],
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(pushed.favourites.len(), 1);
    assert_eq!(pushed.history.len(), 1);
    assert!(pushed.server_revision > 0);
    let cursor = pushed.server_revision;

    // The same account from another device pulls a full snapshot.
    let snapshot = store
        .synchronize(owner, SyncRequest::default())
        .await
        .unwrap();
    assert_eq!(snapshot.favourites.len(), 1);
    assert_eq!(snapshot.history.len(), 1);
    assert_eq!(snapshot.server_revision, cursor);
    // A delta from the cursor returns nothing new.
    let quiet = store
        .synchronize(
            owner,
            SyncRequest {
                since_revision: cursor,
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    assert!(quiet.favourites.is_empty() && quiet.history.is_empty());

    // A stale upsert loses and is echoed back without advancing the cursor.
    let stale = store
        .synchronize(
            owner,
            SyncRequest {
                since_revision: cursor,
                favourites: CollectionChanges {
                    upserts: vec![FavouriteUpsert {
                        record_id: favourite_record,
                        station_id: "station-old".into(),
                        added_at: stamp(time::Duration::hours(-1)),
                        updated_at: stamp(time::Duration::hours(-1)),
                    }],
                    deletes: vec![],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        stale.favourites[0].station_id.as_deref(),
        Some("station-rock-001")
    );
    assert_eq!(stale.server_revision, cursor);

    // Deletions propagate as tombstones and stale upserts cannot resurrect.
    let deleted = store
        .synchronize(
            owner,
            SyncRequest {
                since_revision: cursor,
                favourites: CollectionChanges {
                    upserts: vec![],
                    deletes: vec![RecordDelete {
                        record_id: favourite_record,
                        updated_at: stamp(time::Duration::minutes(1)),
                    }],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    assert!(deleted.favourites[0].deleted_at.is_some());
    let resurrected = store
        .synchronize(
            owner,
            SyncRequest {
                since_revision: deleted.server_revision,
                favourites: CollectionChanges {
                    upserts: vec![FavouriteUpsert {
                        record_id: favourite_record,
                        station_id: "station-rock-001".into(),
                        added_at: stamp(time::Duration::hours(-2)),
                        updated_at: stamp(time::Duration::hours(-2)),
                    }],
                    deletes: vec![],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    assert!(resurrected.favourites[0].deleted_at.is_some());

    // Accounts are isolated.
    let other = store
        .synchronize(foreign, SyncRequest::default())
        .await
        .unwrap();
    assert!(other.favourites.is_empty() && other.history.is_empty());

    // History past the retention window expires server-side on the next push.
    let ancient = store
        .synchronize(
            owner,
            SyncRequest {
                history: CollectionChanges {
                    upserts: vec![HistoryUpsert {
                        record_id: Uuid::new_v4(),
                        station_id: "station-ancient".into(),
                        started_at: stamp(time::Duration::days(-200)),
                        last_played_at: stamp(time::Duration::days(-200)),
                        ended_at: None,
                        play_duration_ms: None,
                        metadata: None,
                        updated_at: stamp(time::Duration::days(-200)),
                    }],
                    deletes: vec![],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        ancient.history.len(),
        2,
        "since_revision 0 returns the full snapshot: both history rows"
    );
    assert!(
        ancient
            .history
            .iter()
            .any(|row| row.station_id.as_deref() == Some("station-ancient")
                && row.deleted_at.is_some()),
        "push-time retention must tombstone ancient history immediately"
    );

    // The fleet-wide sweep expires rows whose clock aged past the window after the push.
    let raw = sqlx::PgPool::connect(&database_url).await.unwrap();
    sqlx::query(
        "UPDATE history_records SET started_at = now() - make_interval(days => 200) \
         WHERE user_id=$1 AND deleted_at IS NULL",
    )
    .bind(owner)
    .execute(&raw)
    .await
    .unwrap();
    let sweep = store.sweep_retention().await.unwrap();
    assert!(
        sweep.history_expired >= 1,
        "the sweep must expire aged rows"
    );
    let after_sweep = store
        .synchronize(owner, SyncRequest::default())
        .await
        .unwrap();
    assert!(
        after_sweep
            .history
            .iter()
            .all(|row| row.deleted_at.is_some()),
        "every history row must be tombstoned after expiry"
    );

    // Rows of accounts deleted past the grace window are purged for good.
    let doomed = Uuid::new_v4();
    accounts.create_user(doomed).await.unwrap();
    store
        .synchronize(
            doomed,
            SyncRequest {
                favourites: CollectionChanges {
                    upserts: vec![FavouriteUpsert {
                        record_id: Uuid::new_v4(),
                        station_id: "station-doomed".into(),
                        added_at: stamp(time::Duration::ZERO),
                        updated_at: stamp(time::Duration::ZERO),
                    }],
                    deletes: vec![],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    let raw = sqlx::PgPool::connect(&database_url).await.unwrap();
    sqlx::query("UPDATE users SET status='deleted', deleted_at=now() - make_interval(days => 40) WHERE id=$1")
        .bind(doomed)
        .execute(&raw)
        .await
        .unwrap();
    let sweep = store.sweep_retention().await.unwrap();
    assert!(
        sweep.deleted_account_rows_removed >= 1,
        "deleted-account rows must be purged"
    );
    let purged = store
        .synchronize(doomed, SyncRequest::default())
        .await
        .unwrap();
    assert!(
        purged.favourites.is_empty(),
        "purged rows must stop being returned"
    );
}
