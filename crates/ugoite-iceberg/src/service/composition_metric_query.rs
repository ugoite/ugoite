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
                            )
                            .await?
                        }
                        None => {
                            self.probe_sql_scalar_authorized_for_principals(
                                space_id,
                                principal_ids,
                                sql_request,
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
        )
        .await
    }

    async fn probe_sql_scalar_authorized_for_composition_metric(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        composition_id: &str,
        request: SqlQueryRequest,
    ) -> Result<SqlScalarProbeResult> {
        self.probe_sql_scalar_authorized_with_composition_access(
            space_id,
            principal_ids,
            Some(composition_id),
            request,
        )
        .await
    }

    async fn probe_sql_scalar_authorized_with_composition_access(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        composition_id: Option<&str>,
        request: SqlQueryRequest,
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
