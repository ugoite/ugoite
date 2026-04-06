//! CLI config helper coverage tests.
//! REQ-OPS-006

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use ugoite_cli::config::{
    auth_session_path, base_url, clear_auth_session, config_path, effective_format_for_stdout,
    load_auth_session, load_config, normalize_space_root, operator_for_path, parse_space_path,
    print_json, print_json_table, resolve_space_reference, save_auth_session, save_config,
    space_ws_path, validate_server_endpoint_url, AuthSession, EndpointConfig, EndpointMode, Format,
};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvState {
    cwd: PathBuf,
    cli_config_path: Option<String>,
    config_home: Option<String>,
    xdg_config_home: Option<String>,
    home: Option<String>,
}

impl EnvState {
    fn capture() -> Self {
        Self {
            cwd: std::env::current_dir().expect("current dir"),
            cli_config_path: std::env::var("UGOITE_CLI_CONFIG_PATH").ok(),
            config_home: std::env::var("UGOITE_CONFIG_HOME").ok(),
            xdg_config_home: std::env::var("XDG_CONFIG_HOME").ok(),
            home: std::env::var("HOME").ok(),
        }
    }

    fn clear_known_vars() {
        for key in [
            "UGOITE_CLI_CONFIG_PATH",
            "UGOITE_CONFIG_HOME",
            "XDG_CONFIG_HOME",
            "HOME",
        ] {
            std::env::remove_var(key);
        }
    }
}

impl Drop for EnvState {
    fn drop(&mut self) {
        Self::clear_known_vars();
        if let Some(value) = &self.cli_config_path {
            std::env::set_var("UGOITE_CLI_CONFIG_PATH", value);
        }
        if let Some(value) = &self.config_home {
            std::env::set_var("UGOITE_CONFIG_HOME", value);
        }
        if let Some(value) = &self.xdg_config_home {
            std::env::set_var("XDG_CONFIG_HOME", value);
        }
        if let Some(value) = &self.home {
            std::env::set_var("HOME", value);
        }
        std::env::set_current_dir(&self.cwd).expect("restore cwd");
    }
}

/// REQ-OPS-006: config path resolution must honor CLI/env precedence and a HOME fallback.
#[test]
fn test_cli_req_ops_006_config_path_precedence_and_home_fallback() {
    let _guard = env_lock().lock().expect("env lock");
    let _env = EnvState::capture();
    EnvState::clear_known_vars();

    let temp = tempfile::tempdir().expect("tempdir");
    let explicit = temp.path().join("explicit.json");
    let config_home = temp.path().join("config-home");
    let xdg_home = temp.path().join("xdg-home");
    let home_dir = temp.path().join("home-dir");

    std::env::set_var("UGOITE_CLI_CONFIG_PATH", &explicit);
    std::env::set_var("UGOITE_CONFIG_HOME", &config_home);
    std::env::set_var("XDG_CONFIG_HOME", &xdg_home);
    std::env::set_var("HOME", &home_dir);
    assert_eq!(config_path(), explicit);

    std::env::set_var("UGOITE_CLI_CONFIG_PATH", "   ");
    assert_eq!(
        config_path(),
        config_home.join("ugoite").join("cli-endpoints.json")
    );

    std::env::set_var("UGOITE_CONFIG_HOME", "   ");
    assert_eq!(
        config_path(),
        xdg_home.join("ugoite").join("cli-endpoints.json")
    );

    std::env::set_var("XDG_CONFIG_HOME", "   ");
    assert_eq!(
        config_path(),
        home_dir.join(".ugoite").join("cli-endpoints.json")
    );

    std::env::remove_var("HOME");
    std::env::set_current_dir(temp.path()).expect("set cwd");
    assert_eq!(
        config_path(),
        PathBuf::from(".")
            .join(".ugoite")
            .join("cli-endpoints.json")
    );
}

/// REQ-OPS-006: invalid or unreadable config files must fall back to defaults.
#[test]
fn test_cli_req_ops_006_load_config_defaults_on_invalid_or_unreadable_data() {
    let _guard = env_lock().lock().expect("env lock");
    let _env = EnvState::capture();
    EnvState::clear_known_vars();

    let temp = tempfile::tempdir().expect("tempdir");
    let invalid_path = temp.path().join("invalid.json");
    std::fs::write(&invalid_path, "{not-json").expect("write invalid config");
    std::env::set_var("UGOITE_CLI_CONFIG_PATH", &invalid_path);
    let invalid_loaded = load_config();
    assert_eq!(invalid_loaded.mode, EndpointMode::Core);
    assert_eq!(invalid_loaded.backend_url, "http://localhost:8000");
    assert_eq!(invalid_loaded.api_url, "http://localhost:3000/api");

    let unreadable_path = temp.path().join("directory-config");
    std::fs::create_dir_all(&unreadable_path).expect("create config dir");
    std::env::set_var("UGOITE_CLI_CONFIG_PATH", &unreadable_path);
    let unreadable_loaded = load_config();
    assert_eq!(unreadable_loaded.mode, EndpointMode::Core);
    assert_eq!(unreadable_loaded.backend_url, "http://localhost:8000");
    assert_eq!(unreadable_loaded.api_url, "http://localhost:3000/api");
}

