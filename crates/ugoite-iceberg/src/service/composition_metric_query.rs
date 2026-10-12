//! Composition Metric execution for local and current-authorized callers.

use super::*;
use crate::index;
use ugoite_core::composition::{
    ResolvedComponentBinding, ResolvedComponentKind, ResolvedSourceRequest,
};
use ugoite_core::composition_metric::{
    encode_composition_metric_value, evaluate_saved_sql_metric_probe,
    CompositionMetricQueryRequest, CompositionMetricQueryResponse, CompositionMetricQueryTarget,
    CompositionMetricQueryValue,
};
use ugoite_core::query::QueryLimits;
use ugoite_core::sql_query::{SqlQueryRequest, SqlScalarProbeResult};
use ugoite_domain::composition::CompositionDiagnosticCode;
use ugoite_domain::id::{validate_entry_id, validate_revision_id};

#[derive(Clone, Copy)]
struct MetricExecutionAuthorization<'a> {
    composition_id: Option<&'a str>,
    principal_ids: Option<&'a [Uuid]>,
}

impl UgoiteService {
    /// Evaluate one metric through the local operator boundary.
    pub async fn query_composition_metric_local(
        &self,
        space_id: &str,
        request: CompositionMetricQueryRequest,
    ) -> Result<CompositionMetricQueryResponse> {
        let (plan, draft_fingerprint, diagnostics) = match &request.target {
            CompositionMetricQueryTarget::SavedRevision {
                composition_id,
                revision_id,
            } => {
                validate_storage_id(validate_entry_id(composition_id))?;
                validate_storage_id(validate_revision_id(revision_id))?;
                let resolution = self
                    .resolve_composition_local(
                        space_id,
                        composition_id,
                        revision_id,
                        &request.parameters,
                    )
                    .await?;
                (
                    resolution
                        .plan
                        .map(|plan| (plan.sources, plan.component_bindings)),
                    None,
                    resolution.diagnostics,
                )
            }
            CompositionMetricQueryTarget::Draft { yaml } => {
                let preview = self
                    .preview_composition_local(space_id, yaml, &request.parameters)
                    .await?;
                (
                    preview
                        .plan
                        .map(|plan| (plan.sources, plan.component_bindings)),
                    preview.draft_fingerprint,
                    preview.diagnostics,
                )
            }
        };
        if let Some((sources, component_bindings)) = plan {
            self.query_composition_metric_from_plan(
                space_id,
                &sources,
                &component_bindings,
                &request.component_id,
                MetricExecutionAuthorization {
                    composition_id: None,
                    principal_ids: None,
                },
                draft_fingerprint,
                None,
            )
            .await
        } else {
            Ok(CompositionMetricQueryResponse::from_diagnostics(
                draft_fingerprint,
                diagnostics,
            ))
        }
    }

