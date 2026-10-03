use crate::cli_config::{resolve_command_target, SpaceTarget};
use crate::http;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use serde::Serialize;
use serde_json::{json, Number, Value};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use ugoite_api_client::{
    prepare_request, CompositionDiagnosticCode as ApiCompositionDiagnosticCode,
    CompositionEntryIntegrity, CompositionEntryMetadata, CompositionHistoryPage,
    CompositionLintError, CompositionLintResponse, CompositionLintValue, CompositionListItem,
    CompositionListPage, CompositionPublicationReceipt, CompositionRawRevision,
    CompositionResolvePlan, CompositionResolveResponse, CompositionResolvedSource,
    CompositionRestoreRequest, CompositionRestoreResponse, CompositionRevisionMetadata,
};
use ugoite_core::composition::{
    evaluate_entry_metric_page, evaluate_saved_sql_metric_page, ResolvedComponentKind,
    ResolvedCompositionPlan, ResolvedSourceRequest,
};
use ugoite_core::entry_query::EntryPage;
use ugoite_core::sql_query::SqlQueryPage;
use ugoite_domain::composition::{
    canonicalize_composition_yaml, parse_composition_yaml, CompositionDiagnosticCode,
    CompositionParameter, CompositionParameterType as DomainCompositionParameterType,
    MAX_COMPOSITION_YAML_BYTES,
};
use ugoite_iceberg::service::UgoiteService;

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
    /// Resolve and execute one bounded page for each Composition source
    Query {
        #[arg(value_name = "COMPOSITION_ID")]
        composition_id: String,
        #[arg(long, value_name = "REVISION_ID")]
        revision: Option<String>,
        #[arg(long = "param", value_name = "KEY=VALUE", action = clap::ArgAction::Append)]
        parameters: Vec<String>,
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
        CompositionSubCmd::Query {
            composition_id,
            revision,
            parameters,
        } => {
            let target =
                resolve_command_target(explicit_config, context_override, "composition query")?;
            query_composition(&target, &composition_id, revision.as_deref(), &parameters).await?;
        }
    }
    Ok(())
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
                tags: item.tags,
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

#[derive(Serialize)]
struct CompositionQueryOutput {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    plan: Option<CompositionResolvePlan>,
    sources: Vec<CompositionQuerySourceOutput>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    diagnostics: Vec<CompositionQueryDiagnostic>,
}

#[derive(Serialize)]
struct CompositionQuerySourceOutput {
    source_id: String,
    kind: &'static str,
    page: Value,
    metrics: Vec<CompositionQueryMetricOutput>,
}

#[derive(Serialize)]
struct CompositionQueryMetricOutput {
    component_id: String,
    value: Value,
}

#[derive(Clone, Debug, Serialize)]
struct CompositionQueryDiagnostic {
    code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parameter_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    component_id: Option<String>,
}

struct CompositionQueryPlan {
    portable: CompositionResolvePlan,
    core: ResolvedCompositionPlan,
}

enum CompositionQueryResolution {
    Plan(CompositionQueryPlan),
    Diagnostics(Vec<CompositionQueryDiagnostic>),
}

