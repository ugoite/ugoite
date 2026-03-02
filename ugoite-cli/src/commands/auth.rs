use crate::config::print_json;
use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Args)]
pub struct AuthCmd {
    #[command(subcommand)]
    pub sub: AuthSubCmd,
}

#[derive(Subcommand)]
pub enum AuthSubCmd {
    /// Show auth setup (env vars)
    Profile,
    /// Print export shell commands for auth
    Login {
        #[arg(long)]
        bearer_token: Option<String>,
        #[arg(long)]
        api_key: Option<String>,
    },
    /// Print unset commands for auth tokens
    TokenClear,
    /// Print authentication capabilities
    Overview,
}

pub async fn run(cmd: AuthCmd) -> Result<()> {
    match cmd.sub {
        AuthSubCmd::Profile => {
            let bearer = std::env::var("UGOITE_AUTH_BEARER_TOKEN").ok();
            let api_key = std::env::var("UGOITE_AUTH_API_KEY").ok();
            print_json(&serde_json::json!({
                "UGOITE_AUTH_BEARER_TOKEN": bearer.as_deref().map(mask_token),
                "UGOITE_AUTH_API_KEY": api_key.as_deref().map(mask_token),
            }));
        }
        AuthSubCmd::Login {
            bearer_token,
            api_key,
        } => {
            if bearer_token.is_some() {
                println!("export UGOITE_AUTH_BEARER_TOKEN=<set-token-here>");
            }
            if api_key.is_some() {
                println!("export UGOITE_AUTH_API_KEY=<set-key-here>");
            }
        }
        AuthSubCmd::TokenClear => {
            println!("unset UGOITE_AUTH_BEARER_TOKEN");
            println!("unset UGOITE_AUTH_API_KEY");
        }
        AuthSubCmd::Overview => {
            let caps = ugoite_core::auth::auth_capabilities_snapshot(None, None, None, None, None);
            print_json(&caps);
        }
    }
    Ok(())
}

fn mask_token(t: &str) -> String {
    if t.len() > 8 {
        format!("{}...", &t[..4])
    } else {
        "****".to_string()
    }
}
