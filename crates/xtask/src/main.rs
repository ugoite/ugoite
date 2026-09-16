use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::{env, fs, path::Path, process::Command};

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        println!("usage: cargo run -p xtask -- <openapi-generate|openapi-check|operation-registry-check|architecture-check|space-compat-check|release-authority-check|docs-current-stack-check|supported-check|legacy-auth-check>");
        return Ok(());
    };
    match command.as_str() {
        "openapi-generate" => openapi_generate(),
        "openapi-check" => openapi_check(),
        "operation-registry-check" => operation_registry_check(),
        "architecture-check" => architecture_check(),
        "space-compat-check" => space_compat_check(),
        "release-authority-check" => release_authority_check(),
        "docs-current-stack-check" => docs_current_stack_check(),
        "supported-check" => supported_check(),
        "legacy-auth-check" => legacy_auth_check(),
        other => bail!("unknown xtask command: {other}"),
    }
}

fn openapi_generate() -> Result<()> {
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
        // ugoite-core backs the portable entry validation/compat boundary used
        // by ugoite-wasm (Lane 1). Keep it free of async/network/wasm runtimes
        // so the WASM dependency stays read-only and portable.
        "tokio",
        "reqwest",
        "axum",
        "wasm-bindgen",
        "web-sys",
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
            // `SearchOperator` is the logical typed-search product type and
            // must not trip the physical `opendal::Operator` guard.
            let haystack = if forbidden == "Operator" {
                content.replace("SearchOperator", "")
            } else {
                content.clone()
            };
            if haystack.contains(forbidden) {
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
    // Lane 1 portable entry boundary: ugoite-wasm may depend on ugoite-core
    // for read-only entry validation/compat (preview_structured_draft,
    // legacy_markdown_to_draft, draft_to_legacy_representation). Core itself
    // is storage/network/runtime free (checked above), so this keeps the
    // browser boundary portable without a WASM-only parser.
    for forbidden in [
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
        "docs/architecture/contracts/overview.md",
        "docs/architecture/contracts/stack.md",
        "docs/architecture/testing/ci-cd.md",
        "docs/architecture/testing/strategy.md",
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
            ["REQ-SEC-009"].as_slice(),
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

    println!(
        "supported contract: {} requirements traced; authentication surface is validated by Mitase",
        supported_ids.len()
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

fn space_compat_check() -> Result<()> {
    let source = fs::read_to_string("crates/ugoite-domain/src/space.rs")
        .context("read Space version authority")?;
    let mut violations = Vec::new();
    let current = match parse_current_space_version(&source) {
        Ok(version) => Some(version),
        Err(error) => {
            violations.push(format!("{error:#}"));
            None
        }
    };
    // Derive the canonical supported-generation inventory from the fixture
    // tree instead of hardcoding it here, so code and fixtures cannot drift
    // apart silently. Fail closed on any drift or malformed inventory.
    let fixture_versions = match canonical_fixture_space_versions() {
        Ok(versions) => Some(versions),
        Err(error) => {
            violations.push(format!("{error:#}"));
            None
        }
    };
    if let (Some(current), Some(fixture_versions)) = (&current, &fixture_versions) {
        if current != "0.1" {
            violations.push(format!(
                "CURRENT_SPACE_VERSION must stay \"0.1\" while Space 0.1 is the frozen compatibility identity, found \"{current}\""
            ));
        }
        if !fixture_versions.contains(current) {
            violations.push(format!(
                "CURRENT_SPACE_VERSION \"{current}\" has no canonical fixture inventory under fixtures/spaces/{current}/"
            ));
        }
        match parse_supported_space_versions(&source, current) {
            Ok(supported) => {
                if supported != *fixture_versions {
                    violations.push(format!(
                        "SUPPORTED_SPACE_VERSIONS must exactly match the canonical fixture inventory {fixture_versions:?}, found {supported:?}"
                    ));
                }
            }
            Err(error) => violations.push(format!("{error:#}")),
        }
    }
    if let Err(error) = check_classify_space_version_body(&source) {
        violations.push(format!("{error:#}"));
    }
    if let Some(fixture_versions) = &fixture_versions {
        for version in fixture_versions {
            check_canonical_space_fixture(version, &mut violations);
        }
    }
    match fs::read_to_string("crates/ugoite-domain/tests/test_space_version.rs") {
        Ok(regression) => {
            if let Err(error) = check_space_regression_coverage(&regression) {
                violations.push(format!("{error:#}"));
            }
        }
        Err(error) => violations.push(format!("read Space regression coverage: {error:#}")),
    }
    match fs::read_to_string("docs/architecture/contracts/space-compatibility.md") {
        Ok(contract) => {
            if let Err(error) = check_space_contract_doc(&contract) {
                violations.push(format!("{error:#}"));
            }
        }
        Err(error) => violations.push(format!("read Space compatibility contract: {error:#}")),
    }
    if !violations.is_empty() {
        bail!("{}", violations.join("\n"));
    }
    println!("space compatibility: Space 0.1 identity, fixture, metadata contract, and regression coverage agree");
    Ok(())
}

/// Scan `fixtures/spaces/*` for canonical Space generation directories and
/// return them sorted as the supported-generation inventory.
///
/// The directory name is the version identity, so it must match the same
/// canonical `<major>.<generation>` shape enforced by
/// `parse_space_version` in `crates/ugoite-domain/src/space.rs`
/// (ASCII digits on both sides, canonical round-trip). Anything else fails
/// closed instead of being silently skipped, so a stray directory can never
/// hide drift between code and fixtures.
fn canonical_fixture_space_versions() -> Result<Vec<String>> {
    let entries =
        fs::read_dir("fixtures/spaces").context("read canonical Space fixture inventory")?;
    let mut versions = Vec::new();
    for entry in entries {
        let entry = entry.context("read canonical Space fixture inventory entry")?;
        if !entry
            .file_type()
            .context("stat fixture inventory entry")?
            .is_dir()
        {
            bail!(
                "fixtures/spaces must contain only canonical version directories, found file {}",
                entry.path().display()
            );
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_canonical_space_version(&name) {
            bail!(
                "fixtures/spaces/{name} is not a canonical <major>.<generation> Space version directory"
            );
        }
        versions.push(name);
    }
    if versions.is_empty() {
        bail!("fixtures/spaces must contain at least one canonical version directory");
    }
    versions.sort();
    versions.dedup();
    Ok(versions)
}

fn is_canonical_space_version(value: &str) -> bool {
    let Some((major, generation)) = value.split_once('.') else {
        return false;
    };
    if major.is_empty()
        || generation.is_empty()
        || !major.bytes().all(|byte| byte.is_ascii_digit())
        || !generation.bytes().all(|byte| byte.is_ascii_digit())
    {
        return false;
    }
    let Ok(major_number): Result<u64, _> = major.parse() else {
        return false;
    };
    let Ok(generation_number): Result<u64, _> = generation.parse() else {
        return false;
    };
    format!("{major_number}.{generation_number}") == value
}

/// Validate one canonical version fixture: `expected.json` and every
/// `spaces/*/meta.json` bootstrap fixture must carry exactly that version,
/// and the frozen compatibility README must exist.
fn check_canonical_space_fixture(version: &str, violations: &mut Vec<String>) {
    let base = format!("fixtures/spaces/{version}");
    let expected_path = format!("{base}/expected.json");
    match fs::read_to_string(&expected_path) {
        Ok(text) => {
            if let Err(error) = check_fixture_meta_json(&text, &expected_path, version) {
                violations.push(format!("{error:#}"));
            }
        }
        Err(error) => violations.push(format!("read canonical Space fixture: {error:#}")),
    }
    match fs::read_dir(format!("{base}/spaces")) {
        Ok(entries) => {
            let mut fixture_count = 0usize;
            for entry in entries {
                let entry = match entry.context("read canonical Space fixture entry") {
                    Ok(entry) => entry,
                    Err(error) => {
                        violations.push(format!("{error:#}"));
                        continue;
                    }
                };
                let meta_path = entry.path().join("meta.json");
                if !meta_path.is_file() {
                    continue;
                }
                match fs::read_to_string(&meta_path) {
                    Ok(text) => {
                        if let Err(error) =
                            check_fixture_meta_json(&text, &meta_path.to_string_lossy(), version)
                        {
                            violations.push(format!("{error:#}"));
                        }
                    }
                    Err(error) => {
                        violations.push(format!("read {}: {error:#}", meta_path.to_string_lossy()))
                    }
                }
                fixture_count += 1;
            }
            if fixture_count == 0 {
                violations.push(format!(
                    "{base}/spaces must contain at least one meta.json bootstrap fixture"
                ));
            }
        }
        Err(error) => violations.push(format!("read canonical Space fixture: {error:#}")),
    }
    if !Path::new(&format!("{base}/README.md")).is_file() {
        violations.push(format!(
            "{base}/README.md must exist as frozen compatibility evidence"
        ));
    }
}

fn parse_current_space_version(source: &str) -> Result<String> {
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("pub const CURRENT_SPACE_VERSION") {
            return extract_rust_string_value(trimmed).context("parse CURRENT_SPACE_VERSION value");
        }
    }
    bail!("CURRENT_SPACE_VERSION is missing from crates/ugoite-domain/src/space.rs")
}

fn extract_rust_string_value(line: &str) -> Result<String> {
    let Some(start) = line.find('"') else {
        bail!("expected a quoted string value in: {line}")
    };
    let Some(end) = line.rfind('"') else {
        bail!("expected a quoted string value in: {line}")
    };
    if start == end {
        bail!("expected a quoted string value in: {line}")
    }
    Ok(line[start + 1..end].to_string())
}

fn parse_supported_space_versions(source: &str, current: &str) -> Result<Vec<String>> {
    for line in source.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("pub const SUPPORTED_SPACE_VERSIONS") {
            continue;
        }
        let Some(eq) = trimmed.find('=') else {
            bail!("expected an assignment in: {trimmed}")
        };
        let rhs = &trimmed[eq + 1..];
        let Some(list_start) = rhs.find("&[") else {
            bail!("expected a slice literal in: {trimmed}")
        };
        let Some(list_end) = rhs.rfind(']') else {
            bail!("expected a slice literal in: {trimmed}")
        };
        let mut supported = Vec::new();
        for item in rhs[list_start + 2..list_end].split(',') {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            if item == "CURRENT_SPACE_VERSION" {
                supported.push(current.to_string());
            } else if item.starts_with('"') && item.ends_with('"') && item.len() >= 2 {
                supported.push(item[1..item.len() - 1].to_string());
            } else {
                bail!("unsupported entry in SUPPORTED_SPACE_VERSIONS: {item}")
            }
        }
        return Ok(supported);
    }
    bail!("SUPPORTED_SPACE_VERSIONS is missing from crates/ugoite-domain/src/space.rs")
}

fn classify_space_version_body(source: &str) -> Result<&str> {
    let Some(start) = source.find("pub fn classify_space_version") else {
        bail!("classify_space_version is missing from crates/ugoite-domain/src/space.rs")
    };
    let rest = &source[start..];
    let end = rest
        .find("\npub fn ")
        .map(|index| start + index)
        .unwrap_or(source.len());
    Ok(&source[start..end])
}

fn check_classify_space_version_body(source: &str) -> Result<()> {
    let body = classify_space_version_body(source)?;
    if !body.contains("get(\"space_version\")") {
        bail!("classify_space_version must read the space_version metadata field");
    }
    if body.contains("schema_version") {
        bail!("classify_space_version must not consult subsystem-local schema_version; subsystem-local format fields stay allowed elsewhere but cannot define Space compatibility");
    }
    if !body.contains("SUPPORTED_SPACE_VERSIONS") {
        bail!("classify_space_version must enforce the single SUPPORTED_SPACE_VERSIONS authority");
    }
    Ok(())
}

fn check_fixture_meta_json(text: &str, origin: &str, expected_version: &str) -> Result<()> {
    let value: Value = serde_json::from_str(text).with_context(|| format!("parse {origin}"))?;
    match value.get("space_version").and_then(Value::as_str) {
        Some(version) if version == expected_version => Ok(()),
        Some(other) => {
            bail!("{origin} must carry space_version \"{expected_version}\", found \"{other}\"")
        }
        None => bail!("{origin} must carry a space_version string identity"),
    }
}

fn check_space_regression_coverage(regression: &str) -> Result<()> {
    let mut violations = Vec::new();
    for required in [
        "classify_space_version",
        "parse_space_version",
        "CURRENT_SPACE_VERSION",
        "SUPPORTED_SPACE_VERSIONS",
        "schema_version",
    ] {
        if !regression.contains(required) {
            violations.push(format!(
                "crates/ugoite-domain/tests/test_space_version.rs must cover {required} (including the schema_version-only negative case)"
            ));
        }
    }
    if !violations.is_empty() {
        bail!("{}", violations.join("\n"));
    }
    Ok(())
}

fn check_space_contract_doc(contract: &str) -> Result<()> {
    let mut violations = Vec::new();
    for required in [
        "\"space_version\": \"0.1\"",
        "UNSUPPORTED_SPACE_VERSION",
        "exactly `0.1`",
        "schema_version",
    ] {
        if !contract.contains(required) {
            violations.push(format!(
                "docs/architecture/contracts/space-compatibility.md must document {required}"
            ));
        }
    }
    if !violations.is_empty() {
        bail!("{}", violations.join("\n"));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TrackerDoc {
    status: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    milestones: Vec<TrackerMilestone>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TrackerMilestone {
    id: String,
    status: String,
    #[serde(default)]
    source: Vec<String>,
    #[serde(default)]
    phases: Vec<TrackerPhase>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TrackerPhase {
    id: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct MilestoneDoc {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    goal: Option<String>,
    #[serde(default)]
    phases: Vec<TrackerPhase>,
}

#[derive(Debug, Deserialize)]
struct RoadmapDoc {
    #[serde(default)]
    phases: Vec<RoadmapPhase>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RoadmapPhase {
    id: String,
    #[serde(default)]
    tasks: Vec<RoadmapTask>,
}

#[derive(Debug, Deserialize)]
struct RoadmapTask {
    #[serde(default)]
    description: String,
}

fn release_authority_check() -> Result<()> {
    let mut violations = Vec::new();
    let v02_text = fs::read_to_string("docs/version/v0.2.yaml").context("read v0.2 tracker")?;
    let v02: TrackerDoc = serde_yaml::from_str(&v02_text).context("parse v0.2 tracker")?;
    check_v02_tracker_doc(&v02, &mut violations);
    for milestone in &v02.milestones {
        for source in &milestone.source {
            if !Path::new(source).is_file() {
                violations.push(format!(
                    "v0.2 milestone {} references missing source {source}",
                    milestone.id
                ));
            }
        }
    }
    if let Some(product_ux) = v02
        .milestones
        .iter()
        .find(|milestone| milestone.id == "product-ux")
    {
        match fs::read_to_string("docs/version/v0.2/product-ux.yaml") {
            Ok(text) => match serde_yaml::from_str::<MilestoneDoc>(&text) {
                Ok(canonical) => {
                    if let Some(canonical_status) = canonical.status.as_deref() {
                        if product_ux.status != canonical_status {
                            violations.push(format!(
                                "v0.2 milestone product-ux status {} disagrees with docs/version/v0.2/product-ux.yaml",
                                product_ux.status
                            ));
                        }
                    }
                    check_product_ux_doc_text(&text, &mut violations);
                }
                Err(error) => violations.push(format!("parse product-ux milestone: {error:#}")),
            },
            Err(error) => violations.push(format!("read product-ux milestone: {error:#}")),
        }
    }
    check_obsolete_absent(
        Path::new("docs/version/v0.2/user-controlled-view.yaml").exists(),
        "docs/version/v0.2/user-controlled-view.yaml",
        &mut violations,
    );
    check_obsolete_absent(
        Path::new("docs/version/v0.2/ai-enabled-and-ai-used.yaml").exists(),
        "docs/version/v0.2/ai-enabled-and-ai-used.yaml",
        &mut violations,
    );
    match fs::read_to_string("docs/version/v0.1.yaml") {
        Ok(text) => match serde_yaml::from_str::<TrackerDoc>(&text) {
            Ok(v01) => check_v01_tracker_doc(&v01, &mut violations),
            Err(error) => violations.push(format!("parse v0.1 tracker: {error:#}")),
        },
        Err(error) => violations.push(format!("read v0.1 tracker: {error:#}")),
    }
    match fs::read_to_string("docs/version/unknown/roadmap.yaml") {
        Ok(text) => match serde_yaml::from_str::<RoadmapDoc>(&text) {
            Ok(roadmap) => check_roadmap_doc(&roadmap, &mut violations),
            Err(error) => violations.push(format!("parse roadmap: {error:#}")),
        },
        Err(error) => violations.push(format!("read roadmap: {error:#}")),
    }
    match fs::read_to_string("docs/architecture/release/v0.2.md") {
        Ok(page) => {
            if !page.contains("sole active v0.2 release authority") {
                violations.push(
                    "docs/architecture/release/v0.2.md must name Product UX as the sole active v0.2 release authority"
                        .to_string(),
                );
            }
            if !page.contains("North Star, not a shipped") {
                violations.push(
                    "docs/architecture/release/v0.2.md must keep Knowledge-to-tools as a North Star, not shipped acceptance"
                        .to_string(),
                );
            }
        }
        Err(error) => violations.push(format!("read v0.2 release page: {error:#}")),
    }
    match fs::read_to_string("docs/architecture/release/versioning.md") {
        Ok(page) => {
            if !page.contains("Product UX sole authority") {
                violations.push(
                    "docs/architecture/release/versioning.md must describe the v0.2 Product UX sole authority"
                        .to_string(),
                );
            }
        }
        Err(error) => violations.push(format!("read versioning page: {error:#}")),
    }
    if !violations.is_empty() {
        bail!("{}", violations.join("\n"));
    }
    println!("release authority: Product UX is the sole active v0.2 authority; v0.1 foundation trackers and roadmap claims agree");
    Ok(())
}

fn check_v02_tracker_doc(v02: &TrackerDoc, violations: &mut Vec<String>) {
    let ids: Vec<&str> = v02
        .milestones
        .iter()
        .map(|milestone| milestone.id.as_str())
        .collect();
    if ids != ["product-ux"] {
        violations.push(format!(
            "docs/version/v0.2.yaml must list product-ux as the sole active milestone, found {ids:?}"
        ));
    }
    let summary = v02.summary.as_deref().unwrap_or("");
    if !summary.contains("Sole active v0.2 authority: Product UX") {
        violations.push(
            "docs/version/v0.2.yaml summary must declare Product UX as the sole active v0.2 authority"
                .to_string(),
        );
    }
    if !summary.contains("Knowledge-to-tools remains a North Star") {
        violations.push(
            "docs/version/v0.2.yaml summary must keep Knowledge-to-tools as a North Star, not shipped acceptance"
                .to_string(),
        );
    }
    let combined = format!(
        "{summary} {} {}",
        ids.join(" "),
        v02.milestones
            .iter()
            .flat_map(|milestone| milestone.source.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    );
    for stale in ["user-controlled-view", "ai-enabled-and-ai-used"] {
        if combined.contains(stale) {
            violations.push(format!(
                "docs/version/v0.2.yaml must not carry the obsolete {stale} authority"
            ));
        }
    }
    if !v02.milestones.iter().any(|milestone| {
        milestone
            .source
            .iter()
            .any(|source| source == "docs/version/v0.2/product-ux.yaml")
    }) {
        violations.push(
            "docs/version/v0.2.yaml must source the active milestone from docs/version/v0.2/product-ux.yaml"
                .to_string(),
        );
    }
}

fn check_product_ux_doc_text(text: &str, violations: &mut Vec<String>) {
    let Ok(doc) = serde_yaml::from_str::<MilestoneDoc>(text) else {
        violations.push(
            "docs/version/v0.2/product-ux.yaml must parse as a milestone document".to_string(),
        );
        return;
    };
    let goal = doc.goal.as_deref().unwrap_or("");
    for dimension in [
        "completion",
        "discoverability",
        "cross-surface consistency",
        "validation clarity",
        "recovery",
        "documentation correctness",
    ] {
        if !goal.contains(dimension) {
            violations.push(format!(
                "docs/version/v0.2/product-ux.yaml goal must describe {dimension}"
            ));
        }
    }
    if !goal.contains("North Star") {
        violations.push(
            "docs/version/v0.2/product-ux.yaml goal must keep Knowledge-to-tools as a North Star"
                .to_string(),
        );
    }
    if doc.phases.is_empty() {
        violations.push("docs/version/v0.2/product-ux.yaml must declare phases".to_string());
    }
}

fn check_obsolete_absent(exists: bool, path: &str, violations: &mut Vec<String>) {
    if exists {
        violations.push(format!(
            "{path} is an obsolete milestone authority and must stay removed from the active tracker"
        ));
    }
}

fn check_v01_tracker_doc(v01: &TrackerDoc, violations: &mut Vec<String>) {
    for frozen in [
        "mvp",
        "full-configuration",
        "markdown-as-table",
        "user-management",
    ] {
        match v01
            .milestones
            .iter()
            .find(|milestone| milestone.id == frozen)
        {
            Some(milestone) if milestone.status == "completed" => {}
            Some(milestone) => violations.push(format!(
                "docs/version/v0.1.yaml milestone {frozen} must stay completed, found {}",
                milestone.status
            )),
            None => violations.push(format!(
                "docs/version/v0.1.yaml must keep the frozen {frozen} milestone"
            )),
        }
    }
    if !v01
        .milestones
        .iter()
        .any(|milestone| milestone.id == "release-preparation")
    {
        violations
            .push("docs/version/v0.1.yaml must keep the release-preparation milestone".to_string());
    }
}

fn check_roadmap_doc(roadmap: &RoadmapDoc, violations: &mut Vec<String>) {
    let descriptions: Vec<&str> = roadmap
        .phases
        .iter()
        .flat_map(|phase| phase.tasks.iter().map(|task| task.description.as_str()))
        .collect();
    let combined = descriptions.join("\n");
    if !(combined.contains("Product UX") && combined.contains("sole active authority")) {
        violations.push(
            "docs/version/unknown/roadmap.yaml must name Product UX as the v0.2 sole active authority"
                .to_string(),
        );
    }
    for stale in ["User Controlled View", "AI-Enabled & AI-Used"] {
        if combined.contains(stale) {
            violations.push(format!(
                "docs/version/unknown/roadmap.yaml must not carry the obsolete {stale} v0.2 authority"
            ));
        }
    }
    if !combined.contains("not v0.2 acceptance") {
        violations.push(
            "docs/version/unknown/roadmap.yaml must mark the Knowledge-to-tools North Star as future, not v0.2 acceptance"
                .to_string(),
        );
    }
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

/// Operation registry check (#2005): the portable operation manifest must
/// stay in sync across every surface that names it.
///
/// - `SUPPORTED_OPERATIONS` in `crates/ugoite-api-client/src/lib.rs` is the
///   Rust authority (also exposed through the WASM `operations` action).
/// - `UGOITE_API_OPERATIONS` in `frontend/src/lib/ugoite-client/protocol.ts`
///   is the TypeScript mirror used by `protocolFetch`.
/// - `GET /auth/config` must exist in `openapi.json` with a GET operation
///   while `auth.get_config` names it in both manifests.
/// - `OPENAPI_PATHS` in `frontend/src/lib/generated/openapi-types.ts` must
///   exactly match the snapshot paths (the same drift `openapi-check`
///   guards), and every step-up operation path must be present.
/// - The WASM adapter must delegate the protocol surface to
///   `ugoite_api_client` (no forked operation table) and `protocol.ts` must
///   reach it through the `operations` function-pointer action.
///
/// Any drift fails closed with the file and entry that diverged.
fn operation_registry_check() -> Result<()> {
    let mut violations = Vec::new();

    let api_client = fs::read_to_string("crates/ugoite-api-client/src/lib.rs")
        .context("read portable operation authority")?;
    let supported = parse_string_list_const(&api_client, "SUPPORTED_OPERATIONS", &mut violations);

    let protocol = fs::read_to_string("frontend/src/lib/ugoite-client/protocol.ts")
        .context("read frontend operation mirror")?;
    let mirrored = parse_string_list_const(&protocol, "UGOITE_API_OPERATIONS", &mut violations);

    if supported != mirrored {
        let only_supported: Vec<&str> = supported
            .iter()
            .filter(|item| !mirrored.contains(item))
            .map(String::as_str)
            .collect();
        let only_mirrored: Vec<&str> = mirrored
            .iter()
            .filter(|item| !supported.contains(item))
            .map(String::as_str)
            .collect();
        violations.push(format!(
            "SUPPORTED_OPERATIONS drifts from UGOITE_API_OPERATIONS (only in Rust: {only_supported:?}; only in TypeScript: {only_mirrored:?})"
        ));
    }

    // Step-up ceremony operations must be named on both sides of the bridge.
    for operation in [
        "auth.step_up.start",
        "auth.step_up.status",
        "auth.step_up.approve",
    ] {
        if !supported.contains(&operation.to_string()) {
            violations.push(format!(
                "SUPPORTED_OPERATIONS is missing step-up operation {operation}"
            ));
        }
        if !mirrored.contains(&operation.to_string()) {
            violations.push(format!(
                "UGOITE_API_OPERATIONS is missing step-up operation {operation}"
            ));
        }
    }

    // GET /auth/config surface: the REST path, its GET operation, and the
    // portable `auth.get_config` name must all exist together.
    let openapi_text = fs::read_to_string("crates/ugoite-server/src/openapi.json")
        .context("read server OpenAPI snapshot")?;
    let openapi: Value =
        serde_json::from_str(&openapi_text).context("parse server OpenAPI snapshot")?;
    let config_get = openapi.pointer("/paths/~1auth~1config/get").is_some();
    if !config_get {
        violations.push("openapi.json must expose GET /auth/config".to_string());
    }
    if !supported.contains(&"auth.get_config".to_string()) {
        violations.push("SUPPORTED_OPERATIONS must name auth.get_config".to_string());
    }
    if !mirrored.contains(&"auth.get_config".to_string()) {
        violations.push("UGOITE_API_OPERATIONS must name auth.get_config".to_string());
    }

    // OPENAPI_PATHS mirror must match the snapshot path inventory exactly.
    let generated = fs::read_to_string("frontend/src/lib/generated/openapi-types.ts")
        .context("read frontend OpenAPI metadata")?;
    let generated_paths = parse_string_list_const(&generated, "OPENAPI_PATHS", &mut violations);
    let snapshot_paths: Vec<String> = openapi
        .get("paths")
        .and_then(Value::as_object)
        .map(|paths| {
            let mut names: Vec<String> = paths.keys().cloned().collect();
            names.sort();
            names
        })
        .unwrap_or_default();
    let mut generated_sorted = generated_paths.clone();
    generated_sorted.sort();
    if generated_sorted != snapshot_paths {
        violations.push(
            "OPENAPI_PATHS drifts from crates/ugoite-server/src/openapi.json paths; run `cargo run -p xtask -- openapi-generate`"
                .to_string(),
        );
    }
    for path in [
        "/auth/step-up/start",
        "/auth/step-up/status",
        "/auth/step-up/approve",
    ] {
        if !generated_sorted.contains(&path.to_string()) {
            violations.push(format!("OPENAPI_PATHS is missing step-up path {path}"));
        }
    }

    // WASM function-pointer table: the adapter must serve the `operations`
    // action from the shared Rust authority instead of a forked table, and
    // TypeScript must query that same entry point.
    let wasm = fs::read_to_string("crates/ugoite-wasm/src/lib.rs").context("read WASM adapter")?;
    if !wasm.contains("ugoite_api_client::invoke_json") {
        violations.push(
            "crates/ugoite-wasm/src/lib.rs must delegate the protocol surface to ugoite_api_client::invoke_json"
                .to_string(),
        );
    }
    // A forked table defines its own operation list; qualified references
    // to the shared authority (`ugoite_api_client::SUPPORTED_OPERATIONS`)
    // are the in-sync path, not drift.
    let wasm_without_shared_refs = wasm
        .replace("ugoite_api_client::SUPPORTED_OPERATIONS", "")
        .replace("api_client::SUPPORTED_OPERATIONS", "");
    if wasm_without_shared_refs.contains("SUPPORTED_OPERATIONS") {
        violations.push(
            "crates/ugoite-wasm/src/lib.rs must not fork SUPPORTED_OPERATIONS; serve the shared ugoite_api_client authority"
                .to_string(),
        );
    }
    if !protocol.contains(r#"action: "operations""#)
        && !protocol.contains(r#"{ action: "operations" }"#)
    {
        violations.push(
            "frontend/src/lib/ugoite-client/protocol.ts must query the WASM operations function-pointer action"
                .to_string(),
        );
    }

    if !violations.is_empty() {
        bail!("{}", violations.join("\n"));
    }
    println!(
        "operation registry: {} operations, {} paths, and the WASM function table agree",
        supported.len(),
        snapshot_paths.len()
    );
    Ok(())
}

/// Extracts the ordered string literals of a `CONST: &[&str] = &[...]` or
/// `CONST = [...] as const` list. Pushes a violation and returns what was
/// found so the caller can still report drift precisely.
fn parse_string_list_const(source: &str, name: &str, violations: &mut Vec<String>) -> Vec<String> {
    let Some(start) = source.find(name) else {
        violations.push(format!("{name} is missing from its registry file"));
        return Vec::new();
    };
    let rest = &source[start..];
    // Start the list at the assignment value, skipping a Rust `&[&str]`
    // type ascription: find `=` first, then the `[` that opens the literal.
    // TypeScript mirrors (`= [...] as const`) work the same way.
    let Some(open) = rest
        .find('=')
        .and_then(|eq| rest[eq..].find('[').map(|bracket| eq + bracket))
    else {
        violations.push(format!("{name} has no list literal"));
        return Vec::new();
    };
    let Some(close) = rest[open..].find(']').map(|index| open + index) else {
        violations.push(format!("{name} has an unterminated list literal"));
        return Vec::new();
    };
    let mut items = Vec::new();
    for chunk in rest[open + 1..close].split(',') {
        let chunk = chunk.trim();
        if chunk.is_empty() {
            continue;
        }
        if chunk.starts_with('"') && chunk.ends_with('"') && chunk.len() >= 2 {
            items.push(chunk[1..chunk.len() - 1].to_string());
        } else {
            violations.push(format!("{name} contains a non-string entry: {chunk}"));
        }
    }
    if items.is_empty() {
        violations.push(format!("{name} must name at least one operation"));
    }
    items
}

#[cfg(test)]
mod gate_contract_tests {
    use super::*;

    const GOOD_SPACE_SOURCE: &str = r#"
pub const CURRENT_SPACE_VERSION: &str = "0.1";
pub const SUPPORTED_SPACE_VERSIONS: &[&str] = &[CURRENT_SPACE_VERSION];

/// Classify the durable Space compatibility identity.
///
/// It deliberately ignores `schema_version`: subsystem-local format fields
/// cannot be promoted to the portable Space compatibility contract.
pub fn classify_space_version(metadata: &serde_json::Value) -> Result<SpaceVersion, SpaceVersionError> {
    let detected = match metadata.get("space_version") {
        Some(serde_json::Value::String(value)) => value.clone(),
        _ => return Err(SpaceVersionError::Missing),
    };
    if !SUPPORTED_SPACE_VERSIONS.contains(&detected.as_str()) {
        return Err(SpaceVersionError::Unsupported { detected });
    }
    Ok(parsed)
}

pub const SUBSYSTEM_FORMAT_VERSION: u32 = 3;

pub fn read_subsystem_schema_version(metadata: &serde_json::Value) -> Option<u64> {
    metadata.get("schema_version").and_then(serde_json::Value::as_u64)
}
"#;

    const GOOD_V02_TRACKER: &str = r#"
version: "0.2"
status: planned
summary: >
  Sole active v0.2 authority: Product UX. Knowledge-to-tools remains a North Star,
  not a shipped v0.2 acceptance claim.
milestones:
  - id: product-ux
    status: planned
    source:
      - docs/version/v0.2/product-ux.yaml
    phases:
      - id: design
        status: planned
"#;

    const GOOD_PRODUCT_UX: &str = r#"
id: product-ux
version: "0.2"
status: planned
title: "Product UX"
goal: >
  completion, discoverability, cross-surface consistency, validation clarity,
  recovery, and documentation correctness. Knowledge-to-tools remains a North Star.
phases:
  - id: design
    status: planned
"#;

    const GOOD_V01_TRACKER: &str = r#"
version: "0.1"
status: in_progress
summary: "Foundation"
milestones:
  - id: mvp
    status: completed
    source: [docs/version/v0.1/mvp.yaml]
    phases: []
  - id: full-configuration
    status: completed
    source: [docs/version/v0.1/full-configuration.yaml]
    phases: []
  - id: markdown-as-table
    status: completed
    source: [docs/version/v0.1/markdown-as-table.yaml]
    phases: []
  - id: user-management
    status: completed
    source: [docs/version/v0.1/user-management.yaml]
    phases: []
  - id: release-preparation
    status: in_progress
    source: [docs/version/v0.1/release-preparation.yaml]
    phases: []
"#;

    const GOOD_ROADMAP: &str = r#"
id: roadmap
version: "unknown"
status: in_progress
phases:
  - id: implementation
    status: in_progress
    tasks:
      - description: "Milestone 5 Product UX - the v0.2 sole active authority"
        done: false
      - description: "Milestone 6 Knowledge-to-tools North Star (future, not v0.2 acceptance)"
        done: false
"#;

    fn violations_of(check: impl FnOnce(&mut Vec<String>)) -> Vec<String> {
        let mut violations = Vec::new();
        check(&mut violations);
        violations
    }

    #[test]
    fn operation_list_const_parses_ordered_strings() {
        let mut violations = Vec::new();
        let items = parse_string_list_const(
            "pub const SUPPORTED_OPERATIONS: &[&str] = &[\"a.b\", \"c.d\",];",
            "SUPPORTED_OPERATIONS",
            &mut violations,
        );
        assert!(violations.is_empty());
        assert_eq!(items, vec!["a.b".to_string(), "c.d".to_string()]);
    }

    #[test]
    fn operation_list_const_rejects_drift_entries() {
        let mut violations = Vec::new();
        parse_string_list_const(
            "export const UGOITE_API_OPERATIONS = [\"a.b\", 42,] as const;",
            "UGOITE_API_OPERATIONS",
            &mut violations,
        );
        assert!(!violations.is_empty());
    }

    #[test]
    fn space_source_with_frozen_identity_passes() {
        let current = parse_current_space_version(GOOD_SPACE_SOURCE).expect("current parses");
        assert_eq!(current, "0.1");
        let supported =
            parse_supported_space_versions(GOOD_SPACE_SOURCE, &current).expect("supported parses");
        assert_eq!(supported, vec!["0.1".to_string()]);
        check_classify_space_version_body(GOOD_SPACE_SOURCE).expect("classify body is scoped");
    }

    #[test]
    fn subsystem_local_schema_version_stays_allowed_outside_classifier() {
        // The gate scopes its schema_version assertion to classify_space_version;
        // subsystem-local format fields elsewhere must not trip the gate.
        assert!(GOOD_SPACE_SOURCE.contains("schema_version"));
        check_classify_space_version_body(GOOD_SPACE_SOURCE).expect("no ban on local fields");
    }

    #[test]
    fn changed_current_version_fails() {
        let source = GOOD_SPACE_SOURCE.replace(
            "pub const CURRENT_SPACE_VERSION: &str = \"0.1\";",
            "pub const CURRENT_SPACE_VERSION: &str = \"0.2\";",
        );
        let current = parse_current_space_version(&source).expect("current parses");
        assert_eq!(current, "0.2");
        let supported =
            parse_supported_space_versions(&source, &current).expect("supported parses");
        assert_ne!(supported, vec!["0.1".to_string()]);
    }

    #[test]
    fn widened_supported_set_fails() {
        let source = GOOD_SPACE_SOURCE.replace(
            "pub const SUPPORTED_SPACE_VERSIONS: &[&str] = &[CURRENT_SPACE_VERSION];",
            "pub const SUPPORTED_SPACE_VERSIONS: &[&str] = &[CURRENT_SPACE_VERSION, \"0.2\"];",
        );
        let supported = parse_supported_space_versions(&source, "0.1").expect("supported parses");
        assert_eq!(supported, vec!["0.1".to_string(), "0.2".to_string()]);
    }

    #[test]
    fn classifier_consulting_schema_version_fails() {
        let source = GOOD_SPACE_SOURCE.replace(
            "let detected = match metadata.get(\"space_version\") {",
            "let detected = match metadata.get(\"space_version\").or(metadata.get(\"schema_version\")) {",
        );
        check_classify_space_version_body(&source).expect_err("schema_version fallback must fail");
    }

    #[test]
    fn fixture_meta_requires_space_version_identity() {
        check_fixture_meta_json(r#"{"space_version": "0.1"}"#, "meta.json", "0.1")
            .expect("0.1 passes");
        check_fixture_meta_json(r#"{"schema_version": 3}"#, "meta.json", "0.1")
            .expect_err("schema_version-only metadata must fail");
        check_fixture_meta_json(r#"{"space_version": "0.2"}"#, "meta.json", "0.1")
            .expect_err("future version must fail");
        check_fixture_meta_json(r#"{}"#, "meta.json", "0.1")
            .expect_err("missing identity must fail");
        check_fixture_meta_json(r#"{"space_version": "0.2"}"#, "meta.json", "0.2")
            .expect("per-generation fixture matches its own version directory");
    }

    #[test]
    fn canonical_fixture_versions_derive_from_version_directories() {
        assert!(is_canonical_space_version("0.1"));
        assert!(is_canonical_space_version("10.20"));
        for invalid in ["", "0", "0.1.0", "v0.1", "01.1", "0.01", "0.x", ".1", "0."] {
            assert!(
                !is_canonical_space_version(invalid),
                "{invalid:?} must not classify as a canonical Space version"
            );
        }
    }

    #[test]
    fn regression_coverage_requires_negative_case() {
        check_space_regression_coverage("classify_space_version parse_space_version CURRENT_SPACE_VERSION SUPPORTED_SPACE_VERSIONS schema_version")
            .expect("full coverage passes");
        check_space_regression_coverage("classify_space_version parse_space_version CURRENT_SPACE_VERSION SUPPORTED_SPACE_VERSIONS")
            .expect_err("missing schema_version negative case must fail");
    }

    #[test]
    fn v02_tracker_with_sole_product_ux_passes() {
        let doc: TrackerDoc = serde_yaml::from_str(GOOD_V02_TRACKER).expect("tracker parses");
        let violations = violations_of(|violations| check_v02_tracker_doc(&doc, violations));
        assert!(
            violations.is_empty(),
            "unexpected violations: {violations:?}"
        );
    }

    #[test]
    fn v02_tracker_with_returned_view_authority_fails() {
        let text = GOOD_V02_TRACKER.replace("  - id: product-ux", "  - id: user-controlled-view");
        let doc: TrackerDoc = serde_yaml::from_str(&text).expect("tracker parses");
        let violations = violations_of(|violations| check_v02_tracker_doc(&doc, violations));
        assert!(!violations.is_empty());
        assert!(violations
            .iter()
            .any(|violation| violation.contains("sole active milestone")));
    }

    #[test]
    fn v02_tracker_without_authority_declaration_fails() {
        let text = GOOD_V02_TRACKER.replace(
            "Sole active v0.2 authority: Product UX.",
            "An earlier draft authority statement sat here.",
        );
        let doc: TrackerDoc = serde_yaml::from_str(&text).expect("tracker parses");
        let violations = violations_of(|violations| check_v02_tracker_doc(&doc, violations));
        assert!(violations
            .iter()
            .any(|violation| violation.contains("sole active v0.2 authority")));
    }

    #[test]
    fn product_ux_goal_requires_all_dimensions() {
        let violations =
            violations_of(|violations| check_product_ux_doc_text(GOOD_PRODUCT_UX, violations));
        assert!(
            violations.is_empty(),
            "unexpected violations: {violations:?}"
        );
        let text = GOOD_PRODUCT_UX.replace("validation clarity,", "");
        let violations = violations_of(|violations| check_product_ux_doc_text(&text, violations));
        assert!(violations
            .iter()
            .any(|violation| violation.contains("validation clarity")));
    }

    #[test]
    fn obsolete_milestone_presence_fails() {
        let violations = violations_of(|violations| {
            check_obsolete_absent(
                true,
                "docs/version/v0.2/user-controlled-view.yaml",
                violations,
            );
        });
        assert_eq!(violations.len(), 1);
        let violations = violations_of(|violations| {
            check_obsolete_absent(
                false,
                "docs/version/v0.2/user-controlled-view.yaml",
                violations,
            );
        });
        assert!(violations.is_empty());
    }

    #[test]
    fn v01_tracker_requires_frozen_foundation() {
        let doc: TrackerDoc = serde_yaml::from_str(GOOD_V01_TRACKER).expect("tracker parses");
        let violations = violations_of(|violations| check_v01_tracker_doc(&doc, violations));
        assert!(
            violations.is_empty(),
            "unexpected violations: {violations:?}"
        );
        let text = GOOD_V01_TRACKER.replace(
            "  - id: user-management\n    status: completed",
            "  - id: user-management\n    status: in_progress",
        );
        let doc: TrackerDoc = serde_yaml::from_str(&text).expect("tracker parses");
        let violations = violations_of(|violations| check_v01_tracker_doc(&doc, violations));
        assert!(violations
            .iter()
            .any(|violation| violation.contains("user-management")));
    }

    #[test]
    fn roadmap_with_returned_view_claim_fails() {
        let doc: RoadmapDoc = serde_yaml::from_str(GOOD_ROADMAP).expect("roadmap parses");
        let violations = violations_of(|violations| check_roadmap_doc(&doc, violations));
        assert!(
            violations.is_empty(),
            "unexpected violations: {violations:?}"
        );
        let text = GOOD_ROADMAP.replace(
            "Milestone 5 Product UX - the v0.2 sole active authority",
            "Milestone 5 User Controlled View - portable query-driven Experiences (v0.2)",
        );
        let doc: RoadmapDoc = serde_yaml::from_str(&text).expect("roadmap parses");
        let violations = violations_of(|violations| check_roadmap_doc(&doc, violations));
        assert!(violations
            .iter()
            .any(|violation| violation.contains("User Controlled View")));
    }
}
