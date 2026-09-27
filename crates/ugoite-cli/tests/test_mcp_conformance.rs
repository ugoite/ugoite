//! Run the official MCP conformance scenarios for Ugoite's advertised surface
//! against the real integrated server. The proxy only injects the test bearer
//! credential; all MCP requests and responses still pass through Ugoite.

use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    http::{header, HeaderName, StatusCode},
    response::Response,
    routing::any,
    Router,
};
use p256::{ecdsa::SigningKey, elliptic_curve::rand_core::OsRng};
use serde_json::json;
use std::{process::Stdio, time::Duration};
use tokio::{net::TcpListener, process::Command, task::JoinHandle};
use ugoite_server::{app, AppState};

const CONFORMANCE_PACKAGE: &str = "@modelcontextprotocol/conformance@0.2.0-alpha.11";
const MCP_VERSION: &str = "2026-07-28";

struct ServerGuard(JoinHandle<()>);

impl Drop for ServerGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Clone)]
struct ProxyState {
    target: String,
    token: String,
    client: reqwest::Client,
}

#[tokio::test]
async fn official_mcp_2026_conformance_covers_advertised_list_methods() {
    tokio::time::timeout(Duration::from_secs(180), run_conformance())
        .await
        .expect("official MCP conformance run timed out");
}

#[tokio::test]
async fn mcp_rejects_wrong_resource_issuer_scope_and_space_selector() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind Ugoite");
    let address = listener.local_addr().expect("Ugoite address");
    let origin = format!("http://localhost:{}", address.port());
    let state = AppState::new_for_tests_with_origin(
        format!("memory://mcp-auth-boundaries-{}", uuid::Uuid::now_v7()),
        &origin,
    )
    .expect("create Ugoite state");
    state
        .initialize_node_for_tests()
        .await
        .expect("initialize Ugoite");
    let read_access = state
        .issue_test_mcp_access(test_public_key_jwk())
        .await
        .expect("issue read-only MCP credential");
    let rest_access = state
        .issue_test_rest_access(test_public_key_jwk())
        .await
        .expect("issue REST-scoped credential");
    let other_space_uid = state
        .issue_test_mcp_access(test_public_key_jwk())
        .await
        .expect("issue credential for another Space")
        .space_uid;

    let foreign_origin = format!("https://foreign-issuer-{}.example", uuid::Uuid::now_v7());
    let foreign_state = AppState::new_for_tests_with_origin(
        format!("memory://mcp-foreign-issuer-{}", uuid::Uuid::now_v7()),
        &foreign_origin,
    )
    .expect("create foreign issuer state");
    foreign_state
        .initialize_node_for_tests()
        .await
        .expect("initialize foreign issuer");
    let foreign_access = foreign_state
        .issue_test_mcp_access(test_public_key_jwk())
        .await
        .expect("issue foreign-issuer MCP credential");

    let server_task = ServerGuard(tokio::spawn(async move {
        axum::serve(listener, app(state))
            .await
            .expect("Ugoite server exited unexpectedly");
    }));
    let client = reqwest::Client::new();
    let endpoint = format!("{origin}/mcp");

    let wrong_resource = send_mcp(
        &client,
        &endpoint,
        &rest_access.access_token,
        "list-tools",
        "tools/list",
        json!({}),
    )
    .await;
    assert_eq!(wrong_resource.status(), reqwest::StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = wrong_resource.json().await.expect("resource error JSON");
    assert_eq!(body["code"], "AUTHENTICATION_REQUIRED");

    let wrong_issuer = send_mcp(
        &client,
        &endpoint,
        &foreign_access.access_token,
        "list-tools",
        "tools/list",
        json!({}),
    )
    .await;
    assert_eq!(wrong_issuer.status(), reqwest::StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = wrong_issuer.json().await.expect("issuer error JSON");
    assert_eq!(body["code"], "AUTHENTICATION_REQUIRED");

    let insufficient_scope = send_mcp(
        &client,
        &endpoint,
        &read_access.access_token,
        "write-tool",
        "tools/call",
        json!({"name":"ugoite.undo","arguments":{}}),
    )
    .await;
    assert_eq!(insufficient_scope.status(), reqwest::StatusCode::FORBIDDEN);
    let body: serde_json::Value = insufficient_scope.json().await.expect("scope error JSON");
    assert_eq!(body["code"], "INSUFFICIENT_SCOPE");
    assert_eq!(body["required_actions"][0], "update");

    let attempted_space_override = send_mcp(
        &client,
        &endpoint,
        &read_access.access_token,
        "search-other-space",
        "tools/call",
        json!({
            "name":"ugoite.search",
            "arguments":{"q":"anything","space_uid":other_space_uid}
        }),
    )
    .await;
    assert_eq!(attempted_space_override.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = attempted_space_override
        .json()
        .await
        .expect("Space override result JSON");
    assert_eq!(body["result"]["isError"], true);
    assert_eq!(
        body["result"]["structuredContent"]["code"],
        "INVALID_ARGUMENT"
    );

    drop(server_task);
}

fn test_public_key_jwk() -> serde_json::Value {
    let key = SigningKey::random(&mut OsRng);
    let point = key.verifying_key().to_encoded_point(false);
    json!({
        "kty": "EC",
        "crv": "P-256",
        "x": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(point.x().expect("x")),
        "y": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(point.y().expect("y")),
    })
}

fn rpc_request(id: &str, method: &str, params: serde_json::Value) -> serde_json::Value {
    let mut params = params;
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": MCP_VERSION,
        "io.modelcontextprotocol/clientCapabilities": {}
    });
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}

