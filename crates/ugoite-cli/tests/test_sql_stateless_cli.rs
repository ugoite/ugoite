//! PR5 (Follow-up H, issue #2985) CLI stateless SQL coverage.
//!
//! The CLI is a thin projection of the stateless `SqlQuery` contract: table,
//! JSON, and NDJSON render the same page; typed nulls require an explicit
//! `--param-type`; invalid scalars fail closed; and the opaque continuation
//! never leaks into human renderings (table/NDJSON keep it hidden — JSON is
//! the machine carrier that returns it for the next request).

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
    if !config.exists() && args.first().copied() != Some("config") {
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
            .expect("configure canonical core connection");
        assert!(configured.status.success(), "connection set failed");
    }
    let mut canonical = vec![
        "--config".to_string(),
        config.to_string_lossy().into_owned(),
    ];
    canonical.extend(args.iter().map(|arg| (*arg).to_string()));
    Command::new(bin)
        .args(canonical)
        .output()
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

struct CliSqlSpace {
    _dir: tempfile::TempDir,
    config_path: std::path::PathBuf,
    relation: String,
    status_column: String,
    priority_column: String,
}

fn setup_cli_sql_space() -> CliSqlSpace {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("cli-config.toml");
    let form_name = "CliSqlForm";
    let output = run_cli(&config_path, &["space", "create", "cli-sql-space"]);
    assert!(
        output.status.success(),
        "space create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let form_file = dir.path().join("cli-sql-form.json");
    std::fs::write(
        &form_file,
        format!(
                    "{{\"name\":\"{form_name}\",\"version\":1,\"template\":\"# {form_name}\",\"fields\":{{\"Status\":{{\"type\":\"string\"}},\"Priority\":{{\"type\":\"long\"}}}}}}"
                ),
    )
    .unwrap();
    let output = run_cli(&config_path, &["form", "save", form_file.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "form establish failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    for (entry_id, status, priority) in [
        ("cli-sql-00", "open", "1"),
        ("cli-sql-01", "closed", "2"),
        ("cli-sql-02", "open", "3"),
    ] {
        let output = run_cli(
            &config_path,
            &[
                "entry",
                "create",
                "--id",
                entry_id,
                "--form",
                form_name,
                "--field",
                &format!("Status={status}"),
                "--field",
                &format!("Priority={priority}"),
            ],
        );
        assert!(
            output.status.success(),
            "entry create failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let form = stdout_json(
        &run_cli(&config_path, &["form", "get", form_name]),
        "form get for SQL relation",
    );
    let form_id = form["id"].as_str().expect("form id").replace('-', "");
    let status_id = form
        .pointer("/fields/Status/id")
        .and_then(|id| id.as_i64())
        .expect("Status field id");
    let priority_id = form
        .pointer("/fields/Priority/id")
        .and_then(|id| id.as_i64())
        .expect("Priority field id");
    CliSqlSpace {
        _dir: dir,
        config_path,
        relation: format!("form_{form_id}"),
        status_column: format!("field_{status_id}"),
        priority_column: format!("field_{priority_id}"),
    }
}

#[test]
fn cli_saved_sql_run_executes_current_or_selected_bound_revision() {
    let space = setup_cli_sql_space();
    let original_sql = "SELECT _ugoite_id FROM \"CliSqlForm\" ORDER BY _ugoite_id";
    let created = stdout_json(
        &run_cli(
            &space.config_path,
            &[
                "sql",
                "saved",
                "create",
                "--name",
                "All CLI entries",
                "--sql",
                original_sql,
            ],
        ),
        "create Saved SQL for run",
    );
    let sql_id = created["id"].as_str().expect("Saved SQL ID");
    let original_revision = created["revision_id"]
        .as_str()
        .expect("initial Saved SQL revision");

    let updated_sql = format!(
        "SELECT _ugoite_id FROM \"CliSqlForm\" WHERE {} = 'open' ORDER BY _ugoite_id",
        space.status_column
    );
    let updated = stdout_json(
        &run_cli(
            &space.config_path,
            &["sql", "saved", "update", sql_id, "--sql", &updated_sql],
        ),
        "update Saved SQL for run",
    );
    assert_ne!(updated["revision_id"].as_str(), Some(original_revision));

    let current = stdout_json(
        &run_cli(&space.config_path, &["sql", "saved", "run", sql_id]),
        "run current Saved SQL revision",
    );
    assert_eq!(current["rows"].as_array().unwrap().len(), 2);

    let historical = stdout_json(
        &run_cli(
            &space.config_path,
            &[
                "sql",
                "saved",
                "run",
                sql_id,
                "--revision-id",
                original_revision,
            ],
        ),
        "run selected Saved SQL revision",
    );
    assert_eq!(historical["rows"].as_array().unwrap().len(), 3);
}

fn base_sql(space: &CliSqlSpace) -> String {
    format!(
        "SELECT _ugoite_id FROM \"{}\" ORDER BY _ugoite_id",
        space.relation
    )
}

#[test]
fn cli_sql_export_writes_complete_ndjson_atomically() {
    let space = setup_cli_sql_space();
    let sql = base_sql(&space);
    let sql_file = space._dir.path().join("query.sql");
    std::fs::write(&sql_file, &sql).unwrap();
    let path = space._dir.path().join("export.ndjson");
    let output = run_cli(
        &space.config_path,
        &[
            "sql",
            "export",
            sql_file.to_str().unwrap(),
            "--max-rows",
            "3",
            "--page-size",
            "2",
            "--output",
            path.to_str().unwrap(),
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rows = std::fs::read_to_string(&path).unwrap();
    let rows = rows
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows.iter()
            .map(|row| row["_ugoite_id"].as_str().unwrap())
            .collect::<std::collections::HashSet<_>>()
            .len(),
        3
    );
    let summary: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["complete"], true);
    assert_eq!(summary["rows"], 3);
    assert!(summary["bytes"].as_u64().unwrap() > 0);
    assert_eq!(summary["output"], path.to_string_lossy().as_ref());
    assert!(summary["query_fingerprint"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert_eq!(summary["rows_exported"], 3);
    assert_eq!(summary["pages_fetched"], 2);

    let streamed = run_cli(
        &space.config_path,
        &["sql", "export", &sql, "--max-rows", "3", "--page-size", "2"],
    );
    assert!(
        streamed.status.success(),
        "{}",
        String::from_utf8_lossy(&streamed.stderr)
    );
    let receipt: serde_json::Value = serde_json::from_slice(&streamed.stderr).unwrap();
    assert_eq!(receipt["complete"], true);
    assert_eq!(receipt["output"], serde_json::Value::Null);
    assert_eq!(receipt["rows"], 3);
    assert_eq!(receipt["pages"], 2);
    let lines = String::from_utf8_lossy(&streamed.stdout)
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(serde_json::Value::is_object));
}

#[test]
fn cli_sql_export_max_bytes_fails_before_crossing_row_and_cleans_temp_file() {
    let space = setup_cli_sql_space();
    let sql = base_sql(&space);
    let first_row = serde_json::json!({"_ugoite_id": "cli-sql-00"});
    let first_row_bytes = serde_json::to_vec(&first_row).unwrap().len() + 1;
    let path = space._dir.path().join("over-limit.ndjson");
    let max_bytes = first_row_bytes.to_string();

    let file_output = run_cli(
        &space.config_path,
        &[
            "sql",
            "export",
            &sql,
            "--max-rows",
            "3",
            "--page-size",
            "3",
            "--max-bytes",
            &max_bytes,
            "--output",
            path.to_str().unwrap(),
        ],
    );
    assert!(!file_output.status.success());
    assert!(!path.exists(), "an incomplete temp export must not publish");
    assert!(file_output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&file_output.stderr).contains("max-bytes"));
    assert!(std::fs::read_dir(space._dir.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".ugoite-export-")
    }));

    let streamed = run_cli(
        &space.config_path,
        &[
            "sql",
            "export",
            &sql,
            "--max-rows",
            "3",
            "--page-size",
            "3",
            "--max-bytes",
            &max_bytes,
        ],
    );
    assert!(!streamed.status.success());
    let lines = String::from_utf8_lossy(&streamed.stdout)
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        lines,
        vec![first_row],
        "only complete rows fit the byte bound"
    );
    let stderr = String::from_utf8_lossy(&streamed.stderr);
    assert!(stderr.contains("max-bytes"), "{stderr}");
    assert!(stderr.contains("rows_exported"), "{stderr}");
}