    /// Evaluate one metric after rechecking current Space, Composition, source,
    /// and Saved SQL authorization for the supplied principals.
    pub async fn query_composition_metric_authorized_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        request: CompositionMetricQueryRequest,
    ) -> Result<CompositionMetricQueryResponse> {
        self.query_composition_metric_authorized_with_scalar_limits(
            space_id,
            principal_ids,
            request,
            None,
        )
        .await
    }

    #[cfg(test)]
    async fn query_composition_metric_authorized_for_test_with_limits(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        request: CompositionMetricQueryRequest,
        query_limits: &QueryLimits,
    ) -> Result<CompositionMetricQueryResponse> {
        self.query_composition_metric_authorized_with_scalar_limits(
            space_id,
            principal_ids,
            request,
            Some(query_limits),
        )
        .await
    }

    async fn query_composition_metric_authorized_with_scalar_limits(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        request: CompositionMetricQueryRequest,
        query_limits_override: Option<&QueryLimits>,
    ) -> Result<CompositionMetricQueryResponse> {
        let (plan, draft_fingerprint, diagnostics, composition_id) = match &request.target {
            CompositionMetricQueryTarget::SavedRevision {
                composition_id,
                revision_id,
            } => {
                validate_storage_id(validate_entry_id(composition_id))?;
                validate_storage_id(validate_revision_id(revision_id))?;
                let resolution = self
                    .resolve_composition_authorized_for_principals(
                        space_id,
                        composition_id,
                        revision_id,
                        &request.parameters,
                        principal_ids,
                    )
                    .await?;
                (
                    resolution
                        .plan
                        .map(|plan| (plan.sources, plan.component_bindings)),
                    None,
                    resolution.diagnostics,
                    Some(composition_id.as_str()),
                )
            }
            CompositionMetricQueryTarget::Draft { yaml } => {
                let preview = self
                    .preview_composition_authorized_for_principals(
                        space_id,
                        yaml,
                        &request.parameters,
                        principal_ids,
                    )
                    .await?;
                (
                    preview
                        .plan
                        .map(|plan| (plan.sources, plan.component_bindings)),
                    preview.draft_fingerprint,
                    preview.diagnostics,
                    None,
                )
            }
        };
        if let Some((sources, component_bindings)) = plan {
            self.query_composition_metric_from_plan(
                space_id,
                &sources,
                &component_bindings,
                &request.component_id,
                MetricExecutionAuthorization {
                    composition_id,
                    principal_ids: Some(principal_ids),
                },
                draft_fingerprint,
                query_limits_override,
            )
            .await
        } else {
            Ok(CompositionMetricQueryResponse::from_diagnostics(
                draft_fingerprint,
                diagnostics,
            ))
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn query_composition_metric_from_plan(
        &self,
        space_id: &str,
        sources: &[ResolvedSourceRequest],
        component_bindings: &[ResolvedComponentBinding],
        component_id: &str,
        authorization: MetricExecutionAuthorization<'_>,
        draft_fingerprint: Option<String>,
        query_limits_override: Option<&QueryLimits>,
    ) -> Result<CompositionMetricQueryResponse> {
        let MetricExecutionAuthorization {
            composition_id,
            principal_ids,
        } = authorization;
        let Some(binding) = component_bindings
            .iter()
            .find(|binding| binding.component_id == component_id)
        else {
            return Ok(metric_query_diagnostic(
                draft_fingerprint,
                component_id,
                CompositionDiagnosticCode::InvalidComposition,
            ));
        };
        if binding.kind != ResolvedComponentKind::Metric {
            return Ok(metric_query_diagnostic(
                draft_fingerprint,
                component_id,
                CompositionDiagnosticCode::InvalidComposition,
            ));
        }
        let Some(source_id) = binding.source_id.as_deref() else {
            return Ok(metric_query_diagnostic(
                draft_fingerprint,
                component_id,
                CompositionDiagnosticCode::InvalidComposition,
            ));
        };
        let Some(source) = sources.iter().find(|source| match source {
            ResolvedSourceRequest::EntryQuery {
                source_id: candidate,
                ..
            }
            | ResolvedSourceRequest::SavedSql {
                source_id: candidate,
                ..
            } => candidate == source_id,
        }) else {
            return Ok(metric_query_diagnostic(
                draft_fingerprint,
                component_id,
                CompositionDiagnosticCode::InvalidComposition,
            ));
        };
        let Some(result_type) = binding.expected_result_type else {
            return Ok(metric_query_diagnostic(
                draft_fingerprint,
                component_id,
                CompositionDiagnosticCode::InvalidComposition,
            ));
        };

        let value = match source {
            ResolvedSourceRequest::SavedSql {
                request: sql_request,
                ..
            } => {
                let sql_request = sql_request.clone();
                let probe = match principal_ids {
                    Some(principal_ids) => match composition_id {
                        Some(composition_id) => {
                            self.probe_sql_scalar_authorized_for_composition_metric(
                                space_id,
                                principal_ids,
                                composition_id,
                                sql_request,
                                query_limits_override,
                            )
                            .await?
                        }
                        None => {
                            self.probe_sql_scalar_authorized_with_composition_access(
                                space_id,
                                principal_ids,
                                None,
                                sql_request,
                                query_limits_override,
                            )
                            .await?
                        }
                    },
                    None => self.probe_sql_scalar_local(space_id, sql_request).await?,
                };
                match evaluate_saved_sql_metric_probe(binding, &probe) {
                    Ok(value) => value,
                    Err(diagnostic) => {
                        return Ok(metric_query_diagnostic(
                            draft_fingerprint,
                            component_id,
                            diagnostic.code,
                        ));
                    }
                }
            }
            ResolvedSourceRequest::EntryQuery { .. } => {
                return Ok(metric_query_diagnostic(
                    draft_fingerprint,
                    component_id,
                    CompositionDiagnosticCode::InvalidComposition,
                ));
            }
        };
        let value = match encode_composition_metric_value(result_type, value) {
            Ok(value) => value,
            Err(diagnostic) => {
                return Ok(metric_query_diagnostic(
                    draft_fingerprint,
                    component_id,
                    diagnostic.code,
                ));
            }
        };
        Ok(CompositionMetricQueryResponse {
            ok: true,
            draft_fingerprint,
            result: Some(CompositionMetricQueryValue {
                component_id: component_id.to_owned(),
                result_type,
                value,
            }),
            diagnostics: Vec::new(),
        })
    }

    /// Probe an exact Saved SQL revision using the local operator boundary.
    pub async fn probe_sql_scalar_local(
        &self,
        space_id: &str,
        request: SqlQueryRequest,
    ) -> Result<SqlScalarProbeResult> {
        validate_scalar_probe_request(&request)?;
        let saved_revision = self
            .read_saved_sql_query_revision(space_id, request.saved_sql.as_ref())
            .await?;
        let query_sql = saved_revision
            .as_ref()
            .map_or(request.sql.as_str(), |revision| revision.sql.as_str());
        let normalized_sql = index::normalize_sql_template(query_sql)?;
        validate_saved_sql_client_sql(saved_revision.is_some(), &request.sql, &normalized_sql)?;
        index::validate_read_only_sql(&normalized_sql)?;
        self.validate_complete_space(space_id).await?;
        let parameter_types = effective_sql_parameter_types(
            &request.parameters,
            &saved_sql_parameter_types(saved_revision.as_ref(), &request.parameter_types)?,
        )?;
        let parameters = index::datafusion_parameters(&request.parameters, &parameter_types)
            .map_err(|error| AppError::invalid_input(ErrorCode::InvalidInput, error.to_string()))?;
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        let publication = workspace.current_publication().await?;
        let checkpoint = workspace.resolve_publication(&publication).await?;
        let forms = workspace.forms_at_checkpoint(&checkpoint).await?;
        let relation_scopes = forms
            .iter()
            .map(|form| (sql_relation_name(form.id), EntryScope::AllCurrent))
            .collect::<BTreeMap<_, _>>();
        let execution_sql = if let Some(revision) = &saved_revision {
            bound_sql_for_revision(revision, &forms, &relation_scopes, &normalized_sql)?
        } else {
            normalized_sql
        };
        let (columns, result_schema, rows) =
            index::execute_sql_scalar_probe_authorized_by_form_at_checkpoint_stateless(
                &self.operator,
                &self.workspace_path(space_id),
                &execution_sql,
                &relation_scopes,
                parameters,
                forms,
                checkpoint,
            )
            .await?;
        Ok(SqlScalarProbeResult {
            columns,
            rows,
            result_schema,
        })
    }

    /// Probe an exact Saved SQL revision after rechecking current read and
    /// Saved SQL access for every principal and retaining the authorization
    /// lease through execution.
    pub async fn probe_sql_scalar_authorized_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        request: SqlQueryRequest,
    ) -> Result<SqlScalarProbeResult> {
        self.probe_sql_scalar_authorized_with_composition_access(
            space_id,
            principal_ids,
            None,
            request,
            None,
        )
        .await
    }

    async fn probe_sql_scalar_authorized_for_composition_metric(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        composition_id: &str,
        request: SqlQueryRequest,
        query_limits_override: Option<&QueryLimits>,
    ) -> Result<SqlScalarProbeResult> {
        self.probe_sql_scalar_authorized_with_composition_access(
            space_id,
            principal_ids,
            Some(composition_id),
            request,
            query_limits_override,
        )
        .await
    }

    async fn probe_sql_scalar_authorized_with_composition_access(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        composition_id: Option<&str>,
        request: SqlQueryRequest,
        query_limits_override: Option<&QueryLimits>,
    ) -> Result<SqlScalarProbeResult> {
        validate_scalar_probe_request(&request)?;
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        let (state, _authorization_lease) = Authorizer::new(self.operator.clone())
            .acquire_state_lease(space_id)
            .await?;
        for principal_id in principal_ids {
            if !effective_actions_for_state(&state, *principal_id, None)?.contains(&Action::Read) {
                return Err(
                    AppError::forbidden("principal is not authorized to read this Space").into(),
                );
            }
        }
        let saved_sql = request.saved_sql.as_ref().expect("validated scalar probe");
        let saved_revision = self
            .read_saved_sql_query_revision(space_id, Some(saved_sql))
            .await?;
        let query_sql = saved_revision
            .as_ref()
            .map_or(request.sql.as_str(), |revision| revision.sql.as_str());
        let normalized_sql = index::normalize_sql_template(query_sql)?;
        validate_saved_sql_client_sql(saved_revision.is_some(), &request.sql, &normalized_sql)?;
        index::validate_read_only_sql(&normalized_sql)?;
        self.validate_complete_space(space_id).await?;
        let parameter_types = effective_sql_parameter_types(
            &request.parameters,
            &saved_sql_parameter_types(saved_revision.as_ref(), &request.parameter_types)?,
        )?;
        let parameters = index::datafusion_parameters(&request.parameters, &parameter_types)
            .map_err(|error| AppError::invalid_input(ErrorCode::InvalidInput, error.to_string()))?;
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        let publication = workspace.current_publication().await?;
        let checkpoint = workspace.resolve_publication(&publication).await?;
        let forms = workspace.forms_at_checkpoint(&checkpoint).await?;
        if let Some(composition_id) = composition_id {
            let composition_scopes =
                Self::authorized_form_entry_scopes_for_forms(&state, principal_ids, forms.clone())?;
            require_composition_entry_read_scope(
                &composition_scopes,
                parse_entry_id(composition_id)?,
                composition_id,
            )?;
        }
        let named_scopes =
            Self::authorized_form_entry_scopes_for_forms(&state, principal_ids, forms.clone())?;
        let relation_scopes = forms
            .iter()
            .filter_map(|form| {
                named_scopes
                    .get(&form.name.to_ascii_lowercase())
                    .cloned()
                    .map(|scope| (sql_relation_name(form.id), scope))
            })
            .collect::<BTreeMap<_, _>>();
        ensure_saved_sql_access(&state, principal_ids, saved_sql)?;
        let execution_sql = if let Some(revision) = &saved_revision {
            bound_sql_for_revision(revision, &forms, &relation_scopes, &normalized_sql)?
        } else {
            normalized_sql
        };
        let (columns, result_schema, rows) = if let Some(query_limits) = query_limits_override {
            index::execute_sql_scalar_probe_authorized_by_form_at_checkpoint_stateless_with_limits(
                &self.operator,
                &self.workspace_path(space_id),
                &execution_sql,
                &relation_scopes,
                parameters,
                forms,
                checkpoint,
                query_limits.clone(),
            )
            .await?
        } else {
            index::execute_sql_scalar_probe_authorized_by_form_at_checkpoint_stateless(
                &self.operator,
                &self.workspace_path(space_id),
                &execution_sql,
                &relation_scopes,
                parameters,
                forms,
                checkpoint,
            )
            .await?
        };
        Ok(SqlScalarProbeResult {
            columns,
            rows,
            result_schema,
        })
    }
}

