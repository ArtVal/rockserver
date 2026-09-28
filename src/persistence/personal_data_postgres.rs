//! PostgreSQL persistence for account-owned personal-data sync (favourites and history).
//!
//! One transaction per synchronize call applies last-writer-wins changes, enforces the
//! bounded collection sizes and per-user history retention, and reads the delta so a
//! client's push and pull stay atomic with respect to its other devices.

use std::collections::BTreeSet;

use async_trait::async_trait;
use sqlx::{PgPool, Postgres, postgres::PgPoolOptions};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::personal_data::{
    DELETED_ACCOUNT_PURGE_DAYS, FavouriteRecord, FavouriteUpsert, HISTORY_RETENTION_DAYS,
    HistoryRecord, HistoryUpsert, MAX_FAVOURITE_RECORDS, MAX_HISTORY_RECORDS, PersonalDataError,
    PersonalDataStore, RecordDelete, RetentionSweep, SyncOutcome, SyncRequest,
    TOMBSTONE_RETENTION_DAYS, Timestamp, delta_and_echo,
};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();

/// UTC RFC 3339 projection shared by every personal-data timestamp column.
///
/// Keeping one fragment avoids five hand-copied `to_char` expressions drifting apart.
fn utc_timestamp(column: &str) -> String {
    format!("to_char({column} AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')")
}

/// PostgreSQL implementation of the personal-data persistence boundary.
#[derive(Clone, Debug)]
pub struct PostgresPersonalDataStore {
    pool: PgPool,
}

impl PostgresPersonalDataStore {
    /// Connects and applies the shared migration sequence.
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await?;
        if let Err(error) = MIGRATOR.run(&pool).await {
            pool.close().await;
            return Err(error.into());
        }
        Ok(Self { pool })
    }
    /// Reuses a caller-owned migrated pool.
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }
    /// Closes the underlying pool.
    pub async fn close(&self) {
        self.pool.close().await;
    }
}

/// Raw favourite projection; timestamps arrive as UTC RFC 3339 text.
type FavouriteRow = (Uuid, Option<String>, String, String, Option<String>, i64);
/// Raw history projection; timestamps arrive as UTC RFC 3339 text.
type HistoryRow = (
    Uuid,
    Option<String>,
    String,
    String,
    Option<String>,
    Option<i64>,
    Option<serde_json::Value>,
    String,
    Option<String>,
    i64,
);

fn favourite_columns() -> String {
    format!(
        "record_id, station_id, {}, {}, {}, sync_revision",
        utc_timestamp("added_at"),
        utc_timestamp("updated_at"),
        utc_timestamp("deleted_at")
    )
}

fn history_columns() -> String {
    format!(
        "record_id, station_id, {}, {}, {}, play_duration_ms, metadata, {}, {}, sync_revision",
        utc_timestamp("started_at"),
        utc_timestamp("last_played_at"),
        utc_timestamp("ended_at"),
        utc_timestamp("updated_at"),
        utc_timestamp("deleted_at")
    )
}

fn database(_: sqlx::Error) -> PersonalDataError {
    PersonalDataError::Database
}

fn instant(text: String) -> Result<Timestamp, PersonalDataError> {
    Timestamp::parse(text).map_err(|_| PersonalDataError::Database)
}

fn revision(raw: i64) -> Result<u64, PersonalDataError> {
    u64::try_from(raw).map_err(|_| PersonalDataError::Database)
}

fn favourite_from_row(row: FavouriteRow) -> Result<(Uuid, FavouriteRecord), PersonalDataError> {
    let (record_id, station_id, added_at, updated_at, deleted_at, sync_revision) = row;
    Ok((
        record_id,
        FavouriteRecord {
            record_id,
            station_id,
            added_at: instant(added_at)?,
            updated_at: instant(updated_at)?,
            deleted_at: deleted_at.map(instant).transpose()?,
            sync_revision: revision(sync_revision)?,
        },
    ))
}

