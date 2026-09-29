//! In-memory deterministic implementation of [`PersonalDataStore`] for testing and offline routers.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use time::OffsetDateTime;
use uuid::Uuid;

use super::domain::{
    FavouriteRecord, FavouriteUpsert, HISTORY_RETENTION_DAYS, HistoryRecord, HistoryUpsert,
    MAX_FAVOURITE_RECORDS, MAX_HISTORY_RECORDS, PersonalDataError, PersonalDataStore, RecordDelete,
    RetentionSweep, SyncOutcome, SyncRequest, TOMBSTONE_RETENTION_DAYS, Timestamp,
};

/// Allocates shared monotonic sync revisions from one atomic counter.
struct RevisionAllocator<'a> {
    counter: &'a AtomicU64,
}

impl RevisionAllocator<'_> {
    fn next(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::Relaxed) + 1
    }
}

/// Deterministic in-memory [`PersonalDataStore`] for unit tests and offline routers.
///
/// It mirrors the PostgreSQL merge, limit, and retention semantics without SQL so unit
/// tests stay offline; deleted-account purge is SQL-only and stays unimplemented here.
#[derive(Default)]
pub struct InMemoryPersonalDataStore {
    favourites: Mutex<BTreeMap<(Uuid, Uuid), FavouriteRecord>>,
    history: Mutex<BTreeMap<(Uuid, Uuid), HistoryRecord>>,
    revision: AtomicU64,
}

impl InMemoryPersonalDataStore {
    fn allocator(&self) -> RevisionAllocator<'_> {
        RevisionAllocator {
            counter: &self.revision,
        }
    }
}

#[async_trait]
impl PersonalDataStore for InMemoryPersonalDataStore {
    async fn synchronize(
        &self,
        user_id: Uuid,
        request: SyncRequest,
    ) -> Result<SyncOutcome, PersonalDataError> {
        let now = OffsetDateTime::now_utc();
        request.validate(now)?;
        let pushed_favourites: BTreeSet<Uuid> =
            request.favourite_record_ids().into_iter().collect();
        let pushed_history: BTreeSet<Uuid> = request.history_record_ids().into_iter().collect();
        let revisions = self.allocator();
        {
            let mut favourites = self
                .favourites
                .lock()
                .expect("favourites mutex is not poisoned");
            for upsert in &request.favourites.upserts {
                merge_favourite(&mut favourites, user_id, upsert, &revisions);
            }
            for delete in &request.favourites.deletes {
                tombstone_favourite(&mut favourites, user_id, delete, &revisions);
            }
            let live = favourites
                .iter()
                .filter(|((owner, _), row)| *owner == user_id && row.deleted_at.is_none())
                .count();
            if live > MAX_FAVOURITE_RECORDS {
                return Err(PersonalDataError::FavouriteLimitExceeded);
            }
        }
        {
            let mut history = self.history.lock().expect("history mutex is not poisoned");
            for upsert in &request.history.upserts {
                merge_history(&mut history, user_id, upsert, &revisions);
            }
            for delete in &request.history.deletes {
                tombstone_history(&mut history, user_id, delete, &revisions);
            }
            expire_history(&mut history, user_id, now, &revisions);
            let live = history
                .iter()
                .filter(|((owner, _), row)| *owner == user_id && row.deleted_at.is_none())
                .count();
            if live > MAX_HISTORY_RECORDS {
                return Err(PersonalDataError::HistoryLimitExceeded);
            }
        }
        let favourites = self
            .favourites
            .lock()
            .expect("favourites mutex is not poisoned");
        let history = self.history.lock().expect("history mutex is not poisoned");
        let (favourite_delta, favourite_cursor) = delta_and_echo(
            favourites
                .iter()
                .filter(|((owner, _), _)| *owner == user_id)
                .map(|((_, record_id), row)| (record_id, row)),
            request.since_revision,
            &pushed_favourites,
            |row: &FavouriteRecord| row.sync_revision,
        );
        let (history_delta, history_cursor) = delta_and_echo(
            history
                .iter()
                .filter(|((owner, _), _)| *owner == user_id)
                .map(|((_, record_id), row)| (record_id, row)),
            request.since_revision,
            &pushed_history,
            |row: &HistoryRecord| row.sync_revision,
        );
        Ok(SyncOutcome {
            server_revision: favourite_cursor.max(history_cursor),
            favourites: favourite_delta,
            history: history_delta,
        })
    }

