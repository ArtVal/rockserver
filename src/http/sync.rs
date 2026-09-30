//! Personal-data sync HTTP boundary (RM-012-A).
//!
//! One authenticated push+pull round trip: `POST /api/v1/sync` accepts a batch of client
//! changes and a delta cursor, applies them last-writer-wins through the personal-data
//! store, and returns every record the client must see plus the new cursor. Validation and
//! merge semantics live in the `personal_data` domain; this module only maps transport.

use std::sync::atomic::{AtomicU64, Ordering};

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::personal_data::{
    Canonical, CollectionChanges, FavouriteRecord, FavouriteUpsert, HistoryRecord, HistoryUpsert,
    MAX_FAVOURITE_RECORDS, MAX_HISTORY_RECORDS, PersonalDataError, RecordDelete, SyncOutcome,
    SyncRequest, Timestamp,
};

use super::{
    control_auth::authenticate_and_throttle,
    state::{AppState, PublicLimit},
    transport::{
        error_response, no_store_json_response, parse_json_request_with_limit, request_id,
    },
};

/// Per-device sync quota; a foreground sync loop stays far below it.
const SYNC_LIMIT: PublicLimit = PublicLimit {
    requests: 60,
    burst: 20,
};
/// Batch sync bodies legitimately exceed the default 16 KiB public cap.
const MAX_SYNC_BODY_BYTES: usize = 256 * 1024;
/// Fleet-wide retention sweeps run at most once per hour per process.
const SWEEP_INTERVAL_SECONDS: u64 = 3600;
static LAST_RETENTION_SWEEP_UNIX: AtomicU64 = AtomicU64::new(0);

/// Transport request for `POST /api/v1/sync`; both collections are optional.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SyncRequestDto {
    #[serde(default)]
    since_revision: Option<u64>,
    #[serde(default)]
    favourites: Option<SyncCollectionDto<FavouriteUpsertDto>>,
    #[serde(default)]
    history: Option<SyncCollectionDto<HistoryUpsertDto>>,
}

/// Transport shape of one collection's pushed changes.
///
/// Absent fields desynchronize to `None`, which the domain mapping treats as empty; the
/// options avoid serde derive's generic-struct requirement that `U: Default`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SyncCollectionDto<U> {
    upserts: Option<Vec<U>>,
    deletes: Option<Vec<RecordDeleteDto>>,
}

impl<U> Default for SyncCollectionDto<U> {
    fn default() -> Self {
        Self {
            upserts: None,
            deletes: None,
        }
    }
}

/// Transport favourite upsert; timestamps stay strings until domain validation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FavouriteUpsertDto {
    record_id: String,
    station_id: String,
    added_at: String,
    updated_at: String,
}

/// Transport history upsert; timestamps stay strings until domain validation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryUpsertDto {
    record_id: String,
    station_id: String,
    started_at: String,
    last_played_at: String,
    #[serde(default)]
    ended_at: Option<String>,
    #[serde(default)]
    play_duration_ms: Option<u64>,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
    updated_at: String,
}

/// Transport deletion marker; `updated_at` arms the tombstone for last-writer-wins.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordDeleteDto {
    record_id: String,
    updated_at: String,
}

/// Transport response with the new cursor and both collections' deltas.
#[derive(Serialize)]
struct SyncResponseDto {
    server_revision: u64,
    server_time: String,
    favourites: SyncCollectionResponseDto<FavouriteRecordDto>,
    history: SyncCollectionResponseDto<HistoryRecordDto>,
}

/// Transport shape of one collection's returned records.
#[derive(Serialize)]
struct SyncCollectionResponseDto<R> {
    records: Vec<R>,
}

/// Transport favourite projection; tombstones omit station and carry `deleted_at`.
#[derive(Serialize)]
struct FavouriteRecordDto {
    record_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    station_id: Option<String>,
    added_at: String,
    updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    deleted_at: Option<String>,
}

/// Transport history projection; tombstones omit station data and carry `deleted_at`.
#[derive(Serialize)]
struct HistoryRecordDto {
    record_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    station_id: Option<String>,
    started_at: String,
    last_played_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ended_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    play_duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<serde_json::Value>,
    updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    deleted_at: Option<String>,
}

