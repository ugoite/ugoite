use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use serde_json::Value;
use std::sync::{Mutex, OnceLock};
use tower::ServiceExt;
use ugoite_server::{app, AppState};

static APP_ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

async fn initialized_app(name: &str) -> axum::Router {
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
async fn req_sec_002_covers_the_static_browser_root() {
    let _lock = APP_ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
    let static_dir = std::env::temp_dir().join(format!(
        "ugoite-security-headers-static-{}",
        uuid::Uuid::now_v7()
    ));
    std::fs::create_dir_all(&static_dir).unwrap();
    std::fs::write(static_dir.join("index.html"), "<!doctype html>").unwrap();
    let previous = std::env::var_os("UGOITE_STATIC_DIR");
    std::env::set_var("UGOITE_STATIC_DIR", &static_dir);
    let app = initialized_app("security-headers-static").await;
    let response = app
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    match previous {
        Some(value) => std::env::set_var("UGOITE_STATIC_DIR", value),
        None => std::env::remove_var("UGOITE_STATIC_DIR"),
    }
    std::fs::remove_dir_all(static_dir).unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_common_security_headers(&response, false);
}

#[tokio::test]
async fn req_sec_002_keeps_security_headers_on_cors_preflight() {
    let _lock = APP_ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
    let previous = std::env::var_os("UGOITE_CORS_ALLOWED_ORIGINS");
    std::env::set_var("UGOITE_CORS_ALLOWED_ORIGINS", "https://frontend.example");
    let app = initialized_app("security-headers-cors").await;
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
    match previous {
        Some(value) => std::env::set_var("UGOITE_CORS_ALLOWED_ORIGINS", value),
        None => std::env::remove_var("UGOITE_CORS_ALLOWED_ORIGINS"),
    }

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
