//! Domain models, limits, validation, and traits for personal-data sync.

use std::collections::BTreeSet;

use async_trait::async_trait;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
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
