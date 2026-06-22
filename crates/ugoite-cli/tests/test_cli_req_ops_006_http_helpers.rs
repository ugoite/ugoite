//! Portable API client transport coverage tests.
//! REQ-OPS-006
#![allow(clippy::await_holding_lock)]

mod support;

use serde_json::json;
use std::net::TcpListener;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use support::spawn_recording_server;
use ugoite_cli::http::execute;

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvState {
    bearer: Option<String>,
    api_key: Option<String>,
    dev_auth_proxy_token: Option<String>,
    dev_auth_file: Option<String>,
    dev_passkey_context: Option<String>,
}

impl EnvState {
    fn capture() -> Self {
        Self {
            bearer: std::env::var("UGOITE_AUTH_BEARER_TOKEN").ok(),
            api_key: std::env::var("UGOITE_AUTH_API_KEY").ok(),
            dev_auth_proxy_token: std::env::var("UGOITE_DEV_AUTH_PROXY_TOKEN").ok(),
            dev_auth_file: std::env::var("UGOITE_DEV_AUTH_FILE").ok(),
            dev_passkey_context: std::env::var("UGOITE_DEV_PASSKEY_CONTEXT").ok(),
        }
    }
}

impl Drop for EnvState {
    fn drop(&mut self) {
        for key in [
            "UGOITE_AUTH_BEARER_TOKEN",
            "UGOITE_AUTH_API_KEY",
            "UGOITE_DEV_AUTH_PROXY_TOKEN",
            "UGOITE_DEV_AUTH_FILE",
            "UGOITE_DEV_PASSKEY_CONTEXT",
        ] {
            std::env::remove_var(key);
        }
        if let Some(value) = &self.bearer {
            std::env::set_var("UGOITE_AUTH_BEARER_TOKEN", value);
        }
        if let Some(value) = &self.api_key {
            std::env::set_var("UGOITE_AUTH_API_KEY", value);
        }
        if let Some(value) = &self.dev_auth_proxy_token {
            std::env::set_var("UGOITE_DEV_AUTH_PROXY_TOKEN", value);
        }
        if let Some(value) = &self.dev_auth_file {
            std::env::set_var("UGOITE_DEV_AUTH_FILE", value);
        }
        if let Some(value) = &self.dev_passkey_context {
            std::env::set_var("UGOITE_DEV_PASSKEY_CONTEXT", value);
        }
    }
}

fn write_dev_auth_file(path: &Path, passkey_context: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create dev auth parent");
    }
    std::fs::write(
        path,
        json!({ "passkey_context": passkey_context }).to_string(),
    )
    .expect("write dev auth file");
}

