//! Passkey, browser-session, and native-token authentication handlers.

use axum::{
    Json,
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use passkey_auth::{AuthenticationResponse, CredentialId, RegistrationResponse};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::auth::{
    BROWSER_SESSION_ABSOLUTE_MINUTES, NewBrowserSession, NewPasskeyRegistration,
    NewWebAuthnChallenge, PasskeyRegistrationOutcome, WebAuthnCeremony, webauthn,
};

use super::{
    state::AppState,
    transport::{
        cookie_value, error_response, is_trusted_browser_request, parse_json_request, request_id,
        token_hash, trusted_proxy_header_matches, unauthorized_response, with_request_id,
    },
};

/// Supplies the migration-safe label for account records created before naming is configurable.
fn default_account_display_name() -> String {
    "Rock account".to_owned()
}

#[derive(Serialize)]
struct BrowserSessionDto {
    account_display_name: String,
    csrf_token: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeviceSessionRequestDto {
    device_id: Uuid,
    device_secret: String,
}

#[derive(Serialize)]
struct DeviceSessionResponseDto {
    access_token: String,
    access_expires_at: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
/// WebAuthn registration assertion plus its server-side challenge ID.
pub(super) struct RegistrationVerifyDto {
    challenge_id: Uuid,
    #[serde(flatten)]
    response: RegistrationResponse,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistrationOptionsRequestDto {
    #[serde(default = "default_account_display_name")]
    account_display_name: String,
}

#[derive(Serialize)]
struct RegistrationOptionsDto {
    challenge_id: Uuid,
    options: passkey_auth::RegistrationChallenge,
}

#[derive(Deserialize, Serialize)]
struct RegistrationStateContext {
    user_id: Uuid,
    account_display_name: String,
    state: passkey_auth::RegistrationState,
}

#[derive(Serialize)]
struct AuthenticationOptionsResponseDto {
    challenge_id: Uuid,
    options: passkey_auth::AuthenticationChallenge,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
/// WebAuthn authentication assertion plus its server-side challenge ID.
pub(super) struct AuthenticationVerifyDto {
    challenge_id: Uuid,
    #[serde(flatten)]
    response: AuthenticationResponse,
}

/// Starts a first-party passkey registration and persists its opaque verifier state.
pub(super) async fn registration_options(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let request_id = request_id(&headers);
    let payload: RegistrationOptionsRequestDto =
        match parse_json_request(&headers, body, &request_id).await {
            Ok(payload) => payload,
            Err(response) => return *response,
        };
    let account_display_name = payload.account_display_name.trim();
    if account_display_name.is_empty() || account_display_name.len() > 128 {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
            "Request validation failed.",
            &request_id,
            json!({"field":"account_display_name"}),
        );
    }
    if !is_trusted_browser_request(&headers)
        || !trusted_proxy_header_matches(&headers, state.trusted_proxy_token.as_deref())
    {
        return error_response(
            StatusCode::FORBIDDEN,
            "untrusted_request",
            "The request must originate from the trusted first-party proxy.",
            &request_id,
            json!({}),
        );
    };
    let Some(store) = state.account_store.as_ref() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "auth_unavailable",
            "Account service is unavailable.",
            &request_id,
            json!({}),
        );
    };
    let user_id = Uuid::new_v4();
    let (options, state_blob) = webauthn::start_registration(user_id, account_display_name);
    let state_blob = RegistrationStateContext {
        user_id,
        account_display_name: account_display_name.to_owned(),
        state: state_blob,
    };
    let encoded = match webauthn::encode_state(&state_blob) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "auth_unavailable",
                "Authentication service is temporarily unavailable.",
                &request_id,
                json!({}),
            );
        }
    };
    let challenge_id = Uuid::new_v4();
    let challenge_hash = token_hash(&state_blob.state.challenge.to_b64url());
    let challenge = NewWebAuthnChallenge {
        challenge_id,
        challenge_hash: &challenge_hash,
        state_blob: &encoded,
        ceremony: WebAuthnCeremony::Registration,
        rp_id: webauthn::RP_ID,
        origin: webauthn::ORIGIN,
        expires_at_rfc3339: "unused: database clock",
        user_id: None,
        browser_session_id: None,
        pairing_request_id: None,
    };
    match store
        .create_webauthn_challenge_for_minutes(challenge, 5)
        .await
    {
        Ok(true) => with_request_id(
            Json(RegistrationOptionsDto {
                challenge_id,
                options,
            })
            .into_response(),
            &request_id,
        ),
        Ok(false) | Err(_) => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "auth_unavailable",
            "Authentication service is temporarily unavailable.",
            &request_id,
            json!({}),
        ),
    }
}

