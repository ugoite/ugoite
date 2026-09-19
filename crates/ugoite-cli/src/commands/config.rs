use crate::cli_config::{
    load_cli_config, mutate_write_target, normalize_core_root_to_absolute, resolve_cli_context,
    ConfigFile, ConnectionConfig,
};
use crate::config::{
    endpoint_transport_warning, load_config, print_json, save_config,
    validate_active_remote_endpoint, validate_server_endpoint_url, EndpointMode,
};
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
    /// Show saved endpoint config
    Show,
    /// Show the active endpoint mode in plain language
    Current,
    /// Save endpoint config (mode, backend URL, API URL)
    #[command(
        long_about = "Save endpoint configuration.\n\nWhich mode should you use?\n  core     - Default. Use when you are working directly with a local checkout or local spaces/ directory.\n  backend  - Use when you want the CLI to talk to a backend server directly.\n  api      - Use when you want the CLI to use the same proxied /api surface as the frontend.\n\nWhy core is the default:\n  core keeps the CLI local-first. Commands read and write your filesystem directly, with no server required.\n\nRemote credentialed endpoints MUST use HTTPS. Cleartext http:// is only accepted for loopback development hosts (`localhost`, `127.0.0.1`, `[::1]`).\n\nExamples:\n  # Core mode (default, uses local filesystem)\n  ugoite config set --mode core\n\n  # Backend mode (connect to local backend)\n  ugoite config set --mode backend --backend-url http://localhost:8000\n\n  # API mode (same proxied /api surface as the frontend)\n  ugoite config set --mode api --api-url https://example.com/api\n\n  # Update only the backend URL (keep current mode)\n  ugoite config set --backend-url http://localhost:9000"
    )]
    Set {
        #[arg(
            long,
            help = "Endpoint mode: core (local spaces/ on this machine, default), backend (direct backend server), or api (same proxied /api surface as the frontend)"
        )]
        mode: Option<String>,
        #[arg(
            long,
            help = "Backend server URL (used in backend mode; use https:// for non-loopback hosts, e.g. http://localhost:8000)"
        )]
        backend_url: Option<String>,
        #[arg(
            long,
            help = "API endpoint URL (used in api mode; use https:// for non-loopback hosts)"
        )]
        api_url: Option<String>,
    },
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

