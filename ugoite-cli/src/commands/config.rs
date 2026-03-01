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
    /// Save endpoint config
    Set {
        #[arg(long)]
        mode: Option<String>,
        #[arg(long)]
        backend_url: Option<String>,
        #[arg(long)]
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
