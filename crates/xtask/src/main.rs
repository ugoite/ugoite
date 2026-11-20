use anyhow::{bail, Context, Result};
use serde::Deserialize;
#[cfg(test)]
use serde_json::json;
use serde_json::Value;
use std::{env, fs, path::Path};
#[cfg(feature = "space-tools")]
mod space;

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        #[cfg(feature = "space-tools")]
        println!("usage: cargo run -p xtask -- <openapi-generate|openapi-check|operation-registry-generate|operation-registry-check|architecture-check|space-compat-check|release-authority-check|docs-current-stack-check|seed|verify-seed>");
        #[cfg(not(feature = "space-tools"))]
        println!("usage: cargo run -p xtask --no-default-features -- <openapi-generate|openapi-check|operation-registry-generate|operation-registry-check|architecture-check|space-compat-check|release-authority-check|docs-current-stack-check>");
        return Ok(());
    };
    match command.as_str() {
        "openapi-generate" => openapi_generate(),
        "openapi-check" => openapi_check(),
        "operation-registry-generate" => operation_registry_generate(),
        "operation-registry-check" => operation_registry_check(),
        "architecture-check" => architecture_check(),
        "space-compat-check" => space_compat_check(),
        "release-authority-check" => release_authority_check(),
        "docs-current-stack-check" => docs_current_stack_check(),
        #[cfg(feature = "space-tools")]
        "seed" => space::seed(args.collect()),
        #[cfg(feature = "space-tools")]
        "verify-seed" => space::verify_seed(args.collect()),
        #[cfg(not(feature = "space-tools"))]
        "seed" | "verify-seed" => {
            bail!("xtask space commands require the default `space-tools` feature")
        }
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
        bail!("Frontend OpenAPI metadata drift detected; run `cargo run -p xtask --no-default-features -- openapi-generate`");
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
            let bodyless_post = method == "post"
                && matches!(
                    path.as_str(),
                    "/auth/logout"
                        | "/spaces/{space_id}/compositions/{entry_id}/history/{revision_id}/snapshot"
                );
            if matches!(method.as_str(), "post" | "put" | "patch")
                && !bodyless_post
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

fn contains_identifier(source: &str, identifier: &str) -> bool {
    source.match_indices(identifier).any(|(start, matched)| {
        let is_identifier_continue =
            |character: char| character.is_alphanumeric() || character == '_';
        let before_is_boundary = source[..start]
            .chars()
            .next_back()
            .is_none_or(|character| !is_identifier_continue(character));
        let after_is_boundary = source[start + matched.len()..]
            .chars()
            .next()
            .is_none_or(|character| !is_identifier_continue(character));
        before_is_boundary && after_is_boundary
    })
}

fn contains_physical_table_type(source: &str) -> bool {
    source
        .replace("CompositionComponent::Table", "")
        // These are portable Domain result-contract variants, not storage or
        // query-adapter table types.
        .replace("VariableGraphResultType::Table", "")
        .replace("VariableGraphValidatedResult::Table", "")
        .contains("Table")
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
            let contains_forbidden = if forbidden == "Operator" {
                // Match the complete Rust identifier so logical names such as
                // `SearchOperator` and `CompositionQueryOperator` do not trip
                // this physical adapter type guard.
                contains_identifier(&content, forbidden)
            } else if forbidden == "Table" {
                // `CompositionComponent::Table` is a portable Domain variant,
                // not a storage/query adapter type.
                contains_physical_table_type(&content)
            } else {
                content.contains(forbidden)
            };
            if contains_forbidden {
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
    // for read-only structured Entry validation. Core itself is
    // storage/network/runtime free (checked above), so the browser boundary
    // stays portable without a WASM-only validator.
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
        "docs/get-started",
        "docs/use",
        "docs/operate",
        "docs/reference",
        "docs/develop",
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
    let deferred_requirements = [(
        "docs/mitase/requirements/security/migrated-controls.yaml",
        ["REQ-SEC-009"].as_slice(),
    )];
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
    canonical_space_versions_in(Path::new("fixtures/spaces"))
}

/// Inventory of canonical Space version directories under `root`.
///
/// The fixture root contains canonical Space directories only: a
/// non-directory entry or a non-canonical directory name is invalid test
/// input, not an alternate layout, and fails closed instead of being
/// silently skipped, so a stray entry can never hide drift between code and
/// fixtures.
fn canonical_space_versions_in(root: &Path) -> Result<Vec<String>> {
    let entries = fs::read_dir(root).context("read canonical Space fixture inventory")?;
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
    version: Option<String>,
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
    version: Option<String>,
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
    status: String,
    #[serde(default)]
    tasks: Vec<RoadmapTask>,
}

#[derive(Debug, Deserialize)]
struct RoadmapTask {
    #[serde(default)]
    description: String,
}

/// Release authority check: mechanical tracker integrity only.
///
/// Source-of-truth rules enforced here:
/// - the prepared version is authoritative in `version.txt` alone;
/// - the prepared version must have a versioned release note under
///   `docs/version/releases/` (the release description authority);
/// - version trackers must parse, use allowed status values, carry unique
///   IDs, reference existing source files, and track the prepared stream.
///
/// This gate asserts no prose. Published identity (release tag and manifest)
/// and implementation status (the Mitase Requirement/Feature graph) live
/// outside hand-authored docs; prose literals about published versions,
/// authorities, or direction are intentionally not checked here.
fn release_authority_check() -> Result<()> {
    let mut violations = Vec::new();
    let prepared = match read_prepared_version() {
        Ok(version) => Some(version),
        Err(error) => {
            violations.push(format!("{error:#}"));
            None
        }
    };
    if let Some(prepared) = prepared.as_deref() {
        check_prepared_release_note(prepared, &mut violations);
    }
    match fs::read_to_string("docs/version/v0.2.yaml") {
        Ok(text) => match serde_yaml::from_str::<TrackerDoc>(&text) {
            Ok(v02) => {
                if let Some(prepared) = prepared.as_deref() {
                    check_version_projection(
                        "docs/version/v0.2.yaml",
                        v02.version.as_deref(),
                        prepared,
                        &mut violations,
                    );
                }
                check_tracker_milestones(
                    "docs/version/v0.2.yaml",
                    &v02.milestones,
                    &mut violations,
                );
                check_v02_product_ux_link(&v02, &mut violations);
                check_tracker_sources("docs/version/v0.2.yaml", &v02.milestones, &mut violations);
                check_product_ux_milestone_file(prepared.as_deref(), &v02, &mut violations);
            }
            Err(error) => violations.push(format!("parse v0.2 tracker: {error:#}")),
        },
        Err(error) => violations.push(format!("read v0.2 tracker: {error:#}")),
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
            Ok(v01) => {
                check_tracker_milestones(
                    "docs/version/v0.1.yaml",
                    &v01.milestones,
                    &mut violations,
                );
                check_tracker_sources("docs/version/v0.1.yaml", &v01.milestones, &mut violations);
                check_v01_tracker_doc(&v01, &mut violations);
            }
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
    if !violations.is_empty() {
        bail!("{}", violations.join("\n"));
    }
    match prepared {
        Some(prepared) => println!(
            "release authority: prepared version {prepared} has a versioned release note; v0.1/v0.2 trackers and roadmap claims are structurally sound"
        ),
        None => println!(
            "release authority: v0.1/v0.2 trackers and roadmap claims are structurally sound"
        ),
    }
    Ok(())
}

/// The prepared version is authoritative in `version.txt` alone. It must be
/// stable SemVer (`major.minor.patch`, no prerelease).
fn read_prepared_version() -> Result<String> {
    let version = fs::read_to_string("version.txt")
        .context("read prepared version")?
        .trim()
        .to_string();
    if parse_stable_semver(&version).is_none() {
        bail!("version.txt must contain stable SemVer, got {version:?}");
    }
    Ok(version)
}

fn parse_stable_semver(value: &str) -> Option<(u64, u64, u64)> {
    let parts: Vec<&str> = value.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let mut numbers = Vec::new();
    for part in parts {
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        if part.len() > 1 && part.starts_with('0') {
            return None;
        }
        numbers.push(part.parse::<u64>().ok()?);
    }
    Some((numbers[0], numbers[1], numbers[2]))
}

/// The release description for the prepared version must exist as a versioned
/// release note. Published identity itself (tag and manifest) is verified by
/// the release workflows, not by prose.
fn check_prepared_release_note(prepared: &str, violations: &mut Vec<String>) {
    let note = format!("docs/version/releases/v{prepared}.md");
    if !Path::new(&note).is_file() {
        violations.push(format!(
            "{note} must exist as the versioned release description for prepared version {prepared}"
        ));
    }
}

/// A tracker that belongs to the prepared stream must declare the stream's
/// `major.minor` version so projections cannot drift from `version.txt`.
fn check_version_projection(
    origin: &str,
    declared: Option<&str>,
    prepared: &str,
    violations: &mut Vec<String>,
) {
    let Some((major, minor, _)) = parse_stable_semver(prepared) else {
        violations.push(format!(
            "prepared version {prepared:?} is not stable SemVer; cannot check {origin} projection"
        ));
        return;
    };
    let expected = format!("{major}.{minor}");
    match declared {
        Some(declared) if declared == expected => {}
        Some(declared) => violations.push(format!(
            "{origin} declares version {declared:?}, which does not track the prepared {prepared} stream (expected {expected:?})"
        )),
        None => violations.push(format!(
            "{origin} must declare version {expected:?} to track the prepared {prepared} stream"
        )),
    }
}

fn tracker_status_allowed(status: &str) -> bool {
    matches!(status, "planned" | "in_progress" | "completed")
}

/// Structural milestone integrity: unique non-empty IDs, allowed statuses,
/// and at least one source file per milestone. Source existence is checked
/// separately by [`check_tracker_sources`].
fn check_tracker_milestones(
    origin: &str,
    milestones: &[TrackerMilestone],
    violations: &mut Vec<String>,
) {
    if milestones.is_empty() {
        violations.push(format!("{origin} must declare at least one milestone"));
    }
    let mut seen_milestones = std::collections::BTreeSet::new();
    for milestone in milestones {
        if milestone.id.trim().is_empty() {
            violations.push(format!("{origin} milestone id must not be empty"));
            continue;
        }
        if !seen_milestones.insert(milestone.id.as_str()) {
            violations.push(format!(
                "{origin} carries duplicate milestone id {}",
                milestone.id
            ));
        }
        if !tracker_status_allowed(&milestone.status) {
            violations.push(format!(
                "{origin} milestone {} has unknown status {:?}",
                milestone.id, milestone.status
            ));
        }
        if milestone.source.is_empty() {
            violations.push(format!(
                "{origin} milestone {} must list at least one source file",
                milestone.id
            ));
        }
        let mut seen_phases = std::collections::BTreeSet::new();
        for phase in &milestone.phases {
            if phase.id.trim().is_empty() {
                violations.push(format!(
                    "{origin} milestone {} carries an empty phase id",
                    milestone.id
                ));
                continue;
            }
            if !seen_phases.insert(phase.id.as_str()) {
                violations.push(format!(
                    "{origin} milestone {} carries duplicate phase id {}",
                    milestone.id, phase.id
                ));
            }
            if !tracker_status_allowed(&phase.status) {
                violations.push(format!(
                    "{origin} milestone {} phase {} has unknown status {:?}",
                    milestone.id, phase.id, phase.status
                ));
            }
        }
    }
}

/// Every milestone source reference must resolve to an existing file.
fn check_tracker_sources(
    origin: &str,
    milestones: &[TrackerMilestone],
    violations: &mut Vec<String>,
) {
    for milestone in milestones {
        for source in &milestone.source {
            if !Path::new(source).is_file() {
                violations.push(format!(
                    "{origin} milestone {} references missing source {source}",
                    milestone.id
                ));
            }
        }
    }
}

/// The v0.2 tracker must carry the product-ux milestone sourced from its
/// milestone file, and must not carry obsolete milestone authorities.
/// Milestone presence and file linkage are structural facts; no prose about
/// authorities is asserted here.
fn check_v02_product_ux_link(v02: &TrackerDoc, violations: &mut Vec<String>) {
    const ORIGIN: &str = "docs/version/v0.2.yaml";
    for stale in ["user-controlled-view", "ai-enabled-and-ai-used"] {
        let combined = format!(
            "{} {}",
            v02.milestones
                .iter()
                .map(|milestone| milestone.id.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            v02.milestones
                .iter()
                .flat_map(|milestone| milestone.source.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" ")
        );
        if combined.contains(stale) {
            violations.push(format!(
                "{ORIGIN} must not carry the obsolete {stale} authority"
            ));
        }
    }
    match v02
        .milestones
        .iter()
        .find(|milestone| milestone.id == "product-ux")
    {
        Some(milestone) => {
            if !milestone
                .source
                .iter()
                .any(|source| source == "docs/version/v0.2/product-ux.yaml")
            {
                violations.push(format!(
                    "{ORIGIN} must source the product-ux milestone from docs/version/v0.2/product-ux.yaml"
                ));
            }
        }
        None => violations.push(format!("{ORIGIN} must list the product-ux milestone")),
    }
}

fn check_product_ux_milestone_file(
    prepared: Option<&str>,
    v02: &TrackerDoc,
    violations: &mut Vec<String>,
) {
    let Some(product_ux) = v02
        .milestones
        .iter()
        .find(|milestone| milestone.id == "product-ux")
    else {
        return;
    };
    match fs::read_to_string("docs/version/v0.2/product-ux.yaml") {
        Ok(text) => match serde_yaml::from_str::<MilestoneDoc>(&text) {
            Ok(canonical) => {
                if let Some(prepared) = prepared {
                    check_version_projection(
                        "docs/version/v0.2/product-ux.yaml",
                        canonical.version.as_deref(),
                        prepared,
                        violations,
                    );
                }
                if let Some(canonical_status) = canonical.status.as_deref() {
                    if product_ux.status != canonical_status {
                        violations.push(format!(
                            "v0.2 milestone product-ux status {} disagrees with docs/version/v0.2/product-ux.yaml",
                            product_ux.status
                        ));
                    }
                }
                check_product_ux_doc_text(&text, violations);
            }
            Err(error) => violations.push(format!("parse product-ux milestone: {error:#}")),
        },
        Err(error) => violations.push(format!("read product-ux milestone: {error:#}")),
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

/// Structural roadmap integrity: unique non-empty phase IDs, allowed phase
/// statuses, non-empty task descriptions, and no obsolete milestone
/// authorities. No prose about authorities or direction is asserted here.
fn check_roadmap_doc(roadmap: &RoadmapDoc, violations: &mut Vec<String>) {
    const ORIGIN: &str = "docs/version/unknown/roadmap.yaml";
    if roadmap.phases.is_empty() {
        violations.push(format!("{ORIGIN} must declare at least one phase"));
    }
    let mut seen_phases = std::collections::BTreeSet::new();
    for phase in &roadmap.phases {
        if phase.id.trim().is_empty() {
            violations.push(format!("{ORIGIN} phase id must not be empty"));
            continue;
        }
        if !seen_phases.insert(phase.id.as_str()) {
            violations.push(format!("{ORIGIN} carries duplicate phase id {}", phase.id));
        }
        if !tracker_status_allowed(&phase.status) {
            violations.push(format!(
                "{ORIGIN} phase {} has unknown status {:?}",
                phase.id, phase.status
            ));
        }
        for task in &phase.tasks {
            if task.description.trim().is_empty() {
                violations.push(format!(
                    "{ORIGIN} phase {} carries a task without a description",
                    phase.id
                ));
            }
        }
    }
    let combined = roadmap
        .phases
        .iter()
        .flat_map(|phase| phase.tasks.iter().map(|task| task.description.as_str()))
        .collect::<Vec<_>>()
        .join("\n");
    for stale in ["User Controlled View", "AI-Enabled & AI-Used"] {
        if combined.contains(stale) {
            violations.push(format!(
                "{ORIGIN} must not carry the obsolete {stale} v0.2 authority"
            ));
        }
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

const OPERATION_REGISTRY_PATH: &str = "frontend/src/lib/generated/operation-registry.ts";

fn generated_operation_registry() -> Result<String> {
    Ok(format!(
        "// Generated by xtask operation-registry-generate. Do not edit by hand.\n\
         export const UGOITE_API_OPERATIONS = {} as const;\n\
         export type UgoiteApiOperation = (typeof UGOITE_API_OPERATIONS)[number];\n",
        serde_json::to_string_pretty(ugoite_api_client::SUPPORTED_OPERATIONS)?
    ))
}

fn operation_registry_generate() -> Result<()> {
    fs::write(OPERATION_REGISTRY_PATH, generated_operation_registry()?)
        .context("write frontend operation registry")
}

fn operation_registry_check() -> Result<()> {
    let committed =
        fs::read_to_string(OPERATION_REGISTRY_PATH).context("read frontend operation registry")?;
    if generated_operation_registry()? != normalize_newlines(&committed) {
        bail!("Frontend operation registry drift detected; run `cargo run -p xtask --no-default-features -- operation-registry-generate`");
    }
    Ok(())
}

#[cfg(test)]
mod gate_contract_tests {
    use super::*;

    #[test]
    fn openapi_validator_allows_only_declared_bodyless_posts() {
        let source = include_str!("../../ugoite-server/src/openapi.json");
        let mut spec: Value = serde_json::from_str(source).expect("OpenAPI snapshot parses");
        let snapshot_path =
            "/spaces/{space_id}/compositions/{entry_id}/history/{revision_id}/snapshot";
        assert!(spec
            .pointer(&format!(
                "/paths/{}/post/requestBody",
                snapshot_path.replace('/', "~1")
            ))
            .is_none());
        validate_openapi_contract(&spec).expect("bodyless snapshot POST is valid");

        spec.pointer_mut("/paths/~1spaces~1{space_id}~1compositions~1evaluate/post")
            .and_then(Value::as_object_mut)
            .expect("Composition evaluate POST exists")
            .remove("requestBody");
        let error = validate_openapi_contract(&spec)
            .expect_err("other POST operations still require request bodies");
        assert!(error
            .to_string()
            .contains("POST /spaces/{space_id}/compositions/evaluate missing requestBody schema"));
    }

    #[test]
    fn physical_operator_guard_matches_complete_identifiers() {
        assert!(contains_identifier("use adapter::Operator;", "Operator"));
        assert!(contains_identifier("let _: Operator = value;", "Operator"));
        assert!(!contains_identifier("SearchOperator", "Operator"));
        assert!(!contains_identifier("CompositionQueryOperator", "Operator"));
    }

    #[test]
    fn physical_table_guard_allows_domain_result_variants_only() {
        assert!(!contains_physical_table_type(
            "matches!(component, CompositionComponent::Table { .. })"
        ));
        assert!(!contains_physical_table_type(
            "matches!(returns, VariableGraphResultType::Table { .. })"
        ));
        assert!(contains_physical_table_type("use iceberg::table::Table;"));
        assert!(contains_physical_table_type("let _: Table = value;"));
    }

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
status: in_progress
summary: "Active v0.2 development tracker for the Product UX milestone."
milestones:
  - id: product-ux
    status: in_progress
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
  recovery, and documentation correctness.
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
      - description: "Milestone 5 Product UX - make the frozen v0.1 Foundation completable, discoverable, and consistent"
        done: false
      - description: "Milestone 6 Knowledge-to-tools - portable Views and bounded AI workflows (future direction)"
        done: false
"#;

    fn violations_of(check: impl FnOnce(&mut Vec<String>)) -> Vec<String> {
        let mut violations = Vec::new();
        check(&mut violations);
        violations
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
    fn fixture_layout_rejects_non_directory_and_non_canonical_entries() {
        let root = std::env::temp_dir().join(format!(
            "ugoite-xtask-fixture-layout-{}-{}",
            std::process::id(),
            "reject"
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("0.1")).expect("canonical fixture dir");
        assert_eq!(
            canonical_space_versions_in(&root).expect("canonical layout passes"),
            vec!["0.1".to_string()]
        );
        fs::write(root.join("stray.json"), "{}").expect("stray file entry");
        canonical_space_versions_in(&root).expect_err(
            "a non-directory fixture-root entry is invalid test input, not an alternate layout",
        );
        fs::remove_file(root.join("stray.json")).expect("remove stray file");
        fs::create_dir_all(root.join("draft")).expect("non-canonical dir");
        canonical_space_versions_in(&root)
            .expect_err("a non-canonical directory name must fail closed");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn regression_coverage_requires_negative_case() {
        check_space_regression_coverage("classify_space_version parse_space_version CURRENT_SPACE_VERSION SUPPORTED_SPACE_VERSIONS schema_version")
            .expect("full coverage passes");
        check_space_regression_coverage("classify_space_version parse_space_version CURRENT_SPACE_VERSION SUPPORTED_SPACE_VERSIONS")
            .expect_err("missing schema_version negative case must fail");
    }

    #[test]
    fn v02_tracker_with_product_ux_milestone_passes() {
        let doc: TrackerDoc = serde_yaml::from_str(GOOD_V02_TRACKER).expect("tracker parses");
        let violations = violations_of(|violations| {
            check_tracker_milestones("docs/version/v0.2.yaml", &doc.milestones, violations);
            check_v02_product_ux_link(&doc, violations);
        });
        assert!(
            violations.is_empty(),
            "unexpected violations: {violations:?}"
        );
    }

    #[test]
    fn v02_tracker_with_returned_view_authority_fails() {
        let text = GOOD_V02_TRACKER.replace("  - id: product-ux", "  - id: user-controlled-view");
        let doc: TrackerDoc = serde_yaml::from_str(&text).expect("tracker parses");
        let violations = violations_of(|violations| {
            check_tracker_milestones("docs/version/v0.2.yaml", &doc.milestones, violations);
            check_v02_product_ux_link(&doc, violations);
        });
        assert!(!violations.is_empty());
        assert!(violations
            .iter()
            .any(|violation| violation.contains("product-ux")));
        assert!(violations
            .iter()
            .any(|violation| violation.contains("user-controlled-view")));
    }

    #[test]
    fn v02_tracker_with_duplicate_milestone_ids_fails() {
        let text = GOOD_V02_TRACKER.replace(
            "    phases:\n      - id: design\n        status: planned",
            "    phases:\n      - id: design\n        status: planned\n  - id: product-ux\n    status: planned\n    source:\n      - docs/version/v0.2/product-ux.yaml\n    phases: []",
        );
        let doc: TrackerDoc = serde_yaml::from_str(&text).expect("tracker parses");
        let violations = violations_of(|violations| {
            check_tracker_milestones("docs/version/v0.2.yaml", &doc.milestones, violations);
        });
        assert!(violations
            .iter()
            .any(|violation| violation.contains("duplicate milestone id")));
    }

    #[test]
    fn v02_tracker_with_unknown_status_fails() {
        let text = GOOD_V02_TRACKER.replace(
            "  - id: product-ux\n    status: in_progress",
            "  - id: product-ux\n    status: shipped",
        );
        let doc: TrackerDoc = serde_yaml::from_str(&text).expect("tracker parses");
        let violations = violations_of(|violations| {
            check_tracker_milestones("docs/version/v0.2.yaml", &doc.milestones, violations);
        });
        assert!(violations
            .iter()
            .any(|violation| violation.contains("unknown status")));
    }

    #[test]
    fn tracker_source_must_resolve_to_a_file() {
        // Unit tests run with the xtask crate directory as the working
        // directory, so only the missing-file rejection is asserted here.
        // The live gate run covers the positive case from the workspace root.
        let missing = TrackerMilestone {
            id: "product-ux".to_string(),
            status: "planned".to_string(),
            source: vec!["docs/version/v0.2/does-not-exist.yaml".to_string()],
            phases: Vec::new(),
        };
        let violations = violations_of(|violations| {
            check_tracker_sources(
                "docs/version/v0.2.yaml",
                std::slice::from_ref(&missing),
                violations,
            );
        });
        assert!(violations
            .iter()
            .any(|violation| violation.contains("missing source")));
    }

    #[test]
    fn prepared_version_must_be_stable_semver() {
        assert_eq!(parse_stable_semver("0.2.1"), Some((0, 2, 1)));
        for invalid in [
            "",
            "0.2",
            "0.2.1.0",
            "v0.2.1",
            "0.2.1-next",
            "01.2.1",
            "0.2.x",
        ] {
            assert!(
                parse_stable_semver(invalid).is_none(),
                "{invalid:?} must not classify as stable SemVer"
            );
        }
    }

    #[test]
    fn prepared_release_note_must_exist() {
        let violations = violations_of(|violations| {
            check_prepared_release_note("9.9.9", violations);
        });
        assert!(violations
            .iter()
            .any(|violation| violation.contains("v9.9.9.md")));
    }

    #[test]
    fn version_projection_must_track_prepared_stream() {
        let violations = violations_of(|violations| {
            check_version_projection("docs/version/v0.2.yaml", Some("0.2"), "0.2.1", violations);
        });
        assert!(
            violations.is_empty(),
            "unexpected violations: {violations:?}"
        );
        let violations = violations_of(|violations| {
            check_version_projection("docs/version/v0.2.yaml", Some("0.1"), "0.2.1", violations);
        });
        assert!(violations
            .iter()
            .any(|violation| violation.contains("does not track")));
        let violations = violations_of(|violations| {
            check_version_projection("docs/version/v0.2.yaml", None, "0.2.1", violations);
        });
        assert!(violations
            .iter()
            .any(|violation| violation.contains("must declare version")));
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
            "Milestone 5 Product UX - make the frozen v0.1 Foundation completable, discoverable, and consistent",
            "Milestone 5 User Controlled View - portable query-driven Experiences (v0.2)",
        );
        let doc: RoadmapDoc = serde_yaml::from_str(&text).expect("roadmap parses");
        let violations = violations_of(|violations| check_roadmap_doc(&doc, violations));
        assert!(violations
            .iter()
            .any(|violation| violation.contains("User Controlled View")));
    }

    #[test]
    fn roadmap_task_without_description_fails() {
        let text = GOOD_ROADMAP.replace(
            "      - description: \"Milestone 6 Knowledge-to-tools - portable Views and bounded AI workflows (future direction)\"",
            "      - description: \"\"",
        );
        let doc: RoadmapDoc = serde_yaml::from_str(&text).expect("roadmap parses");
        let violations = violations_of(|violations| check_roadmap_doc(&doc, violations));
        assert!(violations
            .iter()
            .any(|violation| violation.contains("without a description")));
    }
}