    async fn sweep_retention(&self) -> Result<RetentionSweep, PersonalDataError> {
        let now = OffsetDateTime::now_utc();
        let revisions = self.allocator();
        let mut sweep = RetentionSweep::default();
        {
            let mut history = self.history.lock().expect("history mutex is not poisoned");
            let expired: Vec<(Uuid, Uuid)> = history
                .iter()
                .filter(|(_, row)| {
                    row.deleted_at.is_none() && row.started_at.instant() < now - history_retention()
                })
                .map(|(key, _)| *key)
                .collect();
            for key in expired {
                if let Some(row) = history.get_mut(&key) {
                    row.deleted_at = Some(row.updated_at.clone());
                    row.sync_revision = revisions.next();
                    sweep.history_expired += 1;
                }
            }
            let cutoff = now - tombstone_retention();
            let before = history.len();
            history.retain(|_, row| {
                row.deleted_at
                    .as_ref()
                    .is_none_or(|deleted| deleted.instant() >= cutoff)
            });
            sweep.tombstones_removed += (before - history.len()) as u64;
        }
        {
            let mut favourites = self
                .favourites
                .lock()
                .expect("favourites mutex is not poisoned");
            let cutoff = now - tombstone_retention();
            let before = favourites.len();
            favourites.retain(|_, row| {
                row.deleted_at
                    .as_ref()
                    .is_none_or(|deleted| deleted.instant() >= cutoff)
            });
            sweep.tombstones_removed += (before - favourites.len()) as u64;
        }
        Ok(sweep)
    }
}

pub(crate) fn history_retention() -> time::Duration {
    time::Duration::days(HISTORY_RETENTION_DAYS)
}

pub(crate) fn tombstone_retention() -> time::Duration {
    time::Duration::days(TOMBSTONE_RETENTION_DAYS)
}

/// Shared last-writer-wins decision: an incoming change wins only over a strictly older
/// stored row; equal instants keep the stored row so replays stay idempotent.
fn strictly_newer(stored: Option<&Timestamp>, incoming: &Timestamp) -> bool {
    stored.is_none_or(|current| current.instant() < incoming.instant())
}

fn merge_favourite(
    favourites: &mut BTreeMap<(Uuid, Uuid), FavouriteRecord>,
    user_id: Uuid,
    upsert: &FavouriteUpsert,
    revisions: &RevisionAllocator<'_>,
) {
    let key = (user_id, upsert.record_id);
    if !strictly_newer(
        favourites.get(&key).map(|row| &row.updated_at),
        &upsert.updated_at,
    ) {
        return;
    }
    favourites.insert(
        key,
        FavouriteRecord {
            record_id: upsert.record_id,
            station_id: Some(upsert.station_id.clone()),
            added_at: upsert.added_at.clone(),
            updated_at: upsert.updated_at.clone(),
            deleted_at: None,
            sync_revision: revisions.next(),
        },
    );
}

fn tombstone_favourite(
    favourites: &mut BTreeMap<(Uuid, Uuid), FavouriteRecord>,
    user_id: Uuid,
    delete: &RecordDelete,
    revisions: &RevisionAllocator<'_>,
) {
    let key = (user_id, delete.record_id);
    if let Some(row) = favourites.get_mut(&key) {
        if row.deleted_at.is_none() && strictly_newer(Some(&row.updated_at), &delete.updated_at) {
            row.deleted_at = Some(delete.updated_at.clone());
            row.updated_at = delete.updated_at.clone();
            row.sync_revision = revisions.next();
        }
        return;
    }
    favourites.insert(
        key,
        FavouriteRecord {
            record_id: delete.record_id,
            station_id: None,
            added_at: delete.updated_at.clone(),
            updated_at: delete.updated_at.clone(),
            deleted_at: Some(delete.updated_at.clone()),
            sync_revision: revisions.next(),
        },
    );
}

