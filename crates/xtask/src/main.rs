use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::{env, fs, path::Path, process::Command};

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        println!("usage: cargo run -p xtask -- <openapi-generate|openapi-check|architecture-check|docs-current-stack-check|supported-check|legacy-auth-check>");
        return Ok(());
    };
    match command.as_str() {
        "openapi-generate" => openapi_generate(),
        "openapi-check" => openapi_check(),
        "architecture-check" => architecture_check(),
        "docs-current-stack-check" => docs_current_stack_check(),
        "supported-check" => supported_check(),
        "legacy-auth-check" => legacy_auth_check(),
        other => bail!("unknown xtask command: {other}"),
    }
}

fn openapi_generate() -> Result<()> {
    let snapshot = fs::read_to_string("crates/ugoite-server/src/openapi.json")
        .context("read server OpenAPI snapshot")?;
    fs::write("docs/spec/api/openapi.yaml", snapshot).context("write docs OpenAPI snapshot")?;
    let generated = generated_openapi_types(&openapi_value()?)?;
    fs::create_dir_all("frontend/src/lib/generated").context("create frontend generated dir")?;
    fs::write("frontend/src/lib/generated/openapi-types.ts", generated)
        .context("write frontend OpenAPI metadata")?;
    Ok(())
}

fn openapi_check() -> Result<()> {
    let server = fs::read_to_string("crates/ugoite-server/src/openapi.json")
        .context("read server OpenAPI snapshot")?;
    let spec: Value = serde_json::from_str(&server).context("parse server OpenAPI snapshot")?;
    validate_openapi_contract(&spec)?;
    let docs =
        fs::read_to_string("docs/spec/api/openapi.yaml").context("read docs OpenAPI snapshot")?;
    if normalize_newlines(&server) != normalize_newlines(&docs) {
        bail!("OpenAPI drift detected; run `cargo run -p xtask -- openapi-generate`");
    }
    let generated = generated_openapi_types(&spec)?;
    let committed = fs::read_to_string("frontend/src/lib/generated/openapi-types.ts")
        .context("read frontend OpenAPI metadata")?;
    if normalize_newlines(&generated) != normalize_newlines(&committed) {
        bail!("Frontend OpenAPI metadata drift detected; run `cargo run -p xtask -- openapi-generate`");
    }
    Ok(())
}

fn openapi_value() -> Result<Value> {
    let snapshot = fs::read_to_string("crates/ugoite-server/src/openapi.json")
        .context("read server OpenAPI snapshot")?;
    serde_json::from_str(&snapshot).context("parse server OpenAPI snapshot")
}

fn validate_openapi_contract(spec: &Value) -> Result<()> {
    let Some(paths) = spec.get("paths").and_then(Value::as_object) else {
        bail!("OpenAPI snapshot missing paths object");
    };
    let mut violations = Vec::new();
    for (path, methods) in paths {
        let Some(methods) = methods.as_object() else {
            violations.push(format!("{path} must be an object"));
            continue;
        };
        for (method, operation) in methods {
            let operation_name = format!("{} {}", method.to_uppercase(), path);
            if matches!(method.as_str(), "post" | "put" | "patch")
                && path != "/auth/logout"
                && operation.get("requestBody").is_none()
            {
                violations.push(format!("{operation_name} missing requestBody schema"));
            }
            let has_success_schema = operation
                .get("responses")
                .and_then(Value::as_object)
                .map(|responses| {
                    responses.iter().any(|(status, response)| {
                        (status == "204")
                            || (status.starts_with('2')
                                && response
                                    .get("content")
                                    .and_then(Value::as_object)
                                    .is_some_and(|content| {
                                        content.values().any(|media| media.get("schema").is_some())
                                    }))
                            || (status.starts_with('3')
                                && response.pointer("/headers/Location/schema").is_some())
                    })
                })
                .unwrap_or(false);
            if !has_success_schema {
                violations.push(format!("{operation_name} missing success response schema"));
            }
            let has_error_schema = operation
                .get("responses")
                .and_then(Value::as_object)
                .map(|responses| {
                    responses.iter().any(|(status, response)| {
                        matches!(
                            status.as_str(),
                            "400" | "401" | "403" | "404" | "409" | "410" | "422" | "500"
                        ) && response
                            .pointer("/content/application~1json/schema")
                            .is_some()
                    })
                })
                .unwrap_or(false);
            if !has_error_schema {
                violations.push(format!("{operation_name} missing error response schema"));
            }
        }
    }
    if !violations.is_empty() {
        bail!("{}", violations.join("\n"));
    }
    Ok(())
}

