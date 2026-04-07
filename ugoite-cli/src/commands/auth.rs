use crate::config::{load_config, print_json, validated_base_url, EndpointConfig, EndpointMode};
use crate::http;
use anyhow::{anyhow, Result};
use clap::{Args, Subcommand, ValueEnum};
use std::io::{self, Write};

#[derive(Args)]
pub struct AuthCmd {
    #[command(subcommand)]
    pub sub: AuthSubCmd,
}

#[derive(ValueEnum, Clone, Copy, Debug, Default, Eq, PartialEq)]
enum AuthShell {
    #[default]
    #[value(name = "sh", alias = "posix")]
    Sh,
    #[value(name = "bash")]
    Bash,
    #[value(name = "zsh")]
    Zsh,
    #[value(name = "fish")]
    Fish,
    #[value(name = "powershell", alias = "pwsh")]
    PowerShell,
}

impl AuthShell {
    fn cli_name(self) -> &'static str {
        match self {
            Self::Sh => "sh",
            Self::Bash => "bash",
            Self::Zsh => "zsh",
            Self::Fish => "fish",
            Self::PowerShell => "powershell",
        }
    }
}

#[derive(Args, Clone, Copy, Debug)]
pub struct AuthShellArgs {
    #[arg(
        long,
        value_enum,
        default_value_t = AuthShell::Sh,
        help = "Shell syntax for printed environment commands",
        long_help = "Shell syntax for printed environment commands. Defaults to `sh` for POSIX-compatible `export` and `unset` output. Use `fish` or `powershell` when you want shell-native syntax."
    )]
    shell: AuthShell,
}

#[derive(Subcommand)]
pub enum AuthSubCmd {
    /// Show auth mode, credential status, and next step
    Profile,
    /// Authenticate via local backend/API passkey + 2FA login and print shell-ready env commands.
    ///
    /// Prerequisite: configure backend or api mode first:
    ///   ugoite config set --mode backend --backend-url http://localhost:8000
    ///
    /// Apply the printed export in a POSIX-compatible shell with:
    ///   eval "$(ugoite auth login --username USER --totp-code CODE)"
    #[command(
        long_about = "Authenticate via backend/API passkey + 2FA login and print shell-ready environment commands.\n\nPrerequisite: configure backend or api mode first:\n  ugoite config set --mode backend --backend-url http://localhost:8000\n\nWhen local development auth uses `passkey-totp`, also export UGOITE_DEV_PASSKEY_CONTEXT before logging in.\nDirect loopback backend mode does not require UGOITE_DEV_AUTH_PROXY_TOKEN for `--mock-oauth`, but proxied/container-boundary flows do.\n\nExamples:\n  # Login with username and TOTP code (POSIX export syntax by default)\n  ugoite auth login --username alice --totp-code 123456\n\n  # Apply the escaped token in one step (POSIX shells)\n  eval \"$(ugoite auth login --username alice --totp-code 123456)\"\n\n  # Apply the token in fish\n  ugoite auth login --shell fish --username alice --totp-code 123456 | source\n\n  # Apply the token in PowerShell\n  ugoite auth login --shell powershell --username alice --totp-code 123456 | Invoke-Expression\n\n  # Interactive mode (prompts for username and TOTP)\n  ugoite auth login\n\n  # Development: mock OAuth flow in PowerShell\n  ugoite auth login --shell powershell --mock-oauth | Invoke-Expression"
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
            help = "Use mock OAuth flow (development only; proxied/container flows require UGOITE_DEV_AUTH_PROXY_TOKEN)"
        )]
        mock_oauth: bool,
        #[command(flatten)]
        output: AuthShellArgs,
    },
    /// Print shell-ready clear commands for auth tokens
    TokenClear {
        #[command(flatten)]
        output: AuthShellArgs,
    },
    /// Clear auth tokens (alias for token-clear)
    Logout {
        #[command(flatten)]
        output: AuthShellArgs,
    },
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
            output,
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
                .await?
            };

            if let Some(token) = result.get("bearer_token").and_then(|value| value.as_str()) {
                println!(
                    "{}",
                    auth_env_set_command(output.shell, "UGOITE_AUTH_BEARER_TOKEN", token)
                );
                eprintln!("{}", login_shell_guidance(output.shell, mock_oauth));
            }
        }
        AuthSubCmd::TokenClear { output } | AuthSubCmd::Logout { output } => {
            println!(
                "{}",
                auth_env_unset_command(output.shell, "UGOITE_AUTH_BEARER_TOKEN")
            );
            println!(
                "{}",
                auth_env_unset_command(output.shell, "UGOITE_AUTH_API_KEY")
            );
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
                "Run `ugoite auth login` for a bearer token (use `--shell fish` or `--shell powershell` when you want shell-native env output), or export `UGOITE_AUTH_API_KEY` before using server-backed commands.".to_string()
            } else {
                "Continue with server-backed commands, or run `eval \"$(ugoite auth token-clear)\"` in POSIX shells, `ugoite auth token-clear --shell fish | source` in fish, or `ugoite auth token-clear --shell powershell | Invoke-Expression` in PowerShell to apply credential unsets in your current shell.".to_string()
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

fn fish_shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }

    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for ch in value.chars() {
        match ch {
            '\'' => quoted.push_str("\\'"),
            '\\' => quoted.push_str("\\\\"),
            _ => quoted.push(ch),
        }
    }
    quoted.push('\'');
    quoted
}

