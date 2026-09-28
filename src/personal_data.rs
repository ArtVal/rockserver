//! Account-owned favourites and playback-history personal-data sync domain (RM-012-A).
//!
//! The server is the authoritative merge point for the personal data that RM-007-A clients
//! already hold locally. Records are identified by client-generated `record_id` UUIDs and
//! merged last-writer-wins on `updated_at` (equal instants keep the stored row); a shared
//! monotonic revision sequence gives each account a delta cursor, and tombstones carry
//! deletions to devices that were offline when the deletion happened. This module owns the
//! validated models, limits, merge semantics, and the persistence contract; SQL lives in
//! `persistence`, transport mapping in `http`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use serde::{Deserialize, Serialize, de::Deserializer, ser::Serializer};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

/// Maximum number of live (non-tombstoned) favourite records per account.
pub const MAX_FAVOURITE_RECORDS: usize = 500;
/// Maximum number of live (non-tombstoned) history records per account.
pub const MAX_HISTORY_RECORDS: usize = 500;
/// Maximum upsert or delete entries per collection in one sync request.
pub const MAX_SYNC_BATCH_ITEMS: usize = 300;
/// How long live history entries stay retrievable past their `started_at`.
pub const HISTORY_RETENTION_DAYS: i64 = 90;
/// How long tombstones are kept so offline devices still learn about deletions.
pub const TOMBSTONE_RETENTION_DAYS: i64 = 90;
/// How long after account deletion personal rows are purged for good.
pub const DELETED_ACCOUNT_PURGE_DAYS: i64 = 30;
/// Tolerated client clock advance over server time before a timestamp is rejected.
pub const MAX_CLIENT_CLOCK_SKEW_SECONDS: i64 = 5 * 60;
/// Hard ceiling for one optional history `metadata` JSON object.
pub const MAX_METADATA_BYTES: usize = 4096;
/// Hard ceiling for one playback duration in milliseconds (30 days).
pub const MAX_PLAY_DURATION_MS: u64 = 30 * 24 * 60 * 60 * 1000;
/// Maximum station identifier length; station IDs stay opaque to this domain.
pub const MAX_STATION_ID_LEN: usize = 256;

/// Domain failure for the personal-data boundary; the HTTP layer maps variants to the
/// public error envelope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PersonalDataError {
    /// Input failed validation; the value names the offending request field.
    Validation(&'static str),
    /// Applying the batch would exceed the bounded favourites collection.
    FavouriteLimitExceeded,
    /// Applying the batch would exceed the bounded history collection.
    HistoryLimitExceeded,
    /// The persistence backend failed without exposing raw error details.
    Database,
}

impl std::fmt::Display for PersonalDataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Validation(field) => write!(f, "personal data validation failed: {field}"),
            Self::FavouriteLimitExceeded => write!(f, "favourites collection limit exceeded"),
            Self::HistoryLimitExceeded => write!(f, "history collection limit exceeded"),
            Self::Database => write!(f, "personal data persistence unavailable"),
        }
    }
}

impl std::error::Error for PersonalDataError {}

/// Validated RFC 3339 instant that preserves the client's textual form for persistence.
///
/// Comparisons use the parsed instant, never the text, so mixed offset formats stay correct.
#[derive(Clone, Debug)]
pub struct Timestamp {
    text: String,
    instant: OffsetDateTime,
}

impl Timestamp {
    /// Parses and validates an RFC 3339 timestamp.
    pub fn parse(value: impl Into<String>) -> Result<Self, PersonalDataError> {
        let text = value.into();
        let instant = OffsetDateTime::parse(&text, &Rfc3339)
            .map_err(|_| PersonalDataError::Validation("timestamp"))?;
        Ok(Self { text, instant })
    }
    /// Builds a timestamp from an already-parsed instant (tests and retention sweeps).
    pub fn from_instant(instant: OffsetDateTime) -> Self {
        let text = instant
            .format(&Rfc3339)
            .expect("RFC 3339 formatting of a valid instant");
        Self { text, instant }
    }
    /// Returns the parsed UTC-comparable instant.
    pub fn instant(&self) -> OffsetDateTime {
        self.instant
    }
    /// Returns the validated RFC 3339 representation for persistence bindings.
    pub fn as_rfc3339(&self) -> &str {
        &self.text
    }
}

