//! PR-03 CLI context/credential/config safety boundary.
//!
//! Covers: named credential use, no cross-connection leak, unknown TOML key
//! rejection, malformed URL rejection, atomic round-trip, empty `--context`
//! as usage error (exit 2), and context-first local/remote resolution.
//! CLI-only: no Space semantics or version change.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use ugoite_cli::cli_config::{
    merge::{merge_loaded_configs, LoadedConfigFile},
    resolve_cli_context, resolve_context_target_with_overrides, resolve_named_profile,
    validate_credential_name, write_config_file_atomic, ConfigFile, SpaceTarget,
};
use ugoite_cli::output::{project_error, UsageError};

fn v7(id: &str) -> uuid::Uuid {
    uuid::Uuid::parse_str(id).unwrap()
}

fn effective_from_toml(text: &str) -> ugoite_cli::cli_config::merge::EffectiveConfig {
    let config = ConfigFile::parse_toml(text, "test").unwrap();
    merge_loaded_configs(
        vec![LoadedConfigFile {
            path: PathBuf::from("test.toml"),
            config,
        }],
        PathBuf::from("test.toml"),
    )
    .unwrap()
}

fn credential_store_with(names: &[(&str, &str)]) -> ugoite_cli::cli_config::CredentialStore {
    let mut store = ugoite_cli::cli_config::CredentialStore::empty();
    for (name, connection) in names {
        store.credentials.insert(
            (*name).to_string(),
            serde_json::json!({
                "connection": connection,
                "access_token": "token",
                "base_url": "https://example.com",
            }),
        );
    }
    store
}

#[test]
fn named_credential_resolves_for_matching_connection() {
    let store = credential_store_with(&[("work-cred", "work")]);
    let profile = resolve_named_profile(&store, "work", Some("work-cred"))
        .unwrap()
        .expect("named profile resolves");
    assert_eq!(profile["connection"], "work");
}

#[test]
fn credential_never_leaks_across_connections() {
    let store = credential_store_with(&[("work-cred", "work")]);
    let error = resolve_named_profile(&store, "other", Some("work-cred")).unwrap_err();
    let message = format!("{error:#}");
    assert!(
        message.contains("refusing to reuse it across connections"),
        "cross-connection reuse must fail actionably: {message}"
    );
    // Unknown profile is also an actionable error, never a silent fallback.
    let missing = resolve_named_profile(&store, "work", Some("nope")).unwrap_err();
    assert!(
        format!("{missing:#}").contains("not paired"),
        "missing profile must error actionably"
    );
    // No credential requested resolves to anonymous (None), not a global pick.
    assert!(resolve_named_profile(&store, "work", None)
        .unwrap()
        .is_none());
}

