//! REQ-OPS-006 canonical CLI configuration coverage.
//!
//! These tests intentionally exercise the v1 TOML model. The former endpoint
//! JSON helpers were compatibility code and are not part of the v0.2 surface.

use std::path::Path;
use ugoite_cli::cli_config::{
    build_source_stack, load_cli_config, normalize_core_root_to_absolute,
    project_local_config_path, write_config_file_atomic, ConfigFile, ConnectionConfig,
};
use ugoite_cli::config::{operator_for_path, validate_server_endpoint_url};

#[test]
fn test_cli_req_ops_006_config_path_precedence_and_home_fallback() {
    let project = Path::new("/workspace/.ugoite/config.toml");
    let global = Path::new("/home/user/.ugoite/config.toml");
    let env = [Path::new("/shared/config.toml").to_path_buf()];
    assert_eq!(project_local_config_path(Path::new("/workspace")), project);
    assert_eq!(
        build_source_stack(None, project, true, &env, global, true),
        vec![project.to_path_buf(), env[0].clone()]
    );
    assert_eq!(
        build_source_stack(None, project, false, &[], global, true),
        vec![global.to_path_buf()]
    );
}

#[test]
fn test_cli_req_ops_006_load_config_fails_closed_on_invalid_or_unreadable_data() {
    let temp = tempfile::tempdir().expect("tempdir");
    let invalid = temp.path().join("invalid.toml");
    std::fs::write(&invalid, "version = [").expect("write invalid config");
    let error = match load_cli_config(Some(&invalid), temp.path()) {
        Ok(_) => panic!("invalid TOML must fail"),
        Err(error) => error,
    };
    assert!(error
        .to_string()
        .contains("invalid explicit CLI configuration"));

    let unsupported = temp.path().join("unsupported.toml");
    std::fs::write(&unsupported, "version = 2\n").expect("write unsupported config");
    let error = match load_cli_config(Some(&unsupported), temp.path()) {
        Ok(_) => panic!("unsupported config version must fail"),
        Err(error) => error,
    };
    assert!(format!("{error:#}").contains("unsupported config version"));
}

#[test]
fn test_cli_req_ops_006_save_config_creates_parent_dirs_and_roundtrips() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("nested/config.toml");
    let mut config = ConfigFile::empty();
    config.connections.insert(
        "local".to_string(),
        ConnectionConfig::Core {
            root: "/workspace".to_string(),
        },
    );
    write_config_file_atomic(&path, &config).expect("write canonical config");
    let loaded = load_cli_config(Some(&path), temp.path()).expect("load canonical config");
    assert_eq!(
        loaded
            .effective
            .connections
            .get("local")
            .map(|value| &value.value),
        Some(&ConnectionConfig::Core {
            root: "/workspace".to_string()
        })
    );
}

#[test]
fn test_cli_req_ops_006_operator_for_path_supports_file_and_rejects_remote_uris() {
    let temp = tempfile::tempdir().expect("tempdir");
    assert!(operator_for_path(temp.path().to_str().expect("temp path")).is_ok());
    let error = operator_for_path("s3://bucket/space").expect_err("remote URI rejected");
    assert!(error.to_string().contains("unsupported storage uri"));
}

#[test]
fn test_cli_req_ops_006_parse_space_path_variants() {
    assert_eq!(
        normalize_core_root_to_absolute("./knowledge/", Path::new("/workspace")),
        "/workspace/./knowledge/"
    );
    assert_eq!(
        normalize_core_root_to_absolute("/var/lib/ugoite/spaces", Path::new("/workspace")),
        "/var/lib/ugoite/spaces"
    );
}

#[test]
fn test_cli_req_ops_006_endpoint_helpers_cover_base_url_and_space_path() {
    assert!(validate_server_endpoint_url("https://example.test/api", "API").is_ok());
    assert!(validate_server_endpoint_url("http://localhost:3000", "API").is_ok());
    assert!(validate_server_endpoint_url("https:///missing-host", "API").is_err());
}
