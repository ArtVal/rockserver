//! Administrator-controlled station-icon import and public prepared-artifact delivery.

use std::{fmt::Write, time::Instant};

use axum::{
    Json,
    body::{Body, to_bytes},
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    admin::AdminRequestOutcome,
    station_icons::{MAX_SOURCE_BYTES, ManualIconError},
};

use super::{
    admin_auth::{active_session, record_request},
    state::AppState,
    transport::{
        error_response, is_trusted_admin_browser_request, parse_json_request, request_id,
        with_request_id,
    },
};

const ICON_CACHE_CONTROL: &str = "public, max-age=86400, must-revalidate";

#[derive(Deserialize)]
struct RemoveManualIconRequest {
    confirmation: String,
}

/// Starts one persisted import snapshot and detaches its bounded worker from the request.
pub(super) async fn start_import(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let request_id = request_id(&headers);
    let started = Instant::now();
    if !is_trusted_admin_browser_request(&headers, state.local_admin_origin.as_deref()) {
        return error_response(
            StatusCode::FORBIDDEN,
            "origin_required",
            "The request origin is not allowed.",
            &request_id,
            json!({}),
        );
    }
    let session = match active_session(&state, &headers, &request_id).await {
        Ok(session) => session,
        Err(response) => return *response,
    };
    let Some(importer) = state.icon_import.clone() else {
        return unavailable(&request_id);
    };
    let progress = match importer.start().await {
        Ok(progress) => progress,
        Err(_) => return unavailable(&request_id),
    };
    let job_id = progress.id;
    tokio::spawn(async move { importer.run(job_id).await });
    let response = with_request_id(
        (StatusCode::ACCEPTED, Json(progress)).into_response(),
        &request_id,
    );
    record_request(
        &state,
        &session,
        &request_id,
        "/api/v1/admin/icons/import",
        AdminRequestOutcome::Succeeded,
        started,
    )
    .await;
    response
}

/// Returns the last job snapshot so progress survives an administrator page reload.
pub(super) async fn latest_import(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let request_id = request_id(&headers);
    let started = Instant::now();
    let session = match active_session(&state, &headers, &request_id).await {
        Ok(session) => session,
        Err(response) => return *response,
    };
    let Some(importer) = state.icon_import.as_ref() else {
        return unavailable(&request_id);
    };
    let progress = match importer.latest_progress().await {
        Ok(progress) => progress,
        Err(_) => return unavailable(&request_id),
    };
    let response = with_request_id(Json(progress).into_response(), &request_id);
    record_request(
        &state,
        &session,
        &request_id,
        "/api/v1/admin/icons/import",
        AdminRequestOutcome::Succeeded,
        started,
    )
    .await;
    response
}

/// Returns an individual persisted job's safe counters while it is still being processed.
pub(super) async fn import_progress(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let request_id = request_id(&headers);
    let started = Instant::now();
    let session = match active_session(&state, &headers, &request_id).await {
        Ok(session) => session,
        Err(response) => return *response,
    };
    let Some(importer) = state.icon_import.as_ref() else {
        return unavailable(&request_id);
    };
    match importer.progress(id).await {
        Ok(Some(progress)) => {
            let response = with_request_id(Json(progress).into_response(), &request_id);
            record_request(
                &state,
                &session,
                &request_id,
                "/api/v1/admin/icons/import/{job_id}",
                AdminRequestOutcome::Succeeded,
                started,
            )
            .await;
            response
        }
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "Icon import job was not found.",
            &request_id,
            json!({}),
        ),
        Err(_) => unavailable(&request_id),
    }
}

/// Validates and publishes a manual administrator override from a bounded raw image body.
pub(super) async fn replace_manual(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(station_id): Path<String>,
    body: Body,
) -> Response {
    let request_id = request_id(&headers);
    let started = Instant::now();
    let session = match trusted_admin_session(&state, &headers, &request_id).await {
        Ok(session) => session,
        Err(response) => return *response,
    };
    if invalid_station_id(&station_id) {
        return invalid_station(&request_id);
    }
    let source = match to_bytes(body, MAX_SOURCE_BYTES).await {
        Ok(source) => source,
        Err(_) => return upload_too_large(&request_id),
    };
    let Some(importer) = state.icon_import.as_ref() else {
        return unavailable(&request_id);
    };
    match importer.replace_manual(&station_id, &source).await {
        Ok(()) => {
            let response = with_request_id(StatusCode::NO_CONTENT.into_response(), &request_id);
            record_request(
                &state,
                &session,
                &request_id,
                "/api/v1/admin/stations/{station_id}/icon",
                AdminRequestOutcome::Succeeded,
                started,
            )
            .await;
            response
        }
        Err(ManualIconError::Validation(_)) => invalid_upload(&request_id),
        Err(ManualIconError::NotFound) => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "Station was not found.",
            &request_id,
            json!({}),
        ),
        Err(ManualIconError::Unavailable) => unavailable(&request_id),
    }
}

