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
async fn req_int_003_uses_decoded_mcp_space_scope() {
    let name = "response-hmac-encoded";
    let storage = test_storage(name);
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
    assert_eq!(response.status(), StatusCode::LOCKED);
    assert_response_signature(response, &storage, "spaces/encoded-space/hmac.json").await;
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
                    "Accept, Authorization, Content-Type, Idempotency-Key, DPoP, X-Request-Id",
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
            "x-request-id".to_owned(),
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
    assert_eq!(body["resource"], "http://localhost:8000");
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