/// True when any canonical candidate file exists (project-local, global, or
/// any UGOITE_CONFIG entry). Used to fail closed instead of silently falling
/// back to legacy output.
fn has_canonical_candidate(cwd: &Path) -> bool {
    if crate::cli_config::discover::project_local_config_path(cwd).is_file() {
        return true;
    }
    if crate::cli_config::discover::canonical_global_config_path().is_file() {
        return true;
    }
    if let Ok(value) = std::env::var("UGOITE_CONFIG") {
        for entry in std::env::split_paths(&value) {
            if !entry.as_os_str().is_empty() && entry.is_file() {
                return true;
            }
        }
    }
    false
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
        ConfigSubCmd::Show => {
            let config = load_config()?;
            print_json(&config);
        }
        ConfigSubCmd::Current => {
            // Canonical-first: when any canonical source exists, show the
            // effective inspection (plan 46). Otherwise keep the legacy
            // endpoint-mode output so v0.1.x usage is unchanged. A
            // present-but-broken canonical file fails closed (no silent
            // fallback to legacy).
            let cwd = cwd();
            match load_cli_config(explicit_config, &cwd) {
                Ok(files) if !files.sources.is_empty() => {
                    print_effective_current(
                        &files.effective,
                        &files.sources,
                        &files.write_target,
                        explicit_context,
                    )?;
                    return Ok(());
                }
                Ok(_) => {}
                Err(error) => {
                    if explicit_config.is_some() || has_canonical_candidate(&cwd) {
                        return Err(error);
                    }
                }
            }
            let config = load_config()?;
            print_current_config(&config);
        }
        ConfigSubCmd::Set {
            mode,
            backend_url,
            api_url,
        } => {
            let mut config = load_config()?;
            let previous_mode = config.mode.clone();
            if let Some(m) = mode {
                config.mode = match m.as_str() {
                    "core" => EndpointMode::Core,
                    "backend" => EndpointMode::Backend,
                    "api" => EndpointMode::Api,
                    _ => anyhow::bail!("Invalid mode: {m}. Use core, backend, or api"),
                };
            }
            if let Some(u) = backend_url {
                validate_server_endpoint_url(&u, "Backend endpoint")?;
                config.backend_url = u;
            }
            if let Some(u) = api_url {
                validate_server_endpoint_url(&u, "API endpoint")?;
                config.api_url = u;
            }
            validate_active_remote_endpoint(&config)?;
            print_mode_transition_notice(&previous_mode, &config.mode, &config);
            let path = save_config(&config)?;
            print_json(&serde_json::json!({
                "saved": true,
                "path": path.to_string_lossy(),
                "config": config,
            }));
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
                println!("  {}", resolved.space_uid);
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

fn print_current_config(config: &crate::config::EndpointConfig) {
    match config.mode {
        EndpointMode::Core => {
            println!("Current endpoint mode: core");
            println!("Topology: local filesystem via ugoite-core.");
            println!(
                "Best when: you are working directly with a local checkout or local spaces/ directory."
            );
            println!("Why it stays the default: it is the shortest local-first path and does not require a running server.");
            println!("Future commands read and write your local workspace directly.");
            println!("To switch to a server-backed mode:");
            println!("  ugoite config set --mode backend --backend-url http://localhost:8000");
        }
        EndpointMode::Backend => {
            println!("Current endpoint mode: backend");
            println!("Topology: direct backend server at {}", config.backend_url);
            println!("Best when: you want the CLI to talk to a backend server directly.");
            println!("Trade-off: future commands use the server's storage and auth behavior instead of your local filesystem.");
            if let Some(warning) =
                endpoint_transport_warning(&config.backend_url, "Backend endpoint")
            {
                println!("Warning: {warning}");
            }
            println!("Future commands use the server instead of your local filesystem.");
            println!("To return to local-first mode:");
            println!("  ugoite config set --mode core");
        }
        EndpointMode::Api => {
            println!("Current endpoint mode: api");
            println!("Topology: API endpoint at {}", config.api_url);
            println!(
                "Best when: you want the CLI to use the same proxied /api surface as the frontend."
            );
            println!("Trade-off: future commands follow the frontend-facing API path instead of direct local filesystem access.");
            if let Some(warning) = endpoint_transport_warning(&config.api_url, "API endpoint") {
                println!("Warning: {warning}");
            }
            println!("Future commands use the remote API instead of your local filesystem.");
            println!("To return to local-first mode:");
            println!("  ugoite config set --mode core");
        }
    }
}

fn print_mode_transition_notice(
    previous_mode: &EndpointMode,
    next_mode: &EndpointMode,
    config: &crate::config::EndpointConfig,
) {
    if previous_mode == next_mode {
        return;
    }

    match (previous_mode, next_mode) {
        (EndpointMode::Core, EndpointMode::Backend) => {
            eprintln!(
                "Warning: switching from core mode to backend mode. Future commands will use {} instead of your local filesystem.",
                config.backend_url
            );
            eprintln!("To return to local-first mode: ugoite config set --mode core");
        }
        (EndpointMode::Core, EndpointMode::Api) => {
            eprintln!(
                "Warning: switching from core mode to api mode. Future commands will use {} instead of your local filesystem.",
                config.api_url
            );
            eprintln!("To return to local-first mode: ugoite config set --mode core");
        }
        (_, EndpointMode::Core) => {
            eprintln!("Switched back to core mode. Future commands will use your local filesystem directly.");
        }
        (_, EndpointMode::Backend) => {
            eprintln!(
                "Switched to backend mode. Future commands will use {}.",
                config.backend_url
            );
            eprintln!("To return to local-first mode: ugoite config set --mode core");
        }
        (_, EndpointMode::Api) => {
            eprintln!(
                "Switched to api mode. Future commands will use {}.",
                config.api_url
            );
            eprintln!("To return to local-first mode: ugoite config set --mode core");
        }
    }
}
