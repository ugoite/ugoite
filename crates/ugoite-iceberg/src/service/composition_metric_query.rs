//! Composition Metric execution for local and current-authorized callers.

use super::*;
use crate::index;
use ugoite_core::composition::{
    ResolvedComponentBinding, ResolvedComponentKind, ResolvedSourceRequest,
};
use ugoite_core::composition_metric::{
    encode_composition_metric_value, evaluate_sql_metric_probe, CompositionMetricQueryRequest,
    CompositionMetricQueryResponse, CompositionMetricQueryTarget, CompositionMetricQueryValue,
};
use ugoite_core::query::QueryLimits;
use ugoite_core::sql_query::{SqlQueryRequest, SqlScalarProbeResult};
use ugoite_domain::composition::CompositionDiagnosticCode;
use ugoite_domain::id::{validate_entry_id, validate_revision_id};

#[derive(Clone, Copy)]
struct MetricExecutionAuthorization<'a> {
    composition_id: Option<&'a str>,
    principal_ids: Option<&'a [Uuid]>,
    query_limits_override: Option<&'a QueryLimits>,
}

impl UgoiteService {
    /// Evaluate one metric through the local operator boundary.
    pub async fn query_composition_metric_local(
        &self,
        space_id: &str,
        request: CompositionMetricQueryRequest,
    ) -> Result<CompositionMetricQueryResponse> {
        if request.draft_base.is_some()
            && matches!(
                &request.target,
                CompositionMetricQueryTarget::SavedRevision { .. }
            )
        {
            return Err(AppError::invalid_input(
                ErrorCode::InvalidInput,
                "Composition metric query base is only valid for a draft target",
            )
            .into());
        }
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
                        &request.variables,
                        &request.unset_variables,
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
                    .preview_composition_local_with_base(
                        space_id,
                        yaml,
                        &request.variables,
                        &request.unset_variables,
                        request.draft_base,
                    )
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
                    query_limits_override: None,
                },
                draft_fingerprint,
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
        if request.draft_base.is_some()
            && matches!(
                &request.target,
                CompositionMetricQueryTarget::SavedRevision { .. }
            )
        {
            return Err(AppError::invalid_input(
                ErrorCode::InvalidInput,
                "Composition metric query base is only valid for a draft target",
            )
            .into());
        }
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
                        &request.variables,
                        &request.unset_variables,
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
                    .preview_composition_authorized_for_principals_with_base(
                        space_id,
                        yaml,
                        &request.variables,
                        &request.unset_variables,
                        principal_ids,
                        request.draft_base,
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
                    query_limits_override,
                },
                draft_fingerprint,
            )
            .await
        } else {
            Ok(CompositionMetricQueryResponse::from_diagnostics(
                draft_fingerprint,
                diagnostics,
            ))
        }
    }

    async fn query_composition_metric_from_plan(
        &self,
        space_id: &str,
        sources: &[ResolvedSourceRequest],
        component_bindings: &[ResolvedComponentBinding],
        component_id: &str,
        authorization: MetricExecutionAuthorization<'_>,
        draft_fingerprint: Option<String>,
    ) -> Result<CompositionMetricQueryResponse> {
        let MetricExecutionAuthorization {
            composition_id,
            principal_ids,
            query_limits_override,
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
            ResolvedSourceRequest::Sql {
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

        let ResolvedSourceRequest::Sql {
            request: sql_request,
            ..
        } = source;
        let probe = match principal_ids {
            Some(principal_ids) => {
                self.probe_composition_sql_scalar_authorized(
                    space_id,
                    principal_ids,
                    composition_id,
                    sql_request.clone(),
                    query_limits_override,
                )
                .await?
            }
            None => {
                self.probe_composition_sql_scalar_local(space_id, sql_request.clone())
                    .await?
            }
        };
        let value = match evaluate_sql_metric_probe(binding, &probe) {
            Ok(value) => value,
            Err(diagnostic) => {
                return Ok(metric_query_diagnostic(
                    draft_fingerprint,
                    component_id,
                    diagnostic.code,
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
        let prepared = sql_execution::prepare_sql_request(self, space_id, &request, None).await?;
        self.validate_complete_space(space_id).await?;
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        let publication = workspace.current_publication().await?;
        let checkpoint = workspace.resolve_publication(&publication).await?;
        let forms = workspace.forms_at_checkpoint(&checkpoint).await?;
        let relation_scopes = sql_execution::all_current_relation_scopes(&forms);
        let execution_sql = sql_execution::bind_sql_to_forms(&prepared, &forms, &relation_scopes)?;
        let parameters = prepared.parameters;
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

    async fn probe_composition_sql_scalar_local(
        &self,
        space_id: &str,
        request: SqlQueryRequest,
    ) -> Result<SqlScalarProbeResult> {
        validate_scalar_probe_request_with_inline(&request, true)?;
        let prepared = sql_execution::prepare_sql_request(self, space_id, &request, None).await?;
        self.validate_complete_space(space_id).await?;
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        let publication = workspace.current_publication().await?;
        let checkpoint = workspace.resolve_publication(&publication).await?;
        let forms = workspace.forms_at_checkpoint(&checkpoint).await?;
        let relation_scopes = sql_execution::all_current_relation_scopes(&forms);
        let execution_sql = sql_execution::bind_sql_to_forms(&prepared, &forms, &relation_scopes)?;
        let (columns, result_schema, rows) =
            index::execute_sql_scalar_probe_authorized_by_form_at_checkpoint_stateless(
                &self.operator,
                &self.workspace_path(space_id),
                &execution_sql,
                &relation_scopes,
                prepared.parameters,
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
            false,
        )
        .await
    }

    async fn probe_composition_sql_scalar_authorized(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        composition_id: Option<&str>,
        request: SqlQueryRequest,
        query_limits_override: Option<&QueryLimits>,
    ) -> Result<SqlScalarProbeResult> {
        self.probe_sql_scalar_authorized_with_composition_access(
            space_id,
            principal_ids,
            composition_id,
            request,
            query_limits_override,
            true,
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
        allow_inline_sql: bool,
    ) -> Result<SqlScalarProbeResult> {
        validate_scalar_probe_request_with_inline(&request, allow_inline_sql)?;
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
        let prepared = sql_execution::prepare_sql_request(
            self,
            space_id,
            &request,
            Some(sql_execution::SqlSourceAuthorization {
                state: &state,
                principal_ids,
            }),
        )
        .await?;
        self.validate_complete_space(space_id).await?;
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
        let relation_scopes =
            sql_execution::authorized_relation_scopes(&state, principal_ids, &forms)?;
        let execution_sql = sql_execution::bind_sql_to_forms(&prepared, &forms, &relation_scopes)?;
        let parameters = prepared.parameters;
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
    validate_scalar_probe_request_with_inline(request, false)
}

fn validate_scalar_probe_request_with_inline(
    request: &SqlQueryRequest,
    allow_inline_sql: bool,
) -> Result<()> {
    request.validate().map_err(sql_query_contract_error)?;
    let is_exact_saved_sql = request.saved_sql.is_some() && request.sql.is_empty();
    let is_inline_sql = allow_inline_sql && request.saved_sql.is_none() && !request.sql.is_empty();
    if (!is_exact_saved_sql && !is_inline_sql) || request.continuation.is_some() {
        return Err(AppError::invalid_input(
            ErrorCode::InvalidInput,
            "scalar probes require one SQL source and do not accept continuations",
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
                variable_id: None,
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
    use ugoite_core::composition::CompositionRevisionRef;
    use ugoite_core::query::{QueryLimits, SqlFunctionPolicy};
    use ugoite_domain::composition::{
        canonicalize_composition, CompositionComponent, CompositionDocument,
        CompositionFormBinding, CompositionFormat, CompositionKind, CompositionResultColumn,
        CompositionResultFieldType, CompositionSource, CompositionSpec, DashboardFlowLayout,
        FlowItem, FlowLayoutKind, FlowRow,
    };
    use ugoite_domain::form::{sql_column_name, FormChange, FormChangeSet};
    use ugoite_domain::id::FieldId;
    use uuid::Uuid;

    #[tokio::test]
    async fn draft_metric_uses_exact_base_bindings_and_ignores_forged_bindings() -> Result<()> {
        let operator = Operator::new(Memory::default())?;
        let service = UgoiteService::from_operator(
            operator,
            format!("memory://composition-metric-draft-base-{}", Uuid::now_v7()),
        );
        let owner = Uuid::now_v7();
        let space_id = service
            .create_space_for_principal("metric-draft-base", owner, "Owner")
            .await?
            .to_string();
        let original_form_name = "MetricProvenance";
        let amount_field = FieldId::new(101)?;
        let original_form = service
            .upsert_form_result(
                &space_id,
                &json!({
                    "name": original_form_name,
                    "fields": {
                        "amount": {"id": 101, "type": "long", "required": false}
                    }
                }),
            )
            .await?;
        service
            .create_structured_entry_with_receipt(
                &space_id,
                "metric-provenance-original",
                original_form_name.to_string(),
                Vec::new(),
                BTreeMap::from([("amount".to_string(), json!(5))]),
                BTreeMap::new(),
                "owner",
            )
            .await?;

        let expected_result = vec![CompositionResultColumn {
            name: "total".to_string(),
            result_type: CompositionResultFieldType::Integer,
        }];
        let document = CompositionDocument {
            format: CompositionFormat::UgoiteComposition,
            format_version: 1,
            kind: CompositionKind::Dashboard,
            name: "Metric provenance".to_string(),
            spec: CompositionSpec {
                variables: Vec::new(),
                sources: vec![CompositionSource::InlineSql {
                    id: "metric_source".to_string(),
                    name: "Total".to_string(),
                    sql: format!(
                        "SELECT SUM(\"{}\") AS total FROM \"{original_form_name}\"",
                        sql_column_name(amount_field)
                    ),
                    form_bindings: Vec::new(),
                    expected_result: Vec::new(),
                    schema_fingerprint: String::new(),
                }],
                components: vec![CompositionComponent::Metric {
                    id: "metric".to_string(),
                    source: "metric_source".to_string(),
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

        // Rename the originally bound Form, then reuse its old name for a
        // distinct Form with a different scalar value. The exact base must
        // retain the original ID; an unbased draft resolves the current name.
        let workspace = crate::iceberg_store::native_mutation_workspace(
            service.operator(),
            &service.workspace_path(&space_id),
        )
        .await?;
        let current_form = workspace
            .list_forms()
            .await?
            .into_iter()
            .find(|candidate| candidate.id == original_form.form_id)
            .expect("original Form remains available after save");
        let changes = FormChangeSet {
            form_id: original_form.form_id,
            expected_version: Some(current_form.version),
            changes: vec![FormChange::RenameForm {
                name: "MetricProvenanceRenamed".to_string(),
            }],
        };
        let publication = crate::system_publication_context(
            format!(
                "test-composition-metric-form-rename:{}",
                original_form.form_id
            ),
            "form.evolve",
            &changes,
        )?;
        workspace
            .commit(publication)?
            .evolve_form_with_receipt(&changes)
            .await?;
        let replacement_form = service
            .upsert_form_result(
                &space_id,
                &json!({
                    "name": original_form_name,
                    "fields": {
                        "amount": {"id": 101, "type": "long", "required": false}
                    }
                }),
            )
            .await?;
        for (entry_name, amount) in [("replacement-a", 7), ("replacement-b", 8)] {
            service
                .create_structured_entry_with_receipt(
                    &space_id,
                    entry_name,
                    original_form_name.to_string(),
                    Vec::new(),
                    BTreeMap::from([("amount".to_string(), json!(amount))]),
                    BTreeMap::new(),
                    "owner",
                )
                .await?;
        }

        let mut forged = saved.document.clone();
        let CompositionSource::InlineSql {
            form_bindings,
            expected_result: saved_expected_result,
            ..
        } = &mut forged.spec.sources[0]
        else {
            panic!("metric source is inline SQL");
        };
        *form_bindings = vec![CompositionFormBinding {
            relation: original_form_name.to_string(),
            form_id: replacement_form.form_id,
        }];
        assert_eq!(*saved_expected_result, expected_result);
        let forged_yaml = canonicalize_composition(&forged)
            .map_err(|code| anyhow::anyhow!(code.as_str()))?
            .yaml;
        let request = |draft_base| CompositionMetricQueryRequest {
            target: CompositionMetricQueryTarget::Draft {
                yaml: forged_yaml.clone(),
            },
            draft_base,
            component_id: "metric".to_string(),
            variables: BTreeMap::new(),
            unset_variables: Vec::new(),
        };
        let new_draft = service
            .query_composition_metric_authorized_for_principals(&space_id, &[owner], request(None))
            .await?;
        assert!(new_draft.ok, "new draft metric should resolve");
        assert_eq!(
            new_draft.result.expect("replacement metric").value,
            json!("15")
        );

        let edited_draft = service
            .query_composition_metric_authorized_for_principals(
                &space_id,
                &[owner],
                request(Some(CompositionRevisionRef {
                    entry_id: saved.entry_id,
                    revision_id: saved.revision_id,
                })),
            )
            .await?;
        assert!(edited_draft.ok, "base-edited metric should resolve");
        assert_eq!(
            edited_draft.result.expect("original metric").value,
            json!("5")
        );
        Ok(())
    }

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
            schema_fingerprint: "0".repeat(64),
            variables: BTreeMap::new(),
        };
        let document = CompositionDocument {
            format: CompositionFormat::UgoiteComposition,
            format_version: 1,
            kind: CompositionKind::Dashboard,
            name: "Metric resource limit".to_string(),
            spec: CompositionSpec {
                variables: Vec::new(),
                sources: vec![source],
                components: vec![CompositionComponent::Metric {
                    id: "metric".to_string(),
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
                    draft_base: None,
                    component_id: "metric".to_string(),
                    variables: BTreeMap::new(),
                    unset_variables: Vec::new(),
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
