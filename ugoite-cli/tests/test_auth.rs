//! CLI auth login tests.
//! REQ-OPS-015
//! REQ-SEC-003

use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

fn ugoite_bin() -> String {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("ugoite");
    path.to_string_lossy().to_string()
}

fn spawn_recording_server(
    status_line: &'static str,
    body: &'static str,
) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let mut request = Vec::new();
                    let mut content_length = 0_usize;
                    let mut header_end: Option<usize> = None;

                    loop {
                        let mut buffer = [0_u8; 1024];
                        let read = match stream.read(&mut buffer) {
                            Ok(read) => read,
                            Err(error)
                                if error.kind() == std::io::ErrorKind::WouldBlock
                                    || error.kind() == std::io::ErrorKind::TimedOut
                                    || error.kind() == std::io::ErrorKind::Interrupted =>
                            {
                                assert!(Instant::now() < deadline, "timed out waiting for request");
                                thread::sleep(Duration::from_millis(10));
                                continue;
                            }
                            Err(error) => panic!("failed to read request: {error}"),
                        };
                        if read == 0 {
                            break;
                        }
                        request.extend_from_slice(&buffer[..read]);
                        if header_end.is_none() {
                            if let Some(pos) =
                                request.windows(4).position(|window| window == b"\r\n\r\n")
                            {
                                let end = pos + 4;
                                header_end = Some(end);
                                let headers = String::from_utf8_lossy(&request[..end]);
                                for line in headers.lines() {
                                    let mut parts = line.splitn(2, ':');
                                    if let (Some(name), Some(value)) = (parts.next(), parts.next())
                                    {
                                        if name.eq_ignore_ascii_case("Content-Length") {
                                            content_length = value.trim().parse().unwrap_or(0);
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(end) = header_end {
                            if request.len() >= end + content_length {
                                break;
                            }
                        }
                    }

                    tx.send(String::from_utf8_lossy(&request).into_owned())
                        .unwrap();
                    let response = format!(
                        "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "timed out waiting for request");
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("failed to accept request: {error}"),
            }
        }
    });
    (format!("http://{}", addr), rx, handle)
}

fn parse_stdout_json(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("stdout JSON")
}

#[test]
fn test_cli_auth_login_req_ops_015_posts_dev_login_and_prints_export() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let (base, requests, handle) = spawn_recording_server(
        "HTTP/1.1 200 OK",
        r#"{"bearer_token":"issued-token","user_id":"dev-alice","expires_at":1900000000}"#,
    );

    let set_output = Command::new(ugoite_bin())
        .args(["config", "set", "--mode", "backend", "--backend-url", &base])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(set_output.status.success());

    let output = Command::new(ugoite_bin())
        .args([
            "auth",
            "login",
            "--username",
            "dev-alice",
            "--totp-code",
            "123456",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .env("UGOITE_DEV_AUTH_PROXY_TOKEN", "proxy-secret")
        .env("UGOITE_DEV_PASSKEY_CONTEXT", "passkey-context")
        .output()
        .expect("failed to execute");
    assert!(output.status.success());

    let request_text = requests.recv_timeout(Duration::from_secs(5)).unwrap();
    handle.join().unwrap();

    assert!(request_text.starts_with("POST /auth/login HTTP/1.1"));
    assert!(request_text
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-auth-proxy-token: proxy-secret"));
    assert!(request_text
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-passkey-context: passkey-context"));
    assert!(request_text.contains(r#""username":"dev-alice""#));
    assert!(request_text.contains(r#""totp_code":"123456""#));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("export UGOITE_AUTH_BEARER_TOKEN='issued-token'"));
}

#[test]
fn test_cli_auth_login_req_ops_015_shell_escapes_bearer_token_exports() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let (base, requests, handle) = spawn_recording_server(
        "HTTP/1.1 200 OK",
        r#"{"bearer_token":"unsafe' $(touch /tmp/ugoite-pwned) $HOME","user_id":"dev-alice","expires_at":1900000000}"#,
    );

    let set_output = Command::new(ugoite_bin())
        .args(["config", "set", "--mode", "backend", "--backend-url", &base])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(set_output.status.success());

    let output = Command::new(ugoite_bin())
        .args([
            "auth",
            "login",
            "--username",
            "dev-alice",
            "--totp-code",
            "123456",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .env("UGOITE_DEV_AUTH_PROXY_TOKEN", "proxy-secret")
        .env("UGOITE_DEV_PASSKEY_CONTEXT", "passkey-context")
        .output()
        .expect("failed to execute");
    assert!(output.status.success());

    let request_text = requests.recv_timeout(Duration::from_secs(5)).unwrap();
    handle.join().unwrap();

    assert!(request_text.starts_with("POST /auth/login HTTP/1.1"));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(
            "export UGOITE_AUTH_BEARER_TOKEN='unsafe'\\'' $(touch /tmp/ugoite-pwned) $HOME'"
        ),
        "stdout was {stdout}"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("POSIX shell quoting"),
        "stderr was {stderr}"
    );
}

#[test]
fn test_cli_auth_login_req_ops_015_supports_fish_env_output() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let (base, requests, handle) = spawn_recording_server(
        "HTTP/1.1 200 OK",
        r#"{"bearer_token":"unsafe' $HOME","user_id":"dev-alice","expires_at":1900000000}"#,
    );

    let set_output = Command::new(ugoite_bin())
        .args(["config", "set", "--mode", "backend", "--backend-url", &base])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(set_output.status.success());

    let output = Command::new(ugoite_bin())
        .args([
            "auth",
            "login",
            "--shell",
            "fish",
            "--username",
            "dev-alice",
            "--totp-code",
            "123456",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .env("UGOITE_DEV_AUTH_PROXY_TOKEN", "proxy-secret")
        .env("UGOITE_DEV_PASSKEY_CONTEXT", "passkey-context")
        .output()
        .expect("failed to execute");
    assert!(output.status.success());

    let request_text = requests.recv_timeout(Duration::from_secs(5)).unwrap();
    handle.join().unwrap();

    assert!(request_text.starts_with("POST /auth/login HTTP/1.1"));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("set -gx UGOITE_AUTH_BEARER_TOKEN 'unsafe\\' $HOME'"),
        "stdout was {stdout}"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ugoite auth login --shell fish --username USER --totp-code CODE | source"),
        "stderr was {stderr}"
    );
}

#[test]
fn test_cli_auth_login_req_ops_015_supports_powershell_env_output() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let (base, requests, handle) = spawn_recording_server(
        "HTTP/1.1 200 OK",
        r#"{"bearer_token":"unsafe' $HOME","user_id":"dev-alice","expires_at":1900000000}"#,
    );

    let set_output = Command::new(ugoite_bin())
        .args(["config", "set", "--mode", "backend", "--backend-url", &base])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(set_output.status.success());

    let output = Command::new(ugoite_bin())
        .args([
            "auth",
            "login",
            "--shell",
            "powershell",
            "--username",
            "dev-alice",
            "--totp-code",
            "123456",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .env("UGOITE_DEV_AUTH_PROXY_TOKEN", "proxy-secret")
        .env("UGOITE_DEV_PASSKEY_CONTEXT", "passkey-context")
        .output()
        .expect("failed to execute");
    assert!(output.status.success());

    let request_text = requests.recv_timeout(Duration::from_secs(5)).unwrap();
    handle.join().unwrap();

    assert!(request_text.starts_with("POST /auth/login HTTP/1.1"));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("$env:UGOITE_AUTH_BEARER_TOKEN = 'unsafe'' $HOME'"),
        "stdout was {stdout}"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "ugoite auth login --shell powershell --username USER --totp-code CODE | Invoke-Expression"
        ),
        "stderr was {stderr}"
    );
}

#[test]
fn test_cli_auth_login_req_ops_015_quotes_empty_bearer_token_exports() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let (base, requests, handle) = spawn_recording_server(
        "HTTP/1.1 200 OK",
        r#"{"bearer_token":"","user_id":"dev-alice","expires_at":1900000000}"#,
    );

    let set_output = Command::new(ugoite_bin())
        .args(["config", "set", "--mode", "backend", "--backend-url", &base])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(set_output.status.success());

    let output = Command::new(ugoite_bin())
        .args([
            "auth",
            "login",
            "--username",
            "dev-alice",
            "--totp-code",
            "123456",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .env("UGOITE_DEV_AUTH_PROXY_TOKEN", "proxy-secret")
        .env("UGOITE_DEV_PASSKEY_CONTEXT", "passkey-context")
        .output()
        .expect("failed to execute");
    assert!(output.status.success());

    let request_text = requests.recv_timeout(Duration::from_secs(5)).unwrap();
    handle.join().unwrap();

    assert!(request_text.starts_with("POST /auth/login HTTP/1.1"));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("export UGOITE_AUTH_BEARER_TOKEN=''"),
        "stdout was {stdout}"
    );
}

#[test]
fn test_cli_auth_token_clear_req_ops_015_prints_shell_specific_unsets() {
    for (shell_args, expected_lines) in [
        (
            Vec::<&str>::new(),
            vec![
                "unset UGOITE_AUTH_BEARER_TOKEN",
                "unset UGOITE_AUTH_API_KEY",
            ],
        ),
        (
            vec!["--shell", "bash"],
            vec![
                "unset UGOITE_AUTH_BEARER_TOKEN",
                "unset UGOITE_AUTH_API_KEY",
            ],
        ),
        (
            vec!["--shell", "zsh"],
            vec![
                "unset UGOITE_AUTH_BEARER_TOKEN",
                "unset UGOITE_AUTH_API_KEY",
            ],
        ),
        (
            vec!["--shell", "fish"],
            vec![
                "set -e UGOITE_AUTH_BEARER_TOKEN",
                "set -e UGOITE_AUTH_API_KEY",
            ],
        ),
        (
            vec!["--shell", "powershell"],
            vec![
                "Remove-Item Env:UGOITE_AUTH_BEARER_TOKEN -ErrorAction SilentlyContinue",
                "Remove-Item Env:UGOITE_AUTH_API_KEY -ErrorAction SilentlyContinue",
            ],
        ),
    ] {
        let mut token_clear_args = vec!["auth", "token-clear"];
        token_clear_args.extend(shell_args.iter().copied());
        let token_clear_output = Command::new(ugoite_bin())
            .args(&token_clear_args)
            .output()
            .expect("failed to execute");
        assert!(token_clear_output.status.success(), "{token_clear_args:?}");

        let mut logout_args = vec!["auth", "logout"];
        logout_args.extend(shell_args.iter().copied());
        let logout_output = Command::new(ugoite_bin())
            .args(&logout_args)
            .output()
            .expect("failed to execute");
        assert!(logout_output.status.success(), "{logout_args:?}");

        let token_clear_stdout = String::from_utf8_lossy(&token_clear_output.stdout);
        let logout_stdout = String::from_utf8_lossy(&logout_output.stdout);
        assert_eq!(logout_stdout, token_clear_stdout, "{shell_args:?}");
        for expected in expected_lines {
            assert!(
                token_clear_stdout.contains(expected),
                "{shell_args:?}: {token_clear_stdout}"
            );
        }
    }
}

/// REQ-OPS-015: auth profile distinguishes local-first core mode from backend auth states.
#[test]
fn test_cli_auth_profile_req_ops_015_reports_core_mode_without_backend_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");

    let set_backend_output = Command::new(ugoite_bin())
        .args([
            "config",
            "set",
            "--mode",
            "backend",
            "--backend-url",
            "http://localhost:9000",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(set_backend_output.status.success());

    let set_core_output = Command::new(ugoite_bin())
        .args(["config", "set", "--mode", "core"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(set_core_output.status.success());

    let output = Command::new(ugoite_bin())
        .args(["auth", "profile"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .env_remove("UGOITE_AUTH_BEARER_TOKEN")
        .env_remove("UGOITE_AUTH_API_KEY")
        .output()
        .expect("failed to execute");
    assert!(output.status.success());

    let payload = parse_stdout_json(&output);
    assert_eq!(
        payload.get("endpoint_mode").and_then(Value::as_str),
        Some("core")
    );
    assert_eq!(
        payload.get("topology").and_then(Value::as_str),
        Some("local filesystem via ugoite-core")
    );
    assert!(payload.get("endpoint_url").is_some_and(Value::is_null));
    assert_eq!(
        payload
            .get("backend_auth_required")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        payload.get("credential_state").and_then(Value::as_str),
        Some("none")
    );
    assert_eq!(
        payload.get("status").and_then(Value::as_str),
        Some("Core mode does not require backend authentication.")
    );
    let next_action = payload
        .get("next_action")
        .and_then(Value::as_str)
        .expect("next_action string");
    assert!(
        next_action
            .contains("ugoite config set --mode backend --backend-url http://localhost:9000"),
        "{next_action}"
    );
    assert!(
        !next_action.contains("http://localhost:8000"),
        "{next_action}"
    );
    assert!(payload
        .get("UGOITE_AUTH_BEARER_TOKEN")
        .is_some_and(Value::is_null));
    assert!(payload
        .get("UGOITE_AUTH_API_KEY")
        .is_some_and(Value::is_null));
}

/// REQ-OPS-015: auth profile shows server-backed modes still need credentials before login.
#[test]
fn test_cli_auth_profile_req_ops_015_reports_backend_mode_without_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");

    let set_output = Command::new(ugoite_bin())
        .args([
            "config",
            "set",
            "--mode",
            "backend",
            "--backend-url",
            "http://localhost:9000",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(set_output.status.success());

    let output = Command::new(ugoite_bin())
        .args(["auth", "profile"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .env_remove("UGOITE_AUTH_BEARER_TOKEN")
        .env_remove("UGOITE_AUTH_API_KEY")
        .output()
        .expect("failed to execute");
    assert!(output.status.success());

    let payload = parse_stdout_json(&output);
    assert_eq!(
        payload.get("endpoint_mode").and_then(Value::as_str),
        Some("backend")
    );
    assert_eq!(
        payload.get("endpoint_url").and_then(Value::as_str),
        Some("http://localhost:9000")
    );
    assert_eq!(
        payload
            .get("backend_auth_required")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        payload.get("credential_state").and_then(Value::as_str),
        Some("none")
    );
    assert_eq!(
        payload.get("status").and_then(Value::as_str),
        Some("Backend mode is configured, but no bearer token or API key is currently set.")
    );
    let next_action = payload
        .get("next_action")
        .and_then(Value::as_str)
        .expect("next_action string");
    assert!(next_action.contains("ugoite auth login"), "{next_action}");
    assert!(next_action.contains("UGOITE_AUTH_API_KEY"), "{next_action}");
}

/// REQ-OPS-015: auth profile keeps masked credential state visible once server auth is configured.
#[test]
fn test_cli_auth_profile_req_ops_015_reports_masked_backend_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");

    let set_output = Command::new(ugoite_bin())
        .args([
            "config",
            "set",
            "--mode",
            "backend",
            "--backend-url",
            "http://localhost:9000",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(set_output.status.success());

    let output = Command::new(ugoite_bin())
        .args(["auth", "profile"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .env("UGOITE_AUTH_BEARER_TOKEN", "issued-token")
        .env_remove("UGOITE_AUTH_API_KEY")
        .output()
        .expect("failed to execute");
    assert!(output.status.success());

    let payload = parse_stdout_json(&output);
    assert_eq!(
        payload.get("credential_state").and_then(Value::as_str),
        Some("bearer_token")
    );
    assert_eq!(
        payload.get("status").and_then(Value::as_str),
        Some("Backend mode is configured and a server credential is available.")
    );
    assert_eq!(
        payload
            .get("UGOITE_AUTH_BEARER_TOKEN")
            .and_then(Value::as_str),
        Some("issu...")
    );
    let next_action = payload
        .get("next_action")
        .and_then(Value::as_str)
        .expect("next_action string");
    assert!(
        next_action.contains(r#"eval "$(ugoite auth token-clear)""#),
        "{next_action}"
    );
    assert!(next_action.contains("--shell fish"), "{next_action}");
    assert!(next_action.contains("--shell powershell"), "{next_action}");
}

/// REQ-OPS-015: auth profile distinguishes API mode and ignores blank bearer tokens when an API key is set.
#[test]
fn test_cli_auth_profile_req_ops_015_reports_api_mode_with_api_key() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");

    let set_output = Command::new(ugoite_bin())
        .args([
            "config",
            "set",
            "--mode",
            "api",
            "--api-url",
            "https://api.example.com",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(set_output.status.success());

    let output = Command::new(ugoite_bin())
        .args(["auth", "profile"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .env("UGOITE_AUTH_BEARER_TOKEN", "   ")
        .env("UGOITE_AUTH_API_KEY", "issued-api-key")
        .output()
        .expect("failed to execute");
    assert!(output.status.success());

    let payload = parse_stdout_json(&output);
    assert_eq!(
        payload.get("endpoint_mode").and_then(Value::as_str),
        Some("api")
    );
    assert_eq!(
        payload.get("topology").and_then(Value::as_str),
        Some("API endpoint at https://api.example.com")
    );
    assert_eq!(
        payload.get("endpoint_url").and_then(Value::as_str),
        Some("https://api.example.com")
    );
    assert_eq!(
        payload
            .get("backend_auth_required")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        payload.get("credential_state").and_then(Value::as_str),
        Some("api_key")
    );
    assert_eq!(
        payload.get("status").and_then(Value::as_str),
        Some("API mode is configured and a server credential is available.")
    );
    let next_action = payload
        .get("next_action")
        .and_then(Value::as_str)
        .expect("next_action string");
    assert!(
        next_action.contains(r#"eval "$(ugoite auth token-clear)""#),
        "{next_action}"
    );
    assert!(next_action.contains("--shell fish"), "{next_action}");
    assert!(next_action.contains("--shell powershell"), "{next_action}");
    assert!(payload
        .get("UGOITE_AUTH_BEARER_TOKEN")
        .is_some_and(Value::is_null));
    assert_eq!(
        payload.get("UGOITE_AUTH_API_KEY").and_then(Value::as_str),
        Some("issu...")
    );
}

/// REQ-OPS-015: auth login help scopes proxy-token guidance to proxied mock-oauth flows.
#[test]
fn test_cli_auth_login_req_ops_015_help_scopes_mock_oauth_proxy_token_requirement() {
    let output = Command::new(ugoite_bin())
        .args(["auth", "login", "--help"])
        .output()
        .expect("failed to execute");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout
            .contains("Direct loopback backend mode does not require UGOITE_DEV_AUTH_PROXY_TOKEN"),
        "{stdout}"
    );
    assert!(
        stdout.contains("proxied/container flows require UGOITE_DEV_AUTH_PROXY_TOKEN"),
        "{stdout}"
    );
    assert!(stdout.contains("--shell <SHELL>"), "{stdout}");
    assert!(
        stdout.contains(
            "ugoite auth login --shell fish --username alice --totp-code 123456 | source"
        ),
        "{stdout}"
    );
    assert!(
        stdout.contains(
            "ugoite auth login --shell powershell --username alice --totp-code 123456 | Invoke-Expression"
        ),
        "{stdout}"
    );
    assert!(
        !stdout.contains(
            "Use mock OAuth flow (development only, requires UGOITE_DEV_AUTH_PROXY_TOKEN)",
        ),
        "{stdout}"
    );
}

/// REQ-SEC-003: auth overview includes canonical channels and provider provenance fields.
#[test]
fn test_cli_auth_overview_req_sec_003_includes_channels() {
    let output = Command::new(ugoite_bin())
        .args(["auth", "overview"])
        .output()
        .expect("failed to execute auth overview");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let payload: Value = serde_json::from_str(&stdout).expect("auth overview JSON");
    let channels = payload
        .get("channels")
        .and_then(Value::as_array)
        .expect("channels array");
    let channel_names: Vec<_> = channels
        .iter()
        .map(|value| value.as_str().expect("channel string"))
        .collect();

    assert_eq!(
        channel_names,
        vec![
            "backend(rest)",
            "backend(mcp)",
            "cli(via backend)",
            "frontend(via backend)",
        ],
    );

    let providers = payload
        .get("providers")
        .and_then(Value::as_object)
        .expect("providers object");
    let bearer = providers
        .get("bearer")
        .and_then(Value::as_object)
        .expect("bearer provider");
    let api_key = providers
        .get("api_key")
        .and_then(Value::as_object)
        .expect("api_key provider");

    assert_eq!(
        bearer.get("active_kids_source").and_then(Value::as_str),
        Some("UGOITE_AUTH_BEARER_ACTIVE_KIDS"),
    );
    assert_eq!(
        api_key.get("revocation_source").and_then(Value::as_str),
        Some("UGOITE_AUTH_REVOKED_KEY_IDS"),
    );
}
