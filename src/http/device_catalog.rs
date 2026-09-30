//! Device-facing catalog HTTP handlers authenticated by native device sessions.
//!
//! Both routes reuse the existing catalog and search services but never expose `stream_url`:
//! devices pick a station by stable ID and playback is resolved server-side through
//! `station.play_station` dispatch (RS-1 contract, openapi 0.5.0).

use axum::{
    extract::{Query, State, rejection::QueryRejection},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::search::{QueryParserInput, SearchConstraints, Station, StationHealth};

use super::{
    control_auth::authenticate_and_throttle,
    search::is_valid_locale,
    state::{AppState, PublicLimit},
    transport::{error_response, no_store_json_response, request_id},
};

const DEVICE_CATALOG_BROWSE_LIMIT: PublicLimit = PublicLimit {
    requests: 60,
    burst: 20,
};
const DEVICE_CATALOG_SEARCH_LIMIT: PublicLimit = PublicLimit {
    requests: 30,
    burst: 10,
};
const MAX_DEVICE_QUERY_CHARS: usize = 128;

/// Query parameters accepted by the device catalog browse route.
///
/// Unknown parameters are rejected instead of ignored so firmware stays forward-compatible.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DeviceCatalogQuery {
    #[serde(default)]
    limit: Option<u8>,
    #[serde(default)]
    cursor: Option<String>,
}

/// Query parameters accepted by the device catalog search route.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DeviceSearchQuery {
    q: String,
    #[serde(default)]
    locale: Option<String>,
    #[serde(default)]
    limit: Option<u8>,
}

/// Transport representation of the `DeviceStationDto` OpenAPI schema.
///
/// Constructing this type from a domain [`Station`] drops `stream_url`, ranking fields and any
/// provider or persistence identifiers by construction; they cannot re-enter a device page.
#[derive(Clone, Serialize)]
struct DeviceStationDto {
    id: String,
    name: String,
    homepage_url: Option<String>,
    tags: Vec<String>,
    language: Option<String>,
    country_code: Option<String>,
    codec: Option<String>,
    bitrate_kbps: Option<u32>,
    health: &'static str,
}

impl From<&Station> for DeviceStationDto {
    fn from(station: &Station) -> Self {
        Self {
            id: station.id.clone(),
            name: station.name.clone(),
            homepage_url: station.homepage_url.clone(),
            tags: station.tags.clone(),
            language: station.language.clone(),
            country_code: station.country_code.clone(),
            codec: station.codec.clone(),
            bitrate_kbps: station.bitrate_kbps,
            health: match station.health {
                StationHealth::Healthy => "healthy",
                StationHealth::Degraded => "degraded",
                StationHealth::Unknown => "unknown",
            },
        }
    }
}

/// Transport representation of the `DeviceCatalogPage` OpenAPI schema.
#[derive(Serialize)]
struct DeviceCatalogPageDto {
    request_id: String,
    stations: Vec<DeviceStationDto>,
    next_cursor: Option<String>,
}

/// Transport representation of the `DeviceSearchPage` OpenAPI schema.
#[derive(Serialize)]
struct DeviceSearchPageDto {
    request_id: String,
    stations: Vec<DeviceStationDto>,
}