#[test]
fn connection_override_never_silently_inherits_other_credential() {
    // Hermetic credential store: point HOME at an empty dir so the
    // user-global store fallback sees no candidate.
    let fake_home = tempfile::tempdir().unwrap();
    let saved_home = std::env::var("HOME").ok();
    std::env::set_var("HOME", fake_home.path());
    let result = (|| {
        let effective = effective_from_toml(
            r#"
version = 1
current_context = "personal"
[connections.local]
type = "core"
root = "/tmp/root"
[connections.work]
type = "backend"
url = "https://work.example.com"
[contexts.personal]
connection = "local"
space_uid = "019f1111-1111-7abc-8def-111111111111"
credential = "local-cred"
[contexts.work-ctx]
connection = "work"
space_uid = "019f2222-2222-7abc-8def-222222222222"
credential = "work-cred"
"#,
        );
        // Explicit --connection to a different connection without --credential
        // must never inherit the scoped context's credential ("local-cred"):
        // "work" has exactly one referenced credential, so unique resolution
        // yields "work-cred" — deterministic, never the other connection's.
        let target = resolve_context_target_with_overrides(
            &effective,
            Some("personal"),
            Some("work"),
            None,
        )?;
        match target {
            SpaceTarget::Remote {
                connection,
                credential,
                ..
            } => {
                assert_eq!(connection.as_deref(), Some("work"));
                assert_eq!(
                    credential.as_deref(),
                    Some("work-cred"),
                    "must resolve the requested connection's credential, never inherit local-cred"
                );
            }
            other => panic!("expected remote target, got {other:?}"),
        }
        // Ambiguous credentials for the requested connection error actionably.
        let ambiguous = effective_from_toml(
            r#"
version = 1
current_context = "personal"
[connections.local]
type = "core"
root = "/tmp/root"
[connections.work]
type = "backend"
url = "https://work.example.com"
[contexts.personal]
connection = "local"
space_uid = "019f1111-1111-7abc-8def-111111111111"
credential = "local-cred"
[contexts.work-a]
connection = "work"
space_uid = "019f2222-2222-7abc-8def-222222222222"
credential = "work-cred-a"
[contexts.work-b]
connection = "work"
space_uid = "019f3333-3333-7abc-8def-333333333333"
credential = "work-cred-b"
"#,
        );
        let error =
            resolve_context_target_with_overrides(&ambiguous, Some("personal"), Some("work"), None)
                .unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains("Cannot determine a credential"),
            "expected actionable credential error, got: {message}"
        );
        assert!(
            !message.contains("local-cred"),
            "error must not leak the other connection's credential: {message}"
        );
        // Explicit --credential wins for the requested connection.
        let explicit = resolve_context_target_with_overrides(
            &effective,
            Some("personal"),
            Some("work"),
            Some("work-cred"),
        )?;
        match explicit {
            SpaceTarget::Remote {
                connection,
                credential,
                ..
            } => {
                assert_eq!(connection.as_deref(), Some("work"));
                assert_eq!(credential.as_deref(), Some("work-cred"));
            }
            other => panic!("expected remote target, got {other:?}"),
        }
        Ok::<(), anyhow::Error>(())
    })();
    match saved_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
    result.unwrap();
}

#[test]
fn unknown_toml_key_fails_closed() {
    let text = r#"
version = 1
typo_key = true
[connections.local]
type = "core"
root = "/tmp/root"
"#;
    assert!(
        ConfigFile::parse_toml(text, "test").is_err(),
        "unknown top-level key must fail"
    );
    let context_typo = r#"
version = 1
[connections.local]
type = "core"
root = "/tmp/root"
[contexts.personal]
connection = "local"
space_uid = "019f1111-1111-7abc-8def-111111111111"
bogus = "x"
"#;
    assert!(
        ConfigFile::parse_toml(context_typo, "test").is_err(),
        "unknown context key must fail"
    );
    let connection_typo = r#"
version = 1
[connections.local]
type = "core"
root = "/tmp/root"
bogus = 1
"#;
    assert!(
        ConfigFile::parse_toml(connection_typo, "test").is_err(),
        "unknown connection key must fail"
    );
}

#[test]
fn malformed_remote_urls_fail() {
    for url in [
        "https:///path",
        "https://user:pass@example.com",
        "https://example.com/path#frag",
        "https://example.com/path?query=1",
        "http://ugoite.example.com",
        "ftp://example.com",
    ] {
        let text =
            format!("version = 1\n[connections.work]\ntype = \"backend\"\nurl = \"{url}\"\n");
        assert!(
            ConfigFile::parse_toml(&text, "test").is_err(),
            "malformed URL must fail: {url}"
        );
    }
    // Loopback http stays allowed.
    let ok =
        "version = 1\n[connections.dev]\ntype = \"backend\"\nurl = \"http://localhost:8000\"\n";
    assert!(ConfigFile::parse_toml(ok, "test").is_ok());
}

#[test]
fn shared_name_validation_rejects_path_shapes() {
    for bad in [
        "", " ", "  padded", "padded  ", ".", "..", "a/b", "a\\b", "a\0b",
    ] {
        assert!(
            validate_credential_name(bad).is_err(),
            "name must fail: {bad:?}"
        );
    }
    for good in ["work", "work-cred", "w.c_1", "日本語", "a.b-c_d"] {
        assert!(
            validate_credential_name(good).is_ok(),
            "name must pass: {good:?}"
        );
    }
}