/// Verifies a passkey registration and establishes a Secure, HttpOnly browser session.
pub(super) async fn registration_verify(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<RegistrationVerifyDto>,
) -> Response {
    let request_id = request_id(&headers);
    if !is_trusted_browser_request(&headers)
        || !trusted_proxy_header_matches(&headers, state.trusted_proxy_token.as_deref())
    {
        return error_response(
            StatusCode::FORBIDDEN,
            "untrusted_request",
            "The request must originate from the trusted first-party proxy.",
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
    let Some(encoded) = store
        .load_webauthn_challenge_state(payload.challenge_id)
        .await
        .ok()
        .flatten()
    else {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "challenge_rejected",
            "The passkey challenge is expired or already used.",
            &request_id,
            json!({}),
        );
    };
    let state_blob: RegistrationStateContext = match webauthn::decode_state(&encoded) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "challenge_rejected",
                "The passkey challenge is invalid.",
                &request_id,
                json!({}),
            );
        }
    };
    let credential = match webauthn::finish_registration(&state_blob.state, &payload.response) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "webauthn_rejected",
                "The passkey response was rejected.",
                &request_id,
                json!({}),
            );
        }
    };
    let session_token = Uuid::new_v4().simple().to_string();
    let csrf_token = Uuid::new_v4().simple().to_string();
    let session_token_hash = token_hash(&session_token);
    let csrf_hash = token_hash(&csrf_token);
    let challenge_hash = token_hash(&state_blob.state.challenge.to_b64url());
    let browser = NewBrowserSession {
        session_id: Uuid::new_v4(),
        user_id: state_blob.user_id,
        session_token_hash: &session_token_hash,
        csrf_hash: &csrf_hash,
        passkey_reauthenticated_at_rfc3339: "unused: database clock",
        expires_at_rfc3339: "unused: database clock",
        absolute_expires_at_rfc3339: "unused: database clock",
    };
    let registration = NewPasskeyRegistration {
        user_id: state_blob.user_id,
        account_display_name: &state_blob.account_display_name,
        challenge_id: payload.challenge_id,
        challenge_hash: &challenge_hash,
        credential_id: Uuid::new_v4(),
        credential_bytes: credential.id.as_bytes(),
        public_key: credential.public_key_cose.as_bytes(),
        sign_count: i64::from(credential.counter),
        transports: &credential.transports,
        browser_session: browser,
    };
    let registration_outcome = match store.complete_passkey_registration(registration).await {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "auth_unavailable",
                "Authentication service is temporarily unavailable.",
                &request_id,
                json!({}),
            );
        }
    };
    match registration_outcome {
        PasskeyRegistrationOutcome::ChallengeRejected => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "challenge_rejected",
                "The passkey challenge is expired or already used.",
                &request_id,
                json!({}),
            );
        }
        PasskeyRegistrationOutcome::CredentialAlreadyRegistered => {
            return error_response(
                StatusCode::CONFLICT,
                "credential_rejected",
                "The passkey credential is already registered.",
                &request_id,
                json!({}),
            );
        }
        PasskeyRegistrationOutcome::Created => {}
    }
    let mut response = with_request_id(
        Json(json!({"account_display_name": state_blob.account_display_name, "csrf_token": csrf_token}))
            .into_response(),
        &request_id,
    );
    response.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_str(&format!("rockserver_browser={session_token}; Path=/; Max-Age={}; HttpOnly; Secure; SameSite=Lax", BROWSER_SESSION_ABSOLUTE_MINUTES * 60)).expect("generated cookie is valid"));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Starts a discoverable authentication ceremony without requiring an account identifier.
