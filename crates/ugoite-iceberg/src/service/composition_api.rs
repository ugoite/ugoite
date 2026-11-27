//! Shared projection of Composition service results into portable API DTOs.

use super::{CompositionGraphEvaluationPlan, CompositionQueryEvaluationResponse};
use anyhow::{anyhow, Context, Result};
use std::collections::BTreeMap;
use ugoite_api_client::{
    CompositionComponentBinding, CompositionComponentKind, CompositionDeferredArgument,
    CompositionDependencyIndex, CompositionDiagnostic, CompositionEntryIntegrity,
    CompositionEntryMetadata, CompositionEvaluationQueryDiagnostic,
    CompositionEvaluationRequestDiagnostic, CompositionEvaluationResponse,
    CompositionEvaluationResultContractDiagnostic, CompositionEvaluationResultDiagnostic,
    CompositionFormBinding, CompositionGraphQueryResult, CompositionHistoryPage,
    CompositionLintError, CompositionLintResponse, CompositionLintValue, CompositionListItem,
    CompositionListPage, CompositionQuery, CompositionQueryResultType, CompositionRawRevision,
    CompositionResultColumn, CompositionRevisionMetadata, CompositionRevisionReference,
    CompositionScalarCardinality, CompositionScalarDescriptor, CompositionScalarType,
    CompositionTextTemplatePart, CompositionVariableConsumers,
};
use ugoite_core::composition::{
    CompositionDependencyIndex as CoreCompositionDependencyIndex,
    CompositionScalarCardinality as CoreCompositionScalarCardinality, ResolvedComponentKind,
    ResolvedQueryRequest,
};
use ugoite_domain::composition::{
    canonicalize_composition_yaml, CompositionDiagnosticCode, VariableGraphLiteral,
    VariableGraphResultType, VariableGraphScalarDescriptor, VariableGraphScalarType,
    VariableGraphValidatedResult,
};

/// Project the raw stored Composition revision into its portable API form.
///
/// This deliberately does not parse or canonicalize the stored Composition
/// carrier so unsupported and malformed historical payloads remain inspectable.
pub fn composition_raw_revision_response(
    raw: crate::composition::RawCompositionRevision,
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
            .collect(),
    }
}

/// Project a raw Composition history page while retaining its bounded paging
/// metadata and raw revision carriers.
pub fn composition_raw_history_page_response(
    raw: crate::composition::RawCompositionHistoryPage,
) -> CompositionHistoryPage {
    CompositionHistoryPage {
        entry_id: raw.entry_id.to_string(),
        revisions: raw
            .revisions
            .into_iter()
            .map(composition_raw_revision_response)
            .collect(),
        total: raw.total,
        offset: raw.offset,
        limit: raw.limit,
        has_more: raw.has_more,
    }
}

/// Project a bounded raw Composition list page. Stored spec bodies are not
/// part of the list DTO.
pub fn composition_raw_list_page_response(
    raw: crate::composition::RawCompositionListPage,
) -> CompositionListPage {
    CompositionListPage {
        items: raw
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
        offset: raw.offset,
        limit: raw.limit,
        has_more: raw.has_more,
    }
}

/// Canonicalize Composition YAML and create the portable lint response.
pub fn composition_lint_response(yaml: &str) -> Result<CompositionLintResponse> {
    match canonicalize_composition_yaml(yaml) {
        Ok(canonical) => Ok(CompositionLintResponse {
            ok: true,
            value: Some(CompositionLintValue {
                document: serde_json::to_value(canonical.document)
                    .context("encode Composition lint document")?,
                canonical_yaml: canonical.yaml,
                fingerprint: canonical.fingerprint,
            }),
            error: None,
        }),
        Err(code) => composition_lint_diagnostic_response(code),
    }
}

/// Project a domain Composition diagnostic into the portable lint response.
pub fn composition_lint_diagnostic_response(
    code: CompositionDiagnosticCode,
) -> Result<CompositionLintResponse> {
    let code = composition_diagnostic_code(code)?;
    Ok(CompositionLintResponse {
        ok: false,
        value: None,
        error: Some(CompositionLintError {
            kind: "composition_diagnostic".to_string(),
            code,
        }),
    })
}

