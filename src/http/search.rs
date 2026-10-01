//! Station-search HTTP handlers and search-request validation.

use std::collections::BTreeSet;

use axum::{
    Json,
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::search::{QueryParserInput, SearchConstraints};

use super::{
    state::{AppState, PublicLimit},
    transport::{
        NormalizedQueryDto, SearchResponseDto, StationResultDto, error_response,
        parse_json_request, request_id, with_request_id,
    },
};

const SEARCH_LIMIT: PublicLimit = PublicLimit {
    requests: 120,
    burst: 60,
};

/// Transport representation of the `SearchRequest` OpenAPI schema.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SearchRequestDto {
    pub(super) query: String,
    #[serde(default)]
    pub(super) locale: Option<String>,
    #[serde(default)]
    pub(super) limit: Option<u8>,
    #[serde(default)]
    pub(super) offset: Option<usize>,
    #[serde(default)]
    pub(super) exclude_station_ids: Vec<String>,
}

/// Validated search filters shared by search and voice transports.
pub(super) struct ValidatedSearchRequest {
    pub(super) query: String,
    pub(super) locale: String,
    pub(super) limit: usize,
    pub(super) offset: usize,
    pub(super) exclude_station_ids: BTreeSet<String>,
}

impl TryFrom<SearchRequestDto> for ValidatedSearchRequest {
    type Error = Map<String, Value>;

    fn try_from(value: SearchRequestDto) -> Result<Self, Self::Error> {
        let mut details = Map::new();
        let query = value.query.trim().to_owned();
        if query.is_empty() {
            details.insert(
                "query".to_owned(),
                json!("must not be empty or whitespace only"),
            );
        } else if query.chars().count() > 500 {
            details.insert(
                "query".to_owned(),
                json!("must contain at most 500 characters"),
            );
        }

        let locale = value.locale.unwrap_or_else(|| "en-US".to_owned());
        if !is_valid_locale(&locale) {
            details.insert("locale".to_owned(), json!("must be a BCP 47-style locale"));
        }

        let limit = value.limit.unwrap_or(10);
        if !(1..=50).contains(&limit) {
            details.insert("limit".to_owned(), json!("must be between 1 and 50"));
        }

        if value.exclude_station_ids.len() > 100 {
            details.insert(
                "exclude_station_ids".to_owned(),
                json!("must contain at most 100 station IDs"),
            );
        }
        let excluded_id_count = value.exclude_station_ids.len();
        let ids = value
            .exclude_station_ids
            .into_iter()
            .collect::<BTreeSet<_>>();
        if ids.len() != excluded_id_count {
            details.insert(
                "exclude_station_ids".to_owned(),
                json!("must not contain duplicate station IDs"),
            );
        }
        if ids
            .iter()
            .any(|station_id| station_id.is_empty() || station_id.chars().count() > 128)
        {
            details.insert(
                "exclude_station_ids".to_owned(),
                json!("each station ID must contain between 1 and 128 characters"),
            );
        }

        let offset = value.offset.unwrap_or(0);
        if offset > 10_000 {
            details.insert("offset".to_owned(), json!("must be at most 10000"));
        }

        if details.is_empty() {
            Ok(Self {
                query,
                locale,
                limit: usize::from(limit),
                offset,
                exclude_station_ids: ids,
            })
        } else {
            Err(details)
        }
    }
}

/// Validates the bounded BCP 47-style locale accepted by search and device-catalog transports.
pub(super) fn is_valid_locale(locale: &str) -> bool {
    let mut subtags = locale.split('-');
    let Some(language) = subtags.next() else {
        return false;
    };
    (2..=3).contains(&language.len())
        && language.bytes().all(|byte| byte.is_ascii_alphabetic())
        && subtags.all(|subtag| {
            (2..=8).contains(&subtag.len())
                && subtag.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
}

/// Serves the approved anonymous, bounded station-search operation.
pub(super) async fn public_search(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let request_id = request_id(&headers);
    if let Err(response) = state.public_request_allowed("search", SEARCH_LIMIT, &request_id) {
        return *response;
    }
    search_impl(state, headers, body, request_id, 20).await
}

async fn search_impl(
    state: AppState,
    headers: HeaderMap,
    body: Body,
    request_id: String,
    max_limit: u8,
) -> Response {
    let request = match parse_json_request::<SearchRequestDto>(&headers, body, &request_id).await {
        Ok(request) => request,
        Err(response) => return *response,
    };
    let validated = match ValidatedSearchRequest::try_from(request) {
        Ok(request) => request,
        Err(details) => {
            return error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation_failed",
                "Request validation failed.",
                &request_id,
                Value::Object(details),
            );
        }
    };
    if validated.limit > usize::from(max_limit) {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
            "Request validation failed.",
            &request_id,
            json!({"limit": format!("must be between 1 and {max_limit}")}),
        );
    }

    let constraints = SearchConstraints {
        limit: validated.limit,
        offset: validated.offset,
        excluded_station_ids: validated.exclude_station_ids,
    };
    let outcome = match tokio::time::timeout(
        state.voice_command_timeout,
        state.search_service.interpret_and_search(
            QueryParserInput {
                query: validated.query,
                locale: validated.locale,
            },
            &constraints,
        ),
    )
    .await
    {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(_)) => {
            tracing::warn!(%request_id, endpoint = "search", "public-safe search failure");
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
                StatusCode::GATEWAY_TIMEOUT,
                "search_timeout",
                "Station search timed out.",
                &request_id,
                json!({"timeout_ms": state.voice_command_timeout.as_millis()}),
            );
        }
    };
    tracing::info!(%request_id, endpoint = "search", status = 200, stations = outcome.stations.len(), "public request completed");

    let total = outcome.total;
    let has_more = validated.offset + outcome.stations.len() < total;
    with_request_id(
        Json(SearchResponseDto {
            request_id: request_id.clone(),
            normalized_query: NormalizedQueryDto::from(outcome.query),
            stations: outcome
                .stations
                .iter()
                .map(StationResultDto::from)
                .collect(),
            total,
            has_more,
        })
        .into_response(),
        &request_id,
    )
}
