//! JOURNEY-KNOWLEDGE-001 through the server-backed CLI.
//!
//! Evidence identity: surface=cli, transport=remote. This runs the same
//! scenario as the core journey (Space create -> Form establish -> Entry
//! create -> Entry edit -> Search -> History -> Restore -> Reopen) through
//! the remote transport (`http::execute` operation calls) against a real
//! in-process server, and asserts the same durable postconditions through
//! canonical reads. Transport and auth ceremony stay in setup; validation,
//! concurrency, and history semantics must match the core outcome.

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
    let mut command_args = vec![
        "--config".to_string(),
        config_path.to_str().expect("config path").to_string(),
    ];
    command_args.extend(args.iter().copied().map(str::to_string));
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

async fn run_cli_owned(config_path: &std::path::Path, args: &[String]) -> Output {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_cli(config_path, &refs).await
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

fn contains_substring(value: &serde_json::Value, needle: &str) -> bool {
    match value {
        serde_json::Value::String(text) => text.contains(needle),
        serde_json::Value::Array(items) => {
            items.iter().any(|item| contains_substring(item, needle))
        }
        serde_json::Value::Object(fields) => {
            fields.values().any(|item| contains_substring(item, needle))
        }
        _ => false,
    }
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
async fn journey_cli_remote_reaches_durable_outcome() {
    tokio::time::timeout(Duration::from_secs(120), journey_cli_remote())
        .await
        .expect("journey CLI remote test timed out");
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
        format!("memory://cli-journey-remote-{}", uuid::Uuid::now_v7()),
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
        device_name: "Journey remote test".to_string(),
        public_key_jwk,
        private_key_pkcs8: Some(
            URL_SAFE_NO_PAD.encode(
                key.to_pkcs8_der()
                    .expect("encode test private key")
                    .as_bytes(),
            ),
        ),
        access_token: access.access_token,
        refresh_token: "unused-in-journey-test".to_string(),
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
        "journey".to_string(),
        ContextConfig {
            connection: "remote-api".to_string(),
            space_uid: access.space_uid,
            credential: Some("journey".to_string()),
        },
    );
    config.current_context = Some("journey".to_string());
    let mut profile = serde_json::to_value(&session).expect("serialize CLI credential");
    profile["connection"] = serde_json::Value::String("remote-api".to_string());
    let credentials = serde_json::json!({
        "version": 1,
        "credentials": { "journey": profile },
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

async fn journey_cli_remote() {
    let fixture = setup_remote().await;
    let config_path = &fixture.config_path;
    let space_id: &str = &fixture.space_id;

    // Bare Space IDs select the remote transport in every command below.
    //
    // The journey Space itself comes from credential issuance (fixture
    // setup): remote `space create` is unreachable for token identities by
    // product design, because Space creation requires a browser session with
    // a recent Passkey plus the node-admin role. That transport boundary is
    // tracked separately and is not represented as journey evidence here.
    let form_name = "JourneyRemoteForm";
    let needle = "journey-remote-needle";
    let entry_id = "journey-remote-entry";

    // Space create is intentionally not driven remotely (see above): prove
    // the provisioned Space is durable and reopenable through remote reads.
    let space = stdout_json(&run_cli(config_path, &["space", "get"]).await, "space get");
    assert!(contains_string(&space, space_id));

    // Form establish via `form update`: the upsert path behind a weaker name.
    let form_file = config_path
        .parent()
        .expect("config parent")
        .join("journey-remote-form.json");
    std::fs::write(
        &form_file,
        format!(
            "{{\"name\":\"{form_name}\",\"version\":1,\"template\":\"# {form_name}\\n\\n## Status\\n\\n## Body\\n\",\"fields\":{{\"Status\":{{\"type\":\"string\",\"required\":true}},\"Body\":{{\"type\":\"markdown\"}}}}}}"
        ),
    )
    .expect("write journey form");
    let output = run_cli(
        config_path,
        &["form", "update", form_file.to_str().unwrap()],
    )
    .await;
    assert!(
        output.status.success(),
        "form establish failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let form = stdout_json(
        &run_cli(config_path, &["form", "get", form_name]).await,
        "form get",
    );
    assert_eq!(
        form.get("name").and_then(|name| name.as_str()),
        Some(form_name)
    );
    assert_eq!(
        form.pointer("/fields/Status/type").and_then(|t| t.as_str()),
        Some("string")
    );
    assert_eq!(
        form.pointer("/fields/Status/required")
            .and_then(|r| r.as_bool()),
        Some(true)
    );

    // Entry create appends exactly one revision.
    let created = stdout_json(
        &run_cli(
            config_path,
            &[
                "entry",
                "create",
                entry_id,
                "--form",
                form_name,
                "--field",
                &format!("Status={needle}"),
                "--field",
                "Body=journey remote v1",
            ],
        )
        .await,
        "entry create",
    );
    assert!(contains_string(&created, entry_id));
    let create_change_id = created
        .get("change_id")
        .and_then(|id| id.as_str())
        .expect("create returns durable change_id")
        .to_string();
    let history = stdout_json(
        &run_cli(config_path, &["entry", "history", entry_id]).await,
        "entry history after create",
    );
    let ids = revision_ids(&history);
    assert_eq!(ids.len(), 1);
    assert_eq!(
        history["revisions"][0]["change_id"],
        serde_json::Value::String(create_change_id)
    );
    let rev1 = ids[0].clone();

    // Entry edit appends a revision; a stale parent conflicts.
    let output = run_cli(
        config_path,
        &[
            "entry",
            "update",
            entry_id,
            "--field",
            &format!("Status={needle}"),
            "--field",
            "Body=journey remote v2",
            "--parent-revision-id",
            &rev1,
        ],
    )
    .await;
    assert!(
        output.status.success(),
        "entry update failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let updated = stdout_json(&output, "entry update");
    let update_change_id = updated
        .get("change_id")
        .and_then(|id| id.as_str())
        .expect("update returns durable change_id")
        .to_string();
    let history = stdout_json(
        &run_cli(config_path, &["entry", "history", entry_id]).await,
        "entry history after edit",
    );
    let ids = revision_ids(&history);
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&rev1));
    let rev2 = ids.into_iter().find(|id| id != &rev1).expect("rev2");
    let stale = run_cli(
        config_path,
        &[
            "entry",
            "update",
            entry_id,
            "--field",
            &format!("Status={needle}"),
            "--field",
            "Body=journey remote v2",
            "--parent-revision-id",
            &rev1,
        ],
    )
    .await;
    assert!(
        !stale.status.success(),
        "stale parent revision must conflict instead of overwriting"
    );

    // EntryQuery text search finds the updated durable Entry.
    let results = stdout_json(
        &run_cli(config_path, &["entry", "list", "--text", needle]).await,
        "entry list --text",
    );
    assert!(
        contains_string(&results, entry_id),
        "search must find the updated entry: {results}"
    );

    // Restore appends a new revision replaying rev1; history never shortens.
    let output = run_cli(config_path, &["entry", "restore", entry_id, &rev1]).await;
    assert!(
        output.status.success(),
        "entry restore failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let history = stdout_json(
        &run_cli(config_path, &["entry", "history", entry_id]).await,
        "entry history after restore",
    );
    let ids = revision_ids(&history);
    assert_eq!(ids.len(), 3);
    assert!(ids.contains(&rev1));
    assert!(ids.contains(&rev2));
    let rev3 = ids
        .into_iter()
        .find(|id| id != &rev1 && id != &rev2)
        .expect("rev3");
    assert_eq!(
        history["revisions"]
            .as_array()
            .expect("history revisions")
            .iter()
            .find(|revision| revision["revision_id"] == rev2)
            .expect("updated revision")
            .get("change_id")
            .and_then(|id| id.as_str()),
        Some(update_change_id.as_str())
    );
    let restore = stdout_json(&output, "entry restore");
    let restore_change_id = restore
        .get("change_id")
        .and_then(|id| id.as_str())
        .expect("restore returns durable change_id")
        .to_string();
    assert_eq!(
        history["revisions"]
            .as_array()
            .expect("history revisions")
            .iter()
            .find(|revision| revision["revision_id"] == rev3)
            .expect("restored revision")
            .get("change_id")
            .and_then(|id| id.as_str()),
        Some(restore_change_id.as_str())
    );
    let revision = stdout_json(
        &run_cli(config_path, &["entry", "revision", entry_id, &rev3]).await,
        "entry revision after restore",
    );
    assert_eq!(
        revision.get("revision_id").and_then(|id| id.as_str()),
        Some(rev3.as_str())
    );
    assert!(
        contains_substring(&revision, "journey remote v1"),
        "restored revision must replay rev1 content: {revision}"
    );

    // Reopen: fresh processes read identical durable state.
    let space = stdout_json(
        &run_cli(config_path, &["space", "get"]).await,
        "space get on reopen",
    );
    assert!(contains_string(&space, space_id));
    let history = stdout_json(
        &run_cli(config_path, &["entry", "history", entry_id]).await,
        "entry history on reopen",
    );
    assert_eq!(revision_ids(&history).len(), 3);
    let results = stdout_json(
        &run_cli(config_path, &["entry", "list", "--text", needle]).await,
        "entry list --text on reopen",
    );
    assert!(contains_string(&results, entry_id));
}

/// The server-backed CLI uses the same parent-selection and conflict rules as
/// core mode for structured and compatibility updates.
#[tokio::test]
async fn test_cli_remote_entry_update_parent_revision_matrix() {
    let fixture = setup_remote().await;
    setup_parity_form(
        &fixture,
        r#"{"Status":{"type":"string"},"Body":{"type":"markdown"}}"#,
        "ParentMatrixRemoteForm",
    )
    .await;

    let structured_created = stdout_json(
        &run_cli(
            &fixture.config_path,
            &[
                "entry",
                "create",
                "parent-matrix-structured",
                "--form",
                "ParentMatrixRemoteForm",
                "--field",
                "Body=structured v1",
            ],
        )
        .await,
        "remote structured matrix create",
    );
    assert!(contains_string(
        &structured_created,
        "parent-matrix-structured"
    ));
    let structured_history = stdout_json(
        &run_cli(
            &fixture.config_path,
            &["entry", "history", "parent-matrix-structured"],
        )
        .await,
        "remote structured matrix history after create",
    );
    let structured_rev1 = revision_ids(&structured_history)[0].clone();

    let explicit = run_cli(
        &fixture.config_path,
        &[
            "entry",
            "update",
            "parent-matrix-structured",
            "--field",
            "Body=structured explicit",
            "--parent-revision-id",
            &structured_rev1,
        ],
    )
    .await;
    assert!(
        explicit.status.success(),
        "remote explicit structured update failed: {}",
        String::from_utf8_lossy(&explicit.stderr)
    );
    let structured_history = stdout_json(
        &run_cli(
            &fixture.config_path,
            &["entry", "history", "parent-matrix-structured"],
        )
        .await,
        "remote structured matrix history after explicit update",
    );
    let structured_rev2 = revision_ids(&structured_history)
        .into_iter()
        .find(|revision| revision != &structured_rev1)
        .expect("remote structured rev2");
    let structured_revision = stdout_json(
        &run_cli(
            &fixture.config_path,
            &[
                "entry",
                "revision",
                "parent-matrix-structured",
                &structured_rev2,
            ],
        )
        .await,
        "remote structured matrix revision after explicit update",
    );
    assert_eq!(structured_revision["parent_revision_id"], structured_rev1);

    let omitted = run_cli(
        &fixture.config_path,
        &[
            "entry",
            "update",
            "parent-matrix-structured",
            "--field",
            "Body=structured omitted",
        ],
    )
    .await;
    assert!(
        omitted.status.success(),
        "remote omitted structured update failed: {}",
        String::from_utf8_lossy(&omitted.stderr)
    );
    let structured_history = stdout_json(
        &run_cli(
            &fixture.config_path,
            &["entry", "history", "parent-matrix-structured"],
        )
        .await,
        "remote structured matrix history after omitted update",
    );
    let structured_rev3 = revision_ids(&structured_history)
        .into_iter()
        .find(|revision| revision != &structured_rev1 && revision != &structured_rev2)
        .expect("remote structured rev3");
    let structured_revision = stdout_json(
        &run_cli(
            &fixture.config_path,
            &[
                "entry",
                "revision",
                "parent-matrix-structured",
                &structured_rev3,
            ],
        )
        .await,
        "remote structured matrix revision after omitted update",
    );
    assert_eq!(structured_revision["parent_revision_id"], structured_rev2);

    let created = stdout_json(
        &run_cli(
            &fixture.config_path,
            &[
                "entry",
                "create",
                "parent-matrix-canonical",
                "--form",
                "ParentMatrixRemoteForm",
                "--field",
                "Body=canonical v1",
            ],
        )
        .await,
        "remote canonical matrix create",
    );
    assert!(contains_string(&created, "parent-matrix-canonical"));
    let history = stdout_json(
        &run_cli(
            &fixture.config_path,
            &["entry", "history", "parent-matrix-canonical"],
        )
        .await,
        "remote canonical matrix history after create",
    );
    let canonical_rev1 = revision_ids(&history)[0].clone();

    let explicit = run_cli(
        &fixture.config_path,
        &[
            "entry",
            "update",
            "parent-matrix-canonical",
            "--field",
            "Body=canonical v2",
            "--parent-revision-id",
            &canonical_rev1,
        ],
    )
    .await;
    assert!(
        explicit.status.success(),
        "remote explicit canonical update failed: {}",
        String::from_utf8_lossy(&explicit.stderr)
    );
    let history = stdout_json(
        &run_cli(
            &fixture.config_path,
            &["entry", "history", "parent-matrix-canonical"],
        )
        .await,
        "remote canonical matrix history after explicit update",
    );
    let canonical_rev2 = revision_ids(&history)
        .into_iter()
        .find(|revision| revision != &canonical_rev1)
        .expect("remote canonical rev2");
    let revision = stdout_json(
        &run_cli(
            &fixture.config_path,
            &[
                "entry",
                "revision",
                "parent-matrix-canonical",
                &canonical_rev2,
            ],
        )
        .await,
        "remote canonical matrix revision after explicit update",
    );
    assert_eq!(revision["parent_revision_id"], canonical_rev1);

    let omitted = run_cli(
        &fixture.config_path,
        &[
            "entry",
            "update",
            "parent-matrix-canonical",
            "--field",
            "Body=canonical v3",
        ],
    )
    .await;
    assert!(
        omitted.status.success(),
        "remote omitted canonical update failed: {}",
        String::from_utf8_lossy(&omitted.stderr)
    );
    let history = stdout_json(
        &run_cli(
            &fixture.config_path,
            &["entry", "history", "parent-matrix-canonical"],
        )
        .await,
        "remote canonical matrix history after omitted update",
    );
    let canonical_rev3 = revision_ids(&history)
        .into_iter()
        .find(|revision| revision != &canonical_rev1 && revision != &canonical_rev2)
        .expect("remote canonical rev3");
    let revision = stdout_json(
        &run_cli(
            &fixture.config_path,
            &[
                "entry",
                "revision",
                "parent-matrix-canonical",
                &canonical_rev3,
            ],
        )
        .await,
        "remote canonical matrix revision after omitted update",
    );
    assert_eq!(revision["parent_revision_id"], canonical_rev2);

    let stale = run_cli(
        &fixture.config_path,
        &[
            "entry",
            "update",
            "parent-matrix-canonical",
            "--field",
            "Body=canonical v3",
            "--parent-revision-id",
            &canonical_rev1,
        ],
    )
    .await;
    assert!(!stale.status.success(), "remote stale parent must conflict");
    assert!(String::from_utf8_lossy(&stale.stderr).contains("REVISION_CONFLICT"));
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

// --- Semantic parity corpus (surface=cli, transport=remote) ---
//
// Each case asserts the same two things the core corpus asserts: the
// machine-readable failure classification and the unchanged durable state.
// Presentation wording is never compared across surfaces.

async fn setup_parity_form(fixture: &RemoteFixture, form_fields: &str, form_name: &str) {
    let form_file = fixture
        .config_path
        .parent()
        .expect("config parent")
        .join(format!("parity-remote-{form_name}.json"));
    std::fs::write(
        &form_file,
        format!(
            "{{\"name\":\"{form_name}\",\"version\":1,\"template\":\"# {form_name}\\n\\n## Status\\n\\n## Body\\n\",\"fields\":{form_fields}}}"
        ),
    )
    .expect("write parity form");
    let output = run_cli(
        &fixture.config_path,
        &["form", "update", form_file.to_str().unwrap()],
    )
    .await;
    assert!(
        output.status.success(),
        "parity setup form establish failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn parity_fields(status: Option<&str>, body: &str) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(status) = status {
        args.push("--field".to_string());
        args.push(format!("Status={status}"));
    }
    args.push("--field".to_string());
    args.push(format!("Body={body}"));
    args
}

async fn entry_absent(fixture: &RemoteFixture, entry_id: &str) {
    let output = run_cli(&fixture.config_path, &["entry", "get", entry_id]).await;
    assert!(
        !output.status.success(),
        "rejected mutation must not persist an entry"
    );
}

async fn create_parity_entry(
    fixture: &RemoteFixture,
    entry_id: &str,
    status: Option<&str>,
    body: &str,
) -> String {
    let mut args = vec![
        "entry".to_string(),
        "create".to_string(),
        entry_id.to_string(),
        "--form".to_string(),
        "ParityRemoteForm".to_string(),
    ];
    args.extend(parity_fields(status, body));
    let created = stdout_json(
        &run_cli_owned(&fixture.config_path, &args).await,
        "parity setup entry create",
    );
    assert!(contains_string(&created, entry_id));
    let history = stdout_json(
        &run_cli(&fixture.config_path, &["entry", "history", entry_id]).await,
        "parity setup history",
    );
    let ids = revision_ids(&history);
    assert_eq!(ids.len(), 1);
    ids[0].clone()
}

/// Invalid field values are rejected with field-identifying validation
/// semantics and persist nothing.
#[tokio::test]
async fn test_parity_remote_invalid_field_rejected_without_mutation() {
    let fixture = setup_remote().await;
    let fixture: &RemoteFixture = &fixture;
    setup_parity_form(
            fixture,
            "{\"Status\":{\"type\":\"string\",\"required\":true},\"Count\":{\"type\":\"double\"},\"Body\":{\"type\":\"markdown\"}}",
            "ParityRemoteForm",
        )
        .await;
    let output = run_cli(
        &fixture.config_path,
        &[
            "entry",
            "create",
            "parity-invalid",
            "--form",
            "ParityRemoteForm",
            "--field",
            "Status=ok",
            "--field",
            "Count=not-a-number",
            "--field",
            "Body=x",
        ],
    )
    .await;
    assert!(!output.status.success(), "mistyped field must be rejected");
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Remote renders the shared warning payload inline instead of the core
    // formatted lines; the field-identifying classification must still
    // match. Presentation drift is tracked separately.
    assert!(stderr.contains("invalid"), "stderr: {stderr}");
    assert!(stderr.contains("Count"), "stderr: {stderr}");
    entry_absent(fixture, "parity-invalid").await;
}

/// Missing required fields are rejected and persist nothing.
#[tokio::test]
async fn test_parity_remote_missing_required_rejected_without_mutation() {
    let fixture = setup_remote().await;
    let fixture: &RemoteFixture = &fixture;
    setup_parity_form(
        fixture,
        "{\"Status\":{\"type\":\"string\",\"required\":true},\"Body\":{\"type\":\"markdown\"}}",
        "ParityRemoteForm",
    )
    .await;
    let output = run_cli(
        &fixture.config_path,
        &[
            "entry",
            "create",
            "parity-missing",
            "--form",
            "ParityRemoteForm",
            "--field",
            "Body=x",
        ],
    )
    .await;
    assert!(
        !output.status.success(),
        "missing required field must be rejected"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Same transport-rendering note as the invalid-field case above.
    assert!(stderr.contains("required"), "stderr: {stderr}");
    assert!(stderr.contains("Status"), "stderr: {stderr}");
    entry_absent(fixture, "parity-missing").await;
}

/// Stale parents conflict with 409-equivalent semantics and persist nothing.
#[tokio::test]
async fn test_parity_remote_stale_revision_conflicts_without_mutation() {
    let fixture = setup_remote().await;
    let fixture: &RemoteFixture = &fixture;
    setup_parity_form(
        fixture,
        "{\"Status\":{\"type\":\"string\",\"required\":true},\"Body\":{\"type\":\"markdown\"}}",
        "ParityRemoteForm",
    )
    .await;
    let rev1 = create_parity_entry(fixture, "parity-stale", Some("ok"), "v1").await;
    let updated = run_cli(
        &fixture.config_path,
        &[
            "entry",
            "update",
            "parity-stale",
            "--field",
            "Status=ok",
            "--field",
            "Body=v2",
            "--parent-revision-id",
            &rev1,
        ],
    )
    .await;
    assert!(updated.status.success());
    let stale = run_cli(
        &fixture.config_path,
        &[
            "entry",
            "update",
            "parity-stale",
            "--field",
            "Status=ok",
            "--field",
            "Body=v2",
            "--parent-revision-id",
            &rev1,
        ],
    )
    .await;
    assert!(!stale.status.success(), "stale parent must conflict");
    let stderr = String::from_utf8_lossy(&stale.stderr);
    assert!(stderr.contains("Revision conflict"), "stderr: {stderr}");
    let history = stdout_json(
        &run_cli(&fixture.config_path, &["entry", "history", "parity-stale"]).await,
        "parity history after conflict",
    );
    assert_eq!(revision_ids(&history).len(), 2);
}

/// Restoring an unknown revision is rejected as not-found; history unchanged.
#[tokio::test]
async fn test_parity_remote_restore_unknown_revision_rejected_without_mutation() {
    let fixture = setup_remote().await;
    let fixture: &RemoteFixture = &fixture;
    setup_parity_form(
        fixture,
        "{\"Status\":{\"type\":\"string\",\"required\":true},\"Body\":{\"type\":\"markdown\"}}",
        "ParityRemoteForm",
    )
    .await;
    create_parity_entry(fixture, "parity-restore", Some("ok"), "v1").await;
    let output = run_cli(
        &fixture.config_path,
        &[
            "entry",
            "restore",
            "parity-restore",
            "00000000-0000-0000-0000-000000000000",
        ],
    )
    .await;
    assert!(
        !output.status.success(),
        "unknown revision must be rejected"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not found"), "stderr: {stderr}");
    let history = stdout_json(
        &run_cli(
            &fixture.config_path,
            &["entry", "history", "parity-restore"],
        )
        .await,
        "parity history after rejected restore",
    );
    assert_eq!(revision_ids(&history).len(), 1);
}

/// Unknown Forms are rejected with form-identifying classification.
#[tokio::test]
async fn test_parity_remote_missing_form_rejected_without_mutation() {
    let fixture = setup_remote().await;
    let fixture: &RemoteFixture = &fixture;
    setup_parity_form(
        fixture,
        "{\"Status\":{\"type\":\"string\",\"required\":true},\"Body\":{\"type\":\"markdown\"}}",
        "ParityRemoteForm",
    )
    .await;
    let output = run_cli(
        &fixture.config_path,
        &[
            "entry",
            "create",
            "parity-noform",
            "--form",
            "NoSuchFormParity",
            "--field",
            "Status=ok",
            "--field",
            "Body=x",
        ],
    )
    .await;
    assert!(!output.status.success(), "unknown form must be rejected");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Form not found: NoSuchFormParity"),
        "stderr: {stderr}"
    );
    entry_absent(fixture, "parity-noform").await;
}

/// Unauthenticated remote mutations are rejected without mutation. Local
/// core has no auth boundary, so this case is remote-only by design.
#[tokio::test]
async fn test_parity_remote_unauthenticated_mutation_rejected_without_mutation() {
    let fixture = setup_remote().await;
    let fixture: &RemoteFixture = &fixture;
    setup_parity_form(
        fixture,
        "{\"Status\":{\"type\":\"string\",\"required\":true},\"Body\":{\"type\":\"markdown\"}}",
        "ParityRemoteForm",
    )
    .await;
    create_parity_entry(fixture, "parity-auth", Some("ok"), "v1").await;

    let bare_dir = tempfile::tempdir().expect("bare config directory");
    let bare_config = bare_dir.path().join("config.toml");
    let api_url = {
        let raw = std::fs::read_to_string(&fixture.config_path).expect("read endpoint config");
        let parsed: toml::Value = toml::from_str(&raw).expect("parse canonical config");
        parsed
            .get("connections")
            .and_then(|connections| connections.get("remote-api"))
            .and_then(|connection| connection.get("url"))
            .and_then(|url| url.as_str())
            .expect("remote-api URL")
            .to_string()
    };
    let bare_space_uid = fixture.space_id.parse::<uuid::Uuid>().expect("Space UID");
    std::fs::write(
        &bare_config,
        format!(
            "version = 1\ncurrent_context = \"bare\"\n\n[connections.remote-api]\ntype = \"api\"\nurl = \"{api_url}\"\n\n[contexts.bare]\nconnection = \"remote-api\"\nspace_uid = \"{bare_space_uid}\"\n"
        ),
    )
    .expect("write bare canonical config");

    let denied = run_cli(
        &bare_config,
        &[
            "entry",
            "update",
            "parity-auth",
            "--field",
            "Status=ok",
            "--field",
            "Body=v2",
        ],
    )
    .await;
    assert!(
        !denied.status.success(),
        "unauthenticated mutation must be rejected"
    );
    let stderr = String::from_utf8_lossy(&denied.stderr);
    assert!(stderr.contains("access token"), "stderr: {stderr}");
    let history = stdout_json(
        &run_cli(&fixture.config_path, &["entry", "history", "parity-auth"]).await,
        "parity history after rejected mutation",
    );
    assert_eq!(revision_ids(&history).len(), 1);
}

/// Remote deletes without a single-use human approval are rejected and
/// mutate nothing. The approval ceremony itself stays a browser session
/// concern; the positive tombstone path carries core evidence only.
#[tokio::test]
async fn test_parity_remote_delete_without_approval_rejected_without_mutation() {
    let fixture = setup_remote().await;
    let fixture: &RemoteFixture = &fixture;
    setup_parity_form(
        fixture,
        "{\"Status\":{\"type\":\"string\",\"required\":true},\"Body\":{\"type\":\"markdown\"}}",
        "ParityRemoteForm",
    )
    .await;
    create_parity_entry(fixture, "parity-delauth", Some("ok"), "v1").await;
    let denied = run_cli(&fixture.config_path, &["entry", "delete", "parity-delauth"]).await;
    assert!(
        !denied.status.success(),
        "unapproved remote delete must be rejected"
    );
    let stderr = String::from_utf8_lossy(&denied.stderr);
    assert!(stderr.contains("human approval"), "stderr: {stderr}");
    let current = run_cli(&fixture.config_path, &["entry", "get", "parity-delauth"]).await;
    assert!(
        current.status.success(),
        "rejected delete must leave the entry readable"
    );
    let history = stdout_json(
        &run_cli(
            &fixture.config_path,
            &["entry", "history", "parity-delauth"],
        )
        .await,
        "parity history after rejected delete",
    );
    assert_eq!(revision_ids(&history).len(), 1);
}

/// Remote asset upload returns a sanitized reference (M03).
#[tokio::test]
async fn test_remote_asset_upload_returns_reference() {
    let fixture = setup_remote().await;
    let dir = tempdir().expect("asset staging directory");
    let file = dir.path().join("remote-note.txt");
    std::fs::write(&file, b"remote upload bytes").expect("stage asset file");
    let output = run_cli(
        &fixture.config_path,
        &["asset", "upload", file.to_str().expect("asset path")],
    )
    .await;
    let asset = stdout_json(&output, "remote asset upload");
    for key in ["asset_id", "name", "media_type", "size_bytes", "sha256"] {
        assert!(
            asset.get(key).is_some(),
            "reference is missing {key}: {asset}"
        );
    }
    assert_eq!(asset["name"], "remote-note.txt");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains(dir.path().to_str().unwrap_or("\0")),
        "reference must not leak the local path"
    );
}

/// A second remote upload for the same workflow succeeds (M12).
#[tokio::test]
async fn test_remote_asset_second_upload_returns_reference() {
    let fixture = setup_remote().await;
    let dir = tempdir().expect("asset staging directory");
    let first = dir.path().join("remote-first.txt");
    let second = dir.path().join("remote-second.txt");
    std::fs::write(&first, b"first workflow bytes").expect("stage first file");
    std::fs::write(&second, b"second workflow bytes").expect("stage second file");
    let first_asset = stdout_json(
        &run_cli(
            &fixture.config_path,
            &["asset", "upload", first.to_str().expect("asset path")],
        )
        .await,
        "first remote asset upload",
    );
    let second_asset = stdout_json(
        &run_cli(
            &fixture.config_path,
            &["asset", "upload", second.to_str().expect("asset path")],
        )
        .await,
        "second remote asset upload",
    );
    assert_ne!(first_asset["asset_id"], second_asset["asset_id"]);
    assert_eq!(second_asset["name"], "remote-second.txt");
}

/// Remote asset upload strips traversal from explicit filenames.
#[tokio::test]
async fn test_remote_asset_upload_strips_filename_traversal() {
    let fixture = setup_remote().await;
    let dir = tempdir().expect("asset staging directory");
    let file = dir.path().join("remote-evil.txt");
    std::fs::write(&file, b"traversal bytes").expect("stage asset file");
    let asset = stdout_json(
        &run_cli(
            &fixture.config_path,
            &[
                "asset",
                "upload",
                file.to_str().expect("asset path"),
                "--filename",
                "nested/../../outside.txt",
            ],
        )
        .await,
        "remote traversal asset upload",
    );
    assert_eq!(asset["name"], "outside.txt");
}

/// Remote asset upload fails closed past the size limit.
#[tokio::test]
async fn test_remote_asset_upload_rejects_oversize() {
    let fixture = setup_remote().await;
    let dir = tempdir().expect("asset staging directory");
    let file = dir.path().join("remote-huge.bin");
    let oversize = ugoite_iceberg::asset::MAX_ASSET_BYTES + 1;
    let chunk = vec![7u8; 1024 * 1024];
    let mut handle = std::fs::File::create(&file).expect("stage oversize file");
    let mut remaining = oversize;
    while remaining > 0 {
        let take = remaining.min(chunk.len());
        std::io::Write::write_all(&mut handle, &chunk[..take]).expect("grow file");
        remaining -= take;
    }
    drop(handle);
    let output = run_cli(
        &fixture.config_path,
        &["asset", "upload", file.to_str().expect("asset path")],
    )
    .await;
    assert!(!output.status.success(), "oversize upload must be rejected");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("size limit"), "stderr: {stderr}");
}

/// Lane 1 acceptance: the consolidated structured fixture reaches the real
/// server-backed CLI transport, not only the core implementation.
#[tokio::test]
async fn test_lane1_parity_fixture_converges_on_cli_remote() {
    let acceptance_fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/entry/structured-compat/10-structured-authoring-parity.json"
    ))
    .expect("read structured authoring parity fixture");
    let fixture = setup_remote().await;
    let staging = tempdir().expect("parity staging directory");

    let task_form_file = staging.path().join("parity-task-form.json");
    std::fs::write(
        &task_form_file,
        r##"{"name":"ParityTask","version":1,"template":"# ParityTask","fields":{"Summary":{"type":"string"}}}"##,
    )
    .expect("write task form");
    let task_form_update = run_cli(
        &fixture.config_path,
        &[
            "form",
            "update",
            task_form_file.to_str().expect("task form path"),
        ],
    )
    .await;
    assert!(task_form_update.status.success());
    let task_form = stdout_json(
        &run_cli(&fixture.config_path, &["form", "get", "ParityTask"]).await,
        "get parity task form",
    );
    let task_form_id = task_form["id"].as_str().expect("task form id");

    let mut parity_form_fields = serde_json::Map::new();
    for fixture_field in acceptance_fixture["form"]["fields"]
        .as_array()
        .expect("fixture form fields")
    {
        let name = fixture_field["name"].as_str().expect("fixture field name");
        let mut field = fixture_field
            .as_object()
            .expect("fixture field object")
            .clone();
        field.remove("name");
        field.remove("id");
        if let Some(field_type) = field.remove("field_type") {
            field.insert("type".to_string(), field_type);
        }
        if name == "Ref" {
            field.remove("reference_form");
            field.insert("target_form".to_string(), json!(task_form_id));
        }
        parity_form_fields.insert(name.to_string(), serde_json::Value::Object(field));
    }
    let parity_form_file = staging.path().join("parity-form.json");
    let parity_form = json!({
        "name": "ParityRemote",
        "version": 1,
        "template": "# ParityRemote",
        "fields": parity_form_fields
    });
    std::fs::write(
        &parity_form_file,
        serde_json::to_vec(&parity_form).expect("serialize parity form"),
    )
    .expect("write parity form");
    let form_update = run_cli(
        &fixture.config_path,
        &[
            "form",
            "update",
            parity_form_file.to_str().expect("parity form path"),
        ],
    )
    .await;
    assert!(
        form_update.status.success(),
        "parity form update failed: {}",
        String::from_utf8_lossy(&form_update.stderr)
    );
    let parity_form_read = stdout_json(
        &run_cli(&fixture.config_path, &["form", "get", "ParityRemote"]).await,
        "get remote parity form",
    );
    assert!(parity_form_read["id"].as_str().is_some());
    assert_eq!(
        parity_form_read["fields"]["Ref"]["target_form"],
        task_form_id
    );
    let parity_form_id = parity_form_read["id"].clone();
    let stable_field_ids: serde_json::Map<String, serde_json::Value> = parity_form_read["fields"]
        .as_object()
        .expect("remote parity fields")
        .iter()
        .map(|(name, field)| {
            (
                name.clone(),
                field.get("id").cloned().expect("remote field id"),
            )
        })
        .collect();
    let parity_form_reopened = stdout_json(
        &run_cli(&fixture.config_path, &["form", "get", "ParityRemote"]).await,
        "reopen remote parity form",
    );
    assert_eq!(parity_form_reopened["id"], parity_form_id);
    for (name, field_id) in stable_field_ids {
        assert_eq!(parity_form_reopened["fields"][name]["id"], field_id);
    }

    let target_output = run_cli(
        &fixture.config_path,
        &[
            "entry",
            "create",
            "parity-task-01",
            "--form",
            "ParityTask",
            "--field",
            "Summary=build",
        ],
    )
    .await;
    assert!(target_output.status.success());
    let target_two_output = run_cli(
        &fixture.config_path,
        &[
            "entry",
            "create",
            "parity-task-02",
            "--form",
            "ParityTask",
            "--field",
            "Summary=review",
        ],
    )
    .await;
    assert!(target_two_output.status.success());

    let asset_file = staging.path().join("spec.pdf");
    std::fs::write(&asset_file, b"spec-bytes").expect("write parity asset");
    let asset = stdout_json(
        &run_cli(
            &fixture.config_path,
            &["asset", "upload", asset_file.to_str().expect("asset path")],
        )
        .await,
        "upload parity asset",
    );
    let mut fields = acceptance_fixture["structured"]["fields"]
        .as_object()
        .expect("fixture structured fields")
        .clone();
    fields.insert("Ref".to_string(), json!("parity-task-01"));
    fields.insert("File".to_string(), asset.clone());
    fields.insert("Files".to_string(), json!([asset.clone()]));
    let fields = serde_json::Value::Object(fields);
    let fields_file = staging.path().join("parity-fields.json");
    std::fs::write(
        &fields_file,
        serde_json::to_vec(&fields).expect("serialize parity fields"),
    )
    .expect("write parity fields");
    let created = stdout_json(
        &run_cli(
            &fixture.config_path,
            &[
                "entry",
                "create",
                "parity-remote-entry",
                "--form",
                "ParityRemote",
                "--fields-file",
                fields_file.to_str().expect("fields path"),
            ],
        )
        .await,
        "remote parity structured create",
    );
    assert!(contains_string(&created, "parity-remote-entry"));

    let entry = stdout_json(
        &run_cli(
            &fixture.config_path,
            &["entry", "get", "parity-remote-entry"],
        )
        .await,
        "remote parity entry get",
    );
    assert_eq!(entry["form"], "ParityRemote");
    assert_eq!(
        entry["sections"]["Headline"],
        acceptance_fixture["expected"]["values"]["100"]
    );
    assert_eq!(entry["sections"]["Ref"], "parity-task-01");
    assert_eq!(
        entry["sections"]["At"],
        acceptance_fixture["expected"]["values"]["106"]
    );
    assert_eq!(
        entry["sections"]["AtNs"],
        acceptance_fixture["expected"]["values"]["112"]
    );
    let at_tz = entry["sections"]["AtTz"].as_str().expect("timestamp_tz");
    let at_tz_ns = entry["sections"]["AtTzNs"]
        .as_str()
        .expect("timestamp_tz_ns");
    assert_eq!(
        chrono::DateTime::parse_from_rfc3339(at_tz)
            .expect("valid timestamp_tz")
            .timestamp(),
        chrono::DateTime::parse_from_rfc3339("2026-09-11T10:00:00+09:00")
            .expect("valid expected timestamp_tz")
            .timestamp()
    );
    assert_eq!(
        chrono::DateTime::parse_from_rfc3339(at_tz_ns)
            .expect("valid timestamp_tz_ns")
            .timestamp_nanos_opt(),
        chrono::DateTime::parse_from_rfc3339("2026-09-11T10:00:00.123456789+09:00")
            .expect("valid expected timestamp_tz_ns")
            .timestamp_nanos_opt()
    );
    assert_eq!(entry["sections"]["Labels"], "- alpha\n- beta");
    assert!(!entry["sections"]["Rows"]
        .as_str()
        .unwrap_or_default()
        .is_empty());
    let parsed_file: serde_json::Value = serde_json::from_str(
        entry["sections"]["File"]
            .as_str()
            .expect("asset section is serialized JSON"),
    )
    .expect("asset section JSON");
    assert_eq!(parsed_file["asset_id"], asset["asset_id"]);
    let parsed_files: serde_json::Value = serde_json::from_str(
        entry["sections"]["Files"]
            .as_str()
            .expect("asset list section is serialized JSON"),
    )
    .expect("asset list section JSON");
    assert_eq!(parsed_files[0]["asset_id"], asset["asset_id"]);

    let history = stdout_json(
        &run_cli(
            &fixture.config_path,
            &["entry", "history", "parity-remote-entry"],
        )
        .await,
        "remote parity history after create",
    );
    assert_eq!(history["revisions"].as_array().unwrap().len(), 1);
    let rev1 = revision_ids(&history)[0].clone();
    let rev1_json = stdout_json(
        &run_cli(
            &fixture.config_path,
            &["entry", "revision", "parity-remote-entry", &rev1],
        )
        .await,
        "remote parity created revision",
    );
    for name in [
        "Headline", "Notes", "Done", "Count", "Score", "Due", "At", "AtNs", "AtTz", "AtTzNs",
        "Labels", "Rows", "Ref",
    ] {
        assert_eq!(
            rev1_json["sections"][name], entry["sections"][name],
            "durable field {name}"
        );
    }
    let revision_rows: serde_json::Value = serde_json::from_str(
        rev1_json["sections"]["Rows"]
            .as_str()
            .expect("revision object list section"),
    )
    .expect("revision object list JSON");
    assert_eq!(
        revision_rows,
        acceptance_fixture["expected"]["values"]["108"]
    );
    assert_eq!(rev1_json["sections"]["File"], entry["sections"]["File"]);
    assert_eq!(rev1_json["sections"]["Files"], entry["sections"]["Files"]);

    let mut updated_fields = acceptance_fixture["update"]["fields"]
        .as_object()
        .expect("fixture update fields")
        .clone();
    updated_fields.remove("Notes");
    updated_fields.remove("Labels");
    updated_fields.remove("Files");
    updated_fields.insert("Ref".to_string(), json!("parity-task-02"));
    updated_fields.insert("File".to_string(), asset.clone());
    let updated_fields_file = staging.path().join("parity-fields-update.json");
    std::fs::write(
        &updated_fields_file,
        serde_json::to_vec(&updated_fields).expect("serialize update fields"),
    )
    .expect("write update fields");
    let update = stdout_json(
        &run_cli(
            &fixture.config_path,
            &[
                "entry",
                "update",
                "parity-remote-entry",
                "--fields-file",
                updated_fields_file.to_str().expect("updated fields path"),
                "--parent-revision-id",
                &rev1,
            ],
        )
        .await,
        "remote parity structured update",
    );
    assert!(contains_string(&update, "parity-remote-entry"));

    let history = stdout_json(
        &run_cli(
            &fixture.config_path,
            &["entry", "history", "parity-remote-entry"],
        )
        .await,
        "remote parity history after update",
    );
    let revisions = history["revisions"].as_array().unwrap();
    assert_eq!(revisions.len(), 2);
    let rev2 = revision_ids(&history)
        .into_iter()
        .find(|id| id != &rev1)
        .expect("updated revision");
    let revision = stdout_json(
        &run_cli(
            &fixture.config_path,
            &["entry", "revision", "parity-remote-entry", &rev2],
        )
        .await,
        "remote parity updated revision",
    );
    assert_eq!(revision["parent_revision_id"], rev1);

    let reopened = stdout_json(
        &run_cli(
            &fixture.config_path,
            &["entry", "get", "parity-remote-entry"],
        )
        .await,
        "remote parity reopen",
    );
    assert_eq!(reopened["sections"]["Count"], "43");
    assert!(reopened["sections"].get("Notes").is_none());
    assert!(reopened["sections"].get("Labels").is_none());
    assert!(reopened["sections"].get("Files").is_none());

    let invalid = run_cli(
        &fixture.config_path,
        &[
            "entry",
            "create",
            "parity-remote-invalid",
            "--form",
            "ParityRemote",
            "--field",
            "Headline=hello",
            "--field",
            "Count=not-an-integer",
        ],
    )
    .await;
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("FORM_VALIDATION_FAILED"));
}
