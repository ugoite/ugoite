use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use base64::{engine::general_purpose, Engine as _};
use serde_json::Value;
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::sync::OnceLock;
use tokio::sync::Mutex;
use tower::ServiceExt;
use ugoite_iceberg::{authorization::Authorizer, integrity::RealIntegrityProvider, space};
use ugoite_server::{app, AppState};
use ugoite_storage::{operator_from_uri, OpendalStorage, StorageBackend};
use uuid::Uuid;

static APP_ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

struct EnvVarGuard {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }

    fn unset(key: &'static str) -> Self {
        let previous = std::env::var_os(key);
        std::env::remove_var(key);
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

async fn initialized_app(name: &str) -> axum::Router {
    let _lock = APP_ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    initialized_app_without_env_lock(name).await
}

async fn initialized_app_without_env_lock(name: &str) -> axum::Router {
    let state = AppState::new_for_tests(format!("memory://server-contract-{name}")).expect("state");
    state
        .initialize_node()
        .await
        .expect("initialize Node Identity");
    app(state)
}

async fn json(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json")
}

fn assert_common_security_headers(response: &axum::response::Response, hsts: bool) {
    let headers = response.headers();
    assert_eq!(
        headers.get("x-content-type-options"),
        Some(&"nosniff".parse().unwrap())
    );
    assert_eq!(
        headers.get("x-frame-options"),
        Some(&"DENY".parse().unwrap())
    );
    assert_eq!(
        headers.get("referrer-policy"),
        Some(&"strict-origin-when-cross-origin".parse().unwrap())
    );
    assert_eq!(
        headers.get("permissions-policy"),
        Some(&"camera=(), microphone=(), geolocation=()".parse().unwrap())
    );
    let csp = headers
        .get("content-security-policy")
        .and_then(|value| value.to_str().ok())
        .expect("CSP header");
    assert!(csp.contains("default-src 'self'"));
    assert!(csp.contains("script-src 'self'"));
    assert!(csp.contains("img-src 'self' blob:"));
    assert_eq!(headers.contains_key("strict-transport-security"), hsts);
}

async fn assert_response_signature(
    response: axum::response::Response,
    storage: &OpendalStorage,
    key_path: &str,
) -> Vec<u8> {
    let key_id = response
        .headers()
        .get("x-ugoite-key-id")
        .expect("response key ID")
        .to_str()
        .expect("response key ID is valid UTF-8")
        .to_string();
    let signature = response
        .headers()
        .get("x-ugoite-signature")
        .expect("response signature")
        .to_str()
        .expect("response signature is valid UTF-8")
        .to_string();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let payload: Value = serde_json::from_slice(&storage.read(key_path).await.unwrap())
        .expect("response HMAC payload");
    let stored_key_id = payload["hmac_key_id"].as_str().expect("key ID");
    let secret = general_purpose::STANDARD
        .decode(payload["hmac_key"].as_str().expect("secret"))
        .expect("base64 HMAC secret");
    assert_eq!(key_id, stored_key_id);
    assert_eq!(
        signature,
        RealIntegrityProvider::new(secret).signature_bytes(&body)
    );
    assert!(!body.windows(8).any(|window| window == b"hmac_key"));
    body.to_vec()
}

fn test_storage(name: &str) -> OpendalStorage {
    let operator =
        operator_from_uri(&format!("memory://server-contract-{name}")).expect("test operator");
    OpendalStorage::from_operator(&operator)
}

async fn create_test_space(name: &str, space_id: &str) {
    let operator =
        operator_from_uri(&format!("memory://server-contract-{name}")).expect("test operator");
    space::create_space(&operator, space_id, "/tmp")
        .await
        .expect("Space");
    let metadata = space::get_space(&operator, space_id)
        .await
        .expect("Space metadata");
    let space_uid = metadata.space_uid;
    Authorizer::new(operator)
        .initialize_owner(space_id, space_uid, Uuid::now_v7(), "Test owner")
        .await
        .expect("Space authorization state");
}

#[tokio::test]
/// REQ-INT-003
async fn req_int_003_signs_default_and_space_scoped_api_responses() {
    let name = "response-hmac-scopes";
    let storage = test_storage(name);
    create_test_space(name, "space-a").await;
    create_test_space(name, "space-b").await;
    let app = initialized_app(name).await;

    let default_response = app
        .clone()
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(default_response.status(), StatusCode::OK);
    assert_response_signature(default_response, &storage, "response_hmac/default.json").await;
    assert!(!storage.exists("spaces/default/hmac.json").await.unwrap());

    for space_id in ["space-a", "space-b"] {
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/spaces/{space_id}/health"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::LOCKED);
        assert_response_signature(response, &storage, &format!("spaces/{space_id}/hmac.json"))
            .await;
    }
}

#[tokio::test]
/// REQ-INT-003
async fn req_int_003_signs_head_and_empty_bodies() {
    let name = "response-hmac-head";
    let storage = test_storage(name);
    let app = initialized_app(name).await;
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::HEAD)
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = assert_response_signature(response, &storage, "response_hmac/default.json").await;
    assert!(body.is_empty());
}