/// REQ-OPS-006: saving CLI config must create parent directories and round-trip values.
#[test]
fn test_cli_req_ops_006_save_config_creates_parent_dirs_and_roundtrips() {
    let _guard = env_lock().lock().expect("env lock");
    let _env = EnvState::capture();
    EnvState::clear_known_vars();

    let temp = tempfile::tempdir().expect("tempdir");
    let nested_path = temp.path().join("nested").join("cli").join("config.json");
    std::env::set_var("UGOITE_CLI_CONFIG_PATH", &nested_path);

    let config = EndpointConfig {
        mode: EndpointMode::Api,
        backend_url: "http://backend.example.test".to_string(),
        api_url: "http://frontend.example.test/api".to_string(),
    };
    let saved_path = save_config(&config).expect("save config");
    assert_eq!(saved_path, nested_path);
    assert!(saved_path.exists(), "config file should be written");

    let loaded = load_config();
    assert_eq!(loaded.mode, EndpointMode::Api);
    assert_eq!(loaded.backend_url, "http://backend.example.test");
    assert_eq!(loaded.api_url, "http://frontend.example.test/api");

    std::env::set_var("UGOITE_CLI_CONFIG_PATH", "/");
    let save_err = save_config(&config).expect_err("root path should not be writable as config");
    assert!(save_err.to_string().contains("Is a directory"));

    let blocking_parent = temp.path().join("not-a-directory");
    std::fs::write(&blocking_parent, "blocker").expect("write blocking file");
    std::env::set_var(
        "UGOITE_CLI_CONFIG_PATH",
        blocking_parent.join("config.json").display().to_string(),
    );
    let parent_err = save_config(&config).expect_err("file parent should block config creation");
    let parent_err_text = parent_err.to_string().to_lowercase();
    assert!(
        parent_err_text.contains("directory") || parent_err_text.contains("exists"),
        "unexpected parent creation error: {parent_err_text}"
    );
}

/// REQ-OPS-015: auth session helpers must create parent dirs, ignore unreadable session files, and fail loudly on invalid clears.
#[test]
fn test_cli_req_ops_015_auth_session_helpers_cover_unreadable_and_error_paths() {
    let _guard = env_lock().lock().expect("env lock");
    let _env = EnvState::capture();
    EnvState::clear_known_vars();

    let temp = tempfile::tempdir().expect("tempdir");
    let nested_config_path = temp.path().join("nested").join("cli").join("config.json");
    std::env::set_var("UGOITE_CLI_CONFIG_PATH", &nested_config_path);

    let session = AuthSession {
        bearer_token: Some("issued-token".to_string()),
    };
    let session_path = auth_session_path();
    let saved_path = save_auth_session(&session).expect("save auth session");
    assert_eq!(saved_path, session_path);
    assert!(saved_path.exists(), "auth session should be written");
    assert_eq!(
        load_auth_session().bearer_token.as_deref(),
        Some("issued-token")
    );

    assert!(clear_auth_session().expect("clear saved auth session"));
    assert!(!session_path.exists(), "auth session should be removed");
    assert!(
        !clear_auth_session().expect("missing auth session should report false"),
        "missing auth session should not report a deletion"
    );

    let blocking_parent = temp.path().join("not-a-directory");
    std::fs::write(&blocking_parent, "blocker").expect("write blocking parent file");
    std::env::set_var(
        "UGOITE_CLI_CONFIG_PATH",
        blocking_parent.join("config.json").display().to_string(),
    );
    let save_err =
        save_auth_session(&session).expect_err("file parent should block auth-session creation");
    let save_err_text = save_err.to_string().to_lowercase();
    assert!(
        save_err_text.contains("directory") || save_err_text.contains("exists"),
        "unexpected save-auth-session error: {save_err_text}"
    );

    std::env::set_var("UGOITE_CLI_CONFIG_PATH", &nested_config_path);
    std::fs::create_dir_all(&session_path).expect("create unreadable auth-session directory");
    assert_eq!(load_auth_session(), AuthSession::default());

    let clear_err =
        clear_auth_session().expect_err("directory auth-session path should fail to clear");
    let clear_err_text = clear_err.to_string().to_lowercase();
    assert!(
        clear_err_text.contains("directory") || clear_err_text.contains("is a directory"),
        "unexpected clear-auth-session error: {clear_err_text}"
    );
}

