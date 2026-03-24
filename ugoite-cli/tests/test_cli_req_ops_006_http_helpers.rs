//! CLI HTTP helper coverage tests.
//! REQ-OPS-006

mod support;

use serde_json::json;
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
}

impl EnvState {
    fn capture() -> Self {
        Self {
            bearer: std::env::var("UGOITE_AUTH_BEARER_TOKEN").ok(),
            api_key: std::env::var("UGOITE_AUTH_API_KEY").ok(),
            dev_auth_proxy_token: std::env::var("UGOITE_DEV_AUTH_PROXY_TOKEN").ok(),
        }
    }
}

impl Drop for EnvState {
    fn drop(&mut self) {
        std::env::remove_var("UGOITE_AUTH_BEARER_TOKEN");
        std::env::remove_var("UGOITE_AUTH_API_KEY");
        std::env::remove_var("UGOITE_DEV_AUTH_PROXY_TOKEN");
        if let Some(value) = &self.bearer {
            std::env::set_var("UGOITE_AUTH_BEARER_TOKEN", value);
        }
        if let Some(value) = &self.api_key {
            std::env::set_var("UGOITE_AUTH_API_KEY", value);
        }
        if let Some(value) = &self.dev_auth_proxy_token {
            std::env::set_var("UGOITE_DEV_AUTH_PROXY_TOKEN", value);
        }
    }
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

/// REQ-OPS-006: explicit auth POST helpers must only send the dev proxy header when configured.
#[tokio::test]
async fn test_cli_req_ops_006_http_helpers_apply_dev_auth_proxy_header() {
    let _guard = env_lock().lock().expect("env lock");
    let _env = EnvState::capture();

    std::env::set_var("UGOITE_DEV_AUTH_PROXY_TOKEN", "   ");
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

    std::env::set_var("UGOITE_DEV_AUTH_PROXY_TOKEN", "proxy-secret");
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
}

/// REQ-OPS-006: explicit auth POST helpers must surface proxy-auth errors with response text.
#[tokio::test]
async fn test_cli_req_ops_006_http_helpers_surface_dev_auth_proxy_error_bodies() {
    let _guard = env_lock().lock().expect("env lock");
    let _env = EnvState::capture();

    std::env::set_var("UGOITE_DEV_AUTH_PROXY_TOKEN", "proxy-secret");
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
    assert!(post_err.to_string().contains("HTTP 403"));
    assert!(post_err.to_string().contains("proxy auth failed"));
}
