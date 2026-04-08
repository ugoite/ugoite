//! CLI auth login tests.
//! REQ-OPS-015
//! REQ-SEC-003

use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpListener;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
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

fn write_dev_auth_file(path: &Path, passkey_context: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create dev auth parent");
    }
    std::fs::write(
        path,
        serde_json::json!({
            "mode": "passkey-totp",
            "user_id": "dev-alice",
            "signing_secret": "dev-signing-secret",
            "signing_kid": "dev-local-v1",
            "passkey_context": passkey_context,
        })
        .to_string(),
    )
    .expect("write dev auth file");
}

fn auth_session_path_for_config(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("cli-auth.json")
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
fn test_cli_auth_login_req_ops_015_reuses_cached_dev_auth_file_context() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let auth_file = dir.path().join("dev-auth.json");
    write_dev_auth_file(&auth_file, "cached-passkey-context");
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
        .env("UGOITE_DEV_AUTH_FILE", &auth_file)
        .output()
        .expect("failed to execute");
    assert!(output.status.success());

    let request_text = requests.recv_timeout(Duration::from_secs(5)).unwrap();
    handle.join().unwrap();

    assert!(request_text.starts_with("POST /auth/login HTTP/1.1"));
    assert!(request_text
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-passkey-context: cached-passkey-context"));
    assert!(!request_text
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-auth-proxy-token:"));
}

#[test]
fn test_cli_auth_login_req_ops_015_surfaces_passkey_context_recovery_guidance() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let auth_file = dir.path().join("missing-dev-auth.json");
    let (base, requests, handle) = spawn_recording_server(
        "HTTP/1.1 401 Unauthorized",
        r#"{"detail":"Passkey-bound local context is missing or invalid."}"#,
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
        .env("UGOITE_DEV_AUTH_FILE", &auth_file)
        .output()
        .expect("failed to execute");
    assert!(!output.status.success());

    let request_text = requests.recv_timeout(Duration::from_secs(5)).unwrap();
    handle.join().unwrap();
    assert!(request_text.starts_with("POST /auth/login HTTP/1.1"));
    assert!(!request_text
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-passkey-context:"));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Passkey-bound local context is missing or invalid."),
        "stderr was {stderr}"
    );
    assert!(
        stderr.contains("UGOITE_DEV_PASSKEY_CONTEXT"),
        "stderr was {stderr}"
    );
    assert!(
        stderr.contains(auth_file.to_string_lossy().as_ref()),
        "stderr was {stderr}"
    );
    assert!(
        stderr.contains("scripts/dev-auth-env.sh"),
        "stderr was {stderr}"
    );
}

#[test]
fn test_cli_auth_login_req_ops_015_uses_default_cached_auth_path_in_recovery_guidance() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let (base, requests, handle) = spawn_recording_server(
        "HTTP/1.1 401 Unauthorized",
        r#"{"detail":"Passkey-bound local context is missing or invalid."}"#,
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
        .env_remove("HOME")
        .env_remove("UGOITE_DEV_AUTH_FILE")
        .env_remove("UGOITE_DEV_PASSKEY_CONTEXT")
        .output()
        .expect("failed to execute");
    assert!(!output.status.success());

    let request_text = requests.recv_timeout(Duration::from_secs(5)).unwrap();
    handle.join().unwrap();
    assert!(request_text.starts_with("POST /auth/login HTTP/1.1"));
    assert!(!request_text
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-passkey-context:"));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("~/.ugoite/dev-auth.json"),
        "stderr was {stderr}"
    );
    assert!(
        stderr.contains("scripts/dev-auth-env.sh"),
        "stderr was {stderr}"
    );
}

#[test]
fn test_cli_auth_login_req_ops_015_leaves_non_context_auth_failures_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let (base, requests, handle) = spawn_recording_server(
        "HTTP/1.1 401 Unauthorized",
        r#"{"detail":"Invalid username or 2FA code."}"#,
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
        .env("UGOITE_DEV_PASSKEY_CONTEXT", "passkey-context")
        .output()
        .expect("failed to execute");
    assert!(!output.status.success());

    let request_text = requests.recv_timeout(Duration::from_secs(5)).unwrap();
    handle.join().unwrap();
    assert!(request_text.starts_with("POST /auth/login HTTP/1.1"));
    assert!(request_text
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-passkey-context: passkey-context"));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Invalid username or 2FA code."),
        "stderr was {stderr}"
    );
    assert!(
        !stderr.contains("scripts/dev-auth-env.sh"),
        "stderr was {stderr}"
    );
    assert!(
        !stderr.contains("cached local dev auth file"),
        "stderr was {stderr}"
    );
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

#[test]
fn test_cli_auth_login_req_ops_015_persists_cli_session_for_followup_commands() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let auth_session_path = auth_session_path_for_config(&config_path);
    let (login_base, login_requests, login_handle) = spawn_recording_server(
        "HTTP/1.1 200 OK",
        r#"{"bearer_token":"issued-token","user_id":"dev-alice","expires_at":1900000000}"#,
    );

    let set_output = Command::new(ugoite_bin())
        .args([
            "config",
            "set",
            "--mode",
            "backend",
            "--backend-url",
            &login_base,
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(set_output.status.success());

    let login_output = Command::new(ugoite_bin())
        .args(["auth", "login", "--mock-oauth"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(login_output.status.success());

    let login_request = login_requests.recv_timeout(Duration::from_secs(5)).unwrap();
    login_handle.join().unwrap();
    assert!(login_request.starts_with("POST /auth/mock-oauth HTTP/1.1"));

    let stdout = String::from_utf8_lossy(&login_output.stdout);
    assert!(stdout.contains("export UGOITE_AUTH_BEARER_TOKEN='issued-token'"));

    let stderr = String::from_utf8_lossy(&login_output.stderr);
    assert!(stderr.contains("Saved CLI session"), "stderr was {stderr}");
    assert!(
        auth_session_path.exists(),
        "expected saved session at {}",
        auth_session_path.display()
    );
    let session_text = std::fs::read_to_string(&auth_session_path).unwrap();
    assert!(session_text.contains(r#""bearer_token": "issued-token""#));

    #[cfg(unix)]
    {
        let mode = std::fs::metadata(&auth_session_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    let profile_output = Command::new(ugoite_bin())
        .args(["auth", "profile"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .env_remove("UGOITE_AUTH_BEARER_TOKEN")
        .env_remove("UGOITE_AUTH_API_KEY")
        .output()
        .expect("failed to execute");
    assert!(profile_output.status.success());

    let profile_payload = parse_stdout_json(&profile_output);
    assert_eq!(
        profile_payload
            .get("credential_state")
            .and_then(Value::as_str),
        Some("bearer_token")
    );
    assert_eq!(
        profile_payload
            .get("UGOITE_AUTH_BEARER_TOKEN")
            .and_then(Value::as_str),
        Some("issu...")
    );

    let (space_base, space_requests, space_handle) =
        spawn_recording_server("HTTP/1.1 200 OK", "[]");
    let reset_output = Command::new(ugoite_bin())
        .args(["config", "set", "--backend-url", &space_base])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(reset_output.status.success());

    let list_output = Command::new(ugoite_bin())
        .args(["space", "list"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .env_remove("UGOITE_AUTH_BEARER_TOKEN")
        .env_remove("UGOITE_AUTH_API_KEY")
        .output()
        .expect("failed to execute");
    assert!(
        list_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&list_output.stderr)
    );

    let list_request = space_requests.recv_timeout(Duration::from_secs(5)).unwrap();
    space_handle.join().unwrap();
    assert!(list_request.starts_with("GET /spaces HTTP/1.1"));
    assert!(list_request
        .to_ascii_lowercase()
        .contains("authorization: bearer issued-token"));
}

#[test]
fn test_cli_auth_token_clear_req_ops_015_clears_saved_cli_session() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let auth_session_path = auth_session_path_for_config(&config_path);
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

    let login_output = Command::new(ugoite_bin())
        .args(["auth", "login", "--mock-oauth"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(login_output.status.success());
    requests.recv_timeout(Duration::from_secs(5)).unwrap();
    handle.join().unwrap();
    assert!(auth_session_path.exists());

    let token_clear_output = Command::new(ugoite_bin())
        .args(["auth", "token-clear"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(token_clear_output.status.success());
    assert!(!auth_session_path.exists());

    let clear_stdout = String::from_utf8_lossy(&token_clear_output.stdout);
    assert!(clear_stdout.contains("unset UGOITE_AUTH_BEARER_TOKEN"));
    assert!(clear_stdout.contains("unset UGOITE_AUTH_API_KEY"));

    let clear_stderr = String::from_utf8_lossy(&token_clear_output.stderr);
    assert!(
        clear_stderr.contains("Cleared the saved CLI session"),
        "stderr was {clear_stderr}"
    );

    let profile_output = Command::new(ugoite_bin())
        .args(["auth", "profile"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .env_remove("UGOITE_AUTH_BEARER_TOKEN")
        .env_remove("UGOITE_AUTH_API_KEY")
        .output()
        .expect("failed to execute");
    assert!(profile_output.status.success());

    let profile_payload = parse_stdout_json(&profile_output);
    assert_eq!(
        profile_payload
            .get("credential_state")
            .and_then(Value::as_str),
        Some("none")
    );
    assert!(profile_payload
        .get("UGOITE_AUTH_BEARER_TOKEN")
        .is_some_and(Value::is_null));
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
