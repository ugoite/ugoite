use _ieapp_core::sandbox;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

fn repo_sandbox_wasm_path() -> PathBuf {
    // Keep this deterministic for CI: use the checked-in artifact from ieapp-cli.
    PathBuf::from("../ieapp-cli/src/ieapp/sandbox/sandbox.wasm")
}

/// REQ-SANDBOX-001
#[tokio::test]
async fn test_sandbox_req_sandbox_001_simple_execution() -> anyhow::Result<()> {
    let wasm = repo_sandbox_wasm_path();
    let handler =
        Arc::new(|_method: &str, _path: &str, _body: Option<serde_json::Value>| Ok(json!(null)));

    let result = sandbox::run_script(&wasm, "return 1 + 1;", handler, 100_000_000).await?;
    assert_eq!(result, json!(2));
    Ok(())
}

/// REQ-SANDBOX-002
#[tokio::test]
async fn test_sandbox_req_sandbox_002_host_call() -> anyhow::Result<()> {
    let wasm = repo_sandbox_wasm_path();
    let handler = Arc::new(
        |method: &str, path: &str, _body: Option<serde_json::Value>| {
            if method == "GET" && path == "/test" {
                Ok(json!({"ok": true}))
            } else {
                Ok(json!({"ok": false}))
            }
        },
    );

    let code = r#"
        const res = host.call('GET', '/test');
        return res.ok;
    "#;

    let result = sandbox::run_script(&wasm, code, handler, 100_000_000).await?;
    assert_eq!(result, json!(true));
    Ok(())
}

/// REQ-SANDBOX-003
#[tokio::test]
async fn test_sandbox_req_sandbox_003_execution_error() {
    let wasm = repo_sandbox_wasm_path();
    let handler =
        Arc::new(|_method: &str, _path: &str, _body: Option<serde_json::Value>| Ok(json!(null)));

    let err = sandbox::run_script(&wasm, "throw new Error('boom');", handler, 100_000_000)
        .await
        .expect_err("expected error");
    assert!(err.to_string().contains("boom"));
}

/// REQ-SANDBOX-004
#[tokio::test]
async fn test_sandbox_req_sandbox_004_infinite_loop_fuel() {
    let wasm = repo_sandbox_wasm_path();
    let handler =
        Arc::new(|_method: &str, _path: &str, _body: Option<serde_json::Value>| Ok(json!(null)));

    let err = sandbox::run_script(&wasm, "while(true) {}", handler, 100_000)
        .await
        .expect_err("expected fuel exhaustion");
    let msg = err.to_string();
    assert!(
        msg.to_lowercase().contains("fuel"),
        "expected fuel exhaustion error containing 'fuel', got: {msg}"
    );
}

/// REQ-SANDBOX-005
#[tokio::test]
async fn test_sandbox_req_sandbox_005_missing_wasm_raises() {
    let missing = PathBuf::from("./does-not-exist-sandbox.wasm");
    let handler =
        Arc::new(|_method: &str, _path: &str, _body: Option<serde_json::Value>| Ok(json!(null)));

    let err = sandbox::run_script(&missing, "return 1;", handler, 100_000_000)
        .await
        .expect_err("expected missing wasm error");
    assert!(err.to_string().contains("missing"));
}
