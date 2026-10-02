use crate::cli_config::{resolve_command_target, SpaceTarget};
use crate::http;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use ugoite_api_client::{
    CompositionDiagnosticCode as ApiCompositionDiagnosticCode, CompositionEntryIntegrity,
    CompositionEntryMetadata, CompositionLintError, CompositionLintResponse, CompositionLintValue,
    CompositionRawRevision, CompositionRevisionMetadata,
};
use ugoite_domain::composition::{
    canonicalize_composition_yaml, CompositionDiagnosticCode, MAX_COMPOSITION_YAML_BYTES,
};

#[derive(Args)]
pub struct CompositionCmd {
    #[command(subcommand)]
    pub sub: CompositionSubCmd,
}

#[derive(Subcommand)]
pub enum CompositionSubCmd {
    /// Validate and canonicalize a Composition YAML file without a Space or server
    Lint {
        #[arg(value_name = "FILE")]
        file: PathBuf,
    },
    /// Inspect a saved Composition without requiring its current format version
    #[command(
        long_about = "Use the selected local Core or remote Space context.\n\nBy default, prints the raw carrier and revision metadata as JSON. Use --revision to select an exact revision; a missing revision never falls back to latest. Use --raw to write the stored spec value without canonicalizing it."
    )]
    Inspect {
        #[arg(value_name = "COMPOSITION_ID")]
        composition_id: String,
        #[arg(long, value_name = "REVISION_ID")]
        revision: Option<String>,
        #[arg(long, help = "Write the stored spec value without canonicalizing it")]
        raw: bool,
    },
}

/// Composition lint is offline; saved-document reads resolve a Space context.
pub async fn run(
    cmd: CompositionCmd,
    explicit_config: Option<&Path>,
    context_override: Option<&str>,
) -> Result<()> {
    match cmd.sub {
        CompositionSubCmd::Lint { file } => {
            let response = lint_file(&file)?;
            crate::output::print_json(&response);
        }
        CompositionSubCmd::Inspect {
            composition_id,
            revision,
            raw,
        } => {
            let target =
                resolve_command_target(explicit_config, context_override, "composition inspect")?;
            let record = read_composition(&target, &composition_id, revision.as_deref()).await?;
            if raw {
                write_raw_spec(&record)?;
            } else {
                crate::output::print_json(&record);
            }
        }
    }
    Ok(())
}

async fn read_composition(
    target: &SpaceTarget,
    composition_id: &str,
    revision_id: Option<&str>,
) -> Result<CompositionRawRevision> {
    match target {
        SpaceTarget::Core { root, space_id } => {
            let service =
                ugoite_iceberg::service::UgoiteService::new_without_background_refresh(root)?;
            let raw = match revision_id {
                Some(revision_id) => {
                    service
                        .get_composition_raw_revision_local(space_id, composition_id, revision_id)
                        .await?
                }
                None => {
                    service
                        .get_composition_raw_local(space_id, composition_id)
                        .await?
                }
            };
            Ok(local_raw_revision_to_api(raw))
        }
        SpaceTarget::Remote { space_uid, .. } => {
            let arguments = composition_get_arguments(space_uid, composition_id, revision_id);
            let result =
                http::execute_for_target(target, "composition.get", arguments, None).await?;
            serde_json::from_value(result).context("decode Composition read response")
        }
    }
}

fn composition_get_arguments(
    space_id: &str,
    composition_id: &str,
    revision_id: Option<&str>,
) -> Value {
    let mut arguments = json!({
        "space_id": space_id,
        "composition_id": composition_id,
    });
    if let Some(revision_id) = revision_id {
        arguments["revision_id"] = json!(revision_id);
    }
    arguments
}

