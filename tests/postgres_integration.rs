//! PostgreSQL integration test suite decomposed into thematic modules.
//!
//! Submodules are declared with explicit path attributes to keep all test cases
//! within the single integration test target for controlled execution.

#[path = "postgres_integration/common.rs"]
mod common;

#[path = "postgres_integration/device_control.rs"]
mod device_control;

#[path = "postgres_integration/admin.rs"]
mod admin;

#[path = "postgres_integration/account.rs"]
mod account;

#[path = "postgres_integration/account_cleanup.rs"]
mod account_cleanup;

#[path = "postgres_integration/yandex_home.rs"]
mod yandex_home;

#[path = "postgres_integration/migrations.rs"]
mod migrations;

#[path = "postgres_integration/shared_catalog.rs"]
mod shared_catalog;

#[path = "postgres_integration/personal_data.rs"]
mod personal_data;