#[test]
fn config_atomic_write_round_trips_without_temp_leak() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let config = ConfigFile {
        version: 1,
        current_context: Some("personal".to_owned()),
        connections: BTreeMap::from([(
            "local".to_owned(),
            ugoite_cli::cli_config::ConnectionConfig::Core {
                root: "/tmp/root".to_owned(),
            },
        )]),
        contexts: BTreeMap::from([(
            "personal".to_owned(),
            ugoite_cli::cli_config::ContextConfig {
                connection: "local".to_owned(),
                space_uid: v7("019f1111-1111-7abc-8def-111111111111"),
                credential: None,
            },
        )]),
    };
    write_config_file_atomic(&path, &config).unwrap();
    let reparsed =
        ConfigFile::parse_toml(&std::fs::read_to_string(&path).unwrap(), "roundtrip").unwrap();
    assert_eq!(reparsed, config);
    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "temp files must not leak");
}

#[test]
fn empty_context_is_usage_error_exit_2() {
    let effective = effective_from_toml(
        r#"
version = 1
current_context = "personal"
[connections.local]
type = "core"
root = "/tmp/root"
[contexts.personal]
connection = "local"
space_uid = "019f1111-1111-7abc-8def-111111111111"
"#,
    );
    for empty in ["", "   "] {
        let error = resolve_cli_context(&effective, Some(empty)).unwrap_err();
        let usage = error
            .chain()
            .find_map(|cause| cause.downcast_ref::<UsageError>());
        assert!(usage.is_some(), "empty --context must be UsageError");
        assert_eq!(project_error(&error).exit_code(), 2);
    }
    // Invalid shape is also a usage error, never a plain "not defined".
    let bad = resolve_cli_context(&effective, Some("../x")).unwrap_err();
    assert!(bad
        .chain()
        .find_map(|cause| cause.downcast_ref::<UsageError>())
        .is_some());
}

#[test]
fn context_first_resolves_local_and_remote() {
    // Local (core) target carries no remote identity.
    let local = effective_from_toml(
        r#"
version = 1
current_context = "personal"
[connections.local]
type = "core"
root = "/tmp/root"
[contexts.personal]
connection = "local"
space_uid = "019f1111-1111-7abc-8def-111111111111"
"#,
    );
    // Core validation needs a real Space dir; use a missing dir to assert the
    // resolver reaches the local branch (error names the Space, not transport).
    let error = resolve_context_target_with_overrides(&local, None, None, None).unwrap_err();
    assert!(
        format!("{error:#}").contains("could not be found"),
        "core path must validate locally: {error:#}"
    );
    // Remote target carries explicit connection + credential identity.
    let uid = "019f1111-1111-7abc-8def-111111111111";
    let remote = effective_from_toml(&format!(
        r#"
version = 1
current_context = "work-ctx"
[connections.work]
type = "backend"
url = "https://work.example.com"
[contexts.work-ctx]
connection = "work"
space_uid = "{uid}"
credential = "work-cred"
"#
    ));
    let target = resolve_context_target_with_overrides(&remote, None, None, None).unwrap();
    match target {
        SpaceTarget::Remote {
            base,
            space_uid,
            connection,
            credential,
        } => {
            assert_eq!(base, "https://work.example.com");
            assert_eq!(space_uid, uid);
            assert_eq!(connection.as_deref(), Some("work"));
            assert_eq!(credential.as_deref(), Some("work-cred"));
        }
        other => panic!("expected remote target, got {other:?}"),
    }
}

fn ugoite_bin() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_ugoite") {
        return PathBuf::from(path);
    }
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("ugoite");
    path
}

#[test]
fn empty_context_flag_exits_2_on_cli() {
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let legacy = home.path().join("legacy-endpoints.json");
    let bin = ugoite_bin();
    // Init first so config exists; then empty --context must be usage error.
    let init = Command::new(&bin)
        .env("HOME", home.path())
        .env("UGOITE_CONFIG", "")
        .env("UGOITE_CLI_CONFIG_PATH", &legacy)
        .env("UGOITE_CONFIG_HOME", "")
        .env("XDG_CONFIG_HOME", "")
        .current_dir(work.path())
        .args(["config", "init"])
        .output()
        .unwrap();
    assert!(init.status.success());
    let output = Command::new(&bin)
        .env("HOME", home.path())
        .env("UGOITE_CONFIG", "")
        .env("UGOITE_CLI_CONFIG_PATH", &legacy)
        .env("UGOITE_CONFIG_HOME", "")
        .env("XDG_CONFIG_HOME", "")
        .current_dir(work.path())
        .args(["--context", "", "context", "current"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(2),
        "empty --context must exit 2: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
