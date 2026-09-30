//! Pairing-request HTTP handlers and their browser-safe DTOs.

use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::auth::{
    MAX_ACCOUNT_DEVICES, NewPairingRequest, NewPairingSession, PairingCompletionOutcome,
};

use super::{
    state::AppState,
    transport::{
        cookie_value, error_response, is_trusted_browser_request, parse_json_request, request_id,
        request_rate_limit_scope, retry_after, token_hash, trusted_proxy_header_matches,
        with_request_id,
    },
};

const PAIRING_LIFETIME_MINUTES: i32 = 10;
const PAIRING_CREATE_LIMIT: i64 = 10;
const PAIRING_RATE_LIMIT_MINUTES: i32 = 15;

/// Produces a human-verifiable phrase from random server-generated request material.
fn pairing_phrase(request_id: &Uuid) -> String {
    const WORDS: [&str; 16] = [
        "AMBER", "BIRCH", "CORAL", "DAWN", "EMBER", "FJORD", "GROVE", "HARBOR", "IVORY", "JUNIPER",
        "KITE", "LUNAR", "MAPLE", "NOVA", "OCEAN", "PINE",
    ];
    let bytes = request_id.as_bytes();
    format!(
        "{}-{}",
        WORDS[usize::from(bytes[0] & 15)],
        WORDS[usize::from(bytes[1] & 15)]
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreatePairingRequestDto {
    device_display_name: String,
    device_type: String,
    #[serde(default)]
    app_version: Option<String>,
}

#[derive(Serialize)]
struct CreatedPairingRequestDto {
    pairing_request_id: String,
    desktop_token: String,
    approval_secret: String,
    short_code: String,
    verification_phrase: String,
    device_display_name: String,
    device_type: String,
    expires_at: String,
    status: &'static str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
/// Short code submitted by a browser pairing lookup.
pub(super) struct PairingLookupDto {
    code: String,
}

#[derive(Serialize)]
struct PairingPreviewDto {
    pub(super) request_id: String,
    pub(super) device_display_name: String,
    pub(super) device_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) app_version: Option<String>,
    pub(super) verification_phrase: String,
    pub(super) short_code: String,
    pub(super) expires_at: String,
    pub(super) status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) account_display_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
/// Browser proof submitted to approve a pairing request.
pub(super) struct PairingApprovalDto {
    approval_secret: String,
    verification_phrase: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
/// Native proof submitted to complete an approved pairing request.
pub(super) struct PairingCompletionDto {
    desktop_token: String,
}

#[derive(Serialize)]
struct PairingCompletionResponseDto {
    user_id: String,
    device_id: String,
    session_id: String,
    access_token: String,
    access_expires_at: String,
    device_secret: String,
    account_display_name: String,
    device_display_name: String,
    device_type: String,
}

/// Starts a short-lived desktop pairing request without disclosing its hashed server-side proofs.
pub(super) async fn create_pairing_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let request_id = request_id(&headers);
    let payload: CreatePairingRequestDto =
        match parse_json_request(&headers, body, &request_id).await {
            Ok(payload) => payload,
            Err(response) => return *response,
        };
    if payload.device_display_name.trim().is_empty()
        || payload.device_display_name.len() > 128
        || payload.device_type.trim().is_empty()
        || payload.device_type.len() > 64
        || payload
            .app_version
            .as_ref()
            .is_some_and(|value| value.len() > 64)
    {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
            "Request validation failed.",
            &request_id,
            json!({"field":"device_display_name or device_type"}),
        );
    }
    let Some(store) = state.account_store.as_ref() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "auth_unavailable",
            "Account service is unavailable.",
            &request_id,
            json!({}),
        );
    };
    let rate_scope = request_rate_limit_scope(&headers, state.trusted_proxy_token.as_deref());
    let rate_key = token_hash(&format!("pairing-create:{rate_scope}"));
    match store
        .consume_rate_limit_for_minutes(&rate_key, PAIRING_RATE_LIMIT_MINUTES, PAIRING_CREATE_LIMIT)
        .await
    {
        Ok(true) => {}
        Ok(false) => {
            return retry_after(
                error_response(
                    StatusCode::TOO_MANY_REQUESTS,
                    "rate_limited",
                    "Request rate limit exceeded.",
                    &request_id,
                    json!({"limit_scope":"pairing_create"}),
                ),
                (PAIRING_RATE_LIMIT_MINUTES as u64) * 60,
            );
        }
        Err(_) => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "pairing_unavailable",
                "Pairing service is temporarily unavailable.",
                &request_id,
                json!({}),
            );
        }
    }
    let pairing_request_id = Uuid::new_v4();
    let desktop_token = Uuid::new_v4().simple().to_string();
    let approval_secret = Uuid::new_v4().simple().to_string();
    let short_code = Uuid::new_v4().simple().to_string()[..8].to_ascii_uppercase();
    let verification_phrase = pairing_phrase(&pairing_request_id);
    let desktop_token_hash = token_hash(&desktop_token);
    let approval_secret_hash = token_hash(&approval_secret);
    let short_code_hash = token_hash(&short_code);
    let request = NewPairingRequest {
        request_id: pairing_request_id,
        desktop_token_hash: &desktop_token_hash,
        approval_secret_hash: &approval_secret_hash,
        short_code_hash: &short_code_hash,
        verification_phrase: &verification_phrase,
        device_display_name: payload.device_display_name.trim(),
        device_type: payload.device_type.trim(),
        app_version: payload.app_version.as_deref(),
        expires_at_rfc3339: "unused: database clock",
    };
    match store
        .create_pairing_request_for_minutes(request, PAIRING_LIFETIME_MINUTES)
        .await
    {
        Ok(Some(expires_at)) => {
            let mut response = with_request_id(
                (
                    StatusCode::CREATED,
                    Json(CreatedPairingRequestDto {
                        pairing_request_id: pairing_request_id.to_string(),
                        desktop_token,
                        approval_secret,
                        short_code,
                        verification_phrase,
                        device_display_name: payload.device_display_name.trim().to_owned(),
                        device_type: payload.device_type.trim().to_owned(),
                        expires_at,
                        status: "pending",
                    }),
                )
                    .into_response(),
                &request_id,
            );
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            response
        }
        Ok(None) | Err(_) => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "pairing_unavailable",
            "Pairing service is temporarily unavailable.",
            &request_id,
            json!({}),
        ),
    }
}