#[tokio::test]
/// REQ-INT-003
async fn req_int_003_fails_closed_for_unknown_space_and_key_errors() {
    let name = "response-hmac-fail-closed";
    let storage = test_storage(name);
    let app = initialized_app(name).await;
    let unknown = app
        .clone()
        .oneshot(
            Request::get("/spaces/unknown-space/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unknown.status(), StatusCode::LOCKED);
    assert!(unknown.headers().get("x-ugoite-signature").is_none());
    assert_common_security_headers(&unknown, false);
    assert!(!storage.exists("spaces/unknown-space/").await.unwrap());

    storage.create_dir("response_hmac/").await.unwrap();
    storage
        .write("response_hmac/default.json", b"{}".to_vec())
        .await
        .unwrap();
    let failed = app
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(failed.status(), StatusCode::OK);
    assert!(failed.headers().get("x-ugoite-signature").is_none());
    assert_common_security_headers(&failed, false);
    assert_eq!(json(failed).await["status"], "ok");
}

#[tokio::test]
/// REQ-INT-003
async fn req_int_003_removes_the_pre_v1_mcp_route() {
    let name = "response-hmac-encoded";
    create_test_space(name, "encoded-space").await;
    let app = initialized_app(name).await;
    let response = app
        .oneshot(
            Request::get("/mcp/resources/encoded%2Dspace/entries/list")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn mcp_v1_transport_rejects_legacy_methods_and_validates_the_wire_shape() {
    let app = initialized_app("mcp-v1-transport").await;
    let legacy = app
        .clone()
        .oneshot(Request::get("/mcp").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(legacy.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(legacy.headers().get("allow").unwrap(), "POST");

    let parse_error = app
        .clone()
        .oneshot(
            Request::post("/mcp")
                .header("content-type", "application/json")
                .body(Body::from("{"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(parse_error.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json(parse_error).await["error"]["code"], -32700);

    let missing_header = app
        .clone()
        .oneshot(
            Request::post("/mcp")
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .header("mcp-protocol-version", "2026-07-28")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_header.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json(missing_header).await["error"]["code"], -32020);

    let unauthenticated = app
        .clone()
        .oneshot(
            Request::post("/mcp")
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .header("mcp-protocol-version", "2026-07-28")
                .header("mcp-method", "server/discover")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        json(unauthenticated).await,
        serde_json::json!({"code":"AUTHENTICATION_REQUIRED","message":"MCP authentication is required"})
    );

    let invalid_resource = app
        .oneshot(
            Request::post("/mcp")
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .header("mcp-protocol-version", "2026-07-28")
                .header("mcp-method", "resources/read")
                .header("mcp-name", "ugoite://entry/../history")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","id":"resource","method":"resources/read","params":{"uri":"ugoite://entry/../history","_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid_resource.status(), StatusCode::OK);
    assert_eq!(json(invalid_resource).await["error"]["code"], -32602);
}

#[tokio::test]
async fn mcp_accepts_configured_cross_origin_clients_and_preflight_headers() {
    let _lock = APP_ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _cors_origins = EnvVarGuard::set("UGOITE_CORS_ALLOWED_ORIGINS", "https://frontend.example");
    let app = initialized_app_without_env_lock("mcp-cors").await;
    let preflight = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/mcp")
                .header("origin", "https://frontend.example")
                .header("access-control-request-method", "POST")
                .header(
                    "access-control-request-headers",
                    "Accept, Content-Type, MCP-Method, MCP-Name, MCP-Protocol-Version",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preflight.status(), StatusCode::OK);
    assert_eq!(
        preflight.headers().get("access-control-allow-origin"),
        Some(&"https://frontend.example".parse().unwrap())
    );

    let actual = app
        .oneshot(
            Request::post("/mcp")
                .header("origin", "https://frontend.example")
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .header("mcp-protocol-version", "2026-07-28")
                .header("mcp-method", "server/discover")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(actual.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        actual.headers().get("access-control-allow-origin"),
        Some(&"https://frontend.example".parse().unwrap())
    );
}

#[tokio::test]
async fn req_sec_002_covers_metadata_api_error_and_middleware_responses() {
    let app = initialized_app("security-headers").await;
    for uri in [
        "/",
        "/health",
        "/.well-known/oauth-protected-resource",
        "/missing",
    ] {
        let response = app
            .clone()
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_common_security_headers(&response, false);
    }

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/auth/setup/start")
                .header("cookie", "ugoite_session=present")
                .header("origin", "https://attacker.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_common_security_headers(&response, false);
}

#[tokio::test]
async fn asset_upload_route_is_not_public_before_authentication() {
    let app = initialized_app("asset-upload-auth").await;
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/spaces/remote-space/assets")
                .header(
                    "content-type",
                    "multipart/form-data; boundary=asset-auth-boundary",
                )
                .body(Body::from(
                    "--asset-auth-boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"asset.bin\"\r\nContent-Type: application/octet-stream\r\n\r\ncontent\r\n--asset-auth-boundary--\r\n",
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::LOCKED);
}

#[tokio::test]
async fn req_sec_002_covers_the_static_browser_root() {
    let _lock = APP_ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let static_dir = std::env::temp_dir().join(format!(
        "ugoite-security-headers-static-{}",
        uuid::Uuid::now_v7()
    ));
    std::fs::create_dir_all(&static_dir).unwrap();
    std::fs::write(static_dir.join("index.html"), "<!doctype html>").unwrap();
    let _static_dir = EnvVarGuard::set("UGOITE_STATIC_DIR", &static_dir);
    let app = initialized_app_without_env_lock("security-headers-static").await;
    let response = app
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    std::fs::remove_dir_all(static_dir).unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_common_security_headers(&response, false);
}

#[tokio::test]
async fn req_sec_002_keeps_security_headers_on_cors_preflight() {
    let _lock = APP_ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _cors_origins = EnvVarGuard::set("UGOITE_CORS_ALLOWED_ORIGINS", "https://frontend.example");
    let app = initialized_app_without_env_lock("security-headers-cors").await;
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/health")
                .header("origin", "https://frontend.example")
                .header("access-control-request-method", "GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(response.status().is_success());
    assert_eq!(
        response.headers().get("access-control-allow-origin"),
        Some(&"https://frontend.example".parse().unwrap())
    );
    assert_eq!(
        response.headers().get("access-control-allow-credentials"),
        Some(&"true".parse().unwrap())
    );
    assert_common_security_headers(&response, false);
}

#[tokio::test]
async fn req_sec_010_allows_configured_preflight_method_and_header_contract() {
    let _lock = APP_ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _cors_origins = EnvVarGuard::set("UGOITE_CORS_ALLOWED_ORIGINS", "https://frontend.example");
    let app = initialized_app_without_env_lock("cors-preflight-contract").await;
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/health")
                .header("origin", "https://frontend.example")
                .header("access-control-request-method", "POST")
                .header(
                    "access-control-request-headers",
                    "Accept, Authorization, Content-Type, Idempotency-Key, DPoP, X-Request-Id, MCP-Method, MCP-Name, MCP-Protocol-Version",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("access-control-allow-origin"),
        Some(&"https://frontend.example".parse().unwrap())
    );
    assert_eq!(
        response.headers().get("access-control-allow-credentials"),
        Some(&"true".parse().unwrap())
    );
    let allow_methods = response
        .headers()
        .get("access-control-allow-methods")
        .expect("configured CORS methods")
        .to_str()
        .unwrap()
        .split(',')
        .map(str::trim)
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        allow_methods,
        BTreeSet::from([
            "DELETE".to_owned(),
            "GET".to_owned(),
            "OPTIONS".to_owned(),
            "PATCH".to_owned(),
            "POST".to_owned(),
            "PUT".to_owned(),
        ])
    );
    let allow_headers = response
        .headers()
        .get("access-control-allow-headers")
        .expect("configured CORS request headers")
        .to_str()
        .unwrap()
        .split(',')
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        allow_headers,
        BTreeSet::from([
            "accept".to_owned(),
            "authorization".to_owned(),
            "content-type".to_owned(),
            "dpop".to_owned(),
            "idempotency-key".to_owned(),
            "mcp-method".to_owned(),
            "mcp-name".to_owned(),
            "mcp-protocol-version".to_owned(),
            "x-request-id".to_owned(),
            "x-ugoite-human-approval".to_owned(),
        ])
    );
    assert!(!allow_methods.contains("*"));
    assert!(!allow_headers.contains("*"));
}

#[tokio::test]
async fn req_sec_010_allows_credentials_for_an_allowed_safe_response() {
    let _lock = APP_ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _cors_origins = EnvVarGuard::set("UGOITE_CORS_ALLOWED_ORIGINS", "https://frontend.example");
    let app = initialized_app_without_env_lock("cors-credentialed-safe-response").await;
    let response = app
        .oneshot(
            Request::get("/health")
                .header("origin", "https://frontend.example")
                .header("cookie", "ugoite_session=opaque")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("access-control-allow-origin"),
        Some(&"https://frontend.example".parse().unwrap())
    );
    assert_eq!(
        response.headers().get("access-control-allow-credentials"),
        Some(&"true".parse().unwrap())
    );
}

#[tokio::test]
async fn req_sec_010_omits_allow_origin_for_unlisted_actual_and_preflight_requests() {
    let _lock = APP_ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _cors_origins = EnvVarGuard::set("UGOITE_CORS_ALLOWED_ORIGINS", "https://frontend.example");
    let app = initialized_app_without_env_lock("cors-unlisted-origin").await;

    let actual_response = app
        .clone()
        .oneshot(
            Request::get("/health")
                .header("origin", "https://unlisted.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(actual_response.status(), StatusCode::OK);
    assert!(!actual_response
        .headers()
        .contains_key("access-control-allow-origin"));

    let preflight_response = app
        .oneshot(
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/health")
                .header("origin", "https://unlisted.example")
                .header("access-control-request-method", "GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(!preflight_response
        .headers()
        .contains_key("access-control-allow-origin"));
}

#[tokio::test]
async fn req_sec_010_keeps_cors_disabled_when_origin_allowlist_is_unset() {
    let _lock = APP_ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _cors_origins = EnvVarGuard::unset("UGOITE_CORS_ALLOWED_ORIGINS");
    let app = initialized_app_without_env_lock("cors-default-off").await;
    let response = app
        .oneshot(
            Request::get("/health")
                .header("origin", "https://frontend.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response
        .headers()
        .contains_key("access-control-allow-origin"));
    assert!(!response
        .headers()
        .contains_key("access-control-allow-credentials"));
}

#[tokio::test]
async fn req_sec_010_keeps_canonical_origin_csrf_guard_separate_from_cors_allowlist() {
    let _lock = APP_ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _cors_origins = EnvVarGuard::set("UGOITE_CORS_ALLOWED_ORIGINS", "https://frontend.example");
    let app = initialized_app_without_env_lock("cors-csrf-boundary").await;
    let response = app
        .oneshot(
            Request::post("/auth/setup/start")
                .header("origin", "https://frontend.example")
                .header("cookie", "ugoite_session=opaque")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        response.headers().get("access-control-allow-origin"),
        Some(&"https://frontend.example".parse().unwrap())
    );
}

#[tokio::test]
async fn uninitialized_node_exposes_setup_capability_without_default_identity() {
    let app = initialized_app("config").await;
    let response = app
        .oneshot(Request::get("/auth/config").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = json(response).await;
    assert_eq!(body["status"], "uninitialized");
    assert_eq!(body["passkey"], true);
    assert!(body.get("username_hint").is_none());
}

#[tokio::test]
async fn oidc_links_are_protected_and_provider_listing_is_publicly_redacted() {
    let app = initialized_app("oidc-boundary").await;
    let providers = app
        .clone()
        .oneshot(
            Request::get("/auth/oidc/providers")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(providers.status(), StatusCode::OK);
    assert_eq!(json(providers).await, serde_json::json!([]));

    for request in [
        Request::get("/auth/oidc/links")
            .body(Body::empty())
            .unwrap(),
        Request::get("/auth/oidc/links")
            .header("authorization", "Bearer upstream-token")
            .body(Body::empty())
            .unwrap(),
        Request::delete(format!("/auth/oidc/links/{}", Uuid::now_v7()))
            .body(Body::empty())
            .unwrap(),
    ] {
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::LOCKED);
    }
}

#[tokio::test]
async fn uninitialized_protected_routes_are_locked_for_every_credential_shape() {
    let app = initialized_app("protected").await;
    for request in [
        Request::get("/spaces").body(Body::empty()).unwrap(),
        Request::get("/spaces")
            .header("authorization", "Bearer obsolete")
            .body(Body::empty())
            .unwrap(),
        Request::get("/spaces")
            .header("x-api-key", "obsolete")
            .body(Body::empty())
            .unwrap(),
    ] {
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::LOCKED);
    }
}

#[tokio::test]
async fn invitation_finish_cannot_issue_a_session_from_token_only() {
    let app = initialized_app("invitation-token-session").await;
    let response = app
        .oneshot(
            Request::post("/auth/invitations/finish")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"invitation_token":"token-only","resume":true}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(response.headers().get("set-cookie").is_none());
}

#[tokio::test]
async fn oauth_metadata_describes_device_and_dpop_surface() {
    let app = initialized_app("metadata").await;
    let response = app
        .clone()
        .oneshot(
            Request::get("/.well-known/oauth-protected-resource")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = json(response).await;
    assert_eq!(body["resource"], "http://localhost:8000/mcp");
    let documentation = body["resource_documentation"]
        .as_str()
        .expect("resource documentation URL");
    let documentation_url =
        url::Url::parse(documentation).expect("valid resource documentation URL");
    assert_eq!(documentation_url.scheme(), "https");
    assert_eq!(documentation_url.host_str(), Some("ugoite.github.io"));
    assert_eq!(
        documentation_url.path(),
        "/ugoite/docs/guide/operate/auth/auth-overview/"
    );
    assert_eq!(
        documentation,
        "https://ugoite.github.io/ugoite/docs/guide/operate/auth/auth-overview/"
    );
    let response = app
        .oneshot(
            Request::get("/.well-known/oauth-authorization-server")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = json(response).await;
    assert_eq!(body["dpop_signing_alg_values_supported"][0], "ES256");
    assert!(body["grant_types_supported"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "urn:ietf:params:oauth:grant-type:device_code"));
}

#[test]
fn openapi_does_not_publish_removed_credentials() {
    let snapshot = ugoite_server::openapi_snapshot();
    assert!(snapshot.pointer("/paths/~1auth~1login").is_none());
    assert!(snapshot.pointer("/paths/~1auth~1passkey~1start").is_some());
    assert!(snapshot.pointer("/paths/~1oauth~1token").is_some());
}

#[test]
fn openapi_publishes_oidc_account_linking_and_bootstrap_surfaces() {
    let snapshot = ugoite_server::openapi_snapshot();
    for path in [
        "/auth/oidc/providers",
        "/auth/oidc/providers/{provider_id}",
        "/auth/oidc/links",
        "/auth/oidc/links/{method_id}",
        "/auth/oidc/{provider_id}/start",
        "/auth/oidc/{provider_id}/link",
        "/auth/oidc/callback",
        "/auth/passkeys/bootstrap/start",
        "/auth/passkeys/bootstrap/finish",
    ] {
        assert!(snapshot["paths"].get(path).is_some(), "missing {path}");
    }
    assert_eq!(
        snapshot["paths"]["/auth/oidc/links"]["get"]["responses"]["200"]["content"]
            ["application/json"]["schema"]["type"],
        "array"
    );
    assert_eq!(
        snapshot["paths"]["/auth/oidc/links/{method_id}"]["delete"]["responses"]["204"]
            ["description"],
        "OIDC identity unlinked"
    );
}

#[test]
fn req_sec_004_openapi_publishes_passkey_surfaces() {
    let snapshot = ugoite_server::openapi_snapshot();
    for path in [
        "/auth/config",
        "/auth/passkey/start",
        "/auth/passkey/finish",
        "/auth/passkeys",
        "/auth/passkeys/start",
        "/auth/passkeys/finish",
    ] {
        assert!(snapshot["paths"].get(path).is_some(), "missing {path}");
    }
    assert_eq!(
        snapshot["paths"]["/auth/passkey/start"]["post"]["responses"]["200"]["description"],
        "Success"
    );
}

#[test]
fn req_sec_005_openapi_publishes_setup_surfaces() {
    let snapshot = ugoite_server::openapi_snapshot();
    for path in [
        "/auth/config",
        "/auth/setup/start",
        "/auth/setup/finish",
        "/auth/passkeys/bootstrap/start",
        "/auth/passkeys/bootstrap/finish",
    ] {
        assert!(snapshot["paths"].get(path).is_some(), "missing {path}");
    }
    assert_eq!(
        snapshot["paths"]["/auth/setup/finish"]["post"]["responses"]["200"]["description"],
        "Success"
    );
}

#[test]
fn req_sec_014_openapi_publishes_account_recovery_surfaces() {
    let snapshot = ugoite_server::openapi_snapshot();
    for (path, method, response) in [
        ("/auth/recovery/totp/start", "post", "200"),
        ("/auth/recovery/totp/finish", "post", "200"),
        ("/auth/recovery/start", "post", "200"),
        ("/auth/recovery/finish", "post", "201"),
        ("/auth/audit", "get", "200"),
    ] {
        assert!(
            snapshot["paths"][path][method].is_object(),
            "{method} {path}"
        );
        assert!(
            snapshot["paths"][path][method]["responses"][response].is_object(),
            "{method} {path} publishes {response}"
        );
    }
}

#[test]
fn openapi_documents_resource_bound_mcp_agent_tokens() {
    let snapshot = ugoite_server::openapi_snapshot();
    for path in [
        "/oauth/token",
        "/oauth/device/authorization",
        "/oauth/agent/token",
        "/spaces/{space_id}/agents/{agent_id}/delegated-token",
    ] {
        let schema = &snapshot["paths"][path]["post"]["requestBody"]["content"]["application/json"]
            ["schema"];
        assert_eq!(schema["properties"]["resource"]["format"], "uri", "{path}");
        assert!(
            schema["properties"]["resource"]["description"]
                .as_str()
                .is_some_and(|description| description.contains("{issuer}/mcp")),
            "{path}"
        );
    }
    assert_eq!(
        snapshot["paths"]["/oauth/token"]["post"]["summary"],
        "Issue an access token, optionally bound to the MCP resource"
    );
}

#[test]
fn openapi_publishes_read_only_space_health() {
    let snapshot = ugoite_server::openapi_snapshot();
    let health = snapshot
        .pointer("/paths/~1spaces~1{space_id}~1health/get")
        .expect("Space health endpoint");
    assert!(health["summary"]
        .as_str()
        .expect("summary")
        .contains("Read-only"));
    assert_eq!(
        health["parameters"][1]["name"], "checkpoint",
        "health validates only caller-named checkpoints"
    );
}

#[test]
/// REQ-STO-009
fn test_space_req_sto_009_openapi_documents_authenticated_space_listing() {
    let snapshot = ugoite_server::openapi_snapshot();
    let operation = &snapshot["paths"]["/spaces"]["get"];

    assert_eq!(
        operation["summary"],
        "List spaces visible to the authenticated identity"
    );
    assert_eq!(
        operation["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/JsonValue"
    );
    assert_eq!(
        operation["responses"]["500"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/ErrorResponse"
    );
}

#[test]
fn issue_2125_openapi_documents_entry_list_and_keyword_search_bounds() {
    let snapshot = ugoite_server::openapi_snapshot();
    let limit = &snapshot["components"]["parameters"]["Limit"];
    assert_eq!(limit["schema"]["minimum"], 0);
    assert_eq!(limit["schema"]["maximum"], 10_000);
    assert_eq!(limit["schema"]["default"], 100);

    let search_query = &snapshot["components"]["parameters"]["KeywordSearchQuery"];
    assert_eq!(search_query["name"], "q");
    assert_eq!(search_query["required"], true);
    assert_eq!(search_query["schema"]["maxLength"], 8_192);
    assert!(
        snapshot["paths"]["/spaces/{space_id}/search"]["get"]["parameters"]
            .as_array()
            .expect("search parameters")
            .iter()
            .any(|parameter| parameter["$ref"] == "#/components/parameters/KeywordSearchQuery")
    );
}

#[test]
fn issue_2038_openapi_uses_head_owned_pins_for_immutable_knowledge_reads() {
    let snapshot = ugoite_server::openapi_snapshot();
    assert!(snapshot["paths"]["/spaces/{space_id}/pins/diff"].is_object());
    assert!(snapshot["paths"]["/spaces/{space_id}/checkpoints"].is_null());
    for path in [
        "/spaces/{space_id}/entries/{entry_id}",
        "/spaces/{space_id}/entries/{entry_id}/history",
        "/spaces/{space_id}/entries/{entry_id}/history/{revision_id}",
    ] {
        let parameters = snapshot["paths"][path]["get"]["parameters"]
            .as_array()
            .expect("entry read parameters");
        assert!(parameters
            .iter()
            .any(|parameter| { parameter["$ref"] == "#/components/parameters/Pin" }));
    }
    assert_eq!(
        snapshot["components"]["schemas"]["EntryRestoreRequest"]["properties"]["pin"]["type"],
        "string"
    );
    assert!(
        snapshot["components"]["schemas"]["EntryRestoreRequest"]["properties"]["checkpoint"]
            .is_null()
    );
}

#[test]
fn openapi_human_approval_is_server_derived_and_single_use() {
    let snapshot = ugoite_server::openapi_snapshot();
    let request = snapshot
        .pointer("/components/schemas/HumanApprovalIssueRequest")
        .expect("human approval issue request schema");
    assert_eq!(request["oneOf"].as_array().unwrap().len(), 4);
    assert_eq!(
        request["oneOf"][0],
        serde_json::json!({
            "$ref": "#/components/schemas/HumanApprovalEntryDeleteRequest"
        })
    );
    assert_eq!(
        snapshot["components"]["schemas"]["HumanApprovalEntryDeleteRequest"]["properties"]
            ["operation"]["const"],
        "entry.delete"
    );
    assert_eq!(
        snapshot["components"]["schemas"]["HumanApprovalAccessPutRequest"]["properties"]
            ["operation"]["const"],
        "access.put"
    );
    assert_eq!(
        snapshot["components"]["schemas"]["HumanApprovalResponse"]["properties"]["approval_token"]
            ["readOnly"],
        true
    );
    assert_eq!(
        snapshot["components"]["schemas"]["HumanApprovalResponse"]["properties"]["audit_status"]
            ["$ref"],
        "#/components/schemas/AuditStatus"
    );
    assert_eq!(
        snapshot["components"]["schemas"]["HumanApprovalDeleteMutation"]["required"],
        serde_json::json!(["target_id"])
    );
    assert_eq!(
        snapshot["components"]["schemas"]["HumanApprovalEntryDeleteMutation"]["required"],
        serde_json::json!(["target_id", "hard_delete"])
    );
    let access_put = &snapshot["paths"]["/spaces/{space_id}/policies/{kind}/{resource_id}"]["put"];
    assert_eq!(
        access_put["parameters"].as_array().unwrap().len(),
        4,
        "access.put must describe every path and approval header parameter"
    );
    for status in ["400", "409", "410", "500"] {
        assert!(access_put["responses"].get(status).is_some());
    }
}

#[test]
fn issue_2037_openapi_publishes_the_public_knowledge_contract() {
    let snapshot = ugoite_server::openapi_snapshot();
    let paths = &snapshot["paths"];

    for path in [
        "/spaces/{space_id}/changes",
        "/spaces/{space_id}/changes/{change_id}/revert",
        "/spaces/{space_id}/runs/{run_id}/undo",
        "/spaces/{space_id}/apply",
        "/spaces/{space_id}/pins",
        "/spaces/{space_id}/pins/{pin_name}",
    ] {
        assert!(paths.get(path).is_some(), "missing public route {path}");
    }

    assert_eq!(
        paths["/spaces/{space_id}/changes/{change_id}/revert"]["post"]["responses"]["409"]
            ["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/ErrorResponse"
    );

    let apply = &paths["/spaces/{space_id}/apply"]["post"]["requestBody"];
    assert_eq!(
        apply["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/ApplyRequest"
    );
    assert_eq!(
        snapshot["components"]["schemas"]["ApplyRequest"]["additionalProperties"],
        false
    );
    assert_eq!(
        snapshot["components"]["schemas"]["ApplyUpdate"]["properties"]["version_token"]["type"],
        "string"
    );
    assert_eq!(
        snapshot["components"]["schemas"]["RunUndoRequest"]["additionalProperties"],
        false
    );
    assert_eq!(
        snapshot["components"]["schemas"]["ChangeRevertRequest"]["additionalProperties"],
        false
    );
    assert_eq!(
        snapshot["components"]["schemas"]["PinCreate"]["additionalProperties"],
        false
    );
}

#[test]
fn issue_2029_openapi_documents_storage_connection_contract() {
    let snapshot = ugoite_server::openapi_snapshot();
    let patch = &snapshot["paths"]["/spaces/{space_id}"]["patch"];
    assert_eq!(
        patch["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/SpacePatchRequest"
    );
    assert!(
        snapshot["components"]["schemas"]["SpacePatchRequest"]["properties"]["storage_config"]
            .is_object(),
        "Space patch accepts connector metadata"
    );

    let operation = &snapshot["paths"]["/spaces/{space_id}/test-connection"]["post"];

    assert_eq!(
        operation["requestBody"]["required"], true,
        "storage connection request body is required"
    );
    assert_eq!(
        operation["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/StorageConnectionTestRequest"
    );
    assert_eq!(
        operation["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/StorageConnectionTestResponse"
    );

    let request = &snapshot["components"]["schemas"]["StorageConnectionTestRequest"];
    for properties in [
        &request["properties"],
        &request["properties"]["storage_config"]["properties"],
    ] {
        assert_eq!(properties["endpoint"]["type"], "string");
        assert_eq!(properties["endpoint"]["format"], "uri");
    }
}

#[test]
/// REQ-INT-003
fn openapi_publishes_response_signing_headers_and_unsigned_boundary() {
    let snapshot = ugoite_server::openapi_snapshot();
    let signing = &snapshot["x-ugoite-response-signing"];
    assert_eq!(
        signing["headers"]["key_id"]["$ref"],
        "#/components/headers/X-Ugoite-Key-Id"
    );
    assert_eq!(
        signing["headers"]["signature"]["$ref"],
        "#/components/headers/X-Ugoite-Signature"
    );
    assert_eq!(
        snapshot["components"]["headers"]["X-Ugoite-Signature"]["schema"]["pattern"],
        "^[0-9a-f]{64}$"
    );
    for response in [
        &snapshot["paths"]["/health"]["get"]["responses"]["200"],
        &snapshot["paths"]["/spaces/{space_id}/health"]["get"]["responses"]["200"],
    ] {
        assert_eq!(
            response["headers"]["X-Ugoite-Key-Id"]["$ref"],
            "#/components/headers/X-Ugoite-Key-Id"
        );
        assert_eq!(
            response["headers"]["X-Ugoite-Signature"]["$ref"],
            "#/components/headers/X-Ugoite-Signature"
        );
    }
    assert!(signing["unsigned"]
        .as_array()
        .expect("unsigned response classes")
        .iter()
        .any(|value| value == "static files"));
}