/// Removes a manual override only after an explicit JSON confirmation from the trusted admin UI.
pub(super) async fn remove_manual(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(station_id): Path<String>,
    body: Body,
) -> Response {
    let request_id = request_id(&headers);
    let started = Instant::now();
    let session = match trusted_admin_session(&state, &headers, &request_id).await {
        Ok(session) => session,
        Err(response) => return *response,
    };
    if invalid_station_id(&station_id) {
        return invalid_station(&request_id);
    }
    match parse_json_request::<RemoveManualIconRequest>(&headers, body, &request_id).await {
        Ok(RemoveManualIconRequest { confirmation }) if confirmation == "DELETE" => {}
        Ok(_) => {
            return error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation_failed",
                "Icon removal requires confirmation.",
                &request_id,
                json!({"field":"confirmation"}),
            );
        }
        Err(response) => return *response,
    }
    let Some(importer) = state.icon_import.as_ref() else {
        return unavailable(&request_id);
    };
    match importer.remove_manual(&station_id).await {
        Ok(()) => {
            let response = with_request_id(StatusCode::NO_CONTENT.into_response(), &request_id);
            record_request(
                &state,
                &session,
                &request_id,
                "/api/v1/admin/stations/{station_id}/icon",
                AdminRequestOutcome::Succeeded,
                started,
            )
            .await;
            response
        }
        Err(ManualIconError::NotFound) => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "Manual station icon was not found.",
            &request_id,
            json!({}),
        ),
        Err(ManualIconError::Validation(_) | ManualIconError::Unavailable) => {
            unavailable(&request_id)
        }
    }
}

/// Delivers only prepared WebP artifacts, with a strong validator derived from their content hash.
pub(super) async fn public_icon(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(station_id): Path<String>,
) -> Response {
    let request_id = request_id(&headers);
    if station_id.is_empty() || station_id.len() > 128 {
        return missing_icon(&request_id);
    }
    let Some(importer) = state.icon_import.as_ref() else {
        return missing_icon(&request_id);
    };
    let icon = match importer.ready_icon(&station_id).await {
        Ok(Some(icon)) => icon,
        Ok(None) | Err(_) => return missing_icon(&request_id),
    };
    let etag = content_etag(icon.content_hash);
    let mut response = if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == etag)
    {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        icon.bytes.into_response()
    };
    let response_headers = response.headers_mut();
    response_headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/webp"));
    response_headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(ICON_CACHE_CONTROL),
    );
    response_headers.insert(
        header::ETAG,
        HeaderValue::from_str(&etag).expect("content hash ETag is valid"),
    );
    with_request_id(response, &request_id)
}

fn unavailable(request_id: &str) -> Response {
    error_response(
        StatusCode::SERVICE_UNAVAILABLE,
        "service_unavailable",
        "Station icon import is not configured.",
        request_id,
        json!({}),
    )
}

async fn trusted_admin_session(
    state: &AppState,
    headers: &HeaderMap,
    request_id: &str,
) -> Result<crate::admin::AdminSession, Box<Response>> {
    if !is_trusted_admin_browser_request(headers, state.local_admin_origin.as_deref()) {
        return Err(Box::new(error_response(
            StatusCode::FORBIDDEN,
            "origin_required",
            "The request origin is not allowed.",
            request_id,
            json!({}),
        )));
    }
    active_session(state, headers, request_id).await
}

fn invalid_station_id(station_id: &str) -> bool {
    station_id.is_empty() || station_id.len() > 128
}

fn invalid_station(request_id: &str) -> Response {
    error_response(
        StatusCode::BAD_REQUEST,
        "malformed_request",
        "Station identifier is invalid.",
        request_id,
        json!({"field":"station_id"}),
    )
}

fn invalid_upload(request_id: &str) -> Response {
    error_response(
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_failed",
        "Station icon must be a valid PNG, JPEG, WebP, or ICO within the size limit.",
        request_id,
        json!({"field":"icon"}),
    )
}

fn upload_too_large(request_id: &str) -> Response {
    error_response(
        StatusCode::PAYLOAD_TOO_LARGE,
        "request_too_large",
        "Station icon upload exceeds the allowed size.",
        request_id,
        json!({"max_bytes":MAX_SOURCE_BYTES}),
    )
}

fn missing_icon(request_id: &str) -> Response {
    let mut response = error_response(
        StatusCode::NOT_FOUND,
        "not_found",
        "Station icon was not found.",
        request_id,
        json!({}),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn content_etag(hash: [u8; 32]) -> String {
    let mut value = String::with_capacity(66);
    value.push('"');
    for byte in hash {
        write!(&mut value, "{byte:02x}").expect("writing to String cannot fail");
    }
    value.push('"');
    value
}
