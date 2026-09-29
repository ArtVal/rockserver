//! Administrator-only identity and persistence contracts.
//!
//! These types are deliberately independent from passkey users, native devices,
//! browser sessions, and RockCast machine-client credentials.

pub mod domain;
pub mod fake;
#[cfg(test)]
mod tests;

pub use domain::*;
pub use fake::*;
