//! Shared conversion from Composition evaluation results to the portable API DTO.

use super::{CompositionGraphEvaluationPlan, CompositionQueryEvaluationResponse};
use anyhow::{anyhow, Context, Result};
use std::collections::BTreeMap;
use ugoite_api_client::{
    CompositionComponentBinding, CompositionComponentKind, CompositionDeferredArgument,
    CompositionDependencyIndex, CompositionDiagnostic, CompositionEvaluationQueryDiagnostic,
    CompositionEvaluationRequestDiagnostic, CompositionEvaluationResponse,
    CompositionEvaluationResultContractDiagnostic, CompositionEvaluationResultDiagnostic,
    CompositionFormBinding, CompositionGraphQueryResult, CompositionQuery,
    CompositionQueryResultType, CompositionResultColumn, CompositionRevisionReference,
    CompositionScalarCardinality, CompositionScalarDescriptor, CompositionScalarType,
    CompositionTextTemplatePart, CompositionVariableConsumers,
};
use ugoite_core::composition::{
    CompositionDependencyIndex as CoreCompositionDependencyIndex,
    CompositionScalarCardinality as CoreCompositionScalarCardinality, ResolvedComponentKind,
    ResolvedQueryRequest,
};
use ugoite_domain::composition::{
    VariableGraphLiteral, VariableGraphResultType, VariableGraphScalarDescriptor,
    VariableGraphScalarType, VariableGraphValidatedResult,
};

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
