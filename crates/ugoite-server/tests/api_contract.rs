use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::Value;
use std::{fs, time::SystemTime};
use tower::ServiceExt;
use ugoite_server::{app, openapi_snapshot, AppState};

fn authenticated(request: Request<Body>) -> Request<Body> {
    let (mut parts, body) = request.into_parts();
    parts
        .headers
        .insert("authorization", "Bearer test-token".parse().unwrap());
    Request::from_parts(parts, body)
}

#[tokio::test]
async fn health_and_openapi_are_public() {
    let response = app(AppState::new("memory://server-public").unwrap())
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(openapi_snapshot()["paths"]["/spaces"].is_object());
}

#[tokio::test]
async fn protected_routes_require_authentication() {
    let response = app(AppState::new("memory://server-auth").unwrap())
        .oneshot(Request::get("/spaces").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn bootstrap_default_space_from_env_is_idempotent() {
    std::env::set_var("UGOITE_BOOTSTRAP_DEFAULT_SPACE", "true");
    std::env::set_var(
        "UGOITE_AUTH_BEARER_TOKENS",
        r#"{"test-token":{"user_id":"test-user"}}"#,
    );
    let state = AppState::new("memory://server-bootstrap").unwrap();
    state.bootstrap_default_space_from_env().await.unwrap();
    state.bootstrap_default_space_from_env().await.unwrap();

    let response = app(state)
        .oneshot(authenticated(
            Request::get("/spaces").body(Body::empty()).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert!(body
        .as_array()
        .unwrap()
        .iter()
        .any(|space| space["name"] == "default"));
    std::env::remove_var("UGOITE_BOOTSTRAP_DEFAULT_SPACE");
}

#[tokio::test]
async fn static_dir_serves_frontend_and_api_prefix() {
    std::env::set_var(
        "UGOITE_AUTH_BEARER_TOKENS",
        r#"{"test-token":{"user_id":"test-user"}}"#,
    );
    let static_dir = std::env::temp_dir().join(format!(
        "ugoite-server-static-{}",
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&static_dir).unwrap();
    fs::write(
        static_dir.join("index.html"),
        "<!doctype html><html><body>Ugoite</body></html>",
    )
    .unwrap();
    std::env::set_var("UGOITE_STATIC_DIR", static_dir.as_os_str());

    std::env::set_var("UGOITE_BOOTSTRAP_DEFAULT_SPACE", "true");
    let state = AppState::new("memory://server-static").unwrap();
    state.bootstrap_default_space_from_env().await.unwrap();
    let router = app(state);
    std::env::remove_var("UGOITE_STATIC_DIR");
    std::env::remove_var("UGOITE_BOOTSTRAP_DEFAULT_SPACE");

    let page = router
        .clone()
        .oneshot(Request::get("/spaces").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    let content_type = page
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    assert!(content_type.contains("text/html"));
    let body = to_bytes(page.into_body(), usize::MAX).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("<!doctype html>"));

    let api = router
        .oneshot(authenticated(
            Request::get("/api/spaces").body(Body::empty()).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(api.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&to_bytes(api.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert!(body
        .as_array()
        .unwrap()
        .iter()
        .any(|space| space["name"] == "default"));

    fs::remove_dir_all(static_dir).unwrap();
}

#[tokio::test]
async fn space_entry_search_and_mcp_contract() {
    std::env::set_var(
        "UGOITE_AUTH_BEARER_TOKENS",
        r#"{"test-token":{"user_id":"test-user"}}"#,
    );
    let router = app(AppState::new("memory://server-contract").unwrap());
    let create_space = authenticated(
        Request::post("/spaces")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"name":"demo"}"#))
            .unwrap(),
    );
    assert_eq!(
        router.clone().oneshot(create_space).await.unwrap().status(),
        StatusCode::CREATED
    );

    let create_entry = authenticated(
        Request::post("/spaces/demo/entries")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"id":"first","markdown":"---\nform: Entry\n---\n# First\n\n## Body\nhello"}"#,
            ))
            .unwrap(),
    );
    assert_eq!(
        router.clone().oneshot(create_entry).await.unwrap().status(),
        StatusCode::CREATED
    );

    let list = authenticated(
        Request::get("/spaces/demo/entries")
            .body(Body::empty())
            .unwrap(),
    );
    let response = router.clone().oneshot(list).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body[0]["id"], "first");

    let mcp = authenticated(
        Request::get("/mcp/resources/demo/entries/list")
            .body(Body::empty())
            .unwrap(),
    );
    let response = router.oneshot(mcp).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
