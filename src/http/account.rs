//! Account and browser device-management HTTP handlers.

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::auth::{ActiveSession, MAX_ACCOUNT_DEVICES};

use super::{
    state::AppState,
    transport::{
        bearer_token, cookie_value, error_response, is_trusted_browser_request, request_id,
        token_hash, trusted_proxy_header_matches, unauthorized_response, with_request_id,
    },
};

#[derive(Serialize)]
struct BrowserAccountDto {
    account_display_name: String,
    device_limit: u8,
    devices: Vec<BrowserDeviceDto>,
    yandex_home_connected: bool,
}

#[derive(Serialize)]
struct BrowserDeviceDto {
    device_id: String,
    device_display_name: String,
    device_type: String,
    connected_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_seen_at: Option<String>,
    session_status: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
/// Browser payload for renaming an owned device.
pub(super) struct RenameBrowserDeviceDto {
    device_display_name: String,
}

#[derive(Serialize)]
struct AccountProfileDto {
    user_id: String,
    session_id: String,
    device_id: String,
    account_display_name: String,
    created_at: String,
    device_display_name: String,
    device_type: String,
    device_created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_seen_at: Option<String>,
}

#[derive(Serialize)]
struct DeviceDto {
    device_id: String,
    user_id: String,
    device_display_name: String,
    device_type: String,
    created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_seen_at: Option<String>,
}

#[derive(Serialize)]
struct DeviceListDto {
    devices: Vec<DeviceDto>,
}

/// Returns the authenticated browser account's safe device-management projection.
pub(super) async fn browser_account(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let request_id = request_id(&headers);
    if !trusted_proxy_header_matches(&headers, state.trusted_proxy_token.as_deref()) {
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
    let account_display_name = match store
        .browser_account_display_name(&token_hash(cookie))
        .await
    {
        Ok(Some(name)) => name,
        Ok(None) => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "authentication_required",
                "A browser session is required.",
                &request_id,
                json!({}),
            );
        }
        Err(_) => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "auth_unavailable",
                "Account service is unavailable.",
                &request_id,
                json!({}),
            );
        }
    };
    let user_id = match store.browser_session_user(&token_hash(cookie)).await {
        Ok(Some(user_id)) => user_id,
        Ok(None) => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "authentication_required",
                "A browser session is required.",
                &request_id,
                json!({}),
            );
        }
        Err(_) => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "auth_unavailable",
                "Account service is unavailable.",
                &request_id,
                json!({}),
            );
        }
    };
    match (
        store.list_browser_devices(user_id).await,
        store.has_yandex_home_connection(user_id).await,
    ) {
        (Ok(devices), Ok(yandex_home_connected)) => {
            let mut response = with_request_id(
                Json(BrowserAccountDto {
                    account_display_name,
                    device_limit: MAX_ACCOUNT_DEVICES,
                    devices: devices
                        .into_iter()
                        .map(|device| BrowserDeviceDto {
                            device_id: device.id.to_string(),
                            device_display_name: device.device_display_name,
                            device_type: device.device_type,
                            connected_at: device.created_at,
                            last_seen_at: device.last_seen_at,
                            session_status: device.session_status,
                        })
                        .collect(),
                    yandex_home_connected,
                })
                .into_response(),
                &request_id,
            );
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            response
        }
        (Err(_), _) | (_, Err(_)) => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "auth_unavailable",
            "Account service is unavailable.",
            &request_id,
            json!({}),
        ),
    }
}

/// Validates a browser state-changing request and derives its account owner from cookie and CSRF proofs.
pub(super) async fn browser_mutation_owner(
    state: &AppState,
    headers: &HeaderMap,
    request_id: &str,
) -> Result<Uuid, Box<Response>> {
    if !is_trusted_browser_request(headers)
        || !trusted_proxy_header_matches(headers, state.trusted_proxy_token.as_deref())
    {
        return Err(Box::new(error_response(
            StatusCode::FORBIDDEN,
            "untrusted_request",
            "The request must originate from the trusted first-party proxy.",
            request_id,
            json!({}),
        )));
    }
    let Some(cookie) = cookie_value(headers, "rockserver_browser") else {
        return Err(Box::new(error_response(
            StatusCode::UNAUTHORIZED,
            "authentication_required",
            "A browser session is required.",
            request_id,
            json!({}),
        )));
    };
    let Some(csrf) = headers
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && value.len() <= 256)
    else {
        return Err(Box::new(error_response(
            StatusCode::FORBIDDEN,
            "csrf_failed",
            "A valid CSRF token is required.",
            request_id,
            json!({}),
        )));
    };
    let Some(store) = state.account_store.as_ref() else {
        return Err(Box::new(error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "auth_unavailable",
            "Account service is unavailable.",
            request_id,
            json!({}),
        )));
    };
    match store
        .browser_session_user_with_csrf(&token_hash(cookie), &token_hash(csrf))
        .await
    {
        Ok(Some(user_id)) => Ok(user_id),
        Ok(None) => Err(Box::new(error_response(
            StatusCode::UNAUTHORIZED,
            "authentication_required",
            "A browser session is required.",
            request_id,
            json!({}),
        ))),
        Err(_) => Err(Box::new(error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "auth_unavailable",
            "Account service is unavailable.",
            request_id,
            json!({}),
        ))),
    }
}

