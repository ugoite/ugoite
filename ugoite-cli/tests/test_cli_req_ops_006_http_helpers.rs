//! CLI HTTP helper coverage tests.
//! REQ-OPS-006

mod support;

use serde_json::json;
use std::net::TcpListener;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use support::spawn_recording_server;
use ugoite_cli::http::{
    http_delete, http_get, http_patch, http_post, http_post_with_dev_auth_proxy, http_put,
};

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
        std::env::remove_var("UGOITE_AUTH_BEARER_TOKEN");
        std::env::remove_var("UGOITE_AUTH_API_KEY");
        std::env::remove_var("UGOITE_DEV_AUTH_PROXY_TOKEN");
        std::env::remove_var("UGOITE_DEV_AUTH_FILE");
        std::env::remove_var("UGOITE_DEV_PASSKEY_CONTEXT");
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
        serde_json::json!({
            "passkey_context": passkey_context,
        })
        .to_string(),
    )
    .expect("write dev auth file");
}

/// REQ-OPS-006: each HTTP helper must return JSON on successful responses.
#[tokio::test]
async fn test_cli_req_ops_006_http_helpers_return_json_on_success() {
    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"ok":true}"#);
    let get_value = http_get(&format!("{base}/items"))
        .await
        .expect("http_get success");
    assert_eq!(get_value, json!({"ok": true}));
    let get_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("get request");
    handle.join().expect("join get server");
    assert!(get_request.starts_with("GET /items HTTP/1.1"));

    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"created":true}"#);
    let post_value = http_post(&format!("{base}/items"), &json!({"name": "demo"}))
        .await
        .expect("http_post success");
    assert_eq!(post_value, json!({"created": true}));
    let post_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("post request");
    handle.join().expect("join post server");
    assert!(post_request.starts_with("POST /items HTTP/1.1"));
    assert!(post_request.contains(r#""name":"demo""#));

    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"updated":true}"#);
    let put_value = http_put(&format!("{base}/items"), &json!({"name": "updated"}))
        .await
        .expect("http_put success");
    assert_eq!(put_value, json!({"updated": true}));
    let put_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("put request");
    handle.join().expect("join put server");
    assert!(put_request.starts_with("PUT /items HTTP/1.1"));

    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"patched":true}"#);
    let patch_value = http_patch(&format!("{base}/items"), &json!({"name": "patched"}))
        .await
        .expect("http_patch success");
    assert_eq!(patch_value, json!({"patched": true}));
    let patch_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("patch request");
    handle.join().expect("join patch server");
    assert!(patch_request.starts_with("PATCH /items HTTP/1.1"));

    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"deleted":true}"#);
    let delete_value = http_delete(&format!("{base}/items"))
        .await
        .expect("http_delete success");
    assert_eq!(delete_value, json!({"deleted": true}));
    let delete_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("delete request");
    handle.join().expect("join delete server");
    assert!(delete_request.starts_with("DELETE /items HTTP/1.1"));
}

/// REQ-OPS-006: each HTTP helper must surface non-success status codes with response text.
#[tokio::test]
async fn test_cli_req_ops_006_http_helpers_surface_error_bodies() {
    let (base, _requests, handle) =
        spawn_recording_server("HTTP/1.1 500 Internal Server Error", "get failed");
    let get_err = http_get(&format!("{base}/items"))
        .await
        .expect_err("http_get should fail");
    handle.join().expect("join get error server");
    assert!(get_err.to_string().contains("HTTP 500"));
    assert!(get_err.to_string().contains("get failed"));

    let (base, _requests, handle) =
        spawn_recording_server("HTTP/1.1 422 Unprocessable Entity", "post failed");
    let post_err = http_post(&format!("{base}/items"), &json!({"name": "demo"}))
        .await
        .expect_err("http_post should fail");
    handle.join().expect("join post error server");
    assert!(post_err.to_string().contains("HTTP 422"));
    assert!(post_err.to_string().contains("post failed"));

    let (base, _requests, handle) = spawn_recording_server("HTTP/1.1 409 Conflict", "put failed");
    let put_err = http_put(&format!("{base}/items"), &json!({"name": "updated"}))
        .await
        .expect_err("http_put should fail");
    handle.join().expect("join put error server");
    assert!(put_err.to_string().contains("HTTP 409"));
    assert!(put_err.to_string().contains("put failed"));

    let (base, _requests, handle) =
        spawn_recording_server("HTTP/1.1 400 Bad Request", "patch failed");
    let patch_err = http_patch(&format!("{base}/items"), &json!({"name": "patched"}))
        .await
        .expect_err("http_patch should fail");
    handle.join().expect("join patch error server");
    assert!(patch_err.to_string().contains("HTTP 400"));
    assert!(patch_err.to_string().contains("patch failed"));

    let (base, _requests, handle) =
        spawn_recording_server("HTTP/1.1 403 Forbidden", "delete failed");
    let delete_err = http_delete(&format!("{base}/items"))
        .await
        .expect_err("http_delete should fail");
    handle.join().expect("join delete error server");
    assert!(delete_err.to_string().contains("HTTP 403"));
    assert!(delete_err.to_string().contains("delete failed"));
}