/// Convert a domain Composition diagnostic into the portable API code.
pub fn composition_diagnostic_code(
    code: CompositionDiagnosticCode,
) -> Result<ugoite_api_client::CompositionDiagnosticCode> {
    let api_code = ugoite_api_client::CompositionDiagnosticCode::from_code(code.as_str())
        .with_context(|| {
            format!(
                "Composition diagnostic {} is missing from the portable API contract",
                code.as_str()
            )
        })?;
    Ok(api_code)
}

/// Convert a service evaluation result into the portable API response DTO.
pub fn composition_evaluation_response(
    response: CompositionQueryEvaluationResponse,
) -> Result<CompositionEvaluationResponse> {
    let mut composition_revision = None;
    let mut draft_fingerprint = response.draft_fingerprint.clone();
    let mut variable_values = BTreeMap::new();
    let mut queries = Vec::new();
    let mut component_bindings = Vec::new();
    let mut dependencies = CompositionDependencyIndex::default();
    if let Some(plan) = response.plan {
        let CompositionGraphEvaluationPlan {
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
            .map(|(id, value)| (id, value.to_json_value()))
            .collect();
        (queries, component_bindings) = queries_and_bindings(core_queries, core_bindings)?;
        dependencies = dependency_index(core_dependencies);
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
                .map_err(|error| anyhow!("Could not encode Composition query result: {error}"))?;
            Ok((
                variable_id,
                CompositionGraphQueryResult {
                    value,
                    has_more: result.has_more,
                    next: result.next,
                    count_snapshot: result.count_snapshot,
                    exact_integer_rows,
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;

    let diagnostics = response
        .diagnostics
        .into_iter()
        .map(|diagnostic| {
            let code =
                ugoite_api_client::CompositionDiagnosticCode::from_code(diagnostic.code.as_str())
                    .with_context(|| {
                    format!(
                        "Composition diagnostic {} is missing from the portable API contract",
                        diagnostic.code.as_str()
                    )
                })?;
            Ok(CompositionDiagnostic {
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
                            expected_value_type: detail.expected_value_type.map(scalar_descriptor),
                            observed_value_type: detail.observed_value_type.map(scalar_descriptor),
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

fn scalar_descriptor(descriptor: VariableGraphScalarDescriptor) -> CompositionScalarDescriptor {
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

fn query_result_type(returns: VariableGraphResultType) -> CompositionQueryResultType {
    match returns {
        VariableGraphResultType::Scalar { value_type } => CompositionQueryResultType::Scalar {
            value_type: scalar_descriptor(value_type),
        },
        VariableGraphResultType::Table { columns } => CompositionQueryResultType::Table {
            columns: columns.into_iter().map(result_column).collect(),
        },
    }
}

fn result_column(
    column: ugoite_domain::composition::VariableGraphColumn,
) -> CompositionResultColumn {
    CompositionResultColumn {
        name: column.name,
        value_type: scalar_descriptor(column.value_type),
    }
}

fn dependency_index(dependencies: CoreCompositionDependencyIndex) -> CompositionDependencyIndex {
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
                        table_component_ids: consumers.table_component_ids,
                    },
                )
            })
            .collect(),
    }
}

fn queries_and_bindings(
    queries: Vec<ResolvedQueryRequest>,
    component_bindings: Vec<ugoite_core::composition::ResolvedComponentBinding>,
) -> Result<(Vec<CompositionQuery>, Vec<CompositionComponentBinding>)> {
    let queries = queries
        .into_iter()
        .map(|query| {
            let request = query
                .request
                .map(serde_json::to_value)
                .transpose()
                .context("encode Composition graph SQL request")?;
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
                .map(result_column)
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
                    value_type: scalar_descriptor(argument.value_type),
                })
                .collect();
            Ok(CompositionQuery {
                variable_id: query.variable_id,
                request,
                portable_sql: query.portable_sql,
                deferred_arguments,
                returns: query_result_type(query.returns),
                result_columns,
                form_bindings,
                schema_fingerprint: query.schema_fingerprint,
                query_semantic_fingerprint: query.query_semantic_fingerprint,
                scalar_cardinality,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let component_bindings = component_bindings
        .into_iter()
        .map(|binding| CompositionComponentBinding {
            component_id: binding.component_id,
            kind: match binding.kind {
                ResolvedComponentKind::Input => CompositionComponentKind::Input,
                ResolvedComponentKind::Tabular => CompositionComponentKind::Table,
                ResolvedComponentKind::Text => CompositionComponentKind::Text,
            },
            variable_id: binding.variable_id,
            text_segments: binding
                .text_segments
                .into_iter()
                .map(|part| match part {
                    ugoite_domain::composition::CompositionTextTemplatePart::Literal { text } => {
                        CompositionTextTemplatePart::Literal { text }
                    }
                    ugoite_domain::composition::CompositionTextTemplatePart::Variable {
                        variable,
                    } => CompositionTextTemplatePart::Variable { variable },
                })
                .collect(),
        })
        .collect();
    Ok((queries, component_bindings))
}

#[cfg(test)]
mod tests {
    use super::{
        composition_lint_diagnostic_response, composition_lint_response,
        composition_raw_history_page_response, composition_raw_list_page_response,
        composition_raw_revision_response,
    };
    use crate::composition::{
        RawCompositionHistoryPage, RawCompositionListItem, RawCompositionListPage,
        RawCompositionRevision,
    };
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    use ugoite_domain::composition::{canonicalize_composition_yaml, CompositionDiagnosticCode};
    use ugoite_domain::entry::{EntryMetadata, EntryOperation, EntryRevision, FieldValue};
    use ugoite_domain::form::FormVersion;
    use ugoite_domain::id::{EntryId, FieldId, FormId, RevisionId};
    use uuid::Uuid;

    const BASIC_COMPOSITION: &str = "format: ugoite.composition\nformat_version: 1\nkind: dashboard\nname: Monthly expenses\nspec:\n  variables: []\n  components:\n    - id: note\n      kind: text\n      content: Note\n      style: body\n  layout:\n    kind: flow\n    rows:\n      - id: main\n        items:\n          - kind: component\n            component: note\n";

    fn raw_revision() -> RawCompositionRevision {
        let entry_id = EntryId::from(Uuid::from_u128(1001));
        RawCompositionRevision {
            revision: EntryRevision {
                form_id: FormId::from(Uuid::from_u128(1000)),
                entry_id,
                revision_id: RevisionId::from(Uuid::from_u128(1002)),
                parent_revision_id: Some(RevisionId::from(Uuid::from_u128(1003))),
                entry_version: 3,
                change_id: "change-1".to_string(),
                expected_version: Some(2),
                operation: EntryOperation::Restore,
                committed_at_micros: 123,
                author_id: "author-1".to_string(),
                form_version: FormVersion::new(1).expect("Form version"),
                source_kind: "core".to_string(),
                source_id: Some("source-1".to_string()),
                entry: EntryMetadata {
                    external_id: "monthly-expense".to_string(),
                    tags: vec!["monthly".to_string()],
                    updated_by: "author-1".to_string(),
                    ..EntryMetadata::default()
                },
                values: BTreeMap::from([(
                    FieldId::new(101).expect("Field ID"),
                    FieldValue::String("not: valid: yaml".to_string()),
                )]),
                extra_attributes: BTreeMap::from([("reviewed".to_string(), json!(true))]),
                extension_metadata: BTreeMap::from([("extension".to_string(), json!("kept"))]),
            },
            fields: BTreeMap::from([
                ("name".to_string(), json!("Future format")),
                ("format_version".to_string(), json!(99)),
                ("spec".to_string(), json!("not: valid: yaml")),
            ]),
            unmapped_field_values: BTreeMap::from([(FieldId::new(102).unwrap(), json!(true))]),
        }
    }

    #[test]
    fn raw_revision_projection_preserves_metadata_and_future_carriers() {
        let raw = raw_revision();
        let entry_id = raw.revision.entry_id.to_string();
        let revision_id = raw.revision.revision_id.to_string();

        let projected = composition_raw_revision_response(raw);

        assert_eq!(projected.revision.entry_id, entry_id);
        assert_eq!(projected.revision.revision_id, revision_id);
        assert_eq!(projected.revision.operation, "restore");
        assert_eq!(
            projected.revision.parent_revision_id,
            Some(Uuid::from_u128(1003).to_string())
        );
        assert_eq!(projected.revision.source_id.as_deref(), Some("source-1"));
        assert_eq!(projected.revision.entry.external_id, "monthly-expense");
        assert_eq!(projected.revision.entry.tags, ["monthly"]);
        assert_eq!(projected.fields["format_version"], json!(99));
        assert_eq!(projected.fields["spec"], "not: valid: yaml");
        assert_eq!(projected.unmapped_field_values["102"], true);
        assert_eq!(projected.revision.extra_attributes["reviewed"], true);
        assert_eq!(projected.revision.extension_metadata["extension"], "kept");
    }

    #[test]
    fn raw_revision_projection_maps_each_entry_operation() {
        for (operation, expected) in [
            (EntryOperation::Upsert, "upsert"),
            (EntryOperation::Delete, "delete"),
            (EntryOperation::Restore, "restore"),
        ] {
            let mut raw = raw_revision();
            raw.revision.operation = operation;

            assert_eq!(
                composition_raw_revision_response(raw).revision.operation,
                expected
            );
        }
    }

    #[test]
    fn raw_history_and_list_projection_preserve_bounded_page_metadata() {
        let entry_id = EntryId::from(Uuid::from_u128(2001));
        let history = composition_raw_history_page_response(RawCompositionHistoryPage {
            entry_id,
            revisions: vec![raw_revision()],
            total: 7,
            offset: 2,
            limit: 3,
            has_more: true,
        });
        assert_eq!(history.entry_id, entry_id.to_string());
        assert_eq!(history.total, 7);
        assert_eq!(history.offset, 2);
        assert_eq!(history.limit, 3);
        assert!(history.has_more);
        assert_eq!(history.revisions[0].fields["format_version"], json!(99));

        let list = composition_raw_list_page_response(RawCompositionListPage {
            items: vec![RawCompositionListItem {
                entry_id: entry_id.to_string(),
                revision_id: RevisionId::from(Uuid::from_u128(2002)),
                updated_at: 12.5,
                name: Some(json!("Monthly expenses")),
                kind: Some(json!("dashboard")),
                format_version: Some(json!(1)),
            }],
            offset: 4,
            limit: 2,
            has_more: false,
        });
        assert_eq!(list.offset, 4);
        assert_eq!(list.limit, 2);
        assert!(!list.has_more);
        assert_eq!(list.items[0].composition_id, entry_id.to_string());
        assert_eq!(list.items[0].revision_id, Uuid::from_u128(2002).to_string());
        assert_eq!(list.items[0].name, Some(json!("Monthly expenses")));
        assert_eq!(list.items[0].kind, Some(json!("dashboard")));
        assert_eq!(list.items[0].format_version, Some(json!(1)));
    }

    #[test]
    fn lint_projection_matches_domain_canonicalization() -> anyhow::Result<()> {
        let canonical =
            canonicalize_composition_yaml(BASIC_COMPOSITION).expect("valid Composition fixture");

        let response = composition_lint_response(BASIC_COMPOSITION)?;

        assert!(response.ok);
        let value = response.value.expect("canonical lint value");
        assert_eq!(value.document, serde_json::to_value(canonical.document)?);
        assert_eq!(value.canonical_yaml, canonical.yaml);
        assert_eq!(value.fingerprint, canonical.fingerprint);
        assert!(response.error.is_none());
        Ok(())
    }

    #[test]
    fn lint_projection_uses_portable_diagnostic_code() -> anyhow::Result<()> {
        let response = composition_lint_diagnostic_response(
            CompositionDiagnosticCode::UnsupportedFormatVersion,
        )?;
        assert!(!response.ok);
        assert!(response.value.is_none());
        let value: Value = serde_json::to_value(response)?;
        assert_eq!(
            value.pointer("/error/code").and_then(Value::as_str),
            Some(CompositionDiagnosticCode::UnsupportedFormatVersion.as_str())
        );
        Ok(())
    }
}
