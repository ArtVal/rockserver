//! Bounded, owner-scoped command admission and live routing for protocol v1.

pub mod catalog;
pub mod router;
pub mod target_validation;

pub use catalog::*;
pub use router::*;
pub use target_validation::*;

#[cfg(test)]
#[path = "device_control_command/tests.rs"]
mod tests;