/// Resolves a short code to non-secret device information for a browser pairing screen.
pub(super) async fn lookup_pairing_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<PairingLookupDto>,
) -> Response {
    let request_id = request_id(&headers);
    if query.code.len() != 8 || !query.code.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return error_response(
            StatusCode::NOT_FOUND,
            "pairing_not_found",
            "Pairing request is unavailable.",
            &request_id,
            json!({}),
        );
    }
    let Some(store) = state.account_store.as_ref() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "auth_unavailable",
            "Account service is unavailable.",
            &request_id,
            json!({}),
        );
    };
    let account_display_name = match cookie_value(&headers, "rockserver_browser") {
        Some(cookie) => store
            .browser_account_display_name(&token_hash(cookie))
            .await
            .ok()
            .flatten(),
        None => None,
    };
    match store
        .lookup_pairing_request(&token_hash(&query.code.to_ascii_uppercase()))
        .await
    {
        Ok(Some(preview)) => with_request_id(
            Json(PairingPreviewDto {
                request_id: preview.request_id.to_string(),
                device_display_name: preview.device_display_name,
                device_type: preview.device_type,
                app_version: preview.app_version,
                verification_phrase: preview.verification_phrase,
                short_code: query.code.to_ascii_uppercase(),
                expires_at: preview.expires_at,
                status: "pending",
                account_display_name,
            })
            .into_response(),
            &request_id,
        ),
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            "pairing_not_found",
            "Pairing request is unavailable.",
            &request_id,
            json!({}),
        ),
        Err(_) => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "pairing_unavailable",
            "Pairing service is temporarily unavailable.",
            &request_id,
            json!({}),
        ),
    }
}

/// Approves a pairing request with a fresh browser session and double-submit CSRF proof.
pub(super) async fn approve_pairing_request(
    State(state): State<AppState>,
    Path(pairing_id): Path<Uuid>,
    headers: HeaderMap,
    Json(payload): Json<PairingApprovalDto>,
) -> Response {
    let request_id_header = request_id(&headers);
    if !is_trusted_browser_request(&headers)
        || !trusted_proxy_header_matches(&headers, state.trusted_proxy_token.as_deref())
    {
        return error_response(
            StatusCode::FORBIDDEN,
            "untrusted_request",
            "The request must originate from the trusted first-party proxy.",
            &request_id_header,
            json!({}),
        );
    }
    let Some(cookie) = cookie_value(&headers, "rockserver_browser") else {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "authentication_required",
            "A browser session is required.",
            &request_id_header,
            json!({}),
        );
    };
    let Some(csrf) = headers
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && value.len() <= 256)
    else {
        return error_response(
            StatusCode::FORBIDDEN,
            "csrf_failed",
            "A valid CSRF token is required.",
            &request_id_header,
            json!({}),
        );
    };
    let Some(store) = state.account_store.clone() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "auth_unavailable",
            "Account service is unavailable.",
            &request_id_header,
            json!({}),
        );
    };
    let approval_hash = token_hash(&payload.approval_secret);
    let session_hash = token_hash(cookie);
    let csrf_hash = token_hash(csrf);
    let approved = store
        .approve_pairing_request_with_browser_proof(
            pairing_id,
            &approval_hash,
            &payload.verification_phrase,
            &session_hash,
            &csrf_hash,
        )
        .await;
    match approved {
        Ok(true) => with_request_id(StatusCode::NO_CONTENT.into_response(), &request_id_header),
        Ok(false) => error_response(
            StatusCode::CONFLICT,
            "pairing_not_approvable",
            "Pairing request cannot be approved.",
            &request_id_header,
            json!({}),
        ),
        Err(_) => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "pairing_unavailable",
            "Pairing service is temporarily unavailable.",
            &request_id_header,
            json!({}),
        ),
    }
}

