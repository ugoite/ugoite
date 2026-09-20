//! JOURNEY-LOCATE-RECOVER-001 through the server-backed CLI.
//!
//! Evidence identity: surface=cli, transport=remote. This runs the same
//! logical locate-and-recover scenario as the Frontend and core evidence:
//! Form-backed Entries are discovered by keyword, narrowed by typed
//! structured Search, updated, observed in Space Change history, recovered
//! by Change revert, and re-verified by search and reopen reads. Transport
//! and auth ceremony stay in setup; validation and recovery semantics must
//! match the core outcome.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Utc;
use p256::{ecdsa::SigningKey, elliptic_curve::rand_core::OsRng, pkcs8::EncodePrivateKey};
use serde_json::json;
use std::process::Output;
use std::time::Duration;
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::task::JoinHandle;
use ugoite_cli::cli_config::{ConfigFile, ConnectionConfig, ContextConfig};
use ugoite_cli::config::AuthSession;
use ugoite_server::{app, AppState};

struct ServerGuard(JoinHandle<()>);

impl Drop for ServerGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
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

async fn run_cli(config_path: &std::path::Path, args: &[&str]) -> Output {
    let space_uid = std::fs::read_to_string(config_path).ok().and_then(|text| {
        text.lines().find_map(|line| {
            line.trim()
                .strip_prefix("space_uid = \"")
                .and_then(|value| value.strip_suffix('\"'))
                .map(str::to_owned)
        })
    });
    let mut command_args = vec!["--config", config_path.to_str().expect("config path")];
    command_args.extend(
        args.iter()
            .copied()
            .filter(|arg| Some(*arg) != space_uid.as_deref()),
    );
    Command::new(ugoite_bin())
        .args(command_args)
        .env(
            "HOME",
            config_path.parent().expect("config parent").join("home"),
        )
        .output()
        .await
        .expect("run ugoite")
}

