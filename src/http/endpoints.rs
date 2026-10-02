//! HTTP route composition and entry points for the RockServer API.

use std::{env, sync::Arc, time::Duration};

use axum::Router;

use crate::{
    device_control_command::CommandRouter,
    persistence::{PostgresAccountStore, PostgresAdminStore},
    providers::yandex_home::YandexHomeClient,
    search::{SearchService, StationRepository},
    station_icons::{FilesystemIconStorage, IconImportCoordinator, IconStorage},
    voice::{CommandInterpreter, DeterministicCommandInterpreter, SpeechRecognizers},
};

#[path = "account.rs"]
mod account;
#[path = "admin_auth.rs"]
mod admin_auth;
#[path = "admin_console.rs"]
mod admin_console;
#[path = "auth.rs"]
mod auth;
#[path = "browser_sync.rs"]
mod browser_sync;
#[path = "catalog.rs"]
mod catalog;
#[path = "control.rs"]
mod control;
#[path = "control_auth.rs"]
mod control_auth;
pub use control_auth::authenticate_control_ingress;
#[path = "endpoints/builder.rs"]
mod builder;
#[path = "device_catalog.rs"]
mod device_catalog;
#[path = "directory.rs"]
mod directory;
#[path = "health.rs"]
mod health;
#[path = "pairing.rs"]
mod pairing;
#[path = "relay.rs"]
mod relay;
#[path = "endpoints/routes.rs"]
mod routes;
#[path = "search.rs"]
mod search;
#[path = "state.rs"]
mod state;
#[path = "station_icons.rs"]
mod station_icons;
#[path = "sync.rs"]
mod sync;
#[path = "transport.rs"]
mod transport;
#[path = "voice.rs"]
mod voice;
#[path = "yandex_home.rs"]
mod yandex_home;

pub use builder::RouterBuilder;
pub use health::{HealthResponse, HealthStatus};

/// Deterministic credential used only by convenience routers in offline tests and examples.
///
/// Production startup must supply a unique secret through [`router_with_services_and_bearer_token`].
pub const TEST_API_BEARER_TOKEN: &str = "rockserver-offline-test-token";

/// Environment variable containing the secret Caddy injects into trusted browser requests.
pub const TRUSTED_PROXY_TOKEN_ENV: &str = "ROCKSERVER_TRUSTED_PROXY_TOKEN";
/// Optional loopback-only origin accepted by administrator routes for explicit local development.
pub const LOCAL_ADMIN_ORIGIN_ENV: &str = "ROCKSERVER_LOCAL_ADMIN_ORIGIN";
/// Directory on the persistent server volume used for prepared station-icon WebP artifacts.
pub const STATION_ICON_DIR_ENV: &str = "ROCKSERVER_STATION_ICON_DIR";

/// Maximum duration the voice-command transport waits for query interpretation and search.
pub const DEFAULT_VOICE_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

/// Opens the optional persistent icon store configured for the production process.
///
/// Keeping this optional lets non-production routers retain their fully offline behavior; the
/// protected administrator endpoint reports the feature as unavailable until the operator mounts
/// and configures the persistent directory.
pub(super) fn station_icon_import_from_env(
    pool: sqlx::PgPool,
) -> Option<Arc<IconImportCoordinator>> {
    let root = env::var(STATION_ICON_DIR_ENV).ok()?;
    let storage: Arc<dyn IconStorage> = match FilesystemIconStorage::open(root) {
        Ok(storage) => Arc::new(storage),
        Err(error) => {
            tracing::error!(%error, "station icon storage is unavailable");
            return None;
        }
    };
    match IconImportCoordinator::new(pool, storage) {
        Ok(coordinator) => Some(Arc::new(coordinator)),
        Err(error) => {
            tracing::error!(%error, "station icon importer is unavailable");
            None
        }
    }
}

/// Marks an unfinished server-local icon worker interrupted during process startup.
///
/// This recovery performs only a short metadata update and never starts a download; a trusted
/// administrator must explicitly create the next import job from the console.
pub async fn recover_station_icon_imports(account_store: &PostgresAccountStore) {
    let Some(importer) = station_icon_import_from_env(account_store.pool()) else {
        return;
    };
    if let Err(error) = importer.interrupt_running().await {
        tracing::error!(%error, "could not recover interrupted station icon import");
    }
}

/// Creates the application router with the default in-memory catalog backend.
pub fn router() -> Router {
    RouterBuilder::default().build()
}

/// Creates the application router with a supplied station repository backend.
pub fn router_with_repository(repository: Arc<dyn StationRepository + Send + Sync>) -> Router {
    RouterBuilder::default().with_repository(repository).build()
}

