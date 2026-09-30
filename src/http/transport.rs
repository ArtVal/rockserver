//! Shared HTTP request/response transport primitives and DTOs.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::{
    auth::SecretHash,
    search::{RankedStation, StationHealth},
};
use axum::{
    Json,
    body::{Body, to_bytes},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Canonical first-party origin accepted for browser state changes.
pub(super) const FIRST_PARTY_ORIGIN: &str = "https://rockplatform.win";
const MAX_PUBLIC_JSON_REQUEST_BODY_BYTES: usize = 16 * 1024;
const REQUEST_ID_HEADER: &str = "x-request-id";
const TRUSTED_PROXY_HEADER: &str = "x-rockserver-proxy";
static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

fn is_json_content_type(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().starts_with("application/json"))
}

/// Parses a bounded JSON body and maps malformed input to the public error contract.
pub(super) async fn parse_json_request<T>(
    headers: &HeaderMap,
    body: Body,
    request_id: &str,
) -> Result<T, Box<Response>>
where
    T: DeserializeOwned,
{
    parse_json_request_with_limit(
        headers,
        body,
        request_id,
        MAX_PUBLIC_JSON_REQUEST_BODY_BYTES,
    )
    .await
}

/// Parses a JSON body under an endpoint-specific byte budget.
///
/// Batch endpoints such as personal-data sync legitimately exceed the default public body
/// cap, so they opt into a larger budget; everything else keeps the 16 KiB default.
pub(super) async fn parse_json_request_with_limit<T>(
    headers: &HeaderMap,
    body: Body,
    request_id: &str,
    max_bytes: usize,
) -> Result<T, Box<Response>>
where
    T: DeserializeOwned,
{
    if !is_json_content_type(headers) {
        return Err(Box::new(error_response(
            StatusCode::BAD_REQUEST,
            "malformed_request",
            "Request body must contain valid JSON.",
            request_id,
            json!({"content_type": "application/json is required"}),
        )));
    }
    let body = to_bytes(body, max_bytes).await.map_err(|_| {
        Box::new(error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            "request_too_large",
            "Request body exceeds the allowed size.",
            request_id,
            json!({"max_bytes": max_bytes}),
        ))
    })?;
    let value = serde_json::from_slice::<Value>(&body).map_err(|_error| {
        Box::new(error_response(
            StatusCode::BAD_REQUEST,
            "malformed_request",
            "Request body must contain valid JSON.",
            request_id,
            json!({"field":"body"}),
        ))
    })?;
    serde_json::from_value(value).map_err(|_error| {
        Box::new(error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
            "Request validation failed.",
            request_id,
            json!({"field":"request"}),
        ))
    })
}

/// Returns a validated caller request ID or allocates a process-local fallback.
pub(super) fn request_id(headers: &HeaderMap) -> String {
    headers
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| is_valid_request_id(value))
        .map(str::to_owned)
        .unwrap_or_else(next_request_id)
}

fn is_valid_request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn next_request_id() -> String {
    format!(
        "req_{:016x}",
        REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

/// Builds the stable error body and attaches its request ID header.
pub(super) fn error_response(
    status: StatusCode,
    code: &str,
    message: &str,
    request_id: &str,
    details: Value,
) -> Response {
    with_request_id(
        (
            status,
            Json(ErrorResponseDto {
                code: code.to_owned(),
                message: message.to_owned(),
                request_id: request_id.to_owned(),
                details,
            }),
        )
            .into_response(),
        request_id,
    )
}

/// Creates the contract-shaped response for a missing or invalid Bearer credential.
pub(super) fn unauthorized_response(request_id: &str) -> Response {
    let mut response = error_response(
        StatusCode::UNAUTHORIZED,
        "authentication_required",
        "A valid Bearer token is required.",
        request_id,
        json!({}),
    );
    response
        .headers_mut()
        .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    response
}

/// Attaches a validated request ID to an arbitrary HTTP response.
pub(super) fn with_request_id(mut response: Response, request_id: &str) -> Response {
    let header_value = HeaderValue::from_str(request_id)
        .expect("request IDs are generated or constrained to valid header characters");
    response
        .headers_mut()
        .insert(REQUEST_ID_HEADER, header_value);
    response
}

/// Serializes a JSON payload with the contract's no-store cache policy.
pub(super) fn no_store_json_response(payload: impl Serialize, request_id: &str) -> Response {
    let mut response = with_request_id(Json(payload).into_response(), request_id);
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Adds a bounded caller-visible retry delay to a response.
pub(super) fn retry_after(mut response: Response, seconds: u64) -> Response {
    response.headers_mut().insert(
        header::RETRY_AFTER,
        HeaderValue::from_str(&seconds.to_string())
            .expect("positive retry-after is a valid header"),
    );
    response
}

/// Compares opaque credentials without returning early for a matching prefix.
pub(super) fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= usize::from(
            left.get(index).copied().unwrap_or_default()
                ^ right.get(index).copied().unwrap_or_default(),
        );
    }
    difference == 0
}

/// Extracts a bounded Bearer credential without logging or storing it.
pub(super) fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty() && value.len() <= 512)
}