fn stdout_json(output: &Output, what: &str) -> serde_json::Value {
    assert!(
        output.status.success(),
        "{what} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("{what} stdout is not JSON: {stdout}"))
}

fn contains_string(value: &serde_json::Value, needle: &str) -> bool {
    match value {
        serde_json::Value::String(text) => text == needle,
        serde_json::Value::Array(items) => items.iter().any(|item| contains_string(item, needle)),
        serde_json::Value::Object(fields) => {
            fields.values().any(|item| contains_string(item, needle))
        }
        _ => false,
    }
}

fn search_ids(results: &serde_json::Value) -> Vec<String> {
    results
        .as_array()
        .unwrap_or_else(|| panic!("search results must be an array: {results}"))
        .iter()
        .map(|row| {
            row.get("_ugoite_id")
                .or_else(|| row.get("id"))
                .and_then(|id| id.as_str())
                .unwrap_or_else(|| panic!("search row has no id: {row}"))
                .to_string()
        })
        .collect()
}

fn change_ids(changes: &serde_json::Value) -> Vec<String> {
    changes
        .as_array()
        .unwrap_or_else(|| panic!("change list must be an array: {changes}"))
        .iter()
        .map(|change| {
            change
                .get("change_id")
                .and_then(|id| id.as_str())
                .unwrap_or_else(|| panic!("change has no change_id: {change}"))
                .to_string()
        })
        .collect()
}

fn revision_ids(history: &serde_json::Value) -> Vec<String> {
    history
        .get("revisions")
        .and_then(|revisions| revisions.as_array())
        .unwrap_or_else(|| panic!("history has no revisions array: {history}"))
        .iter()
        .map(|revision| {
            revision
                .get("revision_id")
                .and_then(|id| id.as_str())
                .unwrap_or_else(|| panic!("revision has no revision_id: {revision}"))
                .to_string()
        })
        .collect()
}

#[tokio::test]
async fn journey_cli_remote_locate_recover_reaches_durable_outcome() {
    tokio::time::timeout(
        Duration::from_secs(180),
        journey_cli_remote_locate_recover(),
    )
    .await
    .expect("journey CLI remote locate-recover test timed out");
}

struct RemoteFixture {
    config_path: std::path::PathBuf,
    space_id: String,
    _config_dir: tempfile::TempDir,
    _server: ServerGuard,
}

async fn setup_remote() -> RemoteFixture {
    // Real server over loopback TCP with test-issued REST access: the only
    // fixture is transport and auth ceremony, never business semantics.
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind server");
    let address = listener.local_addr().expect("server address");
    let server_url = format!("http://localhost:{}", address.port());
    // Without UGOITE_STATIC_DIR the app merges API routes at the root, so
    // the API base is the bare server URL (no /api prefix).
    let api_base = server_url.clone();
    let state = AppState::new_for_tests_with_origin(
        format!(
            "memory://cli-locate-recover-remote-{}",
            uuid::Uuid::now_v7()
        ),
        &server_url,
    )
    .expect("server state");
    state.initialize_node().await.expect("initialize server");
    let (key, public_key_jwk) = test_key_and_jwk();
    let access = state_issue_rest_access(&state, public_key_jwk.clone()).await;
    let _server = ServerGuard(tokio::spawn(async move {
        axum::serve(listener, app(state))
            .await
            .expect("integrated server exited unexpectedly");
    }));

    let probe = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("build probe client");
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        match probe.get(format!("{api_base}/spaces")).send().await {
            Ok(response) if (response.status().as_u16()) < 500 => break,
            _ if std::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            other => panic!("server never became reachable: {other:?}"),
        }
    }

    let config_dir = tempdir().expect("config directory");
    let config_path = config_dir.path().join("config.toml");
    let credentials_path = config_dir.path().join("home/.ugoite/credentials.json");
    let session = AuthSession {
        credential_id: access.credential_id,
        device_name: "Locate-recover remote test".to_string(),
        public_key_jwk,
        private_key_pkcs8: Some(
            URL_SAFE_NO_PAD.encode(
                key.to_pkcs8_der()
                    .expect("encode test private key")
                    .as_bytes(),
            ),
        ),
        access_token: access.access_token,
        refresh_token: "unused-in-locate-recover-test".to_string(),
        expires_at: Utc::now().timestamp() + 300,
        base_url: api_base.clone(),
        resource: None,
        space_uid: access.space_uid,
    };
    let mut config = ConfigFile::empty();
    config.connections.insert(
        "remote-api".to_string(),
        ConnectionConfig::Api { url: api_base },
    );
    config.contexts.insert(
        "locate-recover".to_string(),
        ContextConfig {
            connection: "remote-api".to_string(),
            space_uid: access.space_uid,
            credential: Some("locate-recover".to_string()),
        },
    );
    config.current_context = Some("locate-recover".to_string());
    let mut profile = serde_json::to_value(&session).expect("serialize CLI credential");
    profile["connection"] = serde_json::Value::String("remote-api".to_string());
    let credentials = serde_json::json!({
        "version": 1,
        "credentials": { "locate-recover": profile },
    });
    std::fs::write(
        &config_path,
        toml::to_string_pretty(&config).expect("serialize canonical config"),
    )
    .expect("write canonical config");
    std::fs::create_dir_all(credentials_path.parent().expect("credentials parent"))
        .expect("create credentials directory");
    std::fs::write(
        &credentials_path,
        serde_json::to_vec_pretty(&credentials).expect("serialize credential store"),
    )
    .expect("write CLI credential");

    RemoteFixture {
        config_path,
        space_id: access.space_uid.to_string(),
        _config_dir: config_dir,
        _server,
    }
}