#[tokio::test]
async fn test_cli_req_ops_006_executes_shared_methods_paths_and_json_bodies() {
    let cases = [
        ("space.list", json!({}), None, "GET /spaces HTTP/1.1", None),
        (
            "space.create",
            json!({}),
            Some(json!({"name": "demo"})),
            "POST /spaces HTTP/1.1",
            Some(r#"{"name":"demo"}"#),
        ),
        (
            "entry.update",
            json!({"space_id": "demo", "entry_id": "entry/1"}),
            Some(json!({"markdown": "# Updated"})),
            "PUT /spaces/demo/entries/entry%2F1 HTTP/1.1",
            Some(r##"{"markdown":"# Updated"}"##),
        ),
        (
            "space.patch",
            json!({"space_id": "demo"}),
            Some(json!({"name": "renamed"})),
            "PATCH /spaces/demo HTTP/1.1",
            Some(r#"{"name":"renamed"}"#),
        ),
        (
            "entry.delete",
            json!({"space_id": "demo", "entry_id": "entry-1", "hard_delete": true}),
            None,
            "DELETE /spaces/demo/entries/entry-1?hard_delete=true HTTP/1.1",
            None,
        ),
    ];

    for (operation, arguments, body, request_line, expected_body) in cases {
        let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"ok":true}"#);
        let value = execute(&base, operation, arguments, body)
            .await
            .expect("operation succeeds");
        assert_eq!(value, json!({"ok": true}));
        let request = requests
            .recv_timeout(Duration::from_secs(5))
            .expect("recorded request");
        handle.join().expect("join server");
        assert!(request.starts_with(request_line), "request was: {request}");
        if let Some(expected_body) = expected_body {
            assert!(request.contains("content-type: application/json"));
            assert!(request.ends_with(expected_body), "request was: {request}");
        }
    }
}

#[tokio::test]
async fn test_cli_req_ops_006_uses_shared_error_decoder() {
    let (base, _, handle) = spawn_recording_server(
        "HTTP/1.1 422 Unprocessable Entity",
        r#"{"detail":[{"msg":"Input should be at least 1 character"}]}"#,
    );
    let error = execute(&base, "space.create", json!({}), Some(json!({"name": ""})))
        .await
        .expect_err("must fail");
    handle.join().expect("join server");
    assert!(error
        .to_string()
        .contains("Input should be at least 1 character"));
    assert!(!error.to_string().contains("[object Object]"));
}

#[tokio::test]
async fn test_cli_req_ops_006_rejects_invalid_json_and_spa_html() {
    let (base, _, handle) = spawn_recording_server("HTTP/1.1 200 OK", "not-json");
    let error = execute(&base, "space.list", json!({}), None)
        .await
        .expect_err("invalid JSON must fail");
    handle.join().expect("join server");
    assert!(error.to_string().contains("not valid JSON"));

    let (base, _, handle) =
        spawn_recording_server("HTTP/1.1 200 OK", "<!doctype html><title>Ugoite</title>");
    let error = execute(&base, "space.list", json!({}), None)
        .await
        .expect_err("HTML must fail");
    handle.join().expect("join server");
    assert!(error.to_string().contains("ending in `/api`"));
}

#[tokio::test]
async fn test_cli_req_ops_006_reports_unreachable_and_unsafe_endpoints() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("address");
    drop(listener);
    let error = execute(&format!("http://{address}"), "space.list", json!({}), None)
        .await
        .expect_err("closed endpoint must fail");
    assert!(error.to_string().contains("send space.list request"));

    let error = execute("http://example.com", "space.list", json!({}), None)
        .await
        .expect_err("unsafe endpoint must fail");
    assert!(error.to_string().to_ascii_lowercase().contains("https"));
}

#[tokio::test]
async fn test_cli_req_ops_006_prefers_bearer_then_api_key_auth() {
    let _guard = env_lock().lock().expect("env lock");
    let _state = EnvState::capture();
    std::env::set_var("UGOITE_AUTH_BEARER_TOKEN", "bearer-secret");
    std::env::set_var("UGOITE_AUTH_API_KEY", "api-secret");

    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"[]"#);
    execute(&base, "space.list", json!({}), None)
        .await
        .expect("request succeeds");
    let request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("request");
    handle.join().expect("join server");
    let lower = request.to_ascii_lowercase();
    assert!(lower.contains("authorization: bearer bearer-secret"));
    assert!(!lower.contains("x-api-key: api-secret"));

    std::env::remove_var("UGOITE_AUTH_BEARER_TOKEN");
    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"[]"#);
    execute(&base, "space.list", json!({}), None)
        .await
        .expect("request succeeds");
    let request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("request");
    handle.join().expect("join server");
    assert!(request
        .to_ascii_lowercase()
        .contains("x-api-key: api-secret"));
}

#[tokio::test]
async fn test_cli_req_ops_006_applies_dev_proxy_headers_only_to_dev_auth_operations() {
    let _guard = env_lock().lock().expect("env lock");
    let _state = EnvState::capture();
    std::env::set_var("UGOITE_DEV_AUTH_PROXY_TOKEN", "proxy-secret");
    std::env::set_var("UGOITE_DEV_PASSKEY_CONTEXT", "passkey-context");

    let (base, requests, handle) =
        spawn_recording_server("HTTP/1.1 200 OK", r#"{"user_id":"dev","expires_at":1}"#);
    execute(&base, "auth.mock_oauth", json!({}), Some(json!({})))
        .await
        .expect("dev auth request succeeds");
    let request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("request");
    handle.join().expect("join server");
    let lower = request.to_ascii_lowercase();
    assert!(lower.contains("x-ugoite-dev-auth-proxy-token: proxy-secret"));
    assert!(lower.contains("x-ugoite-dev-passkey-context: passkey-context"));

    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"[]"#);
    execute(&base, "space.list", json!({}), None)
        .await
        .expect("standard request succeeds");
    let request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("request");
    handle.join().expect("join server");
    let lower = request.to_ascii_lowercase();
    assert!(!lower.contains("x-ugoite-dev-auth-proxy-token"));
    assert!(!lower.contains("x-ugoite-dev-passkey-context"));
}

#[tokio::test]
async fn test_cli_req_ops_006_reads_cached_dev_passkey_context() {
    let _guard = env_lock().lock().expect("env lock");
    let _state = EnvState::capture();
    std::env::remove_var("UGOITE_DEV_PASSKEY_CONTEXT");
    let temp = tempfile::tempdir().expect("tempdir");
    let auth_file = temp.path().join("dev-auth.json");
    write_dev_auth_file(&auth_file, "cached-context");
    std::env::set_var("UGOITE_DEV_AUTH_FILE", &auth_file);

    let (base, requests, handle) =
        spawn_recording_server("HTTP/1.1 200 OK", r#"{"user_id":"dev","expires_at":1}"#);
    execute(&base, "auth.mock_oauth", json!({}), Some(json!({})))
        .await
        .expect("dev auth request succeeds");
    let request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("request");
    handle.join().expect("join server");
    assert!(request
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-passkey-context: cached-context"));
}