/// Extracts one named cookie value from the request header.
pub(super) fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').map(str::trim).find_map(|cookie| {
                cookie
                    .strip_prefix(name)
                    .and_then(|value| value.strip_prefix('='))
            })
        })
}

/// Enforces the first-party HTTPS origin and proxy protocol marker for browser state changes.
pub(super) fn is_trusted_browser_request(headers: &HeaderMap) -> bool {
    headers
        .get("origin")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|origin| origin == FIRST_PARTY_ORIGIN)
        && headers
            .get("x-forwarded-proto")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|protocol| protocol.eq_ignore_ascii_case("https"))
}

/// Accepts the production HTTPS origin or one explicitly configured loopback origin for admin routes.
pub(super) fn is_trusted_admin_browser_request(
    headers: &HeaderMap,
    local_admin_origin: Option<&str>,
) -> bool {
    is_trusted_browser_request(headers)
        || local_admin_origin.is_some_and(|expected| {
            headers
                .get("origin")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|origin| origin == expected)
        })
}

/// Requires the shared header that only the configured Caddy proxy may inject.
pub(super) fn trusted_proxy_header_matches(headers: &HeaderMap, expected: Option<&str>) -> bool {
    let Some(expected) = expected.filter(|value| !value.is_empty()) else {
        return false;
    };
    let Some(actual) = headers
        .get(TRUSTED_PROXY_HEADER)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    constant_time_eq(actual.as_bytes(), expected.as_bytes())
}

/// Selects a stable, bounded rate-limit scope without trusting forwarded identity from direct peers.
pub(super) fn request_rate_limit_scope(
    headers: &HeaderMap,
    expected_proxy_token: Option<&str>,
) -> String {
    if trusted_proxy_header_matches(headers, expected_proxy_token) {
        if let Some(forwarded) = headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
        {
            let bounded: String = forwarded.chars().take(256).collect();
            if !bounded.trim().is_empty() {
                return bounded;
            }
        }
        return "trusted-proxy-unknown-client".to_owned();
    }
    "direct-peer".to_owned()
}

/// Hashes an opaque proof before it reaches the persistence boundary.
pub(super) fn token_hash(value: &str) -> SecretHash {
    SecretHash::new(Sha256::digest(value.as_bytes()).into())
}

/// Successful transport response for `POST /api/v1/search`.
#[derive(Serialize)]
pub(super) struct SearchResponseDto {
    pub(super) request_id: String,
    pub(super) normalized_query: NormalizedQueryDto,
    pub(super) stations: Vec<StationResultDto>,
    pub(super) total: usize,
    pub(super) has_more: bool,
}

/// Transport representation of normalized query constraints.
#[derive(Serialize)]
pub(super) struct NormalizedQueryDto {
    pub(super) action: crate::search::SearchAction,
    pub(super) original: String,
    pub(super) locale: String,
    pub(super) terms: Vec<String>,
    pub(super) tags: Vec<String>,
    pub(super) language: Option<String>,
    pub(super) country_code: Option<String>,
}

impl From<crate::search::SearchQuery> for NormalizedQueryDto {
    fn from(query: crate::search::SearchQuery) -> Self {
        Self {
            action: query.action,
            original: query.original,
            locale: query.locale,
            terms: query.terms,
            tags: query.tags,
            language: query.language,
            country_code: query.country_code,
        }
    }
}

/// Transport representation of a ranked station result.
#[derive(Clone, Serialize)]
pub(super) struct StationResultDto {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) stream_url: String,
    pub(super) homepage_url: Option<String>,
    pub(super) favicon_url: Option<String>,
    pub(super) tags: Vec<String>,
    pub(super) language: Option<String>,
    pub(super) country_code: Option<String>,
    pub(super) codec: Option<String>,
    pub(super) bitrate_kbps: Option<u32>,
    pub(super) score: f64,
    pub(super) reason: String,
    pub(super) health: &'static str,
}