fn merge_history(
    history: &mut BTreeMap<(Uuid, Uuid), HistoryRecord>,
    user_id: Uuid,
    upsert: &HistoryUpsert,
    revisions: &RevisionAllocator<'_>,
) {
    let key = (user_id, upsert.record_id);
    if !strictly_newer(
        history.get(&key).map(|row| &row.updated_at),
        &upsert.updated_at,
    ) {
        return;
    }
    history.insert(
        key,
        HistoryRecord {
            record_id: upsert.record_id,
            station_id: Some(upsert.station_id.clone()),
            started_at: upsert.started_at.clone(),
            last_played_at: upsert.last_played_at.clone(),
            ended_at: upsert.ended_at.clone(),
            play_duration_ms: upsert.play_duration_ms,
            metadata: upsert.metadata.clone(),
            updated_at: upsert.updated_at.clone(),
            deleted_at: None,
            sync_revision: revisions.next(),
        },
    );
}

fn tombstone_history(
    history: &mut BTreeMap<(Uuid, Uuid), HistoryRecord>,
    user_id: Uuid,
    delete: &RecordDelete,
    revisions: &RevisionAllocator<'_>,
) {
    let key = (user_id, delete.record_id);
    if let Some(row) = history.get_mut(&key) {
        if row.deleted_at.is_none() && strictly_newer(Some(&row.updated_at), &delete.updated_at) {
            row.deleted_at = Some(delete.updated_at.clone());
            row.updated_at = delete.updated_at.clone();
            row.sync_revision = revisions.next();
        }
        return;
    }
    history.insert(
        key,
        HistoryRecord {
            record_id: delete.record_id,
            station_id: None,
            started_at: delete.updated_at.clone(),
            last_played_at: delete.updated_at.clone(),
            ended_at: None,
            play_duration_ms: None,
            metadata: None,
            updated_at: delete.updated_at.clone(),
            deleted_at: Some(delete.updated_at.clone()),
            sync_revision: revisions.next(),
        },
    );
}

fn expire_history(
    history: &mut BTreeMap<(Uuid, Uuid), HistoryRecord>,
    user_id: Uuid,
    now: OffsetDateTime,
    revisions: &RevisionAllocator<'_>,
) {
    let cutoff = now - history_retention();
    let expired: Vec<(Uuid, Uuid)> = history
        .iter()
        .filter(|((owner, _), row)| {
            *owner == user_id && row.deleted_at.is_none() && row.started_at.instant() < cutoff
        })
        .map(|(key, _)| *key)
        .collect();
    for key in expired {
        if let Some(row) = history.get_mut(&key) {
            row.deleted_at = Some(row.updated_at.clone());
            row.sync_revision = revisions.next();
        }
    }
}

/// Collects one account's delta plus echoes of pushed records, ordered by revision.
///
/// Delta rows (revision past the cursor) advance the returned cursor; pushed rows the
/// client must re-learn (for example, a last-writer-wins loser) are echoed without
/// advancing the cursor, so a rejected push never loops forever. Shared by the in-memory
/// fake and the PostgreSQL store so both select identically.
pub fn delta_and_echo<'r, R: Clone + 'r>(
    rows: impl Iterator<Item = (&'r Uuid, &'r R)>,
    since_revision: u64,
    pushed: &BTreeSet<Uuid>,
    revision_of: impl Fn(&R) -> u64,
) -> (Vec<R>, u64) {
    let mut selected: BTreeMap<Uuid, &R> = BTreeMap::new();
    let mut cursor = since_revision;
    for (record_id, row) in rows {
        let revision = revision_of(row);
        if revision > since_revision || pushed.contains(record_id) {
            selected.insert(*record_id, row);
        }
        if revision > cursor {
            cursor = revision;
        }
    }
    let mut ordered: Vec<R> = selected.into_values().cloned().collect();
    ordered.sort_by_key(|row| revision_of(row));
    (ordered, cursor)
}
