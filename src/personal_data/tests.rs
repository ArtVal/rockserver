//! Unit tests for personal-data models, validation, and in-memory synchronization.

use time::OffsetDateTime;
use uuid::Uuid;

use super::in_memory::{history_retention, tombstone_retention};
use super::*;

fn user() -> Uuid {
    Uuid::from_u128(0xA000_0000_0000_0000_0000_0000_0000_0001)
}

fn other_user() -> Uuid {
    Uuid::from_u128(0xA000_0000_0000_0000_0000_0000_0000_0002)
}

fn record(n: u64) -> Uuid {
    Uuid::from_u128(0xB000_0000_0000_0000_0000_0000_0000_0000 + u128::from(n))
}

fn ts(instant: OffsetDateTime) -> Timestamp {
    Timestamp::from_instant(instant)
}

fn favourite(n: u64, updated: OffsetDateTime) -> FavouriteUpsert {
    FavouriteUpsert {
        record_id: record(n),
        station_id: format!("station-{n}"),
        added_at: ts(updated),
        updated_at: ts(updated),
    }
}

fn history_entry(n: u64, started: OffsetDateTime) -> HistoryUpsert {
    HistoryUpsert {
        record_id: record(n),
        station_id: format!("station-{n}"),
        started_at: ts(started),
        last_played_at: ts(started),
        ended_at: None,
        play_duration_ms: None,
        metadata: None,
        updated_at: ts(started),
    }
}

fn store() -> InMemoryPersonalDataStore {
    InMemoryPersonalDataStore::default()
}

