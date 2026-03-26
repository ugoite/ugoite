use crate::config::{base_url, load_config, print_json};
use crate::http;
use anyhow::{anyhow, Result};
use clap::{Args, Subcommand};
use std::io::{self, Write};

#[derive(Args)]
pub struct AuthCmd {
    #[command(subcommand)]
    pub sub: AuthSubCmd,
}

#[derive(Subcommand)]
pub enum AuthSubCmd {
    /// Show auth setup (env vars)
    Profile,
    /// Authenticate via local backend/API login flow and print export shell commands
    Login {
        #[arg(long)]
        username: Option<String>,
        #[arg(long)]
        totp_code: Option<String>,
        #[arg(long, default_value_t = false)]
        mock_oauth: bool,
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
            username,
            totp_code,
            mock_oauth,
        } => {
            let config = load_config();
            let base = base_url(&config)
                .unwrap_or_else(|| config.backend_url.trim_end_matches('/').to_string());

            let result = if mock_oauth {
                http::http_post_with_dev_auth_proxy(
                    &format!("{base}/auth/mock-oauth"),
                    &serde_json::json!({}),
                )
                .await?
            } else {
                let resolved_username = prompt_non_empty_value("Username", username)?;
                let resolved_totp_code = prompt_totp_code(totp_code)?;
                http::http_post_with_dev_auth_proxy(
                    &format!("{base}/auth/login"),
                    &serde_json::json!({
                        "username": resolved_username,
                        "totp_code": resolved_totp_code,
                    }),
                )
                .await?
            };

            if let Some(token) = result.get("bearer_token").and_then(|value| value.as_str()) {
                println!("export UGOITE_AUTH_BEARER_TOKEN={token}");
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

fn prompt_value(label: &str, provided: Option<String>) -> Result<String> {
    if let Some(value) = provided {
        return Ok(value.trim().to_string());
    }

    print!("{label}: ");
    io::stdout().flush()?;
    let mut buffer = String::new();
    io::stdin().read_line(&mut buffer)?;
    Ok(buffer.trim().to_string())
}

fn prompt_non_empty_value(label: &str, provided: Option<String>) -> Result<String> {
    let value = prompt_value(label, provided)?;
    if value.is_empty() {
        return Err(anyhow!("{label} must not be empty."));
    }
    Ok(value)
}

fn prompt_totp_code(provided: Option<String>) -> Result<String> {
    let value = prompt_non_empty_value("2FA code", provided)?;
    if value.len() != 6 || !value.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(anyhow!("2FA code must be exactly 6 digits."));
    }
    Ok(value)
}
