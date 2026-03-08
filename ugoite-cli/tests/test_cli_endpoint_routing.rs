//! Integration tests for CLI endpoint routing configuration.
//! REQ-STO-001, REQ-STO-004, REQ-SEC-003

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
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

fn spawn_json_server(body: &'static str) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buffer = [0_u8; 1024];
                    let _ = stream.read(&mut buffer);
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "timed out waiting for CLI backend request"
                    );
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("failed to accept test request: {error}"),
            }
        }
    });
    (format!("http://{}", addr), handle)
}

/// REQ-STO-001: Config set/show round-trips correctly.
#[test]
fn test_cli_config_set_and_show() {
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

    let show_output = Command::new(ugoite_bin())
        .args(["config", "show"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(show_output.status.success());

    let stdout = String::from_utf8_lossy(&show_output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("JSON");
    assert_eq!(v["mode"].as_str(), Some("backend"));
    assert_eq!(v["backend_url"].as_str(), Some("http://localhost:9000"));
}

/// REQ-STO-004: Space list uses remote endpoint when backend mode is configured.
#[test]
fn test_space_list_uses_remote_endpoint_when_backend_mode() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let root = dir.path().to_string_lossy().to_string();

    // Set to backend mode with an unreachable URL
    Command::new(ugoite_bin())
        .args([
            "config",
            "set",
            "--mode",
            "backend",
            "--backend-url",
            "http://127.0.0.1:19999",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    // Space list should attempt to contact backend (and fail since it's unreachable)
    let output = Command::new(ugoite_bin())
        .args(["space", "list", &root])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    // Should fail (backend unreachable), confirming routing to backend
    assert!(
        !output.status.success(),
        "Expected failure connecting to unreachable backend"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("Cannot drop a runtime"),
        "CLI should return a normal connection error instead of panicking: {stderr}"
    );
}

/// REQ-STO-004: Backend mode returns remote space JSON without Tokio runtime panic.
#[test]
fn test_space_list_req_sto_004_returns_remote_json_without_panicking() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let root = dir.path().to_string_lossy().to_string();
    let (base_url, server_handle) =
        spawn_json_server(r#"[{"id":"remote-space","name":"Remote Space"}]"#);

    let set_output = Command::new(ugoite_bin())
        .args([
            "config",
            "set",
            "--mode",
            "backend",
            "--backend-url",
            &base_url,
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(set_output.status.success());

    let output = Command::new(ugoite_bin())
        .args(["space", "list", &root])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    server_handle.join().unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("Cannot drop a runtime"),
        "CLI should not panic in backend mode: {stderr}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("JSON");
    assert_eq!(value[0]["id"].as_str(), Some("remote-space"));
}

/// REQ-SEC-003: Service account create routes to backend when in backend mode.
#[test]
fn test_space_service_account_create_routes_to_backend() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let root = dir.path().to_string_lossy().to_string();

    // Set to backend mode with an unreachable URL
    Command::new(ugoite_bin())
        .args([
            "config",
            "set",
            "--mode",
            "backend",
            "--backend-url",
            "http://127.0.0.1:19999",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    // Service account creation should attempt to route to backend
    let output = Command::new(ugoite_bin())
        .args([
            "space",
            "service-account-create",
            &root,
            "my-space",
            "sa-name",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    // Should fail (backend unreachable or not in core mode), confirming routing to backend
    assert!(
        !output.status.success(),
        "Expected failure - service account requires backend mode"
    );
}