#[tokio::test]
async fn first_sync_returns_full_snapshot_and_cursor() {
    let now = OffsetDateTime::now_utc();
    let store = store();
    let request = SyncRequest {
        favourites: CollectionChanges {
            upserts: vec![favourite(1, now), favourite(2, now)],
            deletes: vec![],
        },
        ..SyncRequest::default()
    };
    let outcome = store.synchronize(user(), request).await.unwrap();
    assert_eq!(outcome.favourites.len(), 2);
    assert!(outcome.history.is_empty());
    assert!(outcome.server_revision > 0);
    let first_cursor = outcome.server_revision;

    let empty = store
        .synchronize(
            user(),
            SyncRequest {
                since_revision: first_cursor,
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    assert!(empty.favourites.is_empty());
    assert_eq!(empty.server_revision, first_cursor, "no changes, no drift");
}

#[tokio::test]
async fn delta_after_cursor_only_returns_newer_revisions() {
    let now = OffsetDateTime::now_utc();
    let store = store();
    let first = store
        .synchronize(
            user(),
            SyncRequest {
                favourites: CollectionChanges {
                    upserts: vec![favourite(1, now)],
                    deletes: vec![],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    let second = store
        .synchronize(
            user(),
            SyncRequest {
                since_revision: first.server_revision,
                favourites: CollectionChanges {
                    upserts: vec![favourite(2, now)],
                    deletes: vec![],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        second
            .favourites
            .iter()
            .map(|row| row.record_id)
            .collect::<Vec<_>>(),
        vec![record(2)],
        "the earlier record must stay behind the cursor"
    );
    assert!(second.server_revision > first.server_revision);
}

#[tokio::test]
async fn last_writer_wins_rejects_stale_and_equal_upserts() {
    let now = OffsetDateTime::now_utc();
    let store = store();
    let newer = now;
    let older = now - time::Duration::minutes(10);
    store
        .synchronize(
            user(),
            SyncRequest {
                favourites: CollectionChanges {
                    upserts: vec![favourite(1, newer)],
                    deletes: vec![],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    let outcome = store
        .synchronize(
            user(),
            SyncRequest {
                since_revision: 0,
                favourites: CollectionChanges {
                    upserts: vec![favourite(1, older)],
                    deletes: vec![],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    let row = &outcome.favourites[0];
    assert_eq!(row.updated_at, ts(newer), "stale push must lose");
    assert!(
        outcome
            .favourites
            .iter()
            .all(|row| row.deleted_at.is_none()),
        "the winner must survive"
    );

    // Equal instants keep the stored row so replays stay idempotent.
    let same = store
        .synchronize(
            user(),
            SyncRequest {
                favourites: CollectionChanges {
                    upserts: vec![FavouriteUpsert {
                        record_id: record(1),
                        station_id: "renamed".to_owned(),
                        added_at: ts(older),
                        updated_at: ts(newer),
                    }],
                    deletes: vec![],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(same.favourites[0].station_id.as_deref(), Some("station-1"));
}

#[tokio::test]
async fn deletes_tombstone_and_newer_upserts_resurrect() {
    let now = OffsetDateTime::now_utc();
    let store = store();
    let delete_at = now - time::Duration::minutes(5);
    let outcome = store
        .synchronize(
            user(),
            SyncRequest {
                favourites: CollectionChanges {
                    upserts: vec![],
                    deletes: vec![RecordDelete {
                        record_id: record(7),
                        updated_at: ts(delete_at),
                    }],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    let row = &outcome.favourites[0];
    assert_eq!(row.record_id, record(7));
    assert!(row.deleted_at.is_some());
    assert!(row.station_id.is_none(), "bare tombstones carry no station");

    // An older upsert replayed after the deletion must not resurrect the record.
    let stale = store
        .synchronize(
            user(),
            SyncRequest {
                favourites: CollectionChanges {
                    upserts: vec![favourite(7, delete_at - time::Duration::minutes(1))],
                    deletes: vec![],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    assert!(stale.favourites[0].deleted_at.is_some());

    let resurrected = store
        .synchronize(
            user(),
            SyncRequest {
                favourites: CollectionChanges {
                    upserts: vec![favourite(7, now)],
                    deletes: vec![],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    let row = &resurrected.favourites[0];
    assert!(row.deleted_at.is_none());
    assert_eq!(row.station_id.as_deref(), Some("station-7"));
}

#[tokio::test]
async fn pushed_loser_is_echoed_without_advancing_the_cursor() {
    let now = OffsetDateTime::now_utc();
    let store = store();
    let winner = store
        .synchronize(
            user(),
            SyncRequest {
                favourites: CollectionChanges {
                    upserts: vec![favourite(1, now)],
                    deletes: vec![],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    let loser = store
        .synchronize(other_user(), SyncRequest::default())
        .await
        .unwrap();
    assert!(loser.favourites.is_empty());

    let stale = store
        .synchronize(
            user(),
            SyncRequest {
                since_revision: winner.server_revision,
                favourites: CollectionChanges {
                    upserts: vec![favourite(1, now - time::Duration::minutes(30))],
                    deletes: vec![],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        stale.favourites.len(),
        1,
        "the pushed loser must be echoed back"
    );
    assert_eq!(
        stale.server_revision, winner.server_revision,
        "an echoed loser must not advance the cursor"
    );
    assert_eq!(stale.favourites[0].updated_at, ts(now));
}

#[tokio::test]
async fn accounts_are_isolated() {
    let now = OffsetDateTime::now_utc();
    let store = store();
    store
        .synchronize(
            user(),
            SyncRequest {
                favourites: CollectionChanges {
                    upserts: vec![favourite(1, now)],
                    deletes: vec![],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    let other = store
        .synchronize(other_user(), SyncRequest::default())
        .await
        .unwrap();
    assert!(
        other.favourites.is_empty(),
        "one account must never see another account's records"
    );
}

#[tokio::test]
async fn collection_limits_reject_the_batch() {
    let now = OffsetDateTime::now_utc();
    let store = store();
    let first: Vec<FavouriteUpsert> = (1..=MAX_SYNC_BATCH_ITEMS)
        .map(|n| favourite(u64::try_from(n).expect("positive"), now))
        .collect();
    store
        .synchronize(
            user(),
            SyncRequest {
                favourites: CollectionChanges {
                    upserts: first,
                    deletes: vec![],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    let overflowing: Vec<FavouriteUpsert> = (0..MAX_SYNC_BATCH_ITEMS as u64)
        .map(|n| favourite(5000 + n, now))
        .collect();
    let error = store
        .synchronize(
            user(),
            SyncRequest {
                favourites: CollectionChanges {
                    upserts: overflowing,
                    deletes: vec![],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error, PersonalDataError::FavouriteLimitExceeded);
}

#[tokio::test]
async fn history_expires_past_retention_and_sweep_removes_old_tombstones() {
    let now = OffsetDateTime::now_utc();
    let store = store();
    let ancient = now - history_retention() - time::Duration::days(1);
    let fresh = now;
    store
        .synchronize(
            user(),
            SyncRequest {
                history: CollectionChanges {
                    upserts: vec![history_entry(1, ancient), history_entry(2, fresh)],
                    deletes: vec![],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    let outcome = store
        .synchronize(user(), SyncRequest::default())
        .await
        .unwrap();
    let expired = outcome
        .history
        .iter()
        .find(|row| row.record_id == record(1))
        .expect("expired row must be echoed after retention");
    assert!(expired.deleted_at.is_some(), "ancient history must expire");
    assert!(
        outcome
            .history
            .iter()
            .find(|row| row.record_id == record(2))
            .expect("fresh row must be echoed")
            .deleted_at
            .is_none()
    );

    // Force a tombstone past the GC window, then sweep.
    let ancient_deletion = now - tombstone_retention() - time::Duration::days(1);
    store
        .synchronize(
            user(),
            SyncRequest {
                favourites: CollectionChanges {
                    upserts: vec![favourite(9, now - time::Duration::days(200))],
                    deletes: vec![],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    store
        .synchronize(
            user(),
            SyncRequest {
                favourites: CollectionChanges {
                    upserts: vec![],
                    deletes: vec![RecordDelete {
                        record_id: record(9),
                        updated_at: ts(ancient_deletion),
                    }],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    let sweep = store.sweep_retention().await.unwrap();
    assert!(sweep.tombstones_removed >= 1, "old tombstones must be GC'd");
    let after = store
        .synchronize(user(), SyncRequest::default())
        .await
        .unwrap();
    assert!(
        after
            .favourites
            .iter()
            .all(|row| row.record_id != record(9)),
        "GC'd tombstones must stop being returned"
    );
}

#[test]
fn validation_rejects_bad_batches_and_records() {
    let now = OffsetDateTime::now_utc();
    let mut request = SyncRequest::default();
    request.favourites.upserts = (0..=MAX_SYNC_BATCH_ITEMS)
        .map(|n| favourite(u64::try_from(n).expect("positive"), now))
        .collect();
    assert_eq!(
        request.validate(now),
        Err(PersonalDataError::Validation("favourites.upserts"))
    );

    let mut request = SyncRequest::default();
    request.favourites.upserts.push(FavouriteUpsert {
        record_id: record(1),
        station_id: String::new(),
        added_at: ts(now),
        updated_at: ts(now),
    });
    assert_eq!(
        request.validate(now),
        Err(PersonalDataError::Validation("station_id"))
    );

    let mut request = SyncRequest::default();
    request.favourites.upserts.push(FavouriteUpsert {
        record_id: record(1),
        station_id: "station-1".to_owned(),
        added_at: ts(now),
        updated_at: ts(now
            + time::Duration::seconds(MAX_CLIENT_CLOCK_SKEW_SECONDS)
            + time::Duration::seconds(1)),
    });
    assert_eq!(
        request.validate(now),
        Err(PersonalDataError::Validation("timestamp"))
    );

    let mut request = SyncRequest::default();
    request.history.upserts.push(HistoryUpsert {
        record_id: record(1),
        station_id: "station-1".to_owned(),
        started_at: ts(now),
        last_played_at: ts(now - time::Duration::minutes(1)),
        ended_at: None,
        play_duration_ms: None,
        metadata: None,
        updated_at: ts(now),
    });
    assert_eq!(
        request.validate(now),
        Err(PersonalDataError::Validation("history.last_played_at"))
    );

    let mut request = SyncRequest::default();
    request.history.upserts.push(HistoryUpsert {
        record_id: record(1),
        station_id: "station-1".to_owned(),
        started_at: ts(now),
        last_played_at: ts(now),
        ended_at: None,
        play_duration_ms: None,
        metadata: Some(serde_json::json!([1, 2])),
        updated_at: ts(now),
    });
    assert_eq!(
        request.validate(now),
        Err(PersonalDataError::Validation("history.metadata"))
    );

    let duplicated = SyncRequest {
        favourites: CollectionChanges {
            upserts: vec![favourite(1, now)],
            deletes: vec![RecordDelete {
                record_id: record(1),
                updated_at: ts(now),
            }],
        },
        ..SyncRequest::default()
    };
    assert_eq!(
        duplicated.validate(now),
        Err(PersonalDataError::Validation("favourites"))
    );
}
