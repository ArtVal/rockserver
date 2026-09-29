//! Background import job coordinator and PostgreSQL persistence for station icons.
//!
//! Manages job snapshots, claim-and-process concurrency, manual icon overrides,
//! and transition reporting for station-icon imports.

use std::sync::Arc;

use sqlx::PgPool;
use uuid::Uuid;

use super::domain::{
    IconJobProgress, IconSourceFetcher, IconStorage, IconStorageError, IconStorageKey,
    IconValidationError, ManualIconError, PreparedIcon, ReadyIcon, prepare_icon,
};
use super::fetcher::SafeIconFetcher;

/// PostgreSQL coordinator for the one administrator-started station-icon job.
#[derive(Clone)]
pub struct IconImportCoordinator {
    pool: PgPool,
    storage: Arc<dyn IconStorage>,
    fetcher: Arc<dyn IconSourceFetcher>,
}

impl IconImportCoordinator {
    /// Reuses the production pool and prepared storage; it never performs work until `run` is called.
    pub fn new(pool: PgPool, storage: Arc<dyn IconStorage>) -> Result<Self, IconStorageError> {
        Ok(Self {
            pool,
            storage,
            fetcher: Arc::new(SafeIconFetcher::new()?),
        })
    }

    /// Marks a prior interrupted process-local run terminal without fetching any source.
    pub async fn interrupt_running(&self) -> Result<(), IconStorageError> {
        sqlx::query("UPDATE station_icon_jobs SET status = 'interrupted', finished_at = now() WHERE status = 'running'")
            .execute(&self.pool).await.map_err(|_| IconStorageError::Unavailable)?;
        Ok(())
    }

