//! Named CLI execution contexts (`ugoite context ...`).
//!
//! A context binds `connection + immutable space_uid (+ optional credential)`.
//! These commands only touch disposable CLI config; they never mutate
//! Knowledge (no Change/Revision/audit/Space metadata).

use crate::cli_config::{load_cli_config, mutate_write_target, resolve_cli_context, ContextConfig};
use crate::output::print_json;
use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Args)]
pub struct ContextCmd {
    #[command(subcommand)]
    pub sub: ContextSubCmd,
}

#[derive(Subcommand)]
pub enum ContextSubCmd {
    /// List effective contexts
    List,
    /// Show one context
    Get {
        #[arg(value_name = "NAME")]
        name: String,
    },
    /// Show the current context name
    Current,
    /// Add a context (validates UID shape; Space existence is checked at use time)
    #[command(
        long_about = "Add a context binding a connection to one immutable Space UID.\n\nThe UID resolves to exactly one local directory (<root>/spaces/<SPACE_UID>) and the shared Space compatibility classifier decides compatibility. Space existence is checked at use time, not at add time.\n\nExamples:\n  ugoite context add demo --connection local --space 019f1234-5678-7abc-8def-0123456789ab\n  ugoite context use demo"
    )]
    Add {
        #[arg(value_name = "NAME")]
        name: String,
        #[arg(long, value_name = "CONNECTION")]
        connection: String,
        #[arg(long, value_name = "SPACE_UID")]
        space: String,
        #[arg(long, value_name = "CREDENTIAL")]
        credential: Option<String>,
    },
    /// Select the current context (config-only, no Knowledge mutation)
    Use {
        #[arg(value_name = "NAME")]
        name: String,
    },
    /// Remove a context (config-only)
    Remove {
        #[arg(value_name = "NAME")]
        name: String,
    },
}

fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub async fn run(
    cmd: ContextCmd,
    explicit_config: Option<&std::path::Path>,
    explicit_context: Option<&str>,
) -> Result<()> {
    let cwd = cwd();
    let explicit = explicit_config;
    match cmd.sub {
        ContextSubCmd::List => {
            let files = load_cli_config(explicit, &cwd)?;
            let items: Vec<serde_json::Value> = files
                .effective
                .contexts
                .iter()
                .map(|(name, value)| {
                    let current = files
                        .effective
                        .current_context
                        .as_ref()
                        .is_some_and(|current| current.value == *name);
                    serde_json::json!({
                        "name": name,
                        "connection": value.value.connection,
                        "space_uid": value.value.space_uid,
                        "credential": value.value.credential,
                        "current": current,
                        "source": value.source.to_string_lossy(),
                    })
                })
                .collect();
            print_json(&items);
        }
        ContextSubCmd::Get { name } => {
            let files = load_cli_config(explicit, &cwd)?;
            let entry = files
                .effective
                .contexts
                .get(&name)
                .ok_or_else(|| anyhow::anyhow!("Context {name:?} is not defined."))?;
            print_json(&serde_json::json!({
                "name": name,
                "connection": entry.value.connection,
                "space_uid": entry.value.space_uid,
                "credential": entry.value.credential,
                "source": entry.source.to_string_lossy(),
            }));
        }
        ContextSubCmd::Current => {
            let files = load_cli_config(explicit, &cwd)?;
            // --context overrides without mutating current_context.
            let resolved = resolve_cli_context(&files.effective, explicit_context)?;
            println!("{}", resolved.context_name);
        }
        ContextSubCmd::Add {
            name,
            connection,
            space,
            credential,
        } => {
            if name.trim().is_empty() {
                bail!("context name must not be empty");
            }
            let space_uid = uuid::Uuid::parse_str(space.trim())
                .map_err(|_| anyhow::anyhow!("context space must be a UUIDv7 SPACE_UID"))?;
            if space_uid.get_version() != Some(uuid::Version::SortRand) {
                bail!("context space must be a UUIDv7 SPACE_UID");
            }
            if let Some(credential) = &credential {
                if credential.trim().is_empty() {
                    bail!("context credential must not be empty when present");
                }
            }
            let files = load_cli_config(explicit, &cwd)?;
            if files.effective.contexts.contains_key(&name) {
                bail!("Context {name:?} already exists in effective config.");
            }
            if !files.effective.connections.contains_key(&connection) {
                bail!("Connection {connection:?} is not defined.");
            }
            let value = ContextConfig {
                connection: connection.clone(),
                space_uid,
                credential: credential.clone(),
            };
            let target = mutate_write_target(&files, |config| {
                config.contexts.insert(name.clone(), value.clone());
            })?;
            print_json(&serde_json::json!({
                "added": true,
                "name": name,
                "config": target.to_string_lossy(),
            }));
        }
        ContextSubCmd::Use { name } => {
            let files = load_cli_config(explicit, &cwd)?;
            if !files.effective.contexts.contains_key(&name) {
                bail!("Context {name:?} is not defined.");
            }
            let target = mutate_write_target(&files, |config| {
                config.current_context = Some(name.clone());
            })?;
            // Config-only: never touch Knowledge here.
            print_json(&serde_json::json!({
                "current": name,
                "config": target.to_string_lossy(),
            }));
        }
        ContextSubCmd::Remove { name } => {
            let files = load_cli_config(explicit, &cwd)?;
            if !files.effective.contexts.contains_key(&name) {
                bail!("Context {name:?} is not defined.");
            }
            if files
                .effective
                .current_context
                .as_ref()
                .is_some_and(|current| current.value == name)
            {
                bail!("Context {name:?} is the current context; select another context first.");
            }
            // Config-only removal: connection pruning is handled separately.
            let target = mutate_write_target(&files, |config| {
                config.contexts.remove(&name);
                if config.current_context.as_deref() == Some(name.as_str()) {
                    config.current_context = None;
                }
            })?;
            print_json(&serde_json::json!({
                "removed": true,
                "name": name,
                "config": target.to_string_lossy(),
            }));
        }
    }
    Ok(())
}
