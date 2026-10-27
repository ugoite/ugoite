use crate::cli_config::{resolve_command_target, SpaceTarget};
use crate::http;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use ugoite_api_client::{
    prepare_request, CompositionComponentBinding, CompositionComponentKind,
    CompositionDeferredArgument, CompositionDependencyIndex,
    CompositionDiagnostic as ApiCompositionDiagnostic,
    CompositionDiagnosticCode as ApiCompositionDiagnosticCode, CompositionEntryIntegrity,
    CompositionEntryMetadata, CompositionEvaluationQueryDiagnostic, CompositionEvaluationRequest,
    CompositionEvaluationRequestDiagnostic, CompositionEvaluationResponse,
    CompositionEvaluationResultContractDiagnostic, CompositionEvaluationResultDiagnostic,
    CompositionEvaluationTarget, CompositionFormBinding,
    CompositionGraphQueryResult as ApiCompositionGraphQueryResult, CompositionHistoryPage,
    CompositionLintError, CompositionLintResponse, CompositionLintValue, CompositionListItem,
    CompositionListPage, CompositionMetricPresentation as ApiCompositionMetricPresentation,
    CompositionPublicationReceipt, CompositionQuery, CompositionQueryResultType,
    CompositionRawRevision, CompositionRestoreRequest, CompositionRestoreResponse,
    CompositionResultColumn, CompositionResultFieldType as ApiCompositionResultFieldType,
    CompositionRevisionMetadata, CompositionRevisionReference,
    CompositionSaveRequest as ApiCompositionSaveRequest, CompositionSaveResponse,
    CompositionScalarCardinality, CompositionScalarDescriptor, CompositionScalarType,
    CompositionVariableConsumers,
};
use ugoite_core::composition::{
    CompositionScalarCardinality as CoreCompositionScalarCardinality, ResolvedComponentKind,
    ResolvedQueryRequest,
};
use ugoite_domain::composition::{
    canonicalize_composition_yaml, parse_composition_yaml, parse_variable_graph_input,
    CanonicalComposition, CompositionDiagnosticCode, CompositionMetricPresentation,
    VariableGraphInputValue, VariableGraphLiteral, VariableGraphNode, VariableGraphNodeKind,
    VariableGraphResultType, VariableGraphScalarDescriptor, VariableGraphScalarType,
    VariableGraphValidatedResult, MAX_COMPOSITION_YAML_BYTES,
};
use ugoite_domain::id::{EntryId, RevisionId};
use ugoite_iceberg::service::{
    CompositionGraphEvaluationPlan as CoreCompositionGraphEvaluationPlan,
    CompositionQueryEvaluationResponse as CoreCompositionQueryEvaluationResponse,
    CompositionQueryEvaluationTarget as CoreCompositionQueryEvaluationTarget, UgoiteService,
};
use uuid::Uuid;

const COMPOSITION_LIST_PAGE_SIZE: usize = 100;
const COMPOSITION_HISTORY_PAGE_SIZE: usize = 100;

#[derive(Args)]
pub struct CompositionCmd {
    #[command(subcommand)]
    pub sub: CompositionSubCmd,
}

#[derive(Subcommand)]
pub enum CompositionSubCmd {
    /// List saved Compositions in the selected Space
    #[command(
        long_about = "List the current bounded page of saved Compositions. The default page contains up to 100 items; use --limit and --offset to select another page."
    )]
    List {
        #[arg(long, value_name = "ITEMS", help = "Page size (1–100; default: 100)")]
        limit: Option<usize>,
        #[arg(
            long,
            value_name = "ITEMS",
            help = "Number of items to skip (default: 0)"
        )]
        offset: Option<usize>,
    },
    /// Read one bounded page of raw revision history for a saved Composition
    #[command(
        long_about = "Read the append-only raw revision history for a saved Composition in the selected local Core or remote Space. The default page contains up to 100 revisions; use --limit and --offset to select another page."
    )]
    History {
        #[arg(value_name = "COMPOSITION_ID")]
        composition_id: String,
        #[arg(
            long,
            value_name = "REVISIONS",
            help = "Page size (1–100; default: 100)"
        )]
        limit: Option<usize>,
        #[arg(
            long,
            value_name = "REVISIONS",
            help = "Number of revisions to skip (default: 0)"
        )]
        offset: Option<usize>,
    },
    /// Restore one exact historical revision as a new append-only revision
    #[command(
        long_about = "Restore the exact source revision as a new append-only revision. The supplied base revision must still be current. Repeat the same --revision, --base-revision, and --idempotency-key values to safely retry an uncertain result."
    )]
    Restore {
        #[arg(value_name = "COMPOSITION_ID")]
        composition_id: String,
        #[arg(long, value_name = "SOURCE_REVISION_ID")]
        revision: String,
        #[arg(long, value_name = "BASE_REVISION_ID")]
        base_revision: String,
        #[arg(long, value_name = "KEY")]
        idempotency_key: String,
    },
    /// Import a new Composition from a local YAML file
    #[command(
        long_about = "Validate and canonicalize the local Composition YAML, then create a new Space-owned Composition. Repeat the same --idempotency-key with the same file to safely recover an uncertain result. Import always creates; use `save` with --composition-id and --base-revision to update."
    )]
    Import {
        #[arg(value_name = "FILE")]
        file: PathBuf,
        #[arg(long = "var", value_name = "ID=VALUE", action = clap::ArgAction::Append, help = "Set a typed input; use ID= to explicitly unset it")]
        variables: Vec<String>,
        #[arg(long, value_name = "KEY")]
        idempotency_key: String,
    },
    /// Save a Composition file by creating or updating one exact revision
    #[command(
        long_about = "Validate and canonicalize the local Composition YAML, then publish it to the selected Space. Omit --composition-id and --base-revision to create; provide both to update against that exact current base. Repeat the same --idempotency-key and request to safely recover an uncertain result."
    )]
    Save {
        #[arg(value_name = "FILE")]
        file: PathBuf,
        #[arg(long, value_name = "COMPOSITION_ID", requires = "base_revision")]
        composition_id: Option<String>,
        #[arg(long, value_name = "REVISION_ID", requires = "composition_id")]
        base_revision: Option<String>,
        #[arg(long = "var", value_name = "ID=VALUE", action = clap::ArgAction::Append, help = "Set a typed input; use ID= to explicitly unset it")]
        variables: Vec<String>,
        #[arg(long, value_name = "KEY")]
        idempotency_key: String,
    },
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
    /// Export a saved Composition's raw spec to a local file
    #[command(
        long_about = "Export the stored raw Composition spec without parsing or re-canonicalizing it. The default selects the current revision; use --revision to export one exact historical revision. Existing output files are never replaced."
    )]
    Export {
        #[arg(value_name = "COMPOSITION_ID")]
        composition_id: String,
        #[arg(long, value_name = "REVISION_ID")]
        revision: Option<String>,
        #[arg(long, value_name = "FILE")]
        output: PathBuf,
    },
    /// Resolve and execute one bounded page for each Composition query variable
    #[command(
        long_about = "Resolve and execute Composition query variables. Repeat --var ID=VALUE to supply typed inputs. Use --var ID= to explicitly unset an input and bypass its default; omit --var to allow a declared default to apply."
    )]
    Query {
        #[arg(value_name = "COMPOSITION_ID")]
        composition_id: String,
        #[arg(long, value_name = "REVISION_ID")]
        revision: Option<String>,
        #[arg(
            long = "var",
            value_name = "ID=VALUE",
            action = clap::ArgAction::Append,
            help = "Set a typed variable; use ID= to explicitly unset it"
        )]
        variables: Vec<String>,
    },
}