/// REQ-OPS-006: http_get must fail when a successful response body is not valid JSON.
#[tokio::test]
async fn test_cli_req_ops_006_http_get_rejects_invalid_json_success_body() {
    let (base, _requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", "not-json");
    let get_err = http_get(&format!("{base}/items"))
        .await
        .expect_err("http_get should fail on invalid JSON");
    handle.join().expect("join invalid json server");
    assert!(
        !get_err.to_string().is_empty(),
        "invalid JSON should surface a decoding error"
    );
}

/// REQ-OPS-006: HTTP helpers must surface transport errors when loopback endpoints are unreachable.
#[tokio::test]
async fn test_cli_req_ops_006_http_helpers_surface_unreachable_loopback_transport_errors() {
    let _guard = env_lock().lock().expect("env lock");
    let _env = EnvState::capture();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind closed-port probe");
    let addr = listener.local_addr().expect("probe local addr");
    drop(listener);
    let closed_url = format!("http://{addr}/items");

    let get_err = http_get(&closed_url)
        .await
        .expect_err("http_get should fail when loopback endpoint is unreachable");
    let post_err = http_post(&closed_url, &json!({"name": "demo"}))
        .await
        .expect_err("http_post should fail when loopback endpoint is unreachable");
    let proxy_post_err = http_post_with_dev_auth_proxy(&closed_url, &json!({}))
        .await
        .expect_err(
            "http_post_with_dev_auth_proxy should fail when loopback endpoint is unreachable",
        );
    let put_err = http_put(&closed_url, &json!({"name": "updated"}))
        .await
        .expect_err("http_put should fail when loopback endpoint is unreachable");
    let patch_err = http_patch(&closed_url, &json!({"name": "patched"}))
        .await
        .expect_err("http_patch should fail when loopback endpoint is unreachable");
    let delete_err = http_delete(&closed_url)
        .await
        .expect_err("http_delete should fail when loopback endpoint is unreachable");

    for err in [
        get_err,
        post_err,
        proxy_post_err,
        put_err,
        patch_err,
        delete_err,
    ] {
        assert!(
            !err.to_string().is_empty(),
            "transport failures should return a visible error"
        );
    }
}

/// REQ-OPS-006: HTTP helpers must reject unsafe non-loopback cleartext endpoints.
#[tokio::test]
async fn test_cli_req_ops_006_http_helpers_reject_unsafe_remote_urls() {
    let unsafe_url = "http://example.com/items";

    let get_err = http_get(unsafe_url)
        .await
        .expect_err("http_get should reject unsafe remote url");
    assert!(get_err
        .to_string()
        .contains("Remote request URL http://example.com/items uses cleartext http://"));

    let post_err = http_post(unsafe_url, &json!({"name": "demo"}))
        .await
        .expect_err("http_post should reject unsafe remote url");
    assert!(post_err
        .to_string()
        .contains("Remote request URL http://example.com/items uses cleartext http://"));

    let proxy_post_err = http_post_with_dev_auth_proxy(unsafe_url, &json!({}))
        .await
        .expect_err("http_post_with_dev_auth_proxy should reject unsafe remote url");
    assert!(proxy_post_err
        .to_string()
        .contains("Remote request URL http://example.com/items uses cleartext http://"));

    let put_err = http_put(unsafe_url, &json!({"name": "updated"}))
        .await
        .expect_err("http_put should reject unsafe remote url");
    assert!(put_err
        .to_string()
        .contains("Remote request URL http://example.com/items uses cleartext http://"));

    let patch_err = http_patch(unsafe_url, &json!({"name": "patched"}))
        .await
        .expect_err("http_patch should reject unsafe remote url");
    assert!(patch_err
        .to_string()
        .contains("Remote request URL http://example.com/items uses cleartext http://"));

    let delete_err = http_delete(unsafe_url)
        .await
        .expect_err("http_delete should reject unsafe remote url");
    assert!(delete_err
        .to_string()
        .contains("Remote request URL http://example.com/items uses cleartext http://"));
}

/// REQ-OPS-006: the dev-auth proxy helper must reject invalid endpoint URLs before any request is sent.
#[tokio::test]
async fn test_cli_req_ops_006_http_post_with_dev_auth_proxy_rejects_invalid_url() {
    let err = http_post_with_dev_auth_proxy("not-a-url", &json!({}))
        .await
        .expect_err("http_post_with_dev_auth_proxy should reject invalid url");

    assert!(err
        .to_string()
        .contains("Remote request URL \"not-a-url\" is invalid"));
}

/// REQ-OPS-006: auth headers must prefer bearer tokens and fall back to API keys.
#[tokio::test]
async fn test_cli_req_ops_006_http_helpers_apply_auth_headers() {
    let _guard = env_lock().lock().expect("env lock");
    let _env = EnvState::capture();

    std::env::set_var("UGOITE_AUTH_BEARER_TOKEN", "bearer-secret");
    std::env::remove_var("UGOITE_AUTH_API_KEY");
    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"ok":true}"#);
    http_get(&format!("{base}/items"))
        .await
        .expect("bearer request should succeed");
    let bearer_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("bearer request text");
    handle.join().expect("join bearer server");
    assert!(bearer_request
        .to_ascii_lowercase()
        .contains("authorization: bearer bearer-secret\r\n"));

    std::env::remove_var("UGOITE_AUTH_BEARER_TOKEN");
    std::env::set_var("UGOITE_AUTH_API_KEY", "api-key-secret");
    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"ok":true}"#);
    http_get(&format!("{base}/items"))
        .await
        .expect("api key request should succeed");
    let api_key_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("api key request text");
    handle.join().expect("join api key server");
    assert!(api_key_request
        .to_ascii_lowercase()
        .contains("x-api-key: api-key-secret\r\n"));
}