fn history_from_row(row: HistoryRow) -> Result<(Uuid, HistoryRecord), PersonalDataError> {
    let (
        record_id,
        station_id,
        started_at,
        last_played_at,
        ended_at,
        play_duration_ms,
        metadata,
        updated_at,
        deleted_at,
        sync_revision,
    ) = row;
    Ok((
        record_id,
        HistoryRecord {
            record_id,
            station_id,
            started_at: instant(started_at)?,
            last_played_at: instant(last_played_at)?,
            ended_at: ended_at.map(instant).transpose()?,
            play_duration_ms: play_duration_ms.map(|value| value as u64),
            metadata,
            updated_at: instant(updated_at)?,
            deleted_at: deleted_at.map(instant).transpose()?,
            sync_revision: revision(sync_revision)?,
        },
    ))
}

#[async_trait]
impl PersonalDataStore for PostgresPersonalDataStore {
    async fn synchronize(
        &self,
        user_id: Uuid,
        request: SyncRequest,
    ) -> Result<SyncOutcome, PersonalDataError> {
        request.validate(OffsetDateTime::now_utc())?;
        let mut tx = self.pool.begin().await.map_err(database)?;
        for upsert in &request.favourites.upserts {
            apply_favourite_upsert(&mut tx, user_id, upsert).await?;
        }
        for delete in &request.favourites.deletes {
            apply_favourite_delete(&mut tx, user_id, delete).await?;
        }
        if !request.favourites.upserts.is_empty() {
            enforce_live_limit(
                &mut tx,
                user_id,
                "favourite_records",
                MAX_FAVOURITE_RECORDS,
                PersonalDataError::FavouriteLimitExceeded,
            )
            .await?;
        }
        for upsert in &request.history.upserts {
            apply_history_upsert(&mut tx, user_id, upsert).await?;
        }
        for delete in &request.history.deletes {
            apply_history_delete(&mut tx, user_id, delete).await?;
        }
        if !request.history.upserts.is_empty() {
            // Server-side retention first, so expired rows never count against the limit.
            expire_user_history(&mut tx, user_id).await?;
            enforce_live_limit(
                &mut tx,
                user_id,
                "history_records",
                MAX_HISTORY_RECORDS,
                PersonalDataError::HistoryLimitExceeded,
            )
            .await?;
        }
        let favourite_rows: Vec<FavouriteRow> = sqlx::query_as(&format!(
            "SELECT {} FROM favourite_records WHERE user_id=$1",
            favourite_columns()
        ))
        .bind(user_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(database)?;
        let history_rows: Vec<HistoryRow> = sqlx::query_as(&format!(
            "SELECT {} FROM history_records WHERE user_id=$1",
            history_columns()
        ))
        .bind(user_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(database)?;
        tx.commit().await.map_err(database)?;

        let pushed_favourites: BTreeSet<Uuid> =
            request.favourite_record_ids().into_iter().collect();
        let pushed_history: BTreeSet<Uuid> = request.history_record_ids().into_iter().collect();
        let favourites: Vec<(Uuid, FavouriteRecord)> = favourite_rows
            .into_iter()
            .map(favourite_from_row)
            .collect::<Result<_, _>>()?;
        let history: Vec<(Uuid, HistoryRecord)> = history_rows
            .into_iter()
            .map(history_from_row)
            .collect::<Result<_, _>>()?;
        let (favourites, favourite_cursor) = delta_and_echo(
            favourites.iter().map(|(record_id, row)| (record_id, row)),
            request.since_revision,
            &pushed_favourites,
            |row: &FavouriteRecord| row.sync_revision,
        );
        let (history, history_cursor) = delta_and_echo(
            history.iter().map(|(record_id, row)| (record_id, row)),
            request.since_revision,
            &pushed_history,
            |row: &HistoryRecord| row.sync_revision,
        );
        Ok(SyncOutcome {
            server_revision: favourite_cursor.max(history_cursor),
            favourites,
            history,
        })
    }

    async fn sweep_retention(&self) -> Result<RetentionSweep, PersonalDataError> {
        let history_expired = sqlx::query(
            "UPDATE history_records \
             SET deleted_at=now(), updated_at=now(), sync_revision=nextval('personal_data_sync_revision_seq') \
             WHERE deleted_at IS NULL AND started_at < now() - make_interval(days => $1)",
        )
        .bind(HISTORY_RETENTION_DAYS as i32)
        .execute(&self.pool)
        .await
        .map_err(database)?
        .rows_affected();
        let favourite_tombstones = prune_tombstones(&self.pool, "favourite_records").await?;
        let history_tombstones = prune_tombstones(&self.pool, "history_records").await?;
        let purged_favourites = purge_deleted_account_rows(&self.pool, "favourite_records").await?;
        let purged_history = purge_deleted_account_rows(&self.pool, "history_records").await?;
        Ok(RetentionSweep {
            history_expired: history_expired as u64,
            tombstones_removed: (favourite_tombstones + history_tombstones) as u64,
            deleted_account_rows_removed: (purged_favourites + purged_history) as u64,
        })
    }
}

/// Applies one favourite upsert; the guarded DO UPDATE makes the merge last-writer-wins.
async fn apply_favourite_upsert(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    user_id: Uuid,
    upsert: &FavouriteUpsert,
) -> Result<(), PersonalDataError> {
    sqlx::query(
        "INSERT INTO favourite_records(user_id, record_id, station_id, added_at, updated_at, sync_revision) \
         VALUES($1, $2, $3, $4::timestamptz, $5::timestamptz, nextval('personal_data_sync_revision_seq')) \
         ON CONFLICT (user_id, record_id) DO UPDATE SET \
           station_id=EXCLUDED.station_id, added_at=EXCLUDED.added_at, updated_at=EXCLUDED.updated_at, \
           deleted_at=NULL, sync_revision=EXCLUDED.sync_revision \
         WHERE favourite_records.updated_at < EXCLUDED.updated_at",
    )
    .bind(user_id)
    .bind(upsert.record_id)
    .bind(&upsert.station_id)
    .bind(upsert.added_at.as_rfc3339())
    .bind(upsert.updated_at.as_rfc3339())
    .execute(&mut **tx)
    .await
    .map_err(database)?;
    Ok(())
}

/// Applies one favourite deletion; unknown records get a station-less bare tombstone.
async fn apply_favourite_delete(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    user_id: Uuid,
    delete: &RecordDelete,
) -> Result<(), PersonalDataError> {
    sqlx::query(
        "INSERT INTO favourite_records(user_id, record_id, station_id, added_at, updated_at, deleted_at, sync_revision) \
         VALUES($1, $2, NULL, $3::timestamptz, $3::timestamptz, $3::timestamptz, nextval('personal_data_sync_revision_seq')) \
         ON CONFLICT (user_id, record_id) DO UPDATE SET \
           updated_at=EXCLUDED.updated_at, deleted_at=EXCLUDED.deleted_at, sync_revision=EXCLUDED.sync_revision \
         WHERE favourite_records.updated_at < EXCLUDED.updated_at AND favourite_records.deleted_at IS NULL",
    )
    .bind(user_id)
    .bind(delete.record_id)
    .bind(delete.updated_at.as_rfc3339())
    .execute(&mut **tx)
    .await
    .map_err(database)?;
    Ok(())
}

/// Applies one history upsert; the guarded DO UPDATE makes the merge last-writer-wins.
async fn apply_history_upsert(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    user_id: Uuid,
    upsert: &HistoryUpsert,
) -> Result<(), PersonalDataError> {
    sqlx::query(
        "INSERT INTO history_records(user_id, record_id, station_id, started_at, last_played_at, ended_at, play_duration_ms, metadata, updated_at, sync_revision) \
         VALUES($1, $2, $3, $4::timestamptz, $5::timestamptz, $6::timestamptz, $7, $8::jsonb, $9::timestamptz, nextval('personal_data_sync_revision_seq')) \
         ON CONFLICT (user_id, record_id) DO UPDATE SET \
           station_id=EXCLUDED.station_id, started_at=EXCLUDED.started_at, last_played_at=EXCLUDED.last_played_at, \
           ended_at=EXCLUDED.ended_at, play_duration_ms=EXCLUDED.play_duration_ms, metadata=EXCLUDED.metadata, \
           updated_at=EXCLUDED.updated_at, deleted_at=NULL, sync_revision=EXCLUDED.sync_revision \
         WHERE history_records.updated_at < EXCLUDED.updated_at",
    )
    .bind(user_id)
    .bind(upsert.record_id)
    .bind(&upsert.station_id)
    .bind(upsert.started_at.as_rfc3339())
    .bind(upsert.last_played_at.as_rfc3339())
    .bind(upsert.ended_at.as_ref().map(Timestamp::as_rfc3339))
    .bind(upsert.play_duration_ms.map(|value| value as i64))
    .bind(upsert.metadata.as_ref())
    .bind(upsert.updated_at.as_rfc3339())
    .execute(&mut **tx)
    .await
    .map_err(database)?;
    Ok(())
}

/// Applies one history deletion; unknown records get a station-less bare tombstone.
async fn apply_history_delete(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    user_id: Uuid,
    delete: &RecordDelete,
) -> Result<(), PersonalDataError> {
    sqlx::query(
        "INSERT INTO history_records(user_id, record_id, station_id, started_at, last_played_at, ended_at, play_duration_ms, metadata, updated_at, deleted_at, sync_revision) \
         VALUES($1, $2, NULL, $3::timestamptz, $3::timestamptz, NULL, NULL, NULL, $3::timestamptz, $3::timestamptz, nextval('personal_data_sync_revision_seq')) \
         ON CONFLICT (user_id, record_id) DO UPDATE SET \
           updated_at=EXCLUDED.updated_at, deleted_at=EXCLUDED.deleted_at, sync_revision=EXCLUDED.sync_revision \
         WHERE history_records.updated_at < EXCLUDED.updated_at AND history_records.deleted_at IS NULL",
    )
    .bind(user_id)
    .bind(delete.record_id)
    .bind(delete.updated_at.as_rfc3339())
    .execute(&mut **tx)
    .await
    .map_err(database)?;
    Ok(())
}

/// Rejects the whole batch when one collection would pass its bounded size.
async fn enforce_live_limit(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    user_id: Uuid,
    table: &str,
    max_records: usize,
    exceeded: PersonalDataError,
) -> Result<(), PersonalDataError> {
    let live: i64 = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM {table} WHERE user_id=$1 AND deleted_at IS NULL"
    ))
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(database)?;
    if live > max_records as i64 {
        return Err(exceeded);
    }
    Ok(())
}

