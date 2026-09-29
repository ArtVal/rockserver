//! Structural checks for the contract-first OpenAPI document and golden protocol fixtures.

#[path = "openapi_contract/common.rs"]
mod common;
#[path = "openapi_contract/device_catalog.rs"]
mod device_catalog;
#[path = "openapi_contract/device_control.rs"]
mod device_control;
#[path = "openapi_contract/personal_sync.rs"]
mod personal_sync;
#[path = "openapi_contract/surface.rs"]
mod surface;
#[path = "openapi_contract/voice_stream.rs"]
mod voice_stream;