fn local_raw_revision_to_api(
    raw: ugoite_iceberg::composition::RawCompositionRevision,
) -> CompositionRawRevision {
    let revision = raw.revision;
    let entry = revision.entry;
    CompositionRawRevision {
        revision: CompositionRevisionMetadata {
            form_id: revision.form_id.to_string(),
            entry_id: revision.entry_id.to_string(),
            revision_id: revision.revision_id.to_string(),
            parent_revision_id: revision.parent_revision_id.map(|value| value.to_string()),
            entry_version: revision.entry_version,
            change_id: revision.change_id,
            expected_version: revision.expected_version,
            operation: match revision.operation {
                ugoite_domain::entry::EntryOperation::Upsert => "upsert",
                ugoite_domain::entry::EntryOperation::Delete => "delete",
                ugoite_domain::entry::EntryOperation::Restore => "restore",
            }
            .to_string(),
            committed_at_micros: revision.committed_at_micros,
            author_id: revision.author_id,
            form_version: revision.form_version.get(),
            source_kind: revision.source_kind,
            source_id: revision.source_id,
            entry: CompositionEntryMetadata {
                external_id: entry.external_id,
                tags: entry.tags,
                created_at_micros: entry.created_at_micros,
                updated_at_micros: entry.updated_at_micros,
                updated_by: entry.updated_by,
                integrity: CompositionEntryIntegrity {
                    checksum: entry.integrity.checksum,
                    signature: entry.integrity.signature,
                },
                deleted: entry.deleted,
                deleted_at_micros: entry.deleted_at_micros,
                deleted_by: entry.deleted_by,
                restored_from: entry.restored_from.map(|value| value.to_string()),
            },
            extra_attributes: revision.extra_attributes,
            extension_metadata: revision.extension_metadata,
        },
        fields: raw.fields,
        unmapped_field_values: raw
            .unmapped_field_values
            .into_iter()
            .map(|(field_id, value)| (field_id.get().to_string(), value))
            .collect::<BTreeMap<_, _>>(),
    }
}

fn write_raw_spec(record: &CompositionRawRevision) -> Result<()> {
    match raw_spec_output(record) {
        RawSpecOutput::Text(spec) => {
            std::io::stdout()
                .write_all(spec.as_bytes())
                .context("write raw Composition spec")?;
        }
        RawSpecOutput::Json(value) => crate::output::print_json(&value),
    }
    Ok(())
}

enum RawSpecOutput {
    Text(String),
    Json(Value),
}

fn raw_spec_output(record: &CompositionRawRevision) -> RawSpecOutput {
    match record.fields.get("spec") {
        Some(Value::String(spec)) => RawSpecOutput::Text(spec.clone()),
        Some(value) => RawSpecOutput::Json(value.clone()),
        None => RawSpecOutput::Json(json!({
            "revision": record.revision,
            "fields": record.fields,
            "unmapped_field_values": record.unmapped_field_values,
        })),
    }
}

