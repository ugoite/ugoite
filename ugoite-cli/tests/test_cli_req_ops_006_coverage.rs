//! CLI coverage tests for the shipped binary.
//! REQ-OPS-006

mod support;

use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;
use support::spawn_recording_server;

fn ugoite_bin() -> String {
    let mut path = std::env::current_exe().expect("current exe");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("ugoite");
    path.to_string_lossy().to_string()
}

fn cli_command(config_path: &Path) -> Command {
    let mut command = Command::new(ugoite_bin());
    command.env("UGOITE_CLI_CONFIG_PATH", config_path);
    command
}

fn assert_success(output: &Output, label: &str) {
    assert!(
        output.status.success(),
        "{label} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn parse_stdout_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("stdout json")
}

fn write_endpoint_config(config_path: &Path, mode: &str, backend_url: &str, api_url: &str) {
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).expect("create config parent");
    }
    std::fs::write(
        config_path,
        serde_json::json!({
            "mode": mode,
            "backend_url": backend_url,
            "api_url": api_url,
        })
        .to_string(),
    )
    .expect("write endpoint config");
}

fn setup_space(dir: &tempfile::TempDir, space_id: &str) -> (String, PathBuf, String) {
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let output = cli_command(&config_path)
        .args(["create-space", "--root", &root, space_id])
        .output()
        .expect("create space");
    assert_success(&output, "create-space");
    let space_path = format!("{root}/spaces/{space_id}");
    (root, config_path, space_path)
}

fn setup_space_with_form(dir: &tempfile::TempDir, space_id: &str) -> (String, PathBuf, String) {
    let (root, config_path, space_path) = setup_space(dir, space_id);
    let form_file = dir.path().join(format!("{space_id}-form.json"));
    std::fs::write(
        &form_file,
        r#"{"name":"Entry","fields":{"Body":{"type":"markdown"}}}"#,
    )
    .expect("write form file");
    let output = cli_command(&config_path)
        .args([
            "form",
            "update",
            &space_path,
            form_file.to_str().expect("form path"),
        ])
        .output()
        .expect("form update");
    assert_success(&output, "form update");
    (root, config_path, space_path)
}

fn create_entry(config_path: &Path, space_path: &str, entry_id: &str, content: &str) {
    let output = cli_command(config_path)
        .args([
            "entry",
            "create",
            "--content",
            content,
            space_path,
            entry_id,
        ])
        .output()
        .expect("entry create");
    assert_success(&output, "entry create");
}

