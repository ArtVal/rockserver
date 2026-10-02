//! Browser-authenticated personal-data sync HTTP boundary.
//!
//! Exposes `POST /api/v1/browser/sync` for the first-party web account centre. Authenticates
//! via browser cookie (`rockserver_browser`) and CSRF token (`X-CSRF-Token`), verifies the
//! trusted proxy, applies changes through [`PersonalDataStore`], and enriches active favourites
//! with full [`PublicStationDto`] metadata so the browser can immediately render station cards.

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde::Serialize;
use serde_json::json;

use crate::personal_data::{FavouriteRecord, HistoryRecord, SyncOutcome, Timestamp};

use super::{
    state::{AppState, PublicLimit},
    sync::{
        FavouriteRecordDto, HistoryRecordDto, MAX_SYNC_BODY_BYTES, SyncCollectionResponseDto,
        SyncRequestDto, domain_request, maybe_sweep_retention, sync_error_response,
    },
    transport::{
        PublicStationDto, cookie_value, error_response, no_store_json_response,
        parse_json_request_with_limit, request_id, token_hash, trusted_proxy_header_matches,
        unauthorized_response,
    },
};

/// Per-user browser sync quota.
const BROWSER_SYNC_LIMIT: PublicLimit = PublicLimit {
    requests: 60,
    burst: 20,
};

/// Transport response for `POST /api/v1/browser/sync`.
///
/// Carries the new cursor, collection deltas, and resolved station metadata for active favourites.
#[derive(Serialize)]
pub(super) struct BrowserPersonalSyncResponseDto {
    pub(super) server_revision: u64,
    pub(super) server_time: String,
    pub(super) favourites: SyncCollectionResponseDto<FavouriteRecordDto>,
    pub(super) history: SyncCollectionResponseDto<HistoryRecordDto>,
    pub(super) stations: Vec<PublicStationDto>,
}

/// Serves `POST /api/v1/browser/sync` for authenticated browser sessions.
pub(super) async fn browser_sync_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let request_id = request_id(&headers);
    if !trusted_proxy_header_matches(&headers, state.trusted_proxy_token.as_deref()) {
        return error_response(
            StatusCode::FORBIDDEN,
            "untrusted_request",
            "The request must originate from the trusted first-party proxy.",
            &request_id,
            json!({}),
        );
    }
    let Some(cookie) = cookie_value(&headers, "rockserver_browser") else {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "authentication_required",
            "A browser session is required.",
            &request_id,
            json!({}),
        );
    };
    let Some(csrf) = headers
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return error_response(
            StatusCode::FORBIDDEN,
            "csrf_required",
            "A matching CSRF token is required.",
            &request_id,
            json!({}),
        );
    };
    let Some(account_store) = state.account_store.as_ref() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "auth_unavailable",
            "Account service is unavailable.",
            &request_id,
            json!({}),
        );
    };
    let user_id = match account_store
        .browser_session_user_with_csrf(&token_hash(cookie), &token_hash(csrf))
        .await
    {
        Ok(Some(id)) => id,
        Ok(None) => return unauthorized_response(&request_id),
        Err(_) => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "auth_unavailable",
                "Account service is unavailable.",
                &request_id,
                json!({}),
            );
        }
    };
    if let Err(response) =
        state.device_request_allowed("browser_sync", user_id, BROWSER_SYNC_LIMIT, &request_id)
    {
        return *response;
    }
    let Some(personal_store) = state.personal_store.clone() else {
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
    match personal_store.synchronize(user_id, request).await {
        Ok(outcome) => {
            tracing::info!(
                %request_id,
                endpoint = "browser_sync",
                status = 200,
                user_id = %user_id,
                favourites = outcome.favourites.len(),
                history = outcome.history.len(),
                cursor = outcome.server_revision,
                "browser personal data sync completed"
            );
            browser_sync_response(&state, outcome, &request_id).await
        }
        Err(error) => sync_error_response(error, &request_id),
    }
}

/// Enriches active favourites with catalog station representations and builds the response.
async fn browser_sync_response(
    state: &AppState,
    outcome: SyncOutcome,
    request_id: &str,
) -> Response {
    let mut stations = Vec::new();
    let mut seen_stations = std::collections::BTreeSet::new();
    for fav in &outcome.favourites {
        let Some(station_id) = &fav.station_id else {
            continue;
        };
        if fav.deleted_at.is_some() || !seen_stations.insert(station_id.clone()) {
            continue;
        }
        if let Ok(Some(station)) = state.search_service.public_station(station_id).await {
            stations.push(PublicStationDto::from(&station));
        }
    }

    no_store_json_response(
        BrowserPersonalSyncResponseDto {
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
            stations,
        },
        request_id,
    )
}
