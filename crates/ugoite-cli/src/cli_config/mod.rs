//! Canonical CLI configuration v1 (TOML) foundation.
//!
//! This module is additive only: existing `crate::config` (legacy
//! `cli-endpoints.json`) behavior is untouched. It implements plan sections
//! 5-8, 16-25, 47-48:
//! - TOML `ConfigFile` model (`version = 1`, named connections/contexts)
//! - CWD-only project-local discovery (no parent search)
//! - `UGOITE_CONFIG` stack + `--config` single-file override
//! - deterministic first-wins merge (no field-level merge)
//! - deterministic write-target resolution + atomic writes
//! - fail-closed validation + central context resolver
//!
//! CLI configuration is disposable work-environment context. It never mutates
//! Knowledge: no Change/Revision/audit/Space metadata may result from config
//! operations.

pub mod credentials;
pub mod discover;
pub mod merge;
pub mod model;
pub mod resolve;
pub mod runtime;
pub mod target;
pub mod write;

pub use credentials::{credentials_path, CredentialStore};
pub use discover::{
    build_source_stack, canonical_global_config_path, project_local_config_path,
    resolve_write_target, source_stack_from_environment,
};
pub use merge::{
    load_effective_config, load_explicit_config_file, merge_loaded_configs, EffectiveConfig,
    EffectiveValue, LoadedConfigFile,
};
pub use model::{ConfigFile, ConnectionConfig, ContextConfig};
pub use resolve::{resolve_cli_context, ResolvedCliContext, ResolvedConnection};
pub use runtime::{load_cli_config, mutate_write_target, read_write_target_file, CliConfigFiles};
pub use target::{
    resolve_command_target, resolve_command_triple, resolve_context_target, split_space_and_id,
    split_space_id_and_revision, SpaceTarget,
};
pub use write::{normalize_core_root_to_absolute, unique_context_name, write_config_file_atomic};