impl PartialEq for Timestamp {
    fn eq(&self, other: &Self) -> bool {
        self.instant == other.instant
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.text)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(text).map_err(serde::de::Error::custom)
    }
}

/// Server projection of one client favourite record.
///
/// Tombstones keep their last known station; only deleting a never-synced record carries
/// no station data, because a deletion marker identifies the record, not the station.
#[derive(Clone, Debug, PartialEq)]
pub struct FavouriteRecord {
    /// Canonical client-generated record identifier.
    pub record_id: Uuid,
    /// Opaque canonical station identifier, absent on tombstones.
    pub station_id: Option<String>,
    /// When the client added the favourite.
    pub added_at: Timestamp,
    /// Last-writer-wins arbitration instant.
    pub updated_at: Timestamp,
    /// Deletion marker kept for offline-device propagation.
    pub deleted_at: Option<Timestamp>,
    /// Shared monotonic revision stamped when this row last changed.
    pub sync_revision: u64,
}

/// Server projection of one client playback-history record; tombstones behave like
/// [`FavouriteRecord`] tombstones and keep their last known fields.
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryRecord {
    /// Canonical client-generated record identifier.
    pub record_id: Uuid,
    /// Opaque canonical station identifier, absent on tombstones.
    pub station_id: Option<String>,
    /// When the listening session started.
    pub started_at: Timestamp,
    /// When the station was last played within the session.
    pub last_played_at: Timestamp,
    /// Optional session end instant.
    pub ended_at: Option<Timestamp>,
    /// Optional non-negative playback duration.
    pub play_duration_ms: Option<u64>,
    /// Optional bounded display-only metadata object.
    pub metadata: Option<serde_json::Value>,
    /// Last-writer-wins arbitration instant.
    pub updated_at: Timestamp,
    /// Deletion marker kept for offline-device propagation.
    pub deleted_at: Option<Timestamp>,
    /// Shared monotonic revision stamped when this row last changed.
    pub sync_revision: u64,
}

/// Client favourite record offered for last-writer-wins merge.
#[derive(Clone, Debug, PartialEq)]
pub struct FavouriteUpsert {
    /// Client-generated record identifier, already canonical.
    pub record_id: Uuid,
    /// Opaque canonical station identifier.
    pub station_id: String,
    /// When the client added the favourite.
    pub added_at: Timestamp,
    /// Last-writer-wins arbitration instant.
    pub updated_at: Timestamp,
}

/// Client playback-history record offered for last-writer-wins merge.
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryUpsert {
    /// Client-generated record identifier, already canonical.
    pub record_id: Uuid,
    /// Opaque canonical station identifier.
    pub station_id: String,
    /// When the listening session started.
    pub started_at: Timestamp,
    /// When the station was last played within the session.
    pub last_played_at: Timestamp,
    /// Optional session end instant.
    pub ended_at: Option<Timestamp>,
    /// Optional non-negative playback duration.
    pub play_duration_ms: Option<u64>,
    /// Optional bounded display-only metadata object.
    pub metadata: Option<serde_json::Value>,
    /// Last-writer-wins arbitration instant.
    pub updated_at: Timestamp,
}

/// Client deletion marker; `updated_at` lets the tombstone participate in last-writer-wins
/// so a later replay of an older upsert cannot resurrect the record.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordDelete {
    /// Client-generated record identifier, already canonical.
    pub record_id: Uuid,
    /// When the client deleted the record.
    pub updated_at: Timestamp,
}