/// Composition lint is offline; saved-document reads resolve a Space context.
pub async fn run(
    cmd: CompositionCmd,
    explicit_config: Option<&Path>,
    context_override: Option<&str>,
) -> Result<()> {
    match cmd.sub {
        CompositionSubCmd::List { limit, offset } => {
            let target =
                resolve_command_target(explicit_config, context_override, "composition list")?;
            let page = list_compositions(
                &target,
                limit.unwrap_or(COMPOSITION_LIST_PAGE_SIZE),
                offset.unwrap_or(0),
            )
            .await?;
            crate::output::print_json(&page);
        }
        CompositionSubCmd::History {
            composition_id,
            limit,
            offset,
        } => {
            let target =
                resolve_command_target(explicit_config, context_override, "composition history")?;
            let page = read_composition_history(
                &target,
                &composition_id,
                limit.unwrap_or(COMPOSITION_HISTORY_PAGE_SIZE),
                offset.unwrap_or(0),
            )
            .await?;
            crate::output::print_json(&page);
        }
        CompositionSubCmd::Restore {
            composition_id,
            revision,
            base_revision,
            idempotency_key,
        } => {
            let target =
                resolve_command_target(explicit_config, context_override, "composition restore")?;
            let restored = restore_composition(
                &target,
                &composition_id,
                &revision,
                &base_revision,
                &idempotency_key,
            )
            .await?;
            crate::output::print_json(&restored);
        }
        CompositionSubCmd::Import {
            file,
            variables,
            idempotency_key,
        } => {
            let target =
                resolve_command_target(explicit_config, context_override, "composition import")?;
            let saved =
                save_composition_file(&target, &file, None, None, &variables, &idempotency_key)
                    .await?;
            crate::output::print_json(&saved);
        }
        CompositionSubCmd::Save {
            file,
            composition_id,
            base_revision,
            variables,
            idempotency_key,
        } => {
            let target =
                resolve_command_target(explicit_config, context_override, "composition save")?;
            let saved = save_composition_file(
                &target,
                &file,
                composition_id.as_deref(),
                base_revision.as_deref(),
                &variables,
                &idempotency_key,
            )
            .await?;
            crate::output::print_json(&saved);
        }
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
        CompositionSubCmd::Export {
            composition_id,
            revision,
            output,
        } => {
            let target =
                resolve_command_target(explicit_config, context_override, "composition export")?;
            let receipt =
                export_composition(&target, &composition_id, revision.as_deref(), &output).await?;
            crate::output::print_json(&receipt);
        }
        CompositionSubCmd::Query {
            composition_id,
            revision,
            variables,
        } => {
            let target =
                resolve_command_target(explicit_config, context_override, "composition query")?;
            query_composition(&target, &composition_id, revision.as_deref(), &variables).await?;
        }
    }
    Ok(())
}

async fn save_composition_file(
    target: &SpaceTarget,
    file: &Path,
    composition_id: Option<&str>,
    base_revision_id: Option<&str>,
    variable_assignments: &[String],
    idempotency_key: &str,
) -> Result<CompositionSaveResponse> {
    anyhow::ensure!(
        composition_id.is_some() == base_revision_id.is_some(),
        "Composition updates require both --composition-id and --base-revision"
    );

    // Validate and canonicalize the shared Domain document before a request
    // can create or mutate the Composition Registry.
    let canonical = canonicalize_composition_file(file)?;
    let (variables, unset_variables) =
        parse_cli_variables(&canonical.document.spec.variables, variable_assignments)?;
    let parsed_entry_id = composition_id
        .map(|value| parse_composition_uuid(value, "Composition ID").map(EntryId::from_uuid))
        .transpose()?;
    let parsed_base_revision_id = base_revision_id
        .map(|value| parse_composition_uuid(value, "base revision ID").map(RevisionId::from_uuid))
        .transpose()?;
    let request = ApiCompositionSaveRequest {
        composition_id: composition_id.map(str::to_owned),
        base_revision_id: base_revision_id.map(str::to_owned),
        yaml: canonical.yaml.clone(),
        variables: variables.clone(),
        unset_variables: unset_variables.clone(),
    };
    let arguments = json!({
        "space_id": target_space_id(target),
        "idempotency_key": idempotency_key,
    });
    let body = serde_json::to_value(&request).context("encode Composition save request")?;
    // Keep Core and remote request validation aligned with the portable
    // operation, including required retry identity.
    prepare_request("composition.save", &arguments, Some(&body))?;

    let response = match target {
        SpaceTarget::Core { root, space_id } => {
            let service = UgoiteService::new_without_background_refresh(root)?;
            let result = service
                .save_composition_local_with_work(
                    space_id,
                    ugoite_iceberg::composition::CompositionSaveRequest {
                        entry_id: parsed_entry_id,
                        base_revision_id: parsed_base_revision_id,
                        document: canonical.document.clone(),
                    },
                    "local-cli",
                    idempotency_key,
                    ugoite_iceberg::composition::CompositionSaveWork {
                        variables: variables.clone(),
                        unset_variables: unset_variables.clone(),
                    },
                )
                .await?;
            CompositionSaveResponse {
                composition_id: result.entry_id.to_string(),
                revision_id: result.revision_id.to_string(),
                canonical_yaml: result.canonical_yaml,
                receipt: CompositionPublicationReceipt {
                    command_id: result.receipt.command_id,
                    catalog_generation: result.receipt.catalog_generation,
                    snapshot_id: result.receipt.snapshot_id,
                    committed_revision_ids: result
                        .receipt
                        .committed_revision_ids
                        .into_iter()
                        .map(|revision_id| revision_id.to_string())
                        .collect(),
                    committed_at_micros: result.receipt.committed_at_micros,
                    data_file_count: result.receipt.data_file_count,
                },
            }
        }
        SpaceTarget::Remote { .. } => {
            let value =
                http::execute_for_target(target, "composition.save", arguments, Some(body)).await?;
            serde_json::from_value(value).context("decode Composition save response")?
        }
    };

    anyhow::ensure!(
        Uuid::parse_str(&response.composition_id).is_ok()
            && composition_id.is_none_or(|expected| {
                composition_identifiers_match(&response.composition_id, expected)
            })
            && Uuid::parse_str(&response.revision_id).is_ok()
            && response.canonical_yaml == canonical.yaml
            && response.receipt.committed_revision_ids == [response.revision_id.as_str()],
        "Composition save receipt does not confirm the canonical publication"
    );
    Ok(response)
}

fn parse_composition_uuid(value: &str, name: &str) -> Result<Uuid> {
    Uuid::parse_str(value).map_err(|_| {
        ugoite_core::error::AppError::invalid_input(
            ugoite_core::error::ErrorCode::InvalidInput,
            format!("{name} must be a UUID"),
        )
        .into()
    })
}

fn canonicalize_composition_file(path: &Path) -> Result<CanonicalComposition> {
    let file = File::open(path).map_err(|error| {
        ugoite_core::error::AppError::invalid_input(
            ugoite_core::error::ErrorCode::InvalidInput,
            format!("open Composition file {}: {error}", path.display()),
        )
    })?;
    let mut bytes = Vec::with_capacity(MAX_COMPOSITION_YAML_BYTES + 1);
    file.take((MAX_COMPOSITION_YAML_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            ugoite_core::error::AppError::invalid_input(
                ugoite_core::error::ErrorCode::InvalidInput,
                format!("read Composition file {}: {error}", path.display()),
            )
        })?;
    if bytes.len() > MAX_COMPOSITION_YAML_BYTES {
        return Err(crate::output::CompositionDiagnosticError(
            CompositionDiagnosticCode::InvalidComposition,
        )
        .into());
    }
    let yaml = std::str::from_utf8(&bytes).map_err(|_| {
        anyhow::Error::from(crate::output::CompositionDiagnosticError(
            CompositionDiagnosticCode::InvalidComposition,
        ))
    })?;
    canonicalize_composition_yaml(yaml)
        .map_err(|code| crate::output::CompositionDiagnosticError(code).into())
}

