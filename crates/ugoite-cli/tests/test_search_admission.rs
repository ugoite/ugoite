//! Shared Search admission parity across CLI entry points.
//!
//! Evidence identity: surface=cli, transport=core/local. Empty, whitespace,
//! and oversize keyword queries share one classification (exit 2 with the
//! canonical code) without starting any Storage scan.

use std::process::{Command, Output};

fn ugoite_bin() -> std::path::PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_ugoite") {
        return std::path::PathBuf::from(path);
    }

    let mut path = std::env::current_exe().unwrap();
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("ugoite");
    path
}

fn run_cli(config: &std::path::Path, args: &[&str]) -> Output {
    let bin = ugoite_bin();
    if !config.exists() {
        let initialized = Command::new(&bin)
            .args(["--config", config.to_str().unwrap(), "config", "init"])
            .output()
            .expect("initialize canonical config");
        assert!(initialized.status.success(), "config init failed");
        let configured = Command::new(&bin)
            .args([
                "--config",
                config.to_str().unwrap(),
                "config",
                "connection",
                "set",
                "local",
                "--type",
                "core",
                "--root",
                config.parent().unwrap().to_str().unwrap(),
            ])
            .output()
            .expect("configure canonical connection");
        assert!(configured.status.success(), "connection set failed");
    }
    let mut canonical = vec![
        "--config".to_string(),
        config.to_string_lossy().into_owned(),
    ];
    let mut index = 0;
    while index < args.len() {
        match args[index] {
            "create-space" => canonical.extend(["space".into(), "create".into()]),
            "--root" => index += 1,
            arg if arg.contains("/spaces/") => {}
            arg => canonical.push(arg.to_string()),
        }
        index += 1;
    }
    Command::new(bin)
        .args(canonical)
        .output()
        .expect("run ugoite")
}

#[test]
fn keyword_search_rejects_invalid_queries_with_stable_classification() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_id = "search-admission-space";
    let space_path = format!("{root}/spaces/{space_id}");
    let output = run_cli(&config_path, &["create-space", "--root", &root, space_id]);
    assert!(
        output.status.success(),
        "space create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    for (query, code) in [
        ("", "SEARCH_QUERY_EMPTY"),
        ("   ", "SEARCH_QUERY_EMPTY"),
        (&"x".repeat(8 * 1024 + 1), "INVALID_INPUT"),
    ] {
        let output = run_cli(&config_path, &["search", "keyword", &space_path, query]);
        assert_eq!(
            output.status.code(),
            Some(2),
            "query {query:?} must exit with usage error"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(code),
            "query {query:?} must report {code}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