/// One collection's pushed changes.
#[derive(Clone, Debug, PartialEq)]
pub struct CollectionChanges<U> {
    /// Records offered for last-writer-wins merge.
    pub upserts: Vec<U>,
    /// Deletions to apply as tombstones.
    pub deletes: Vec<RecordDelete>,
}

impl<U> Default for CollectionChanges<U> {
    fn default() -> Self {
        Self {
            upserts: Vec::new(),
            deletes: Vec::new(),
        }
    }
}

/// Account synchronization request; record identifiers and timestamps are already validated
/// by construction, `validate` checks the cross-record rules.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SyncRequest {
    /// Delta cursor from the client's last successful sync; zero means full snapshot.
    pub since_revision: u64,
    /// Favourite changes to apply.
    pub favourites: CollectionChanges<FavouriteUpsert>,
    /// History changes to apply.
    pub history: CollectionChanges<HistoryUpsert>,
}

/// Result of one account synchronization: the new cursor and every record the client must see.
#[derive(Clone, Debug, PartialEq)]
pub struct SyncOutcome {
    /// Cursor the client must persist and send as the next `since_revision`.
    pub server_revision: u64,
    /// Favourite delta plus echoes of pushed records, ordered by revision.
    pub favourites: Vec<FavouriteRecord>,
    /// History delta plus echoes of pushed records, ordered by revision.
    pub history: Vec<HistoryRecord>,
}

/// Counters reported by one fleet-wide retention sweep.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RetentionSweep {
    /// Live history entries tombstoned because they passed the retention window.
    pub history_expired: u64,
    /// Tombstoned rows physically removed after the tombstone window.
    pub tombstones_removed: u64,
    /// Rows purged because their account was deleted longer than the purge grace ago.
    pub deleted_account_rows_removed: u64,
}

/// Parsing and normalization helpers shared by the transport layer.
pub struct Canonical;

impl Canonical {
    /// Parses a client record identifier and returns its canonical UUID form.
    pub fn record_id(raw: &str) -> Result<Uuid, PersonalDataError> {
        Uuid::parse_str(raw).map_err(|_| PersonalDataError::Validation("record_id"))
    }
    /// Validates one opaque station identifier; unresolved references stay acceptable.
    pub fn station_id(raw: &str) -> Result<String, PersonalDataError> {
        (!raw.is_empty()
            && raw.chars().count() <= MAX_STATION_ID_LEN
            && !raw.chars().any(char::is_control))
        .then(|| raw.to_owned())
        .ok_or(PersonalDataError::Validation("station_id"))
    }
}

impl SyncRequest {
    /// Validates batch caps and every record against server time.
    ///
    /// `now` is the server's current instant; client timestamps more than the tolerated
    /// clock skew in the future are rejected so a bad clock cannot poison
    /// last-writer-wins arbitration forever.
    pub fn validate(&self, now: OffsetDateTime) -> Result<(), PersonalDataError> {
        for (field, len) in [
            ("favourites.upserts", self.favourites.upserts.len()),
            ("favourites.deletes", self.favourites.deletes.len()),
            ("history.upserts", self.history.upserts.len()),
            ("history.deletes", self.history.deletes.len()),
        ] {
            if len > MAX_SYNC_BATCH_ITEMS {
                return Err(PersonalDataError::Validation(field));
            }
        }
        for (collection, ids) in [
            ("favourites", self.favourite_record_ids()),
            ("history", self.history_record_ids()),
        ] {
            if ids.len() != ids.iter().collect::<BTreeSet<_>>().len() {
                return Err(PersonalDataError::Validation(collection));
            }
        }
        for upsert in &self.favourites.upserts {
            upsert.validate(now)?;
        }
        for upsert in &self.history.upserts {
            upsert.validate(now)?;
        }
        Ok(())
    }

    /// Every pushed favourite record identifier; stores echo these back unconditionally.
    pub fn favourite_record_ids(&self) -> Vec<Uuid> {
        collection_record_ids(
            self.favourites.upserts.iter().map(|u| u.record_id),
            self.favourites.deletes.iter().map(|d| d.record_id),
        )
    }

