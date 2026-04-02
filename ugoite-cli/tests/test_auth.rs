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

/// REQ-OPS-015: auth profile distinguishes local-first core mode from backend auth states.
#[test]
fn test_cli_auth_profile_req_ops_015_reports_core_mode_without_backend_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");

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
    assert!(payload
        .get("next_action")
        .and_then(Value::as_str)
        .is_some_and(|value| value.contains("ugoite config set --mode backend")));
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
    assert!(payload
        .get("next_action")
        .and_then(Value::as_str)
        .is_some_and(|value| value.contains("ugoite auth token-clear")));
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
    assert!(
        !stdout.contains(
            "Use mock OAuth flow (development only, requires UGOITE_DEV_AUTH_PROXY_TOKEN)",
        ),
        "{stdout}"
    );
}

/// REQ-SEC-003: auth overview includes the canonical channels field.
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
}
