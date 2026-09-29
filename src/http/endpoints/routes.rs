//! Route registration and middleware pipeline for the HTTP server.

use axum::Router;
use tower_http::trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::Level;

use super::{
    account, admin_auth, admin_console, auth, catalog, control, device_catalog, directory, health,
    pairing, search, state::AppState, station_icons, sync, voice, yandex_home,
};

/// Assembles the complete Axum router tree with public and administrative routes,
/// telemetry middleware, and shared application state.
pub(super) fn build_router(state: AppState) -> Router {
    let admin_routes = Router::new()
        .route(
            "/api/v1/admin/auth/login",
            axum::routing::post(admin_auth::login),
        )
        .route(
            "/api/v1/admin/auth/refresh",
            axum::routing::post(admin_auth::refresh),
        )
        .route(
            "/api/v1/admin/auth/logout",
            axum::routing::post(admin_auth::logout),
        )
        .route(
            "/api/v1/admin/session",
            axum::routing::get(admin_auth::session),
        )
        .route(
            "/api/v1/admin/stations",
            axum::routing::get(admin_console::stations),
        )
        .route(
            "/api/v1/admin/icons/import",
            axum::routing::get(station_icons::latest_import).post(station_icons::start_import),
        )
        .route(
            "/api/v1/admin/icons/import/{job_id}",
            axum::routing::get(station_icons::import_progress),
        )
        .route(
            "/api/v1/admin/stations/{station_id}/icon",
            axum::routing::put(station_icons::replace_manual).delete(station_icons::remove_manual),
        )
        .route(
            "/api/v1/admin/devices",
            axum::routing::get(admin_console::devices),
        )
        .route(
            "/api/v1/admin/audit",
            axum::routing::get(admin_console::audit),
        )
        .route_layer(axum::middleware::from_fn(admin_console::security_headers));

    Router::new()
        .merge(admin_routes)
        .route("/health/live", axum::routing::get(health::live))
        .route("/health/ready", axum::routing::get(health::ready))
        .route(
            "/api/v1/catalog/stations",
            axum::routing::get(catalog::public_catalog_list),
        )
        .route(
            "/api/v1/catalog/stations/{station_id}",
            axum::routing::get(catalog::public_catalog_get),
        )
        .route(
            "/api/v1/stations/{station_id}/icon",
            axum::routing::get(station_icons::public_icon),
        )
        .route("/api/v1/search", axum::routing::post(search::public_search))
        .route(
            "/api/v1/voice/command",
            axum::routing::post(voice::voice_command),
        )
        .route(
            "/api/v1/voice/stream",
            axum::routing::get(voice::voice_stream),
        )
        .route(
            "/api/v1/devices/connect",
            axum::routing::get(control::connect),
        )
        .route(
            "/api/v1/device-control/directory",
            axum::routing::get(directory::get),
        )
        .route("/api/v1/sync", axum::routing::post(sync::sync_request))
        .route(
            "/api/v1/device-control/catalog/stations",
            axum::routing::get(device_catalog::browse),
        )
        .route(
            "/api/v1/device-control/catalog/search",
            axum::routing::get(device_catalog::search),
        )
        .route(
            "/api/v1/pairing-requests",
            axum::routing::post(pairing::create_pairing_request),
        )
        .route(
            "/api/v1/pairing-requests/lookup",
            axum::routing::get(pairing::lookup_pairing_request),
        )
        .route(
            "/api/v1/auth/browser-session",
            axum::routing::post(auth::browser_session),
        )
        .route(
            "/api/v1/browser/account",
            axum::routing::get(account::browser_account),
        )
        .route(
            "/api/v1/browser/yandex-home/authorize",
            axum::routing::post(yandex_home::begin_authorization),
        )
        .route(
            "/api/v1/browser/yandex-home/callback",
            axum::routing::get(yandex_home::authorization_callback),
        )
        .route(
            "/api/v1/browser/yandex-home/sensors",
            axum::routing::get(yandex_home::sensors),
        )
        .route(
            "/api/v1/browser/yandex-home",
            axum::routing::delete(yandex_home::disconnect),
        )
        .route(
            "/api/v1/auth/browser-logout",
            axum::routing::post(account::logout_browser_session),
        )
        .route(
            "/api/v1/pairing-requests/{request_id}/approve",
            axum::routing::post(pairing::approve_pairing_request),
        )
        .route(
            "/api/v1/auth/passkeys/registration/options",
            axum::routing::post(auth::registration_options),
        )
        .route(
            "/api/v1/auth/passkeys/registration/verify",
            axum::routing::post(auth::registration_verify),
        )
        .route(
            "/api/v1/auth/passkeys/authentication/options",
            axum::routing::post(auth::authentication_options),
        )
        .route(
            "/api/v1/auth/passkeys/authentication/verify",
            axum::routing::post(auth::authentication_verify),
        )
        .route(
            "/api/v1/pairing-requests/{request_id}/complete",
            axum::routing::post(pairing::complete_pairing_request),
        )
        .route(
            "/api/v1/auth/device-session",
            axum::routing::post(auth::create_device_session),
        )
        .route(
            "/api/v1/account/profile",
            axum::routing::get(account::account_profile),
        )
        .route(
            "/api/v1/account",
            axum::routing::delete(account::delete_account),
        )
        .route("/api/v1/devices", axum::routing::get(account::list_devices))
        .route(
            "/api/v1/devices/{device_id}",
            axum::routing::delete(account::revoke_device),
        )
        .route(
            "/api/v1/browser/devices/{device_id}",
            axum::routing::patch(account::rename_browser_device)
                .delete(account::revoke_browser_device),
        )
        .with_state(state)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
}
