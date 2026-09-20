use crate::cli_config::{
    load_cli_config, mutate_write_target, normalize_core_root_to_absolute, resolve_cli_context,
    ConfigFile, ConnectionConfig,
};
use crate::config::print_json;
use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Args)]
pub struct ConfigCmd {
    #[command(subcommand)]
    pub sub: ConfigSubCmd,
}

#[derive(Subcommand)]
pub enum ConfigSubCmd {
    /// Show the effective canonical configuration and selected context.
    Current,
    /// Initialize a canonical TOML config (default: ~/.ugoite/config.toml)
    Init {
        /// Create project-local ./.ugoite/config.toml instead of global
        #[arg(long)]
        local: bool,
    },
    /// Manage named canonical connections
    Connection {
        #[command(subcommand)]
        sub: ConnectionSubCmd,
    },
}

#[derive(Subcommand)]
pub enum ConnectionSubCmd {
    /// List effective connections
    List,
    /// Show one connection
    Get {
        #[arg(value_name = "NAME")]
        name: String,
    },
    /// Add a named connection
    Add {
        #[arg(value_name = "NAME")]
        name: String,
        #[arg(long, value_name = "TYPE")]
        r#type: String,
        #[arg(long, value_name = "PATH")]
        root: Option<String>,
        #[arg(long, value_name = "URL")]
        url: Option<String>,
    },
    /// Update a connection (materializes the full object into the write target)
    Set {
        #[arg(value_name = "NAME")]
        name: String,
        #[arg(long, value_name = "TYPE")]
        r#type: Option<String>,
        #[arg(long, value_name = "PATH")]
        root: Option<String>,
        #[arg(long, value_name = "URL")]
        url: Option<String>,
    },
    /// Remove a connection (refused when contexts still reference it)
    Remove {
        #[arg(value_name = "NAME")]
        name: String,
    },
}

fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn build_connection(
    kind: &str,
    root: Option<String>,
    url: Option<String>,
    cwd: &Path,
) -> Result<ConnectionConfig> {
    match kind {
        "core" => {
            let root = root.ok_or_else(|| anyhow::anyhow!("--root is required for core"))?;
            Ok(ConnectionConfig::Core {
                root: normalize_core_root_to_absolute(&root, cwd),
            })
        }
        "backend" => {
            let url = url.ok_or_else(|| anyhow::anyhow!("--url is required for backend"))?;
            Ok(ConnectionConfig::Backend { url })
        }
        "api" => {
            let url = url.ok_or_else(|| anyhow::anyhow!("--url is required for api"))?;
            Ok(ConnectionConfig::Api { url })
        }
        other => bail!("Invalid --type {other:?}. Use core, backend, or api"),
    }
}

pub async fn run(
    cmd: ConfigCmd,
    explicit_config: Option<&Path>,
    explicit_context: Option<&str>,
) -> Result<()> {
    match cmd.sub {
        ConfigSubCmd::Current => {
            let cwd = cwd();
            let files = load_cli_config(explicit_config, &cwd)?;
            print_effective_current(
                &files.effective,
                &files.sources,
                &files.write_target,
                explicit_context,
            )?;
        }
        ConfigSubCmd::Init { local } => {
            let cwd = cwd();
            let target = if let Some(explicit) = explicit_config {
                explicit.to_path_buf()
            } else if local {
                crate::cli_config::discover::project_local_config_path(&cwd)
            } else {
                crate::cli_config::discover::canonical_global_config_path()
            };
            if target.exists() {
                bail!(
                    "Refusing to overwrite existing config at {}",
                    target.display()
                );
            }
            let mut config = ConfigFile::empty();
            config.connections.insert(
                "local".to_owned(),
                ConnectionConfig::Core {
                    root: normalize_core_root_to_absolute(&cwd.to_string_lossy(), &cwd),
                },
            );
            config.validate(&target.display().to_string())?;
            crate::cli_config::write::write_config_file_atomic(&target, &config)?;
            print_json(&serde_json::json!({
                "initialized": true,
                "path": target.to_string_lossy(),
            }));
        }
        ConfigSubCmd::Connection { sub } => {
            run_connection(sub, explicit_config).await?;
        }
    }
    Ok(())
}