pub(super) async fn authentication_options(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id(&headers);
    if !is_trusted_browser_request(&headers)
        || !trusted_proxy_header_matches(&headers, state.trusted_proxy_token.as_deref())
    {
        return error_response(
            StatusCode::FORBIDDEN,
            "untrusted_request",
            "The request must originate from the trusted first-party proxy.",
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
    let (options, state_blob) = webauthn::start_authentication();
    let encoded = match webauthn::encode_state(&state_blob) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "auth_unavailable",
                "Authentication service is temporarily unavailable.",
                &request_id,
                json!({}),
            );
        }
    };
    let challenge_id = Uuid::new_v4();
    let challenge_hash = token_hash(&state_blob.state.challenge.to_b64url());
    let challenge = NewWebAuthnChallenge {
        challenge_id,
        challenge_hash: &challenge_hash,
        state_blob: &encoded,
        ceremony: WebAuthnCeremony::Authentication,
        rp_id: webauthn::RP_ID,
        origin: webauthn::ORIGIN,
        expires_at_rfc3339: "unused: database clock",
        user_id: None,
        browser_session_id: None,
        pairing_request_id: None,
    };
    match store
        .create_webauthn_challenge_for_minutes(challenge, 5)
        .await
    {
        Ok(true) => with_request_id(
            Json(AuthenticationOptionsResponseDto {
                challenge_id,
                options,
            })
            .into_response(),
            &request_id,
        ),
        Ok(false) | Err(_) => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "auth_unavailable",
            "Authentication service is temporarily unavailable.",
            &request_id,
            json!({}),
        ),
    }
}

/// Verifies a passkey assertion, applies the rollback guard and issues a browser session cookie.
pub(super) async fn authentication_verify(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<AuthenticationVerifyDto>,
) -> Response {
    let request_id = request_id(&headers);
    if !is_trusted_browser_request(&headers)
        || !trusted_proxy_header_matches(&headers, state.trusted_proxy_token.as_deref())
    {
        return error_response(
            StatusCode::FORBIDDEN,
            "untrusted_request",
            "The request must originate from the trusted first-party proxy.",
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
    let Some(encoded) = store
        .load_webauthn_challenge_state(payload.challenge_id)
        .await
        .ok()
        .flatten()
    else {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "challenge_rejected",
            "The passkey challenge is expired or already used.",
            &request_id,
            json!({}),
        );
    };
    let state_blob: webauthn::AuthenticationStateContext = match webauthn::decode_state(&encoded) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "challenge_rejected",
                "The passkey challenge is invalid.",
                &request_id,
                json!({}),
            );
        }
    };
    let user_id = match webauthn::user_id_from_handle(payload.response.user_handle.as_deref()) {
        Ok(user_id) => user_id,
        Err(_) => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "webauthn_rejected",
                "The passkey response was rejected.",
                &request_id,
                json!({}),
            );
        }
    };
    if state_blob
        .user_id
        .is_some_and(|expected| expected != user_id)
    {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "webauthn_rejected",
            "The passkey response was rejected.",
            &request_id,
            json!({}),
        );
    }
    let credential_id = match CredentialId::from_b64url(&payload.response.id) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "webauthn_rejected",
                "The passkey response was rejected.",
                &request_id,
                json!({}),
            );
        }
    };
    let Some(credential) = store
        .find_passkey_credential_for_user(user_id, credential_id.as_bytes())
        .await
        .ok()
        .flatten()
    else {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "webauthn_rejected",
            "The passkey response was rejected.",
            &request_id,
            json!({}),
        );
    };
    let outcome = match webauthn::finish_authentication(&state_blob, &payload.response, &credential)
    {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "webauthn_rejected",
                "The passkey response was rejected.",
                &request_id,
                json!({}),
            );
        }
    };
    if !store
        .consume_webauthn_challenge(
            payload.challenge_id,
            &token_hash(&state_blob.state.challenge.to_b64url()),
            WebAuthnCeremony::Authentication,
            webauthn::ORIGIN,
            webauthn::RP_ID,
        )
        .await
        .unwrap_or(false)
        || !store
            .advance_passkey_sign_count(credential_id.as_bytes(), i64::from(outcome.new_counter))
            .await
            .unwrap_or(false)
    {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "challenge_rejected",
            "The passkey challenge is expired, replayed or rolled back.",
            &request_id,
            json!({}),
        );
    }
    let session_token = Uuid::new_v4().simple().to_string();
    let csrf_token = Uuid::new_v4().simple().to_string();
    let session = NewBrowserSession {
        session_id: Uuid::new_v4(),
        user_id,
        session_token_hash: &token_hash(&session_token),
        csrf_hash: &token_hash(&csrf_token),
        passkey_reauthenticated_at_rfc3339: "unused: database clock",
        expires_at_rfc3339: "unused: database clock",
        absolute_expires_at_rfc3339: "unused: database clock",
    };
    if !store
        .create_browser_session_with_policy(session)
        .await
        .unwrap_or(false)
    {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "auth_unavailable",
            "Authentication service is temporarily unavailable.",
            &request_id,
            json!({}),
        );
    }
    let mut response = with_request_id(
        Json(json!({"csrf_token": csrf_token})).into_response(),
        &request_id,
    );
    response.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_str(&format!("rockserver_browser={session_token}; Path=/; Max-Age={}; HttpOnly; Secure; SameSite=Lax", BROWSER_SESSION_ABSOLUTE_MINUTES * 60)).expect("generated cookie is valid"));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Refreshes the tab-local CSRF proof for an active browser account session.
