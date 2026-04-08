use crate::config::{load_config, print_json, validated_base_url, EndpointConfig, EndpointMode};
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
    /// Show auth mode, credential status, and next step
    Profile,
    /// Authenticate via local backend/API passkey + 2FA login and print POSIX-shell-safe export commands.
    ///
    /// Prerequisite: configure backend or api mode first:
    ///   ugoite config set --mode backend --backend-url http://localhost:8000
    ///
    /// Apply the printed export in a POSIX-compatible shell with:
    ///   eval "$(ugoite auth login --username USER --totp-code CODE)"
    #[command(
        long_about = "Authenticate via backend/API passkey + 2FA login and print POSIX-shell-safe export commands.\n\nPrerequisite: configure backend or api mode first:\n  ugoite config set --mode backend --backend-url http://localhost:8000\n\nWhen local development auth uses `passkey-totp`, the CLI first reads UGOITE_DEV_PASSKEY_CONTEXT from the current shell and then falls back to the cached local dev auth file prepared by `eval \"$(bash scripts/dev-auth-env.sh)\"` for loopback backend/API endpoints.\nDirect loopback backend mode does not require UGOITE_DEV_AUTH_PROXY_TOKEN for `--mock-oauth`, but proxied/container flows require UGOITE_DEV_AUTH_PROXY_TOKEN.\n\nExamples:\n  # Login with username and TOTP code\n  ugoite auth login --username alice --totp-code 123456\n\n  # Apply the escaped token in one step (POSIX shells)\n  eval \"$(ugoite auth login --username alice --totp-code 123456)\"\n\n  # Interactive mode (prompts for username and TOTP)\n  ugoite auth login\n\n  # Development: mock OAuth flow\n  eval \"$(ugoite auth login --mock-oauth)\""
    )]
    Login {
        #[arg(
            long,
            help = "Username to authenticate with (prompted interactively if omitted)"
        )]
        username: Option<String>,
        #[arg(
            long,
            help = "6-digit TOTP code from your authenticator app (prompted interactively if omitted)"
        )]
        totp_code: Option<String>,
        #[arg(
            long,
            default_value_t = false,
            help = "Use mock OAuth flow (development only; direct loopback backend mode does not require UGOITE_DEV_AUTH_PROXY_TOKEN, but proxied/container flows do)"
        )]
        mock_oauth: bool,
    },
    /// Print unset commands for auth tokens
    TokenClear,
    /// Clear auth tokens (alias for token-clear)
    Logout,
    /// Print authentication capabilities
    Overview,
}

