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
use ugoite_cli::config::{AuthSession, EndpointConfig, EndpointMode};
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
    Command::new(ugoite_bin())
        .args(args)
        .env("UGOITE_CLI_CONFIG_PATH", config_path)
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
    let config_path = config_dir.path().join("cli-endpoints.json");
    let credentials_path = config_dir.path().join("cli-credentials.json");
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
    let config = EndpointConfig {
        mode: EndpointMode::Api,
        backend_url: server_url,
        api_url: api_base,
    };
    std::fs::write(
        &config_path,
        serde_json::to_vec_pretty(&config).expect("serialize endpoint config"),
    )
    .expect("write endpoint config");
    std::fs::write(
        &credentials_path,
        serde_json::to_vec_pretty(&session).expect("serialize CLI credential"),
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
    let space = stdout_json(
        &run_cli(config_path, &["space", "get", space_id]).await,
        "space get",
    );
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
        &["form", "update", space_id, form_file.to_str().unwrap()],
    )
    .await;
    assert!(
        output.status.success(),
        "form establish failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let form = stdout_json(
        &run_cli(config_path, &["form", "get", space_id, form_name]).await,
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
    let v1 = format!(
        "---\nform: {form_name}\n---\n# Journey remote v1\n\n## Status\n{needle}\n\n## Body\njourney remote v1\n"
    );
    let created = stdout_json(
        &run_cli(
            config_path,
            &["entry", "create", "--content", &v1, space_id, entry_id],
        )
        .await,
        "entry create",
    );
    assert!(contains_string(&created, entry_id));
    let history = stdout_json(
        &run_cli(config_path, &["entry", "history", space_id, entry_id]).await,
        "entry history after create",
    );
    let ids = revision_ids(&history);
    assert_eq!(ids.len(), 1);
    let rev1 = ids[0].clone();

    // Entry edit appends a revision; a stale parent conflicts.
    let v2 = format!(
        "---\nform: {form_name}\n---\n# Journey remote v2\n\n## Status\n{needle}\n\n## Body\njourney remote v2\n"
    );
    // NOTE: `--markdown=<value>` keeps frontmatter (leading `---`) from
    // parsing as a flag; the update flag lacks allow_hyphen_values.
    let markdown_arg = format!("--markdown={v2}");
    let output = run_cli(
        config_path,
        &[
            "entry",
            "update",
            space_id,
            entry_id,
            markdown_arg.as_str(),
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
    let history = stdout_json(
        &run_cli(config_path, &["entry", "history", space_id, entry_id]).await,
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
            space_id,
            entry_id,
            markdown_arg.as_str(),
            "--parent-revision-id",
            &rev1,
        ],
    )
    .await;
    assert!(
        !stale.status.success(),
        "stale parent revision must conflict instead of overwriting"
    );

    // Search finds the updated durable Entry.
    let results = stdout_json(
        &run_cli(config_path, &["search", "keyword", space_id, needle]).await,
        "search keyword",
    );
    assert!(
        contains_string(&results, entry_id),
        "search must find the updated entry: {results}"
    );

    // Restore appends a new revision replaying rev1; history never shortens.
    let output = run_cli(
        config_path,
        &["entry", "restore", space_id, entry_id, &rev1],
    )
    .await;
    assert!(
        output.status.success(),
        "entry restore failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let history = stdout_json(
        &run_cli(config_path, &["entry", "history", space_id, entry_id]).await,
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
    let revision = stdout_json(
        &run_cli(
            config_path,
            &["entry", "revision", space_id, entry_id, &rev3],
        )
        .await,
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
        &run_cli(config_path, &["space", "get", space_id]).await,
        "space get on reopen",
    );
    assert!(contains_string(&space, space_id));
    let history = stdout_json(
        &run_cli(config_path, &["entry", "history", space_id, entry_id]).await,
        "entry history on reopen",
    );
    assert_eq!(revision_ids(&history).len(), 3);
    let results = stdout_json(
        &run_cli(config_path, &["search", "keyword", space_id, needle]).await,
        "search keyword on reopen",
    );
    assert!(contains_string(&results, entry_id));
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
        &[
            "form",
            "update",
            &fixture.space_id,
            form_file.to_str().unwrap(),
        ],
    )
    .await;
    assert!(
        output.status.success(),
        "parity setup form establish failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn parity_markdown(form_name: &str, title: &str, status: Option<&str>, body: &str) -> String {
    let status_section = status
        .map(|value| format!("\n## Status\n{value}\n"))
        .unwrap_or_default();
    format!("---\nform: {form_name}\n---\n# {title}\n{status_section}\n## Body\n{body}\n")
}

async fn entry_absent(fixture: &RemoteFixture, entry_id: &str) {
    let output = run_cli(
        &fixture.config_path,
        &["entry", "get", &fixture.space_id, entry_id],
    )
    .await;
    assert!(
        !output.status.success(),
        "rejected mutation must not persist an entry"
    );
}

async fn create_parity_entry(fixture: &RemoteFixture, entry_id: &str, markdown: &str) -> String {
    let created = stdout_json(
        &run_cli(
            &fixture.config_path,
            &[
                "entry",
                "create",
                "--content",
                markdown,
                &fixture.space_id,
                entry_id,
            ],
        )
        .await,
        "parity setup entry create",
    );
    assert!(contains_string(&created, entry_id));
    let history = stdout_json(
        &run_cli(
            &fixture.config_path,
            &["entry", "history", &fixture.space_id, entry_id],
        )
        .await,
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
    let markdown = "---\nform: ParityRemoteForm\n---\n# Parity invalid\n\n## Status\nok\n\n## Count\nnot-a-number\n\n## Body\nx\n";
    let output = run_cli(
        &fixture.config_path,
        &[
            "entry",
            "create",
            "--content",
            markdown,
            &fixture.space_id,
            "parity-invalid",
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
    let markdown = parity_markdown("ParityRemoteForm", "Parity missing", None, "x");
    let output = run_cli(
        &fixture.config_path,
        &[
            "entry",
            "create",
            "--content",
            &markdown,
            &fixture.space_id,
            "parity-missing",
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
    let v1 = parity_markdown("ParityRemoteForm", "Parity stale", Some("ok"), "v1");
    let rev1 = create_parity_entry(fixture, "parity-stale", &v1).await;
    let v2 = parity_markdown("ParityRemoteForm", "Parity stale v2", Some("ok"), "v2");
    let markdown_arg = format!("--markdown={v2}");
    let updated = run_cli(
        &fixture.config_path,
        &[
            "entry",
            "update",
            &fixture.space_id,
            "parity-stale",
            markdown_arg.as_str(),
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
            &fixture.space_id,
            "parity-stale",
            markdown_arg.as_str(),
            "--parent-revision-id",
            &rev1,
        ],
    )
    .await;
    assert!(!stale.status.success(), "stale parent must conflict");
    let stderr = String::from_utf8_lossy(&stale.stderr);
    assert!(stderr.contains("Revision conflict"), "stderr: {stderr}");
    let history = stdout_json(
        &run_cli(
            &fixture.config_path,
            &["entry", "history", &fixture.space_id, "parity-stale"],
        )
        .await,
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
    let v1 = parity_markdown("ParityRemoteForm", "Parity restore", Some("ok"), "v1");
    create_parity_entry(fixture, "parity-restore", &v1).await;
    let output = run_cli(
        &fixture.config_path,
        &[
            "entry",
            "restore",
            &fixture.space_id,
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
            &["entry", "history", &fixture.space_id, "parity-restore"],
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
    let markdown = parity_markdown("NoSuchFormParity", "Parity noform", Some("ok"), "x");
    let output = run_cli(
        &fixture.config_path,
        &[
            "entry",
            "create",
            "--content",
            &markdown,
            &fixture.space_id,
            "parity-noform",
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
    let v1 = parity_markdown("ParityRemoteForm", "Parity auth", Some("ok"), "v1");
    create_parity_entry(fixture, "parity-auth", &v1).await;

    let bare_dir = tempfile::tempdir().expect("bare config directory");
    let bare_config = bare_dir.path().join("cli-endpoints.json");
    let api_url = {
        let raw = std::fs::read_to_string(&fixture.config_path).expect("read endpoint config");
        let parsed: serde_json::Value = serde_json::from_str(&raw).expect("parse endpoint config");
        parsed
            .get("api_url")
            .and_then(|url| url.as_str())
            .expect("api_url")
            .to_string()
    };
    std::fs::write(
        &bare_config,
        serde_json::to_vec_pretty(&serde_json::json!({
            "mode": "api",
            "backend_url": api_url,
            "api_url": api_url,
        }))
        .expect("serialize bare endpoint config"),
    )
    .expect("write bare endpoint config");

    let v2 = parity_markdown("ParityRemoteForm", "Parity auth v2", Some("ok"), "v2");
    let markdown_arg = format!("--markdown={v2}");
    let denied = run_cli(
        &bare_config,
        &[
            "entry",
            "update",
            &fixture.space_id,
            "parity-auth",
            markdown_arg.as_str(),
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
        &run_cli(
            &fixture.config_path,
            &["entry", "history", &fixture.space_id, "parity-auth"],
        )
        .await,
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
    let v1 = parity_markdown("ParityRemoteForm", "Parity delauth", Some("ok"), "v1");
    create_parity_entry(fixture, "parity-delauth", &v1).await;
    let denied = run_cli(
        &fixture.config_path,
        &["entry", "delete", &fixture.space_id, "parity-delauth"],
    )
    .await;
    assert!(
        !denied.status.success(),
        "unapproved remote delete must be rejected"
    );
    let stderr = String::from_utf8_lossy(&denied.stderr);
    assert!(stderr.contains("human approval"), "stderr: {stderr}");
    let current = run_cli(
        &fixture.config_path,
        &["entry", "get", &fixture.space_id, "parity-delauth"],
    )
    .await;
    assert!(
        current.status.success(),
        "rejected delete must leave the entry readable"
    );
    let history = stdout_json(
        &run_cli(
            &fixture.config_path,
            &["entry", "history", &fixture.space_id, "parity-delauth"],
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
        &[
            "asset",
            "upload",
            &fixture.space_id,
            file.to_str().expect("asset path"),
        ],
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
            &[
                "asset",
                "upload",
                &fixture.space_id,
                first.to_str().expect("asset path"),
            ],
        )
        .await,
        "first remote asset upload",
    );
    let second_asset = stdout_json(
        &run_cli(
            &fixture.config_path,
            &[
                "asset",
                "upload",
                &fixture.space_id,
                second.to_str().expect("asset path"),
            ],
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
                &fixture.space_id,
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
        &[
            "asset",
            "upload",
            &fixture.space_id,
            file.to_str().expect("asset path"),
        ],
    )
    .await;
    assert!(!output.status.success(), "oversize upload must be rejected");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("size limit"), "stderr: {stderr}");
}