async fn restore_composition(
    target: &SpaceTarget,
    composition_id: &str,
    source_revision_id: &str,
    base_revision_id: &str,
    idempotency_key: &str,
) -> Result<CompositionRestoreResponse> {
    let request = CompositionRestoreRequest {
        source_revision_id: source_revision_id.to_owned(),
        base_revision_id: base_revision_id.to_owned(),
    };
    let arguments = json!({
        "space_id": target_space_id(target),
        "composition_id": composition_id,
        "idempotency_key": idempotency_key,
    });
    let body = serde_json::to_value(&request).context("encode Composition restore request")?;
    // Use the shared portable request validator in Core mode too, so argument
    // and body constraints do not drift between local and remote execution.
    prepare_request("composition.restore", &arguments, Some(&body))?;

    let response = match target {
        SpaceTarget::Core { root, space_id } => {
            let service = UgoiteService::new_without_background_refresh(root)?;
            let result = service
                .restore_composition_local_with_operation_id(
                    space_id,
                    composition_id,
                    source_revision_id,
                    base_revision_id,
                    "local-cli",
                    idempotency_key,
                )
                .await?;
            CompositionRestoreResponse {
                composition_id: result.entry_id.to_string(),
                revision_id: result.revision_id.to_string(),
                restored_from_revision_id: result.restored_from_revision_id.to_string(),
                canonical_yaml: result.canonical_yaml,
                receipt: CompositionPublicationReceipt {
                    command_id: result.receipt.command_id,
                    catalog_generation: result.receipt.catalog_generation,
                    snapshot_id: result.receipt.snapshot_id,
                    committed_revision_ids: result
                        .receipt
                        .committed_revision_ids
                        .into_iter()
                        .map(|revision_id| revision_id.to_string())
                        .collect(),
                    committed_at_micros: result.receipt.committed_at_micros,
                    data_file_count: result.receipt.data_file_count,
                },
            }
        }
        SpaceTarget::Remote { .. } => {
            let result =
                http::execute_for_target(target, "composition.restore", arguments, Some(body))
                    .await?;
            serde_json::from_value(result).context("decode Composition restore response")?
        }
    };

    anyhow::ensure!(
        composition_identifiers_match(&response.composition_id, composition_id),
        "Composition restore response names an unexpected Composition"
    );
    anyhow::ensure!(
        composition_identifiers_match(&response.restored_from_revision_id, source_revision_id,),
        "Composition restore response names an unexpected source revision"
    );
    anyhow::ensure!(
        response.receipt.committed_revision_ids == [response.revision_id.as_str()],
        "Composition restore receipt does not confirm the returned revision"
    );
    Ok(response)
}

fn composition_identifiers_match(actual: &str, requested: &str) -> bool {
    match (
        uuid::Uuid::parse_str(actual),
        uuid::Uuid::parse_str(requested),
    ) {
        (Ok(actual), Ok(requested)) => actual == requested,
        _ => actual == requested,
    }
}

async fn list_compositions(
    target: &SpaceTarget,
    limit: usize,
    offset: usize,
) -> Result<CompositionListPage> {
    match target {
        SpaceTarget::Core { root, space_id } => {
            let service =
                ugoite_iceberg::service::UgoiteService::new_without_background_refresh(root)?;
            let raw = service
                .list_compositions_local_page(space_id, limit, offset)
                .await?;
            Ok(local_composition_list_page_to_api(raw))
        }
        SpaceTarget::Remote { space_uid, .. } => {
            let arguments = composition_list_arguments(space_uid, limit, offset);
            let result =
                http::execute_for_target(target, "composition.list", arguments, None).await?;
            serde_json::from_value(result).context("decode Composition list response")
        }
    }
}

fn composition_list_arguments(space_id: &str, limit: usize, offset: usize) -> Value {
    json!({
        "space_id": space_id,
        "limit": limit,
        "offset": offset,
    })
}

fn local_composition_list_page_to_api(
    page: ugoite_iceberg::composition::RawCompositionListPage,
) -> CompositionListPage {
    CompositionListPage {
        items: page
            .items
            .into_iter()
            .map(|item| CompositionListItem {
                composition_id: item.entry_id,
                revision_id: item.revision_id.to_string(),
                updated_at: item.updated_at,
                name: item.name,
                kind: item.kind,
                format_version: item.format_version,
            })
            .collect(),
        offset: page.offset,
        limit: page.limit,
        has_more: page.has_more,
    }
}

async fn read_composition_history(
    target: &SpaceTarget,
    composition_id: &str,
    limit: usize,
    offset: usize,
) -> Result<CompositionHistoryPage> {
    match target {
        SpaceTarget::Core { root, space_id } => {
            let service =
                ugoite_iceberg::service::UgoiteService::new_without_background_refresh(root)?;
            let raw = service
                .composition_history_local_page(space_id, composition_id, limit, offset)
                .await?;
            Ok(local_composition_history_page_to_api(raw))
        }
        SpaceTarget::Remote { space_uid, .. } => {
            let arguments = composition_history_arguments(space_uid, composition_id, limit, offset);
            let result =
                http::execute_for_target(target, "composition.history", arguments, None).await?;
            serde_json::from_value(result).context("decode Composition history response")
        }
    }
}

fn composition_history_arguments(
    space_id: &str,
    composition_id: &str,
    limit: usize,
    offset: usize,
) -> Value {
    json!({
        "space_id": space_id,
        "composition_id": composition_id,
        "limit": limit,
        "offset": offset,
    })
}

