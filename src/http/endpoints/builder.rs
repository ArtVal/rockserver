//! Fluent builder for configuring and assembling the application [`Router`].

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::Router;

use crate::{
    admin::AdminStore,
    auth::NativeSessionResolver,
    device_control::DeviceControlStore,
    device_control_command::CommandRouter,
    device_control_presence::ConnectionRegistry,
    device_control_state::StateHub,
    persistence::{
        PostgresAccountStore, PostgresAdminStore, PostgresDeviceControlStore,
        PostgresPersonalDataStore,
    },
    personal_data::PersonalDataStore,
    providers::yandex_home::YandexHomeClient,
    search::{
        InMemoryStationRepository, SearchService, StationRepository, UnavailableStationRepository,
    },
    station_icons::IconImportCoordinator,
    voice::{
        CommandInterpreter, DeterministicCommandInterpreter, SpeechRecognizers,
        StreamingSpeechRecognizer, UnavailableSpeechRecognizer,
    },
};

use super::{
    DEFAULT_VOICE_COMMAND_TIMEOUT, TEST_API_BEARER_TOKEN, local_admin_origin_from_env,
    routes::build_router,
    state::{AppState, ControlTiming, PublicLimitState},
    station_icon_import_from_env, station_resolving_command_router,
};

/// Fluent builder for the RockServer Axum [`Router`].
///
/// Encapsulates application state creation with sensible defaults for offline testing,
/// and allows fine-grained customization of search, auth, control, and storage services.
pub struct RouterBuilder {
    search_service: Option<SearchService>,
    speech_recognizers: Option<SpeechRecognizers>,
    voice_command_interpreter: Option<Arc<dyn CommandInterpreter>>,
    voice_command_timeout: Duration,
    api_bearer_token: String,
    account_store: Option<PostgresAccountStore>,
    admin_store: Option<Arc<dyn AdminStore>>,
    trusted_proxy_token: Option<String>,
    local_admin_origin: Option<String>,
    public_limits: Option<Arc<Mutex<PublicLimitState>>>,
    control_registry: ConnectionRegistry,
    control_commands: Option<CommandRouter>,
    control_state_hub: StateHub,
    control_store: Option<Arc<dyn DeviceControlStore>>,
    control_session_resolver: Option<Arc<dyn NativeSessionResolver>>,
    personal_store: Option<Arc<dyn PersonalDataStore>>,
    icon_import: Option<Arc<IconImportCoordinator>>,
    yandex_home: Option<Arc<YandexHomeClient>>,
    control_timing: ControlTiming,
}

impl Default for RouterBuilder {
    fn default() -> Self {
        Self {
            search_service: None,
            speech_recognizers: None,
            voice_command_interpreter: None,
            voice_command_timeout: DEFAULT_VOICE_COMMAND_TIMEOUT,
            api_bearer_token: TEST_API_BEARER_TOKEN.to_owned(),
            account_store: None,
            admin_store: None,
            trusted_proxy_token: None,
            local_admin_origin: local_admin_origin_from_env(),
            public_limits: None,
            control_registry: ConnectionRegistry::default(),
            control_commands: None,
            control_state_hub: StateHub::default(),
            control_store: None,
            control_session_resolver: None,
            personal_store: None,
            icon_import: None,
            yandex_home: None,
            control_timing: ControlTiming::default(),
        }
    }
}

impl RouterBuilder {
    /// Creates a new router builder with offline test defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the station repository backend, constructing a default [`SearchService`].
    pub fn with_repository(mut self, repository: Arc<dyn StationRepository + Send + Sync>) -> Self {
        self.search_service = Some(SearchService::new(repository));
        self
    }

    /// Sets the search orchestration service.
    pub fn with_search_service(mut self, search_service: SearchService) -> Self {
        self.search_service = Some(search_service);
        self
    }

    /// Sets a single streaming speech recognizer for all voice sessions.
    pub fn with_speech_recognizer(
        mut self,
        speech_recognizer: Arc<dyn StreamingSpeechRecognizer>,
    ) -> Self {
        self.speech_recognizers = Some(SpeechRecognizers::same(speech_recognizer));
        self
    }

    /// Sets session-selectable speech recognizers.
    pub fn with_speech_recognizers(mut self, speech_recognizers: SpeechRecognizers) -> Self {
        self.speech_recognizers = Some(speech_recognizers);
        self
    }

    /// Sets the typed voice-command interpreter.
    pub fn with_voice_command_interpreter(
        mut self,
        interpreter: Arc<dyn CommandInterpreter>,
    ) -> Self {
        self.voice_command_interpreter = Some(interpreter);
        self
    }

    /// Sets the voice command service timeout covering query interpretation and search.
    pub fn with_voice_command_timeout(mut self, timeout: Duration) -> Self {
        self.voice_command_timeout = timeout;
        self
    }

    /// Sets the Bearer credential required by application APIs.
    pub fn with_api_bearer_token(mut self, token: impl Into<String>) -> Self {
        self.api_bearer_token = token.into();
        self
    }

    /// Sets the PostgreSQL account store without auto-deriving ancillary stores.
    pub fn with_account_store(mut self, account_store: PostgresAccountStore) -> Self {
        self.account_store = Some(account_store);
        self
    }