fn generated_openapi_types(spec: &Value) -> Result<String> {
    let mut schemas: Vec<String> = spec
        .pointer("/components/schemas")
        .and_then(Value::as_object)
        .context("OpenAPI snapshot missing components.schemas")?
        .keys()
        .cloned()
        .collect();
    schemas.sort();
    let mut paths: Vec<String> = spec
        .get("paths")
        .and_then(Value::as_object)
        .context("OpenAPI snapshot missing paths")?
        .keys()
        .cloned()
        .collect();
    paths.sort();
    Ok(format!(
        "// Generated by xtask openapi-generate. Do not edit by hand.\nexport const OPENAPI_SCHEMA_NAMES = {} as const;\nexport type OpenApiSchemaName = typeof OPENAPI_SCHEMA_NAMES[number];\n\nexport const OPENAPI_PATHS = {} as const;\nexport type OpenApiPath = typeof OPENAPI_PATHS[number];\n",
        serde_json::to_string_pretty(&schemas)?,
        serde_json::to_string_pretty(&paths)?,
    ))
}

fn architecture_check() -> Result<()> {
    let mut violations = Vec::new();
    let server_manifest = fs::read_to_string("crates/ugoite-server/Cargo.toml")
        .context("read ugoite-server Cargo.toml")?;
    if server_manifest
        .lines()
        .any(|line| line.trim_start().starts_with("opendal"))
    {
        violations.push("ugoite-server must not depend on OpenDAL directly".to_string());
    }

    let core_manifest = fs::read_to_string("crates/ugoite-core/Cargo.toml")
        .context("read ugoite-core Cargo.toml")?;
    for forbidden in [
        "opendal",
        "iceberg",
        "arrow-",
        "parquet",
        "datafusion",
        "sqlparser",
    ] {
        if core_manifest
            .lines()
            .any(|line| line.trim_start().starts_with(forbidden))
        {
            violations.push(format!(
                "ugoite-core must not depend on physical adapter crate {forbidden}"
            ));
        }
    }
    for path in collect_files(Path::new("crates/ugoite-core/src"))? {
        let path_text = path.to_string_lossy();
        let content = fs::read_to_string(&path).with_context(|| format!("read {path_text}"))?;
        for forbidden in [
            "opendal::",
            "iceberg::",
            "arrow_",
            "parquet::",
            "datafusion::",
            "sqlparser::",
            "Operator",
            "Table",
            "RecordBatch",
            "Transaction",
            "SessionContext",
        ] {
            if content.contains(forbidden) {
                violations.push(format!(
                    "{path_text} leaks physical adapter type or dependency {forbidden}"
                ));
            }
        }
    }

    for path in collect_files(Path::new("frontend/src"))? {
        let path_text = path.to_string_lossy();
        if path_text.contains("/lib/ugoite-client/")
            || path_text.ends_with(".test.ts")
            || path_text.ends_with(".test.tsx")
            || path_text.ends_with(".wasm")
        {
            continue;
        }
        let content = fs::read_to_string(&path).with_context(|| format!("read {path_text}"))?;
        for raw_module in [
            "~/lib/entry-api",
            "~/lib/space-api",
            "~/lib/form-api",
            "~/lib/asset-api",
            "~/lib/sql-api",
            "~/lib/sql-session-api",
            "./entry-api",
            "./space-api",
        ] {
            if content.contains(raw_module) {
                violations.push(format!(
                    "{path_text} imports raw API module {raw_module}; use ~/lib/ugoite-client"
                ));
            }
        }
    }

    for path in collect_files(Path::new("crates/ugoite-cli/src/commands"))? {
        let path_text = path.to_string_lossy();
        if path_text.ends_with("form.rs") || path_text.ends_with("space.rs") {
            continue;
        }
        let content = fs::read_to_string(&path).with_context(|| format!("read {path_text}"))?;
        for raw_call in [
            "ugoite_iceberg::entry::update_entry",
            "ugoite_iceberg::entry::delete_entry",
            "ugoite_iceberg::entry::get_entry_history",
            "ugoite_iceberg::entry::get_entry_revision",
            "ugoite_iceberg::entry::restore_entry",
            "ugoite_iceberg::index::execute_sql_query",
            "ugoite_iceberg::index::get_space_stats",
            "ugoite_iceberg::index::reindex_all",
            "ugoite_iceberg::saved_sql::create_sql",
            "ugoite_iceberg::saved_sql::delete_sql",
            "ugoite_iceberg::saved_sql::get_sql",
            "ugoite_iceberg::saved_sql::list_sql",
            "ugoite_iceberg::saved_sql::update_sql",
        ] {
            if content.contains(raw_call) {
                violations.push(format!(
                    "{path_text} calls {raw_call} directly; use UgoiteService for stateful CLI operations"
                ));
            }
        }
    }

    let api_client_manifest = fs::read_to_string("crates/ugoite-api-client/Cargo.toml")
        .context("read ugoite-api-client Cargo.toml")?;
    for forbidden in [
        "reqwest",
        "tokio",
        "wasm-bindgen",
        "web-sys",
        "axum",
        "ugoite-core",
        "ugoite-domain",
        "ugoite-storage",
        "ugoite-iceberg",
        "iceberg",
        "arrow-array",
        "arrow-schema",
        "parquet",
        "opendal",
    ] {
        if api_client_manifest
            .lines()
            .any(|line| line.trim_start().starts_with(forbidden))
        {
            violations.push(format!(
                "ugoite-api-client must stay transport-neutral and must not depend on {forbidden}"
            ));
        }
    }

    for path in collect_files(Path::new("crates/ugoite-cli/src/commands"))? {
        let path_text = path.to_string_lossy();
        let content = fs::read_to_string(&path).with_context(|| format!("read {path_text}"))?;
        for forbidden in [
            "http::http_get",
            "http::http_post",
            "http::http_put",
            "http::http_patch",
            "http::http_delete",
            "format!(\"{base}/",
        ] {
            if content.contains(forbidden) {
                violations.push(format!(
                    "{path_text} constructs remote HTTP directly via {forbidden}; use http::execute with a portable operation name"
                ));
            }
        }
    }

    let wasm_manifest = fs::read_to_string("crates/ugoite-wasm/Cargo.toml")
        .context("read ugoite-wasm Cargo.toml")?;
    for forbidden in [
        "ugoite-core",
        "ugoite-storage",
        "ugoite-iceberg",
        "iceberg",
        "arrow-array",
        "arrow-schema",
        "parquet",
        "opendal",
        "tokio",
        "reqwest",
    ] {
        if wasm_manifest
            .lines()
            .any(|line| line.trim_start().starts_with(forbidden))
        {
            violations.push(format!("ugoite-wasm must not depend on {forbidden}"));
        }
    }

    for path in collect_files(Path::new("frontend/src/lib"))? {
        let path_text = path.to_string_lossy();
        if !path_text.ends_with("-api.ts") {
            continue;
        }
        let content = fs::read_to_string(&path).with_context(|| format!("read {path_text}"))?;
        if content.contains("apiFetch") {
            violations.push(format!(
                "{path_text} uses apiFetch directly; use ugoite-client/protocol so endpoint semantics stay in Rust/WASM"
            ));
        }
    }

    let domain_manifest = fs::read_to_string("crates/ugoite-domain/Cargo.toml")
        .context("read ugoite-domain Cargo.toml")?;
    let domain_dependencies = domain_manifest
        .split("[dev-dependencies]")
        .next()
        .unwrap_or(&domain_manifest);
    for forbidden in [
        "tokio",
        "opendal",
        "axum",
        "iceberg",
        "arrow-array",
        "arrow-schema",
        "parquet",
        "datafusion",
    ] {
        if domain_dependencies
            .lines()
            .any(|line| line.trim_start().starts_with(forbidden))
        {
            violations.push(format!("ugoite-domain must not depend on {forbidden}"));
        }
    }

    if !violations.is_empty() {
        bail!("{}", violations.join("\n"));
    }
    Ok(())
}

