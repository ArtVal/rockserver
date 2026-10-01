//! Authenticated, bounded station stream relay and latest ICY title snapshot.

#[path = "relay/icy.rs"]
mod icy;
#[path = "relay/metadata.rs"]
mod metadata;
#[path = "relay/security.rs"]
mod security;

pub(super) use metadata::{events, now_playing};

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    body::{Body, Bytes},
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use reqwest::Url;
use serde_json::json;
use tokio::{
    sync::{Semaphore, broadcast, mpsc},
    time::timeout,
};
use tokio_stream::wrappers::ReceiverStream;

use super::{
    control_auth::authenticate_control_ingress,
    state::AppState,
    transport::{
        cookie_value, error_response, request_id, retry_after, token_hash,
        trusted_proxy_header_matches, with_request_id,
    },
};

const MAX_CONNECTIONS: usize = 64;
const MAX_REDIRECTS: usize = 3;
const MAX_HEADERS: usize = 16 * 1024;
const MAX_METAINT: usize = 1024 * 1024;
const STREAM_LIFETIME: Duration = Duration::from_secs(60 * 60);

/// Latest bounded title observed from an active relay connection.
#[derive(Clone)]
pub(crate) struct TitleSnapshot {
    pub(crate) raw_title: String,
    pub(crate) observed_at: Instant,
    pub(crate) updated_at: String,
}

/// Process-local relay capacity and best-effort title snapshots and notifications.
#[derive(Clone)]
pub(crate) struct RelayState {
    slots: Arc<Semaphore>,
    titles: Arc<Mutex<HashMap<String, TitleSnapshot>>>,
    subscribers: Arc<Semaphore>,
    changes: broadcast::Sender<(String, TitleSnapshot)>,
    allow_loopback: bool,
}

impl Default for RelayState {
    fn default() -> Self {
        let (changes, _) = broadcast::channel(64);
        Self {
            slots: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
            titles: Arc::new(Mutex::new(HashMap::new())),
            subscribers: Arc::new(Semaphore::new(64)),
            changes,
            allow_loopback: false,
        }
    }
}

fn failure(status: StatusCode, code: &str, message: &str, id: &str) -> Response {
    error_response(status, code, message, id, json!({}))
}

/// Relays a catalog station to a current browser or native device session.
pub(super) async fn stream(
    State(state): State<AppState>,
    Path(station_id): Path<String>,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    let id = request_id(&headers);
    let url = match station_access(&state, &station_id, &uri, &headers, &id).await {
        Ok(url) => url,
        Err(response) => return response,
    };
    let Ok(url) = Url::parse(&url) else {
        return failure(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream",
            "Station upstream is invalid.",
            &id,
        );
    };
    let Ok(permit) = Arc::clone(&state.relay.slots).try_acquire_owned() else {
        return retry_after(
            failure(
                StatusCode::TOO_MANY_REQUESTS,
                "relay_capacity",
                "Relay is at capacity.",
                &id,
            ),
            5,
        );
    };
    let upstream = match open_upstream(url, state.relay.allow_loopback).await {
        Ok(upstream) => upstream,
        Err(_) => {
            return failure(
                StatusCode::BAD_GATEWAY,
                "upstream_unavailable",
                "Station upstream is unavailable.",
                &id,
            );
        }
    };
    let wants_icy = headers
        .get("icy-metadata")
        .is_some_and(|value| value == "1");
    let metaint = upstream
        .headers()
        .get("icy-metaint")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| (1..=MAX_METAINT).contains(value));
    let content_type = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("application/octet-stream"));
    let (sender, receiver) = mpsc::channel::<Result<Bytes, std::io::Error>>(2);
    let relay = state.relay.clone();
    tokio::spawn(async move {
        let _permit = permit;
        tokio::select! {
            _ = sender.closed() => {},
            _ = tokio::time::sleep(STREAM_LIFETIME) => {},
            _ = pump(icy::Reader::new(upstream), metaint, wants_icy, &station_id, relay, sender.clone()) => {},
        }
    });
    let mut response = with_request_id(
        Body::from_stream(ReceiverStream::new(receiver)).into_response(),
        &id,
    );
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, content_type);
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    if wants_icy && let Some(metaint) = metaint {
        response.headers_mut().insert(
            "icy-metaint",
            HeaderValue::from_str(&metaint.to_string()).expect("bounded decimal metaint"),
        );
    }
    response
}

