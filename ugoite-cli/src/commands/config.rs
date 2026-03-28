use crate::config::{load_config, print_json, save_config, EndpointMode};
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
    /// Save endpoint config (mode, backend URL, API URL)
    #[command(long_about = "Save endpoint configuration.\n\nThree modes are supported:\n  core     - Direct local filesystem access (no backend needed)\n  backend  - Connect to a running ugoite backend server\n  api      - Connect to a remote API endpoint\n\nExamples:\n  # Core mode (default, uses local filesystem)\n  ugoite config set --mode core\n\n  # Backend mode (connect to local backend)\n  ugoite config set --mode backend --backend-url http://localhost:8000\n\n  # API mode (connect to remote API)\n  ugoite config set --mode api --api-url https://api.example.com\n\n  # Update only the backend URL (keep current mode)\n  ugoite config set --backend-url http://localhost:9000")]
    Set {
        #[arg(long, help = "Endpoint mode: core (local filesystem), backend (ugoite server), or api (remote API)")]
        mode: Option<String>,
        #[arg(long, help = "Backend server URL (used in backend mode, e.g. http://localhost:8000)")]
        backend_url: Option<String>,
        #[arg(long, help = "API endpoint URL (used in api mode)")]
        api_url: Option<String>,
    },
}

pub async fn run(cmd: ConfigCmd) -> Result<()> {
    match cmd.sub {
        ConfigSubCmd::Show => {
            let config = load_config();
            print_json(&config);
        }
        ConfigSubCmd::Set {
            mode,
            backend_url,
            api_url,
        } => {
            let mut config = load_config();
            if let Some(m) = mode {
                config.mode = match m.as_str() {
                    "core" => EndpointMode::Core,
                    "backend" => EndpointMode::Backend,
                    "api" => EndpointMode::Api,
                    _ => anyhow::bail!("Invalid mode: {m}. Use core, backend, or api"),
                };
            }
            if let Some(u) = backend_url {
                config.backend_url = u;
            }
            if let Some(u) = api_url {
                config.api_url = u;
            }
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
