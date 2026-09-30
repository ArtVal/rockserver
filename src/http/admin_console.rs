//! Protected, presentation-only read models for the first-party administrator SPA.

use super::{
    admin_auth::{active_session, record_request},
    state::AppState,
    transport::{error_response, request_id, with_request_id},
};
use crate::{
    admin::{AdminAuditFilter, AdminRequestOutcome},
    persistence::AdminDeviceReadModel,
    search::{Station, StationHealth},
};
use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, Request, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Instant;

const ADMIN_CSP: &str = "default-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self'; font-src 'none'; object-src 'none'";
const MAX_PAGE_SIZE: u8 = 50;
const MAX_OFFSET: u32 = 10_000;

/// Adds strict browser-response protections to administrator API responses.
pub(super) async fn security_headers(request: Request<axum::body::Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(ADMIN_CSP),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        "permissions-policy",
        HeaderValue::from_static("geolocation=(), microphone=(), camera=()"),
    );
    response
}

/// Bounded pagination parameters shared by SPA read models.
#[derive(Deserialize)]
pub(super) struct PageQuery {
    #[serde(default)]
    limit: Option<u8>,
    #[serde(default)]
    offset: Option<u32>,
}
/// Bounded station-list query parameters; search is executed by the repository.
#[derive(Deserialize)]
pub(super) struct StationQuery {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    limit: Option<u8>,
    #[serde(default)]
    offset: Option<u32>,
}
/// Bounded audit timeline query parameters.
#[derive(Deserialize)]
pub(super) struct AuditQuery {
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    until: Option<String>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    outcome: Option<String>,
    #[serde(default)]
    limit: Option<u8>,
    #[serde(default)]
    offset: Option<u32>,
}
#[derive(Serialize)]
struct Page<T> {
    items: Vec<T>,
    limit: u8,
    offset: u32,
    has_more: bool,
}

/// SPA station view deliberately excludes stream URLs and persistence identifiers.
#[derive(Serialize)]
struct StationDto {
    station_id: String,
    name: String,
    tags: Vec<String>,
    language: Option<String>,
    country_code: Option<String>,
    health: &'static str,
}
impl From<&Station> for StationDto {
    fn from(station: &Station) -> Self {
        Self {
            station_id: station.id.clone(),
            name: station.name.clone(),
            tags: station.tags.clone(),
            language: station.language.clone(),
            country_code: station.country_code.clone(),
            health: match station.health {
                StationHealth::Healthy => "healthy",
                StationHealth::Degraded => "degraded",
                StationHealth::Unknown => "unknown",
            },
        }
    }
}
#[derive(Serialize)]
struct DeviceDto {
    product: &'static str,
    device_type: String,
    display_name: String,
    status: String,
    created_at: String,
    last_seen_at: Option<String>,
}
impl From<AdminDeviceReadModel> for DeviceDto {
    fn from(device: AdminDeviceReadModel) -> Self {
        let product = if device.device_type.to_ascii_lowercase().contains("mobile") {
            "RockMobile"
        } else {
            "RockCast"
        };
        Self {
            product,
            device_type: device.device_type,
            display_name: device.device_display_name,
            status: device.status,
            created_at: device.created_at,
            last_seen_at: device.last_seen_at,
        }
    }
}
#[derive(Serialize)]
struct AuditDto {
    occurred_at: String,
    action: String,
    outcome: String,
}

fn page(
    query: &PageQuery,
    request_id: &str,
    default_limit: u8,
) -> Result<(u8, u32), Box<Response>> {
    let limit = query.limit.unwrap_or(default_limit);
    let offset = query.offset.unwrap_or(0);
    if !(1..=MAX_PAGE_SIZE).contains(&limit) || offset > MAX_OFFSET {
        return Err(Box::new(error_response(
            StatusCode::BAD_REQUEST,
            "malformed_request",
            "Pagination is invalid.",
            request_id,
            json!({"fields":["limit","offset"]}),
        )));
    }
    Ok((limit, offset))
}

