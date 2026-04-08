use crate::config::{
    clear_auth_session, effective_api_key, effective_bearer_token, load_config, print_json,
    save_auth_session, validated_base_url, AuthSession, EndpointConfig, EndpointMode,
};
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

            let result = login_request(&base, username, totp_code, mock_oauth).await?;
            persist_login_session(&result, output.shell, mock_oauth)?;
        }
        AuthSubCmd::TokenClear { output } | AuthSubCmd::Logout { output } => {
            let cleared_saved_session = clear_auth_session()?;
            println!(
                "{}",
                auth_env_unset_command(output.shell, "UGOITE_AUTH_BEARER_TOKEN")
            );
            println!(
                "{}",
                auth_env_unset_command(output.shell, "UGOITE_AUTH_API_KEY")
            );
            if cleared_saved_session {
                eprintln!("# Cleared the saved CLI session.");
            }
        }
        AuthSubCmd::Overview => {
            let caps = ugoite_core::auth::auth_capabilities_snapshot(None, None, None, None, None);
            print_json(&caps);
        }
    }
    Ok(())
}

async fn login_request(
    base: &str,
    username: Option<String>,
    totp_code: Option<String>,
    mock_oauth: bool,
) -> Result<serde_json::Value> {
    if mock_oauth {
        return http::http_post_with_dev_auth_proxy(
            &format!("{base}/auth/mock-oauth"),
            &serde_json::json!({}),
        )
        .await;
    }

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
}

fn persist_login_session(
    result: &serde_json::Value,
    shell: AuthShell,
    mock_oauth: bool,
) -> Result<()> {
    if let Some(token) = result.get("bearer_token").and_then(|value| value.as_str()) {
        let session_path = save_auth_session(&AuthSession {
            bearer_token: Some(token.to_string()),
        })?;
        println!(
            "{}",
            auth_env_set_command(shell, "UGOITE_AUTH_BEARER_TOKEN", token)
        );
        eprintln!(
            "# Saved CLI session to {} with owner-only permissions where supported.\n# Future `ugoite` commands will use it automatically.",
            session_path.display()
        );
        eprintln!("{}", login_shell_guidance(shell, mock_oauth));
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
    let bearer = effective_bearer_token();
    let api_key = effective_api_key();
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
                "Core mode does not require backend authentication. Saved or exported server credentials are present but only matter after switching to backend or api mode.".to_string()
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
                "Continue with server-backed commands, or run `ugoite auth token-clear` to clear any saved CLI session. In POSIX shells, `eval \"$(ugoite auth token-clear)\"` also applies the printed credential unsets to your current shell; in fish, use `ugoite auth token-clear --shell fish | source`; in PowerShell, use `ugoite auth token-clear --shell powershell | Invoke-Expression`.".to_string()
            }
        }
    }
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
    let shell_name = shell.cli_name();
    if shell_name != "sh" {
        command.push_str(" --shell ");
        command.push_str(shell_name);
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
    prompt_value_with(
        label,
        provided,
        || io::stdout().flush(),
        |buffer| io::stdin().read_line(buffer),
    )
}

fn prompt_value_with(
    label: &str,
    provided: Option<String>,
    flush_stdout: fn() -> io::Result<()>,
    read_line: fn(&mut String) -> io::Result<usize>,
) -> Result<String> {
    if let Some(value) = provided {
        return Ok(value.trim().to_string());
    }

    print!("{label}: ");
    flush_stdout()?;
    let mut buffer = String::new();
    read_line(&mut buffer)?;
    Ok(buffer.trim().to_string())
}

fn prompt_non_empty_value(label: &str, provided: Option<String>) -> Result<String> {
    prompt_non_empty_value_with(label, provided, prompt_value)
}

fn prompt_non_empty_value_with(
    label: &str,
    provided: Option<String>,
    prompt: fn(&str, Option<String>) -> Result<String>,
) -> Result<String> {
    let value = prompt(label, provided)?;
    if value.is_empty() {
        return Err(anyhow!("{label} must not be empty."));
    }
    Ok(value)
}

fn prompt_totp_code(provided: Option<String>) -> Result<String> {
    prompt_totp_code_with(provided, prompt_non_empty_value)
}

