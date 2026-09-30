//! Versioned administrator authentication HTTP boundary.

use std::{sync::Arc, time::Instant};

use argon2::{Argon2, PasswordHash, PasswordVerifier};
use axum::{
    Json,
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    admin::{
        AdminLoginAttempt, AdminLoginOutcome, AdminRequestOutcome, AdminRequestRecord,
        AdminSecurityEvent, AdminSecurityEventType, AdminSession, AdminStore, AdminUsername,
        NewAdminSession,
    },
    auth::SecretHash,
};

use super::{
    state::AppState,
    transport::{
        bearer_token, error_response, is_trusted_admin_browser_request, parse_json_request,
        request_id, request_rate_limit_scope, retry_after, unauthorized_response, with_request_id,
    },
};

const SESSION_TTL_SECONDS: i64 = 15 * 60;
const LOCKOUT_AFTER_FAILURES: u64 = 5;

/// Admin password-login payload; password never implements Debug or reaches audit metadata.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginRequest {
    username: String,
    password: String,
}

/// Deliberately short-lived opaque bearer returned only to an authenticated caller.
#[derive(Serialize)]
struct SessionResponse {
    access_token: String,
    token_type: &'static str,
    expires_in: i64,
}

/// Logs an administrator in after trusted-origin, durable-throttle, and Argon2id checks.
pub(super) async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let request_id = request_id(&headers);
    if !is_trusted_admin_browser_request(&headers, state.local_admin_origin.as_deref()) {
        return error_response(
            StatusCode::FORBIDDEN,
            "origin_required",
            "The request origin is not allowed.",
            &request_id,
            json!({}),
        );
    }
    let request = match parse_json_request::<LoginRequest>(&headers, body, &request_id).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let Ok(username) = AdminUsername::parse(request.username) else {
        return generic_login_failure(&state, &request_id, None, &headers, "invalid").await;
    };
    let Some(store) = state.admin_store.as_ref() else {
        return unavailable(&request_id);
    };
    let account_hash = secret_hash(username.as_str());
    let source_hash = secret_hash(&request_rate_limit_scope(
        &headers,
        state.trusted_proxy_token.as_deref(),
    ));
    let failures = match store
        .recent_failed_login_count(&account_hash, &source_hash)
        .await
    {
        Ok(value) => value,
        Err(_) => return unavailable(&request_id),
    };
    if failures >= LOCKOUT_AFTER_FAILURES {
        return locked(store, None, account_hash, source_hash, &request_id).await;
    }
    let credential = match store.login_credential(&username).await {
        Ok(value) => value,
        Err(_) => return unavailable(&request_id),
    };
    let valid = credential.as_ref().is_some_and(|value| {
        PasswordHash::new(value.credential.password_hash.as_str())
            .ok()
            .is_some_and(|hash| {
                Argon2::default()
                    .verify_password(request.password.as_bytes(), &hash)
                    .is_ok()
            })
    });
    if !valid {
        return generic_login_failure(
            &state,
            &request_id,
            credential.map(|value| value.credential.principal_id),
            &headers,
            username.as_str(),
        )
        .await;
    }
    let credential = credential.expect("verified credential exists").credential;
    let token = new_token();
    let session = NewAdminSession {
        id: Uuid::new_v4(),
        principal_id: credential.principal_id,
        token_hash: secret_hash(&token),
        ttl_seconds: SESSION_TTL_SECONDS,
    };
    if store.create_session(session.clone()).await.is_err() {
        return unavailable(&request_id);
    }
    for event in [
        AdminSecurityEventType::LoginSucceeded,
        AdminSecurityEventType::SessionCreated,
    ] {
        let _ = store
            .record_security_event(AdminSecurityEvent {
                id: Uuid::new_v4(),
                principal_id: Some(credential.principal_id),
                session_id: Some(session.id),
                source_ip_hash: Some(source_hash.clone()),
                event_type: event,
            })
            .await;
    }
    let _ = store
        .record_login_attempt(AdminLoginAttempt {
            id: Uuid::new_v4(),
            principal_id: Some(credential.principal_id),
            account_key_hash: account_hash,
            source_ip_hash: source_hash,
            outcome: AdminLoginOutcome::Succeeded,
        })
        .await;
    with_request_id(
        (
            StatusCode::OK,
            Json(SessionResponse {
                access_token: token,
                token_type: "Bearer",
                expires_in: SESSION_TTL_SECONDS,
            }),
        )
            .into_response(),
        &request_id,
    )
}