async fn query_composition(
    target: &SpaceTarget,
    composition_id: &str,
    requested_revision: Option<&str>,
    raw_parameters: &[String],
) -> Result<()> {
    let composition = read_composition(target, composition_id, requested_revision).await?;
    let parameter_values = match composition.fields.get("spec").and_then(Value::as_str) {
        Some(yaml) => match parse_composition_yaml(yaml) {
            Ok(document) => parse_cli_parameter_values(&document.spec.parameters, raw_parameters)?,
            Err(_) => parse_cli_parameter_values(&[], raw_parameters)?,
        },
        None => parse_cli_parameter_values(&[], raw_parameters)?,
    };
    let local_service = match target {
        SpaceTarget::Core { root, .. } => {
            Some(UgoiteService::new_without_background_refresh(root)?)
        }
        SpaceTarget::Remote { .. } => None,
    };

    let resolution = resolve_composition(
        target,
        target_space_id(target),
        composition_id,
        &composition.revision.revision_id,
        parameter_values,
        local_service.as_ref(),
    )
    .await?;
    let plan = match resolution {
        CompositionQueryResolution::Plan(plan) => plan,
        CompositionQueryResolution::Diagnostics(diagnostics) => {
            crate::output::print_json(&CompositionQueryOutput {
                ok: false,
                plan: None,
                sources: Vec::new(),
                diagnostics,
            });
            return Ok(());
        }
    };

    anyhow::ensure!(
        plan.portable.sources.len() == plan.core.sources.len(),
        "portable and Core Composition source counts diverged"
    );

    let mut sources = Vec::with_capacity(plan.portable.sources.len());
    let mut diagnostics = Vec::new();
    for (portable_source, core_source) in plan.portable.sources.iter().zip(plan.core.sources.iter())
    {
        let (source, source_diagnostics) = execute_composition_source(
            target,
            portable_source,
            core_source,
            &plan.core,
            local_service.as_ref(),
        )
        .await?;
        sources.push(source);
        diagnostics.extend(source_diagnostics);
    }

    crate::output::print_json(&CompositionQueryOutput {
        ok: diagnostics.is_empty(),
        plan: Some(plan.portable),
        sources,
        diagnostics,
    });
    Ok(())
}

