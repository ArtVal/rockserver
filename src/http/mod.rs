//! HTTP transport for the RockServer API.

mod endpoints;

pub use endpoints::{
    DEFAULT_VOICE_COMMAND_TIMEOUT, HealthResponse, HealthStatus, RouterBuilder,
    TEST_API_BEARER_TOKEN, TRUSTED_PROXY_TOKEN_ENV, authenticate_control_ingress,
    recover_station_icon_imports, router, router_with_device_voice_services,
    router_with_personal_data_and_native_session_resolver, router_with_repository,
    router_with_search_service, router_with_search_service_and_native_session_resolver,
    router_with_search_service_and_voice_timeout, router_with_services,
    router_with_services_and_bearer_token, router_with_speech_recognizers_and_bearer_token,
    router_with_speech_recognizers_bearer_account_admin_store_and_proxy,
    router_with_speech_recognizers_bearer_account_admin_store_proxy_and_voice_interpreter,
    router_with_speech_recognizers_bearer_account_store_and_proxy,
    router_with_speech_recognizers_bearer_and_account_store,
};