    /// Creates the one active job and snapshots its eligible station IDs for stable progress.
    pub async fn start(&self) -> Result<IconJobProgress, IconStorageError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| IconStorageError::Unavailable)?;
        let id = Uuid::new_v4();
        let inserted = sqlx::query("INSERT INTO station_icon_jobs (id, status) VALUES ($1, 'running') ON CONFLICT DO NOTHING")
            .bind(id).execute(&mut *transaction).await.map_err(|_| IconStorageError::Unavailable)?;
        if inserted.rows_affected() != 1 {
            return Err(IconStorageError::Unavailable);
        }
        let selected = sqlx::query("INSERT INTO station_icon_job_items (job_id, station_id) SELECT $1, id FROM stations s LEFT JOIN station_icons i ON i.station_id = s.id WHERE s.retired_at IS NULL AND COALESCE(i.manual_override, false) = false AND (i.status IS NULL OR i.status IN ('pending', 'missing', 'retryable_error') OR i.refresh_needed) ON CONFLICT DO NOTHING")
            .bind(id).execute(&mut *transaction).await.map_err(|_| IconStorageError::Unavailable)?.rows_affected();
        sqlx::query("UPDATE station_icon_jobs SET selected_count = $2 WHERE id = $1")
            .bind(id)
            .bind(i32::try_from(selected).map_err(|_| IconStorageError::Unavailable)?)
            .execute(&mut *transaction)
            .await
            .map_err(|_| IconStorageError::Unavailable)?;
        transaction
            .commit()
            .await
            .map_err(|_| IconStorageError::Unavailable)?;
        self.progress(id)
            .await?
            .ok_or(IconStorageError::Unavailable)
    }

    /// Reads the current persisted job counters without exposing source URLs or bytes.
    pub async fn progress(&self, id: Uuid) -> Result<Option<IconJobProgress>, IconStorageError> {
        sqlx::query_as::<_, IconJobProgressRow>("SELECT id, status, selected_count AS selected, processed_count AS processed, ready_count AS ready, missing_count AS missing, retryable_error_count AS retryable_error, permanent_error_count AS permanent_error, skipped_count AS skipped FROM station_icon_jobs WHERE id = $1")
            .bind(id).fetch_optional(&self.pool).await.map_err(|_| IconStorageError::Unavailable).map(|row| row.map(Into::into))
    }

    /// Returns the latest persisted job so a reloaded administrator console can resume progress.
    pub async fn latest_progress(&self) -> Result<Option<IconJobProgress>, IconStorageError> {
        sqlx::query_as::<_, IconJobProgressRow>("SELECT id, status, selected_count AS selected, processed_count AS processed, ready_count AS ready, missing_count AS missing, retryable_error_count AS retryable_error, permanent_error_count AS permanent_error, skipped_count AS skipped FROM station_icon_jobs ORDER BY started_at DESC LIMIT 1")
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| IconStorageError::Unavailable)
            .map(|row| row.map(Into::into))
    }

    /// Resolves only a ready metadata row whose content-addressed file is still present.
    pub async fn ready_icon(
        &self,
        station_id: &str,
    ) -> Result<Option<ReadyIcon>, IconStorageError> {
        let row = sqlx::query_as::<_, ReadyIconRow>("SELECT storage_key, content_hash FROM station_icons WHERE station_id = $1 AND status = 'ready'")
            .bind(station_id).fetch_optional(&self.pool).await.map_err(|_| IconStorageError::Unavailable)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let key = IconStorageKey::parse(&row.storage_key)?;
        let hash: [u8; 32] = row
            .content_hash
            .try_into()
            .map_err(|_| IconStorageError::Unavailable)?;
        self.storage.get(&key).await.map(|bytes| {
            bytes.map(|bytes| ReadyIcon {
                bytes,
                content_hash: hash,
            })
        })
    }

    /// Validates and publishes a manual override without replacing the preserved source metadata.
    ///
    /// The artifact is stored before its ready metadata is committed, so an unavailable database
    /// can at worst leave an unreferenced content-addressed file, never a partial ready row.
    pub async fn replace_manual(
        &self,
        station_id: &str,
        source: &[u8],
    ) -> Result<(), ManualIconError> {
        let icon = prepare_icon(source).map_err(ManualIconError::Validation)?;
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM stations WHERE id = $1 AND retired_at IS NULL)",
        )
        .bind(station_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| ManualIconError::Unavailable)?;
        if !exists {
            return Err(ManualIconError::NotFound);
        }
        let key = IconStorageKey::from_hash(&icon.content_hash);
        self.storage
            .put_atomic(&key, &icon.bytes)
            .await
            .map_err(|_| ManualIconError::Unavailable)?;
        sqlx::query("INSERT INTO station_icons (station_id, storage_key, content_type, byte_size, width, height, content_hash, status, manual_override, refresh_needed) VALUES ($1, $2, 'image/webp', $3, $4, $5, $6, 'ready', true, false) ON CONFLICT (station_id) DO UPDATE SET storage_key = EXCLUDED.storage_key, content_type = EXCLUDED.content_type, byte_size = EXCLUDED.byte_size, width = EXCLUDED.width, height = EXCLUDED.height, content_hash = EXCLUDED.content_hash, status = 'ready', manual_override = true, refresh_needed = false, retry_after = NULL, last_error_code = NULL, updated_at = now()")
            .bind(station_id)
            .bind(key.as_str())
            .bind(i32::try_from(icon.bytes.len()).map_err(|_| ManualIconError::Unavailable)?)
            .bind(i32::try_from(icon.width).map_err(|_| ManualIconError::Unavailable)?)
            .bind(i32::try_from(icon.height).map_err(|_| ManualIconError::Unavailable)?)
            .bind(icon.content_hash.as_slice())
            .execute(&self.pool)
            .await
            .map_err(|_| ManualIconError::Unavailable)?;
        Ok(())
    }

    /// Detaches a manual override without deleting a possibly shared content-addressed artifact.
    ///
    /// A later explicit import may use the preserved automatic source. Orphan cleanup is an
    /// independent operator action so a remove can never break another station sharing the hash.
    pub async fn remove_manual(&self, station_id: &str) -> Result<(), ManualIconError> {
        let changed = sqlx::query("UPDATE station_icons SET storage_key = NULL, content_type = NULL, byte_size = NULL, width = NULL, height = NULL, content_hash = NULL, status = 'missing', manual_override = false, refresh_needed = true, retry_after = NULL, last_error_code = NULL, updated_at = now() WHERE station_id = $1 AND manual_override = true")
            .bind(station_id)
            .execute(&self.pool)
            .await
            .map_err(|_| ManualIconError::Unavailable)?;
        (changed.rows_affected() == 1)
            .then_some(())
            .ok_or(ManualIconError::NotFound)
    }

    /// Processes the snapshot one item at a time; callers run this only in a detached server task.
    pub async fn run(&self, id: Uuid) {
        while let Ok(Some(item)) = self.claim_item(id).await {
            let outcome = match resolve_item_plan(self.fetcher.as_ref(), &item).await {
                ItemPlan::Missing => "missing",
                ItemPlan::Retryable => "retryable_error",
                ItemPlan::Permanent => "permanent_error",
                ItemPlan::Ready {
                    icon,
                    source_url,
                    source_priority,
                } => match self
                    .publish(&item.station_id, &source_url, source_priority, &icon)
                    .await
                {
                    Ok(()) => "ready",
                    Err(_) => "retryable_error",
                },
            };
            let _ = self.finish_item(id, &item.station_id, outcome).await;
        }
        let _ = sqlx::query("UPDATE station_icon_jobs SET status = 'completed', finished_at = now() WHERE id = $1 AND status = 'running'")
            .bind(id).execute(&self.pool).await;
    }

    async fn claim_item(&self, id: Uuid) -> Result<Option<IconJobItem>, IconStorageError> {
        sqlx::query_as::<_, IconJobItem>("WITH next_item AS (SELECT station_id FROM station_icon_job_items WHERE job_id = $1 AND status = 'pending' ORDER BY station_id FOR UPDATE SKIP LOCKED LIMIT 1) UPDATE station_icon_job_items item SET status = 'processing' FROM next_item WHERE item.job_id = $1 AND item.station_id = next_item.station_id RETURNING item.station_id, (SELECT source_url FROM station_icons WHERE station_id = item.station_id) AS source_url, (SELECT homepage_url FROM stations WHERE id = item.station_id) AS homepage_url")
            .bind(id).fetch_optional(&self.pool).await.map_err(|_| IconStorageError::Unavailable)
    }

    /// Upserts ready metadata so a station without a prior metadata row becomes ready atomically.
    ///
    /// A manual override row is never touched: the job cannot have selected it, but a concurrent
    /// administrator upload while the job runs must keep its explicit artifact and source.
    async fn publish(
        &self,
        station_id: &str,
        source_url: &str,
        source_priority: i16,
        icon: &PreparedIcon,
    ) -> Result<(), IconStorageError> {
        let key = IconStorageKey::from_hash(&icon.content_hash);
        self.storage.put_atomic(&key, &icon.bytes).await?;
        sqlx::query("INSERT INTO station_icons (station_id, source_url, source_priority, storage_key, content_type, byte_size, width, height, content_hash, status, refresh_needed) VALUES ($1, $2, $3, $4, 'image/webp', $5, $6, $7, $8, 'ready', false) ON CONFLICT (station_id) DO UPDATE SET source_url = CASE WHEN station_icons.manual_override THEN station_icons.source_url ELSE EXCLUDED.source_url END, source_priority = CASE WHEN station_icons.manual_override THEN station_icons.source_priority ELSE EXCLUDED.source_priority END, storage_key = CASE WHEN station_icons.manual_override THEN station_icons.storage_key ELSE EXCLUDED.storage_key END, content_type = CASE WHEN station_icons.manual_override THEN station_icons.content_type ELSE EXCLUDED.content_type END, byte_size = CASE WHEN station_icons.manual_override THEN station_icons.byte_size ELSE EXCLUDED.byte_size END, width = CASE WHEN station_icons.manual_override THEN station_icons.width ELSE EXCLUDED.width END, height = CASE WHEN station_icons.manual_override THEN station_icons.height ELSE EXCLUDED.height END, content_hash = CASE WHEN station_icons.manual_override THEN station_icons.content_hash ELSE EXCLUDED.content_hash END, status = CASE WHEN station_icons.manual_override THEN station_icons.status ELSE 'ready' END, refresh_needed = CASE WHEN station_icons.manual_override THEN station_icons.refresh_needed ELSE false END, retry_after = CASE WHEN station_icons.manual_override THEN station_icons.retry_after ELSE NULL END, last_error_code = CASE WHEN station_icons.manual_override THEN station_icons.last_error_code ELSE NULL END, updated_at = now()")
            .bind(station_id).bind(source_url).bind(source_priority).bind(key.as_str())
            .bind(i32::try_from(icon.bytes.len()).map_err(|_| IconStorageError::Unavailable)?)
            .bind(i32::try_from(icon.width).map_err(|_| IconStorageError::Unavailable)?)
            .bind(i32::try_from(icon.height).map_err(|_| IconStorageError::Unavailable)?)
            .bind(icon.content_hash.as_slice())
            .execute(&self.pool)
            .await
            .map_err(|_| IconStorageError::Unavailable)?;
        Ok(())
    }

    async fn finish_item(
        &self,
        id: Uuid,
        station_id: &str,
        outcome: &str,
    ) -> Result<(), IconStorageError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| IconStorageError::Unavailable)?;
        sqlx::query("UPDATE station_icon_job_items SET status = $3 WHERE job_id = $1 AND station_id = $2 AND status = 'processing'")
            .bind(id).bind(station_id).bind(outcome).execute(&mut *transaction).await.map_err(|_| IconStorageError::Unavailable)?;
        sqlx::query("UPDATE station_icon_jobs SET processed_count = processed_count + 1, ready_count = ready_count + CASE WHEN $2 = 'ready' THEN 1 ELSE 0 END, missing_count = missing_count + CASE WHEN $2 = 'missing' THEN 1 ELSE 0 END, retryable_error_count = retryable_error_count + CASE WHEN $2 = 'retryable_error' THEN 1 ELSE 0 END, permanent_error_count = permanent_error_count + CASE WHEN $2 = 'permanent_error' THEN 1 ELSE 0 END WHERE id = $1")
            .bind(id).bind(outcome).execute(&mut *transaction).await.map_err(|_| IconStorageError::Unavailable)?;
        transaction
            .commit()
            .await
            .map_err(|_| IconStorageError::Unavailable)
    }
}

