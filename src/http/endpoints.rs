//! HTTP route composition for the RockServer API.

use std::{
    env,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::Router;
use tower_http::trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::{
    device_control_command::CommandRouter,
    persistence::{
        PostgresAccountStore, PostgresAdminStore, PostgresDeviceControlStore,
        PostgresPersonalDataStore,
    },
    providers::yandex_home::YandexHomeClient,
    search::{
        InMemoryStationRepository, SearchService, StationRepository, UnavailableStationRepository,
    },
    station_icons::{FilesystemIconStorage, IconImportCoordinator, IconStorage},
    voice::{
        CommandInterpreter, DeterministicCommandInterpreter, SpeechRecognizers,
        UnavailableSpeechRecognizer,
    },
};

#[path = "account.rs"]
mod account;
#[path = "admin_auth.rs"]
mod admin_auth;
#[path = "admin_console.rs"]
mod admin_console;
#[path = "auth.rs"]
mod auth;
#[path = "catalog.rs"]
mod catalog;
#[path = "control.rs"]
mod control;
#[path = "control_auth.rs"]
mod control_auth;
pub use control_auth::authenticate_control_ingress;
#[path = "device_catalog.rs"]
mod device_catalog;
#[path = "directory.rs"]
mod directory;
#[path = "health.rs"]
mod health;
#[path = "pairing.rs"]
mod pairing;
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

pub use health::{HealthResponse, HealthStatus};
use state::{AppState, PublicLimitState};

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
fn station_icon_import_from_env(pool: sqlx::PgPool) -> Option<Arc<IconImportCoordinator>> {
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
    let repository: Arc<dyn StationRepository + Send + Sync> =
        match InMemoryStationRepository::with_builtin_catalog() {
            Ok(repository) => Arc::new(repository),
            Err(error) => Arc::new(UnavailableStationRepository::from_preflight_error(error)),
        };
    router_with_repository(repository)
}

/// Creates the application router with a supplied station repository backend.
pub fn router_with_repository(repository: Arc<dyn StationRepository + Send + Sync>) -> Router {
    router_with_search_service(SearchService::new(repository))
}

/// Creates the application router with fully configured search orchestration.
pub fn router_with_search_service(search_service: SearchService) -> Router {
    router_with_services(
        search_service,
        Arc::new(UnavailableSpeechRecognizer),
        DEFAULT_VOICE_COMMAND_TIMEOUT,
    )
}

/// Creates the application router with an explicit voice-command service timeout.
///
/// The timeout covers query interpretation and repository search only. Audio capture and speech
/// recognition are intentionally outside this JSON transport boundary.
pub fn router_with_search_service_and_voice_timeout(
    search_service: SearchService,
    voice_command_timeout: Duration,
) -> Router {
    router_with_services(
        search_service,
        Arc::new(UnavailableSpeechRecognizer),
        voice_command_timeout,
    )
}

/// Creates the router with explicit search and streaming speech-recognition services.
pub fn router_with_services(
    search_service: SearchService,
    speech_recognizer: Arc<dyn crate::voice::StreamingSpeechRecognizer>,
    voice_command_timeout: Duration,
) -> Router {
    router_with_services_and_bearer_token(
        search_service,
        speech_recognizer,
        voice_command_timeout,
        TEST_API_BEARER_TOKEN,
    )
}

/// Creates the router with explicit services and the Bearer credential required by application APIs.
pub fn router_with_services_and_bearer_token(
    search_service: SearchService,
    speech_recognizer: Arc<dyn crate::voice::StreamingSpeechRecognizer>,
    voice_command_timeout: Duration,
    api_bearer_token: impl Into<String>,
) -> Router {
    router_with_speech_recognizers_and_bearer_token(
        search_service,
        SpeechRecognizers::same(speech_recognizer),
        voice_command_timeout,
        api_bearer_token,
    )
}

/// Creates the router with recognizers selectable by each voice WebSocket session.
pub fn router_with_speech_recognizers_and_bearer_token(
    search_service: SearchService,
    speech_recognizers: SpeechRecognizers,
    voice_command_timeout: Duration,
    api_bearer_token: impl Into<String>,
) -> Router {
    let control_commands = station_resolving_command_router(&search_service);
    build_router(AppState {
        search_service,
        speech_recognizers,
        voice_command_interpreter: Arc::new(DeterministicCommandInterpreter),
        voice_command_timeout,
        api_bearer_token: api_bearer_token.into(),
        account_store: None,
        admin_store: None,
        trusted_proxy_token: None,
        local_admin_origin: local_admin_origin_from_env(),
        public_limits: Arc::new(Mutex::new(PublicLimitState::default())),
        control_registry: Default::default(),
        control_commands,
        control_state_hub: Default::default(),
        control_store: None,
        control_session_resolver: None,
        personal_store: None,
        icon_import: None,
        yandex_home: None,
        control_timing: Default::default(),
    })
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
    let control_commands = station_resolving_command_router(&search_service);
    build_router(AppState {
        search_service,
        speech_recognizers: SpeechRecognizers::same(Arc::new(UnavailableSpeechRecognizer)),
        voice_command_interpreter: Arc::new(DeterministicCommandInterpreter),
        voice_command_timeout,
        api_bearer_token: TEST_API_BEARER_TOKEN.to_owned(),
        account_store: None,
        admin_store: None,
        trusted_proxy_token: None,
        local_admin_origin: local_admin_origin_from_env(),
        public_limits: Arc::new(Mutex::new(PublicLimitState::default())),
        control_registry: Default::default(),
        control_commands,
        control_state_hub: Default::default(),
        control_store: None,
        control_session_resolver: Some(session_resolver),
        personal_store: None,
        icon_import: None,
        yandex_home: None,
        control_timing: Default::default(),
    })
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
    let control_commands = station_resolving_command_router(&search_service);
    build_router(AppState {
        search_service,
        speech_recognizers: SpeechRecognizers::same(Arc::new(UnavailableSpeechRecognizer)),
        voice_command_interpreter: Arc::new(DeterministicCommandInterpreter),
        voice_command_timeout,
        api_bearer_token: TEST_API_BEARER_TOKEN.to_owned(),
        account_store: None,
        admin_store: None,
        trusted_proxy_token: None,
        local_admin_origin: local_admin_origin_from_env(),
        public_limits: Arc::new(Mutex::new(PublicLimitState::default())),
        control_registry: Default::default(),
        control_commands,
        control_state_hub: Default::default(),
        control_store: None,
        control_session_resolver: Some(session_resolver),
        personal_store: Some(personal_store),
        icon_import: None,
        yandex_home: None,
        control_timing: Default::default(),
    })
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
    let control_session_resolver: Arc<dyn crate::auth::NativeSessionResolver> =
        Arc::new(account_store.clone());
    let control_store: Arc<dyn crate::device_control::DeviceControlStore> =
        Arc::new(PostgresDeviceControlStore::from_pool(account_store.pool()));
    let personal_store: Arc<dyn crate::personal_data::PersonalDataStore> =
        Arc::new(PostgresPersonalDataStore::from_pool(account_store.pool()));
    let icon_import = station_icon_import_from_env(account_store.pool());
    let control_commands = station_resolving_command_router(&search_service);
    build_router(AppState {
        search_service,
        speech_recognizers,
        voice_command_interpreter: Arc::new(DeterministicCommandInterpreter),
        voice_command_timeout,
        api_bearer_token: api_bearer_token.into(),
        account_store: Some(account_store),
        admin_store: None,
        trusted_proxy_token: Some(trusted_proxy_token.into()),
        local_admin_origin: local_admin_origin_from_env(),
        public_limits: Arc::new(Mutex::new(PublicLimitState::default())),
        control_registry: Default::default(),
        control_commands,
        control_state_hub: Default::default(),
        control_store: Some(control_store),
        control_session_resolver: Some(control_session_resolver),
        personal_store: Some(personal_store),
        icon_import,
        yandex_home: None,
        control_timing: Default::default(),
    })
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
    let control_session_resolver: Arc<dyn crate::auth::NativeSessionResolver> =
        Arc::new(account_store.clone());
    let control_store: Arc<dyn crate::device_control::DeviceControlStore> =
        Arc::new(PostgresDeviceControlStore::from_pool(account_store.pool()));
    let personal_store: Arc<dyn crate::personal_data::PersonalDataStore> =
        Arc::new(PostgresPersonalDataStore::from_pool(account_store.pool()));
    let icon_import = station_icon_import_from_env(account_store.pool());
    let control_commands = station_resolving_command_router(&search_service);
    build_router(AppState {
        search_service,
        speech_recognizers,
        voice_command_interpreter,
        voice_command_timeout,
        api_bearer_token: api_bearer_token.into(),
        account_store: Some(account_store),
        admin_store: Some(Arc::new(admin_store)),
        trusted_proxy_token: Some(trusted_proxy_token.into()),
        local_admin_origin: local_admin_origin_from_env(),
        public_limits: Arc::new(Mutex::new(PublicLimitState::default())),
        control_registry: Default::default(),
        control_commands,
        control_state_hub: Default::default(),
        control_store: Some(control_store),
        control_session_resolver: Some(control_session_resolver),
        personal_store: Some(personal_store),
        icon_import,
        yandex_home: yandex_home.map(Arc::new),
        control_timing: Default::default(),
    })
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
    build_router(AppState {
        search_service,
        speech_recognizers: SpeechRecognizers::same(speech_recognizer),
        voice_command_interpreter,
        voice_command_timeout,
        api_bearer_token: TEST_API_BEARER_TOKEN.to_owned(),
        account_store: None,
        admin_store: None,
        trusted_proxy_token: None,
        local_admin_origin: local_admin_origin_from_env(),
        public_limits: Arc::new(Mutex::new(PublicLimitState::default())),
        control_registry,
        control_commands,
        control_state_hub: Default::default(),
        control_store: Some(control_store),
        control_session_resolver: Some(session_resolver),
        personal_store: None,
        icon_import: None,
        yandex_home: None,
        control_timing: Default::default(),
    })
}

/// Reads only an explicit HTTP loopback origin; arbitrary local-network origins stay rejected.
fn local_admin_origin_from_env() -> Option<String> {
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
fn station_resolving_command_router(search_service: &SearchService) -> CommandRouter {
    CommandRouter::default().with_station_catalog(Arc::new(search_service.clone()))
}

fn build_router(state: AppState) -> Router {
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

#[cfg(test)]
mod tests {
    use argon2::{
        Argon2, PasswordHasher,
        password_hash::{SaltString, rand_core::OsRng},
    };
    use axum::{
        body::Body,
        http::{Request, StatusCode, header},
    };
    use http_body_util::BodyExt;
    use sha2::{Digest, Sha256};
    use tower::ServiceExt;
    use uuid::Uuid;

    use super::{
        AppState, PublicLimitState, build_router,
        health::{HealthResponse, HealthStatus},
        router,
    };
    use crate::{
        admin::{
            AdminPasswordHash, AdminPrincipal, AdminPrincipalStatus, AdminStore, AdminUsername,
            FakeAdminStore, NewAdminBootstrap, NewAdminSession,
        },
        auth::SecretHash,
        search::{InMemoryStationRepository, SearchService},
        voice::{SpeechRecognizers, UnavailableSpeechRecognizer},
    };
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };

    #[tokio::test]
    async fn liveness_returns_stable_json_response() {
        assert_health_endpoint("/health/live").await;
    }

    #[tokio::test]
    async fn direct_server_does_not_serve_the_admin_spa() {
        let response = router()
            .oneshot(Request::get("/admin").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn public_icon_route_returns_a_non_cacheable_miss_without_configured_storage() {
        let response = router()
            .oneshot(
                Request::get("/api/v1/stations/station-rock-001/icon")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }

    #[tokio::test]
    async fn protected_admin_station_read_model_requires_and_accepts_a_revocable_session() {
        let token = "admin-test-token";
        let store = Arc::new(FakeAdminStore::default());
        let principal_id = Uuid::new_v4();
        store
            .create_principal(AdminPrincipal {
                id: principal_id,
                status: AdminPrincipalStatus::Active,
            })
            .await
            .unwrap();
        store
            .create_session(NewAdminSession {
                id: Uuid::new_v4(),
                principal_id,
                token_hash: SecretHash::new(Sha256::digest(token.as_bytes()).into()),
                ttl_seconds: 60,
            })
            .await
            .unwrap();
        let app = build_router(AppState {
            search_service: SearchService::new(Arc::new(
                InMemoryStationRepository::with_builtin_catalog().unwrap(),
            )),
            speech_recognizers: SpeechRecognizers::same(Arc::new(UnavailableSpeechRecognizer)),
            voice_command_interpreter: Arc::new(crate::voice::DeterministicCommandInterpreter),
            voice_command_timeout: Duration::from_secs(5),
            api_bearer_token: "unrelated".to_owned(),
            account_store: None,
            admin_store: Some(store),
            trusted_proxy_token: None,
            local_admin_origin: None,
            public_limits: Arc::new(Mutex::new(PublicLimitState::default())),
            control_registry: Default::default(),
            control_commands: Default::default(),
            control_state_hub: Default::default(),
            control_store: None,
            control_session_resolver: None,
            personal_store: None,
            icon_import: None,
            yandex_home: None,
            control_timing: Default::default(),
        });
        let denied = app
            .clone()
            .oneshot(
                Request::get("/api/v1/admin/stations")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
        let origin_rejected = app
            .clone()
            .oneshot(
                Request::post("/api/v1/admin/icons/import")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(origin_rejected.status(), StatusCode::FORBIDDEN);
        let manual_icon_requires_configured_storage = app
            .clone()
            .oneshot(
                Request::put("/api/v1/admin/stations/station-rock-001/icon")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header("origin", super::transport::FIRST_PARTY_ORIGIN)
                    .header("x-forwarded-proto", "https")
                    .body(Body::from("not an image"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            manual_icon_requires_configured_storage.status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        let allowed = app
            .oneshot(
                Request::get("/api/v1/admin/stations")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(allowed.status(), StatusCode::OK);
        assert_eq!(allowed.headers()[header::CACHE_CONTROL], "no-store");
        let body = allowed.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()["limit"].as_u64(),
            Some(50)
        );
        assert!(
            !serde_json::from_slice::<serde_json::Value>(&body).unwrap()["items"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn admin_refresh_atomically_replaces_the_bearer_and_records_safe_request_metadata() {
        let token = "admin-refresh-old-token";
        let store = Arc::new(FakeAdminStore::default());
        let records = Arc::clone(&store);
        let principal_id = Uuid::new_v4();
        store
            .create_principal(AdminPrincipal {
                id: principal_id,
                status: AdminPrincipalStatus::Active,
            })
            .await
            .unwrap();
        store
            .create_session(NewAdminSession {
                id: Uuid::new_v4(),
                principal_id,
                token_hash: SecretHash::new(Sha256::digest(token.as_bytes()).into()),
                ttl_seconds: 60,
            })
            .await
            .unwrap();
        let app = build_router(AppState {
            search_service: SearchService::new(Arc::new(
                InMemoryStationRepository::with_builtin_catalog().unwrap(),
            )),
            speech_recognizers: SpeechRecognizers::same(Arc::new(UnavailableSpeechRecognizer)),
            voice_command_interpreter: Arc::new(crate::voice::DeterministicCommandInterpreter),
            voice_command_timeout: Duration::from_secs(5),
            api_bearer_token: "unrelated".to_owned(),
            account_store: None,
            admin_store: Some(store),
            trusted_proxy_token: None,
            local_admin_origin: None,
            public_limits: Arc::new(Mutex::new(PublicLimitState::default())),
            control_registry: Default::default(),
            control_commands: Default::default(),
            control_state_hub: Default::default(),
            control_store: None,
            control_session_resolver: None,
            personal_store: None,
            icon_import: None,
            yandex_home: None,
            control_timing: Default::default(),
        });
        let refresh = Request::post("/api/v1/admin/auth/refresh")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header("origin", super::transport::FIRST_PARTY_ORIGIN)
            .header("x-forwarded-proto", "https")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(refresh).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let fresh_token = serde_json::from_slice::<serde_json::Value>(
            &response.into_body().collect().await.unwrap().to_bytes(),
        )
        .unwrap()["access_token"]
            .as_str()
            .unwrap()
            .to_owned();
        let old = app
            .clone()
            .oneshot(
                Request::get("/api/v1/admin/session")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(old.status(), StatusCode::UNAUTHORIZED);
        let current = app
            .oneshot(
                Request::get("/api/v1/admin/session")
                    .header(header::AUTHORIZATION, format!("Bearer {fresh_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(current.status(), StatusCode::OK);
        let request_records = records.request_records();
        assert_eq!(request_records.len(), 2);
        assert!(
            request_records
                .iter()
                .all(|record| !record.request_id.contains(token))
        );
        assert!(request_records.iter().all(|record| matches!(
            record.endpoint,
            "/api/v1/admin/auth/refresh" | "/api/v1/admin/session"
        )));
    }

    #[tokio::test]
    async fn admin_login_accepts_valid_credentials_and_rejects_invalid_credentials() {
        let store = Arc::new(FakeAdminStore::default());
        let principal_id = Uuid::new_v4();
        let hash = Argon2::default()
            .hash_password(b"correct password", &SaltString::generate(&mut OsRng))
            .unwrap()
            .to_string();
        store
            .bootstrap_admin(NewAdminBootstrap {
                principal_id,
                credential_id: Uuid::new_v4(),
                security_event_id: Uuid::new_v4(),
                username: AdminUsername::parse("admin".to_owned()).unwrap(),
                password_hash: AdminPasswordHash::parse(hash).unwrap(),
            })
            .await
            .unwrap();
        let app = build_router(AppState {
            search_service: SearchService::new(Arc::new(
                InMemoryStationRepository::with_builtin_catalog().unwrap(),
            )),
            speech_recognizers: SpeechRecognizers::same(Arc::new(UnavailableSpeechRecognizer)),
            voice_command_interpreter: Arc::new(crate::voice::DeterministicCommandInterpreter),
            voice_command_timeout: Duration::from_secs(5),
            api_bearer_token: "unrelated".to_owned(),
            account_store: None,
            admin_store: Some(store),
            trusted_proxy_token: None,
            local_admin_origin: Some("http://127.0.0.1:3000".to_owned()),
            public_limits: Arc::new(Mutex::new(PublicLimitState::default())),
            control_registry: Default::default(),
            control_commands: Default::default(),
            control_state_hub: Default::default(),
            control_store: None,
            control_session_resolver: None,
            personal_store: None,
            icon_import: None,
            yandex_home: None,
            control_timing: Default::default(),
        });
        let login = |password: &str| {
            Request::post("/api/v1/admin/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .header("origin", "http://127.0.0.1:3000")
                .body(Body::from(
                    serde_json::json!({"username":"admin", "password":password}).to_string(),
                ))
                .unwrap()
        };
        assert_eq!(
            app.clone()
                .oneshot(login("incorrect"))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let response = app.oneshot(login("correct password")).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert!(
            serde_json::from_slice::<serde_json::Value>(
                &response.into_body().collect().await.unwrap().to_bytes()
            )
            .unwrap()["access_token"]
                .as_str()
                .unwrap()
                .len()
                >= 32
        );
    }

    #[tokio::test]
    async fn readiness_returns_stable_json_response() {
        assert_health_endpoint("/health/ready").await;
    }

    #[tokio::test]
    async fn pairing_creation_fails_closed_without_the_postgres_account_store() {
        let response = router()
            .oneshot(
                Request::post("/api/v1/pairing-requests")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"device_display_name":"Test desktop","device_type":"windows"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()["code"],
            "auth_unavailable"
        );
    }

    #[tokio::test]
    async fn pairing_completion_rejects_a_client_supplied_owner() {
        let response = router()
            .oneshot(
                Request::post("/api/v1/pairing-requests/00000000-0000-0000-0000-000000000000/complete")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"desktop_token":"0123456789abcdef","user_id":"00000000-0000-0000-0000-000000000000"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn passkey_authentication_rejects_a_client_supplied_account_identifier() {
        let response = router()
            .oneshot(
                Request::post("/api/v1/auth/passkeys/authentication/verify")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"challenge_id":"00000000-0000-0000-0000-000000000000","user_id":"00000000-0000-0000-0000-000000000000","id":"x","authenticatorData":"x","signature":"x","clientDataJSON":"x","userHandle":"x"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn browser_session_refresh_rejects_direct_requests() {
        let response = router()
            .oneshot(
                Request::post("/api/v1/auth/browser-session")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    async fn assert_health_endpoint(uri: &str) {
        let response = router()
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let payload: HealthResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            payload,
            HealthResponse {
                status: HealthStatus::Ok
            }
        );
        assert_eq!(body.as_ref(), br#"{"status":"ok"}"#);
    }
}