/// Rotates a valid administrator bearer session.
pub(super) async fn refresh(State(state): State<AppState>, headers: HeaderMap) -> Response {
    rotate(state, headers, false).await
}
/// Revokes a valid administrator bearer session immediately.
pub(super) async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    rotate(state, headers, true).await
}

/// Proves that an admin-only route accepts only active administrator Bearer sessions.
pub(super) async fn session(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let request_id = request_id(&headers);
    let started = Instant::now();
    match active_session(&state, &headers, &request_id).await {
        Ok(session) => {
            let response = with_request_id(
                (StatusCode::OK, Json(json!({"role":"admin"}))).into_response(),
                &request_id,
            );
            record_request(
                &state,
                &session,
                &request_id,
                "/api/v1/admin/session",
                AdminRequestOutcome::Succeeded,
                started,
            )
            .await;
            response
        }
        Err(response) => *response,
    }
}

/// Resolves an active administrator bearer session without exposing session identity.
pub(super) async fn active_session(
    state: &AppState,
    headers: &HeaderMap,
    request_id: &str,
) -> Result<AdminSession, Box<Response>> {
    let Some(token) = bearer_token(headers) else {
        return Err(Box::new(unauthorized_response(request_id)));
    };
    let Some(store) = state.admin_store.as_ref() else {
        return Err(Box::new(unavailable(request_id)));
    };
    match store.find_active_session(&secret_hash(token)).await {
        Ok(Some(session)) => Ok(session),
        Ok(None) => Err(Box::new(unauthorized_response(request_id))),
        Err(_) => Err(Box::new(unavailable(request_id))),
    }
}

async fn rotate(state: AppState, headers: HeaderMap, logout: bool) -> Response {
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
    let Some(token) = bearer_token(&headers) else {
        return unauthorized_response(&request_id);
    };
    let Some(store) = state.admin_store.as_ref() else {
        return unavailable(&request_id);
    };
    let session = match store.find_active_session(&secret_hash(token)).await {
        Ok(Some(value)) => value,
        Ok(None) => return unauthorized_response(&request_id),
        Err(_) => return unavailable(&request_id),
    };
    let source_hash = secret_hash(&request_rate_limit_scope(
        &headers,
        state.trusted_proxy_token.as_deref(),
    ));
    if logout {
        if store.revoke_session(session.id, None).await != Ok(true) {
            return unauthorized_response(&request_id);
        }
        let _ = store
            .record_security_event(AdminSecurityEvent {
                id: Uuid::new_v4(),
                principal_id: Some(session.principal_id),
                session_id: Some(session.id),
                source_ip_hash: Some(source_hash),
                event_type: AdminSecurityEventType::Logout,
            })
            .await;
        let response = with_request_id(StatusCode::NO_CONTENT.into_response(), &request_id);
        record_request(
            &state,
            &session,
            &request_id,
            "/api/v1/admin/auth/logout",
            AdminRequestOutcome::Succeeded,
            started,
        )
        .await;
        return response;
    }
    let token = new_token();
    let replacement = NewAdminSession {
        id: Uuid::new_v4(),
        principal_id: session.principal_id,
        token_hash: secret_hash(&token),
        ttl_seconds: SESSION_TTL_SECONDS,
    };
    if store.rotate_session(session.id, replacement.clone()).await != Ok(true) {
        return unavailable(&request_id);
    }
    let _ = store
        .record_security_event(AdminSecurityEvent {
            id: Uuid::new_v4(),
            principal_id: Some(session.principal_id),
            session_id: Some(session.id),
            source_ip_hash: Some(source_hash),
            event_type: AdminSecurityEventType::SessionRotated,
        })
        .await;
    let response = with_request_id(
        (
            StatusCode::OK,
            Json(SessionResponse {
                access_token: token,
                token_type: "Bearer",
                expires_in: SESSION_TTL_SECONDS,
            }),
        )
            .into_response(),
        &request_id,
    );
    record_request(
        &state,
        &session,
        &request_id,
        "/api/v1/admin/auth/refresh",
        AdminRequestOutcome::Succeeded,
        started,
    )
    .await;
    response
}

