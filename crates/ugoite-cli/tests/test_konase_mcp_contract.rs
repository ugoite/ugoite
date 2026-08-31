//! Contract coverage for the CLI Konase MCP endpoint fallback.
//!
//! Issue #2072 intentionally stops at the adapter boundary: the real CLI
//! connects its real rmcp transport to the integrated server and completes
//! the discovery/tools-list handshake without invoking a model provider.

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::{self, Next},
    response::Response,
    Extension,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Utc;
use p256::{ecdsa::SigningKey, elliptic_curve::rand_core::OsRng, pkcs8::EncodePrivateKey};
use serde_json::json;
use std::{
    fs,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::task::JoinHandle;
use ugoite_cli::config::{AuthSession, EndpointConfig, EndpointMode};
use ugoite_server::{app, AppState};

struct ServerGuard(JoinHandle<()>);

impl Drop for ServerGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Clone, Debug)]
struct RequestObservation {
    method: String,
    path: String,
    status: StatusCode,
}

fn ugoite_bin() -> std::path::PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_ugoite") {
        return std::path::PathBuf::from(path);
    }

    let mut path = std::env::current_exe().expect("current exe");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("ugoite");
    path
}

fn test_key_and_jwk() -> (SigningKey, serde_json::Value) {
    let key = SigningKey::random(&mut OsRng);
    let point = key.verifying_key().to_encoded_point(false);
    let jwk = json!({
        "kty": "EC",
        "crv": "P-256",
        "x": URL_SAFE_NO_PAD.encode(point.x().expect("public key x")),
        "y": URL_SAFE_NO_PAD.encode(point.y().expect("public key y")),
    });
    (key, jwk)
}

#[tokio::test]
async fn issue_2072_cli_konase_api_base_reaches_authenticated_root_mcp() {
    tokio::time::timeout(Duration::from_secs(45), issue_2072_contract())
        .await
        .expect("issue 2072 contract test timed out");
}

async fn issue_2072_contract() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind server");
    let address = listener.local_addr().expect("server address");
    let server_url = format!("http://localhost:{}", address.port());
    let state = AppState::new_for_tests_with_origin(
        format!(
            "memory://cli-konase-mcp-credential-{}",
            uuid::Uuid::now_v7()
        ),
        &server_url,
    )
    .expect("server state");
    state.initialize_node().await.expect("initialize server");
    let (key, public_key_jwk) = test_key_and_jwk();
    let access = state
        .issue_test_mcp_access(public_key_jwk.clone())
        .await
        .expect("issue test MCP credential");

    let requests = Arc::new(Mutex::new(Vec::<RequestObservation>::new()));
    let server_requests = Arc::clone(&requests);
    let _server_task = ServerGuard(tokio::spawn(async move {
        let server_app = app(state)
            .layer(middleware::from_fn(record_request))
            .layer(Extension(server_requests));
        axum::serve(listener, server_app)
            .await
            .expect("integrated server exited unexpectedly");
    }));
    let api_base = format!("{server_url}/api");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("build probe client");
    let api_mcp = client
        .get(format!("{api_base}/mcp"))
        .send()
        .await
        .expect("probe API-scoped MCP path");
    assert_eq!(api_mcp.status(), reqwest::StatusCode::NOT_FOUND);

    let config_dir = tempdir().expect("config directory");
    let config_path = config_dir.path().join("cli-endpoints.json");
    let credentials_path = config_dir.path().join("cli-credentials.json");
    let session = AuthSession {
        credential_id: access.credential_id,
        device_name: "Issue 2072 contract test".to_string(),
        public_key_jwk,
        private_key_pkcs8: Some(
            URL_SAFE_NO_PAD.encode(
                key.to_pkcs8_der()
                    .expect("encode test private key")
                    .as_bytes(),
            ),
        ),
        access_token: access.access_token,
        refresh_token: "unused-in-contract-test".to_string(),
        expires_at: Utc::now().timestamp() + 300,
        base_url: api_base.clone(),
        resource: Some(access.resource),
        space_uid: access.space_uid,
    };
    let config = EndpointConfig {
        mode: EndpointMode::Api,
        backend_url: server_url,
        api_url: api_base,
    };
    fs::write(
        &config_path,
        serde_json::to_vec_pretty(&config).expect("serialize endpoint config"),
    )
    .expect("write endpoint config");
    fs::write(
        &credentials_path,
        serde_json::to_vec_pretty(&session).expect("serialize CLI credential"),
    )
    .expect("write CLI credential");

    let child = Command::new(ugoite_bin())
        .arg("konase")
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .env("UGOITE_MODEL_API_KEY", "contract-test-key")
        .spawn()
        .expect("start CLI Konase");
    let output = tokio::time::timeout(Duration::from_secs(30), child.wait_with_output())
        .await
        .expect("CLI Konase timed out")
        .expect("wait for CLI Konase");
    assert!(
        output.status.success(),
        "CLI Konase failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let observed = requests.lock().expect("request log lock").clone();
    let api_metadata = observed
        .iter()
        .position(|request| {
            request.method == "GET" && request.path == "/api/.well-known/oauth-protected-resource"
        })
        .expect("CLI should probe API-scoped MCP metadata");
    assert!(!observed[api_metadata].status.is_success());
    let root_metadata = observed
        .iter()
        .position(|request| {
            request.method == "GET" && request.path == "/.well-known/oauth-protected-resource"
        })
        .expect("CLI should fall back to root MCP metadata");
    assert!(observed[root_metadata].status.is_success());
    let root_mcp = observed
        .iter()
        .position(|request| request.method == "POST" && request.path == "/mcp")
        .expect("rmcp should call the root MCP endpoint");
    assert_eq!(observed[root_mcp].status, StatusCode::OK);
    assert!(api_metadata < root_metadata);
    assert!(root_metadata < root_mcp);
}

async fn record_request(
    Extension(requests): Extension<Arc<Mutex<Vec<RequestObservation>>>>,
    request: Request,
    next: Next,
) -> Response {
    let method = request.method().to_string();
    let path = request.uri().path().to_string();
    let response = next.run(request).await;
    requests
        .lock()
        .expect("request log lock")
        .push(RequestObservation {
            method,
            path,
            status: response.status(),
        });
    response
}