/// Creates the application router with fully configured search orchestration.
pub fn router_with_search_service(search_service: SearchService) -> Router {
    RouterBuilder::default()
        .with_search_service(search_service)
        .build()
}

/// Creates the application router with an explicit voice-command service timeout.
///
/// The timeout covers query interpretation and repository search only. Audio capture and speech
/// recognition are intentionally outside this JSON transport boundary.
pub fn router_with_search_service_and_voice_timeout(
    search_service: SearchService,
    voice_command_timeout: Duration,
) -> Router {
    RouterBuilder::default()
        .with_search_service(search_service)
        .with_voice_command_timeout(voice_command_timeout)
        .build()
}

/// Creates the router with explicit search and streaming speech-recognition services.
pub fn router_with_services(
    search_service: SearchService,
    speech_recognizer: Arc<dyn crate::voice::StreamingSpeechRecognizer>,
    voice_command_timeout: Duration,
) -> Router {
    RouterBuilder::default()
        .with_search_service(search_service)
        .with_speech_recognizer(speech_recognizer)
        .with_voice_command_timeout(voice_command_timeout)
        .build()
}

/// Creates the router with explicit services and the Bearer credential required by application APIs.
pub fn router_with_services_and_bearer_token(
    search_service: SearchService,
    speech_recognizer: Arc<dyn crate::voice::StreamingSpeechRecognizer>,
    voice_command_timeout: Duration,
    api_bearer_token: impl Into<String>,
) -> Router {
    RouterBuilder::default()
        .with_search_service(search_service)
        .with_speech_recognizer(speech_recognizer)
        .with_voice_command_timeout(voice_command_timeout)
        .with_api_bearer_token(api_bearer_token)
        .build()
}

/// Creates the router with recognizers selectable by each voice WebSocket session.
pub fn router_with_speech_recognizers_and_bearer_token(
    search_service: SearchService,
    speech_recognizers: SpeechRecognizers,
    voice_command_timeout: Duration,
    api_bearer_token: impl Into<String>,
) -> Router {
    RouterBuilder::default()
        .with_search_service(search_service)
        .with_speech_recognizers(speech_recognizers)
        .with_voice_command_timeout(voice_command_timeout)
        .with_api_bearer_token(api_bearer_token)
        .build()
}

/// Creates the router with an explicit native-session resolver for device-facing endpoints.
///
/// The resolver authenticates the short-lived RockserverBearer device sessions accepted by
/// `GET /api/v1/device-control/catalog/*`; production routers derive it from the PostgreSQL
/// account store, while tests may supply a deterministic fake.
pub fn router_with_search_service_and_native_session_resolver(
    search_service: SearchService,
    voice_command_timeout: Duration,
    session_resolver: Arc<dyn crate::auth::NativeSessionResolver>,
) -> Router {
    RouterBuilder::default()
        .with_search_service(search_service)
        .with_voice_command_timeout(voice_command_timeout)
        .with_control_session_resolver(session_resolver)
        .build()
}

/// Creates a router with an explicit personal-data store and native-session resolver.
///
/// Offline tests use this to exercise `POST /api/v1/sync` against a deterministic store;
/// production routers derive both from the PostgreSQL account store instead.
pub fn router_with_personal_data_and_native_session_resolver(
    search_service: SearchService,
    voice_command_timeout: Duration,
    session_resolver: Arc<dyn crate::auth::NativeSessionResolver>,
    personal_store: Arc<dyn crate::personal_data::PersonalDataStore>,
) -> Router {
    RouterBuilder::default()
        .with_search_service(search_service)
        .with_voice_command_timeout(voice_command_timeout)
        .with_control_session_resolver(session_resolver)
        .with_personal_store(personal_store)
        .build()
}

/// Creates a router with the PostgreSQL account store required by passkey and pairing endpoints.
pub fn router_with_speech_recognizers_bearer_and_account_store(
    search_service: SearchService,
    speech_recognizers: SpeechRecognizers,
    voice_command_timeout: Duration,
    api_bearer_token: impl Into<String>,
    account_store: PostgresAccountStore,
) -> Router {
    router_with_speech_recognizers_bearer_account_store_and_proxy(
        search_service,
        speech_recognizers,
        voice_command_timeout,
        api_bearer_token,
        account_store,
        env::var(TRUSTED_PROXY_TOKEN_ENV).unwrap_or_default(),
    )
}