/// Minimal public station representation that excludes ranking and provider metadata.
#[derive(Clone, Serialize)]
pub(super) struct PublicStationDto {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) stream_url: String,
    pub(super) homepage_url: Option<String>,
    pub(super) favicon_url: Option<String>,
    pub(super) tags: Vec<String>,
    pub(super) language: Option<String>,
    pub(super) country_code: Option<String>,
    pub(super) codec: Option<String>,
    pub(super) bitrate_kbps: Option<u32>,
}

impl From<&crate::search::Station> for PublicStationDto {
    fn from(station: &crate::search::Station) -> Self {
        Self {
            id: station.id.clone(),
            name: station.name.clone(),
            stream_url: station.stream_url.clone(),
            homepage_url: station.homepage_url.clone(),
            favicon_url: station.favicon_url.clone(),
            tags: station.tags.clone(),
            language: station.language.clone(),
            country_code: station.country_code.clone(),
            codec: station.codec.clone(),
            bitrate_kbps: station.bitrate_kbps,
        }
    }
}

/// Bounded public catalog page with an opaque next cursor.
#[derive(Serialize)]
pub(super) struct CatalogPageDto {
    pub(super) request_id: String,
    pub(super) stations: Vec<PublicStationDto>,
    pub(super) next_cursor: Option<String>,
}

/// Successful response for the stable voice-command JSON boundary.
#[derive(Serialize)]
pub(super) struct VoiceCommandResponseDto {
    pub(super) request_id: String,
    pub(super) transcript: String,
    pub(super) normalized_query: NormalizedQueryDto,
    pub(super) selected_station: Option<StationResultDto>,
    pub(super) stations: Vec<StationResultDto>,
}

impl From<&RankedStation> for StationResultDto {
    fn from(ranked: &RankedStation) -> Self {
        let station = &ranked.station;
        Self {
            id: station.id.clone(),
            name: station.name.clone(),
            stream_url: station.stream_url.clone(),
            homepage_url: station.homepage_url.clone(),
            favicon_url: station.favicon_url.clone(),
            tags: station.tags.clone(),
            language: station.language.clone(),
            country_code: station.country_code.clone(),
            codec: station.codec.clone(),
            bitrate_kbps: station.bitrate_kbps,
            score: ranked.score,
            reason: ranked.reason.clone(),
            health: match station.health {
                StationHealth::Healthy => "healthy",
                StationHealth::Degraded => "degraded",
                StationHealth::Unknown => "unknown",
            },
        }
    }
}

/// Contract-compliant transport error body.
#[derive(Serialize)]
pub(super) struct ErrorResponseDto {
    pub(super) code: String,
    pub(super) message: String,
    pub(super) request_id: String,
    pub(super) details: Value,
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderMap;

    use super::{
        FIRST_PARTY_ORIGIN, is_trusted_browser_request, request_rate_limit_scope,
        trusted_proxy_header_matches,
    };

    #[test]
    fn browser_state_changes_require_first_party_https_proxy_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", FIRST_PARTY_ORIGIN.parse().unwrap());
        headers.insert("x-forwarded-proto", "https".parse().unwrap());
        assert!(is_trusted_browser_request(&headers));
        headers.insert("origin", "https://evil.example".parse().unwrap());
        assert!(!is_trusted_browser_request(&headers));
    }

    #[test]
    fn browser_state_changes_require_the_configured_proxy_secret() {
        let mut headers = HeaderMap::new();
        headers.insert("x-rockserver-proxy", "proxy-secret".parse().unwrap());
        assert!(trusted_proxy_header_matches(&headers, Some("proxy-secret")));
        assert!(!trusted_proxy_header_matches(
            &headers,
            Some("wrong-secret")
        ));
        assert!(!trusted_proxy_header_matches(&headers, None));
    }

    #[test]
    fn rate_limit_scope_ignores_forwarded_identity_from_direct_peers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "198.51.100.10".parse().unwrap());
        assert_eq!(
            request_rate_limit_scope(&headers, Some("proxy-secret")),
            "direct-peer"
        );
        headers.insert("x-rockserver-proxy", "proxy-secret".parse().unwrap());
        assert_eq!(
            request_rate_limit_scope(&headers, Some("proxy-secret")),
            "198.51.100.10"
        );
    }
}