fn powershell_shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn auth_env_set_command(shell: AuthShell, key: &str, value: &str) -> String {
    match shell {
        AuthShell::Sh | AuthShell::Bash | AuthShell::Zsh => {
            format!("export {key}={}", posix_shell_quote(value))
        }
        AuthShell::Fish => format!("set -gx {key} {}", fish_shell_quote(value)),
        AuthShell::PowerShell => format!("$env:{key} = {}", powershell_shell_quote(value)),
    }
}

fn auth_env_unset_command(shell: AuthShell, key: &str) -> String {
    match shell {
        AuthShell::Sh | AuthShell::Bash | AuthShell::Zsh => format!("unset {key}"),
        AuthShell::Fish => format!("set -e {key}"),
        AuthShell::PowerShell => {
            format!("Remove-Item Env:{key} -ErrorAction SilentlyContinue")
        }
    }
}

fn auth_login_command_example(shell: AuthShell, mock_oauth: bool) -> String {
    let mut command = String::from("ugoite auth login");
    if shell != AuthShell::Sh {
        command.push_str(" --shell ");
        command.push_str(shell.cli_name());
    }
    if mock_oauth {
        command.push_str(" --mock-oauth");
    } else {
        command.push_str(" --username USER --totp-code CODE");
    }
    command
}

fn login_shell_guidance(shell: AuthShell, mock_oauth: bool) -> String {
    let command = auth_login_command_example(shell, mock_oauth);
    match shell {
        AuthShell::Sh | AuthShell::Bash | AuthShell::Zsh => format!(
            "# Output uses POSIX shell quoting.\n# To apply: eval \"$({command})\"\n# Or copy the export line above into your shell."
        ),
        AuthShell::Fish => format!(
            "# To apply in fish: {command} | source\n# Or copy the `set` line above into your shell."
        ),
        AuthShell::PowerShell => format!(
            "# To apply in PowerShell: {command} | Invoke-Expression\n# Or copy the `$env:` line above into your shell."
        ),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_shell_helpers_req_ops_015_cover_shell_variants() {
        assert_eq!(AuthShell::Sh.cli_name(), "sh");
        assert_eq!(AuthShell::Bash.cli_name(), "bash");
        assert_eq!(AuthShell::Zsh.cli_name(), "zsh");

        assert_eq!(fish_shell_quote(""), "''");
        assert_eq!(fish_shell_quote(r"C:\tmp\ugoite"), r"'C:\\tmp\\ugoite'");

        assert_eq!(
            auth_login_command_example(AuthShell::Bash, true),
            "ugoite auth login --shell bash --mock-oauth",
        );
        assert_eq!(
            auth_login_command_example(AuthShell::Zsh, false),
            "ugoite auth login --shell zsh --username USER --totp-code CODE",
        );
    }
}
