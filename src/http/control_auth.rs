//! HTTP extractor shared by device-control ingress handlers.

use axum::http::{HeaderMap, header};
use axum::response::Response;

use crate::{
    auth::NativeSessionResolver,
    device_control_auth::{
        DeviceControlAuthenticationError, DeviceControlPrincipal, authenticate_device_control,
    },
};

/// Authenticates a control ingress without accepting client-claimed identity fields.
///
/// Transport handlers map `InvalidCredential` to a generic 401 and `Unavailable` to a retryable
/// 503 before a WebSocket upgrade. This does not implement that upgrade or connection lifecycle.
pub async fn authenticate_control_ingress(
    headers: &HeaderMap,
    resolver: &(impl NativeSessionResolver + ?Sized),
) -> Result<DeviceControlPrincipal, DeviceControlAuthenticationError> {
    authenticate_device_control(
        headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok()),
        resolver,
    )
    .await
}

/// Authenticates the native device session, then applies the endpoint's per-device quota.
///
/// Shared by every native-session REST endpoint so the failure mapping stays identical:
/// retryable 503 when the resolver is not configured or temporarily unavailable, generic 401
/// for an absent, malformed, expired, revoked, or non-native credential, and 429 when the
/// device exhausted its quota. Returns the server-derived principal; the bearer value itself
/// is never logged.
pub(super) async fn authenticate_and_throttle(
    state: &super::state::AppState,
    headers: &HeaderMap,
    endpoint: &'static str,
    limit: super::state::PublicLimit,
    request_id: &str,
) -> Result<DeviceControlPrincipal, Box<Response>> {
    use super::transport::{error_response, retry_after, unauthorized_response};
    use axum::http::StatusCode;
    use serde_json::json;

    let Some(resolver) = state.control_session_resolver.as_ref() else {
        return Err(Box::new(retry_after(
            error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "control_auth_unavailable",
                "Device control authentication is temporarily unavailable.",
                request_id,
                json!({}),
            ),
            1,
        )));
    };
    let principal = match authenticate_control_ingress(headers, resolver.as_ref()).await {
        Ok(principal) => principal,
        Err(DeviceControlAuthenticationError::InvalidCredential) => {
            return Err(Box::new(unauthorized_response(request_id)));
        }
        Err(DeviceControlAuthenticationError::Unavailable) => {
            return Err(Box::new(retry_after(
                error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "control_auth_unavailable",
                    "Device control authentication is temporarily unavailable.",
                    request_id,
                    json!({}),
                ),
                1,
            )));
        }
    };
    state.device_request_allowed(endpoint, principal.device_id, limit, request_id)?;
    Ok(principal)
}