/// REQ-OPS-006: local file roots must stay portable while unsupported remote URIs fail explicitly.
#[test]
fn test_cli_req_ops_006_operator_for_path_supports_file_and_rejects_remote_uris() {
    let temp = tempfile::tempdir().expect("tempdir");
    operator_for_path("").expect("empty path should resolve to the filesystem root");
    operator_for_path("file://").expect("file:// should resolve to the filesystem root");
    let file_uri = format!("file://{}", temp.path().display());
    operator_for_path(&file_uri).expect("file:// root should be supported");
    operator_for_path(&format!("{}/", temp.path().display()))
        .expect("local filesystem root should be supported");

    let err = operator_for_path("s3://bucket/demo").expect_err("s3 uri should fail");
    assert!(err
        .to_string()
        .contains("unsupported storage uri in core mode: s3://bucket/demo"));

    let nul_err = operator_for_path("file://\0").expect_err("nul path should fail");
    assert!(nul_err.to_string().contains("null byte"));
}

/// REQ-OPS-006: space path parsing must support both workspace paths and bare IDs.
#[test]
fn test_cli_req_ops_006_parse_space_path_variants() {
    let (root, space_id) = parse_space_path("/tmp/demo/spaces/my-space/assets/logo.png");
    assert_eq!(root, "/tmp/demo");
    assert_eq!(space_id, "my-space");

    let core = EndpointConfig {
        mode: EndpointMode::Core,
        backend_url: "http://backend.example.test".to_string(),
        api_url: "http://frontend.example.test/api".to_string(),
    };
    let (resolved_root, resolved_space_id) =
        resolve_space_reference(&core, "/tmp/demo/spaces/my-space", "entry list")
            .expect("full core path should resolve");
    assert_eq!(resolved_root, "/tmp/demo");
    assert_eq!(resolved_space_id, "my-space");

    let (root, space_id) = parse_space_path("backend-space");
    assert_eq!(root, "");
    assert_eq!(space_id, "backend-space");

    let err =
        resolve_space_reference(&core, "backend-space", "entry list").expect_err("bare core ID");
    assert!(err.to_string().contains(
        "entry list requires SPACE_ID_OR_PATH as /path/to/root/spaces/<id> in core mode"
    ));

    let malformed = resolve_space_reference(&core, "/tmp/demo/spaces//nested", "entry list")
        .expect_err("malformed core path");
    assert!(malformed.to_string().contains(
        "entry list requires SPACE_ID_OR_PATH as /path/to/root/spaces/<id> in core mode"
    ));

    assert_eq!(normalize_space_root("/"), "/");
    assert_eq!(normalize_space_root("/spaces"), "/");
    assert_eq!(normalize_space_root("/tmp/demo/spaces"), "/tmp/demo");
    assert_eq!(normalize_space_root("/tmp/demo"), "/tmp/demo");
}

/// REQ-OPS-006: endpoint helpers must stay covered for path, URL, and JSON output handling.
#[test]
fn test_cli_req_ops_006_endpoint_helpers_cover_base_url_and_space_path() {
    struct BrokenSerialize;

    impl serde::Serialize for BrokenSerialize {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            Err(serde::ser::Error::custom("broken serializer"))
        }
    }

    assert_eq!(
        space_ws_path("/unused/root", "demo-space"),
        "spaces/demo-space"
    );

    let core = EndpointConfig {
        mode: EndpointMode::Core,
        backend_url: "http://backend.example.test/".to_string(),
        api_url: "http://frontend.example.test/api/".to_string(),
    };
    assert_eq!(base_url(&core), None);

    let backend = EndpointConfig {
        mode: EndpointMode::Backend,
        backend_url: "http://backend.example.test/".to_string(),
        api_url: "http://frontend.example.test/api/".to_string(),
    };
    assert_eq!(
        base_url(&backend),
        Some("http://backend.example.test".to_string())
    );

    let api = EndpointConfig {
        mode: EndpointMode::Api,
        backend_url: "http://backend.example.test/".to_string(),
        api_url: "http://frontend.example.test/api/".to_string(),
    };
    assert_eq!(
        base_url(&api),
        Some("http://frontend.example.test/api".to_string())
    );

    let malformed =
        validate_server_endpoint_url("not a url", "Backend endpoint").expect_err("bad URL");
    assert!(malformed
        .to_string()
        .contains("Backend endpoint URL \"not a url\" is invalid"));

    let unsupported_scheme =
        validate_server_endpoint_url("ftp://backend.example.test", "Backend endpoint")
            .expect_err("unsupported scheme should fail");
    assert!(unsupported_scheme.to_string().contains(
        "Backend endpoint URL ftp://backend.example.test must use http:// or https://, not ftp://."
    ));

    assert_eq!(effective_format_for_stdout(None, false), Format::Json);
    assert_eq!(effective_format_for_stdout(None, true), Format::Table);
    assert_eq!(
        effective_format_for_stdout(Some(Format::Plain), true),
        Format::Plain
    );

    print_json(&serde_json::json!({"ok": true}));
    print_json(&BrokenSerialize);
    print_json_table(
        &[serde_json::json!({
            "name": serde_json::Value::Null,
            "count": 1,
        })],
        &[("NAME", "name"), ("COUNT", "count")],
    );
}