pub(super) async fn browser_session(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let request_id = request_id(&headers);
    if !is_trusted_browser_request(&headers)
        || !trusted_proxy_header_matches(&headers, state.trusted_proxy_token.as_deref())
    {
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
    let Some(store) = state.account_store.as_ref() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "auth_unavailable",
            "Account service is unavailable.",
            &request_id,
            json!({}),
        );
    };
    let csrf_token = Uuid::new_v4().simple().to_string();
    match store
        .rotate_browser_csrf(&token_hash(cookie), &token_hash(&csrf_token))
        .await
    {
        Ok(Some(account_display_name)) => {
            let mut response = with_request_id(
                Json(BrowserSessionDto {
                    account_display_name,
                    csrf_token,
                })
                .into_response(),
                &request_id,
            );
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            response
        }
        Ok(None) => error_response(
            StatusCode::UNAUTHORIZED,
            "authentication_required",
            "A browser session is required.",
            &request_id,
            json!({}),
        ),
        Err(_) => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "auth_unavailable",
            "Account service is unavailable.",
            &request_id,
            json!({}),
        ),
    }
}

/// Issues a short-lived access token for an already paired native device.
pub(super) async fn create_device_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let request_id = request_id(&headers);
    let payload: DeviceSessionRequestDto =
        match parse_json_request(&headers, body, &request_id).await {
            Ok(payload) => payload,
            Err(response) => return *response,
        };
    if payload.device_secret.len() < 32 || payload.device_secret.len() > 512 {
        return unauthorized_response(&request_id);
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
    let result = store
        .issue_device_session(
            payload.device_id,
            &token_hash(&payload.device_secret),
            Uuid::new_v4(),
            &token_hash(&access_token),
        )
        .await;
    match result {
        Ok(Some(access_expires_at)) => {
            let mut response = with_request_id(
                Json(DeviceSessionResponseDto {
                    access_token,
                    access_expires_at,
                })
                .into_response(),
                &request_id,
            );
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            response
        }
        Ok(None) => error_response(
            StatusCode::UNAUTHORIZED,
            "device_credential_invalid",
            "This device credential is no longer valid.",
            &request_id,
            json!({}),
        ),
        Err(_) => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "auth_unavailable",
            "Account service is unavailable.",
            &request_id,
            json!({}),
        ),
    }
}