async fn run_connection(sub: ConnectionSubCmd, explicit_config: Option<&Path>) -> Result<()> {
    let cwd = cwd();
    match sub {
        ConnectionSubCmd::List => {
            let files = load_cli_config(explicit_config, &cwd)?;
            let items: Vec<serde_json::Value> = files
                .effective
                .connections
                .iter()
                .map(|(name, value)| match &value.value {
                    ConnectionConfig::Core { root } => serde_json::json!({
                        "name": name, "type": "core", "root": root,
                        "source": value.source.to_string_lossy(),
                    }),
                    ConnectionConfig::Backend { url } => serde_json::json!({
                        "name": name, "type": "backend", "url": url,
                        "source": value.source.to_string_lossy(),
                    }),
                    ConnectionConfig::Api { url } => serde_json::json!({
                        "name": name, "type": "api", "url": url,
                        "source": value.source.to_string_lossy(),
                    }),
                })
                .collect();
            print_json(&items);
        }
        ConnectionSubCmd::Get { name } => {
            let files = load_cli_config(explicit_config, &cwd)?;
            let entry = files
                .effective
                .connections
                .get(&name)
                .ok_or_else(|| anyhow::anyhow!("Connection {name:?} is not defined."))?;
            print_json(&connection_json(&name, &entry.value, &entry.source));
        }
        ConnectionSubCmd::Add {
            name,
            r#type,
            root,
            url,
        } => {
            if name.trim().is_empty() {
                bail!("connection name must not be empty");
            }
            let files = load_cli_config(explicit_config, &cwd)?;
            if files.effective.connections.contains_key(&name) {
                bail!("Connection {name:?} already exists in effective config.");
            }
            let connection = build_connection(&r#type, root, url, &cwd)?;
            // Validate before writing (e.g. endpoint rules).
            crate::cli_config::model::validate_connection(&connection)?;
            let target = mutate_write_target(&files, |config| {
                config.connections.insert(name.clone(), connection.clone());
            })?;
            print_json(&serde_json::json!({
                "added": true,
                "name": name,
                "config": target.to_string_lossy(),
            }));
        }
        ConnectionSubCmd::Set {
            name,
            r#type,
            root,
            url,
        } => {
            let files = load_cli_config(explicit_config, &cwd)?;
            let current = files
                .effective
                .connections
                .get(&name)
                .ok_or_else(|| anyhow::anyhow!("Connection {name:?} is not defined."))?;
            // Start from the effective object, then apply requested fields so
            // the full object is materialized into the write target (no
            // partial overrides).
            let mut next = current.value.clone();
            if let Some(kind) = r#type {
                if kind == "core" && url.is_some() {
                    bail!("--url does not apply to core connections");
                }
                if (kind == "backend" || kind == "api") && root.is_some() {
                    bail!("--root only applies to core connections");
                }
                next = build_connection(&kind, root.clone(), url.clone(), &cwd)?;
            } else {
                match &mut next {
                    ConnectionConfig::Core { root: current_root } => {
                        if let Some(root) = root {
                            *current_root = normalize_core_root_to_absolute(&root, &cwd);
                        }
                        if url.is_some() {
                            bail!("--url does not apply to core connections");
                        }
                    }
                    ConnectionConfig::Backend { url: current_url }
                    | ConnectionConfig::Api { url: current_url } => {
                        if let Some(url) = url {
                            *current_url = url;
                        }
                        if root.is_some() {
                            bail!("--root only applies to core connections");
                        }
                    }
                }
            }
            crate::cli_config::model::validate_connection(&next)?;
            let target = mutate_write_target(&files, |config| {
                config.connections.insert(name.clone(), next.clone());
            })?;
            // Report which file actually changed; lower-priority files stay
            // untouched (local override pattern).
            print_json(&serde_json::json!({
                "updated": true,
                "name": name,
                "config": target.to_string_lossy(),
            }));
        }
        ConnectionSubCmd::Remove { name } => {
            let files = load_cli_config(explicit_config, &cwd)?;
            if !files.effective.connections.contains_key(&name) {
                bail!("Connection {name:?} is not defined.");
            }
            let referrers: Vec<String> = files
                .effective
                .contexts
                .iter()
                .filter(|(_, context)| context.value.connection == name)
                .map(|(context_name, _)| context_name.clone())
                .collect();
            if !referrers.is_empty() {
                bail!(
                    "Connection {name:?} is still referenced by contexts: {}. Remove or repoint them first.",
                    referrers.join(", ")
                );
            }
            let target = mutate_write_target(&files, |config| {
                config.connections.remove(&name);
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

fn connection_json(name: &str, connection: &ConnectionConfig, source: &Path) -> serde_json::Value {
    match connection {
        ConnectionConfig::Core { root } => serde_json::json!({
            "name": name, "type": "core", "root": root,
            "source": source.to_string_lossy(),
        }),
        ConnectionConfig::Backend { url } => serde_json::json!({
            "name": name, "type": "backend", "url": url,
            "source": source.to_string_lossy(),
        }),
        ConnectionConfig::Api { url } => serde_json::json!({
            "name": name, "type": "api", "url": url,
            "source": source.to_string_lossy(),
        }),
    }
}

fn print_effective_current(
    effective: &crate::cli_config::EffectiveConfig,
    sources: &[PathBuf],
    write_target: &Path,
    explicit_context: Option<&str>,
) -> Result<()> {
    println!("Config sources:");
    if sources.is_empty() {
        println!("  (none)");
    }
    for (index, source) in sources.iter().enumerate() {
        println!("  {}. {}", index + 1, source.display());
    }
    println!("Write target:");
    println!("  {}", write_target.display());
    match effective.current_context.as_ref() {
        None => {
            println!("Current context:");
            println!("  (none)");
        }
        Some(current) => {
            println!("Current context:");
            println!("  {}", current.value);
            if let Ok(resolved) = resolve_cli_context(effective, explicit_context) {
                println!("Connection:");
                println!(
                    "  {} ({})",
                    resolved.connection_name,
                    resolved.connection.transport_label()
                );
                match &resolved.connection {
                    crate::cli_config::ResolvedConnection::Core { root } => {
                        println!("Root:");
                        println!("  {}", root.display());
                    }
                    crate::cli_config::ResolvedConnection::Backend { url }
                    | crate::cli_config::ResolvedConnection::Api { url } => {
                        println!("Endpoint:");
                        println!("  {url}");
                    }
                }
                println!("Space:");
                // Render the UID through the same JSON-value boundary used by
                // `context list/get` and `space list`: the Space UID is a
                // non-secret immutable identifier (plaintext TOML, CLI arg,
                // list output), and the value boundary keeps every UID
                // display on the single established output path.
                let details = serde_json::json!({ "space_uid": resolved.space_uid });
                println!("  {}", details["space_uid"].as_str().unwrap_or_default());
                println!("Credential:");
                println!(
                    "  {}",
                    resolved.credential_name.as_deref().unwrap_or("none")
                );
            }
        }
    }
    Ok(())
}