    /// Every pushed history record identifier; stores echo these back unconditionally.
    pub fn history_record_ids(&self) -> Vec<Uuid> {
        collection_record_ids(
            self.history.upserts.iter().map(|u| u.record_id),
            self.history.deletes.iter().map(|d| d.record_id),
        )
    }
}

fn collection_record_ids(
    upserts: impl Iterator<Item = Uuid>,
    deletes: impl Iterator<Item = Uuid>,
) -> Vec<Uuid> {
    upserts.chain(deletes).collect()
}

impl FavouriteUpsert {
    /// Validates one favourite against server time.
    pub fn validate(&self, now: OffsetDateTime) -> Result<(), PersonalDataError> {
        Canonical::station_id(&self.station_id).map(|_| ())?;
        validate_timestamp(&self.added_at, now)?;
        validate_timestamp(&self.updated_at, now)?;
        Ok(())
    }
}

impl HistoryUpsert {
    /// Validates one history entry against server time, including intra-record ordering.
    pub fn validate(&self, now: OffsetDateTime) -> Result<(), PersonalDataError> {
        Canonical::station_id(&self.station_id).map(|_| ())?;
        validate_timestamp(&self.started_at, now)?;
        validate_timestamp(&self.last_played_at, now)?;
        if self.last_played_at.instant() < self.started_at.instant() {
            return Err(PersonalDataError::Validation("history.last_played_at"));
        }
        if let Some(ended_at) = &self.ended_at {
            validate_timestamp(ended_at, now)?;
            if ended_at.instant() < self.started_at.instant() {
                return Err(PersonalDataError::Validation("history.ended_at"));
            }
        }
        if self
            .play_duration_ms
            .is_some_and(|duration| duration > MAX_PLAY_DURATION_MS)
        {
            return Err(PersonalDataError::Validation("history.play_duration_ms"));
        }
        validate_metadata(&self.metadata)?;
        validate_timestamp(&self.updated_at, now)?;
        Ok(())
    }
}

fn validate_timestamp(value: &Timestamp, now: OffsetDateTime) -> Result<(), PersonalDataError> {
    let skew = time::Duration::seconds(MAX_CLIENT_CLOCK_SKEW_SECONDS);
    (value.instant() <= now + skew)
        .then_some(())
        .ok_or(PersonalDataError::Validation("timestamp"))
}

fn validate_metadata(metadata: &Option<serde_json::Value>) -> Result<(), PersonalDataError> {
    let Some(metadata) = metadata else {
        return Ok(());
    };
    let bounded = metadata.is_object()
        && serde_json::to_vec(metadata)
            .map(|bytes| bytes.len() <= MAX_METADATA_BYTES)
            .unwrap_or(false);
    bounded
        .then_some(())
        .ok_or(PersonalDataError::Validation("history.metadata"))
}

/// Owner-scoped personal-data persistence boundary.
///
/// Implementations must validate the request, apply changes, enforce collection limits and
/// retention, and read the delta in one transaction so a client's push and pull stay atomic
/// with respect to other devices of the same account.
#[async_trait]
pub trait PersonalDataStore: Send + Sync {
    /// Applies last-writer-wins changes for one account and returns its delta.
    async fn synchronize(
        &self,
        user_id: Uuid,
        request: SyncRequest,
    ) -> Result<SyncOutcome, PersonalDataError>;
    /// Applies fleet-wide retention: history expiry, tombstone GC, and deleted-account purge.
    async fn sweep_retention(&self) -> Result<RetentionSweep, PersonalDataError>;
}

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

fn history_retention() -> time::Duration {
    time::Duration::days(HISTORY_RETENTION_DAYS)
}

fn tombstone_retention() -> time::Duration {
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
pub(crate) fn delta_and_echo<'r, R: Clone + 'r>(
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

#[cfg(test)]
mod tests {
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
}