/// Serves `POST /api/v1/sync` for account-authenticated native devices.
pub(super) async fn sync_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let request_id = request_id(&headers);
    let principal =
        match authenticate_and_throttle(&state, &headers, "personal_sync", SYNC_LIMIT, &request_id)
            .await
        {
            Ok(principal) => principal,
            Err(response) => return *response,
        };
    let Some(store) = state.personal_store.clone() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "sync_unavailable",
            "Personal data sync is unavailable.",
            &request_id,
            json!({}),
        );
    };
    let payload: SyncRequestDto =
        match parse_json_request_with_limit(&headers, body, &request_id, MAX_SYNC_BODY_BYTES).await
        {
            Ok(payload) => payload,
            Err(response) => return *response,
        };
    let request = match domain_request(payload) {
        Ok(request) => request,
        Err(error) => return sync_error_response(error, &request_id),
    };
    maybe_sweep_retention(&state).await;
    match store.synchronize(principal.user_id, request).await {
        Ok(outcome) => {
            tracing::info!(
                %request_id,
                endpoint = "personal_sync",
                status = 200,
                user_id = %principal.user_id,
                device_id = %principal.device_id,
                favourites = outcome.favourites.len(),
                history = outcome.history.len(),
                cursor = outcome.server_revision,
                "personal data sync completed"
            );
            sync_response(outcome, &request_id)
        }
        Err(error) => sync_error_response(error, &request_id),
    }
}

/// Converts the transport request into the domain request; stores validate it against
/// server time, this step only parses identifiers and timestamps.
fn domain_request(payload: SyncRequestDto) -> Result<SyncRequest, PersonalDataError> {
    Ok(SyncRequest {
        since_revision: payload.since_revision.unwrap_or(0),
        favourites: favourite_changes(payload.favourites)?,
        history: history_changes(payload.history)?,
    })
}

fn favourite_changes(
    payload: Option<SyncCollectionDto<FavouriteUpsertDto>>,
) -> Result<CollectionChanges<FavouriteUpsert>, PersonalDataError> {
    let payload = payload.unwrap_or_default();
    let upserts = payload
        .upserts
        .unwrap_or_default()
        .iter()
        .map(|upsert| {
            Ok(FavouriteUpsert {
                record_id: Canonical::record_id(&upsert.record_id)?,
                station_id: Canonical::station_id(&upsert.station_id)?,
                added_at: Timestamp::parse(&upsert.added_at)?,
                updated_at: Timestamp::parse(&upsert.updated_at)?,
            })
        })
        .collect::<Result<_, _>>()?;
    Ok(CollectionChanges {
        upserts,
        deletes: record_deletes(&payload.deletes.unwrap_or_default())?,
    })
}

fn history_changes(
    payload: Option<SyncCollectionDto<HistoryUpsertDto>>,
) -> Result<CollectionChanges<HistoryUpsert>, PersonalDataError> {
    let payload = payload.unwrap_or_default();
    let upserts = payload
        .upserts
        .unwrap_or_default()
        .iter()
        .map(|upsert| {
            Ok(HistoryUpsert {
                record_id: Canonical::record_id(&upsert.record_id)?,
                station_id: Canonical::station_id(&upsert.station_id)?,
                started_at: Timestamp::parse(&upsert.started_at)?,
                last_played_at: Timestamp::parse(&upsert.last_played_at)?,
                ended_at: upsert
                    .ended_at
                    .as_deref()
                    .map(Timestamp::parse)
                    .transpose()?,
                play_duration_ms: upsert.play_duration_ms,
                metadata: upsert.metadata.clone(),
                updated_at: Timestamp::parse(&upsert.updated_at)?,
            })
        })
        .collect::<Result<_, _>>()?;
    Ok(CollectionChanges {
        upserts,
        deletes: record_deletes(&payload.deletes.unwrap_or_default())?,
    })
}