#[test]
fn cli_sql_export_documents_default_byte_limit_in_help() {
    let help = Command::new(ugoite_bin())
        .args(["sql", "export", "--help"])
        .output()
        .expect("show sql export help");
    assert!(help.status.success());
    let help_text = String::from_utf8_lossy(&help.stdout);
    assert!(help_text.contains("104857600"), "{help_text}");
    assert!(help_text.contains("100 MiB"), "{help_text}");
}

#[test]
fn cli_sql_export_publishes_empty_result() {
    let space = setup_cli_sql_space();
    let sql = format!(
        "SELECT _ugoite_id FROM \"{}\" WHERE {} = $status ORDER BY _ugoite_id",
        space.relation, space.status_column
    );
    let path = space._dir.path().join("empty.ndjson");
    let output = run_cli(
        &space.config_path,
        &[
            "sql",
            "export",
            &sql,
            "--param",
            "status=null",
            "--param-type",
            "status=string",
            "--max-rows",
            "5",
            "--output",
            path.to_str().unwrap(),
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), "");
    let summary: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["rows_exported"], 0);
    assert_eq!(summary["pages_fetched"], 1);
}

#[test]
fn cli_sql_export_never_overwrites_existing_output() {
    let space = setup_cli_sql_space();
    let sql = base_sql(&space);
    let path = space._dir.path().join("already-exists.ndjson");
    std::fs::write(&path, "original\n").unwrap();
    let output = run_cli(
        &space.config_path,
        &[
            "sql",
            "export",
            &sql,
            "--max-rows",
            "5",
            "--output",
            path.to_str().unwrap(),
        ],
    );
    assert!(!output.status.success());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "original\n");
}