fn lint_file(path: &Path) -> Result<CompositionLintResponse> {
    let file =
        File::open(path).with_context(|| format!("open Composition file {}", path.display()))?;
    let mut bytes = Vec::with_capacity(MAX_COMPOSITION_YAML_BYTES + 1);
    file.take((MAX_COMPOSITION_YAML_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .with_context(|| format!("read Composition file {}", path.display()))?;
    Ok(lint_yaml_bytes(&bytes))
}

fn lint_yaml_bytes(bytes: &[u8]) -> CompositionLintResponse {
    if bytes.len() > MAX_COMPOSITION_YAML_BYTES {
        return diagnostic_response(CompositionDiagnosticCode::InvalidComposition);
    }
    let Ok(yaml) = std::str::from_utf8(bytes) else {
        return diagnostic_response(CompositionDiagnosticCode::InvalidComposition);
    };

    match canonicalize_composition_yaml(yaml) {
        Ok(canonical) => CompositionLintResponse {
            ok: true,
            value: Some(CompositionLintValue {
                document: serde_json::to_value(canonical.document)
                    .expect("Composition domain document is JSON serializable"),
                canonical_yaml: canonical.yaml,
                fingerprint: canonical.fingerprint,
            }),
            error: None,
        },
        Err(code) => diagnostic_response(code),
    }
}

fn diagnostic_response(code: CompositionDiagnosticCode) -> CompositionLintResponse {
    let api_code = ApiCompositionDiagnosticCode::from_code(code.as_str())
        .expect("Composition domain diagnostics are represented in the portable API");
    CompositionLintResponse {
        ok: false,
        value: None,
        error: Some(CompositionLintError {
            kind: "composition_diagnostic".to_string(),
            code: api_code,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        composition_get_arguments, lint_file, lint_yaml_bytes, local_raw_revision_to_api,
        raw_spec_output, read_composition, RawSpecOutput,
    };
    use crate::cli_config::SpaceTarget;
    use anyhow::Result;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    use std::io::Write;
    use ugoite_api_client::{prepare_request, CompositionDiagnosticCode};
    use ugoite_core::error::{AppError, ErrorCode};
    use ugoite_domain::composition::MAX_COMPOSITION_YAML_BYTES;
    use ugoite_domain::entry::{EntryMetadata, EntryOperation, EntryRevision, FieldValue};
    use ugoite_domain::form::FormVersion;
    use ugoite_domain::id::{EntryId, FieldId, FormId, RevisionId};
    use uuid::Uuid;

    const MONTHLY_EXPENSE: &str = include_str!(
        "../../../../crates/ugoite-domain/tests/fixtures/composition/monthly-expense.ugcomp.yaml"
    );
    const MONTHLY_EXPENSE_CANONICAL: &str = include_str!(
        "../../../../crates/ugoite-domain/tests/fixtures/composition/monthly-expense.canonical.ugcomp.yaml"
    );

    #[test]
    fn lint_file_returns_the_shared_canonical_document_response() {
        let mut file = tempfile::NamedTempFile::new().expect("temporary YAML file");
        file.write_all(MONTHLY_EXPENSE.as_bytes())
            .expect("write YAML fixture");

        let response = lint_file(file.path()).expect("lint succeeds");

        assert!(response.ok);
        assert!(response.error.is_none());
        let value = response.value.expect("canonical value");
        assert_eq!(value.canonical_yaml, MONTHLY_EXPENSE_CANONICAL);
        assert_eq!(value.fingerprint.len(), 64);
        assert_eq!(value.document["format_version"], 1);
        assert_eq!(value.document["name"], "Monthly expenses");
    }

    #[test]
    fn lint_returns_the_domain_diagnostic_code_for_unsupported_versions() {
        let response =
            lint_yaml_bytes(b"format_version: 22\nname: Future\nkind: dashboard\nspec: {}\n");

        assert!(!response.ok);
        assert!(response.value.is_none());
        assert_eq!(
            response.error.expect("diagnostic").code,
            CompositionDiagnosticCode::UnsupportedFormatVersion
        );
    }

    #[test]
    fn lint_rejects_oversized_and_non_utf8_files_as_invalid_composition() {
        let oversized = vec![b' '; MAX_COMPOSITION_YAML_BYTES + 1];
        let oversized_response = lint_yaml_bytes(&oversized);
        let invalid_utf8_response = lint_yaml_bytes(&[0xff]);

        for response in [oversized_response, invalid_utf8_response] {
            assert!(!response.ok);
            assert_eq!(
                response.error.expect("diagnostic").code,
                CompositionDiagnosticCode::InvalidComposition
            );
        }
    }

    #[test]
    fn lint_reads_at_most_one_byte_over_the_yaml_limit() {
        let mut file = tempfile::NamedTempFile::new().expect("temporary YAML file");
        file.write_all(&vec![b' '; MAX_COMPOSITION_YAML_BYTES + 100])
            .expect("write oversized file");

        let response = lint_file(file.path()).expect("file read succeeds");

        assert!(!response.ok);
        assert_eq!(
            response.error.expect("diagnostic").code,
            CompositionDiagnosticCode::InvalidComposition
        );
    }

    #[test]
    fn local_raw_projection_matches_the_portable_composition_read_dto() {
        let entry_id = EntryId::from(Uuid::from_u128(1001));
        let revision_id = RevisionId::from(Uuid::from_u128(1002));
        let raw = ugoite_iceberg::composition::RawCompositionRevision {
            revision: EntryRevision {
                form_id: FormId::from(Uuid::from_u128(1000)),
                entry_id,
                revision_id,
                parent_revision_id: None,
                entry_version: 3,
                change_id: "change-1".to_string(),
                expected_version: Some(2),
                operation: EntryOperation::Upsert,
                committed_at_micros: 123,
                author_id: "author-1".to_string(),
                form_version: FormVersion::new(1).expect("Form version"),
                source_kind: "core".to_string(),
                source_id: None,
                entry: EntryMetadata {
                    external_id: entry_id.to_string(),
                    tags: vec!["monthly".to_string()],
                    updated_by: "author-1".to_string(),
                    ..EntryMetadata::default()
                },
                values: BTreeMap::from([(
                    FieldId::new(101).expect("Field ID"),
                    FieldValue::String("not: valid: yaml".to_string()),
                )]),
                extra_attributes: BTreeMap::new(),
                extension_metadata: BTreeMap::new(),
            },
            fields: BTreeMap::from([
                ("name".to_string(), json!("Future format")),
                ("format_version".to_string(), json!(99)),
                ("spec".to_string(), json!("not: valid: yaml")),
            ]),
            unmapped_field_values: BTreeMap::from([(FieldId::new(102).unwrap(), json!(true))]),
        };

        let projected = local_raw_revision_to_api(raw);
        assert_eq!(projected.revision.entry_id, entry_id.to_string());
        assert_eq!(projected.revision.revision_id, revision_id.to_string());
        assert_eq!(projected.revision.entry.tags, ["monthly"]);
        assert_eq!(projected.fields["format_version"], json!(99));
        assert_eq!(projected.fields["spec"], "not: valid: yaml");
        assert_eq!(projected.unmapped_field_values["102"], true);
    }

    #[test]
    fn raw_spec_output_preserves_strings_and_represents_non_string_carriers() {
        let mut record: ugoite_api_client::CompositionRawRevision = serde_json::from_value(json!({
            "revision": {
                "form_id": "form-1",
                "entry_id": "entry-1",
                "revision_id": "revision-1",
                "parent_revision_id": null,
                "entry_version": 1,
                "change_id": "change-1",
                "expected_version": null,
                "operation": "upsert",
                "committed_at_micros": 1,
                "author_id": "author-1",
                "form_version": 1,
                "source_kind": "core",
                "source_id": null,
                "entry": {
                    "external_id": "entry-1",
                    "tags": [],
                    "created_at_micros": 1,
                    "updated_at_micros": 1,
                    "updated_by": "author-1",
                    "integrity": {"checksum": "", "signature": ""},
                    "deleted": false,
                    "deleted_at_micros": null,
                    "deleted_by": null,
                    "restored_from": null
                },
                "extra_attributes": {},
                "extension_metadata": {}
            },
            "fields": {"spec": "format_version: 99\nraw: true"},
            "unmapped_field_values": {}
        }))
        .expect("raw Composition DTO");

        assert!(matches!(
            raw_spec_output(&record),
            RawSpecOutput::Text(ref spec) if spec == "format_version: 99\nraw: true"
        ));

        record
            .fields
            .insert("spec".to_string(), json!({"broken": true}));
        assert!(matches!(
            raw_spec_output(&record),
            RawSpecOutput::Json(Value::Object(value)) if value.get("broken") == Some(&json!(true))
        ));
    }

    #[test]
    fn inspect_selects_only_the_requested_composition_revision() {
        let latest = prepare_request(
            "composition.get",
            &composition_get_arguments("demo", "composition-1", None),
            None,
        )
        .expect("latest read request");
        assert_eq!(latest.path, "/spaces/demo/compositions/composition-1");

        let exact = prepare_request(
            "composition.get",
            &composition_get_arguments("demo", "composition-1", Some("revision-2")),
            None,
        )
        .expect("exact revision read request");
        assert_eq!(
            exact.path,
            "/spaces/demo/compositions/composition-1/history/revision-2"
        );
    }

    #[tokio::test]
    async fn inspect_missing_exact_revision_does_not_fall_back_to_latest() -> Result<()> {
        let root = tempfile::tempdir()?;
        let root_path = root.path().to_string_lossy().into_owned();
        let service =
            ugoite_iceberg::service::UgoiteService::new_without_background_refresh(&root_path)?;
        let owner = Uuid::from_u128(2_001);
        let space_id = service
            .create_space_for_principal("inspect-exact-no-fallback", owner, "Owner")
            .await?
            .to_string();
        let document = ugoite_domain::composition::canonicalize_composition_yaml(MONTHLY_EXPENSE)
            .expect("shared Composition fixture parses")
            .document;
        let saved = service
            .save_composition_authorized_for_principals(
                &space_id,
                ugoite_iceberg::composition::CompositionSaveRequest {
                    entry_id: None,
                    base_revision_id: None,
                    document,
                    tags: None,
                },
                "Owner",
                &[owner],
            )
            .await?;
        let target = SpaceTarget::Core {
            root: root_path,
            space_id,
        };
        let missing_revision_id = Uuid::from_u128(2_002).to_string();

        let error = read_composition(
            &target,
            &saved.entry_id.to_string(),
            Some(&missing_revision_id),
        )
        .await
        .expect_err("a missing exact revision must not return latest");
        assert!(error
            .downcast_ref::<AppError>()
            .is_some_and(|error| error.code() == ErrorCode::EntryNotFound));

        let latest = read_composition(&target, &saved.entry_id.to_string(), None).await?;
        assert_eq!(latest.revision.revision_id, saved.revision_id.to_string());
        Ok(())
    }
}
