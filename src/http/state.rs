//! Shared HTTP state and process-local admission controls.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    http::{HeaderMap, StatusCode, header},
    response::Response,
};
use serde_json::json;

use crate::{
    admin::AdminStore,
    auth::NativeSessionResolver,
    device_control::DeviceControlStore,
    device_control_command::CommandRouter,
    device_control_presence::ConnectionRegistry,
    device_control_state::StateHub,
    persistence::PostgresAccountStore,
    personal_data::PersonalDataStore,
    providers::yandex_home::YandexHomeClient,
    search::SearchService,
    station_icons::IconImportCoordinator,
    voice::{CommandInterpreter, SpeechRecognizers},
};

use super::{
    transport::{constant_time_eq, error_response, retry_after},
    voice::VoiceSlot,
};

const GLOBAL_VOICE_SESSIONS: usize = 100;
const RATE_WINDOW: Duration = Duration::from_secs(60);
const RETRY_AFTER_SECONDS: u64 = 60;
const VOICE_CAPACITY_RETRY_AFTER_SECONDS: u64 = 30;

#[derive(Clone, Copy)]
/// Server-owned timing policy for a device-control socket.
pub(crate) struct ControlTiming {
    pub(crate) registration_deadline: Duration,
    pub(crate) offline_ttl: Duration,
}

impl Default for ControlTiming {
    fn default() -> Self {
        Self {
            registration_deadline: Duration::from_secs(10),
            offline_ttl: Duration::from_secs(60),
        }
    }
}

#[derive(Clone, Copy)]
/// Per-endpoint anonymous request and burst limits.
pub(crate) struct PublicLimit {
    pub(crate) requests: usize,
    pub(crate) burst: usize,
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) search_service: SearchService,
    pub(crate) speech_recognizers: SpeechRecognizers,
    pub(crate) voice_command_interpreter: Arc<dyn CommandInterpreter>,
    pub(crate) voice_command_timeout: Duration,
    pub(crate) api_bearer_token: String,
    pub(crate) account_store: Option<PostgresAccountStore>,
    pub(crate) admin_store: Option<Arc<dyn AdminStore>>,
    pub(crate) trusted_proxy_token: Option<String>,
    pub(crate) local_admin_origin: Option<String>,
    pub(crate) public_limits: Arc<Mutex<PublicLimitState>>,
    pub(crate) control_registry: ConnectionRegistry,
    /// Shared bounded command lifecycle router for live control sessions.
    pub(crate) control_commands: CommandRouter,
    /// Ephemeral owner-scoped latest state and bounded internal fan-out.
    pub(crate) control_state_hub: StateHub,
    /// Durable account-owned state projections, configured only for production routers.
    pub(crate) control_store: Option<Arc<dyn DeviceControlStore>>,
    /// Resolver used only by the native device-control ingress before WebSocket upgrade.
    pub(crate) control_session_resolver: Option<Arc<dyn NativeSessionResolver>>,
    /// Durable account-owned personal-data store backing `POST /api/v1/sync`.
    pub(crate) personal_store: Option<Arc<dyn PersonalDataStore>>,
    /// Durable administrator-started station-icon importer when its storage root is configured.
    pub(crate) icon_import: Option<Arc<IconImportCoordinator>>,
    /// Optional read-only Yandex Smart Home client configured for browser accounts.
    pub(crate) yandex_home: Option<Arc<YandexHomeClient>>,
    pub(crate) control_timing: ControlTiming,
    pub(crate) relay: super::relay::RelayState,
}

#[derive(Default)]
/// Process-local buckets used by anonymous and per-device HTTP admission control.
pub(crate) struct PublicLimitState {
    pub(crate) requests: HashMap<&'static str, Vec<std::time::Instant>>,
    /// Per-device buckets for authenticated device-facing endpoints, keyed `endpoint:device_id`.
    pub(crate) device_requests: HashMap<String, Vec<std::time::Instant>>,
    pub(crate) active_voice: usize,
}

impl AppState {
    /// Applies the fail-closed direct-peer anonymous quota.
    pub(super) fn public_request_allowed(
        &self,
        endpoint: &'static str,
        limit: PublicLimit,
        request_id: &str,
    ) -> Result<(), Box<Response>> {
        let mut state = self
            .public_limits
            .lock()
            .expect("public limiter mutex is not poisoned");
        let now = std::time::Instant::now();
        let bucket = state.requests.entry(endpoint).or_default();
        bucket.retain(|seen| now.duration_since(*seen) < RATE_WINDOW);
        if bucket.len() >= limit.burst {
            tracing::warn!(%request_id, endpoint, limit_scope = "direct_peer", requests_per_minute = limit.requests, "public rate limit rejected");
            return Err(Box::new(retry_after(
                error_response(
                    StatusCode::TOO_MANY_REQUESTS,
                    "rate_limited",
                    "Request rate limit exceeded.",
                    request_id,
                    json!({"limit_scope":"direct_peer"}),
                ),
                RETRY_AFTER_SECONDS,
            )));
        }
        bucket.push(now);
        Ok(())
    }

    /// Applies the per-device quota for authenticated device-facing endpoints.
    ///
    /// Buckets are scoped to one authenticated device so a single chatty device cannot exhaust
    /// the quota of the rest of the fleet; requests rejected before authentication never enter
    /// a device bucket. Window and retry policy match the anonymous limiter.
    pub(super) fn device_request_allowed(
        &self,
        endpoint: &'static str,
        device_id: uuid::Uuid,
        limit: PublicLimit,
        request_id: &str,
    ) -> Result<(), Box<Response>> {
        let mut state = self
            .public_limits
            .lock()
            .expect("public limiter mutex is not poisoned");
        let now = std::time::Instant::now();
        let bucket = state
            .device_requests
            .entry(format!("{endpoint}:{device_id}"))
            .or_default();
        bucket.retain(|seen| now.duration_since(*seen) < RATE_WINDOW);
        if bucket.len() >= limit.burst {
            tracing::warn!(%request_id, endpoint, limit_scope = "device", requests_per_minute = limit.requests, "device rate limit rejected");
            return Err(Box::new(retry_after(
                error_response(
                    StatusCode::TOO_MANY_REQUESTS,
                    "rate_limited",
                    "Request rate limit exceeded.",
                    request_id,
                    json!({"limit_scope":"device"}),
                ),
                RETRY_AFTER_SECONDS,
            )));
        }
        bucket.push(now);
        Ok(())
    }

    /// Reserves a global public voice slot before a WebSocket upgrade.
    pub(super) fn reserve_voice_slot(&self, request_id: &str) -> Result<VoiceSlot, Box<Response>> {
        let mut state = self
            .public_limits
            .lock()
            .expect("public limiter mutex is not poisoned");
        if state.active_voice >= GLOBAL_VOICE_SESSIONS {
            return Err(Box::new(retry_after(
                error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "voice_capacity_exhausted",
                    "Voice service is temporarily at capacity.",
                    request_id,
                    json!({}),
                ),
                VOICE_CAPACITY_RETRY_AFTER_SECONDS,
            )));
        }
        state.active_voice += 1;
        Ok(VoiceSlot {
            limiter: Arc::clone(&self.public_limits),
        })
    }

    /// Verifies the configured opaque application credential without logging the supplied value.
    pub(super) fn is_authorized(&self, headers: &HeaderMap) -> bool {
        let Some(value) = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
        else {
            return false;
        };
        let Some(token) = value.strip_prefix("Bearer ") else {
            return false;
        };
        constant_time_eq(token.as_bytes(), self.api_bearer_token.as_bytes())
    }
}