#[test]
fn cli_sql_export_max_rows_does_not_publish_partial_file() {
    let space = setup_cli_sql_space();
    let sql = base_sql(&space);
    let path = space._dir.path().join("incomplete.ndjson");
    std::fs::write(&path, "preserve me\n").unwrap();
    let output = run_cli(
        &space.config_path,
        &[
            "sql",
            "export",
            &sql,
            "--max-rows",
            "2",
            "--page-size",
            "2",
            "--output",
            path.to_str().unwrap(),
        ],
    );
    assert!(!output.status.success());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "preserve me\n");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("max-rows"), "{stderr}");
    assert!(stderr.contains("rows_exported"), "{stderr}");
    assert!(std::fs::read_dir(space._dir.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".ugoite-export-")
    }));

    let streamed = run_cli(
        &space.config_path,
        &["sql", "export", &sql, "--max-rows", "2", "--page-size", "2"],
    );
    assert!(!streamed.status.success());
    assert_eq!(String::from_utf8_lossy(&streamed.stdout).lines().count(), 2);
    assert!(String::from_utf8_lossy(&streamed.stderr).contains("rows_exported"));
}

/// Table, JSON, and NDJSON render the same page; the opaque continuation is
/// carried only by JSON and stays hidden in table/NDJSON output.
#[test]
fn cli_sql_table_json_ndjson_render_and_hide_continuation() {
    let space = setup_cli_sql_space();
    let sql = base_sql(&space);

    let page = stdout_json(
        &run_cli(
            &space.config_path,
            &["sql", "query", &sql, "--limit", "2", "--format", "json"],
        ),
        "JSON SQL page",
    );
    assert_eq!(page["rows"].as_array().map(Vec::len), Some(2));
    assert_eq!(page["has_more"], true);
    let token = page["next"]
        .as_str()
        .expect("JSON carries continuation")
        .to_string();

    let table = run_cli(
        &space.config_path,
        &["sql", "query", &sql, "--limit", "2", "--format", "table"],
    );
    assert!(
        table.status.success(),
        "table SQL page failed: {}",
        String::from_utf8_lossy(&table.stderr)
    );
    let table_stdout = String::from_utf8_lossy(&table.stdout).to_string();
    let table_stderr = String::from_utf8_lossy(&table.stderr).to_string();
    assert!(
        table_stdout.contains("_ugoite_id") && table_stdout.contains("cli-sql-00"),
        "table renders columns and rows: {table_stdout}"
    );
    assert!(
        !table_stdout.contains("v1.") && !table_stdout.contains(&token),
        "table must hide the opaque continuation: {table_stdout}"
    );
    assert!(
        table_stderr.contains("more results available"),
        "table points at JSON for continuation: {table_stderr}"
    );

    let ndjson = run_cli(
        &space.config_path,
        &["sql", "query", &sql, "--limit", "2", "--format", "ndjson"],
    );
    assert!(
        ndjson.status.success(),
        "ndjson SQL page failed: {}",
        String::from_utf8_lossy(&ndjson.stderr)
    );
    let ndjson_stdout = String::from_utf8_lossy(&ndjson.stdout).to_string();
    let lines: Vec<&str> = ndjson_stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(
        lines.len(),
        2,
        "ndjson prints one row per line: {ndjson_stdout}"
    );
    for line in &lines {
        assert!(
            serde_json::from_str::<serde_json::Value>(line).is_ok(),
            "ndjson line is JSON: {line}"
        );
    }
    assert!(
        !ndjson_stdout.contains("v1.") && !ndjson_stdout.contains(&token),
        "ndjson must hide the opaque continuation: {ndjson_stdout}"
    );

    // The JSON continuation still resolves the remaining row.
    let next = stdout_json(
        &run_cli(
            &space.config_path,
            &[
                "sql",
                "query",
                &sql,
                "--limit",
                "2",
                "--continuation",
                &token,
                "--format",
                "json",
            ],
        ),
        "continued JSON SQL page",
    );
    assert_eq!(next["rows"].as_array().map(Vec::len), Some(1));
    assert_eq!(next["rows"][0]["_ugoite_id"], "cli-sql-02");
}