/// Renames a caller-owned device through the first-party browser management boundary.
pub(super) async fn rename_browser_device(
    State(state): State<AppState>,
    Path(device_id): Path<Uuid>,
    headers: HeaderMap,
    Json(payload): Json<RenameBrowserDeviceDto>,
) -> Response {
    let request_id = request_id(&headers);
    let user_id = match browser_mutation_owner(&state, &headers, &request_id).await {
        Ok(user_id) => user_id,
        Err(response) => return *response,
    };
    let Some(name) = validated_device_display_name(&payload.device_display_name) else {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_device_name",
            "Device name must be 1 to 128 printable characters.",
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
    match store.rename_owned_device(user_id, device_id, name).await {
        Ok(true) => with_request_id(StatusCode::NO_CONTENT.into_response(), &request_id),
        Ok(false) => error_response(
            StatusCode::NOT_FOUND,
            "device_not_found",
            "Device is unavailable.",
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

/// Normalizes a user-visible device name while rejecting control characters that could affect logs or UI text.
fn validated_device_display_name(value: &str) -> Option<&str> {
    let name = value.trim();
    (!name.is_empty() && name.chars().count() <= 128 && !name.chars().any(char::is_control))
        .then_some(name)
}

/// Revokes another caller-owned native device from the browser account centre.
pub(super) async fn revoke_browser_device(
    State(state): State<AppState>,
    Path(device_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id(&headers);
    let user_id = match browser_mutation_owner(&state, &headers, &request_id).await {
        Ok(user_id) => user_id,
        Err(response) => return *response,
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
    match store.revoke_owned_device(user_id, device_id).await {
        Ok(true) => {
            state
                .control_registry
                .revoke(user_id, device_id, std::time::Instant::now());
            with_request_id(StatusCode::NO_CONTENT.into_response(), &request_id)
        }
        Ok(false) => error_response(
            StatusCode::NOT_FOUND,
            "device_not_found",
            "Device is unavailable.",
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

/// Logs out only the current browser cookie session; native device sessions remain unchanged.
pub(super) async fn logout_browser_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id(&headers);
    let user_id = match browser_mutation_owner(&state, &headers, &request_id).await {
        Ok(user_id) => user_id,
        Err(response) => return *response,
    };
    let Some(cookie) = cookie_value(&headers, "rockserver_browser") else {
        return unauthorized_response(&request_id);
    };
    let Some(csrf) = headers
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
    else {
        return unauthorized_response(&request_id);
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
    match store
        .logout_browser_session(user_id, &token_hash(cookie), &token_hash(csrf))
        .await
    {
        Ok(true) => with_request_id(StatusCode::NO_CONTENT.into_response(), &request_id),
        Ok(false) => unauthorized_response(&request_id),
        Err(_) => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "auth_unavailable",
            "Account service is unavailable.",
            &request_id,
            json!({}),
        ),
    }
}

/// Returns the caller's account and current device projection.
pub(super) async fn account_profile(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let request_id = request_id(&headers);
    let Some(session) = match_native_session(&state, &headers, &request_id).await else {
        return unauthorized_response(&request_id);
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
    let projection = match store
        .account_projection(session.user_id, session.device_id)
        .await
    {
        Ok(Some(projection)) => projection,
        Ok(None) => return unauthorized_response(&request_id),
        Err(_) => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "auth_unavailable",
                "Account service is unavailable.",
                &request_id,
                json!({}),
            );
        }
    };
    with_request_id(
        Json(AccountProfileDto {
            user_id: session.user_id.to_string(),
            session_id: session.session_id.to_string(),
            device_id: session.device_id.to_string(),
            account_display_name: projection.account_display_name,
            created_at: projection.created_at,
            device_display_name: projection.device_display_name,
            device_type: projection.device_type,
            device_created_at: projection.device_created_at,
            last_seen_at: projection.last_seen_at,
        })
        .into_response(),
        &request_id,
    )
}

/// Tombstones the account only after native authentication and fresh browser passkey proof.
pub(super) async fn delete_account(State(state): State<AppState>, headers: HeaderMap) -> Response {
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
    let Some(session) = match_native_session(&state, &headers, &request_id).await else {
        return unauthorized_response(&request_id);
    };
    let Some(cookie) = cookie_value(&headers, "rockserver_browser") else {
        return unauthorized_response(&request_id);
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
            &request_id,
            json!({}),
        );
    };
    match store
        .browser_session_is_fresh_for_user(session.user_id, &token_hash(cookie), &token_hash(csrf))
        .await
    {
        Ok(true) => {}
        Ok(false) => {
            return error_response(
                StatusCode::FORBIDDEN,
                "reauthentication_required",
                "A recent passkey assertion is required.",
                &request_id,
                json!({}),
            );
        }
        Err(_) => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "auth_unavailable",
                "Account service is unavailable.",
                &request_id,
                json!({}),
            );
        }
    }
    match store.delete_account(session.user_id).await {
        Ok(true) => with_request_id(StatusCode::ACCEPTED.into_response(), &request_id),
        Ok(false) => unauthorized_response(&request_id),
        Err(_) => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "auth_unavailable",
            "Account service is unavailable.",
            &request_id,
            json!({}),
        ),
    }
}

/// Lists active native devices owned by the authenticated account.
pub(super) async fn list_devices(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let request_id = request_id(&headers);
    let Some(store) = state.account_store.clone() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "auth_unavailable",
            "Account service is unavailable.",
            &request_id,
            json!({}),
        );
    };
    let Some(session) = match_native_session(&state, &headers, &request_id).await else {
        return unauthorized_response(&request_id);
    };
    match store.list_owned_devices(session.user_id).await {
        Ok(devices) => with_request_id(
            Json(DeviceListDto {
                devices: devices
                    .into_iter()
                    .map(|device| DeviceDto {
                        device_id: device.id.to_string(),
                        user_id: device.user_id.to_string(),
                        device_display_name: device.device_display_name,
                        device_type: device.device_type,
                        created_at: device.created_at,
                        last_seen_at: device.last_seen_at,
                    })
                    .collect(),
            })
            .into_response(),
            &request_id,
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

/// Revokes one device only when it belongs to the authenticated account.
pub(super) async fn revoke_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(device_id): Path<Uuid>,
) -> Response {
    let request_id = request_id(&headers);
    let Some(store) = state.account_store.clone() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "auth_unavailable",
            "Account service is unavailable.",
            &request_id,
            json!({}),
        );
    };
    let Some(session) = match_native_session(&state, &headers, &request_id).await else {
        return unauthorized_response(&request_id);
    };
    match store.revoke_owned_device(session.user_id, device_id).await {
        Ok(true) => {
            state
                .control_registry
                .revoke(session.user_id, device_id, std::time::Instant::now());
            with_request_id(StatusCode::NO_CONTENT.into_response(), &request_id)
        }
        Ok(false) => error_response(
            StatusCode::NOT_FOUND,
            "device_not_found",
            "Device is unavailable.",
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

/// Resolves the active native session from the Bearer credential.
pub(super) async fn match_native_session(
    state: &AppState,
    headers: &HeaderMap,
    request_id: &str,
) -> Option<ActiveSession> {
    let token = bearer_token(headers)?;
    let store = state.account_store.as_ref()?;
    match store
        .find_active_session_by_access_hash(&token_hash(token))
        .await
    {
        Ok(value) => value,
        Err(_) => {
            tracing::warn!(%request_id, "native session lookup failed");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::validated_device_display_name;

    #[test]
    fn device_name_validation_trims_but_rejects_empty_control_and_overlong_values() {
        assert_eq!(
            validated_device_display_name("  Living room PC  "),
            Some("Living room PC")
        );
        assert_eq!(validated_device_display_name("Rock\nCast"), None);
        assert_eq!(validated_device_display_name(&"a".repeat(129)), None);
    }
}