/// Persists only bounded metadata for a completed authenticated admin route; failures stay out of responses.
pub(super) async fn record_request(
    state: &AppState,
    session: &AdminSession,
    request_id: &str,
    endpoint: &'static str,
    outcome: AdminRequestOutcome,
    started: Instant,
) {
    let Some(store) = state.admin_store.as_ref() else {
        return;
    };
    let duration_ms = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
    let _ = store
        .record_request(AdminRequestRecord {
            id: Uuid::new_v4(),
            request_id: request_id.to_owned(),
            principal_id: session.principal_id,
            session_id: session.id,
            endpoint,
            outcome,
            duration_ms,
        })
        .await;
}

async fn generic_login_failure(
    state: &AppState,
    request_id: &str,
    principal_id: Option<Uuid>,
    headers: &HeaderMap,
    account: &str,
) -> Response {
    let Some(store) = state.admin_store.as_ref() else {
        return unavailable(request_id);
    };
    let account_hash = secret_hash(account);
    let source_hash = secret_hash(&request_rate_limit_scope(
        headers,
        state.trusted_proxy_token.as_deref(),
    ));
    let _ = store
        .record_login_attempt(AdminLoginAttempt {
            id: Uuid::new_v4(),
            principal_id,
            account_key_hash: account_hash,
            source_ip_hash: source_hash.clone(),
            outcome: AdminLoginOutcome::Failed,
        })
        .await;
    let _ = store
        .record_security_event(AdminSecurityEvent {
            id: Uuid::new_v4(),
            principal_id,
            session_id: None,
            source_ip_hash: Some(source_hash),
            event_type: AdminSecurityEventType::LoginFailed,
        })
        .await;
    error_response(
        StatusCode::UNAUTHORIZED,
        "invalid_credentials",
        "The supplied credentials are invalid.",
        request_id,
        json!({}),
    )
}

async fn locked(
    store: &Arc<dyn AdminStore>,
    principal_id: Option<Uuid>,
    account_hash: SecretHash,
    source_hash: SecretHash,
    request_id: &str,
) -> Response {
    let _ = store
        .record_login_attempt(AdminLoginAttempt {
            id: Uuid::new_v4(),
            principal_id,
            account_key_hash: account_hash,
            source_ip_hash: source_hash.clone(),
            outcome: AdminLoginOutcome::Locked,
        })
        .await;
    let _ = store
        .record_security_event(AdminSecurityEvent {
            id: Uuid::new_v4(),
            principal_id,
            session_id: None,
            source_ip_hash: Some(source_hash),
            event_type: AdminSecurityEventType::LoginLocked,
        })
        .await;
    retry_after(
        error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "login_temporarily_locked",
            "Login is temporarily unavailable.",
            request_id,
            json!({}),
        ),
        15 * 60,
    )
}

fn secret_hash(value: &str) -> SecretHash {
    SecretHash::new(Sha256::digest(value.as_bytes()).into())
}
fn new_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}
fn unavailable(request_id: &str) -> Response {
    error_response(
        StatusCode::SERVICE_UNAVAILABLE,
        "admin_auth_unavailable",
        "Administrator authentication is temporarily unavailable.",
        request_id,
        json!({}),
    )
}