fn docs_current_stack_check() -> Result<()> {
    let mut violations = Vec::new();
    for root in [
        "README.md",
        "docs/spec/index.md",
        "docs/spec/architecture/overview.md",
        "docs/spec/architecture/stack.md",
        "docs/spec/testing/ci-cd.md",
        "docs/spec/testing/strategy.md",
        "docs/guide",
        "docsite/src/pages/app",
    ] {
        let path = Path::new(root);
        let files = if path.is_file() {
            vec![path.to_path_buf()]
        } else {
            collect_files(path)?
        };
        for file in files {
            let extension = file
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            if !matches!(extension, "md" | "yaml" | "yml" | "astro" | "ts") {
                continue;
            }
            let text = fs::read_to_string(&file)
                .with_context(|| format!("read {}", file.to_string_lossy()))?;
            let lower = text.to_ascii_lowercase();
            for forbidden in ["fastapi", "python backend", "pyo3", "bun/uv"] {
                if lower.contains(forbidden) && !lower.contains("historical") {
                    violations.push(format!(
                        "{} mentions {forbidden} without marking it historical/planned",
                        file.to_string_lossy()
                    ));
                }
            }
        }
    }
    let deferred_requirements = [
        (
            "docs/spec/requirements/security.yaml",
            ["REQ-SEC-009", "REQ-SEC-012", "REQ-SEC-013", "REQ-SEC-014"].as_slice(),
        ),
        (
            "docs/spec/requirements/ops.yaml",
            ["REQ-OPS-015"].as_slice(),
        ),
    ];
    for (path, ids) in deferred_requirements {
        let text = fs::read_to_string(path).with_context(|| format!("read {path}"))?;
        for block in text.split("- set_id:").skip(1) {
            let Some(id) = ids.iter().find(|id| block.contains(&format!("id: {id}"))) else {
                continue;
            };
            if block
                .lines()
                .any(|line| line.trim() == "status: implemented")
            {
                violations.push(format!(
                    "{path} marks future capability {id} as implemented"
                ));
            }
        }
    }
    let openapi = fs::read_to_string("crates/ugoite-server/src/openapi.json")
        .context("read OpenAPI release boundary")?;
    if !openapi.contains("v0.1 supports mandatory browser authentication with Passkey/WebAuthn") {
        violations.push(
            "server OpenAPI must carry the v0.1 supported authentication boundary".to_string(),
        );
    }
    if !violations.is_empty() {
        bail!("{}", violations.join("\n"));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct RequirementCatalog {
    requirements: Vec<RequirementRecord>,
}

#[derive(Debug, Deserialize)]
struct RequirementRecord {
    id: String,
    status: String,
    verification: String,
    #[serde(default)]
    tests: Vec<TestReference>,
}

#[derive(Debug, Deserialize)]
struct TestReference {
    file: String,
    #[serde(default)]
    cases: Vec<String>,
    #[serde(default)]
    tests: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AuthFeatureCatalog {
    apis: Vec<AuthApiRecord>,
}

#[derive(Debug, Deserialize)]
struct AuthApiRecord {
    id: String,
    status: String,
    backend: AuthBackendRecord,
}

#[derive(Debug, Deserialize)]
struct AuthBackendRecord {
    file: String,
}

#[derive(Debug, Deserialize)]
struct UserManagementRelease {
    status: String,
    requirement_ids: Vec<String>,
    #[serde(default)]
    future_requirement_ids: Vec<String>,
    phases: Vec<ReleasePhase>,
}

#[derive(Debug, Deserialize)]
struct ReleasePhase {
    id: String,
    status: String,
}

fn supported_check() -> Result<()> {
    let security_text = fs::read_to_string("docs/spec/requirements/security.yaml")
        .context("read security requirements")?;
    let security: RequirementCatalog =
        serde_yaml::from_str(&security_text).context("parse security requirements")?;
    let requirements = security
        .requirements
        .iter()
        .map(|requirement| (requirement.id.as_str(), requirement))
        .collect::<std::collections::BTreeMap<_, _>>();

    let release_text = fs::read_to_string("docs/version/v0.1/user-management.yaml")
        .context("read v0.1 user-management tracker")?;
    let release: UserManagementRelease =
        serde_yaml::from_str(&release_text).context("parse v0.1 user-management tracker")?;
    if release.status != "completed" {
        bail!("v0.1 user-management tracker must be completed");
    }
    for phase in &release.phases {
        if matches!(phase.id.as_str(), "implementation" | "testing") && phase.status != "completed"
        {
            bail!("v0.1 user-management phase {} must be completed", phase.id);
        }
    }
    let supported_ids = release
        .requirement_ids
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    let future_ids = release
        .future_requirement_ids
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    if supported_ids.intersection(&future_ids).next().is_some() {
        bail!("v0.1 supported and future requirement lists overlap");
    }

    for id in &supported_ids {
        let requirement = requirements
            .get(id)
            .with_context(|| format!("v0.1 requirement {id} is missing from security.yaml"))?;
        if requirement.status != "implemented" {
            bail!("supported requirement {id} must have status: implemented");
        }
        if requirement.verification != "traced" {
            bail!("supported requirement {id} must have verification: traced");
        }
        let mut concrete_case_count = 0usize;
        for test in &requirement.tests {
            let path = Path::new(&test.file);
            if !path.is_file() {
                bail!(
                    "supported requirement {id} references missing test file {}",
                    test.file
                );
            }
            let source = fs::read_to_string(path)
                .with_context(|| format!("read requirement test reference {}", test.file))?;
            for case in test.cases.iter().chain(test.tests.iter()) {
                concrete_case_count += 1;
                if !source.contains(case) {
                    bail!(
                        "supported requirement {id} references missing test case {case} in {}",
                        test.file
                    );
                }
            }
        }
        if requirement.tests.is_empty() || concrete_case_count == 0 {
            bail!("supported requirement {id} must reference a concrete test case");
        }
    }

    let auth_text = fs::read_to_string("docs/spec/features/auth.yaml")
        .context("read authentication feature registry")?;
    let auth: AuthFeatureCatalog =
        serde_yaml::from_str(&auth_text).context("parse authentication feature registry")?;
    let allowed_statuses = ["supported", "internal", "experimental", "future"];
    let mut feature_ids = std::collections::BTreeSet::new();
    for api in &auth.apis {
        if !feature_ids.insert(api.id.as_str()) {
            bail!("authentication feature {} is duplicated", api.id);
        }
        if !allowed_statuses.contains(&api.status.as_str()) {
            bail!(
                "authentication feature {} has invalid status {}",
                api.id,
                api.status
            );
        }
        if !Path::new(&api.backend.file).is_file() {
            bail!(
                "authentication feature {} references missing backend {}",
                api.id,
                api.backend.file
            );
        }
    }
    if auth.apis.is_empty() {
        bail!("authentication feature registry is empty");
    }

    println!(
        "supported contract: {} requirements traced, {} authentication APIs classified",
        supported_ids.len(),
        auth.apis.len()
    );
    Ok(())
}

fn legacy_auth_check() -> Result<()> {
    let patterns = [
        ["/auth/mock", "oauth"].join("-"),
        ["mock", "oauth"].join("_"),
        ["UGOITE_DEV", "AUTH_MODE"].join("_"),
        ["UGOITE_DEV", "USER_ID"].join("_"),
        ["UGOITE_DEV", "PASSKEY_CONTEXT"].join("_"),
        ["UGOITE", "BOOTSTRAP_TOKEN"].join("_"),
        ["UGOITE_AUTH", "BEARER"].join("_"),
        ["UGOITE_AUTH", "API_KEY"].join("_"),
        ["ugoite_auth", "bearer_token"].join("_"),
        ["cli", "auth.json"].join("-"),
        ["passkey", "totp"].join("-"),
    ];
    let output = Command::new("git")
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .output()
        .context("list tracked files for legacy authentication check")?;
    if !output.status.success() {
        bail!("git ls-files failed during legacy authentication check");
    }
    let mut violations = Vec::new();
    for raw_path in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let path = Path::new(std::str::from_utf8(raw_path).context("tracked path is not UTF-8")?);
        let Ok(bytes) = fs::read(path) else { continue };
        let text = String::from_utf8_lossy(&bytes);
        for pattern in &patterns {
            if text.contains(pattern) {
                violations.push(format!(
                    "{} contains removed authentication name",
                    path.display()
                ));
            }
        }
    }
    violations.sort();
    violations.dedup();
    if !violations.is_empty() {
        bail!("{}", violations.join("\n"));
    }
    Ok(())
}

fn collect_files(root: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    if !root.exists() {
        return Ok(files);
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        if path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| matches!(name, "node_modules" | "target" | ".output" | "dist"))
        {
            continue;
        }
        if path.is_dir() {
            for entry in fs::read_dir(&path)? {
                stack.push(entry?.path());
            }
        } else {
            files.push(path);
        }
    }
    Ok(files)
}

fn normalize_newlines(value: &str) -> String {
    value.replace("\r\n", "\n")
}