/// Typed nulls require an explicit `--param-type` and bind; invalid scalars
/// (arrays/objects, mismatched values) fail closed.
#[test]
fn cli_sql_typed_null_and_invalid_scalar() {
    let space = setup_cli_sql_space();
    let null_sql = format!(
        "SELECT _ugoite_id FROM \"{}\" WHERE $probe IS NULL ORDER BY _ugoite_id",
        space.relation
    );

    // Untyped null is a usage error: nulls must carry a declared type.
    let untyped = run_cli(
        &space.config_path,
        &["sql", "query", &null_sql, "--param", "probe=null"],
    );
    assert!(!untyped.status.success(), "untyped null must fail");
    assert!(
        String::from_utf8_lossy(&untyped.stderr).contains("--param-type"),
        "untyped null must demand --param-type: {}",
        String::from_utf8_lossy(&untyped.stderr)
    );

    // Declared typed null binds and matches all three rows.
    let typed = stdout_json(
        &run_cli(
            &space.config_path,
            &[
                "sql",
                "query",
                &null_sql,
                "--param",
                "probe=null",
                "--param-type",
                "probe=string",
            ],
        ),
        "typed null SQL page",
    );
    assert_eq!(typed["rows"].as_array().map(Vec::len), Some(3));

    // Non-scalar parameters are rejected before execution when untyped,
    // and as a declared-type mismatch when a type is given.
    let array = run_cli(
        &space.config_path,
        &["sql", "query", &null_sql, "--param", "probe=[1,2]"],
    );
    assert!(!array.status.success(), "array parameter must fail");
    assert!(
        String::from_utf8_lossy(&array.stderr).contains("scalar"),
        "array parameter must report the scalar contract: {}",
        String::from_utf8_lossy(&array.stderr)
    );
    let typed_array = run_cli(
        &space.config_path,
        &[
            "sql",
            "query",
            &null_sql,
            "--param",
            "probe=[1,2]",
            "--param-type",
            "probe=string",
        ],
    );
    assert!(
        !typed_array.status.success(),
        "typed array parameter must fail"
    );
    assert!(
        String::from_utf8_lossy(&typed_array.stderr).contains("does not match"),
        "typed array parameter must report the type mismatch: {}",
        String::from_utf8_lossy(&typed_array.stderr)
    );

    // A value that does not match its declared type fails closed.
    let mismatched = run_cli(
        &space.config_path,
        &[
            "sql",
            "query",
            &format!(
                "SELECT _ugoite_id FROM \"{}\" WHERE \"{}\" = $status ORDER BY _ugoite_id",
                space.relation, space.status_column
            ),
            "--param",
            "status=3",
            "--param-type",
            "status=string",
        ],
    );
    assert!(!mismatched.status.success(), "mismatched scalar must fail");
}