/// Serves `GET /api/v1/device-control/catalog/stations`: one bounded station-metadata page.
///
/// The cursor is the stable station ID of the last item of the previous page; unknown, empty,
/// oversized, or non-ASCII-alphanumeric-hyphen cursors and limits outside 1..=20 are rejected
/// deterministically with 400.
pub(super) async fn browse(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<DeviceCatalogQuery>, QueryRejection>,
) -> Response {
    let request_id = request_id(&headers);
    if let Err(response) = authenticate_and_throttle(
        &state,
        &headers,
        "device_catalog_browse",
        DEVICE_CATALOG_BROWSE_LIMIT,
        &request_id,
    )
    .await
    {
        return *response;
    }
    let Query(query) = match query {
        Ok(query) => query,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "malformed_request",
                "Device catalog request is invalid.",
                &request_id,
                json!({"field":"limit_or_cursor"}),
            );
        }
    };
    let limit = query.limit.unwrap_or(20);
    if !(1..=20).contains(&limit)
        || query.cursor.as_ref().is_some_and(|cursor| {
            cursor.is_empty()
                || cursor.len() > 512
                || !cursor
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return error_response(
            StatusCode::BAD_REQUEST,
            "malformed_request",
            "Device catalog request is invalid.",
            &request_id,
            json!({"field":"limit_or_cursor"}),
        );
    }
    let stations = match state
        .search_service
        .public_catalog(query.cursor.as_deref(), usize::from(limit))
        .await
    {
        Ok(stations) => stations,
        Err(_) => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "service_unavailable",
                "Catalog is temporarily unavailable.",
                &request_id,
                json!({}),
            );
        }
    };
    let next_cursor = (stations.len() == usize::from(limit))
        .then(|| stations.last().map(|station| station.id.clone()))
        .flatten();
    tracing::info!(%request_id, endpoint = "device_catalog_browse", status = 200, stations = stations.len(), "device catalog request completed");
    no_store_json_response(
        DeviceCatalogPageDto {
            request_id: request_id.clone(),
            stations: stations.iter().map(DeviceStationDto::from).collect(),
            next_cursor,
        },
        &request_id,
    )
}

/// Serves `GET /api/v1/device-control/catalog/search`: one ranked, non-cursorable page.
///
/// `q` must contain 1..=128 non-whitespace-only characters, `locale` must be BCP 47-like, and
/// `limit` must be within 1..=20; violations and unknown parameters are rejected with 400.
/// Search runs under the same timeout budget as the public search route.
pub(super) async fn search(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<DeviceSearchQuery>, QueryRejection>,
) -> Response {
    let request_id = request_id(&headers);
    if let Err(response) = authenticate_and_throttle(
        &state,
        &headers,
        "device_catalog_search",
        DEVICE_CATALOG_SEARCH_LIMIT,
        &request_id,
    )
    .await
    {
        return *response;
    }
    let Query(query) = match query {
        Ok(query) => query,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "malformed_request",
                "Device search request is invalid.",
                &request_id,
                json!({"field":"q"}),
            );
        }
    };
    let mut details = serde_json::Map::new();
    let station_query = query.q.trim().to_owned();
    if station_query.is_empty() {
        details.insert(
            "q".to_owned(),
            json!("must not be empty or whitespace only"),
        );
    } else if station_query.chars().count() > MAX_DEVICE_QUERY_CHARS {
        details.insert("q".to_owned(), json!("must contain at most 128 characters"));
    }
    let locale = query.locale.unwrap_or_else(|| "en-US".to_owned());
    if !is_valid_locale(&locale) {
        details.insert("locale".to_owned(), json!("must be a BCP 47-style locale"));
    }
    let limit = query.limit.unwrap_or(20);
    if !(1..=20).contains(&limit) {
        details.insert("limit".to_owned(), json!("must be between 1 and 20"));
    }
    if !details.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "malformed_request",
            "Device search request is invalid.",
            &request_id,
            serde_json::Value::Object(details),
        );
    }

    let constraints = SearchConstraints {
        limit: usize::from(limit),
        offset: 0,
        excluded_station_ids: Default::default(),
    };
    let outcome = match tokio::time::timeout(
        state.voice_command_timeout,
        state.search_service.interpret_and_search(
            QueryParserInput {
                query: station_query,
                locale,
            },
            &constraints,
        ),
    )
    .await
    {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(_)) => {
            tracing::warn!(%request_id, endpoint = "device_catalog_search", "device-safe search failure");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "An unexpected server error occurred.",
                &request_id,
                json!({}),
            );
        }
        Err(_) => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "service_unavailable",
                "Station search timed out.",
                &request_id,
                json!({"timeout_ms": state.voice_command_timeout.as_millis()}),
            );
        }
    };
    tracing::info!(%request_id, endpoint = "device_catalog_search", status = 200, stations = outcome.stations.len(), "device catalog request completed");
    no_store_json_response(
        DeviceSearchPageDto {
            request_id: request_id.clone(),
            stations: outcome
                .stations
                .iter()
                .take(constraints.limit)
                .map(|ranked| DeviceStationDto::from(&ranked.station))
                .collect(),
        },
        &request_id,
    )
}
