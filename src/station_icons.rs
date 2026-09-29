//! Persistent storage and processing for prepared station-icon artifacts.
//!
//! This module coordinates icon normalization, storage, external favicon discovery,
//! and administrative background import jobs across isolated layers:
//!
//! - [`domain`]: Image normalization, WebP encoding, content-addressed keys, and abstract traits.
//! - [`storage`]: Filesystem storage implementation for normalized artifacts.
//! - [`fetcher`]: Bounded external HTTP fetching with SSRF protection and HTML favicon discovery.
//! - [`coordinator`]: PostgreSQL background import coordination, state machines, and manual overrides.

#[path = "station_icons/coordinator.rs"]
mod coordinator;
#[path = "station_icons/domain.rs"]
mod domain;
#[path = "station_icons/fetcher.rs"]
mod fetcher;
#[path = "station_icons/storage.rs"]
mod storage;

pub use coordinator::IconImportCoordinator;
pub use domain::{
    IconJobProgress, IconSourceFetcher, IconStorage, IconStorageError, IconStorageKey,
    IconValidationError, MAX_SOURCE_BYTES, ManualIconError, PreparedIcon, ReadyIcon, prepare_icon,
};
pub use fetcher::SafeIconFetcher;
pub use storage::FilesystemIconStorage;

#[cfg(test)]
#[path = "station_icons/tests.rs"]
mod tests;