/// Lists a server-filtered, stable administrator station page.
pub(super) async fn stations(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<StationQuery>,
) -> Response {
    let request_id = request_id(&headers);
    let started = Instant::now();
    let session = match active_session(&state, &headers, &request_id).await {
        Ok(session) => session,
        Err(response) => return *response,
    };
    let (limit, offset) = match page(
        &PageQuery {
            limit: query.limit,
            offset: query.offset,
        },
        &request_id,
        50,
    ) {
        Ok(page) => page,
        Err(response) => return *response,
    };
    let term = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if term.is_some_and(|value| value.len() > 100) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "malformed_request",
            "Station search is invalid.",
            &request_id,
            json!({"field":"q"}),
        );
    }
    match state
        .search_service
        .admin_catalog(term, None, offset as usize + usize::from(limit) + 1)
        .await
    {
        Ok(mut stations) => {
            let start = offset as usize;
            let remaining = stations.split_off(start.min(stations.len()));
            stations = remaining;
            let has_more = stations.len() > usize::from(limit);
            stations.truncate(usize::from(limit));
            let response = with_request_id(
                Json(Page {
                    items: stations.iter().map(StationDto::from).collect(),
                    limit,
                    offset,
                    has_more,
                })
                .into_response(),
                &request_id,
            );
            record_request(
                &state,
                &session,
                &request_id,
                "/api/v1/admin/stations",
                AdminRequestOutcome::Succeeded,
                started,
            )
            .await;
            response
        }
        Err(_) => unavailable(
            &state,
            &session,
            &request_id,
            "/api/v1/admin/stations",
            started,
        ),
    }
}
/// Lists a protected device inventory without owner, credential, or device identifiers.
pub(super) async fn devices(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<PageQuery>,
) -> Response {
    let request_id = request_id(&headers);
    let started = Instant::now();
    let session = match active_session(&state, &headers, &request_id).await {
        Ok(session) => session,
        Err(response) => return *response,
    };
    let (limit, offset) = match page(&query, &request_id, 25) {
        Ok(page) => page,
        Err(response) => return *response,
    };
    let Some(store) = &state.account_store else {
        return unavailable(
            &state,
            &session,
            &request_id,
            "/api/v1/admin/devices",
            started,
        );
    };
    match store
        .list_admin_devices(i64::from(offset), i64::from(limit) + 1)
        .await
    {
        Ok(mut devices) => {
            let has_more = devices.len() > usize::from(limit);
            devices.truncate(usize::from(limit));
            let response = with_request_id(
                Json(Page {
                    items: devices.into_iter().map(DeviceDto::from).collect(),
                    limit,
                    offset,
                    has_more,
                })
                .into_response(),
                &request_id,
            );
            record_request(
                &state,
                &session,
                &request_id,
                "/api/v1/admin/devices",
                AdminRequestOutcome::Succeeded,
                started,
            )
            .await;
            response
        }
        Err(_) => unavailable(
            &state,
            &session,
            &request_id,
            "/api/v1/admin/devices",
            started,
        ),
    }
}
/// Lists the safe, unified administrator request and security-event timeline.
pub(super) async fn audit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AuditQuery>,
) -> Response {
    let request_id = request_id(&headers);
    let started = Instant::now();
    let session = match active_session(&state, &headers, &request_id).await {
        Ok(session) => session,
        Err(response) => return *response,
    };
    let (limit, offset) = match page(
        &PageQuery {
            limit: query.limit,
            offset: query.offset,
        },
        &request_id,
        25,
    ) {
        Ok(page) => page,
        Err(response) => return *response,
    };
    let Some(store) = &state.admin_store else {
        return unavailable(
            &state,
            &session,
            &request_id,
            "/api/v1/admin/audit",
            started,
        );
    };
    let filter = AdminAuditFilter {
        from: query.from,
        until: query.until,
        action: query.action,
        outcome: query.outcome,
        offset: i64::from(offset),
        limit: i64::from(limit) + 1,
    };
    match store.list_audit(filter).await {
        Ok(mut entries) => {
            let has_more = entries.len() > usize::from(limit);
            entries.truncate(usize::from(limit));
            let response = with_request_id(
                Json(Page {
                    items: entries
                        .into_iter()
                        .map(|entry| AuditDto {
                            occurred_at: entry.occurred_at,
                            action: entry.action,
                            outcome: entry.outcome,
                        })
                        .collect(),
                    limit,
                    offset,
                    has_more,
                })
                .into_response(),
                &request_id,
            );
            record_request(
                &state,
                &session,
                &request_id,
                "/api/v1/admin/audit",
                AdminRequestOutcome::Succeeded,
                started,
            )
            .await;
            response
        }
        Err(_) => unavailable(
            &state,
            &session,
            &request_id,
            "/api/v1/admin/audit",
            started,
        ),
    }
}
fn unavailable(
    state: &AppState,
    session: &crate::admin::AdminSession,
    request_id: &str,
    endpoint: &'static str,
    started: Instant,
) -> Response {
    let response = error_response(
        StatusCode::SERVICE_UNAVAILABLE,
        "service_unavailable",
        "Administrator read model is temporarily unavailable.",
        request_id,
        json!({}),
    );
    let state = state.clone();
    let session = *session;
    let request_id = request_id.to_owned();
    tokio::spawn(async move {
        record_request(
            &state,
            &session,
            &request_id,
            endpoint,
            AdminRequestOutcome::Failed,
            started,
        )
        .await;
    });
    response
}
