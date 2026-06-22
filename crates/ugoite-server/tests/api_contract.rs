#![allow(clippy::await_holding_lock)]

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::Value;
use std::{
    fs,
    sync::{Mutex, OnceLock},
    time::SystemTime,
};
use tower::ServiceExt;
use ugoite_core::service::UgoiteService;
use ugoite_server::{app, openapi_snapshot, AppState};

const AUTH_FIXTURE_TOKENS: &str = r#"{
    "test-token":{"user_id":"test-user"},
    "alice-token":{"user_id":"alice"},
    "bob-token":{"user_id":"bob"},
    "dev-token":{"user_id":"dev-local-user"},
    "scoped-token":{"user_id":"test-user","principal_type":"service","scopes":["space_read"],"scope_enforced":true,"service_account_id":"svc-readonly"}
}"#;

fn set_auth_fixture() {
    std::env::set_var("UGOITE_AUTH_BEARER_TOKENS", AUTH_FIXTURE_TOKENS);
}

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn authenticated(request: Request<Body>) -> Request<Body> {
    let (mut parts, body) = request.into_parts();
    parts
        .headers
        .insert("authorization", "Bearer test-token".parse().unwrap());
    Request::from_parts(parts, body)
}

fn authenticated_with(request: Request<Body>, token: &str) -> Request<Body> {
    let (mut parts, body) = request.into_parts();
    parts
        .headers
        .insert("authorization", format!("Bearer {token}").parse().unwrap());
    Request::from_parts(parts, body)
}

async fn response_json(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&body).unwrap_or_else(|error| {
        panic!(
            "response body was not valid JSON: {error}; body={}",
            String::from_utf8_lossy(&body)
        )
    })
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
async fn openapi_snapshot_has_methods_for_runtime_routes() {
    let snapshot = openapi_snapshot();
    let paths = snapshot["paths"].as_object().expect("paths object");
    assert!(!paths.is_empty());
    for (path, value) in paths {
        assert!(
            value.as_object().is_some_and(|object| !object.is_empty()),
            "OpenAPI path object must not be empty: {path}"
        );
    }

    for (path, method) in [
        ("/spaces", "get"),
        ("/spaces", "post"),
        ("/spaces/{space_id}", "patch"),
        (
            "/spaces/{space_id}/entries/{entry_id}/history/{revision_id}",
            "get",
        ),
        ("/spaces/{space_id}/entries/{entry_id}/restore", "post"),
        ("/spaces/{space_id}/forms", "post"),
        ("/spaces/{space_id}/sql-sessions", "post"),
        ("/spaces/{space_id}/assets/{asset_id}", "delete"),
    ] {
        assert!(
            paths
                .get(path)
                .and_then(|methods| methods.get(method))
                .is_some(),
            "OpenAPI snapshot missing {method} {path}"
        );
    }
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
    let _guard = env_lock().lock().expect("env lock");
    std::env::set_var("UGOITE_BOOTSTRAP_DEFAULT_SPACE", "true");
    std::env::set_var("UGOITE_DEV_USER_ID", "test-user");
    set_auth_fixture();
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
    let body = response_json(response).await;
    assert_eq!(body[0]["name"], "default");
    assert_eq!(body[1]["name"], "admin-space");
    assert!(body
        .as_array()
        .unwrap()
        .iter()
        .any(|space| space["name"] == "default"));
    assert!(body
        .as_array()
        .unwrap()
        .iter()
        .any(|space| space["name"] == "admin-space" && space["is_admin_space"] == true));
    std::env::remove_var("UGOITE_BOOTSTRAP_DEFAULT_SPACE");
    std::env::remove_var("UGOITE_DEV_USER_ID");
}