async fn journey_cli_remote_locate_recover() {
    let fixture = setup_remote().await;
    let config_path = &fixture.config_path;
    let space_id: &str = &fixture.space_id;

    // Bare Space IDs select the remote transport in every command below.
    let form_name = "LocateTask";

    // Form establish via `form update`: the upsert path behind a weaker name.
    let form_file = config_path
        .parent()
        .expect("config parent")
        .join("locate-recover-form.json");
    std::fs::write(
        &form_file,
        format!(
            "{{\"name\":\"{form_name}\",\"version\":1,\"template\":\"# {form_name}\\n\\n## status\\n\\n## priority\\n\",\"fields\":{{\"status\":{{\"type\":\"string\",\"required\":true}},\"priority\":{{\"type\":\"integer\",\"required\":false}}}}}}"
        ),
    )
    .expect("write locate-recover form");
    let output = run_cli(
        config_path,
        &["form", "update", space_id, form_file.to_str().unwrap()],
    )
    .await;
    assert!(
        output.status.success(),
        "form establish failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Multiple Form-backed Entries.
    for (entry_id, status, priority) in
        [("locate-task-a", "open", 3), ("locate-task-b", "closed", 7)]
    {
        let priority = priority.to_string();
        let status_field = format!("status={status}");
        let priority_field = format!("priority={priority}");
        let output = run_cli(
            config_path,
            &[
                "entry",
                "create",
                "--form",
                form_name,
                "--field",
                &status_field,
                "--field",
                &priority_field,
                entry_id,
            ],
        )
        .await;
        assert!(
            output.status.success(),
            "entry create failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Keyword Search discovers the target.
    let results = stdout_json(
        &run_cli(
            config_path,
            &["search", "keyword", space_id, "locate-task-a"],
        )
        .await,
        "keyword search discovers the target",
    );
    assert!(contains_string(&results, "locate-task-a"));

    // Typed structured Search narrows to the intended Entry set.
    let results = stdout_json(
        &run_cli(
            config_path,
            &[
                "search",
                "query",
                space_id,
                "--form",
                form_name,
                "--eq",
                "status=open",
            ],
        )
        .await,
        "structured search narrows to open tasks",
    );
    assert_eq!(search_ids(&results), vec!["locate-task-a".to_string()]);

    // Update the Entry; the receipt carries the durable Change ID.
    let history = stdout_json(
        &run_cli(
            config_path,
            &["entry", "history", space_id, "locate-task-a"],
        )
        .await,
        "entry history after create",
    );
    assert_eq!(revision_ids(&history).len(), 1);
    let rev1 = revision_ids(&history)[0].clone();
    let status_field = "status=in-progress";
    let priority_field = "priority=3";
    let updated = stdout_json(
        &run_cli(
            config_path,
            &[
                "entry",
                "update",
                "locate-task-a",
                "--form",
                form_name,
                "--field",
                status_field,
                "--field",
                priority_field,
                "--parent-revision-id",
                &rev1,
            ],
        )
        .await,
        "entry update",
    );
    let update_change_id = updated
        .get("change_id")
        .and_then(|id| id.as_str())
        .expect("update returns durable change_id")
        .to_string();

    // Space History observes the timeline.
    let changes = stdout_json(
        &run_cli(config_path, &["change", "list", space_id]).await,
        "change list observes the timeline",
    );
    let before_ids = change_ids(&changes);
    assert!(before_ids.contains(&update_change_id));

    // Change revert appends its inverse; the reverted Change is kept.
    let reverted = stdout_json(
        &run_cli(
            config_path,
            &["change", "revert", space_id, &update_change_id],
        )
        .await,
        "change revert",
    );
    let revert_id = reverted
        .get("change_id")
        .and_then(|id| id.as_str())
        .expect("revert returns the appended change_id")
        .to_string();
    assert_ne!(revert_id, update_change_id);

    // Entry history grows append-only; current search reflects recovery.
    let history = stdout_json(
        &run_cli(
            config_path,
            &["entry", "history", space_id, "locate-task-a"],
        )
        .await,
        "entry history after revert",
    );
    let ids = revision_ids(&history);
    assert_eq!(ids.len(), 3);
    assert!(ids.contains(&rev1));
    let changes = stdout_json(
        &run_cli(config_path, &["change", "list", space_id]).await,
        "change list after revert",
    );
    let after_ids = change_ids(&changes);
    assert!(after_ids.contains(&update_change_id));
    assert!(after_ids.contains(&revert_id));
    let results = stdout_json(
        &run_cli(
            config_path,
            &[
                "search",
                "query",
                space_id,
                "--form",
                form_name,
                "--eq",
                "status=open",
            ],
        )
        .await,
        "structured search reflects recovered state",
    );
    assert_eq!(search_ids(&results), vec!["locate-task-a".to_string()]);

    // Reopen: fresh invocations read the same durable state.
    let history = stdout_json(
        &run_cli(
            config_path,
            &["entry", "history", space_id, "locate-task-a"],
        )
        .await,
        "entry history on reopen",
    );
    assert_eq!(revision_ids(&history).len(), 3);
    let results = stdout_json(
        &run_cli(
            config_path,
            &["search", "keyword", space_id, "locate-task-a"],
        )
        .await,
        "keyword search on reopen",
    );
    assert!(contains_string(&results, "locate-task-a"));
}

async fn state_issue_rest_access(
    state: &AppState,
    public_key_jwk: serde_json::Value,
) -> ugoite_server::TestRestAccess {
    state
        .issue_test_rest_access(public_key_jwk)
        .await
        .expect("issue test REST credential")
}
