//! Node-scoped authentication, recovery, OAuth, and credential state.
//!
//! This crate never owns portable Space authorization decisions.

pub mod control_store;
pub mod node_identity;
pub mod oauth;
pub mod secret_store;

pub use control_store::{ControlRecord, NodeControlStore, OpenDalNodeControlStore};
pub use secret_store::{EnvironmentSecretStore, NodeSecretStore};