/// Creates the production router with account state and an authenticated Caddy proxy token.
pub fn router_with_speech_recognizers_bearer_account_store_and_proxy(
    search_service: SearchService,
    speech_recognizers: SpeechRecognizers,
    voice_command_timeout: Duration,
    api_bearer_token: impl Into<String>,
    account_store: PostgresAccountStore,
    trusted_proxy_token: impl Into<String>,
) -> Router {
    RouterBuilder::default()
        .with_search_service(search_service)
        .with_speech_recognizers(speech_recognizers)
        .with_voice_command_timeout(voice_command_timeout)
        .with_api_bearer_token(api_bearer_token)
        .with_postgres_account_stores(account_store)
        .with_trusted_proxy_token(trusted_proxy_token)
        .build()
}

/// Creates the production router with the separate durable administrator-authentication store.
pub fn router_with_speech_recognizers_bearer_account_admin_store_and_proxy(
    search_service: SearchService,
    speech_recognizers: SpeechRecognizers,
    voice_command_timeout: Duration,
    api_bearer_token: impl Into<String>,
    account_store: PostgresAccountStore,
    admin_store: PostgresAdminStore,
    trusted_proxy_token: impl Into<String>,
) -> Router {
    router_with_speech_recognizers_bearer_account_admin_store_proxy_and_voice_interpreter(
        search_service,
        (
            speech_recognizers,
            Arc::new(DeterministicCommandInterpreter),
        ),
        voice_command_timeout,
        api_bearer_token,
        account_store,
        admin_store,
        trusted_proxy_token,
        None,
    )
}

/// Creates the production router with an explicit typed voice-command interpreter.
#[allow(clippy::too_many_arguments)]
pub fn router_with_speech_recognizers_bearer_account_admin_store_proxy_and_voice_interpreter(
    search_service: SearchService,
    voice_services: (SpeechRecognizers, Arc<dyn CommandInterpreter>),
    voice_command_timeout: Duration,
    api_bearer_token: impl Into<String>,
    account_store: PostgresAccountStore,
    admin_store: PostgresAdminStore,
    trusted_proxy_token: impl Into<String>,
    yandex_home: Option<YandexHomeClient>,
) -> Router {
    let (speech_recognizers, voice_command_interpreter) = voice_services;
    RouterBuilder::default()
        .with_search_service(search_service)
        .with_speech_recognizers(speech_recognizers)
        .with_voice_command_interpreter(voice_command_interpreter)
        .with_voice_command_timeout(voice_command_timeout)
        .with_api_bearer_token(api_bearer_token)
        .with_postgres_account_stores(account_store)
        .with_postgres_admin_store(admin_store)
        .with_trusted_proxy_token(trusted_proxy_token)
        .with_yandex_home(yandex_home)
        .build()
}

/// Creates a deterministic device-voice harness over explicit control-plane dependencies.
pub fn router_with_device_voice_services(
    search_service: SearchService,
    speech_recognizer: Arc<dyn crate::voice::StreamingSpeechRecognizer>,
    voice_command_interpreter: Arc<dyn CommandInterpreter>,
    voice_command_timeout: Duration,
    session_resolver: Arc<dyn crate::auth::NativeSessionResolver>,
    control: (
        crate::device_control_presence::ConnectionRegistry,
        CommandRouter,
        Arc<dyn crate::device_control::DeviceControlStore>,
    ),
) -> Router {
    let (control_registry, control_commands, control_store) = control;
    RouterBuilder::default()
        .with_search_service(search_service)
        .with_speech_recognizer(speech_recognizer)
        .with_voice_command_interpreter(voice_command_interpreter)
        .with_voice_command_timeout(voice_command_timeout)
        .with_control_session_resolver(session_resolver)
        .with_control_registry(control_registry)
        .with_control_commands(control_commands)
        .with_control_store(control_store)
        .build()
}

/// Reads only an explicit HTTP loopback origin; arbitrary local-network origins stay rejected.
pub(super) fn local_admin_origin_from_env() -> Option<String> {
    let origin = env::var(LOCAL_ADMIN_ORIGIN_ENV).ok()?;
    if matches!(origin.as_str(), "http://localhost" | "http://127.0.0.1") {
        return Some(origin);
    }
    let port = origin
        .strip_prefix("http://127.0.0.1:")
        .or_else(|| origin.strip_prefix("http://localhost:"))?
        .parse::<u16>()
        .ok()?;
    (port != 0).then_some(origin)
}

/// Builds the shared command router with the station catalog wired for play_station resolution.
pub(super) fn station_resolving_command_router(search_service: &SearchService) -> CommandRouter {
    CommandRouter::default().with_station_catalog(Arc::new(search_service.clone()))
}

#[cfg(test)]
#[path = "endpoints/tests.rs"]
mod tests;