/// REQ-OPS-006: explicit auth POST helpers must only send dev local-auth headers when configured.
#[tokio::test]
async fn test_cli_req_ops_006_http_helpers_apply_dev_auth_proxy_header() {
    let _guard = env_lock().lock().expect("env lock");
    let _env = EnvState::capture();
    let dir = tempfile::tempdir().expect("tempdir");
    let missing_auth_file = dir.path().join("missing-dev-auth.json");
    let cached_auth_file = dir.path().join("cached-dev-auth.json");
    let blank_cached_auth_file = dir.path().join("blank-dev-auth.json");

    write_dev_auth_file(&cached_auth_file, "cached-passkey-context");
    write_dev_auth_file(&blank_cached_auth_file, "   ");

    std::env::set_var("UGOITE_DEV_AUTH_PROXY_TOKEN", "   ");
    std::env::set_var("UGOITE_DEV_AUTH_FILE", &missing_auth_file);
    std::env::remove_var("UGOITE_DEV_PASSKEY_CONTEXT");
    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"ok":true}"#);
    http_post_with_dev_auth_proxy(&format!("{base}/auth/mock-oauth"), &json!({}))
        .await
        .expect("blank proxy token request should succeed");
    let blank_proxy_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("blank proxy request text");
    handle.join().expect("join blank proxy server");
    assert!(!blank_proxy_request
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-auth-proxy-token:"),);
    assert!(!blank_proxy_request
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-passkey-context:"));

    std::env::set_var("UGOITE_DEV_AUTH_PROXY_TOKEN", "proxy-secret");
    std::env::set_var("UGOITE_DEV_AUTH_FILE", &missing_auth_file);
    std::env::set_var("UGOITE_DEV_PASSKEY_CONTEXT", "   ");
    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"ok":true}"#);
    http_post_with_dev_auth_proxy(&format!("{base}/auth/mock-oauth"), &json!({}))
        .await
        .expect("blank passkey context request should succeed");
    let blank_context_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("blank passkey context request text");
    handle.join().expect("join blank passkey context server");
    assert!(blank_context_request
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-auth-proxy-token: proxy-secret\r\n"));
    assert!(!blank_context_request
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-passkey-context:"));

    std::env::set_var("UGOITE_DEV_AUTH_PROXY_TOKEN", "proxy-secret");
    std::env::set_var("UGOITE_DEV_AUTH_FILE", &cached_auth_file);
    std::env::set_var("UGOITE_DEV_PASSKEY_CONTEXT", "   ");
    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"ok":true}"#);
    http_post_with_dev_auth_proxy(&format!("{base}/auth/mock-oauth"), &json!({}))
        .await
        .expect("cached passkey context request should succeed");
    let cached_context_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("cached passkey context request text");
    handle.join().expect("join cached passkey context server");
    assert!(cached_context_request
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-auth-proxy-token: proxy-secret\r\n"));
    assert!(cached_context_request
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-passkey-context: cached-passkey-context\r\n"));

    std::env::set_var("UGOITE_DEV_AUTH_PROXY_TOKEN", "proxy-secret");
    std::env::set_var("UGOITE_DEV_AUTH_FILE", &blank_cached_auth_file);
    std::env::set_var("UGOITE_DEV_PASSKEY_CONTEXT", "   ");
    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"ok":true}"#);
    http_post_with_dev_auth_proxy(&format!("{base}/auth/mock-oauth"), &json!({}))
        .await
        .expect("blank cached passkey context request should succeed");
    let blank_cached_context_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("blank cached passkey context request text");
    handle
        .join()
        .expect("join blank cached passkey context server");
    assert!(blank_cached_context_request
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-auth-proxy-token: proxy-secret\r\n"));
    assert!(!blank_cached_context_request
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-passkey-context:"));

    std::env::set_var("UGOITE_DEV_AUTH_PROXY_TOKEN", "proxy-secret");
    std::env::set_var("UGOITE_DEV_AUTH_FILE", &cached_auth_file);
    std::env::set_var("UGOITE_DEV_PASSKEY_CONTEXT", "passkey-context");
    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"ok":true}"#);
    http_post_with_dev_auth_proxy(&format!("{base}/auth/mock-oauth"), &json!({}))
        .await
        .expect("proxy token request should succeed");
    let proxy_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("proxy request text");
    handle.join().expect("join proxy server");
    assert!(proxy_request
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-auth-proxy-token: proxy-secret\r\n"));
    assert!(proxy_request
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-passkey-context: passkey-context\r\n"));
    assert!(!proxy_request
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-passkey-context: cached-passkey-context\r\n"));
}

/// REQ-OPS-015: loopback auth helpers must ignore malformed cached dev auth files.
#[tokio::test]
async fn test_cli_req_ops_015_http_helpers_ignore_malformed_cached_dev_auth_file() {
    let _guard = env_lock().lock().expect("env lock");
    let _env = EnvState::capture();
    let dir = tempfile::tempdir().expect("tempdir");
    let malformed_auth_file = dir.path().join("malformed-dev-auth.json");
    std::fs::write(&malformed_auth_file, "{not-json").expect("write malformed dev auth file");

    std::env::set_var("UGOITE_DEV_AUTH_PROXY_TOKEN", "proxy-secret");
    std::env::set_var("UGOITE_DEV_AUTH_FILE", &malformed_auth_file);
    std::env::remove_var("UGOITE_DEV_PASSKEY_CONTEXT");
    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"ok":true}"#);
    http_post_with_dev_auth_proxy(&format!("{base}/auth/mock-oauth"), &json!({}))
        .await
        .expect("malformed cached auth file request should succeed");
    let request_text = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("malformed cached auth file request text");
    handle
        .join()
        .expect("join malformed cached auth file request server");

    assert!(request_text
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-auth-proxy-token: proxy-secret\r\n"));
    assert!(!request_text
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-passkey-context:"));
}

/// REQ-OPS-006: explicit auth POST helpers must surface proxy-auth errors with response text.
#[tokio::test]
async fn test_cli_req_ops_006_http_helpers_surface_dev_auth_proxy_error_bodies() {
    let _guard = env_lock().lock().expect("env lock");
    let _env = EnvState::capture();

    std::env::set_var("UGOITE_DEV_AUTH_PROXY_TOKEN", "proxy-secret");
    std::env::set_var("UGOITE_DEV_PASSKEY_CONTEXT", "passkey-context");
    let (base, requests, handle) =
        spawn_recording_server("HTTP/1.1 403 Forbidden", "proxy auth failed");
    let post_err = http_post_with_dev_auth_proxy(&format!("{base}/auth/mock-oauth"), &json!({}))
        .await
        .expect_err("http_post_with_dev_auth_proxy should fail");
    let proxy_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("proxy error request text");
    handle.join().expect("join proxy error server");
    assert!(proxy_request
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-auth-proxy-token: proxy-secret\r\n"));
    assert!(proxy_request
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-passkey-context: passkey-context\r\n"));
    assert!(post_err.to_string().contains("HTTP 403"));
    assert!(post_err.to_string().contains("proxy auth failed"));
}
