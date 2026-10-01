//! Authorized latest-title reads and bounded server-sent events.

use std::{convert::Infallible, sync::Arc, time::Duration};

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, Uri, header},
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
};
use serde::Serialize;
use tokio::{sync::mpsc, time::Instant};
use tokio_stream::wrappers::ReceiverStream;

use super::super::{
    state::AppState,
    transport::{request_id, retry_after, with_request_id},
};
use super::{RelayState, failure, station_access};

const STALE_AFTER: Duration = Duration::from_secs(120);
const EVENT_LIFETIME: Duration = Duration::from_secs(60 * 60);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NowPlaying {
    station_id: String,
    raw_title: Option<String>,
    updated_at: Option<String>,
    state: &'static str,
}

fn snapshot(relay: &RelayState, station_id: &str) -> NowPlaying {
    let titles = relay.titles.lock().expect("title mutex not poisoned");
    let title = titles.get(station_id);
    NowPlaying {
        station_id: station_id.to_owned(),
        raw_title: title.map(|title| title.raw_title.clone()),
        updated_at: title.map(|title| title.updated_at.clone()),
        state: match title {
            None => "missing",
            Some(title) if title.observed_at.elapsed() >= STALE_AFTER => "stale",
            Some(_) => "fresh",
        },
    }
}

fn stale_deadline(relay: &RelayState, station_id: &str) -> Option<Instant> {
    relay
        .titles
        .lock()
        .expect("title mutex not poisoned")
        .get(station_id)
        .filter(|title| title.observed_at.elapsed() < STALE_AFTER)
        .map(|title| Instant::from_std(title.observed_at + STALE_AFTER))
}

fn event(snapshot: NowPlaying) -> Event {
    Event::default()
        .event("snapshot")
        .json_data(snapshot)
        .expect("snapshot is serializable")
}

/// Returns the last observed title, including explicit missing or stale state.
pub(in crate::http::endpoints) async fn now_playing(
    State(state): State<AppState>,
    Path(station_id): Path<String>,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    let id = request_id(&headers);
    if let Err(response) = station_access(&state, &station_id, &uri, &headers, &id).await {
        return response;
    }
    let mut response = with_request_id(
        Json(snapshot(&state.relay, &station_id)).into_response(),
        &id,
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}

/// Sends an immediate snapshot, then title changes until disconnect or the one-hour limit.
pub(in crate::http::endpoints) async fn events(
    State(state): State<AppState>,
    Path(station_id): Path<String>,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    let id = request_id(&headers);
    if let Err(response) = station_access(&state, &station_id, &uri, &headers, &id).await {
        return response;
    }
    let Ok(permit) = Arc::clone(&state.relay.subscribers).try_acquire_owned() else {
        return retry_after(
            failure(
                StatusCode::TOO_MANY_REQUESTS,
                "relay_subscriber_capacity",
                "Metadata subscribers are at capacity.",
                &id,
            ),
            5,
        );
    };
    let relay = state.relay.clone();
    // Subscribe before reading the snapshot so a concurrent update cannot be missed.
    let mut changes = relay.changes.subscribe();
    let (sender, receiver) = mpsc::channel(2);
    sender
        .try_send(Ok::<_, Infallible>(event(snapshot(&relay, &station_id))))
        .expect("initial slot is free");
    tokio::spawn(async move {
        let _permit = permit;
        let deadline = Instant::now() + EVENT_LIFETIME;
        let mut stale_at = stale_deadline(&relay, &station_id);
        loop {
            tokio::select! {
                _ = sender.closed() => break,
                _ = tokio::time::sleep_until(deadline) => break,
                _ = async {
                    match stale_at {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending().await,
                    }
                } => {
                    stale_at = None;
                    if sender.try_send(Ok(event(snapshot(&relay, &station_id)))).is_err() { break; }
                }
                change = changes.recv() => match change {
                    Ok((changed_id, _)) if changed_id == station_id => {
                        // A full queue means the subscriber cannot keep up; reconnect gets current state.
                        if sender.try_send(Ok(event(snapshot(&relay, &station_id)))).is_err() { break; }
                        stale_at = stale_deadline(&relay, &station_id);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        changes = relay.changes.subscribe();
                        if sender.try_send(Ok(event(snapshot(&relay, &station_id)))).is_err() { break; }
                        stale_at = stale_deadline(&relay, &station_id);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    _ => {}
                }
            }
        }
    });
    let mut response =
        with_request_id(Sse::new(ReceiverStream::new(receiver)).into_response(), &id);
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}