async fn resolve_composition(
    target: &SpaceTarget,
    space_id: &str,
    composition_id: &str,
    revision_id: &str,
    parameters: BTreeMap<String, Value>,
    local_service: Option<&UgoiteService>,
) -> Result<CompositionQueryResolution> {
    match target {
        SpaceTarget::Core { .. } => {
            let service = local_service.context("local Composition service is unavailable")?;
            let resolution = service
                .resolve_composition_local(space_id, composition_id, revision_id, &parameters)
                .await?;
            if let Some(plan) = resolution.plan {
                Ok(CompositionQueryResolution::Plan(
                    composition_query_plan_from_core(plan)?,
                ))
            } else {
                Ok(CompositionQueryResolution::Diagnostics(
                    resolution
                        .diagnostics
                        .iter()
                        .map(|diagnostic| CompositionQueryDiagnostic {
                            code: diagnostic.code.as_str().to_string(),
                            parameter_id: diagnostic.parameter_id.clone(),
                            source_id: None,
                            component_id: None,
                        })
                        .collect(),
                ))
            }
        }
        SpaceTarget::Remote { .. } => {
            let arguments = json!({
                "space_id": space_id,
                "composition_id": composition_id,
            });
            let body = composition_resolve_body(revision_id, parameters);
            let response =
                http::execute_for_target(target, "composition.resolve", arguments, Some(body))
                    .await?;
            let response: CompositionResolveResponse =
                serde_json::from_value(response).context("decode Composition resolve response")?;
            if response.ok {
                let plan = response
                    .plan
                    .context("Composition resolve succeeded without a plan")?;
                Ok(CompositionQueryResolution::Plan(
                    composition_query_plan_from_portable(plan)?,
                ))
            } else {
                let diagnostics = response
                    .diagnostics
                    .iter()
                    .map(|diagnostic| {
                        Ok(CompositionQueryDiagnostic {
                            code: serde_json::to_value(diagnostic.code)?
                                .as_str()
                                .context("Composition diagnostic code must serialize as text")?
                                .to_string(),
                            parameter_id: diagnostic.parameter_id.clone(),
                            source_id: None,
                            component_id: None,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(CompositionQueryResolution::Diagnostics(diagnostics))
            }
        }
    }
}

fn composition_query_plan_from_core(plan: ResolvedCompositionPlan) -> Result<CompositionQueryPlan> {
    let portable = serde_json::from_value(serde_json::to_value(&plan)?)
        .context("encode local Composition resolve plan through the portable DTO")?;
    Ok(CompositionQueryPlan {
        portable,
        core: plan,
    })
}

fn composition_query_plan_from_portable(
    plan: CompositionResolvePlan,
) -> Result<CompositionQueryPlan> {
    let core = serde_json::from_value(serde_json::to_value(&plan)?)
        .context("decode portable Composition resolve plan for shared Core evaluation")?;
    Ok(CompositionQueryPlan {
        portable: plan,
        core,
    })
}

fn composition_resolve_body(revision_id: &str, parameters: BTreeMap<String, Value>) -> Value {
    json!({
        "revision_id": revision_id,
        "parameters": parameters,
    })
}

fn parse_cli_parameter_values(
    definitions: &[CompositionParameter],
    assignments: &[String],
) -> Result<BTreeMap<String, Value>> {
    let mut values = BTreeMap::new();
    for assignment in assignments {
        let (key, raw_value) = assignment.split_once('=').ok_or_else(|| {
            crate::output::UsageError(format!("--param must be KEY=VALUE, got {assignment:?}"))
        })?;
        if key.is_empty() {
            return Err(
                crate::output::UsageError("--param key must not be empty".to_string()).into(),
            );
        }
        let definition = definitions.iter().find(|definition| definition.id == key);
        let value = definition.map_or_else(
            || Value::String(raw_value.to_string()),
            |definition| parse_cli_parameter_value(definition.parameter_type, raw_value),
        );
        if values.insert(key.to_string(), value).is_some() {
            return Err(crate::output::UsageError(format!(
                "Composition parameter {key:?} was supplied more than once"
            ))
            .into());
        }
    }
    Ok(values)
}

fn parse_cli_parameter_value(
    parameter_type: DomainCompositionParameterType,
    raw_value: &str,
) -> Value {
    match parameter_type {
        DomainCompositionParameterType::String
        | DomainCompositionParameterType::Date
        | DomainCompositionParameterType::Timestamp => Value::String(raw_value.to_string()),
        DomainCompositionParameterType::Boolean => raw_value
            .parse::<bool>()
            .map(Value::Bool)
            .unwrap_or_else(|_| Value::String(raw_value.to_string())),
        DomainCompositionParameterType::Integer => raw_value
            .parse::<i64>()
            .map(Number::from)
            .map(Value::Number)
            .unwrap_or_else(|_| Value::String(raw_value.to_string())),
        DomainCompositionParameterType::Float => raw_value
            .parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(raw_value.to_string())),
    }
}

async fn execute_composition_source(
    target: &SpaceTarget,
    portable_source: &CompositionResolvedSource,
    core_source: &ResolvedSourceRequest,
    plan: &ResolvedCompositionPlan,
    local_service: Option<&UgoiteService>,
) -> Result<(
    CompositionQuerySourceOutput,
    Vec<CompositionQueryDiagnostic>,
)> {
    let space_id = target_space_id(target);
    let (source_id, kind, operation, request) = composition_source_dispatch(portable_source);
    let arguments = json!({"space_id": space_id});
    let core_source_id = match core_source {
        ResolvedSourceRequest::EntryQuery { source_id, .. }
        | ResolvedSourceRequest::SavedSql { source_id, .. } => source_id,
    };
    anyhow::ensure!(
        source_id == core_source_id,
        "portable and Core Composition source order diverged"
    );

    let (page, metrics, diagnostics) = match (target, portable_source, core_source) {
        (
            SpaceTarget::Remote { .. },
            CompositionResolvedSource::EntryQuery { .. },
            ResolvedSourceRequest::EntryQuery { source_id, .. },
        ) => {
            let page_value =
                http::execute_for_target(target, operation, arguments, Some(request.clone()))
                    .await?;
            let page: EntryPage = serde_json::from_value(page_value.clone())
                .context("entry.query returned an invalid Composition page")?;
            let (metrics, diagnostics) = evaluate_entry_metrics(plan, source_id, &page);
            (page_value, metrics, diagnostics)
        }
        (
            SpaceTarget::Remote { .. },
            CompositionResolvedSource::SavedSql { .. },
            ResolvedSourceRequest::SavedSql { source_id, .. },
        ) => {
            let page_value =
                http::execute_for_target(target, operation, arguments, Some(request.clone()))
                    .await?;
            let page: SqlQueryPage = serde_json::from_value(page_value.clone())
                .context("sql.query returned an invalid Composition page")?;
            let (metrics, diagnostics) = evaluate_saved_sql_metrics(plan, source_id, &page);
            (page_value, metrics, diagnostics)
        }
        (
            SpaceTarget::Core { .. },
            CompositionResolvedSource::EntryQuery { .. },
            ResolvedSourceRequest::EntryQuery {
                source_id, request, ..
            },
        ) => {
            let page = local_service
                .context("local Composition query service is unavailable")?
                .query_entry_page(space_id, request.clone())
                .await?;
            let page_value = serde_json::to_value(&page)?;
            let (metrics, diagnostics) = evaluate_entry_metrics(plan, source_id, &page);
            (page_value, metrics, diagnostics)
        }
        (
            SpaceTarget::Core { .. },
            CompositionResolvedSource::SavedSql { .. },
            ResolvedSourceRequest::SavedSql {
                source_id, request, ..
            },
        ) => {
            let page = local_service
                .context("local Composition query service is unavailable")?
                .query_sql(space_id, request.clone())
                .await?;
            let page_value = serde_json::to_value(&page)?;
            let (metrics, diagnostics) = evaluate_saved_sql_metrics(plan, source_id, &page);
            (page_value, metrics, diagnostics)
        }
        _ => anyhow::bail!("portable and Core Composition source kinds diverged"),
    };

    Ok((
        CompositionQuerySourceOutput {
            source_id: source_id.to_string(),
            kind,
            page,
            metrics,
        },
        diagnostics,
    ))
}

fn evaluate_entry_metrics(
    plan: &ResolvedCompositionPlan,
    source_id: &str,
    page: &EntryPage,
) -> (
    Vec<CompositionQueryMetricOutput>,
    Vec<CompositionQueryDiagnostic>,
) {
    let mut metrics = Vec::new();
    let mut diagnostics = Vec::new();
    for binding in plan.component_bindings.iter().filter(|binding| {
        binding.source_id == source_id && binding.kind == ResolvedComponentKind::Metric
    }) {
        match evaluate_entry_metric_page(binding, page) {
            Ok(value) => metrics.push(CompositionQueryMetricOutput {
                component_id: binding.component_id.clone(),
                value,
            }),
            Err(diagnostic) => diagnostics.push(CompositionQueryDiagnostic {
                code: diagnostic.code.as_str().to_string(),
                parameter_id: None,
                source_id: Some(source_id.to_string()),
                component_id: Some(binding.component_id.clone()),
            }),
        }
    }
    (metrics, diagnostics)
}

fn evaluate_saved_sql_metrics(
    plan: &ResolvedCompositionPlan,
    source_id: &str,
    page: &SqlQueryPage,
) -> (
    Vec<CompositionQueryMetricOutput>,
    Vec<CompositionQueryDiagnostic>,
) {
    let mut metrics = Vec::new();
    let mut diagnostics = Vec::new();
    for binding in plan.component_bindings.iter().filter(|binding| {
        binding.source_id == source_id && binding.kind == ResolvedComponentKind::Metric
    }) {
        match evaluate_saved_sql_metric_page(binding, page) {
            Ok(value) => metrics.push(CompositionQueryMetricOutput {
                component_id: binding.component_id.clone(),
                value,
            }),
            Err(diagnostic) => diagnostics.push(CompositionQueryDiagnostic {
                code: diagnostic.code.as_str().to_string(),
                parameter_id: None,
                source_id: Some(source_id.to_string()),
                component_id: Some(binding.component_id.clone()),
            }),
        }
    }
    (metrics, diagnostics)
}

fn composition_source_dispatch(
    source: &CompositionResolvedSource,
) -> (&str, &'static str, &'static str, &Value) {
    match source {
        CompositionResolvedSource::EntryQuery {
            source_id, request, ..
        } => (source_id, "entry_query", "entry.query", request),
        CompositionResolvedSource::SavedSql {
            source_id, request, ..
        } => (source_id, "saved_sql", "sql.query", request),
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
        composition_query_plan_from_core, composition_query_plan_from_portable,
        composition_resolve_body, composition_source_dispatch, evaluate_entry_metrics,
        evaluate_saved_sql_metrics, lint_file, lint_yaml_bytes, list_compositions,
        local_raw_revision_to_api, parse_cli_parameter_value, parse_cli_parameter_values,
        raw_spec_output, read_composition, read_composition_history, CompositionParameter,
        DomainCompositionParameterType, RawSpecOutput,
    };
    use crate::cli_config::SpaceTarget;
    use anyhow::Result;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    use std::io::Write;
    use ugoite_api_client::{
        prepare_request, CompositionDiagnosticCode, CompositionHistoryPage, CompositionListPage,
        CompositionResolvePlan, CompositionResolvedSource, HttpMethod, RequestBodyKind,
    };
    use ugoite_core::error::{AppError, ErrorCode};
    use ugoite_core::{
        composition::ResolvedCompositionPlan, entry_query::EntryPage, sql_query::SqlQueryPage,
    };
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
        let response = lint_yaml_bytes(
            b"format: ugoite.composition\nformat_version: 22\nkind: dashboard\nname: Future\ntags: []\nspec: {}\n",
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

    #[test]
    fn query_cli_parameters_follow_the_shared_domain_types() {
        let definitions = vec![
            CompositionParameter {
                id: "enabled".to_string(),
                label: None,
                parameter_type: DomainCompositionParameterType::Boolean,
                required: true,
                default: None,
                format: None,
            },
            CompositionParameter {
                id: "count".to_string(),
                label: None,
                parameter_type: DomainCompositionParameterType::Integer,
                required: true,
                default: None,
                format: None,
            },
            CompositionParameter {
                id: "ratio".to_string(),
                label: None,
                parameter_type: DomainCompositionParameterType::Float,
                required: true,
                default: None,
                format: None,
            },
            CompositionParameter {
                id: "date".to_string(),
                label: None,
                parameter_type: DomainCompositionParameterType::Date,
                required: true,
                default: None,
                format: None,
            },
        ];
        let values = parse_cli_parameter_values(
            &definitions,
            &[
                "enabled=true".to_string(),
                "count=12".to_string(),
                "ratio=1.5".to_string(),
                "date=2026-10-01".to_string(),
                "custom=value".to_string(),
            ],
        )
        .expect("typed CLI values");

        assert_eq!(values["enabled"], json!(true));
        assert_eq!(values["count"], json!(12));
        assert_eq!(values["ratio"], json!(1.5));
        assert_eq!(values["date"], json!("2026-10-01"));
        assert_eq!(values["custom"], json!("value"));
    }

    #[test]
    fn invalid_typed_cli_values_are_left_for_the_shared_resolver_diagnostic() {
        assert_eq!(
            parse_cli_parameter_value(DomainCompositionParameterType::Integer, "12x"),
            json!("12x")
        );
        assert_eq!(
            parse_cli_parameter_value(DomainCompositionParameterType::Boolean, "yes"),
            json!("yes")
        );
    }

    #[test]
    fn query_cli_parameter_assignments_reject_malformed_or_duplicate_names() {
        assert!(parse_cli_parameter_values(&[], &["month".to_string()]).is_err());
        assert!(parse_cli_parameter_values(
            &[],
            &["month=2026-10".to_string(), "month=2026-11".to_string()]
        )
        .is_err());
    }

    #[test]
    fn query_resolve_body_keeps_the_exact_revision_and_typed_parameters() {
        let body = composition_resolve_body(
            "revision-1",
            BTreeMap::from([
                ("month".to_string(), json!("2026-10")),
                ("include_archived".to_string(), json!(true)),
            ]),
        );
        assert_eq!(
            body,
            json!({
                "revision_id": "revision-1",
                "parameters": {
                    "month": "2026-10",
                    "include_archived": true
                }
            })
        );

        let request = prepare_request(
            "composition.resolve",
            &json!({"space_id":"demo", "composition_id":"composition-1"}),
            Some(&body),
        )
        .expect("exact resolve operation");
        assert_eq!(
            request.path,
            "/spaces/demo/compositions/composition-1/resolve"
        );
    }

    #[test]
    fn query_dispatches_each_resolved_request_unchanged_to_existing_operations() {
        let entry_request = json!({
            "query": {"scope":{"kind":"all"}},
            "projection": {"kind":"preview"},
            "limit": 20
        });
        let entry_source = CompositionResolvedSource::EntryQuery {
            source_id: "entries".to_string(),
            request: entry_request.clone(),
            source_schema_fingerprint: "a".repeat(64),
        };
        let (source_id, kind, operation, request) = composition_source_dispatch(&entry_source);
        assert_eq!(source_id, "entries");
        assert_eq!(kind, "entry_query");
        assert_eq!(operation, "entry.query");
        assert_eq!(request, &entry_request);

        let sql_request = json!({
            "sql": "SELECT * FROM entries",
            "limit": 1,
            "saved_sql": {"id":"sql-1", "revision_id":"revision-2"}
        });
        let sql_source = CompositionResolvedSource::SavedSql {
            source_id: "summary".to_string(),
            request: sql_request.clone(),
            source_schema_fingerprint: "b".repeat(64),
        };
        let (source_id, kind, operation, request) = composition_source_dispatch(&sql_source);
        assert_eq!(source_id, "summary");
        assert_eq!(kind, "saved_sql");
        assert_eq!(operation, "sql.query");
        assert_eq!(request, &sql_request);
    }

    #[test]
    fn portable_resolve_plan_round_trips_through_core_with_exact_sql_revision_and_metric_type() {
        let portable: CompositionResolvePlan = serde_json::from_value(json!({
            "composition_revision": {
                "entry_id": "00000000-0000-0000-0000-000000000001",
                "revision_id": "00000000-0000-0000-0000-000000000002"
            },
            "sources": [{
                "kind": "saved_sql",
                "source_id": "summary",
                "request": {
                    "sql": "SELECT 42 AS total",
                    "parameters": {},
                    "parameter_types": {},
                    "limit": 1,
                    "saved_sql": {
                        "id": "sql-entry-1",
                        "revision_id": "sql-revision-7"
                    }
                },
                "source_schema_fingerprint": "abc"
            }],
            "component_bindings": [{
                "component_id": "total",
                "kind": "metric",
                "source_id": "summary",
                "result_property_key": "total",
                "expected_result_type": "integer"
            }]
        }))
        .expect("portable resolve plan");

        let resolved = composition_query_plan_from_portable(portable.clone())
            .expect("portable plan adapts to the shared Core types");
        assert_eq!(resolved.portable, portable);
        assert_eq!(
            resolved.portable.component_bindings[0].expected_result_type,
            Some(ugoite_api_client::CompositionResultFieldType::Integer)
        );
        let ugoite_core::composition::ResolvedSourceRequest::SavedSql { request, .. } =
            &resolved.core.sources[0]
        else {
            panic!("expected the Saved SQL source")
        };
        let saved_sql = request
            .saved_sql
            .as_ref()
            .expect("exact Saved SQL selector");
        assert_eq!(saved_sql.id, "sql-entry-1");
        assert_eq!(saved_sql.revision_id, "sql-revision-7");

        let local_plan = composition_query_plan_from_core(resolved.core.clone())
            .expect("local plan adapts to the portable DTO");
        assert_eq!(local_plan.portable, resolved.portable);
    }

    #[test]
    fn cli_metric_output_uses_the_shared_core_page_evaluator() {
        let plan: ResolvedCompositionPlan = serde_json::from_value(json!({
            "composition_revision": {
                "entry_id": "00000000-0000-0000-0000-000000000001",
                "revision_id": "00000000-0000-0000-0000-000000000002"
            },
            "sources": [],
            "component_bindings": [{
                "component_id": "total",
                "kind": "metric",
                "source_id": "entries",
                "metric_field_id": 101,
                "result_property_key": "amount",
                "expected_result_type": "integer"
            }]
        }))
        .expect("Core plan with a typed metric binding");
        let page: EntryPage = serde_json::from_value(json!({
            "rows": [{
                "id": "00000000-0000-0000-0000-00000000002a",
                "form_id": "00000000-0000-0000-0000-00000000002c",
                "revision_id": "00000000-0000-0000-0000-00000000002b",
                "created_at_micros": 0,
                "updated_at_micros": 0,
                "properties": {"amount": 42}
            }],
            "has_more": false
        }))
        .expect("one bounded Entry page");

        let (metrics, diagnostics) = evaluate_entry_metrics(&plan, "entries", &page);
        assert!(diagnostics.is_empty());
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].component_id, "total");
        assert_eq!(metrics[0].value, json!(42));

        let incomplete_page: EntryPage = serde_json::from_value(json!({
            "rows": [{
                "id": "00000000-0000-0000-0000-00000000002a",
                "form_id": "00000000-0000-0000-0000-00000000002c",
                "revision_id": "00000000-0000-0000-0000-00000000002b",
                "created_at_micros": 0,
                "updated_at_micros": 0,
                "properties": {"amount": 42}
            }],
            "has_more": true,
            "next": "continuation-2"
        }))
        .expect("bounded page with continuation");
        let (metrics, diagnostics) = evaluate_entry_metrics(&plan, "entries", &incomplete_page);
        assert!(metrics.is_empty());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].code,
            ugoite_domain::composition::CompositionDiagnosticCode::MetricResultPageIncomplete
                .as_str()
        );
        assert_eq!(diagnostics[0].component_id.as_deref(), Some("total"));
    }

    #[test]
    fn cli_saved_sql_metric_output_uses_the_shared_core_page_evaluator() {
        let plan: ResolvedCompositionPlan = serde_json::from_value(json!({
            "composition_revision": {
                "entry_id": "00000000-0000-0000-0000-000000000001",
                "revision_id": "00000000-0000-0000-0000-000000000002"
            },
            "sources": [],
            "component_bindings": [{
                "component_id": "total",
                "kind": "metric",
                "source_id": "summary",
                "result_property_key": "total",
                "expected_result_type": "float"
            }]
        }))
        .expect("Core plan with a typed Saved SQL metric binding");
        let page: SqlQueryPage = serde_json::from_value(json!({
            "columns": ["total"],
            "rows": [{"total": 42.5}],
            "has_more": false
        }))
        .expect("one bounded SQL page");

        let (metrics, diagnostics) = evaluate_saved_sql_metrics(&plan, "summary", &page);
        assert!(diagnostics.is_empty());
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].component_id, "total");
        assert_eq!(metrics[0].value, json!(42.5));
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
        let document = ugoite_domain::composition::canonicalize_composition_yaml(MONTHLY_EXPENSE)
            .expect("shared Composition fixture parses")
            .document;

        for index in 0..3 {
            let mut tagged_document = document.clone();
            tagged_document.tags = vec![format!("item-{index}")];
            service
                .save_composition_authorized_for_principals(
                    &space_id,
                    ugoite_iceberg::composition::CompositionSaveRequest {
                        entry_id: None,
                        base_revision_id: None,
                        document: tagged_document,
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
        assert_eq!(first.items[0].name, Some(json!("Monthly expenses")));
        assert_eq!(first.items[0].kind, Some(json!("dashboard")));
        assert_eq!(first.items[0].format_version, Some(json!(1)));
        assert!(first.items[0].tags[0].starts_with("item-"));

        let serialized = serde_json::to_value(&first)?;
        assert!(serialized["items"][0].get("spec").is_none());
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
                "format_version": 1,
                "tags": ["finance"]
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
                body.len(), body
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
            ugoite_domain::composition::canonicalize_composition_yaml(MONTHLY_EXPENSE)
                .expect("shared Composition fixture parses")
                .document;
        document.tags = vec!["first".to_string()];
        let expected_first_spec = ugoite_domain::composition::canonicalize_composition(&document)
            .expect("tagged Composition document canonicalizes")
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
        document.tags = vec!["second".to_string()];
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
                body.len(), body
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
