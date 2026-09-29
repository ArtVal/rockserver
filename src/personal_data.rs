//! Account-owned favourites and playback-history personal-data sync domain (RM-012-A).
//!
//! The server is the authoritative merge point for the personal data that RM-007-A clients
//! already hold locally. Records are identified by client-generated `record_id` UUIDs and
//! merged last-writer-wins on `updated_at` (equal instants keep the stored row); a shared
//! monotonic revision sequence gives each account a delta cursor, and tombstones carry
//! deletions to devices that were offline when the deletion happened. This module owns the
//! validated models, limits, merge semantics, and the persistence contract; SQL lives in
//! `persistence`, transport mapping in `http`.

#[path = "personal_data/domain.rs"]
mod domain;
#[path = "personal_data/in_memory.rs"]
mod in_memory;

pub use domain::*;
pub use in_memory::*;

#[cfg(test)]
#[path = "personal_data/tests.rs"]
mod tests;