fn validate_scalar_probe_request(request: &SqlQueryRequest) -> Result<()> {
    request.validate().map_err(sql_query_contract_error)?;
    if request.saved_sql.is_none() || request.continuation.is_some() || !request.sql.is_empty() {
        return Err(AppError::invalid_input(
            ErrorCode::InvalidInput,
            "scalar probes require an exact Saved SQL revision and do not accept SQL text or continuations",
        )
        .into());
    }
    Ok(())
}

fn metric_query_diagnostic(
    draft_fingerprint: Option<String>,
    component_id: &str,
    code: CompositionDiagnosticCode,
) -> CompositionMetricQueryResponse {
    CompositionMetricQueryResponse {
        ok: false,
        draft_fingerprint,
        result: None,
        diagnostics: vec![
            ugoite_core::composition_metric::CompositionMetricQueryDiagnostic {
                code,
                component_id: Some(component_id.to_owned()),
                parameter_id: None,
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use opendal::{services::Memory, Operator};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::time::Duration;
    use ugoite_core::query::{QueryLimits, SqlFunctionPolicy};
    use ugoite_domain::composition::{
        CompositionComponent, CompositionDocument, CompositionFormat, CompositionKind,
        CompositionResultColumn, CompositionResultFieldType, CompositionSource, CompositionSpec,
        DashboardFlowLayout, FlowItem, FlowLayoutKind, FlowRow,
    };
    use ugoite_domain::form::sql_column_name;
    use ugoite_domain::id::FieldId;
    use uuid::Uuid;

    #[tokio::test]
    async fn authorized_saved_sql_metric_returns_no_value_when_query_exceeds_memory_limit(
    ) -> Result<()> {
        let operator = Operator::new(Memory::default())?;
        let service = UgoiteService::from_operator(
            operator,
            format!(
                "memory://composition-metric-resource-limit-{}",
                Uuid::now_v7()
            ),
        );
        let owner = Uuid::from_u128(4_138_001);
        let space_id = service
            .create_space_for_principal("metric-resource-limit", owner, "Owner")
            .await?
            .to_string();
        let form_name = "MetricResourceLimit";
        service
            .upsert_form_result(
                &space_id,
                &json!({
                    "name": form_name,
                    "fields": {
                        "amount": {"id": 101, "type": "long", "required": false}
                    }
                }),
            )
            .await?;
        let field_column = sql_column_name(FieldId::new(101)?);
        service
            .create_structured_entry_with_receipt(
                &space_id,
                "metric-resource-limit-entry",
                form_name.to_string(),
                Vec::new(),
                BTreeMap::from([("amount".to_string(), json!(7))]),
                BTreeMap::new(),
                "owner",
            )
            .await?;

        let saved_sql = service
            .create_saved_sql(
                &space_id,
                None,
                &crate::saved_sql::SqlPayload {
                    name: Some("Metric resource limit".to_string()),
                    kind: crate::saved_sql::SqlKind::UserQuery,
                    metadata: None,
                    sql: format!(
                        "SELECT SUM(\"{field_column}\") AS total FROM \"{form_name}\" GROUP BY \"{field_column}\""
                    ),
                    variables: json!([]),
                },
                "owner",
            )
            .await?;
        let source = CompositionSource::SavedSql {
            id: "source".to_string(),
            entry_id: serde_json::from_value(saved_sql["id"].clone())?,
            revision_id: serde_json::from_value(saved_sql["revision_id"].clone())?,
            expected_result: vec![CompositionResultColumn {
                name: "total".to_string(),
                result_type: CompositionResultFieldType::Integer,
            }],
            variables: BTreeMap::new(),
        };
        let document = CompositionDocument {
            format: CompositionFormat::UgoiteComposition,
            format_version: 1,
            kind: CompositionKind::Dashboard,
            name: "Metric resource limit".to_string(),
            tags: Vec::new(),
            spec: CompositionSpec {
                parameters: Vec::new(),
                sources: vec![source],
                components: vec![CompositionComponent::Metric {
                    id: "metric".to_string(),
                    label: None,
                    source: "source".to_string(),
                    column: "total".to_string(),
                    presentation: None,
                }],
                layout: DashboardFlowLayout {
                    kind: FlowLayoutKind::Flow,
                    rows: vec![FlowRow {
                        id: "main".to_string(),
                        items: vec![FlowItem::Component {
                            component: "metric".to_string(),
                        }],
                    }],
                },
            },
        };
        let saved = service
            .save_composition_authorized_for_principals(
                &space_id,
                crate::composition::CompositionSaveRequest {
                    entry_id: None,
                    base_revision_id: None,
                    document,
                },
                &owner.to_string(),
                &[owner],
            )
            .await?;

        let query_limits = QueryLimits {
            max_memory_bytes: 1,
            max_rows: index::SQL_MAX_OUTPUT_ROWS.saturating_add(1),
            timeout: Duration::from_secs(5),
            max_concurrency: 1,
            function_policy: SqlFunctionPolicy::ReadOnlyAggregateV1,
        };
        let error = service
            .query_composition_metric_authorized_for_test_with_limits(
                &space_id,
                &[owner],
                CompositionMetricQueryRequest {
                    target: CompositionMetricQueryTarget::SavedRevision {
                        composition_id: saved.entry_id.to_string(),
                        revision_id: saved.revision_id.to_string(),
                    },
                    component_id: "metric".to_string(),
                    parameters: BTreeMap::new(),
                },
                &query_limits,
            )
            .await
            .expect_err("a memory-limited SQL scalar must not return a partial Metric value");
        assert!(
            error.chain().any(|cause| cause
                .to_string()
                .contains("SQL query exceeds the configured resource limit")),
            "expected the SQL resource-limit failure, got: {error:#}"
        );
        Ok(())
    }
}