async fn send_mcp(
    client: &reqwest::Client,
    endpoint: &str,
    token: &str,
    id: &str,
    method: &str,
    params: serde_json::Value,
) -> reqwest::Response {
    let body = rpc_request(id, method, params);
    let mut request = client
        .post(endpoint)
        .bearer_auth(token)
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", MCP_VERSION)
        .header("mcp-method", method);
    if method == "tools/call" {
        request = request.header(
            "mcp-name",
            body["params"]["name"].as_str().expect("tool name"),
        );
    }
    request.json(&body).send().await.expect("send MCP request")
}

async fn run_conformance() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind Ugoite");
    let address = listener.local_addr().expect("Ugoite address");
    let origin = format!("http://localhost:{}", address.port());
    let state = AppState::new_for_tests_with_origin(
        format!("memory://mcp-conformance-{}", uuid::Uuid::now_v7()),
        &origin,
    )
    .expect("create Ugoite state");
    state
        .initialize_node_for_tests()
        .await
        .expect("initialize Ugoite");

    let access = state
        .issue_test_mcp_access(test_public_key_jwk())
        .await
        .expect("issue read-only test credential");
    let ugoite_task = ServerGuard(tokio::spawn(async move {
        axum::serve(listener, app(state))
            .await
            .expect("Ugoite server exited unexpectedly");
    }));

    let proxy_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind authenticated test proxy");
    let proxy_address = proxy_listener.local_addr().expect("proxy address");
    let proxy_state = ProxyState {
        target: format!("{origin}/mcp"),
        token: access.access_token,
        client: reqwest::Client::new(),
    };
    let proxy_app = Router::new()
        .route("/mcp", any(forward_to_ugoite))
        .with_state(proxy_state);
    let proxy_task = ServerGuard(tokio::spawn(async move {
        axum::serve(proxy_listener, proxy_app)
            .await
            .expect("authenticated test proxy exited unexpectedly");
    }));

    for scenario in ["tools-list", "resources-list"] {
        let output = Command::new("npx")
            .args([
                "--yes",
                CONFORMANCE_PACKAGE,
                "server",
                "--url",
                &format!("http://127.0.0.1:{}/mcp", proxy_address.port()),
                "--scenario",
                scenario,
                "--force",
                "--spec-version",
                MCP_VERSION,
            ])
            .stdin(Stdio::null())
            .output()
            .await
            .expect("run pinned official MCP conformance CLI");
        assert!(
            output.status.success(),
            "official MCP scenario {scenario} failed (status {}):\n{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    drop(proxy_task);
    drop(ugoite_task);
}

async fn forward_to_ugoite(
    State(state): State<ProxyState>,
    request: Request,
) -> Result<Response, StatusCode> {
    let mut forward = state
        .client
        .request(request.method().clone(), &state.target)
        .bearer_auth(&state.token);
    for name in [
        header::CONTENT_TYPE,
        header::ACCEPT,
        HeaderName::from_static("mcp-protocol-version"),
        HeaderName::from_static("mcp-method"),
        HeaderName::from_static("mcp-name"),
        header::ORIGIN,
    ] {
        if let Some(value) = request.headers().get(&name) {
            forward = forward.header(name, value);
        }
    }
    let body = to_bytes(request.into_body(), 2 * 1024 * 1024)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let response = forward
        .body(body)
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    let status =
        StatusCode::from_u16(response.status().as_u16()).map_err(|_| StatusCode::BAD_GATEWAY)?;
    let content_type = response.headers().get(header::CONTENT_TYPE).cloned();
    let session_id = response.headers().get("mcp-session-id").cloned();
    let body = response
        .bytes()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    let mut builder = Response::builder().status(status);
    if let Some(value) = content_type {
        builder = builder.header(header::CONTENT_TYPE, value);
    }
    if let Some(value) = session_id {
        builder = builder.header("mcp-session-id", value);
    }
    builder
        .body(Body::from(body))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

// This makes the base64 trait import local and keeps the test's credential
// representation consistent with the server's test access helper.
use base64::Engine as _;