#[tokio::test]
async fn public_space_create_requires_admin_space_admin_and_rejects_reserved_id() {
    let _guard = env_lock().lock().expect("env lock");
    std::env::set_var("UGOITE_BOOTSTRAP_DEFAULT_SPACE", "true");
    std::env::set_var("UGOITE_DEV_USER_ID", "test-user");
    set_auth_fixture();
    let state = AppState::new("memory://server-public-create").unwrap();
    state.bootstrap_default_space_from_env().await.unwrap();
    let router = app(state);

    let forbidden = router
        .clone()
        .oneshot(authenticated_with(
            Request::post("/spaces")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"alpha"}"#))
                .unwrap(),
            "alice-token",
        ))
        .await
        .unwrap();
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

    let created = router
        .clone()
        .oneshot(authenticated(
            Request::post("/spaces")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"alpha"}"#))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);

    let reserved = router
        .oneshot(authenticated(
            Request::post("/spaces")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"admin-space"}"#))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(reserved.status(), StatusCode::CONFLICT);
    assert!(response_json(reserved).await["detail"]
        .as_str()
        .unwrap()
        .contains("admin-space"));

    std::env::remove_var("UGOITE_BOOTSTRAP_DEFAULT_SPACE");
    std::env::remove_var("UGOITE_DEV_USER_ID");
}

#[tokio::test]
async fn public_space_get_and_patch_redact_secret_fields() {
    let _guard = env_lock().lock().expect("env lock");
    std::env::set_var("UGOITE_BOOTSTRAP_DEFAULT_SPACE", "true");
    std::env::set_var("UGOITE_DEV_USER_ID", "test-user");
    set_auth_fixture();
    let state = AppState::new("memory://server-redaction").unwrap();
    state.bootstrap_default_space_from_env().await.unwrap();
    let router = app(state);

    let space = router
        .clone()
        .oneshot(authenticated(
            Request::get("/spaces/default").body(Body::empty()).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(space.status(), StatusCode::OK);
    let body = response_json(space).await;
    assert!(body.get("hmac_key").is_none());
    assert!(body.get("hmac_key_id").is_none());
    assert!(body.get("last_rotation").is_none());
    assert!(body
        .get("settings")
        .and_then(Value::as_object)
        .is_some_and(|settings| settings.get("members").is_none()));
    assert!(body
        .get("settings")
        .and_then(Value::as_object)
        .is_some_and(|settings| settings.get("member_invitations").is_none()));
    assert!(body
        .get("settings")
        .and_then(Value::as_object)
        .is_some_and(|settings| settings.get("membership_version").is_none()));

    let patched = router
        .clone()
        .oneshot(authenticated(
            Request::patch("/spaces/default")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"name":"default-renamed","settings":{"default_form":"Entry"}}"#,
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(patched.status(), StatusCode::OK);
    let body = response_json(patched).await;
    assert_eq!(body["name"], "default-renamed");
    assert!(body.get("hmac_key").is_none());
    assert!(body.get("storage_config").is_none());

    std::env::remove_var("UGOITE_BOOTSTRAP_DEFAULT_SPACE");
    std::env::remove_var("UGOITE_DEV_USER_ID");
}

#[tokio::test]
async fn bootstrap_default_space_repairs_existing_owner_membership() {
    let _guard = env_lock().lock().expect("env lock");
    std::env::set_var("UGOITE_BOOTSTRAP_DEFAULT_SPACE", "true");
    std::env::set_var("UGOITE_DEV_USER_ID", "test-user");
    set_auth_fixture();
    let root_uri = "memory://server-bootstrap-existing-membership";
    UgoiteService::new(root_uri)
        .unwrap()
        .create_space("default")
        .await
        .unwrap();

    let state = AppState::new(root_uri).unwrap();
    state.bootstrap_default_space_from_env().await.unwrap();

    let response = app(state)
        .oneshot(authenticated(
            Request::post("/spaces/default/forms")
                .header("content-type", "application/json")
                .body(Body::from(
                    r##"{
                        "name": "Entry",
                        "version": 1,
                        "template": "# Entry\n\n## Body\n",
                        "fields": {"Body": {"type": "markdown"}}
                    }"##,
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    std::env::remove_var("UGOITE_BOOTSTRAP_DEFAULT_SPACE");
    std::env::remove_var("UGOITE_DEV_USER_ID");
}

#[tokio::test]
async fn static_dir_serves_frontend_and_api_prefix() {
    let _guard = env_lock().lock().expect("env lock");
    set_auth_fixture();
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
    std::env::set_var("UGOITE_DEV_USER_ID", "test-user");
    let state = AppState::new("memory://server-static").unwrap();
    state.bootstrap_default_space_from_env().await.unwrap();
    let router = app(state);
    std::env::remove_var("UGOITE_STATIC_DIR");
    std::env::remove_var("UGOITE_BOOTSTRAP_DEFAULT_SPACE");
    std::env::remove_var("UGOITE_DEV_USER_ID");

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
        .clone()
        .oneshot(authenticated(
            Request::get("/api/spaces").body(Body::empty()).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(api.status(), StatusCode::OK);
    let body = response_json(api).await;
    assert!(body
        .as_array()
        .unwrap()
        .iter()
        .any(|space| space["name"] == "default"));

    let missing_api = router
        .oneshot(authenticated(
            Request::get("/api/nonexistent-endpoint-xyz")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(missing_api.status(), StatusCode::NOT_FOUND);

    fs::remove_dir_all(static_dir).unwrap();
}

#[tokio::test]
async fn space_entry_search_and_mcp_contract() {
    let _guard = env_lock().lock().expect("env lock");
    std::env::set_var("UGOITE_BOOTSTRAP_DEFAULT_SPACE", "true");
    std::env::set_var("UGOITE_DEV_USER_ID", "test-user");
    set_auth_fixture();
    let state = AppState::new("memory://server-contract").unwrap();
    state.bootstrap_default_space_from_env().await.unwrap();
    let router = app(state);
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
                r#"{"id":"first","markdown":"---\nform: Entry\n---\n# <script>alert(1)</script>First\n\n## Body\nhello <!-- hidden --> <b>bold</b>\n\n```html\n<script>kept in code</script>\n```"}"#,
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
    let body = response_json(response).await;
    assert_eq!(body[0]["id"], "first");

    let mcp = authenticated(
        Request::get("/mcp/resources/demo/entries/list")
            .body(Body::empty())
            .unwrap(),
    );
    let response = router.oneshot(mcp).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let serialized = serde_json::to_string(&body).unwrap();
    assert!(!serialized.contains("alert(1)"));
    assert!(!serialized.contains("<!-- hidden -->"));
    assert!(!serialized.contains("<b>"));
    assert!(serialized.contains("<script>kept in code</script>"));
    assert_eq!(body["_untrusted_content"], true);
    std::env::remove_var("UGOITE_BOOTSTRAP_DEFAULT_SPACE");
    std::env::remove_var("UGOITE_DEV_USER_ID");
}

#[tokio::test]
async fn auth_config_mock_oauth_and_session_contract() {
    let _guard = env_lock().lock().expect("env lock");
    set_auth_fixture();
    std::env::set_var("UGOITE_DEV_AUTH_MODE", "mock-oauth");
    std::env::set_var("UGOITE_DEV_USER_ID", "dev-local-user");
    std::env::set_var("UGOITE_BOOTSTRAP_TOKEN", "dev-token");
    let router = app(AppState::new("memory://server-auth-contract").unwrap());

    let config = router
        .clone()
        .oneshot(Request::get("/auth/config").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(config.status(), StatusCode::OK);
    let body = response_json(config).await;
    assert_eq!(body["mode"], "mock-oauth");
    assert_eq!(body["username_hint"], "dev-local-user");
    assert_eq!(body["supports_passkey_totp"], false);
    assert_eq!(body["supports_mock_oauth"], true);
    assert_eq!(body["login_required"], true);

    let login = router
        .clone()
        .oneshot(
            Request::post("/auth/mock-oauth")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::OK);
    let cookie = login
        .headers()
        .get("set-cookie")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_string();
    assert!(cookie.contains("ugoite_auth_bearer_token=dev-token"));
    assert!(cookie.contains("HttpOnly"));
    let body = response_json(login).await;
    assert_eq!(body["user_id"], "dev-local-user");
    assert_eq!(body["bearer_token"], "dev-token");
    assert!(body["expires_at"].as_i64().unwrap() > 0);

    let session = router
        .clone()
        .oneshot(
            Request::get("/auth/session")
                .header("cookie", "ugoite_auth_bearer_token=dev-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(session.status(), StatusCode::OK);
    assert_eq!(response_json(session).await["authenticated"], true);

    let cleared = router
        .oneshot(
            Request::delete("/auth/session")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cleared.status(), StatusCode::OK);
    assert!(cleared
        .headers()
        .get("set-cookie")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .contains("Max-Age=0"));
    std::env::remove_var("UGOITE_DEV_AUTH_MODE");
    std::env::remove_var("UGOITE_DEV_USER_ID");
    std::env::remove_var("UGOITE_BOOTSTRAP_TOKEN");
}

#[tokio::test]
async fn scope_enforced_service_identity_is_not_accepted_in_this_release() {
    let _guard = env_lock().lock().expect("env lock");
    set_auth_fixture();
    let router = app(AppState::new("memory://server-scoped-identity").unwrap());

    let response = router
        .oneshot(authenticated_with(
            Request::get("/spaces").body(Body::empty()).unwrap(),
            "scoped-token",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(response_json(response).await["detail"]
        .as_str()
        .unwrap()
        .contains("scope-enforced"));
}

#[tokio::test]
async fn preferences_are_scoped_to_authenticated_identity() {
    let _guard = env_lock().lock().expect("env lock");
    set_auth_fixture();
    let router = app(AppState::new("memory://server-preferences").unwrap());

    let default_response = router
        .clone()
        .oneshot(authenticated_with(
            Request::get("/preferences/me").body(Body::empty()).unwrap(),
            "alice-token",
        ))
        .await
        .unwrap();
    assert_eq!(default_response.status(), StatusCode::OK);
    assert_eq!(response_json(default_response).await["locale"], Value::Null);

    let patched = router
        .clone()
        .oneshot(authenticated_with(
            Request::patch("/preferences/me")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"locale":"ja","selected_space_id":"demo","ui_theme":"classic"}"#,
                ))
                .unwrap(),
            "alice-token",
        ))
        .await
        .unwrap();
    assert_eq!(patched.status(), StatusCode::OK);
    let body = response_json(patched).await;
    assert_eq!(body["locale"], "ja");
    assert_eq!(body["selected_space_id"], "demo");
    assert_eq!(body["ui_theme"], "classic");

    let persisted = router
        .oneshot(authenticated_with(
            Request::get("/preferences/me").body(Body::empty()).unwrap(),
            "alice-token",
        ))
        .await
        .unwrap();
    assert_eq!(response_json(persisted).await["selected_space_id"], "demo");
}

#[tokio::test]
async fn members_lifecycle_contract() {
    let _guard = env_lock().lock().expect("env lock");
    std::env::set_var("UGOITE_BOOTSTRAP_DEFAULT_SPACE", "true");
    std::env::set_var("UGOITE_DEV_USER_ID", "test-user");
    set_auth_fixture();
    let state = AppState::new("memory://server-members").unwrap();
    state.bootstrap_default_space_from_env().await.unwrap();
    let router = app(state);
    assert_eq!(
        router
            .clone()
            .oneshot(authenticated(
                Request::post("/spaces")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"team"}"#))
                    .unwrap(),
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::CREATED
    );

    let invited = router
        .clone()
        .oneshot(authenticated(
            Request::post("/spaces/team/members/invitations")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"user_id":"bob","role":"editor"}"#))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(invited.status(), StatusCode::CREATED);
    let invited_body = response_json(invited).await;
    assert_eq!(invited_body["invitation"]["state"], "pending");
    assert_eq!(invited_body["invitation"]["invited_by"], "test-user");
    let token = invited_body["invitation"]["token"].as_str().unwrap();

    let listed = router
        .clone()
        .oneshot(authenticated(
            Request::get("/spaces/team/members")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response_json(listed).await[0]["state"], "invited");

    let accepted = router
        .clone()
        .oneshot(authenticated_with(
            Request::post("/spaces/team/members/accept")
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"token":"{token}"}}"#)))
                .unwrap(),
            "bob-token",
        ))
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::OK);
    assert_eq!(response_json(accepted).await["member"]["state"], "active");

    let role = router
        .clone()
        .oneshot(authenticated(
            Request::post("/spaces/team/members/bob/role")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"role":"viewer"}"#))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(role.status(), StatusCode::OK);
    assert_eq!(response_json(role).await["member"]["role"], "viewer");

    let revoked = router
        .oneshot(authenticated(
            Request::delete("/spaces/team/members/bob")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(revoked.status(), StatusCode::OK);
    assert_eq!(response_json(revoked).await["member"]["state"], "revoked");
    std::env::remove_var("UGOITE_BOOTSTRAP_DEFAULT_SPACE");
    std::env::remove_var("UGOITE_DEV_USER_ID");
}

#[tokio::test]
async fn space_membership_authorization_contract() {
    let _guard = env_lock().lock().expect("env lock");
    std::env::set_var("UGOITE_BOOTSTRAP_DEFAULT_SPACE", "true");
    std::env::set_var("UGOITE_DEV_USER_ID", "test-user");
    set_auth_fixture();
    let state = AppState::new("memory://server-members-authz").unwrap();
    state.bootstrap_default_space_from_env().await.unwrap();
    let router = app(state);
    assert_eq!(
        router
            .clone()
            .oneshot(authenticated(
                Request::post("/spaces")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"team"}"#))
                    .unwrap(),
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::CREATED
    );

    let non_member_read = router
        .clone()
        .oneshot(authenticated_with(
            Request::get("/spaces/team").body(Body::empty()).unwrap(),
            "alice-token",
        ))
        .await
        .unwrap();
    assert_eq!(non_member_read.status(), StatusCode::FORBIDDEN);

    let invited = router
        .clone()
        .oneshot(authenticated(
            Request::post("/spaces/team/members/invitations")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"user_id":"alice","role":"viewer"}"#))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(invited.status(), StatusCode::CREATED);
    let token = response_json(invited).await["invitation"]["token"]
        .as_str()
        .unwrap()
        .to_string();

    let wrong_user_accept = router
        .clone()
        .oneshot(authenticated_with(
            Request::post("/spaces/team/members/accept")
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"token":"{token}"}}"#)))
                .unwrap(),
            "bob-token",
        ))
        .await
        .unwrap();
    assert_eq!(wrong_user_accept.status(), StatusCode::FORBIDDEN);

    let accepted = router
        .clone()
        .oneshot(authenticated_with(
            Request::post("/spaces/team/members/accept")
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"token":"{token}"}}"#)))
                .unwrap(),
            "alice-token",
        ))
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::OK);

    let viewer_read = router
        .clone()
        .oneshot(authenticated_with(
            Request::get("/spaces/team").body(Body::empty()).unwrap(),
            "alice-token",
        ))
        .await
        .unwrap();
    assert_eq!(viewer_read.status(), StatusCode::OK);

    let viewer_patch = router
        .clone()
        .oneshot(authenticated_with(
            Request::patch("/spaces/team")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"settings":{"default_form":"Entry"}}"#))
                .unwrap(),
            "alice-token",
        ))
        .await
        .unwrap();
    assert_eq!(viewer_patch.status(), StatusCode::FORBIDDEN);

    let promoted = router
        .clone()
        .oneshot(authenticated(
            Request::post("/spaces/team/members/alice/role")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"role":"editor"}"#))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(promoted.status(), StatusCode::OK);

    let editor_write = router
        .clone()
        .oneshot(authenticated_with(
            Request::post("/spaces/team/entries")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"id":"viewer-now-editor","markdown":"---\nform: Entry\n---\n# Editor\n\n## Body\nhello"}"#,
                ))
                .unwrap(),
            "alice-token",
        ))
        .await
        .unwrap();
    assert_eq!(editor_write.status(), StatusCode::CREATED);

    let editor_space_patch = router
        .clone()
        .oneshot(authenticated_with(
            Request::patch("/spaces/team")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"settings":{"default_form":"Entry"}}"#))
                .unwrap(),
            "alice-token",
        ))
        .await
        .unwrap();
    assert_eq!(editor_space_patch.status(), StatusCode::FORBIDDEN);

    let revoked = router
        .clone()
        .oneshot(authenticated(
            Request::delete("/spaces/team/members/alice")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(revoked.status(), StatusCode::OK);

    let revoked_read = router
        .oneshot(authenticated_with(
            Request::get("/spaces/team").body(Body::empty()).unwrap(),
            "alice-token",
        ))
        .await
        .unwrap();
    assert_eq!(revoked_read.status(), StatusCode::FORBIDDEN);
    std::env::remove_var("UGOITE_BOOTSTRAP_DEFAULT_SPACE");
    std::env::remove_var("UGOITE_DEV_USER_ID");
}

#[tokio::test]
async fn test_connection_requires_manage_space_and_probes_uri() {
    let _guard = env_lock().lock().expect("env lock");
    std::env::set_var("UGOITE_BOOTSTRAP_DEFAULT_SPACE", "true");
    std::env::set_var("UGOITE_DEV_USER_ID", "test-user");
    set_auth_fixture();
    let state = AppState::new("memory://server-test-connection").unwrap();
    state.bootstrap_default_space_from_env().await.unwrap();
    let router = app(state);
    assert_eq!(
        router
            .clone()
            .oneshot(authenticated(
                Request::post("/spaces")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"team"}"#))
                    .unwrap(),
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::CREATED
    );
    let invited = router
        .clone()
        .oneshot(authenticated(
            Request::post("/spaces/team/members/invitations")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"user_id":"alice","role":"viewer"}"#))
                .unwrap(),
        ))
        .await
        .unwrap();
    let token = response_json(invited).await["invitation"]["token"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        router
            .clone()
            .oneshot(authenticated_with(
                Request::post("/spaces/team/members/accept")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(r#"{{"token":"{token}"}}"#)))
                    .unwrap(),
                "alice-token",
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );

    let viewer = router
        .clone()
        .oneshot(authenticated_with(
            Request::post("/spaces/team/test-connection")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"storage_config":{"uri":"memory://probe-viewer"}}"#,
                ))
                .unwrap(),
            "alice-token",
        ))
        .await
        .unwrap();
    assert_eq!(viewer.status(), StatusCode::FORBIDDEN);

    let missing = router
        .clone()
        .oneshot(authenticated(
            Request::post("/spaces/missing/test-connection")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"storage_config":{"uri":"memory://probe"}}"#))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    let unsupported = router
        .clone()
        .oneshot(authenticated(
            Request::post("/spaces/team/test-connection")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"storage_config":{"uri":"ftp://example.test"}}"#,
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(unsupported.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let ok = router
        .oneshot(authenticated(
            Request::post("/spaces/team/test-connection")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"storage_config":{"uri":"memory://probe-ok"}}"#,
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::OK);
    assert_eq!(response_json(ok).await["status"], "ok");
    std::env::remove_var("UGOITE_BOOTSTRAP_DEFAULT_SPACE");
    std::env::remove_var("UGOITE_DEV_USER_ID");
}

#[tokio::test]
async fn sql_session_routes_create_status_count_and_rows() {
    let _guard = env_lock().lock().expect("env lock");
    std::env::set_var("UGOITE_BOOTSTRAP_DEFAULT_SPACE", "true");
    std::env::set_var("UGOITE_DEV_USER_ID", "test-user");
    set_auth_fixture();
    let state = AppState::new("memory://server-sql-sessions").unwrap();
    state.bootstrap_default_space_from_env().await.unwrap();
    let router = app(state);
    assert_eq!(
        router
            .clone()
            .oneshot(authenticated(
                Request::post("/spaces")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"queries"}"#))
                    .unwrap(),
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        router
            .clone()
            .oneshot(authenticated(
                Request::post("/spaces/queries/entries")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"id":"first","markdown":"---\nform: Entry\n---\n# First\n\n## Body\nhello"}"#,
                    ))
                    .unwrap(),
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::CREATED
    );

    let created = router
        .clone()
        .oneshot(authenticated(
            Request::post("/spaces/queries/sql-sessions")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"sql":"SELECT * FROM entries WHERE title = 'First'"}"#,
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let created_body = response_json(created).await;
    assert_eq!(created_body["status"], "ready");
    let session_id = created_body["id"].as_str().unwrap();

    let status = router
        .clone()
        .oneshot(authenticated(
            Request::get(format!("/spaces/queries/sql-sessions/{session_id}"))
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response_json(status).await["id"], session_id);

    let count = router
        .clone()
        .oneshot(authenticated(
            Request::get(format!("/spaces/queries/sql-sessions/{session_id}/count"))
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response_json(count).await["count"], 1);

    let rows = router
        .oneshot(authenticated(
            Request::get(format!(
                "/spaces/queries/sql-sessions/{session_id}/rows?offset=0&limit=10"
            ))
            .body(Body::empty())
            .unwrap(),
        ))
        .await
        .unwrap();
    let rows_body = response_json(rows).await;
    assert_eq!(rows_body["total_count"], 1);
    assert_eq!(rows_body["rows"][0]["id"], "first");
    std::env::remove_var("UGOITE_BOOTSTRAP_DEFAULT_SPACE");
    std::env::remove_var("UGOITE_DEV_USER_ID");
}