/// Atomically consumes an approved desktop request and returns native bearer credentials.
pub(super) async fn complete_pairing_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(pairing_id): Path<Uuid>,
    Json(payload): Json<PairingCompletionDto>,
) -> Response {
    let request_id = request_id(&headers);
    if payload.desktop_token.len() < 16 || payload.desktop_token.len() > 128 {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "pairing_rejected",
            "Pairing proof is invalid or expired.",
            &request_id,
            json!({}),
        );
    }
    let Some(store) = state.account_store.clone() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "auth_unavailable",
            "Account service is unavailable.",
            &request_id,
            json!({}),
        );
    };
    let access_token = Uuid::new_v4().simple().to_string();
    let device_secret = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let device_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();
    let access_hash = token_hash(&access_token);
    let device_secret_hash = token_hash(&device_secret);
    let session = NewPairingSession {
        session_id,
        device_id,
        access_hash: &access_hash,
        access_expires_at_rfc3339: "db:15m",
        device_secret_hash: &device_secret_hash,
    };
    match store
        .complete_pairing(pairing_id, &token_hash(&payload.desktop_token), session)
        .await
    {
        Ok(PairingCompletionOutcome::Completed(result)) => {
            let mut response = with_request_id(
                Json(PairingCompletionResponseDto {
                    user_id: result.user_id.to_string(),
                    device_id: result.device_id.to_string(),
                    session_id: result.session_id.to_string(),
                    access_token,
                    access_expires_at: result.access_expires_at,
                    device_secret,
                    account_display_name: result.account_display_name,
                    device_display_name: result.device_display_name,
                    device_type: result.device_type,
                })
                .into_response(),
                &request_id,
            );
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            response
        }
        Ok(PairingCompletionOutcome::Pending) => error_response(
            StatusCode::ACCEPTED,
            "pairing_pending",
            "Pairing is waiting for browser approval.",
            &request_id,
            json!({}),
        ),
        Ok(PairingCompletionOutcome::NoLongerAvailable) => error_response(
            StatusCode::GONE,
            "pairing_expired",
            "Pairing request is no longer available.",
            &request_id,
            json!({}),
        ),
        Ok(PairingCompletionOutcome::DeviceLimit) => {
            let message =
                format!("This account has reached the {MAX_ACCOUNT_DEVICES}-device limit.");
            error_response(
                StatusCode::CONFLICT,
                "device_limit_reached",
                &message,
                &request_id,
                json!({"limit": MAX_ACCOUNT_DEVICES}),
            )
        }
        Ok(PairingCompletionOutcome::InvalidProof) => error_response(
            StatusCode::UNAUTHORIZED,
            "pairing_rejected",
            "Pairing proof is invalid or expired.",
            &request_id,
            json!({}),
        ),
        Err(_) => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "pairing_unavailable",
            "Pairing service is temporarily unavailable.",
            &request_id,
            json!({}),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::PairingPreviewDto;

    #[test]
    fn pairing_preview_serializes_only_browser_safe_context() {
        let value = serde_json::to_value(PairingPreviewDto {
            request_id: "00000000-0000-0000-0000-000000000000".into(),
            device_display_name: "RockCast — This PC".into(),
            device_type: "rockcast_windows".into(),
            app_version: None,
            verification_phrase: "AMBER-FJORD".into(),
            short_code: "A1B2C3D4".into(),
            expires_at: "2030-01-01T00:00:00Z".into(),
            status: "pending",
            account_display_name: Some("Alex's Rock account".into()),
        })
        .unwrap();
        assert_eq!(value["status"], "pending");
        assert!(value.get("desktop_token").is_none());
        assert!(value.get("approval_secret").is_none());
        assert!(value.get("credential_id").is_none());
        assert!(value.get("device_secret").is_none());
    }
}