// Shared gate: neither metadata endpoint nor audio opens an upstream before session/catalog checks.
async fn station_access(
    state: &AppState,
    station_id: &str,
    uri: &Uri,
    headers: &HeaderMap,
    id: &str,
) -> Result<String, Response> {
    if uri.query().is_some() {
        return Err(failure(
            StatusCode::BAD_REQUEST,
            "invalid_query",
            "Query parameters are not accepted.",
            id,
        ));
    }
    if station_id.is_empty() || station_id.len() > 128 {
        return Err(failure(
            StatusCode::BAD_REQUEST,
            "invalid_station_id",
            "Station identifier is invalid.",
            id,
        ));
    }
    // A bearer takes precedence; a malformed bearer cannot silently fall back to a cookie.
    let authorized = if headers.contains_key(header::AUTHORIZATION) {
        match state.control_session_resolver.as_ref() {
            Some(resolver) => authenticate_control_ingress(headers, resolver.as_ref())
                .await
                .is_ok(),
            None => false,
        }
    } else if let (Some(cookie), Some(store)) = (
        cookie_value(headers, "rockserver_browser"),
        state.account_store.as_ref(),
    ) {
        if !trusted_proxy_header_matches(headers, state.trusted_proxy_token.as_deref()) {
            false
        } else {
            matches!(
                store.browser_session_user(&token_hash(cookie)).await,
                Ok(Some(_))
            )
        }
    } else {
        false
    };
    if !authorized {
        return Err(failure(
            StatusCode::UNAUTHORIZED,
            "authentication_required",
            "A current user or device session is required.",
            id,
        ));
    }

    let station = match state.search_service.public_station(station_id).await {
        Ok(Some(station)) => station,
        Ok(None) => {
            return Err(failure(
                StatusCode::NOT_FOUND,
                "station_not_found",
                "Station is unavailable.",
                id,
            ));
        }
        Err(_) => {
            return Err(failure(
                StatusCode::SERVICE_UNAVAILABLE,
                "catalog_unavailable",
                "Catalog is unavailable.",
                id,
            ));
        }
    };
    Ok(station.stream_url)
}

// Each hop gets a fresh pinned client; a redirect never inherits trust from its parent.
async fn open_upstream(mut url: Url, allow_loopback: bool) -> Result<reqwest::Response, ()> {
    for hop in 0..=MAX_REDIRECTS {
        let client = security::pinned_client(&url, allow_loopback).await?;
        let response = timeout(
            Duration::from_secs(10),
            client.get(url.clone()).header("Icy-MetaData", "1").send(),
        )
        .await
        .map_err(|_| ())?
        .map_err(|_| ())?;
        if response.status().is_redirection() {
            if hop == MAX_REDIRECTS {
                return Err(());
            }
            let location = response
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or(())?;
            url = url.join(location).map_err(|_| ())?;
            continue;
        }
        if !response.status().is_success()
            || response
                .headers()
                .iter()
                .map(|(k, v)| k.as_str().len() + v.as_bytes().len())
                .sum::<usize>()
                > MAX_HEADERS
        {
            return Err(());
        }
        return Ok(response);
    }
    Err(())
}

// Keep only one transport chunk and one ICY block while applying backpressure to a slow client.
async fn pump(
    mut reader: icy::Reader,
    metaint: Option<usize>,
    wants_icy: bool,
    station_id: &str,
    relay: RelayState,
    sender: mpsc::Sender<Result<Bytes, std::io::Error>>,
) {
    let mut remaining = metaint.unwrap_or(usize::MAX);
    loop {
        if remaining > 0 {
            match reader.take(remaining.min(16 * 1024)).await {
                Ok(Some(bytes)) => {
                    remaining -= bytes.len();
                    if sender.send(Ok(bytes)).await.is_err() {
                        return;
                    }
                }
                _ => return,
            }
            continue;
        }
        let Ok(Some(length)) = reader.exact(1).await else {
            return;
        };
        let size = usize::from(length[0]) * 16;
        let Ok(Some(block)) = reader.exact(size).await else {
            return;
        };
        if let Some(raw_title) = icy::raw_title(&block) {
            let snapshot = TitleSnapshot {
                raw_title,
                observed_at: Instant::now(),
                updated_at: time::OffsetDateTime::now_utc()
                    .format(&time::format_description::well_known::Rfc3339)
                    .expect("current time is representable"),
            };
            let mut snapshots = relay.titles.lock().expect("title mutex not poisoned");
            if snapshots.len() >= 1024
                && !snapshots.contains_key(station_id)
                && let Some(oldest) = snapshots
                    .iter()
                    .min_by_key(|(_, item)| item.observed_at)
                    .map(|(id, _)| id.clone())
            {
                snapshots.remove(&oldest);
            }
            snapshots.insert(station_id.to_owned(), snapshot.clone());
            drop(snapshots);
            let _ = relay.changes.send((station_id.to_owned(), snapshot));
        }
        if wants_icy && sender.send(Ok(Bytes::from(length))).await.is_ok() && !block.is_empty() {
            if sender.send(Ok(Bytes::from(block))).await.is_err() {
                return;
            }
        } else if wants_icy && sender.is_closed() {
            return;
        }
        remaining = metaint.expect("metadata interval exists when remaining reaches zero");
    }
}

#[cfg(test)]
#[path = "relay/tests.rs"]
mod tests;