fn local_composition_history_page_to_api(
    page: ugoite_iceberg::composition::RawCompositionHistoryPage,
) -> CompositionHistoryPage {
    CompositionHistoryPage {
        entry_id: page.entry_id.to_string(),
        revisions: page
            .revisions
            .into_iter()
            .map(local_raw_revision_to_api)
            .collect(),
        total: page.total,
        offset: page.offset,
        limit: page.limit,
        has_more: page.has_more,
    }
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

#[derive(Debug, Serialize)]
struct CompositionExportReceipt {
    composition_id: String,
    revision_id: String,
    output: String,
    bytes: usize,
}

async fn export_composition(
    target: &SpaceTarget,
    composition_id: &str,
    revision_id: Option<&str>,
    output_path: &Path,
) -> Result<CompositionExportReceipt> {
    let record = read_composition(target, composition_id, revision_id).await?;
    let bytes = write_raw_spec_to_path(&record, output_path)?;
    Ok(CompositionExportReceipt {
        composition_id: record.revision.entry_id,
        revision_id: record.revision.revision_id,
        output: output_path.to_string_lossy().into_owned(),
        bytes,
    })
}

async fn query_composition(
    target: &SpaceTarget,
    composition_id: &str,
    requested_revision: Option<&str>,
    raw_variables: &[String],
) -> Result<()> {
    let composition = read_composition(target, composition_id, requested_revision).await?;
    let (_variable_definitions, variable_values, unset_variables) =
        match composition.fields.get("spec").and_then(Value::as_str) {
            Some(yaml) => match parse_composition_yaml(yaml) {
                Ok(document) => {
                    let definitions = document.spec.variables;
                    let (values, unset) = parse_cli_variables(&definitions, raw_variables)?;
                    (definitions, values, unset)
                }
                Err(_) => {
                    let (values, unset) = parse_cli_variables(&[], raw_variables)?;
                    (Vec::new(), values, unset)
                }
            },
            None => {
                let (values, unset) = parse_cli_variables(&[], raw_variables)?;
                (Vec::new(), values, unset)
            }
        };
    if matches!(target, SpaceTarget::Remote { .. }) {
        return query_composition_remote(
            target,
            composition_id,
            &composition.revision.revision_id,
            variable_values,
            unset_variables,
        )
        .await;
    }
    let local_service = match target {
        SpaceTarget::Core { root, .. } => {
            Some(UgoiteService::new_without_background_refresh(root)?)
        }
        SpaceTarget::Remote { .. } => None,
    };

    let response = local_service
        .as_ref()
        .context("local Composition graph evaluation service is unavailable")?
        .evaluate_composition_graph_local(
            target_space_id(target),
            CoreCompositionQueryEvaluationTarget::SavedRevision {
                composition_id: composition_id.to_owned(),
                revision_id: composition.revision.revision_id.clone(),
            },
            Uuid::now_v7(),
            None,
            None,
            &variable_values,
            &unset_variables,
            None,
            None,
        )
        .await?;
    crate::output::print_json(&local_composition_evaluation_response(response)?);
    Ok(())
}

async fn query_composition_remote(
    target: &SpaceTarget,
    composition_id: &str,
    revision_id: &str,
    variables: BTreeMap<String, Value>,
    unset_variables: Vec<String>,
) -> Result<()> {
    let request = CompositionEvaluationRequest {
        evaluation_id: Uuid::now_v7().to_string(),
        target: CompositionEvaluationTarget::SavedRevision {
            composition_id: composition_id.to_owned(),
            revision_id: revision_id.to_owned(),
        },
        base_entry_id: None,
        base_revision_id: None,
        variables,
        unset_variables,
        requested_queries: None,
        page: None,
    };
    let arguments = json!({"space_id": target_space_id(target)});
    let body = serde_json::to_value(&request).context("encode Composition evaluation request")?;
    prepare_request("composition.evaluate", &arguments, Some(&body))?;
    let result =
        http::execute_for_target(target, "composition.evaluate", arguments, Some(body)).await?;
    let response: ugoite_api_client::CompositionEvaluationResponse =
        serde_json::from_value(result).context("decode Composition graph evaluation response")?;
    crate::output::print_json(&response);
    Ok(())
}

fn local_composition_evaluation_response(
    response: CoreCompositionQueryEvaluationResponse,
) -> Result<CompositionEvaluationResponse> {
    let mut composition_revision = None;
    let mut draft_fingerprint = response.draft_fingerprint.clone();
    let mut variable_values = BTreeMap::new();
    let mut queries = Vec::new();
    let mut component_bindings = Vec::new();
    let mut dependencies = CompositionDependencyIndex::default();
    if let Some(plan) = response.plan {
        let CoreCompositionGraphEvaluationPlan {
            composition_revision: revision,
            draft_fingerprint: draft,
            variable_values: values,
            queries: core_queries,
            component_bindings: core_bindings,
            dependencies: core_dependencies,
        } = plan;
        composition_revision = revision.map(|revision| CompositionRevisionReference {
            entry_id: revision.entry_id.to_string(),
            revision_id: revision.revision_id.to_string(),
        });
        draft_fingerprint = draft;
        variable_values = values
            .into_iter()
            .map(|(id, value)| (id, composition_literal_to_json(&value)))
            .collect();
        (queries, component_bindings) =
            composition_queries_and_bindings(core_queries, core_bindings)?;
        dependencies = composition_dependency_index(core_dependencies);
    }

    let query_results = response
        .query_results
        .into_iter()
        .map(|(variable_id, result)| {
            let exact_integer_rows = match &result.result {
                VariableGraphValidatedResult::Table { rows, .. } => Some(
                    rows.iter()
                        .map(|row| {
                            row.iter()
                                .map(|cell| match cell {
                                    VariableGraphLiteral::Integer(value) => Some(value.to_string()),
                                    _ => None,
                                })
                                .collect::<Vec<_>>()
                        })
                        .collect::<Vec<_>>(),
                ),
                VariableGraphValidatedResult::Scalar { .. } => None,
            };
            let value = serde_json::to_value(&result.result)
                .context("encode local Composition graph query result")?;
            Ok((
                variable_id,
                ApiCompositionGraphQueryResult {
                    value,
                    has_more: result.has_more,
                    next: result.next,
                    exact_integer_rows,
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;

    let diagnostics = response
        .diagnostics
        .into_iter()
        .map(|diagnostic| {
            let code = ApiCompositionDiagnosticCode::from_code(diagnostic.code.as_str()).context(
                "local Composition diagnostic is missing from the portable API contract",
            )?;
            Ok(ApiCompositionDiagnostic {
                code,
                variable_id: diagnostic.variable_id,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let query_diagnostics = response
        .query_diagnostics
        .into_iter()
        .map(|diagnostic| CompositionEvaluationQueryDiagnostic {
            code: diagnostic.code.as_str().to_owned(),
            variable_id: diagnostic.variable_id,
            request_diagnostic: diagnostic.request_diagnostic.map(|request| {
                CompositionEvaluationRequestDiagnostic {
                    code: request.code.as_str().to_owned(),
                    argument_name: request.argument_name,
                    reference_id: request.reference_id,
                    domain_code: request.domain_code,
                    cycle: request.cycle,
                }
            }),
            result_diagnostic: diagnostic.result_diagnostic.map(|result| {
                CompositionEvaluationResultDiagnostic {
                    code: result.code.as_str().to_owned(),
                    variable_id: result.variable_id,
                    column_name: result.column_name,
                    result_diagnostic: result.result_diagnostic.map(|detail| {
                        CompositionEvaluationResultContractDiagnostic {
                            code: detail.code.as_str().to_owned(),
                            expected_count: detail.expected_count,
                            observed_count: detail.observed_count,
                            row_index: detail.row_index,
                            column_index: detail.column_index,
                            column_name: detail.column_name,
                            expected_column_name: detail.expected_column_name,
                            expected_value_type: detail
                                .expected_value_type
                                .map(composition_scalar_descriptor),
                            observed_value_type: detail
                                .observed_value_type
                                .map(composition_scalar_descriptor),
                        }
                    }),
                }
            }),
        })
        .collect();

    Ok(CompositionEvaluationResponse {
        evaluation_id: response.evaluation_id.to_string(),
        ok: response.ok,
        snapshot_fingerprint: response.snapshot_fingerprint,
        publication_fingerprint: response.publication_fingerprint,
        authorization_fingerprint: response.authorization_fingerprint,
        composition_revision,
        draft_fingerprint,
        variable_values,
        queries,
        component_bindings,
        dependencies,
        query_results,
        computed_query_ids: response.computed_query_ids,
        diagnostics,
        query_diagnostics,
    })
}

fn composition_literal_to_json(value: &VariableGraphLiteral) -> Value {
    match value {
        VariableGraphLiteral::String(value)
        | VariableGraphLiteral::Date(value)
        | VariableGraphLiteral::Timestamp(value)
        | VariableGraphLiteral::Decimal(value) => json!(value),
        VariableGraphLiteral::Boolean(value) => json!(value),
        VariableGraphLiteral::Integer(value) => json!(value),
        VariableGraphLiteral::Float(value) => json!(value),
        VariableGraphLiteral::Json(value) => value.clone(),
        VariableGraphLiteral::Null => Value::Null,
    }
}

fn composition_scalar_descriptor(
    descriptor: VariableGraphScalarDescriptor,
) -> CompositionScalarDescriptor {
    CompositionScalarDescriptor {
        scalar_type: match descriptor.scalar_type {
            VariableGraphScalarType::String => CompositionScalarType::String,
            VariableGraphScalarType::Boolean => CompositionScalarType::Boolean,
            VariableGraphScalarType::Integer => CompositionScalarType::Integer,
            VariableGraphScalarType::Float => CompositionScalarType::Float,
            VariableGraphScalarType::Decimal => CompositionScalarType::Decimal,
            VariableGraphScalarType::Date => CompositionScalarType::Date,
            VariableGraphScalarType::Timestamp => CompositionScalarType::Timestamp,
            VariableGraphScalarType::Json => CompositionScalarType::Json,
        },
        nullable: descriptor.nullable,
    }
}

fn composition_query_result_type(returns: VariableGraphResultType) -> CompositionQueryResultType {
    match returns {
        VariableGraphResultType::Scalar { value_type } => CompositionQueryResultType::Scalar {
            value_type: composition_scalar_descriptor(value_type),
        },
        VariableGraphResultType::Table { columns } => CompositionQueryResultType::Table {
            columns: columns.into_iter().map(composition_result_column).collect(),
        },
    }
}

fn composition_result_column(
    column: ugoite_domain::composition::VariableGraphColumn,
) -> CompositionResultColumn {
    CompositionResultColumn {
        name: column.name,
        value_type: composition_scalar_descriptor(column.value_type),
    }
}

fn composition_result_field_type(
    result_type: ugoite_domain::composition::CompositionResultFieldType,
) -> ApiCompositionResultFieldType {
    match result_type {
        ugoite_domain::composition::CompositionResultFieldType::String => {
            ApiCompositionResultFieldType::String
        }
        ugoite_domain::composition::CompositionResultFieldType::Boolean => {
            ApiCompositionResultFieldType::Boolean
        }
        ugoite_domain::composition::CompositionResultFieldType::Integer => {
            ApiCompositionResultFieldType::Integer
        }
        ugoite_domain::composition::CompositionResultFieldType::Float => {
            ApiCompositionResultFieldType::Float
        }
        ugoite_domain::composition::CompositionResultFieldType::Decimal => {
            ApiCompositionResultFieldType::Decimal
        }
        ugoite_domain::composition::CompositionResultFieldType::Date => {
            ApiCompositionResultFieldType::Date
        }
        ugoite_domain::composition::CompositionResultFieldType::Timestamp => {
            ApiCompositionResultFieldType::Timestamp
        }
        ugoite_domain::composition::CompositionResultFieldType::Json => {
            ApiCompositionResultFieldType::Json
        }
    }
}

fn composition_metric_presentation(
    presentation: CompositionMetricPresentation,
) -> ApiCompositionMetricPresentation {
    match presentation {
        CompositionMetricPresentation::Number {
            unit,
            fraction_digits,
        } => ApiCompositionMetricPresentation::Number {
            unit,
            fraction_digits,
        },
    }
}

fn composition_dependency_index(
    dependencies: ugoite_core::composition::CompositionDependencyIndex,
) -> CompositionDependencyIndex {
    CompositionDependencyIndex {
        variable_consumers: dependencies
            .variable_consumers
            .into_iter()
            .map(|(id, consumers)| {
                (
                    id,
                    CompositionVariableConsumers {
                        input_component_ids: consumers.input_component_ids,
                        text_component_ids: consumers.text_component_ids,
                        query_variable_ids: consumers.query_variable_ids,
                        metric_component_ids: consumers.metric_component_ids,
                        table_component_ids: consumers.table_component_ids,
                    },
                )
            })
            .collect(),
    }
}

fn composition_queries_and_bindings(
    core_queries: Vec<ResolvedQueryRequest>,
    core_bindings: Vec<ugoite_core::composition::ResolvedComponentBinding>,
) -> Result<(Vec<CompositionQuery>, Vec<CompositionComponentBinding>)> {
    let queries = core_queries
        .into_iter()
        .map(|query| {
            let request = query
                .request
                .map(serde_json::to_value)
                .transpose()
                .context("encode local Composition graph SQL request")?;
            let form_bindings = query
                .form_bindings
                .into_iter()
                .map(|binding| CompositionFormBinding {
                    relation: binding.relation,
                    form_id: binding.form_id.to_string(),
                })
                .collect();
            let result_columns = query
                .result_columns
                .into_iter()
                .map(composition_result_column)
                .collect();
            let scalar_cardinality =
                query
                    .scalar_cardinality
                    .map(|cardinality| match cardinality {
                        CoreCompositionScalarCardinality::ExactlyOneRow => {
                            CompositionScalarCardinality::ExactlyOneRow
                        }
                    });
            let deferred_arguments = query
                .deferred_arguments
                .into_iter()
                .map(|argument| CompositionDeferredArgument {
                    argument_name: argument.argument_name,
                    variable_id: argument.variable_id,
                    value_type: composition_scalar_descriptor(argument.value_type),
                })
                .collect();
            Ok(CompositionQuery {
                variable_id: query.variable_id,
                request,
                sql_text: query.sql_text,
                portable_sql: query.portable_sql,
                deferred_arguments,
                returns: composition_query_result_type(query.returns),
                result_columns,
                form_bindings,
                schema_fingerprint: query.schema_fingerprint,
                query_semantic_fingerprint: query.query_semantic_fingerprint,
                scalar_cardinality,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let component_bindings = core_bindings
        .into_iter()
        .map(|binding| CompositionComponentBinding {
            component_id: binding.component_id,
            kind: match binding.kind {
                ResolvedComponentKind::Input => CompositionComponentKind::Input,
                ResolvedComponentKind::Metric => CompositionComponentKind::Metric,
                ResolvedComponentKind::Tabular => CompositionComponentKind::Table,
                ResolvedComponentKind::Text => CompositionComponentKind::Text,
            },
            variable_id: binding.variable_id,
            expected_result_type: binding
                .expected_result_type
                .map(composition_result_field_type),
            presentation: binding.presentation.map(composition_metric_presentation),
        })
        .collect();
    Ok((queries, component_bindings))
}

fn parse_cli_variables(
    definitions: &[VariableGraphNode],
    assignments: &[String],
) -> Result<(BTreeMap<String, Value>, Vec<String>)> {
    let mut values = BTreeMap::new();
    let mut assigned = BTreeSet::new();
    let mut unset_variables = BTreeSet::new();
    for assignment in assignments {
        let (key, raw_value) = assignment.split_once('=').ok_or_else(|| {
            crate::output::UsageError(format!("--var must be ID=VALUE, got {assignment:?}"))
        })?;
        if key.is_empty() {
            return Err(crate::output::UsageError("--var id must not be empty".to_string()).into());
        }
        if !assigned.insert(key.to_string()) {
            return Err(crate::output::UsageError(format!(
                "Composition variable {key:?} was supplied more than once"
            ))
            .into());
        }
        let definition = definitions.iter().find(|definition| definition.id == key);
        let value = match definition {
            Some(VariableGraphNode {
                kind: VariableGraphNodeKind::Input { value_type, .. },
                ..
            }) => {
                if raw_value.is_empty() {
                    unset_variables.insert(key.to_string());
                    continue;
                }
                parse_cli_input_value(*value_type, raw_value).ok_or_else(|| {
                    crate::output::UsageError(format!(
                        "Composition variable {key:?} has an invalid value for its declared type"
                    ))
                })?
            }
            Some(_) => {
                return Err(crate::output::UsageError(format!(
                    "Composition variable {key:?} is not an input"
                ))
                .into());
            }
            None if raw_value.is_empty() => {
                unset_variables.insert(key.to_string());
                continue;
            }
            None => Value::String(raw_value.to_string()),
        };
        values.insert(key.to_string(), value);
    }
    Ok((values, unset_variables.into_iter().collect()))
}

fn parse_cli_input_value(
    descriptor: ugoite_domain::composition::VariableGraphScalarDescriptor,
    raw: &str,
) -> Option<Value> {
    match parse_variable_graph_input(descriptor, raw) {
        VariableGraphInputValue::Valid(value) => Some(value.to_json_value()),
        VariableGraphInputValue::Unset | VariableGraphInputValue::Invalid => None,
    }
}

fn target_space_id(target: &SpaceTarget) -> &str {
    match target {
        SpaceTarget::Core { space_id, .. }
        | SpaceTarget::Remote {
            space_uid: space_id,
            ..
        } => space_id,
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

fn write_raw_spec_to_path(record: &CompositionRawRevision, path: &Path) -> Result<usize> {
    let bytes = match raw_spec_output(record) {
        RawSpecOutput::Text(spec) => spec.into_bytes(),
        RawSpecOutput::Json(value) => {
            let mut bytes = serde_json::to_vec_pretty(&value)
                .context("serialize raw Composition carrier for export")?;
            bytes.push(b'\n');
            bytes
        }
    };
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temp = tempfile::Builder::new()
        .prefix(".ugoite-composition-export-")
        .tempfile_in(directory)
        .with_context(|| {
            format!(
                "failed to create temporary Composition export beside {}",
                path.display()
            )
        })?;
    let mut writer = temp.reopen()?;
    writer
        .write_all(&bytes)
        .with_context(|| format!("write Composition export {}", path.display()))?;
    writer.sync_all()?;
    drop(writer);
    temp.persist_noclobber(path).map_err(|error| {
        anyhow::anyhow!(
            "failed to safely finalize Composition export at {}: {}",
            path.display(),
            error.error
        )
    })?;
    if let Ok(directory) = File::open(directory) {
        let _ = directory.sync_all();
    }
    Ok(bytes.len())
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
        composition_get_arguments, composition_history_arguments, composition_list_arguments,
        export_composition, lint_file, lint_yaml_bytes, list_compositions,
        local_composition_evaluation_response, local_raw_revision_to_api, parse_cli_variables,
        raw_spec_output, read_composition, read_composition_history, write_raw_spec_to_path,
        CoreCompositionGraphEvaluationPlan, CoreCompositionQueryEvaluationResponse, RawSpecOutput,
    };
    use crate::cli_config::SpaceTarget;
    use anyhow::Result;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    use std::io::Write;
    use ugoite_api_client::{
        prepare_request, CompositionDiagnosticCode, CompositionHistoryPage, CompositionListPage,
        HttpMethod, RequestBodyKind,
    };
    use ugoite_core::error::{AppError, ErrorCode};
    use ugoite_domain::composition::{
        VariableGraphNode, VariableGraphNodeKind, VariableGraphScalarDescriptor,
        VariableGraphScalarType, MAX_COMPOSITION_VARIABLE_STRING_BYTES, MAX_COMPOSITION_YAML_BYTES,
    };
    use ugoite_domain::entry::{EntryMetadata, EntryOperation, EntryRevision, FieldValue};
    use ugoite_domain::form::FormVersion;
    use ugoite_domain::id::{EntryId, FieldId, FormId, RevisionId};
    use uuid::Uuid;

    const SQL_FIRST_COMPOSITION: &str = include_str!(
        "../../../../crates/ugoite-domain/tests/fixtures/composition/sql-first.ugcomp.yaml"
    );
    const SQL_FIRST_COMPOSITION_CANONICAL: &str = include_str!(
        "../../../../crates/ugoite-domain/tests/fixtures/composition/sql-first.canonical.ugcomp.yaml"
    );
    const SQL_FIRST_COMPOSITION_FINGERPRINT: &str = include_str!(
        "../../../../crates/ugoite-domain/tests/fixtures/composition/sql-first.fingerprint.txt"
    );
    const METADATA_ONLY_COMPOSITION: &str = "format: ugoite.composition\nformat_version: 1\nname: Metadata only\nkind: dashboard\nspec:\n  variables: []\n  components:\n    - id: note\n      kind: text\n      content:\n        kind: literal\n        text: metadata\n      style: body\n  layout:\n    kind: flow\n    rows:\n      - id: main\n        items:\n          - kind: component\n            component: note\n";

    #[test]
    fn local_graph_evaluation_uses_the_portable_response_shape() {
        let response = CoreCompositionQueryEvaluationResponse {
            evaluation_id: Uuid::from_u128(1),
            ok: true,
            draft_fingerprint: None,
            snapshot_fingerprint: Some("snapshot".to_string()),
            publication_fingerprint: Some("publication".to_string()),
            authorization_fingerprint: Some("local-operator".to_string()),
            variable_definitions: None,
            plan: Some(CoreCompositionGraphEvaluationPlan {
                composition_revision: None,
                draft_fingerprint: None,
                variable_values: BTreeMap::new(),
                queries: Vec::new(),
                component_bindings: vec![ugoite_core::composition::ResolvedComponentBinding {
                    component_id: "count".to_string(),
                    kind: ugoite_core::composition::ResolvedComponentKind::Metric,
                    variable_id: Some("total".to_string()),
                    expected_result_type: Some(
                        ugoite_domain::composition::CompositionResultFieldType::Integer,
                    ),
                    presentation: None,
                }],
                dependencies: ugoite_core::composition::CompositionDependencyIndex::default(),
            }),
            query_results: BTreeMap::new(),
            computed_query_ids: Vec::new(),
            diagnostics: Vec::new(),
            query_diagnostics: Vec::new(),
        };

        let response = local_composition_evaluation_response(response)
            .expect("Core graph response maps to the portable API response");
        let value = serde_json::to_value(response).expect("portable response serializes");
        assert_eq!(value["component_bindings"][0]["kind"], "metric");
        assert_eq!(value["component_bindings"][0]["variable_id"], "total");
        assert_eq!(
            value["component_bindings"][0]["expected_result_type"],
            "integer"
        );
        assert!(value.get("plan").is_none());
        assert!(value.get("metrics").is_none());
    }

    #[test]
    fn lint_file_returns_the_shared_canonical_document_response() {
        let mut file = tempfile::NamedTempFile::new().expect("temporary YAML file");
        file.write_all(SQL_FIRST_COMPOSITION.as_bytes())
            .expect("write YAML fixture");

        let response = lint_file(file.path()).expect("lint succeeds");

        assert!(response.ok);
        assert!(response.error.is_none());
        let value = response.value.expect("canonical value");
        assert_eq!(value.canonical_yaml, SQL_FIRST_COMPOSITION_CANONICAL);
        assert_eq!(value.fingerprint.len(), 64);
        assert_eq!(value.document["format_version"], 1);
        assert_eq!(value.document["name"], "Monthly expenses");
    }

    #[test]
    fn lint_file_preserves_sql_first_query_variables_and_shared_fingerprint() {
        let mut file = tempfile::NamedTempFile::new().expect("temporary YAML file");
        file.write_all(SQL_FIRST_COMPOSITION.as_bytes())
            .expect("write SQL-first Composition fixture");

        let response = lint_file(file.path()).expect("lint succeeds");

        assert!(response.ok);
        let value = response.value.expect("canonical value");
        assert_eq!(value.canonical_yaml, SQL_FIRST_COMPOSITION_CANONICAL);
        assert_eq!(value.fingerprint, SQL_FIRST_COMPOSITION_FINGERPRINT.trim());
        assert_eq!(value.document["spec"]["variables"][2]["kind"], "query");
        assert_eq!(
            value.document["spec"]["variables"][2]["query"]["kind"],
            "inline_sql"
        );
        assert_eq!(value.document["spec"]["variables"][3]["kind"], "query");
        assert_eq!(
            value.document["spec"]["variables"][3]["query"]["kind"],
            "saved_sql"
        );
    }

    #[test]
    fn lint_returns_the_domain_diagnostic_code_for_unsupported_versions() {
        let response = lint_yaml_bytes(
            b"format: ugoite.composition\nformat_version: 22\nkind: dashboard\nname: Future\nspec: {}\n",
        );

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

    fn input_node(
        id: &str,
        scalar_type: VariableGraphScalarType,
        required: bool,
    ) -> VariableGraphNode {
        VariableGraphNode {
            id: id.to_owned(),
            kind: VariableGraphNodeKind::Input {
                value_type: VariableGraphScalarDescriptor {
                    scalar_type,
                    nullable: false,
                },
                required,
                default: None,
            },
        }
    }

    #[test]
    fn query_cli_variables_follow_the_shared_domain_types() {
        let definitions = vec![
            input_node("enabled", VariableGraphScalarType::Boolean, true),
            input_node("count", VariableGraphScalarType::Integer, true),
            input_node("ratio", VariableGraphScalarType::Float, true),
            input_node("date", VariableGraphScalarType::Date, true),
            input_node("timestamp", VariableGraphScalarType::Timestamp, true),
            input_node("title", VariableGraphScalarType::String, false),
        ];
        let (values, unset_variables) = parse_cli_variables(
            &definitions,
            &[
                "enabled=true".to_string(),
                "count=+00042".to_string(),
                "ratio=.5".to_string(),
                "date=2026-10-01".to_string(),
                "timestamp=2026-10-13T14:59:59+09:00".to_string(),
                "title=".to_string(),
                "custom=value".to_string(),
            ],
        )
        .expect("typed CLI values");

        assert_eq!(values["enabled"], json!(true));
        assert_eq!(values["count"], json!(42));
        assert_eq!(values["ratio"], json!(0.5));
        assert_eq!(values["date"], json!("2026-10-01"));
        assert_eq!(values["timestamp"], json!("2026-10-13T14:59:59+09:00"));
        assert!(!values.contains_key("title"));
        assert_eq!(unset_variables, vec!["title"]);
        assert_eq!(values["custom"], json!("value"));
    }

    #[test]
    fn query_cli_values_use_the_shared_bounded_lexical_contract() {
        let definitions = vec![
            ("enabled", VariableGraphScalarType::Boolean),
            ("count", VariableGraphScalarType::Integer),
            ("ratio", VariableGraphScalarType::Float),
            ("date", VariableGraphScalarType::Date),
            ("timestamp", VariableGraphScalarType::Timestamp),
            ("title", VariableGraphScalarType::String),
        ]
        .into_iter()
        .map(|(id, scalar_type)| input_node(id, scalar_type, false))
        .collect::<Vec<_>>();
        let max_float_token = format!("1{}", "0".repeat(63));
        let invalid_assignments = [
            "enabled=TRUE".to_string(),
            "count=12x".to_string(),
            "count=9223372036854775808".to_string(),
            "count=-9223372036854775809".to_string(),
            "ratio=1e309".to_string(),
            format!("ratio={}", "1".to_string() + &"0".repeat(64)),
            "date=2025-02-29".to_string(),
            "timestamp=2026-10-13T14:59:59".to_string(),
            format!(
                "title={}",
                "x".repeat(MAX_COMPOSITION_VARIABLE_STRING_BYTES + 1)
            ),
        ];
        for assignment in invalid_assignments {
            assert!(
                parse_cli_variables(&definitions, std::slice::from_ref(&assignment)).is_err(),
                "CLI accepted invalid typed variable assignment {assignment:?}"
            );
        }

        let (at_float_limit, unset_variables) =
            parse_cli_variables(&definitions, &[format!("ratio={max_float_token}")])
                .expect("64-byte finite float token is valid");
        assert_eq!(at_float_limit["ratio"], json!(1e63));
        assert!(unset_variables.is_empty());
        for value in [i64::MIN, i64::MAX] {
            let (values, _) = parse_cli_variables(&definitions, &[format!("count={value}")])
                .expect("signed 64-bit integer token is valid");
            assert_eq!(values["count"], json!(value));
        }
    }

    #[test]
    fn query_cli_variable_assignments_reject_malformed_or_duplicate_names() {
        assert!(parse_cli_variables(&[], &["month".to_string()]).is_err());
        assert!(parse_cli_variables(
            &[],
            &["month=2026-10".to_string(), "month=2026-11".to_string()]
        )
        .is_err());
        assert!(
            parse_cli_variables(&[], &["month=".to_string(), "month=2026-10".to_string()]).is_err()
        );
    }

    #[test]
    fn query_cli_variables_send_unknown_empty_assignments_as_unsets() {
        let (values, unset_variables) =
            parse_cli_variables(&[], &["unknown=".to_string()]).expect("valid assignment");

        assert!(values.is_empty());
        assert_eq!(unset_variables, vec!["unknown"]);
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
        let document =
            ugoite_domain::composition::canonicalize_composition_yaml(METADATA_ONLY_COMPOSITION)
                .expect("shared Composition fixture parses")
                .document;
        let saved = service
            .save_composition_authorized_for_principals(
                &space_id,
                ugoite_iceberg::composition::CompositionSaveRequest {
                    entry_id: None,
                    base_revision_id: None,
                    document,
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

    #[tokio::test]
    async fn export_writes_the_exact_local_revision_and_does_not_replace_existing_files(
    ) -> Result<()> {
        let root = tempfile::tempdir()?;
        let root_path = root.path().to_string_lossy().into_owned();
        let service =
            ugoite_iceberg::service::UgoiteService::new_without_background_refresh(&root_path)?;
        let owner = Uuid::from_u128(2_011);
        let space_id = service
            .create_space_for_principal("composition-export-exact", owner, "Owner")
            .await?
            .to_string();
        let first_document =
            ugoite_domain::composition::canonicalize_composition_yaml(METADATA_ONLY_COMPOSITION)
                .expect("shared Composition fixture parses")
                .document;
        let first = service
            .save_composition_authorized_for_principals(
                &space_id,
                ugoite_iceberg::composition::CompositionSaveRequest {
                    entry_id: None,
                    base_revision_id: None,
                    document: first_document.clone(),
                },
                "Owner",
                &[owner],
            )
            .await?;
        let mut second_document = first_document;
        second_document.name = "Updated expenses".to_string();
        let second = service
            .save_composition_authorized_for_principals(
                &space_id,
                ugoite_iceberg::composition::CompositionSaveRequest {
                    entry_id: Some(first.entry_id),
                    base_revision_id: Some(first.revision_id),
                    document: second_document,
                },
                "Owner",
                &[owner],
            )
            .await?;

        let target = SpaceTarget::Core {
            root: root_path,
            space_id,
        };
        let output = root.path().join("exact.ugcomp.yaml");
        let receipt = export_composition(
            &target,
            &first.entry_id.to_string(),
            Some(&first.revision_id.to_string()),
            &output,
        )
        .await?;

        assert_eq!(receipt.composition_id, first.entry_id.to_string());
        assert_eq!(receipt.revision_id, first.revision_id.to_string());
        assert_eq!(receipt.output, output.to_string_lossy());
        assert_eq!(std::fs::read(&output)?, first.canonical_yaml.as_bytes());
        assert_eq!(receipt.bytes, first.canonical_yaml.len());

        let latest_output = root.path().join("latest.ugcomp.yaml");
        let latest_receipt =
            export_composition(&target, &first.entry_id.to_string(), None, &latest_output).await?;
        assert_eq!(latest_receipt.revision_id, second.revision_id.to_string());
        assert_eq!(
            std::fs::read(&latest_output)?,
            second.canonical_yaml.as_bytes()
        );

        let missing_revision_id = Uuid::from_u128(2_012).to_string();
        let missing_output = root.path().join("missing.ugcomp.yaml");
        let error = export_composition(
            &target,
            &first.entry_id.to_string(),
            Some(&missing_revision_id),
            &missing_output,
        )
        .await
        .expect_err("a missing exact revision must not export the current revision");
        assert!(error
            .downcast_ref::<AppError>()
            .is_some_and(|error| error.code() == ErrorCode::EntryNotFound));
        assert!(!missing_output.exists());

        let error = write_raw_spec_to_path(
            &read_composition(&target, &first.entry_id.to_string(), None).await?,
            &output,
        )
        .expect_err("export must not replace an existing output file");
        assert!(error
            .to_string()
            .contains("failed to safely finalize Composition export"));
        assert_eq!(std::fs::read(&output)?, first.canonical_yaml.as_bytes());
        Ok(())
    }

    #[tokio::test]
    async fn export_remote_selects_exact_revision_and_preserves_unknown_format_yaml() -> Result<()>
    {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let raw_yaml = "format: ugoite.composition\nformat_version: 99\nkind: future\n";
        let response_body = json!({
            "revision": {
                "form_id": "form-1",
                "entry_id": "comp-1",
                "revision_id": "revision-99",
                "parent_revision_id": null,
                "entry_version": 4,
                "change_id": "change-4",
                "expected_version": 3,
                "operation": "upsert",
                "committed_at_micros": 40,
                "author_id": "owner",
                "form_version": 1,
                "source_kind": "core",
                "source_id": null,
                "entry": {
                    "external_id": "comp-1",
                    "tags": [],
                    "created_at_micros": 1,
                    "updated_at_micros": 40,
                    "updated_by": "owner",
                    "integrity": {"checksum": "", "signature": ""},
                    "deleted": false,
                    "deleted_at_micros": null,
                    "deleted_by": null,
                    "restored_from": null
                },
                "extra_attributes": {},
                "extension_metadata": {}
            },
            "fields": {"spec": raw_yaml},
            "unmapped_field_values": {}
        })
        .to_string();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept CLI request");
            let mut request = [0_u8; 4096];
            let bytes_read = stream.read(&mut request).await.expect("read CLI request");
            let request = String::from_utf8_lossy(&request[..bytes_read]).to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write CLI response");
            request
        });
        let target = SpaceTarget::Remote {
            base: format!("http://{address}"),
            space_uid: "demo".to_string(),
            connection: "test".to_string(),
            credential: None,
        };
        let output_root = tempfile::tempdir()?;
        let output = output_root.path().join("future.ugcomp.yaml");

        let receipt = export_composition(&target, "comp-1", Some("revision-99"), &output).await?;
        let request = server.await.expect("mock server completes");

        assert!(request
            .starts_with("GET /spaces/demo/compositions/comp-1/history/revision-99 HTTP/1.1\r\n"));
        assert_eq!(receipt.revision_id, "revision-99");
        assert_eq!(std::fs::read(&output)?, raw_yaml.as_bytes());
        Ok(())
    }

    #[tokio::test]
    async fn list_reads_bounded_local_pages_without_loading_specs() -> Result<()> {
        let root = tempfile::tempdir()?;
        let root_path = root.path().to_string_lossy().into_owned();
        let service =
            ugoite_iceberg::service::UgoiteService::new_without_background_refresh(&root_path)?;
        let owner = Uuid::from_u128(2_101);
        let space_id = service
            .create_space_for_principal("composition-list-cli", owner, "Owner")
            .await?
            .to_string();
        let document =
            ugoite_domain::composition::canonicalize_composition_yaml(METADATA_ONLY_COMPOSITION)
                .expect("shared Composition fixture parses")
                .document;

        for _ in 0..3 {
            service
                .save_composition_authorized_for_principals(
                    &space_id,
                    ugoite_iceberg::composition::CompositionSaveRequest {
                        entry_id: None,
                        base_revision_id: None,
                        document: document.clone(),
                    },
                    "Owner",
                    &[owner],
                )
                .await?;
        }

        let target = SpaceTarget::Core {
            root: root_path,
            space_id,
        };
        let first = list_compositions(&target, 2, 0).await?;
        let second = list_compositions(&target, 2, 2).await?;

        assert_eq!(first.items.len(), 2);
        assert_eq!(first.offset, 0);
        assert_eq!(first.limit, 2);
        assert!(first.has_more);
        assert_eq!(second.items.len(), 1);
        assert_eq!(second.offset, 2);
        assert_eq!(second.limit, 2);
        assert!(!second.has_more);
        assert_ne!(first.items[0].composition_id, first.items[1].composition_id);
        assert_eq!(first.items[0].name, Some(json!("Metadata only")));
        assert_eq!(first.items[0].kind, Some(json!("dashboard")));
        assert_eq!(first.items[0].format_version, Some(json!(1)));

        let serialized = serde_json::to_value(&first)?;
        assert!(serialized["items"][0].get("spec").is_none());
        assert!(serialized["items"][0].get("tags").is_none());
        assert_eq!(
            serialized["items"][0]["composition_id"],
            first.items[0].composition_id
        );
        Ok(())
    }

    #[test]
    fn list_prepares_a_bounded_page_request_through_the_portable_protocol() {
        let request = prepare_request(
            "composition.list",
            &composition_list_arguments("demo", 25, 50),
            None,
        )
        .expect("Composition list request");

        assert_eq!(request.method, HttpMethod::Get);
        assert_eq!(request.body_kind, RequestBodyKind::None);
        assert_eq!(request.body, None);
        assert_eq!(request.path, "/spaces/demo/compositions?limit=25&offset=50");
    }

    #[tokio::test]
    async fn list_remote_request_decodes_and_returns_the_portable_page() -> Result<()> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let page_json = json!({
            "items": [{
                "composition_id": "entry-1",
                "revision_id": "revision-2",
                "updated_at": 12.5,
                "name": "Quarterly report",
                "kind": "dashboard",
                "format_version": 1
            }],
            "offset": 50,
            "limit": 25,
            "has_more": true
        });
        let response_body = page_json.to_string();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept CLI request");
            let mut request = [0_u8; 4096];
            let bytes_read = stream.read(&mut request).await.expect("read CLI request");
            let request = String::from_utf8_lossy(&request[..bytes_read]).to_string();
            let body = response_body;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write CLI response");
            request
        });

        let target = SpaceTarget::Remote {
            base: format!("http://{address}"),
            space_uid: "demo".to_string(),
            connection: "test".to_string(),
            credential: None,
        };
        let page = list_compositions(&target, 25, 50).await?;
        let request = server.await.expect("mock server completes");

        assert!(
            request.starts_with("GET /spaces/demo/compositions?limit=25&offset=50 HTTP/1.1\r\n")
        );
        assert_eq!(
            page,
            serde_json::from_value::<CompositionListPage>(page_json.clone())?
        );
        assert_eq!(page.items[0].name, Some(json!("Quarterly report")));
        assert!(serde_json::to_value(&page)?["items"][0]
            .get("spec")
            .is_none());
        assert!(serde_json::to_value(&page)?["items"][0]
            .get("tags")
            .is_none());
        Ok(())
    }

    #[tokio::test]
    async fn history_reads_bounded_local_pages_with_raw_revision_data() -> Result<()> {
        let root = tempfile::tempdir()?;
        let root_path = root.path().to_string_lossy().into_owned();
        let service =
            ugoite_iceberg::service::UgoiteService::new_without_background_refresh(&root_path)?;
        let owner = Uuid::from_u128(2_201);
        let space_id = service
            .create_space_for_principal("composition-history-cli", owner, "Owner")
            .await?
            .to_string();
        let mut document =
            ugoite_domain::composition::canonicalize_composition_yaml(METADATA_ONLY_COMPOSITION)
                .expect("shared Composition fixture parses")
                .document;
        document.name = "First version".to_string();
        let expected_first_spec = ugoite_domain::composition::canonicalize_composition(&document)
            .expect("Composition document canonicalizes")
            .yaml;
        let first = service
            .save_composition_authorized_for_principals(
                &space_id,
                ugoite_iceberg::composition::CompositionSaveRequest {
                    entry_id: None,
                    base_revision_id: None,
                    document: document.clone(),
                },
                "Owner",
                &[owner],
            )
            .await?;
        document.name = "Second version".to_string();
        let second = service
            .save_composition_authorized_for_principals(
                &space_id,
                ugoite_iceberg::composition::CompositionSaveRequest {
                    entry_id: Some(first.entry_id),
                    base_revision_id: Some(first.revision_id),
                    document,
                },
                "Owner",
                &[owner],
            )
            .await?;
        let target = SpaceTarget::Core {
            root: root_path,
            space_id,
        };

        let first_page =
            read_composition_history(&target, &first.entry_id.to_string(), 1, 0).await?;
        let second_page =
            read_composition_history(&target, &first.entry_id.to_string(), 1, 1).await?;

        assert_eq!(first_page.entry_id, first.entry_id.to_string());
        assert_eq!(first_page.total, 2);
        assert_eq!(first_page.offset, 0);
        assert_eq!(first_page.limit, 1);
        assert!(first_page.has_more);
        assert_eq!(first_page.revisions.len(), 1);
        assert_eq!(second_page.total, 2);
        assert_eq!(second_page.offset, 1);
        assert_eq!(second_page.limit, 1);
        assert!(!second_page.has_more);
        assert_eq!(second_page.revisions.len(), 1);
        let returned_revisions = [
            first_page.revisions[0].revision.revision_id.clone(),
            second_page.revisions[0].revision.revision_id.clone(),
        ];
        assert!(returned_revisions.contains(&first.revision_id.to_string()));
        assert!(returned_revisions.contains(&second.revision_id.to_string()));
        assert_eq!(first_page.revisions[0].fields["spec"], expected_first_spec);
        assert!(serde_json::to_value(&first_page)?["revisions"][0]["fields"]
            .get("spec")
            .is_some());
        Ok(())
    }

    #[test]
    fn history_prepares_a_bounded_page_request_through_the_portable_protocol() {
        let request = prepare_request(
            "composition.history",
            &composition_history_arguments("demo", "comp-1", 25, 50),
            None,
        )
        .expect("Composition history request");

        assert_eq!(request.method, HttpMethod::Get);
        assert_eq!(request.body_kind, RequestBodyKind::None);
        assert_eq!(request.body, None);
        assert_eq!(
            request.path,
            "/spaces/demo/compositions/comp-1/history?limit=25&offset=50"
        );
    }

    #[tokio::test]
    async fn history_remote_request_decodes_and_returns_the_portable_page() -> Result<()> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let page_json = json!({
            "entry_id": "entry-1",
            "revisions": [{
                "revision": {
                    "form_id": "form-1",
                    "entry_id": "entry-1",
                    "revision_id": "revision-2",
                    "parent_revision_id": "revision-1",
                    "entry_version": 2,
                    "change_id": "change-2",
                    "expected_version": 1,
                    "operation": "upsert",
                    "committed_at_micros": 12,
                    "author_id": "owner",
                    "form_version": 1,
                    "source_kind": "core",
                    "source_id": null,
                    "entry": {
                        "external_id": "entry-1",
                        "tags": ["finance"],
                        "created_at_micros": 1,
                        "updated_at_micros": 12,
                        "updated_by": "owner",
                        "integrity": {"checksum": "", "signature": ""},
                        "deleted": false,
                        "deleted_at_micros": null,
                        "deleted_by": null,
                        "restored_from": null
                    },
                    "extra_attributes": {},
                    "extension_metadata": {}
                },
                "fields": {
                    "name": "Quarterly report",
                    "spec": "format_version: 1\nname: Quarterly report"
                },
                "unmapped_field_values": {}
            }],
            "total": 2,
            "offset": 50,
            "limit": 25,
            "has_more": true
        });
        let response_body = page_json.to_string();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept CLI request");
            let mut request = [0_u8; 4096];
            let bytes_read = stream.read(&mut request).await.expect("read CLI request");
            let request = String::from_utf8_lossy(&request[..bytes_read]).to_string();
            let body = response_body;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write CLI response");
            request
        });

        let target = SpaceTarget::Remote {
            base: format!("http://{address}"),
            space_uid: "demo".to_string(),
            connection: "test".to_string(),
            credential: None,
        };
        let page = read_composition_history(&target, "comp-1", 25, 50).await?;
        let request = server.await.expect("mock server completes");

        assert!(request.starts_with(
            "GET /spaces/demo/compositions/comp-1/history?limit=25&offset=50 HTTP/1.1\r\n"
        ));
        assert_eq!(
            page,
            serde_json::from_value::<CompositionHistoryPage>(page_json.clone())?
        );
        assert_eq!(page.entry_id, "entry-1");
        assert_eq!(page.revisions[0].fields["name"], "Quarterly report");
        assert_eq!(
            page.revisions[0].fields["spec"],
            "format_version: 1\nname: Quarterly report"
        );
        Ok(())
    }
}