    /// Configures the production PostgreSQL account store and derives the corresponding
    /// control session resolver, device control store, personal data store, and icon importer.
    pub fn with_postgres_account_stores(mut self, account_store: PostgresAccountStore) -> Self {
        let pool = account_store.pool();
        self.control_session_resolver = Some(Arc::new(account_store.clone()));
        self.control_store = Some(Arc::new(PostgresDeviceControlStore::from_pool(
            pool.clone(),
        )));
        self.personal_store = Some(Arc::new(PostgresPersonalDataStore::from_pool(pool.clone())));
        self.icon_import = station_icon_import_from_env(pool);
        self.account_store = Some(account_store);
        self
    }

    /// Sets the administrator-authentication store trait object.
    pub fn with_admin_store(mut self, admin_store: Arc<dyn AdminStore>) -> Self {
        self.admin_store = Some(admin_store);
        self
    }

    /// Sets the persistent PostgreSQL administrator-authentication store.
    pub fn with_postgres_admin_store(mut self, admin_store: PostgresAdminStore) -> Self {
        self.admin_store = Some(Arc::new(admin_store));
        self
    }

    /// Sets the authenticated proxy token injected by Caddy for browser requests.
    pub fn with_trusted_proxy_token(mut self, token: impl Into<String>) -> Self {
        self.trusted_proxy_token = Some(token.into());
        self
    }

    /// Overrides the loopback origin accepted by administrator routes.
    pub fn with_local_admin_origin(mut self, origin: Option<String>) -> Self {
        self.local_admin_origin = origin;
        self
    }

    /// Sets the connection registry for device control sockets.
    pub fn with_control_registry(mut self, registry: ConnectionRegistry) -> Self {
        self.control_registry = registry;
        self
    }

    /// Sets the command router for device control.
    pub fn with_control_commands(mut self, commands: CommandRouter) -> Self {
        self.control_commands = Some(commands);
        self
    }

    /// Sets the ephemeral device-control state hub.
    pub fn with_control_state_hub(mut self, hub: StateHub) -> Self {
        self.control_state_hub = hub;
        self
    }

    /// Sets the durable device-control store.
    pub fn with_control_store(mut self, store: Arc<dyn DeviceControlStore>) -> Self {
        self.control_store = Some(store);
        self
    }

    /// Sets the native-session resolver used by device-facing endpoints.
    pub fn with_control_session_resolver(
        mut self,
        resolver: Arc<dyn NativeSessionResolver>,
    ) -> Self {
        self.control_session_resolver = Some(resolver);
        self
    }

    /// Sets the account-owned personal-data store.
    pub fn with_personal_store(mut self, store: Arc<dyn PersonalDataStore>) -> Self {
        self.personal_store = Some(store);
        self
    }

    /// Sets the station icon import coordinator.
    pub fn with_icon_import(mut self, coordinator: Option<Arc<IconImportCoordinator>>) -> Self {
        self.icon_import = coordinator;
        self
    }

    /// Sets the optional read-only Yandex Smart Home client.
    pub fn with_yandex_home(mut self, client: Option<YandexHomeClient>) -> Self {
        self.yandex_home = client.map(Arc::new);
        self
    }

    /// Sets server-owned timing policies for device-control connections.
    #[cfg(test)]
    pub(crate) fn with_control_timing(mut self, timing: ControlTiming) -> Self {
        self.control_timing = timing;
        self
    }

    /// Resolves all configured and default dependencies into [`AppState`].
    pub(super) fn build_state(self) -> AppState {
        let search_service = self.search_service.unwrap_or_else(|| {
            match InMemoryStationRepository::with_builtin_catalog() {
                Ok(repository) => SearchService::new(Arc::new(repository)),
                Err(error) => SearchService::new(Arc::new(
                    UnavailableStationRepository::from_preflight_error(error),
                )),
            }
        });
        let control_commands = self
            .control_commands
            .unwrap_or_else(|| station_resolving_command_router(&search_service));
        let speech_recognizers = self
            .speech_recognizers
            .unwrap_or_else(|| SpeechRecognizers::same(Arc::new(UnavailableSpeechRecognizer)));
        let voice_command_interpreter = self
            .voice_command_interpreter
            .unwrap_or_else(|| Arc::new(DeterministicCommandInterpreter));
        let public_limits = self
            .public_limits
            .unwrap_or_else(|| Arc::new(Mutex::new(PublicLimitState::default())));

        AppState {
            search_service,
            speech_recognizers,
            voice_command_interpreter,
            voice_command_timeout: self.voice_command_timeout,
            api_bearer_token: self.api_bearer_token,
            account_store: self.account_store,
            admin_store: self.admin_store,
            trusted_proxy_token: self.trusted_proxy_token,
            local_admin_origin: self.local_admin_origin,
            public_limits,
            control_registry: self.control_registry,
            control_commands,
            control_state_hub: self.control_state_hub,
            control_store: self.control_store,
            control_session_resolver: self.control_session_resolver,
            personal_store: self.personal_store,
            icon_import: self.icon_import,
            yandex_home: self.yandex_home,
            control_timing: self.control_timing,
        }
    }

    /// Assembles the complete Axum [`Router`] from the configured dependencies.
    pub fn build(self) -> Router {
        build_router(self.build_state())
    }
}