/// Declared `long`/`double` values, inferred numbers, and typed nulls for
/// `long`/`double` bind as typed values through the CLI parameter spelling.
#[test]
fn cli_sql_declared_long_double_and_typed_nulls() {
    let space = setup_cli_sql_space();
    let priority_sql = format!(
        "SELECT _ugoite_id FROM \"{}\" WHERE \"{}\" = $priority ORDER BY _ugoite_id",
        space.relation, space.priority_column
    );

    // Declared `long` binds as typed `int64`.
    let declared_long = stdout_json(
        &run_cli(
            &space.config_path,
            &[
                "sql",
                "query",
                &priority_sql,
                "--param",
                "priority=2",
                "--param-type",
                "priority=long",
            ],
        ),
        "declared long SQL page",
    );
    assert_eq!(declared_long["rows"].as_array().map(Vec::len), Some(1));
    assert_eq!(declared_long["rows"][0]["_ugoite_id"], "cli-sql-01");

    // Inferred `long` binds the same JSON number without a declared type.
    let inferred_long = stdout_json(
        &run_cli(
            &space.config_path,
            &["sql", "query", &priority_sql, "--param", "priority=2"],
        ),
        "inferred long SQL page",
    );
    assert_eq!(inferred_long["rows"], declared_long["rows"]);

    // Declared and inferred `double` numbers bind as typed `float64`.
    let ratio_sql = format!(
        "SELECT _ugoite_id FROM \"{}\" WHERE $ratio > 1.5 ORDER BY _ugoite_id",
        space.relation
    );
    for (name, extra) in [
        ("declared", vec!["--param-type", "ratio=double"]),
        ("inferred", Vec::new()),
    ] {
        let mut args = vec!["sql", "query", ratio_sql.as_str(), "--param", "ratio=2.5"];
        args.extend(extra);
        let page = stdout_json(
            &run_cli(&space.config_path, &args),
            &format!("{name} double SQL page"),
        );
        assert_eq!(page["rows"].as_array().map(Vec::len), Some(3));
    }

    // Typed nulls bind for `long` and `double` without substitution.
    let null_sql = format!(
        "SELECT _ugoite_id FROM \"{}\" WHERE $probe IS NULL ORDER BY _ugoite_id",
        space.relation
    );
    for kind in ["long", "double"] {
        let typed = stdout_json(
            &run_cli(
                &space.config_path,
                &[
                    "sql",
                    "query",
                    &null_sql,
                    "--param",
                    "probe=null",
                    "--param-type",
                    &format!("probe={kind}"),
                ],
            ),
            &format!("typed null {kind} SQL page"),
        );
        assert_eq!(typed["rows"].as_array().map(Vec::len), Some(3));
    }
}

/// The CLI continuation is opaque (versioned, no embedded SQL) and bound to
/// its query context: reuse after a change, or tampering, fails closed.
#[test]
fn cli_sql_continuation_is_opaque_and_context_bound() {
    let space = setup_cli_sql_space();
    let sql = base_sql(&space);
    let page = stdout_json(
        &run_cli(&space.config_path, &["sql", "query", &sql, "--limit", "1"]),
        "first SQL page",
    );
    let token = page["next"].as_str().expect("continuation").to_string();
    assert!(token.starts_with("v1."), "continuation is versioned");
    assert_eq!(
        token.split('.').count(),
        3,
        "continuation is a signed token"
    );
    assert!(
        !token.contains(&space.relation) && !token.contains("SELECT"),
        "continuation is opaque: {token}"
    );

    let changed_sql = format!(
        "SELECT _ugoite_id FROM \"{}\" ORDER BY \"{}\"",
        space.relation, space.status_column
    );
    let changed = run_cli(
        &space.config_path,
        &[
            "sql",
            "query",
            &changed_sql,
            "--limit",
            "1",
            "--continuation",
            &token,
        ],
    );
    assert!(
        !changed.status.success(),
        "changed SQL must reset the continuation"
    );

    let tampered = format!("{token}x");
    let tampered_out = run_cli(
        &space.config_path,
        &[
            "sql",
            "query",
            &sql,
            "--limit",
            "1",
            "--continuation",
            &tampered,
        ],
    );
    assert!(
        !tampered_out.status.success(),
        "tampered continuation must fail"
    );

    // Same SQL text and values with only the declared parameter type changed
    // resets the continuation instead of reading the old coordinate.
    let null_sql = format!(
        "SELECT _ugoite_id FROM \"{}\" WHERE $probe IS NULL ORDER BY _ugoite_id",
        space.relation
    );
    let null_first = stdout_json(
        &run_cli(
            &space.config_path,
            &[
                "sql",
                "query",
                &null_sql,
                "--param",
                "probe=null",
                "--param-type",
                "probe=string",
                "--limit",
                "1",
            ],
        ),
        "first typed-null SQL page",
    );
    let null_token = null_first["next"]
        .as_str()
        .expect("typed-null continuation")
        .to_string();
    let changed_types = run_cli(
        &space.config_path,
        &[
            "sql",
            "query",
            &null_sql,
            "--param",
            "probe=null",
            "--param-type",
            "probe=long",
            "--limit",
            "1",
            "--continuation",
            &null_token,
        ],
    );
    assert!(
        !changed_types.status.success(),
        "changed parameter types must reset the continuation"
    );
    assert!(
        String::from_utf8_lossy(&changed_types.stderr).contains("fingerprint"),
        "changed parameter types must fail on the fingerprint: {}",
        String::from_utf8_lossy(&changed_types.stderr)
    );

    // Explicit count stays separate from paging and matches the rows.
    let count = stdout_json(
        &run_cli(&space.config_path, &["sql", "count", &sql]),
        "SQL count",
    );
    assert_eq!(count["count"], 3);
}