fn prompt_totp_code_with(
    provided: Option<String>,
    prompt_non_empty: fn(&str, Option<String>) -> Result<String>,
) -> Result<String> {
    let value = prompt_non_empty("2FA code", provided)?;
    if value.len() != 6 || !value.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(anyhow!("2FA code must be exactly 6 digits."));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{mpsc, Mutex, OnceLock};
    use std::thread;
    use std::time::Duration;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn clear_test_env() {
        for key in [
            "UGOITE_CLI_CONFIG_PATH",
            "UGOITE_AUTH_BEARER_TOKEN",
            "UGOITE_AUTH_API_KEY",
            "UGOITE_DEV_AUTH_PROXY_TOKEN",
            "UGOITE_DEV_PASSKEY_CONTEXT",
        ] {
            std::env::remove_var(key);
        }
    }

    fn assert_io_error_kind(error: &anyhow::Error, kind: std::io::ErrorKind) {
        assert!(error.chain().any(|cause| {
            cause
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io_error| io_error.kind() == kind)
        }));
    }

    fn spawn_recording_server(
        status_line: &'static str,
        body: &'static str,
    ) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            let mut content_length = 0_usize;

            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                request.push_str(&line);
                let lower = line.to_ascii_lowercase();
                if let Some(value) = lower.strip_prefix("content-length:") {
                    content_length = value.trim().parse().unwrap();
                }
                if line == "\r\n" {
                    break;
                }
            }

            let mut body_buffer = vec![0_u8; content_length];
            reader.read_exact(&mut body_buffer).unwrap();
            request.push_str(&String::from_utf8_lossy(&body_buffer));
            tx.send(request).unwrap();

            let response = format!(
                "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        });
        (format!("http://{}", addr), rx, handle)
    }

    #[test]
    fn test_auth_shell_helpers_req_ops_015_cover_shell_variants() {
        assert_eq!(AuthShell::Sh.cli_name(), "sh");
        assert_eq!(AuthShell::Bash.cli_name(), "bash");
        assert_eq!(AuthShell::Zsh.cli_name(), "zsh");

        assert_eq!(fish_shell_quote(""), "''");
        assert_eq!(fish_shell_quote("it's"), r"'it\'s'");
        assert_eq!(fish_shell_quote(r"C:\tmp\ugoite"), r"'C:\\tmp\\ugoite'");

        assert_eq!(
            auth_login_command_example(AuthShell::Bash, true),
            "ugoite auth login --shell bash --mock-oauth",
        );
        assert_eq!(
            auth_login_command_example(AuthShell::Zsh, false),
            "ugoite auth login --shell zsh --username USER --totp-code CODE",
        );

        assert_eq!(posix_shell_quote(""), "''");
        assert_eq!(posix_shell_quote("it's"), "'it'\\''s'");
        assert_eq!(powershell_shell_quote("value"), "'value'");
        assert_eq!(powershell_shell_quote("it's"), "'it''s'");
    }

    /// REQ-OPS-015: core mode login must fail before any network prompt or request.
    #[test]
    fn test_auth_run_req_ops_015_rejects_core_mode_login() {
        let _guard = env_lock().lock().unwrap();
        clear_test_env();

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("cli-endpoints.json");
        std::env::set_var("UGOITE_CLI_CONFIG_PATH", &config_path);
        crate::config::save_config(&EndpointConfig {
            mode: EndpointMode::Core,
            backend_url: "http://localhost:8000".to_string(),
            api_url: "http://localhost:3000/api".to_string(),
        })
        .unwrap();

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime
            .block_on(run(AuthCmd {
                sub: AuthSubCmd::Login {
                    username: Some("dev-alice".to_string()),
                    totp_code: Some("123456".to_string()),
                    mock_oauth: false,
                    output: AuthShellArgs {
                        shell: AuthShell::Sh,
                    },
                },
            }))
            .unwrap_err();

        clear_test_env();
        assert!(error
            .to_string()
            .contains("auth login requires backend or api mode"));
    }

    /// REQ-OPS-015: direct auth run coverage must surface invalid configured backend URLs.
    #[test]
    fn test_auth_run_req_ops_015_rejects_invalid_backend_url() {
        let _guard = env_lock().lock().unwrap();
        clear_test_env();

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("cli-endpoints.json");
        std::env::set_var("UGOITE_CLI_CONFIG_PATH", &config_path);
        crate::config::save_config(&EndpointConfig {
            mode: EndpointMode::Backend,
            backend_url: "http://example.com".to_string(),
            api_url: "http://localhost:3000/api".to_string(),
        })
        .unwrap();

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime
            .block_on(run(AuthCmd {
                sub: AuthSubCmd::Login {
                    username: Some("dev-alice".to_string()),
                    totp_code: Some("123456".to_string()),
                    mock_oauth: false,
                    output: AuthShellArgs {
                        shell: AuthShell::Sh,
                    },
                },
            }))
            .unwrap_err();

        clear_test_env();
        assert!(error
            .to_string()
            .contains("uses cleartext http:// for a non-loopback host"));
    }

    /// REQ-OPS-015: direct auth run coverage must surface provided login validation failures.
    #[test]
    fn test_auth_run_req_ops_015_rejects_blank_username_before_request() {
        let _guard = env_lock().lock().unwrap();
        clear_test_env();

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("cli-endpoints.json");
        std::env::set_var("UGOITE_CLI_CONFIG_PATH", &config_path);
        crate::config::save_config(&EndpointConfig {
            mode: EndpointMode::Backend,
            backend_url: "http://127.0.0.1:8000".to_string(),
            api_url: "http://localhost:3000/api".to_string(),
        })
        .unwrap();

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime
            .block_on(run(AuthCmd {
                sub: AuthSubCmd::Login {
                    username: Some("   ".to_string()),
                    totp_code: Some("123456".to_string()),
                    mock_oauth: false,
                    output: AuthShellArgs {
                        shell: AuthShell::Sh,
                    },
                },
            }))
            .unwrap_err();

        clear_test_env();
        assert!(error.to_string().contains("Username must not be empty"));
    }

    /// REQ-OPS-015: direct auth run coverage must exercise the overview subcommand path.
    #[test]
    fn test_auth_run_req_ops_015_overview_succeeds() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime
            .block_on(run(AuthCmd {
                sub: AuthSubCmd::Overview,
            }))
            .unwrap();
    }

    /// REQ-OPS-015: direct auth run coverage must exercise the profile subcommand path.
    #[test]
    fn test_auth_run_req_ops_015_profile_succeeds() {
        let _guard = env_lock().lock().unwrap();
        clear_test_env();

        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime
            .block_on(run(AuthCmd {
                sub: AuthSubCmd::Profile,
            }))
            .unwrap();
    }

    /// REQ-OPS-015: direct auth run coverage must exercise the mock OAuth loopback path.
    #[test]
    fn test_auth_run_req_ops_015_posts_mock_oauth_directly() {
        let _guard = env_lock().lock().unwrap();
        clear_test_env();

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("cli-endpoints.json");
        let (base, requests, handle) = spawn_recording_server(
            "HTTP/1.1 200 OK",
            r#"{"bearer_token":"issued-token","user_id":"dev-alice","expires_at":1900000000}"#,
        );

        std::env::set_var("UGOITE_CLI_CONFIG_PATH", &config_path);
        crate::config::save_config(&EndpointConfig {
            mode: EndpointMode::Backend,
            backend_url: base,
            api_url: "http://localhost:3000/api".to_string(),
        })
        .unwrap();

        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime
            .block_on(run(AuthCmd {
                sub: AuthSubCmd::Login {
                    username: None,
                    totp_code: None,
                    mock_oauth: true,
                    output: AuthShellArgs {
                        shell: AuthShell::Sh,
                    },
                },
            }))
            .unwrap();

        let request_text = requests.recv_timeout(Duration::from_secs(5)).unwrap();
        handle.join().unwrap();
        clear_test_env();
        assert!(request_text.starts_with("POST /auth/mock-oauth HTTP/1.1"));
    }

    /// REQ-OPS-015: direct auth run coverage must exercise the provided credential path.
    #[test]
    fn test_auth_run_req_ops_015_posts_totp_credentials_directly() {
        let _guard = env_lock().lock().unwrap();
        clear_test_env();

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("cli-endpoints.json");
        let (base, requests, handle) = spawn_recording_server(
            "HTTP/1.1 200 OK",
            r#"{"bearer_token":"issued-token","user_id":"dev-alice","expires_at":1900000000}"#,
        );

        std::env::set_var("UGOITE_CLI_CONFIG_PATH", &config_path);
        std::env::set_var("UGOITE_DEV_AUTH_PROXY_TOKEN", "proxy-secret");
        std::env::set_var("UGOITE_DEV_PASSKEY_CONTEXT", "passkey-context");
        crate::config::save_config(&EndpointConfig {
            mode: EndpointMode::Backend,
            backend_url: base,
            api_url: "http://localhost:3000/api".to_string(),
        })
        .unwrap();

        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime
            .block_on(run(AuthCmd {
                sub: AuthSubCmd::Login {
                    username: Some("dev-alice".to_string()),
                    totp_code: Some("123456".to_string()),
                    mock_oauth: false,
                    output: AuthShellArgs {
                        shell: AuthShell::Sh,
                    },
                },
            }))
            .unwrap();

        let request_text = requests.recv_timeout(Duration::from_secs(5)).unwrap();
        handle.join().unwrap();
        clear_test_env();
        assert!(request_text.starts_with("POST /auth/login HTTP/1.1"));
        assert!(request_text.contains("dev-alice"));
        assert!(request_text.contains("123456"));
    }

    /// REQ-OPS-015: direct auth run coverage must surface login session persistence failures.
    #[test]
    fn test_auth_run_req_ops_015_surfaces_session_write_failures() {
        let _guard = env_lock().lock().unwrap();
        clear_test_env();

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("cli-endpoints.json");
        let (base, requests, handle) = spawn_recording_server(
            "HTTP/1.1 200 OK",
            r#"{"bearer_token":"issued-token","user_id":"dev-alice","expires_at":1900000000}"#,
        );

        std::env::set_var("UGOITE_CLI_CONFIG_PATH", &config_path);
        crate::config::save_config(&EndpointConfig {
            mode: EndpointMode::Backend,
            backend_url: base,
            api_url: "http://localhost:3000/api".to_string(),
        })
        .unwrap();
        std::fs::create_dir(crate::config::auth_session_path()).unwrap();

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime
            .block_on(run(AuthCmd {
                sub: AuthSubCmd::Login {
                    username: None,
                    totp_code: None,
                    mock_oauth: true,
                    output: AuthShellArgs {
                        shell: AuthShell::Sh,
                    },
                },
            }))
            .unwrap_err();

        requests.recv_timeout(Duration::from_secs(5)).unwrap();
        handle.join().unwrap();
        clear_test_env();
        assert_io_error_kind(&error, std::io::ErrorKind::IsADirectory);
    }

    /// REQ-OPS-015: direct auth run coverage must surface token-clear session removal failures.
    #[test]
    fn test_auth_run_req_ops_015_surfaces_token_clear_session_remove_failures() {
        let _guard = env_lock().lock().unwrap();
        clear_test_env();

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("cli-endpoints.json");
        std::env::set_var("UGOITE_CLI_CONFIG_PATH", &config_path);
        std::fs::create_dir(crate::config::auth_session_path()).unwrap();

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime
            .block_on(run(AuthCmd {
                sub: AuthSubCmd::TokenClear {
                    output: AuthShellArgs {
                        shell: AuthShell::Sh,
                    },
                },
            }))
            .unwrap_err();

        clear_test_env();
        assert_io_error_kind(&error, std::io::ErrorKind::IsADirectory);
    }

    /// REQ-OPS-015: direct auth run coverage must exercise token-clear when no saved session exists.
    #[test]
    fn test_auth_run_req_ops_015_token_clear_without_saved_session_succeeds() {
        let _guard = env_lock().lock().unwrap();
        clear_test_env();

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("cli-endpoints.json");
        std::env::set_var("UGOITE_CLI_CONFIG_PATH", &config_path);

        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime
            .block_on(run(AuthCmd {
                sub: AuthSubCmd::TokenClear {
                    output: AuthShellArgs {
                        shell: AuthShell::Sh,
                    },
                },
            }))
            .unwrap();

        clear_test_env();
    }

    /// REQ-OPS-015: direct auth run coverage must exercise the logout alias success path.
    #[test]
    fn test_auth_run_req_ops_015_logout_clears_saved_session_successfully() {
        let _guard = env_lock().lock().unwrap();
        clear_test_env();

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("cli-endpoints.json");
        std::env::set_var("UGOITE_CLI_CONFIG_PATH", &config_path);
        save_auth_session(&AuthSession {
            bearer_token: Some("issued-token".to_string()),
        })
        .unwrap();

        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime
            .block_on(run(AuthCmd {
                sub: AuthSubCmd::Logout {
                    output: AuthShellArgs {
                        shell: AuthShell::Sh,
                    },
                },
            }))
            .unwrap();

        assert!(!crate::config::auth_session_path().exists());
        clear_test_env();
    }

    /// REQ-OPS-015: helper coverage must validate provided auth prompt values without stdin.
    #[test]
    fn test_prompt_helpers_req_ops_015_validate_provided_values() {
        assert_eq!(
            prompt_value("Username", Some("  dev-alice  ".to_string())).unwrap(),
            "dev-alice",
        );
        assert!(prompt_non_empty_value("Username", Some("   ".to_string()))
            .unwrap_err()
            .to_string()
            .contains("Username must not be empty"));
        assert_eq!(
            prompt_totp_code(Some("123456".to_string())).unwrap(),
            "123456",
        );
        assert!(prompt_totp_code(Some("abc123".to_string()))
            .unwrap_err()
            .to_string()
            .contains("2FA code must be exactly 6 digits"));
    }

    /// REQ-OPS-015: helper coverage must surface prompt reader failures without touching stdio globals.
    #[test]
    fn test_prompt_helpers_req_ops_015_surface_prompt_failures() {
        let username_error = prompt_non_empty_value_with("Username", None, |_label, _provided| {
            Err(anyhow!("prompt unavailable"))
        })
        .unwrap_err();
        assert!(username_error.to_string().contains("prompt unavailable"));

        let totp_error =
            prompt_totp_code_with(None, |_label, _provided| Err(anyhow!("prompt unavailable")))
                .unwrap_err();
        assert!(totp_error.to_string().contains("prompt unavailable"));
    }

    /// REQ-OPS-015: helper coverage must surface prompt stdio failures without mutating global stdio.
    #[test]
    fn test_prompt_helpers_req_ops_015_surface_prompt_stdio_failures() {
        let flush_error = prompt_value_with(
            "Username",
            None,
            || Err(io::Error::other("flush failed")),
            |_buffer| Ok(0),
        )
        .unwrap_err();
        assert!(flush_error.to_string().contains("flush failed"));

        let read_error = prompt_value_with(
            "Username",
            None,
            || Ok(()),
            |_buffer| Err(io::Error::other("read failed")),
        )
        .unwrap_err();
        assert!(read_error.to_string().contains("read failed"));
    }

    /// REQ-OPS-015: helper coverage must still trim prompt input when injected stdio succeeds.
    #[test]
    fn test_prompt_helpers_req_ops_015_prompt_value_with_trims_injected_input() {
        let value = prompt_value_with(
            "Username",
            None,
            || Ok(()),
            |buffer| {
                buffer.push_str("  dev-alice  \n");
                Ok(buffer.len())
            },
        )
        .unwrap();
        assert_eq!(value, "dev-alice");
    }

    /// REQ-OPS-015: direct login coverage must surface provided username validation failures.
    #[test]
    fn test_login_request_req_ops_015_rejects_blank_username_directly() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime
            .block_on(login_request(
                "http://127.0.0.1:65535",
                Some("   ".to_string()),
                Some("123456".to_string()),
                false,
            ))
            .unwrap_err();

        assert!(error.to_string().contains("Username must not be empty"));
    }

    /// REQ-OPS-015: direct login coverage must surface provided 2FA validation failures.
    #[test]
    fn test_login_request_req_ops_015_rejects_invalid_totp_directly() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime
            .block_on(login_request(
                "http://127.0.0.1:65535",
                Some("dev-alice".to_string()),
                Some("12ab56".to_string()),
                false,
            ))
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("2FA code must be exactly 6 digits"));
    }

    /// REQ-OPS-015: auth login must surface session persistence I/O failures.
    #[test]
    fn test_persist_login_session_req_ops_015_surfaces_session_write_failures() {
        let _guard = env_lock().lock().unwrap();
        clear_test_env();

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("cli-endpoints.json");
        std::env::set_var("UGOITE_CLI_CONFIG_PATH", &config_path);
        std::fs::create_dir(crate::config::auth_session_path()).unwrap();

        let error = persist_login_session(
            &serde_json::json!({
                "bearer_token": "issued-token",
            }),
            AuthShell::Sh,
            false,
        )
        .unwrap_err();

        clear_test_env();
        assert_io_error_kind(&error, std::io::ErrorKind::IsADirectory);
    }

    /// REQ-OPS-015: core-mode profile snapshots ignore blank env vars and stay local-first.
    #[test]
    fn test_auth_profile_snapshot_req_ops_015_core_mode_ignores_blank_env_vars() {
        let _guard = env_lock().lock().unwrap();
        clear_test_env();
        std::env::set_var("UGOITE_AUTH_BEARER_TOKEN", "   ");
        std::env::set_var("UGOITE_AUTH_API_KEY", "   ");

        let payload = auth_profile_snapshot(&EndpointConfig {
            mode: EndpointMode::Core,
            backend_url: "http://localhost:8000".to_string(),
            api_url: "http://localhost:3000/api".to_string(),
        });

        clear_test_env();
        assert_eq!(payload["endpoint_mode"], "core");
        assert_eq!(payload["topology"], "local filesystem via ugoite-core");
        assert!(payload["endpoint_url"].is_null());
        assert_eq!(payload["credential_state"], "none");
        assert_eq!(
            payload["next_action"],
            "Run CLI commands directly against your local workspace, or switch to backend mode with `ugoite config set --mode backend --backend-url http://localhost:8000`.",
        );
    }

    /// REQ-OPS-015: backend/api profile snapshots must report remote credential states directly.
    #[test]
    fn test_auth_profile_snapshot_req_ops_015_reports_remote_modes_and_credentials() {
        let _guard = env_lock().lock().unwrap();
        clear_test_env();

        let backend_config = EndpointConfig {
            mode: EndpointMode::Backend,
            backend_url: "http://localhost:8000".to_string(),
            api_url: "http://localhost:3000/api".to_string(),
        };
        let api_config = EndpointConfig {
            mode: EndpointMode::Api,
            backend_url: "http://localhost:8000".to_string(),
            api_url: "http://localhost:3000/api".to_string(),
        };

        let backend_none = auth_profile_snapshot(&backend_config);
        assert_eq!(backend_none["endpoint_mode"], "backend");
        assert_eq!(backend_none["credential_state"], "none",);
        assert_eq!(
            backend_none["status"],
            "Backend mode is configured, but no bearer token or API key is currently set.",
        );
        assert!(backend_none["next_action"]
            .as_str()
            .is_some_and(|value| value.contains("ugoite auth login")));

        std::env::set_var("UGOITE_AUTH_BEARER_TOKEN", "issued-token");
        let backend_bearer = auth_profile_snapshot(&backend_config);
        assert_eq!(backend_bearer["credential_state"], "bearer_token");
        assert_eq!(
            backend_bearer["status"],
            "Backend mode is configured and a server credential is available.",
        );
        assert!(backend_bearer["next_action"]
            .as_str()
            .is_some_and(|value| value.contains("ugoite auth token-clear")));

        clear_test_env();
        std::env::set_var("UGOITE_AUTH_API_KEY", "api-secret");
        let api_key = auth_profile_snapshot(&api_config);
        assert_eq!(api_key["endpoint_mode"], "api");
        assert_eq!(
            api_key["topology"],
            "API endpoint at http://localhost:3000/api"
        );
        assert_eq!(api_key["credential_state"], "api_key");
        assert_eq!(
            api_key["status"],
            "API mode is configured and a server credential is available.",
        );

        std::env::set_var("UGOITE_AUTH_BEARER_TOKEN", "issued-token");
        let both_creds = auth_profile_snapshot(&backend_config);
        assert_eq!(both_creds["credential_state"], "bearer_token_and_api_key",);
        assert_eq!(
            credential_state_label(Some("issued-token"), Some("api-secret")),
            "bearer_token_and_api_key",
        );

        clear_test_env();
    }
}
