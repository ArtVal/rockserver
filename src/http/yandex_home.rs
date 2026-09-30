//! First-party browser endpoints for read-only Yandex Smart Home temperatures.

use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{persistence::EncryptedYandexHomeToken, providers::yandex_home::YandexHomeError};

use super::{
    account::browser_mutation_owner,
    state::AppState,
    transport::{
        cookie_value, error_response, request_id, token_hash, trusted_proxy_header_matches,
        with_request_id,
    },
};

#[derive(Serialize)]
struct AuthorizationUrlDto {
    authorization_url: String,
}

#[derive(Serialize)]
struct SensorsDto {
    devices: Vec<DeviceDto>,
    sensors: Vec<SensorDto>,
}

#[derive(Serialize)]
struct DeviceDto {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    room_name: Option<String>,
    properties: Vec<PropertyDto>,
}

#[derive(Serialize)]
struct PropertyDto {
    property_type: String,
    instance: String,
    name: String,
    value: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    unit: Option<String>,
    formatted_value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    updated_at: Option<String>,
}

#[derive(Serialize)]
struct SensorDto {
    device_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    room_name: Option<String>,
    property: String,
    name: String,
    value: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    unit: Option<String>,
    formatted_value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    updated_at: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
}

/// Starts a CSRF-protected Yandex OAuth authorization for the signed-in browser account.
pub(super) async fn begin_authorization(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id(&headers);
    let user_id = match browser_mutation_owner(&state, &headers, &request_id).await {
        Ok(user_id) => user_id,
        Err(response) => return *response,
    };
    let Some(client) = state.yandex_home.as_ref() else {
        return yandex_unavailable(&request_id);
    };
    let Some(store) = state.account_store.as_ref() else {
        return auth_unavailable(&request_id);
    };
    let oauth_state = Uuid::new_v4().simple().to_string();
    match store
        .create_yandex_home_oauth_state(user_id, &token_hash(&oauth_state))
        .await
    {
        Ok(true) => match client.authorization_url(&oauth_state) {
            Ok(authorization_url) => no_store(with_request_id(
                Json(AuthorizationUrlDto { authorization_url }).into_response(),
                &request_id,
            )),
            Err(_) => yandex_unavailable(&request_id),
        },
        Ok(false) => error_response(
            StatusCode::UNAUTHORIZED,
            "authentication_required",
            "A browser session is required.",
            &request_id,
            json!({}),
        ),
        Err(_) => auth_unavailable(&request_id),
    }
}

/// Completes a one-time OAuth state and returns the browser to its account cabinet.
pub(super) async fn authorization_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<CallbackQuery>,
) -> Response {
    if !trusted_proxy_header_matches(&headers, state.trusted_proxy_token.as_deref()) {
        tracing::warn!("Yandex OAuth callback rejected: untrusted proxy header");
        return Redirect::to("/?yandex_home=failed").into_response();
    }
    let (Some(client), Some(store), Some(oauth_state), Some(code)) = (
        state.yandex_home.as_ref(),
        state.account_store.as_ref(),
        query
            .state
            .filter(|value| !value.is_empty() && value.len() <= 128),
        query
            .code
            .filter(|value| !value.is_empty() && value.len() <= 2048),
    ) else {
        tracing::warn!("Yandex OAuth callback rejected: missing client, store, state, or code");
        return Redirect::to("/?yandex_home=failed").into_response();
    };
    let owner = match store
        .consume_yandex_home_oauth_state(&token_hash(&oauth_state))
        .await
    {
        Ok(Some(user_id)) => user_id,
        Ok(None) => {
            tracing::warn!("Yandex OAuth callback rejected: OAuth state expired or not found");
            return Redirect::to("/?yandex_home=failed").into_response();
        }
        Err(err) => {
            tracing::error!(error = %err, "Yandex OAuth callback failed to consume OAuth state");
            return Redirect::to("/?yandex_home=failed").into_response();
        }
    };
    if let Some(cookie) = cookie_value(&headers, "rockserver_browser")
        && let Ok(Some(browser_user)) = store.browser_session_user(&token_hash(cookie)).await
        && browser_user != owner
    {
        tracing::warn!(%owner, %browser_user, "Yandex OAuth callback rejected: session cookie belongs to a different user");
        return Redirect::to("/?yandex_home=failed").into_response();
    }
    let (ciphertext, nonce) = match client.exchange_code(&code).await {
        Ok(token) => token,
        Err(err) => {
            tracing::error!(error = %err, "Yandex OAuth code exchange failed");
            return Redirect::to("/?yandex_home=failed").into_response();
        }
    };
    match store
        .save_yandex_home_connection(
            owner,
            &EncryptedYandexHomeToken {
                ciphertext,
                nonce: nonce.to_vec(),
            },
        )
        .await
    {
        Ok(true) => {
            tracing::info!(%owner, "Yandex Smart Home connected successfully");
            Redirect::to("/?yandex_home=connected").into_response()
        }
        Ok(false) => {
            tracing::warn!(%owner, "Yandex Smart Home connection could not be saved");
            Redirect::to("/?yandex_home=failed").into_response()
        }
        Err(err) => {
            tracing::error!(error = %err, %owner, "Failed to persist Yandex Smart Home connection");
            Redirect::to("/?yandex_home=failed").into_response()
        }
    }
}