#[derive(sqlx::FromRow)]
struct IconJobProgressRow {
    id: Uuid,
    status: String,
    selected: i32,
    processed: i32,
    ready: i32,
    missing: i32,
    retryable_error: i32,
    permanent_error: i32,
    skipped: i32,
}

impl From<IconJobProgressRow> for IconJobProgress {
    fn from(row: IconJobProgressRow) -> Self {
        Self {
            id: row.id,
            status: row.status,
            selected: row.selected,
            processed: row.processed,
            ready: row.ready,
            missing: row.missing,
            retryable_error: row.retryable_error,
            permanent_error: row.permanent_error,
            skipped: row.skipped,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(super) struct IconJobItem {
    pub(super) station_id: String,
    pub(super) source_url: Option<String>,
    pub(super) homepage_url: Option<String>,
}

/// One item's fully resolved terminal plan before any persistence side effect.
#[derive(Debug, PartialEq)]
pub(super) enum ItemPlan {
    /// No automatic source exists for the station.
    Missing,
    /// A bounded source operation failed transiently and may succeed on a later job.
    Retryable,
    /// The source is unusable and will not heal without new metadata.
    Permanent,
    /// A normalized artifact plus the automatic source that produced it.
    Ready {
        icon: PreparedIcon,
        source_url: String,
        source_priority: i16,
    },
}

/// Resolves one item's outcome without touching persistence so the decision stays testable.
///
/// Source priority follows the roadmap order: an explicit catalog icon URL wins (2), favicon
/// candidates discovered on the station homepage are the fallback (1), and neither leaves the
/// item missing. Homepage candidates are tried in order, so a dead declared link cannot hide
/// a living root icon. Only transport-level failures are retryable; unusable URLs (invalid,
/// non-public, unsuccessfully answered) and sources exceeding a fixed byte limit are
/// permanent so a dead address or oversized payload cannot loop forever.
pub(super) async fn resolve_item_plan(
    fetcher: &dyn IconSourceFetcher,
    item: &IconJobItem,
) -> ItemPlan {
    let (sources, source_priority) = match item.source_url.as_deref() {
        Some(source) => (vec![source.to_owned()], 2),
        None => match item.homepage_url.as_deref() {
            None => return ItemPlan::Missing,
            Some(homepage) => match fetcher.discover_homepage_icons(homepage).await {
                Ok(candidates) if candidates.is_empty() => return ItemPlan::Permanent,
                Ok(candidates) => (candidates, 1),
                Err(IconValidationError::Decode) => {
                    return ItemPlan::Retryable;
                }
                Err(_) => return ItemPlan::Permanent,
            },
        },
    };
    let mut saw_retryable = false;
    for source in sources {
        match fetcher.fetch_icon(&source).await {
            Ok(icon) => {
                return ItemPlan::Ready {
                    icon,
                    source_url: source,
                    source_priority,
                };
            }
            // A candidate that merely timed out or overflowed does not poison the rest:
            // the next declared link may still work, and any retryable result keeps the
            // whole item eligible for a later job.
            Err(IconValidationError::Decode) => saw_retryable = true,
            Err(_) => {}
        }
    }
    if saw_retryable {
        ItemPlan::Retryable
    } else {
        ItemPlan::Permanent
    }
}

#[derive(sqlx::FromRow)]
struct ReadyIconRow {
    storage_key: String,
    content_hash: Vec<u8>,
}
