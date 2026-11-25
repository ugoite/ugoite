//! Shared test process helper.
//!
//! This module intentionally provides no compatibility behavior: it
//! re-exports the standard process [`Command`] so tests invoke the
//! canonical CLI directly. Tests set up canonical config/context
//! explicitly (`config init`, `config connection set`, `space create`)
//! instead of relying on behind-the-scenes generation.

pub use std::process::Command;