fn record_deletes(deletes: &[RecordDeleteDto]) -> Result<Vec<RecordDelete>, PersonalDataError> {
    deletes
        .iter()
        .map(|delete| {
            Ok(RecordDelete {
                record_id: Canonical::record_id(&delete.record_id)?,
                updated_at: Timestamp::parse(&delete.updated_at)?,
            })
        })
        .collect()
}

/// Builds the contract response body with server time and both deltas.
fn sync_response(outcome: SyncOutcome, request_id: &str) -> Response {
    no_store_json_response(
        SyncResponseDto {
            server_revision: outcome.server_revision,
            server_time: Timestamp::from_instant(time::OffsetDateTime::now_utc())
                .as_rfc3339()
                .to_owned(),
            favourites: SyncCollectionResponseDto {
                records: outcome
                    .favourites
                    .into_iter()
                    .map(|record: FavouriteRecord| FavouriteRecordDto {
                        record_id: record.record_id.to_string(),
                        station_id: record.station_id,
                        added_at: record.added_at.as_rfc3339().to_owned(),
                        updated_at: record.updated_at.as_rfc3339().to_owned(),
                        deleted_at: record
                            .deleted_at
                            .as_ref()
                            .map(|stamp| stamp.as_rfc3339().to_owned()),
                    })
                    .collect(),
            },
            history: SyncCollectionResponseDto {
                records: outcome
                    .history
                    .into_iter()
                    .map(|record: HistoryRecord| HistoryRecordDto {
                        record_id: record.record_id.to_string(),
                        station_id: record.station_id,
                        started_at: record.started_at.as_rfc3339().to_owned(),
                        last_played_at: record.last_played_at.as_rfc3339().to_owned(),
                        ended_at: record
                            .ended_at
                            .as_ref()
                            .map(|stamp| stamp.as_rfc3339().to_owned()),
                        play_duration_ms: record.play_duration_ms,
                        metadata: record.metadata,
                        updated_at: record.updated_at.as_rfc3339().to_owned(),
                        deleted_at: record
                            .deleted_at
                            .as_ref()
                            .map(|stamp| stamp.as_rfc3339().to_owned()),
                    })
                    .collect(),
            },
        },
        request_id,
    )
}

/// Maps domain failures onto the public error envelope.
fn sync_error_response(error: PersonalDataError, request_id: &str) -> Response {
    match error {
        PersonalDataError::Validation(field) => error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
            "Sync request validation failed.",
            request_id,
            json!({"field": field}),
        ),
        PersonalDataError::FavouriteLimitExceeded => error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
            "Favourites collection limit exceeded.",
            request_id,
            json!({"collection": "favourites", "limit": MAX_FAVOURITE_RECORDS}),
        ),
        PersonalDataError::HistoryLimitExceeded => error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
            "History collection limit exceeded.",
            request_id,
            json!({"collection": "history", "limit": MAX_HISTORY_RECORDS}),
        ),
        PersonalDataError::Database => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "sync_unavailable",
            "Personal data sync is temporarily unavailable.",
            request_id,
            json!({}),
        ),
    }
}

/// Runs the fleet-wide retention sweep at most once per hour per process.
///
/// The compare-and-swap makes concurrent requests elect exactly one sweeper; failures only
/// log so a sweep outage never blocks client syncs, and the next hour retries.
async fn maybe_sweep_retention(state: &AppState) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    let last = LAST_RETENTION_SWEEP_UNIX.load(Ordering::Relaxed);
    if now.saturating_sub(last) < SWEEP_INTERVAL_SECONDS {
        return;
    }
    if LAST_RETENTION_SWEEP_UNIX
        .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    let Some(store) = state.personal_store.clone() else {
        return;
    };
    tokio::spawn(async move {
        match store.sweep_retention().await {
            Ok(sweep) if sweep == Default::default() => {}
            Ok(sweep) => {
                tracing::info!(
                    history_expired = sweep.history_expired,
                    tombstones_removed = sweep.tombstones_removed,
                    deleted_account_rows_removed = sweep.deleted_account_rows_removed,
                    "personal data retention sweep completed"
                );
            }
            Err(error) => {
                tracing::warn!(%error, "personal data retention sweep failed");
            }
        }
    });
}