/// Removes tombstoned rows that passed the tombstone window from one table.
async fn prune_tombstones(pool: &PgPool, table: &str) -> Result<u64, PersonalDataError> {
    let sql = format!(
        "DELETE FROM {table} \
         WHERE deleted_at IS NOT NULL AND deleted_at < now() - make_interval(days => $1)"
    );
    Ok(sqlx::query(&sql)
        .bind(TOMBSTONE_RETENTION_DAYS as i32)
        .execute(pool)
        .await
        .map_err(database)?
        .rows_affected())
}

/// Purges one table's rows of accounts deleted past the operator grace window.
async fn purge_deleted_account_rows(pool: &PgPool, table: &str) -> Result<u64, PersonalDataError> {
    let sql = format!(
        "DELETE FROM {table} p USING users u WHERE p.user_id=u.id \
         AND u.status='deleted' AND u.deleted_at < now() - make_interval(days => $1)"
    );
    Ok(sqlx::query(&sql)
        .bind(DELETED_ACCOUNT_PURGE_DAYS as i32)
        .execute(pool)
        .await
        .map_err(database)?
        .rows_affected())
}

/// Tomstones one account's history rows that passed the retention window.
async fn expire_user_history(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<(), PersonalDataError> {
    sqlx::query(
        "UPDATE history_records \
         SET deleted_at=now(), updated_at=now(), sync_revision=nextval('personal_data_sync_revision_seq') \
         WHERE user_id=$1 AND deleted_at IS NULL \
           AND started_at < now() - make_interval(days => $2)",
    )
    .bind(user_id)
    .bind(HISTORY_RETENTION_DAYS as i32)
    .execute(&mut **tx)
    .await
    .map_err(database)?;
    Ok(())
}