pub async fn run(cmd: AuthCmd) -> Result<()> {
    match cmd.sub {
        AuthSubCmd::Profile => {
            let config = load_config();
            print_json(&auth_profile_snapshot(&config));
        }
        AuthSubCmd::Login {
            username,
            totp_code,
            mock_oauth,
        } => {
            let config = load_config();
            if config.mode == EndpointMode::Core {
                return Err(anyhow!(
                    "auth login requires backend or api mode.\nRun: ugoite config set --mode backend --backend-url http://localhost:8000"
                ));
            }
            let base =
                validated_base_url(&config)?.expect("backend/api mode always has a base URL");

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
                .await
                .map_err(add_passkey_context_recovery_hint)?
            };

            if let Some(token) = result.get("bearer_token").and_then(|value| value.as_str()) {
                let apply_args = if mock_oauth {
                    " --mock-oauth"
                } else {
                    " --username USER --totp-code CODE"
                };
                let quoted_token = posix_shell_quote(token);
                println!("export UGOITE_AUTH_BEARER_TOKEN={quoted_token}");
                eprintln!(
                    "# Output uses POSIX shell quoting.\n# To apply: eval \"$(ugoite auth login{apply_args})\"\n# Or copy the export line above into your shell."
                );
            }
        }
        AuthSubCmd::TokenClear | AuthSubCmd::Logout => {
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

fn add_passkey_context_recovery_hint(error: anyhow::Error) -> anyhow::Error {
    let message = error.to_string();
    if !message.contains("Passkey-bound local context is missing or invalid.") {
        return error;
    }

    let cached_path = http::dev_auth_file_path()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "~/.ugoite/dev-auth.json".to_string());
    anyhow!(
        "{message}\nHint: passkey-totp CLI login requires UGOITE_DEV_PASSKEY_CONTEXT. The CLI reuses the cached local dev auth file at {cached_path} when it exists. If that file is missing or stale, rerun `eval \"$(bash scripts/dev-auth-env.sh)\"` from the repo root or export UGOITE_DEV_PASSKEY_CONTEXT in this shell before retrying `ugoite auth login`."
    )
}

fn mask_token(t: &str) -> String {
    if t.len() > 8 {
        format!("{}...", &t[..4])
    } else {
        "****".to_string()
    }
}

fn auth_profile_snapshot(config: &EndpointConfig) -> serde_json::Value {
    let bearer = non_empty_env("UGOITE_AUTH_BEARER_TOKEN");
    let api_key = non_empty_env("UGOITE_AUTH_API_KEY");
    let credential_state = credential_state_label(bearer.as_deref(), api_key.as_deref());

    let (topology, endpoint_url) = match &config.mode {
        EndpointMode::Core => ("local filesystem via ugoite-core".to_string(), None),
        EndpointMode::Backend => (
            format!("direct backend server at {}", config.backend_url),
            Some(config.backend_url.as_str()),
        ),
        EndpointMode::Api => (
            format!("API endpoint at {}", config.api_url),
            Some(config.api_url.as_str()),
        ),
    };

    serde_json::json!({
        "endpoint_mode": endpoint_mode_label(&config.mode),
        "topology": topology,
        "endpoint_url": endpoint_url,
        "backend_auth_required": config.mode != EndpointMode::Core,
        "credential_state": credential_state,
        "status": auth_profile_status(&config.mode, credential_state),
        "next_action": auth_profile_next_action(config, credential_state),
        "UGOITE_AUTH_BEARER_TOKEN": bearer.as_deref().map(mask_token),
        "UGOITE_AUTH_API_KEY": api_key.as_deref().map(mask_token),
    })
}

fn endpoint_mode_label(mode: &EndpointMode) -> &'static str {
    match mode {
        EndpointMode::Core => "core",
        EndpointMode::Backend => "backend",
        EndpointMode::Api => "api",
    }
}

fn credential_state_label(bearer: Option<&str>, api_key: Option<&str>) -> &'static str {
    match (bearer, api_key) {
        (Some(_), Some(_)) => "bearer_token_and_api_key",
        (Some(_), None) => "bearer_token",
        (None, Some(_)) => "api_key",
        (None, None) => "none",
    }
}

fn auth_profile_status(mode: &EndpointMode, credential_state: &str) -> String {
    match mode {
        EndpointMode::Core => {
            if credential_state == "none" {
                "Core mode does not require backend authentication.".to_string()
            } else {
                "Core mode does not require backend authentication. Token env vars are present but only matter after switching to backend or api mode.".to_string()
            }
        }
        EndpointMode::Backend => server_auth_status("Backend", credential_state),
        EndpointMode::Api => server_auth_status("API", credential_state),
    }
}

fn server_auth_status(mode_label: &str, credential_state: &str) -> String {
    if credential_state == "none" {
        format!("{mode_label} mode is configured, but no bearer token or API key is currently set.")
    } else {
        format!("{mode_label} mode is configured and a server credential is available.")
    }
}

fn auth_profile_next_action(config: &EndpointConfig, credential_state: &str) -> String {
    match &config.mode {
        EndpointMode::Core => format!(
            "Run CLI commands directly against your local workspace, or switch to backend mode with `ugoite config set --mode backend --backend-url {}`.",
            config.backend_url
        ),
        EndpointMode::Backend | EndpointMode::Api => {
            if credential_state == "none" {
                "Run `ugoite auth login` for a bearer token, or export `UGOITE_AUTH_API_KEY` before using server-backed commands.".to_string()
            } else {
                "Continue with server-backed commands, or run `eval \"$(ugoite auth token-clear)\"` to print and apply credential unsets in your current shell.".to_string()
            }
        }
    }
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().and_then(|value| {
        if value.trim().is_empty() {
            None
        } else {
            Some(value)
        }
    })
}

fn posix_shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }

    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
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
