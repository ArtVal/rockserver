//! RockServer service library.
//!
//! See `src/ARCHITECTURE.md` for a compact source map, dependency rules, and test locations.

/// Preview-first cleanup boundary for one exact staging account, device, or passkey row.
pub mod account_cleanup;
/// Administrator identity and persistence contracts separate from Rock accounts.
pub mod admin;
/// Protected one-shot administrator bootstrap command boundary.
pub mod admin_bootstrap;
/// Passkey-only account, device, and native-session domain boundaries.
pub mod auth;
/// Catalog domain and controlled import orchestration.
pub mod catalog;
/// Service configuration loaded from the process environment.
pub mod config;
/// Transport-independent device-control protocol v1 domain types and validation.
pub mod device_control;
/// Authentication foundation for future device-control ingress.
pub mod device_control_auth;
/// Bounded authorization, idempotency and lifecycle routing for live device commands.
pub mod device_control_command;
/// Deterministic typed intent resolution and presentation planning for device control.
pub mod device_control_intent;
/// Process-local live connection registry and owner-scoped presence transitions.
pub mod device_control_presence;
/// Bounded owner-scoped latest-state cache and internal event fan-out.
pub mod device_control_state;
/// HTTP routes and transport types.
pub mod http;
/// Deterministic prebuilt SQLite export for RockMobile's extended offline catalog.
pub mod mobile_export;
/// Persistent station-catalog backends and startup selection.
pub mod persistence;
/// Account-owned favourites and playback-history sync domain (RM-012-A).
pub mod personal_data;
/// External catalog providers used only by explicit background workflows.
pub mod providers;
/// Deterministic station-search domain and catalog boundary.
pub mod search;
/// Persistent, atomic storage for normalized station-icon artifacts.
pub mod station_icons;
/// Structured tracing setup.
pub mod telemetry;
/// Provider-neutral voice-command and streaming speech boundaries.
pub mod voice;
/// Backward-compatible facade for the pre-package speech API.
pub mod speech {
    pub use crate::voice::speech::*;
}
/// Backward-compatible facade for the pre-package voice-command API.
pub mod voice_command {
    pub use crate::voice::command::*;
}

use std::{future::Future, io};

use axum::Router;
use tokio::net::TcpListener;

/// Serves the application until the provided shutdown future completes.
pub async fn serve(
    listener: TcpListener,
    app: Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> io::Result<()> {
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown.await;
            crate::device_control_presence::signal_control_shutdown();
        })
        .await
}

/// Waits for the process Ctrl+C signal used by the current binary entry point.
pub async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to install Ctrl+C handler");
    }
}