/// Returns current readable temperatures for the signed-in account's linked Yandex home.
pub(super) async fn sensors(State(state): State<AppState>, headers: HeaderMap) -> Response {
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
        return auth_unavailable(&request_id);
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
        Err(_) => return auth_unavailable(&request_id),
    };
    let Some(client) = state.yandex_home.as_ref() else {
        return yandex_unavailable(&request_id);
    };
    let connection = match store.yandex_home_connection(user_id).await {
        Ok(Some(connection)) => connection,
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "yandex_home_not_connected",
                "Yandex Smart Home is not connected.",
                &request_id,
                json!({}),
            );
        }
        Err(_) => return auth_unavailable(&request_id),
    };
    match client
        .home_devices(&connection.ciphertext, &connection.nonce)
        .await
    {
        Ok(devices) => {
            let mut flat_sensors = Vec::new();
            let mut devices_dto = Vec::new();
            for device in devices {
                let mut prop_dtos = Vec::new();
                for prop in device.properties {
                    let temp = if prop.instance == "temperature" {
                        prop.value.as_f64()
                    } else {
                        None
                    };
                    flat_sensors.push(SensorDto {
                        device_name: device.name.clone(),
                        room_name: device.room_name.clone(),
                        property: prop.instance.clone(),
                        name: prop.name.clone(),
                        value: prop.value.clone(),
                        unit: prop.unit.clone(),
                        formatted_value: prop.formatted_value.clone(),
                        temperature: temp,
                        updated_at: prop.updated_at.clone(),
                    });
                    prop_dtos.push(PropertyDto {
                        property_type: prop.property_type,
                        instance: prop.instance,
                        name: prop.name,
                        value: prop.value,
                        unit: prop.unit,
                        formatted_value: prop.formatted_value,
                        updated_at: prop.updated_at,
                    });
                }
                devices_dto.push(DeviceDto {
                    id: device.id,
                    name: device.name,
                    device_type: device.device_type,
                    room_name: device.room_name,
                    properties: prop_dtos,
                });
            }
            no_store(with_request_id(
                Json(SensorsDto {
                    devices: devices_dto,
                    sensors: flat_sensors,
                })
                .into_response(),
                &request_id,
            ))
        }
        Err(YandexHomeError::AuthorizationRejected | YandexHomeError::StoredTokenInvalid) => {
            let _ = store.revoke_yandex_home_connection(user_id).await;
            error_response(
                StatusCode::CONFLICT,
                "yandex_home_reconnect_required",
                "Reconnect Yandex Smart Home to refresh access.",
                &request_id,
                json!({}),
            )
        }
        Err(_) => error_response(
            StatusCode::BAD_GATEWAY,
            "yandex_home_unavailable",
            "Yandex Smart Home is temporarily unavailable.",
            &request_id,
            json!({}),
        ),
    }
}

/// Revokes the signed-in account's stored Yandex authorization.
pub(super) async fn disconnect(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let request_id = request_id(&headers);
    let user_id = match browser_mutation_owner(&state, &headers, &request_id).await {
        Ok(user_id) => user_id,
        Err(response) => return *response,
    };
    let Some(store) = state.account_store.as_ref() else {
        return auth_unavailable(&request_id);
    };
    match store.revoke_yandex_home_connection(user_id).await {
        Ok(_) => with_request_id(StatusCode::NO_CONTENT.into_response(), &request_id),
        Err(_) => auth_unavailable(&request_id),
    }
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn auth_unavailable(request_id: &str) -> Response {
    error_response(
        StatusCode::SERVICE_UNAVAILABLE,
        "auth_unavailable",
        "Account service is unavailable.",
        request_id,
        json!({}),
    )
}

fn yandex_unavailable(request_id: &str) -> Response {
    error_response(
        StatusCode::SERVICE_UNAVAILABLE,
        "yandex_home_unavailable",
        "Yandex Smart Home is unavailable.",
        request_id,
        json!({}),
    )
}
