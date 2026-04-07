use crate::config::{
    endpoint_transport_warning, load_config, print_json, save_config,
    validate_active_remote_endpoint, validate_remote_endpoint_url, EndpointMode,
};
use anyhow::Result;
use clap::{Args, Subcommand};

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
}

pub async fn run(cmd: ConfigCmd) -> Result<()> {
    match cmd.sub {
        ConfigSubCmd::Show => {
            let config = load_config();
            print_json(&config);
        }
        ConfigSubCmd::Current => {
            let config = load_config();
            validate_active_remote_endpoint(&config)?;
            print_current_config(&config);
        }
        ConfigSubCmd::Set {
            mode,
            backend_url,
            api_url,
        } => {
            let mut config = load_config();
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
                validate_remote_endpoint_url(&u, "--backend-url")?;
                config.backend_url = u;
            }
            if let Some(u) = api_url {
                validate_remote_endpoint_url(&u, "--api-url")?;
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
