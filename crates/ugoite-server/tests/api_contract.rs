use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::Value;
use tower::ServiceExt;
use ugoite_server::{app, AppState};

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
    assert_eq!(json(response).await["resource"], "http://localhost:8000");
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