/// REQ-OPS-006: CLI commands must keep error exits, auth flows, and invalid mode handling covered.
#[test]
fn test_cli_req_ops_006_main_auth_and_config_error_paths() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("cli-config.json");

    let invalid_mode = cli_command(&config_path)
        .args(["config", "set", "--mode", "invalid"])
        .output()
        .expect("invalid config mode");
    assert!(!invalid_mode.status.success());
    assert!(String::from_utf8_lossy(&invalid_mode.stderr)
        .contains("Invalid mode: invalid. Use core, backend, or api"));

    let config_without_mode = cli_command(&config_path)
        .args([
            "config",
            "set",
            "--backend-url",
            "http://backend.example.test",
        ])
        .output()
        .expect("config set without mode");
    assert_success(&config_without_mode, "config set without mode");
    assert_eq!(
        parse_stdout_json(&config_without_mode)["config"]["backend_url"].as_str(),
        Some("http://backend.example.test")
    );

    let (_base, _requests, _handle) =
        spawn_recording_server("HTTP/1.1 200 OK", r#"{"bearer_token":"core-mode-token"}"#);
    write_endpoint_config(&config_path, "core", "http://localhost:8000", "http://localhost:3000/api");
    let core_mode_login = cli_command(&config_path)
        .args([
            "auth",
            "login",
            "--username",
            "alice",
            "--totp-code",
            "123456",
        ])
        .output()
        .expect("core mode auth login");
    assert!(
        !core_mode_login.status.success(),
        "core mode auth login should fail with actionable error"
    );
    assert!(
        String::from_utf8_lossy(&core_mode_login.stderr)
            .contains("auth login requires backend or api mode"),
        "core mode error should mention mode requirement"
    );

    let (base, requests, handle) =
        spawn_recording_server("HTTP/1.1 200 OK", r#"{"bearer_token":"backend-mode-token"}"#);
    write_endpoint_config(&config_path, "backend", &base, &format!("{base}/api"));
    let backend_mode_login = cli_command(&config_path)
        .args([
            "auth",
            "login",
            "--username",
            "alice",
            "--totp-code",
            "123456",
        ])
        .output()
        .expect("backend mode auth login");
    assert_success(&backend_mode_login, "backend mode auth login");
    let core_mode_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("core mode request");
    handle.join().expect("join core mode server");
    assert!(core_mode_request.starts_with("POST /auth/login HTTP/1.1"));
    assert!(String::from_utf8_lossy(&backend_mode_login.stdout)
        .contains("export UGOITE_AUTH_BEARER_TOKEN=backend-mode-token"));

    write_endpoint_config(
        &config_path,
        "backend",
        "http://127.0.0.1:9",
        "http://127.0.0.1:9/api",
    );
    let invalid_totp = cli_command(&config_path)
        .args([
            "auth",
            "login",
            "--username",
            "alice",
            "--totp-code",
            "12345",
        ])
        .output()
        .expect("invalid totp login");
    assert!(!invalid_totp.status.success());
    assert!(
        String::from_utf8_lossy(&invalid_totp.stderr).contains("2FA code must be exactly 6 digits")
    );

    let (base, requests, handle) =
        spawn_recording_server("HTTP/1.1 200 OK", r#"{"bearer_token":"interactive-token"}"#);
    write_endpoint_config(&config_path, "backend", &base, &format!("{base}/api"));
    let mut child = cli_command(&config_path)
        .args(["auth", "login"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn interactive auth login");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"alice\n123456\n")
        .expect("write interactive auth login");
    let interactive_output = child.wait_with_output().expect("interactive auth output");
    assert_success(&interactive_output, "interactive auth login");
    let interactive_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("interactive request");
    handle.join().expect("join interactive server");
    assert!(interactive_request.starts_with("POST /auth/login HTTP/1.1"));
    assert!(interactive_request.contains(r#""username":"alice""#));
    assert!(interactive_request.contains(r#""totp_code":"123456""#));

    let mut child = cli_command(&config_path)
        .args(["auth", "login"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn empty username auth login");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"\n123456\n")
        .expect("write empty username auth login");
    let empty_username_output = child
        .wait_with_output()
        .expect("empty username auth login output");
    assert!(!empty_username_output.status.success());
    assert!(String::from_utf8_lossy(&empty_username_output.stderr)
        .contains("Username must not be empty"));

    let (base, requests, handle) =
        spawn_recording_server("HTTP/1.1 200 OK", r#"{"bearer_token":"oauth-token"}"#);
    write_endpoint_config(&config_path, "backend", &base, &format!("{base}/api"));
    let mock_oauth_output = cli_command(&config_path)
        .args(["auth", "login", "--mock-oauth"])
        .env("UGOITE_DEV_AUTH_PROXY_TOKEN", "proxy-secret")
        .output()
        .expect("mock oauth login");
    assert_success(&mock_oauth_output, "mock oauth login");
    let mock_oauth_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("mock oauth request");
    handle.join().expect("join mock oauth server");
    assert!(mock_oauth_request.starts_with("POST /auth/mock-oauth HTTP/1.1"));
    assert!(mock_oauth_request
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-auth-proxy-token: proxy-secret"));
}

/// REQ-OPS-006: auxiliary auth commands must keep masking, overview, and token clearing covered.
#[test]
fn test_cli_req_ops_006_auth_profile_token_clear_and_overview() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("cli-config.json");

    let profile_output = Command::new(ugoite_bin())
        .args(["auth", "profile"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .env("UGOITE_AUTH_BEARER_TOKEN", "1234567890")
        .env("UGOITE_AUTH_API_KEY", "short")
        .output()
        .expect("auth profile");
    assert_success(&profile_output, "auth profile");
    let profile_json = parse_stdout_json(&profile_output);
    assert_eq!(
        profile_json["UGOITE_AUTH_BEARER_TOKEN"].as_str(),
        Some("1234...")
    );
    assert_eq!(profile_json["UGOITE_AUTH_API_KEY"].as_str(), Some("****"));

    let token_clear_output = cli_command(&config_path)
        .args(["auth", "token-clear"])
        .output()
        .expect("auth token-clear");
    assert_success(&token_clear_output, "auth token-clear");
    let clear_stdout = String::from_utf8_lossy(&token_clear_output.stdout);
    assert!(clear_stdout.contains("unset UGOITE_AUTH_BEARER_TOKEN"));
    assert!(clear_stdout.contains("unset UGOITE_AUTH_API_KEY"));

    let overview_output = cli_command(&config_path)
        .args(["auth", "overview"])
        .output()
        .expect("auth overview");
    assert_success(&overview_output, "auth overview");
    assert!(parse_stdout_json(&overview_output).is_object());
}

/// REQ-OPS-006: asset commands must cover local delete, remote list/delete, and remote upload rejection.
#[test]
fn test_cli_req_ops_006_asset_local_and_remote_paths() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (_root, config_path, space_path) = setup_space(&dir, "asset-space");
    let asset_file = dir.path().join("asset.txt");
    std::fs::write(&asset_file, b"asset body").expect("write asset file");

    let upload_output = cli_command(&config_path)
        .args([
            "asset",
            "upload",
            &space_path,
            asset_file.to_str().expect("asset path"),
        ])
        .output()
        .expect("asset upload");
    assert_success(&upload_output, "asset upload");

    let list_output = cli_command(&config_path)
        .args(["asset", "list", &space_path])
        .output()
        .expect("asset list");
    assert_success(&list_output, "asset list");
    let list_json = parse_stdout_json(&list_output);
    let asset_id = list_json
        .as_array()
        .and_then(|items| items.first())
        .and_then(|asset| asset.get("id"))
        .and_then(Value::as_str)
        .expect("asset id")
        .to_string();

    let delete_output = cli_command(&config_path)
        .args(["asset", "delete", &space_path, &asset_id])
        .output()
        .expect("asset delete");
    assert_success(&delete_output, "asset delete");
    assert_eq!(
        parse_stdout_json(&delete_output),
        serde_json::json!({"deleted": true})
    );

    let list_after_delete = cli_command(&config_path)
        .args(["asset", "list", &space_path])
        .output()
        .expect("asset list after delete");
    assert_success(&list_after_delete, "asset list after delete");
    assert_eq!(parse_stdout_json(&list_after_delete), serde_json::json!([]));

    let remote_config_path = dir.path().join("remote-config.json");
    let (base, requests, handle) =
        spawn_recording_server("HTTP/1.1 200 OK", r#"[{"id":"remote-asset"}]"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_list_output = cli_command(&remote_config_path)
        .args(["asset", "list", "remote-space"])
        .output()
        .expect("remote asset list");
    assert_success(&remote_list_output, "remote asset list");
    let remote_list_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote asset list request");
    handle.join().expect("join remote asset list server");
    assert!(remote_list_request.starts_with("GET /spaces/remote-space/assets HTTP/1.1"));

    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"deleted":true}"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_delete_output = cli_command(&remote_config_path)
        .args(["asset", "delete", "remote-space", "remote-asset"])
        .output()
        .expect("remote asset delete");
    assert_success(&remote_delete_output, "remote asset delete");
    let remote_delete_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote asset delete request");
    handle.join().expect("join remote asset delete server");
    assert!(remote_delete_request
        .starts_with("DELETE /spaces/remote-space/assets/remote-asset HTTP/1.1"));

    write_endpoint_config(
        &remote_config_path,
        "backend",
        "http://127.0.0.1:9",
        "http://127.0.0.1:9/api",
    );
    let remote_upload_output = cli_command(&remote_config_path)
        .args([
            "asset",
            "upload",
            "remote-space",
            asset_file.to_str().expect("asset path"),
        ])
        .output()
        .expect("remote asset upload");
    assert!(!remote_upload_output.status.success());
    assert!(String::from_utf8_lossy(&remote_upload_output.stderr)
        .contains("asset upload in remote mode not yet supported via CLI"));
}

/// REQ-OPS-006: form commands must keep local listing/migration and remote routing covered.
#[test]
fn test_cli_req_ops_006_form_local_and_remote_paths() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (_root, config_path, space_path) = setup_space_with_form(&dir, "form-space");
    create_entry(
        &config_path,
        &space_path,
        "entry-1",
        "---\nform: Entry\n---\n# Demo Entry\n\n## Body\n\nInitial body.",
    );

    let list_output = cli_command(&config_path)
        .args(["form", "list", &space_path])
        .output()
        .expect("form list");
    assert_success(&list_output, "form list");
    assert!(String::from_utf8_lossy(&list_output.stdout).contains("Entry"));

    let migrated_form_file = dir.path().join("migrated-form.json");
    std::fs::write(
        &migrated_form_file,
        r#"{"name":"Entry","fields":{"Body":{"type":"markdown"},"Status":{"type":"text"}}}"#,
    )
    .expect("write migrated form");
    let update_output = cli_command(&config_path)
        .args([
            "form",
            "update",
            &space_path,
            migrated_form_file.to_str().expect("form path"),
            "--strategies",
            r#"{"Status":"Open"}"#,
        ])
        .output()
        .expect("form update with strategies");
    assert_success(&update_output, "form update with strategies");

    let null_strategy_output = cli_command(&config_path)
        .args([
            "form",
            "update",
            &space_path,
            migrated_form_file.to_str().expect("form path"),
            "--strategies",
            "null",
        ])
        .output()
        .expect("form update with null strategies");
    assert_success(&null_strategy_output, "form update with null strategies");

    let entry_get_output = cli_command(&config_path)
        .args(["entry", "get", &space_path, "entry-1"])
        .output()
        .expect("entry get after form migration");
    assert_success(&entry_get_output, "entry get after form migration");
    assert!(String::from_utf8_lossy(&entry_get_output.stdout).contains("Open"));

    let remote_config_path = dir.path().join("remote-form-config.json");
    let (base, requests, handle) =
        spawn_recording_server("HTTP/1.1 200 OK", r#"[{"name":"Entry"}]"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_list_output = cli_command(&remote_config_path)
        .args(["form", "list", "remote-space"])
        .output()
        .expect("remote form list");
    assert_success(&remote_list_output, "remote form list");
    let remote_list_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote form list request");
    handle.join().expect("join remote form list server");
    assert!(remote_list_request.starts_with("GET /spaces/remote-space/forms HTTP/1.1"));

    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"name":"Entry"}"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_get_output = cli_command(&remote_config_path)
        .args(["form", "get", "remote-space", "Entry"])
        .output()
        .expect("remote form get");
    assert_success(&remote_get_output, "remote form get");
    let remote_get_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote form get request");
    handle.join().expect("join remote form get server");
    assert!(remote_get_request.starts_with("GET /spaces/remote-space/forms/Entry HTTP/1.1"));

    let remote_form_file = dir.path().join("remote-form.json");
    std::fs::write(
        &remote_form_file,
        r#"{"name":"Entry","fields":{"Body":{"type":"markdown"}}}"#,
    )
    .expect("write remote form");
    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"updated":true}"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_update_output = cli_command(&remote_config_path)
        .args([
            "form",
            "update",
            "remote-space",
            remote_form_file.to_str().expect("remote form path"),
        ])
        .output()
        .expect("remote form update");
    assert_success(&remote_update_output, "remote form update");
    let remote_update_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote form update request");
    handle.join().expect("join remote form update server");
    assert!(remote_update_request.starts_with("PUT /spaces/remote-space/forms/Entry HTTP/1.1"));
    assert!(remote_update_request.contains(r#""name":"Entry""#));
}

/// REQ-OPS-006: search, index/query, and deprecated link routing must all stay covered.
#[test]
fn test_cli_req_ops_006_search_index_query_and_link_paths() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (_root, config_path, space_path) = setup_space_with_form(&dir, "search-space");
    create_entry(
        &config_path,
        &space_path,
        "entry-alpha",
        "---\nform: Entry\n---\n# Alpha Entry\n\n## Body\n\nalpha keyword here.",
    );

    let local_search_output = cli_command(&config_path)
        .args(["search", "keyword", &space_path, "alpha"])
        .output()
        .expect("local search keyword");
    assert_success(&local_search_output, "local search keyword");
    assert!(String::from_utf8_lossy(&local_search_output.stdout).contains("entry-alpha"));

    let remote_config_path = dir.path().join("remote-search-config.json");
    let (base, requests, handle) =
        spawn_recording_server("HTTP/1.1 200 OK", r#"{"reindexed":true}"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_index_run_output = cli_command(&remote_config_path)
        .args(["index", "run", "remote-space"])
        .output()
        .expect("remote index run");
    assert_success(&remote_index_run_output, "remote index run");
    let remote_index_run_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote index run request");
    handle.join().expect("join remote index run server");
    assert!(remote_index_run_request.starts_with("POST /spaces/remote-space/index HTTP/1.1"));

    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"entries":2}"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_index_stats_output = cli_command(&remote_config_path)
        .args(["index", "stats", "remote-space"])
        .output()
        .expect("remote index stats");
    assert_success(&remote_index_stats_output, "remote index stats");
    let remote_index_stats_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote index stats request");
    handle.join().expect("join remote index stats server");
    assert!(remote_index_stats_request.starts_with("GET /spaces/remote-space/stats HTTP/1.1"));

    let (base, requests, handle) =
        spawn_recording_server("HTTP/1.1 200 OK", r#"[{"id":"entry-alpha"}]"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_query_output = cli_command(&remote_config_path)
        .args(["query", "remote-space", "--sql", "SELECT id FROM entries"])
        .output()
        .expect("remote query");
    assert_success(&remote_query_output, "remote query");
    let remote_query_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote query request");
    handle.join().expect("join remote query server");
    assert!(remote_query_request.starts_with("GET /spaces/remote-space/query?sql="));
    assert!(remote_query_request.contains("SELECT"));

    let (base, requests, handle) =
        spawn_recording_server("HTTP/1.1 200 OK", r#"[{"id":"entry-alpha"}]"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_search_output = cli_command(&remote_config_path)
        .args(["search", "keyword", "remote-space", "alpha"])
        .output()
        .expect("remote search");
    assert_success(&remote_search_output, "remote search");
    let remote_search_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote search request");
    handle.join().expect("join remote search server");
    assert!(remote_search_request.starts_with("GET /spaces/remote-space/search?q=alpha HTTP/1.1"));

    let link_create_output = cli_command(&config_path)
        .args([
            "link",
            "create",
            "/tmp/demo",
            "space-1",
            "entry-1",
            "entry-2",
            "--kind",
            "related",
        ])
        .output()
        .expect("link create");
    assert!(!link_create_output.status.success());
    assert!(String::from_utf8_lossy(&link_create_output.stderr)
        .contains("Link commands removed. Use row_reference fields instead."));

    let link_list_output = cli_command(&config_path)
        .args(["link", "list", "/tmp/demo", "space-1", "entry-1"])
        .output()
        .expect("link list");
    assert!(!link_list_output.status.success());
    assert!(String::from_utf8_lossy(&link_list_output.stderr)
        .contains("Link commands removed. Use row_reference fields instead."));

    let link_delete_output = cli_command(&config_path)
        .args(["link", "delete", "/tmp/demo", "space-1", "link-1"])
        .output()
        .expect("link delete");
    assert!(!link_delete_output.status.success());
    assert!(String::from_utf8_lossy(&link_delete_output.stderr)
        .contains("Link commands removed. Use row_reference fields instead."));
}

/// REQ-OPS-006: space commands must keep local sample/test flows and remote-only routes covered.
#[test]
fn test_cli_req_ops_006_space_local_and_remote_paths() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (root, config_path, space_path) = setup_space(&dir, "space-local");

    let list_output = cli_command(&config_path)
        .args(["space", "list", "--root", &root])
        .output()
        .expect("space list");
    assert_success(&list_output, "space list");
    assert!(String::from_utf8_lossy(&list_output.stdout).contains("space-local"));

    let get_output = cli_command(&config_path)
        .args(["space", "get", "--root", &root, "space-local"])
        .output()
        .expect("space get");
    assert_success(&get_output, "space get");
    assert_eq!(
        parse_stdout_json(&get_output)["id"].as_str(),
        Some("space-local")
    );

    let patch_output = cli_command(&config_path)
        .args([
            "space",
            "patch",
            "--root",
            &root,
            "space-local",
            "--name",
            "Renamed Space",
            "--storage-config",
            r#"{"uri":"file:///tmp/demo"}"#,
            "--settings",
            r#"{"theme":"dark"}"#,
        ])
        .output()
        .expect("space patch");
    assert_success(&patch_output, "space patch");
    let patch_json = parse_stdout_json(&patch_output);
    assert_eq!(patch_json["name"].as_str(), Some("Renamed Space"));

    let name_only_patch_output = cli_command(&config_path)
        .args([
            "space",
            "patch",
            "--root",
            &root,
            "space-local",
            "--name",
            "Name Only",
        ])
        .output()
        .expect("space patch name only");
    assert_success(&name_only_patch_output, "space patch name only");

    let sample_data_output = cli_command(&config_path)
        .args([
            "space",
            "sample-data",
            &root,
            "sample-space",
            "--scenario",
            "renewable-ops",
            "--entry-count",
            "6",
            "--seed",
            "1",
        ])
        .output()
        .expect("space sample-data");
    assert_success(&sample_data_output, "space sample-data");
    assert_eq!(
        parse_stdout_json(&sample_data_output),
        serde_json::json!({"created": true})
    );

    let scenarios_output = cli_command(&config_path)
        .args(["space", "sample-scenarios"])
        .output()
        .expect("space sample-scenarios");
    assert_success(&scenarios_output, "space sample-scenarios");
    assert!(parse_stdout_json(&scenarios_output)
        .as_array()
        .map(|items| !items.is_empty())
        .unwrap_or(false));

    let sample_job_output = cli_command(&config_path)
        .args([
            "space",
            "sample-job",
            &root,
            "job-space",
            "--scenario",
            "renewable-ops",
            "--entry-count",
            "6",
            "--seed",
            "2",
        ])
        .output()
        .expect("space sample-job");
    assert_success(&sample_job_output, "space sample-job");
    let sample_job_json = parse_stdout_json(&sample_job_output);
    let job_id = sample_job_json["job_id"]
        .as_str()
        .expect("sample job id")
        .to_string();

    let sample_job_status_output = cli_command(&config_path)
        .args(["space", "sample-job-status", &root, &job_id])
        .output()
        .expect("space sample-job-status");
    assert_success(&sample_job_status_output, "space sample-job-status");
    assert_eq!(
        parse_stdout_json(&sample_job_status_output)["job_id"].as_str(),
        Some(job_id.as_str())
    );

    for (uri, expected_mode) in [
        ("/tmp/demo", "local"),
        ("memory://", "memory"),
        ("s3://bucket/demo", "s3"),
        ("ftp://unsupported", "unknown"),
    ] {
        let connection_output = cli_command(&config_path)
            .args(["space", "test-connection", &format!(r#"{{"uri":"{uri}"}}"#)])
            .output()
            .expect("space test-connection");
        assert_success(&connection_output, "space test-connection");
        assert_eq!(
            parse_stdout_json(&connection_output)["mode"].as_str(),
            Some(expected_mode)
        );
    }

    let missing_root_output = cli_command(&config_path)
        .args(["space", "list"])
        .output()
        .expect("space list missing root");
    assert!(!missing_root_output.status.success());
    assert!(String::from_utf8_lossy(&missing_root_output.stderr)
        .contains("space list requires --root <LOCAL_ROOT> in core mode"));

    let service_account_list_core = cli_command(&config_path)
        .args(["space", "service-account-list", "space-local"])
        .output()
        .expect("space service-account-list core");
    assert!(!service_account_list_core.status.success());
    assert!(String::from_utf8_lossy(&service_account_list_core.stderr)
        .contains("service-account-list requires backend or api mode"));

    let service_account_create_core = cli_command(&config_path)
        .args([
            "space",
            "service-account-create",
            "space-local",
            "--display-name",
            "Bot",
            "--scopes",
            "read,write",
        ])
        .output()
        .expect("space service-account-create core");
    assert!(!service_account_create_core.status.success());
    assert!(String::from_utf8_lossy(&service_account_create_core.stderr)
        .contains("service-account-create requires backend or api mode"));

    let members_core = cli_command(&config_path)
        .args(["space", "members", &space_path])
        .output()
        .expect("space members core");
    assert!(!members_core.status.success());
    assert!(String::from_utf8_lossy(&members_core.stderr)
        .contains("members requires backend or api mode"));

    let audit_events_core = cli_command(&config_path)
        .args(["space", "audit-events", &space_path])
        .output()
        .expect("space audit-events core");
    assert!(!audit_events_core.status.success());
    assert!(String::from_utf8_lossy(&audit_events_core.stderr)
        .contains("audit-events requires backend or api mode"));

    let remote_config_path = dir.path().join("remote-space-config.json");
    let (base, requests, handle) =
        spawn_recording_server("HTTP/1.1 200 OK", r#"{"id":"remote-space"}"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_get_output = cli_command(&remote_config_path)
        .args(["space", "get", "remote-space"])
        .output()
        .expect("remote space get");
    assert_success(&remote_get_output, "remote space get");
    let remote_get_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote space get request");
    handle.join().expect("join remote space get server");
    assert!(remote_get_request.starts_with("GET /spaces/remote-space HTTP/1.1"));

    let (base, requests, handle) = spawn_recording_server(
        "HTTP/1.1 200 OK",
        r#"{"id":"remote-space","name":"Remote"}"#,
    );
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_patch_output = cli_command(&remote_config_path)
        .args([
            "space",
            "patch",
            "remote-space",
            "--name",
            "Remote",
            "--storage-config",
            r#"{"uri":"memory://remote"}"#,
            "--settings",
            r#"{"theme":"dark"}"#,
        ])
        .output()
        .expect("remote space patch");
    assert_success(&remote_patch_output, "remote space patch");
    let remote_patch_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote space patch request");
    handle.join().expect("join remote space patch server");
    assert!(remote_patch_request.starts_with("PATCH /spaces/remote-space HTTP/1.1"));
    assert!(remote_patch_request.contains(r#""name":"Remote""#));
    assert!(remote_patch_request.contains(r#""storage_config":{"uri":"memory://remote"}"#));
    assert!(remote_patch_request.contains(r#""settings":{"theme":"dark"}"#));

    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"[]"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_service_account_list_output = cli_command(&remote_config_path)
        .args(["space", "service-account-list", "remote-space"])
        .output()
        .expect("remote service-account-list");
    assert_success(
        &remote_service_account_list_output,
        "remote service-account-list",
    );
    let remote_service_account_list_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote service-account-list request");
    handle
        .join()
        .expect("join remote service-account-list server");
    assert!(remote_service_account_list_request
        .starts_with("GET /spaces/remote-space/service-accounts HTTP/1.1"));

    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"id":"svc-1"}"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_service_account_create_output = cli_command(&remote_config_path)
        .args([
            "space",
            "service-account-create",
            "remote-space",
            "--display-name",
            "Bot",
            "--scopes",
            "read,write",
        ])
        .output()
        .expect("remote service-account-create");
    assert_success(
        &remote_service_account_create_output,
        "remote service-account-create",
    );
    let remote_service_account_create_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote service-account-create request");
    handle
        .join()
        .expect("join remote service-account-create server");
    assert!(remote_service_account_create_request
        .starts_with("POST /spaces/remote-space/service-accounts HTTP/1.1"));
    assert!(remote_service_account_create_request.contains(r#""display_name":"Bot""#));
    assert!(remote_service_account_create_request.contains(r#""scopes":["read","write"]"#));

    let (base, requests, handle) =
        spawn_recording_server("HTTP/1.1 200 OK", r#"[{"user_id":"alice"}]"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_members_output = cli_command(&remote_config_path)
        .args(["space", "members", "remote-space"])
        .output()
        .expect("remote members");
    assert_success(&remote_members_output, "remote members");
    let remote_members_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote members request");
    handle.join().expect("join remote members server");
    assert!(remote_members_request.starts_with("GET /spaces/remote-space/members HTTP/1.1"));

    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"events":[]}"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_audit_events_output = cli_command(&remote_config_path)
        .args([
            "space",
            "audit-events",
            "remote-space",
            "--offset",
            "5",
            "--limit",
            "10",
        ])
        .output()
        .expect("remote audit-events");
    assert_success(&remote_audit_events_output, "remote audit-events");
    let remote_audit_events_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote audit-events request");
    handle.join().expect("join remote audit-events server");
    assert!(remote_audit_events_request
        .starts_with("GET /spaces/remote-space/audit-events?offset=5&limit=10 HTTP/1.1"));
}

/// REQ-OPS-006: entry commands must keep full local lifecycle and remote routing covered.
#[test]
fn test_cli_req_ops_006_entry_local_and_remote_paths() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (_root, config_path, space_path) = setup_space_with_form(&dir, "entry-space");
    create_entry(
        &config_path,
        &space_path,
        "entry-1",
        "---\nform: Entry\n---\n# Entry One\n\n## Body\n\nInitial body.",
    );
    let local_markdown =
        "--markdown=---\nform: Entry\n---\n# Entry One\n\n## Body\n\nUpdated body.";

    let local_update_output = cli_command(&config_path)
        .args([
            "entry",
            "update",
            local_markdown,
            "--assets",
            "[]",
            &space_path,
            "entry-1",
        ])
        .output()
        .expect("entry update");
    assert_success(&local_update_output, "entry update");

    let history_output = cli_command(&config_path)
        .args(["entry", "history", &space_path, "entry-1"])
        .output()
        .expect("entry history");
    assert_success(&history_output, "entry history");
    let history_json = parse_stdout_json(&history_output);
    let revision_id = history_json["revisions"]
        .as_array()
        .and_then(|revisions| revisions.first())
        .and_then(|revision| revision.get("revision_id"))
        .and_then(Value::as_str)
        .expect("revision id")
        .to_string();

    let revision_output = cli_command(&config_path)
        .args(["entry", "revision", &space_path, "entry-1", &revision_id])
        .output()
        .expect("entry revision");
    assert_success(&revision_output, "entry revision");

    let restore_output = cli_command(&config_path)
        .args(["entry", "restore", &space_path, "entry-1", &revision_id])
        .output()
        .expect("entry restore");
    assert_success(&restore_output, "entry restore");

    let delete_output = cli_command(&config_path)
        .args(["entry", "delete", &space_path, "entry-1"])
        .output()
        .expect("entry delete");
    assert_success(&delete_output, "entry delete");
    assert_eq!(
        parse_stdout_json(&delete_output),
        serde_json::json!({"deleted": true})
    );

    let remote_config_path = dir.path().join("remote-entry-config.json");
    let (base, requests, handle) =
        spawn_recording_server("HTTP/1.1 200 OK", r#"[{"id":"entry-1"}]"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_list_output = cli_command(&remote_config_path)
        .args(["entry", "list", "remote-space"])
        .output()
        .expect("remote entry list");
    assert_success(&remote_list_output, "remote entry list");
    let remote_list_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote entry list request");
    handle.join().expect("join remote entry list server");
    assert!(remote_list_request.starts_with("GET /spaces/remote-space/entries HTTP/1.1"));

    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"id":"entry-1"}"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_get_output = cli_command(&remote_config_path)
        .args(["entry", "get", "remote-space", "entry-1"])
        .output()
        .expect("remote entry get");
    assert_success(&remote_get_output, "remote entry get");
    let remote_get_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote entry get request");
    handle.join().expect("join remote entry get server");
    assert!(remote_get_request.starts_with("GET /spaces/remote-space/entries/entry-1 HTTP/1.1"));

    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"id":"entry-1"}"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_create_output = cli_command(&remote_config_path)
        .args([
            "entry",
            "create",
            "remote-space",
            "entry-1",
            "--content",
            "# Remote Entry",
            "--author",
            "remote-author",
        ])
        .output()
        .expect("remote entry create");
    assert_success(&remote_create_output, "remote entry create");
    let remote_create_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote entry create request");
    handle.join().expect("join remote entry create server");
    assert!(remote_create_request.starts_with("POST /spaces/remote-space/entries/entry-1 HTTP/1.1"));
    assert!(remote_create_request.contains("\"content\":\"# Remote Entry\""));
    assert!(remote_create_request.contains(r#""author":"remote-author""#));

    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"updated":true}"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_update_output = cli_command(&remote_config_path)
        .args([
            "entry",
            "update",
            "--markdown",
            "# Remote Update",
            "--parent-revision-id",
            "rev-1",
            "--assets",
            "[]",
            "--author",
            "remote-author",
            "remote-space",
            "entry-1",
        ])
        .output()
        .expect("remote entry update");
    assert_success(&remote_update_output, "remote entry update");
    let remote_update_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote entry update request");
    handle.join().expect("join remote entry update server");
    assert!(remote_update_request.starts_with("PUT /spaces/remote-space/entries/entry-1 HTTP/1.1"));
    assert!(remote_update_request.contains(r#""parent_revision_id":"rev-1""#));
    assert!(
        remote_update_request.contains(r#""assets":[],"#)
            || remote_update_request.contains(r#""assets":[]}"#)
    );

    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"updated":true}"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_update_without_assets_output = cli_command(&remote_config_path)
        .args([
            "entry",
            "update",
            "--markdown",
            "# Remote Update Without Assets",
            "--author",
            "remote-author",
            "remote-space",
            "entry-1",
        ])
        .output()
        .expect("remote entry update without assets");
    assert_success(
        &remote_update_without_assets_output,
        "remote entry update without assets",
    );
    let remote_update_without_assets_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote entry update without assets request");
    handle
        .join()
        .expect("join remote entry update without assets server");
    assert!(
        !remote_update_without_assets_request.contains(r#""assets":"#),
        "assets should be omitted when the CLI flag is not provided"
    );

    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"deleted":true}"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_delete_output = cli_command(&remote_config_path)
        .args(["entry", "delete", "remote-space", "entry-1"])
        .output()
        .expect("remote entry delete");
    assert_success(&remote_delete_output, "remote entry delete");
    let remote_delete_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote entry delete request");
    handle.join().expect("join remote entry delete server");
    assert!(
        remote_delete_request.starts_with("DELETE /spaces/remote-space/entries/entry-1 HTTP/1.1")
    );

    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"deleted":true}"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_hard_delete_output = cli_command(&remote_config_path)
        .args([
            "entry",
            "delete",
            "remote-space",
            "entry-1",
            "--hard-delete",
        ])
        .output()
        .expect("remote entry hard delete");
    assert_success(&remote_hard_delete_output, "remote entry hard delete");
    let remote_hard_delete_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote entry hard delete request");
    handle.join().expect("join remote entry hard delete server");
    assert!(remote_hard_delete_request
        .starts_with("DELETE /spaces/remote-space/entries/entry-1?hard_delete=true HTTP/1.1"));

    let (base, requests, handle) = spawn_recording_server(
        "HTTP/1.1 200 OK",
        r#"{"revisions":[{"revision_id":"rev-1"}]}"#,
    );
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_history_output = cli_command(&remote_config_path)
        .args(["entry", "history", "remote-space", "entry-1"])
        .output()
        .expect("remote entry history");
    assert_success(&remote_history_output, "remote entry history");
    let remote_history_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote entry history request");
    handle.join().expect("join remote entry history server");
    assert!(remote_history_request
        .starts_with("GET /spaces/remote-space/entries/entry-1/history HTTP/1.1"));

    let (base, requests, handle) =
        spawn_recording_server("HTTP/1.1 200 OK", r#"{"revision_id":"rev-1"}"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_revision_output = cli_command(&remote_config_path)
        .args(["entry", "revision", "remote-space", "entry-1", "rev-1"])
        .output()
        .expect("remote entry revision");
    assert_success(&remote_revision_output, "remote entry revision");
    let remote_revision_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote entry revision request");
    handle.join().expect("join remote entry revision server");
    assert!(remote_revision_request
        .starts_with("GET /spaces/remote-space/entries/entry-1/revisions/rev-1 HTTP/1.1"));

    let (base, requests, handle) =
        spawn_recording_server("HTTP/1.1 200 OK", r#"{"restored":true}"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_restore_output = cli_command(&remote_config_path)
        .args([
            "entry",
            "restore",
            "remote-space",
            "entry-1",
            "rev-1",
            "--author",
            "remote-author",
        ])
        .output()
        .expect("remote entry restore");
    assert_success(&remote_restore_output, "remote entry restore");
    let remote_restore_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote entry restore request");
    handle.join().expect("join remote entry restore server");
    assert!(remote_restore_request
        .starts_with("POST /spaces/remote-space/entries/entry-1/restore/rev-1 HTTP/1.1"));
    assert!(remote_restore_request.contains(r#""author":"remote-author""#));
}

/// REQ-OPS-006: SQL commands must keep lint, local CRUD, and remote routing covered.
#[test]
fn test_cli_req_ops_006_sql_local_and_remote_paths() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (root, config_path, space_path) = setup_space(&dir, "sql-space");

    let lint_output = cli_command(&config_path)
        .args(["sql", "lint", "SELECT * FROM entries"])
        .output()
        .expect("sql lint");
    assert_success(&lint_output, "sql lint");
    assert_eq!(
        parse_stdout_json(&lint_output),
        serde_json::json!({"valid": true, "sql": "SELECT * FROM entries"})
    );

    let saved_create_output = cli_command(&config_path)
        .args([
            "sql",
            "saved-create",
            &space_path,
            "sql-1",
            "--name",
            "Demo Query",
            "--sql",
            "SELECT * FROM entries",
            "--variables",
            "not-json",
        ])
        .output()
        .expect("sql saved-create");
    assert_success(&saved_create_output, "sql saved-create");

    let saved_list_output = cli_command(&config_path)
        .args(["sql", "saved-list", &space_path])
        .output()
        .expect("sql saved-list");
    assert_success(&saved_list_output, "sql saved-list");
    assert!(
        String::from_utf8_lossy(&saved_list_output.stdout).contains("sql-1"),
        "saved SQL list should include created query"
    );

    let saved_get_output = cli_command(&config_path)
        .args(["sql", "saved-get", &space_path, "sql-1"])
        .output()
        .expect("sql saved-get");
    assert_success(&saved_get_output, "sql saved-get");

    let saved_update_output = cli_command(&config_path)
        .args([
            "sql",
            "saved-update",
            &space_path,
            "sql-1",
            "--name",
            "Updated Query",
            "--sql",
            "SELECT * FROM entries LIMIT {{limit}}",
            "--variables",
            r#"[{"name":"limit","type":"number","description":"Row limit"}]"#,
            "--author",
            "cli",
        ])
        .output()
        .expect("sql saved-update");
    assert_success(&saved_update_output, "sql saved-update");

    let saved_delete_output = cli_command(&config_path)
        .args(["sql", "saved-delete", &space_path, "sql-1"])
        .output()
        .expect("sql saved-delete");
    assert_success(&saved_delete_output, "sql saved-delete");
    assert_eq!(
        parse_stdout_json(&saved_delete_output),
        serde_json::json!({"deleted": true})
    );

    let remote_config_path = PathBuf::from(root).join("remote-sql-config.json");
    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"[{"id":"sql-1"}]"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_saved_list_output = cli_command(&remote_config_path)
        .args(["sql", "saved-list", "remote-space"])
        .output()
        .expect("remote sql saved-list");
    assert_success(&remote_saved_list_output, "remote sql saved-list");
    let remote_saved_list_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote sql saved-list request");
    handle.join().expect("join remote sql saved-list server");
    assert!(remote_saved_list_request.starts_with("GET /spaces/remote-space/sql HTTP/1.1"));

    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"id":"sql-1"}"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_saved_get_output = cli_command(&remote_config_path)
        .args(["sql", "saved-get", "remote-space", "sql-1"])
        .output()
        .expect("remote sql saved-get");
    assert_success(&remote_saved_get_output, "remote sql saved-get");
    let remote_saved_get_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote sql saved-get request");
    handle.join().expect("join remote sql saved-get server");
    assert!(remote_saved_get_request.starts_with("GET /spaces/remote-space/sql/sql-1 HTTP/1.1"));

    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"id":"sql-1"}"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_saved_create_output = cli_command(&remote_config_path)
        .args([
            "sql",
            "saved-create",
            "remote-space",
            "sql-1",
            "--name",
            "Remote Query",
            "--sql",
            "SELECT * FROM entries",
            "--variables",
            r#"[{"name":"limit"}]"#,
            "--author",
            "remote-author",
        ])
        .output()
        .expect("remote sql saved-create");
    assert_success(&remote_saved_create_output, "remote sql saved-create");
    let remote_saved_create_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote sql saved-create request");
    handle.join().expect("join remote sql saved-create server");
    assert!(remote_saved_create_request.starts_with("POST /spaces/remote-space/sql HTTP/1.1"));
    assert!(remote_saved_create_request.contains(r#""author":"remote-author""#));

    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"id":"sql-1"}"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_saved_update_output = cli_command(&remote_config_path)
        .args([
            "sql",
            "saved-update",
            "remote-space",
            "sql-1",
            "--name",
            "Remote Query Updated",
            "--sql",
            "SELECT id FROM entries",
            "--variables",
            r#"[{"name":"limit"}]"#,
            "--parent-revision-id",
            "rev-1",
            "--author",
            "remote-author",
        ])
        .output()
        .expect("remote sql saved-update");
    assert_success(&remote_saved_update_output, "remote sql saved-update");
    let remote_saved_update_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote sql saved-update request");
    handle.join().expect("join remote sql saved-update server");
    assert!(remote_saved_update_request.starts_with("PUT /spaces/remote-space/sql/sql-1 HTTP/1.1"));
    assert!(remote_saved_update_request.contains(r#""parent_revision_id":"rev-1""#));

    let (base, requests, handle) = spawn_recording_server("HTTP/1.1 200 OK", r#"{"deleted":true}"#);
    write_endpoint_config(
        &remote_config_path,
        "backend",
        &base,
        &format!("{base}/api"),
    );
    let remote_saved_delete_output = cli_command(&remote_config_path)
        .args(["sql", "saved-delete", "remote-space", "sql-1"])
        .output()
        .expect("remote sql saved-delete");
    assert_success(&remote_saved_delete_output, "remote sql saved-delete");
    let remote_saved_delete_request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("remote sql saved-delete request");
    handle.join().expect("join remote sql saved-delete server");
    assert!(
        remote_saved_delete_request.starts_with("DELETE /spaces/remote-space/sql/sql-1 HTTP/1.1")
    );
}
