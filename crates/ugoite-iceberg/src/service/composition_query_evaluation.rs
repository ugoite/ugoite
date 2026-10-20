//! Stateless execution of a typed Composition query graph.
//!
//! Every request resolves the exact Composition target and current query
//! descriptors again. SQL results live only for the duration of this request;
//! table continuations are signed SQL cursors additionally scoped to the
//! Composition snapshot and query variable.

use anyhow::{anyhow, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use ugoite_core::composition::{
    CompositionDependencyIndex, CompositionDiagnostic, CompositionRevisionRef,
    ResolvedComponentBinding, ResolvedQueryRequest,
};
use ugoite_core::composition_query::{
    VariableGraphQueryEvaluation, VariableGraphQueryEvaluationDiagnostic,
};
use ugoite_core::error::{AppError, ErrorCode, ErrorKind};
use ugoite_core::sql_query::{SqlContinuation, SqlQueryPage, SqlQueryRequest};
use ugoite_domain::composition::{
    VariableGraph, VariableGraphLiteral, VariableGraphNode, VariableGraphNodeKind,
    VariableGraphResultType, VariableGraphValidatedResult,
};
use ugoite_domain::id::{validate_entry_id, validate_revision_id, EntryId, RevisionId};
use ugoite_domain::identity::Action;
use uuid::Uuid;

use super::{
    effective_actions_for_state, require_nonempty_authorized_principals,
    sql_query_authorization_fingerprint, sql_query_contract_error, validate_storage_id, Authorizer,
    PublicationRef, UgoiteService,
};
use crate::iceberg_store;

/// Stored revision or unsaved draft evaluated by the graph executor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompositionQueryEvaluationTarget {
    SavedRevision {
        composition_id: String,
        revision_id: String,
    },
    Draft {
        yaml: String,
    },
}

/// One table page selected from a query variable's result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompositionQueryEvaluationPage {
    pub variable_id: String,
    pub cursor: String,
}

/// A query result validated against the graph declaration.
#[derive(Clone, Debug, PartialEq)]
pub struct CompositionGraphQueryResult {
    pub result: VariableGraphValidatedResult,
    pub has_more: bool,
    pub next: Option<String>,
}

/// Resolved graph metadata shared by all components referencing each query
/// variable. The result map is keyed by query variable ID, not component ID.
#[derive(Clone, Debug, PartialEq)]
pub struct CompositionGraphEvaluationPlan {
    pub composition_revision: Option<CompositionRevisionRef>,
    pub draft_fingerprint: Option<String>,
    pub variable_values: BTreeMap<String, VariableGraphLiteral>,
    pub queries: Vec<ResolvedQueryRequest>,
    pub component_bindings: Vec<ResolvedComponentBinding>,
    pub dependencies: CompositionDependencyIndex,
}

/// Transient response for one immutable graph evaluation snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct CompositionQueryEvaluationResponse {
    pub evaluation_id: Uuid,
    pub ok: bool,
    pub draft_fingerprint: Option<String>,
    pub snapshot_fingerprint: Option<String>,
    pub publication_fingerprint: Option<String>,
    pub authorization_fingerprint: Option<String>,
    pub variable_definitions: Option<Vec<VariableGraphNode>>,
    pub plan: Option<CompositionGraphEvaluationPlan>,
    pub query_results: BTreeMap<String, CompositionGraphQueryResult>,
    pub computed_query_ids: Vec<String>,
    pub diagnostics: Vec<CompositionDiagnostic>,
    pub query_diagnostics: Vec<VariableGraphQueryEvaluationDiagnostic>,
}

impl UgoiteService {
    /// Resolve and execute the selected query variables and their transitive
    /// query dependencies once in dependency order. Results and input values
    /// are request-scoped; no Entry or intermediate SQL result is written to
    /// Space storage.
    #[allow(clippy::too_many_arguments)]
    pub async fn evaluate_composition_graph_authorized_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        target: CompositionQueryEvaluationTarget,
        evaluation_id: Uuid,
        base_entry_id: Option<&str>,
        base_revision_id: Option<&str>,
        variables: &BTreeMap<String, Value>,
        unset_variables: &[String],
        requested_queries: Option<&[String]>,
        page: Option<CompositionQueryEvaluationPage>,
    ) -> Result<CompositionQueryEvaluationResponse> {
        self.evaluate_composition_graph(
            space_id,
            Some(principal_ids),
            target,
            evaluation_id,
            base_entry_id,
            base_revision_id,
            variables,
            unset_variables,
            requested_queries,
            page,
        )
        .await
    }

    /// Resolve and execute a graph at the trusted local operator boundary.
    /// Local Space access has no application principal, but uses the same
    /// snapshot-bound query graph executor and produces the same transient
    /// response semantics as the authorized graph route.
    #[allow(clippy::too_many_arguments)]
    pub async fn evaluate_composition_graph_local(
        &self,
        space_id: &str,
        target: CompositionQueryEvaluationTarget,
        evaluation_id: Uuid,
        base_entry_id: Option<&str>,
        base_revision_id: Option<&str>,
        variables: &BTreeMap<String, Value>,
        unset_variables: &[String],
        requested_queries: Option<&[String]>,
        page: Option<CompositionQueryEvaluationPage>,
    ) -> Result<CompositionQueryEvaluationResponse> {
        self.evaluate_composition_graph(
            space_id,
            None,
            target,
            evaluation_id,
            base_entry_id,
            base_revision_id,
            variables,
            unset_variables,
            requested_queries,
            page,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn evaluate_composition_graph(
        &self,
        space_id: &str,
        principal_ids: Option<&[Uuid]>,
        target: CompositionQueryEvaluationTarget,
        evaluation_id: Uuid,
        base_entry_id: Option<&str>,
        base_revision_id: Option<&str>,
        variables: &BTreeMap<String, Value>,
        unset_variables: &[String],
        requested_queries: Option<&[String]>,
        page: Option<CompositionQueryEvaluationPage>,
    ) -> Result<CompositionQueryEvaluationResponse> {
        if evaluation_id.is_nil() {
            return Err(AppError::invalid_input(
                ErrorCode::InvalidInput,
                "Composition evaluation_id must be a non-nil UUID",
            )
            .into());
        }
        let base = match (base_entry_id, base_revision_id) {
            (None, None) => None,
            (Some(entry_id), Some(revision_id)) => {
                validate_storage_id(validate_entry_id(entry_id))?;
                validate_storage_id(validate_revision_id(revision_id))?;
                Some(CompositionRevisionRef {
                    entry_id: EntryId::from_uuid(Uuid::parse_str(entry_id)?),
                    revision_id: RevisionId::from_uuid(Uuid::parse_str(revision_id)?),
                })
            }
            _ => {
                return Err(AppError::invalid_input(
                    ErrorCode::InvalidInput,
                    "Composition base_entry_id and base_revision_id must be supplied together",
                )
                .into());
            }
        };

        let decoded_cursor = match &page {
            Some(page) => {
                let key = self.sql_query_signing_key(space_id).await?;
                let cursor = SqlContinuation::decode(&page.cursor, &key)
                    .map_err(sql_query_contract_error)?;
                if cursor.context_fingerprint.is_none() {
                    return Err(AppError::invalid_input(
                        ErrorCode::InvalidInput,
                        "Composition page cursor is missing its graph snapshot binding",
                    )
                    .into());
                }
                Some(cursor)
            }
            None => None,
        };
        let pinned_publication = decoded_cursor
            .as_ref()
            .map(|cursor| cursor.publication.clone());
        let base_acl_entry = match &target {
            CompositionQueryEvaluationTarget::SavedRevision { composition_id, .. } => {
                Some(composition_id.clone())
            }
            CompositionQueryEvaluationTarget::Draft { .. } => base_entry_id.map(str::to_owned),
        };
        // Resolve descriptors only after capturing the snapshot they must
        // match. The resolver reads the live Publication, so changes during
        // resolution are rejected by the check immediately afterward.
        let (authorization_fingerprint, publication) = self
            .capture_graph_snapshot(
                space_id,
                principal_ids,
                base_acl_entry.as_deref(),
                pinned_publication.as_ref(),
            )
            .await?;

        let (
            composition_entry_id,
            identity,
            variable_definitions,
            plan,
            draft_fingerprint,
            diagnostics,
        ) = match target {
            CompositionQueryEvaluationTarget::SavedRevision {
                composition_id,
                revision_id,
            } => {
                if base.is_some() {
                    return Err(AppError::invalid_input(
                        ErrorCode::InvalidInput,
                        "Composition evaluation base is only valid for a draft target",
                    )
                    .into());
                }
                let resolution = match principal_ids {
                    Some(principal_ids) => {
                        self.resolve_composition_authorized_for_query_selection(
                            space_id,
                            &composition_id,
                            &revision_id,
                            variables,
                            unset_variables,
                            principal_ids,
                            requested_queries,
                        )
                        .await?
                    }
                    None => {
                        self.resolve_composition_local_for_query_selection(
                            space_id,
                            &composition_id,
                            &revision_id,
                            variables,
                            unset_variables,
                            requested_queries,
                        )
                        .await?
                    }
                };
                let identity = match &resolution.plan {
                    Some(plan) => format!(
                        "saved:{}:{}",
                        plan.composition_revision.entry_id, plan.composition_revision.revision_id
                    ),
                    None => format!("saved:{composition_id}:{revision_id}"),
                };
                (
                    Some(composition_id),
                    identity,
                    resolution.variable_definitions,
                    resolution.plan.map(|plan| CompositionGraphEvaluationPlan {
                        composition_revision: Some(plan.composition_revision),
                        draft_fingerprint: None,
                        variable_values: plan.variable_values,
                        queries: plan.queries,
                        component_bindings: plan.component_bindings,
                        dependencies: plan.dependencies,
                    }),
                    None,
                    resolution.diagnostics,
                )
            }
            CompositionQueryEvaluationTarget::Draft { yaml } => {
                let preview = match principal_ids {
                    Some(principal_ids) => {
                        self.preview_composition_authorized_for_principals_with_base_and_query_selection(
                            space_id,
                            &yaml,
                            variables,
                            unset_variables,
                            principal_ids,
                            base,
                            requested_queries,
                        )
                        .await?
                    }
                    None => {
                        self.preview_composition_local_with_base_and_query_selection(
                            space_id,
                            &yaml,
                            variables,
                            unset_variables,
                            base,
                            requested_queries,
                        )
                        .await?
                    }
                };
                let draft_fingerprint = preview.draft_fingerprint.clone();
                let identity = format!(
                    "draft:{}:{}",
                    draft_fingerprint.as_deref().unwrap_or("invalid"),
                    base.map(|base| format!("{}:{}", base.entry_id, base.revision_id))
                        .unwrap_or_default()
                );
                (
                    base_entry_id.map(str::to_owned),
                    identity,
                    preview.variable_definitions,
                    preview.plan.map(|plan| CompositionGraphEvaluationPlan {
                        composition_revision: None,
                        draft_fingerprint: Some(plan.draft_fingerprint),
                        variable_values: plan.variable_values,
                        queries: plan.queries,
                        component_bindings: plan.component_bindings,
                        dependencies: plan.dependencies,
                    }),
                    draft_fingerprint,
                    preview.diagnostics,
                )
            }
        };

        self.ensure_graph_resolution_snapshot_unchanged(
            space_id,
            principal_ids,
            base_acl_entry.as_deref(),
            &authorization_fingerprint,
            &publication,
        )
        .await?;

        let Some(plan) = plan else {
            return Ok(CompositionQueryEvaluationResponse {
                evaluation_id,
                ok: false,
                draft_fingerprint,
                snapshot_fingerprint: None,
                publication_fingerprint: None,
                authorization_fingerprint: None,
                variable_definitions,
                plan: None,
                query_results: BTreeMap::new(),
                computed_query_ids: Vec::new(),
                diagnostics,
                query_diagnostics: Vec::new(),
            });
        };

        let definitions = variable_definitions.clone().ok_or_else(|| {
            anyhow!("resolved Composition graph did not include variable definitions")
        })?;
        let mut graph = VariableGraph {
            variables: definitions,
        };
        apply_resolved_query_descriptors(&mut graph, &plan.queries)?;
        remove_explicitly_unset_input_defaults(&mut graph, unset_variables);
        if let Some(page) = &page {
            let Some(node) = graph
                .variables
                .iter()
                .find(|node| node.id == page.variable_id)
            else {
                return Err(AppError::invalid_input(
                    ErrorCode::InvalidInput,
                    "Composition page variable does not exist",
                )
                .into());
            };
            if !matches!(
                &node.kind,
                VariableGraphNodeKind::Query {
                    returns: VariableGraphResultType::Table { .. },
                    ..
                }
            ) {
                return Err(AppError::invalid_input(
                    ErrorCode::InvalidInput,
                    "Composition page cursors apply only to table query variables",
                )
                .into());
            }
        }

        let snapshot_fingerprint = graph_snapshot_fingerprint(
            evaluation_id,
            &identity,
            &publication,
            &authorization_fingerprint,
            &plan,
        )?;
        let publication_fingerprint = fingerprint_serializable(&publication)?;

        let input_values = plan
            .variable_values
            .iter()
            .filter_map(|(id, value)| {
                graph
                    .variables
                    .iter()
                    .find(|node| node.id == *id)
                    .is_some_and(|node| matches!(node.kind, VariableGraphNodeKind::Input { .. }))
                    .then_some((id.clone(), value.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        let mut evaluation = match VariableGraphQueryEvaluation::new_for_queries(
            &graph,
            evaluation_id,
            &input_values,
            requested_queries,
        ) {
            Ok(evaluation) => evaluation,
            Err(query_diagnostics) => {
                return self
                    .query_evaluation_diagnostics_response_for_access(
                        space_id,
                        principal_ids,
                        base_acl_entry.as_deref(),
                        &publication,
                        &authorization_fingerprint,
                        evaluation_id,
                        draft_fingerprint,
                        snapshot_fingerprint,
                        publication_fingerprint,
                        variable_definitions,
                        plan,
                        query_diagnostics,
                    )
                    .await;
            }
        };
        let computed_query_ids = evaluation.selected_query_ids().to_vec();
        if let Some(page) = &page {
            if !computed_query_ids.contains(&page.variable_id) {
                return self
                    .query_evaluation_diagnostics_response_for_access(
                        space_id,
                        principal_ids,
                        base_acl_entry.as_deref(),
                        &publication,
                        &authorization_fingerprint,
                        evaluation_id,
                        draft_fingerprint,
                        snapshot_fingerprint,
                        publication_fingerprint,
                        variable_definitions,
                        plan,
                        vec![VariableGraphQueryEvaluationDiagnostic::for_variable(
                            ugoite_core::composition_query::VariableGraphQueryEvaluationDiagnosticCode::QueryNotSelected,
                            &page.variable_id,
                        )],
                    )
                    .await;
            }
        }
        let queries_by_id = plan
            .queries
            .iter()
            .map(|query| (query.variable_id.as_str(), query))
            .collect::<BTreeMap<_, _>>();

        for variable_id in &computed_query_ids {
            let Some(query) = queries_by_id.get(variable_id.as_str()).copied() else {
                continue;
            };
            let selected_cursor = page
                .as_ref()
                .filter(|page| page.variable_id == *variable_id);
            let request = match evaluation.compile_page_request(
                variable_id,
                selected_cursor.map(|page| page.cursor.as_str()),
            ) {
                Ok(request) => request,
                Err(query_diagnostics) => {
                    return self
                        .query_evaluation_diagnostics_response_for_access(
                            space_id,
                            principal_ids,
                            base_acl_entry.as_deref(),
                            &publication,
                            &authorization_fingerprint,
                            evaluation_id,
                            draft_fingerprint,
                            snapshot_fingerprint,
                            publication_fingerprint,
                            variable_definitions,
                            plan,
                            query_diagnostics,
                        )
                        .await;
                }
            };
            let query_context = query_snapshot_fingerprint(&snapshot_fingerprint, query, &request)?;
            let page_result = if matches!(query.returns, VariableGraphResultType::Scalar { .. }) {
                let probe = match principal_ids {
                    Some(principal_ids) => {
                        self.probe_composition_sql_scalar_authorized_at_publication(
                            space_id,
                            principal_ids,
                            composition_entry_id.as_deref(),
                            request.clone(),
                            &publication,
                        )
                        .await?
                    }
                    None => {
                        self.probe_composition_sql_scalar_local_at_publication(
                            space_id,
                            request.clone(),
                            Some(&publication),
                        )
                        .await?
                    }
                };
                SqlQueryPage {
                    columns: probe.columns,
                    rows: probe.rows,
                    has_more: false,
                    next: None,
                    execution_id: request.execution_id,
                    request_fingerprint: Some(
                        request
                            .execution_fingerprint()
                            .map_err(sql_query_contract_error)?,
                    ),
                    result_schema: Some(probe.result_schema),
                    exact_integer_rows: None,
                }
            } else {
                match principal_ids {
                    Some(principal_ids) => {
                        self.query_sql_authorized_for_composition(
                            space_id,
                            principal_ids,
                            request.clone(),
                            composition_entry_id.as_deref(),
                            &publication,
                            &query_context,
                        )
                        .await?
                    }
                    None => {
                        self.query_sql_local_for_composition(
                            space_id,
                            request.clone(),
                            &publication,
                            &query_context,
                        )
                        .await?
                    }
                }
            };
            if let Err(query_diagnostics) =
                evaluation.accept_page(variable_id, &request, &page_result)
            {
                return self
                    .query_evaluation_diagnostics_response_for_access(
                        space_id,
                        principal_ids,
                        base_acl_entry.as_deref(),
                        &publication,
                        &authorization_fingerprint,
                        evaluation_id,
                        draft_fingerprint,
                        snapshot_fingerprint,
                        publication_fingerprint,
                        variable_definitions,
                        plan,
                        query_diagnostics,
                    )
                    .await;
            }
        }

        let preview = match evaluation.preview() {
            Ok(preview) => preview,
            Err(query_diagnostics) => {
                return self
                    .query_evaluation_diagnostics_response_for_access(
                        space_id,
                        principal_ids,
                        base_acl_entry.as_deref(),
                        &publication,
                        &authorization_fingerprint,
                        evaluation_id,
                        draft_fingerprint,
                        snapshot_fingerprint,
                        publication_fingerprint,
                        variable_definitions,
                        plan,
                        query_diagnostics,
                    )
                    .await;
            }
        };
        let (current_authorization_fingerprint, _) = self
            .capture_graph_snapshot(
                space_id,
                principal_ids,
                base_acl_entry.as_deref(),
                Some(&publication),
            )
            .await?;
        ensure_authorization_fingerprint_unchanged(
            &authorization_fingerprint,
            &current_authorization_fingerprint,
        )?;
        let query_results = preview
            .query_results
            .into_iter()
            .map(|(variable_id, value)| {
                (
                    variable_id,
                    CompositionGraphQueryResult {
                        result: value.result,
                        has_more: value.has_more,
                        next: value.next,
                    },
                )
            })
            .collect();

        Ok(CompositionQueryEvaluationResponse {
            evaluation_id,
            ok: true,
            draft_fingerprint,
            snapshot_fingerprint: Some(snapshot_fingerprint),
            publication_fingerprint: Some(publication_fingerprint),
            authorization_fingerprint: Some(current_authorization_fingerprint),
            variable_definitions,
            plan: Some(plan),
            query_results,
            computed_query_ids,
            diagnostics: Vec::new(),
            query_diagnostics: Vec::new(),
        })
    }

    async fn capture_graph_snapshot(
        &self,
        space_id: &str,
        principal_ids: Option<&[Uuid]>,
        composition_entry_id: Option<&str>,
        pinned_publication: Option<&PublicationRef>,
    ) -> Result<(String, PublicationRef)> {
        match principal_ids {
            Some(principal_ids) => {
                self.capture_authorized_graph_snapshot(
                    space_id,
                    principal_ids,
                    composition_entry_id,
                    pinned_publication,
                )
                .await
            }
            None => {
                self.capture_local_graph_snapshot(space_id, pinned_publication)
                    .await
            }
        }
    }

    async fn capture_local_graph_snapshot(
        &self,
        space_id: &str,
        pinned_publication: Option<&PublicationRef>,
    ) -> Result<(String, PublicationRef)> {
        const LOCAL_AUTHORIZATION_FINGERPRINT: &str = "local-operator";

        self.validate_complete_space(space_id).await?;
        let workspace = iceberg_store::native_workspace_read_only(
            &self.operator,
            &self.workspace_path(space_id),
        )
        .await?;
        let publication = match pinned_publication {
            Some(publication) => publication.clone(),
            None => workspace.current_publication().await?,
        };
        workspace.resolve_publication(&publication).await?;
        Ok((LOCAL_AUTHORIZATION_FINGERPRINT.to_owned(), publication))
    }

    async fn capture_authorized_graph_snapshot(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        composition_entry_id: Option<&str>,
        pinned_publication: Option<&PublicationRef>,
    ) -> Result<(String, PublicationRef)> {
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
        let workspace = iceberg_store::native_workspace_read_only(
            &self.operator,
            &self.workspace_path(space_id),
        )
        .await?;
        let publication = match pinned_publication {
            Some(publication) => publication.clone(),
            None => workspace.current_publication().await?,
        };
        let checkpoint = workspace.resolve_publication(&publication).await?;
        if let Some(composition_entry_id) = composition_entry_id {
            let forms = workspace.forms_at_checkpoint(&checkpoint).await?;
            let scopes =
                Self::authorized_form_entry_scopes_for_forms(&state, principal_ids, forms)?;
            super::require_composition_entry_read_scope(
                &scopes,
                super::parse_entry_id(composition_entry_id)?,
                composition_entry_id,
            )?;
        }
        let authorization_fingerprint = sql_query_authorization_fingerprint(&state, principal_ids)?;
        Ok((authorization_fingerprint, publication))
    }

    async fn ensure_graph_resolution_snapshot_unchanged(
        &self,
        space_id: &str,
        principal_ids: Option<&[Uuid]>,
        composition_entry_id: Option<&str>,
        expected_authorization_fingerprint: &str,
        expected_publication: &PublicationRef,
    ) -> Result<()> {
        let (current_authorization_fingerprint, current_publication) = self
            .capture_graph_snapshot(space_id, principal_ids, composition_entry_id, None)
            .await?;
        ensure_graph_snapshot_unchanged(
            expected_authorization_fingerprint,
            expected_publication,
            &current_authorization_fingerprint,
            &current_publication,
        )
    }

    #[allow(clippy::too_many_arguments)]
    async fn query_evaluation_diagnostics_response_for_access(
        &self,
        space_id: &str,
        principal_ids: Option<&[Uuid]>,
        composition_entry_id: Option<&str>,
        publication: &PublicationRef,
        expected_authorization_fingerprint: &str,
        evaluation_id: Uuid,
        draft_fingerprint: Option<String>,
        snapshot_fingerprint: String,
        publication_fingerprint: String,
        variable_definitions: Option<Vec<VariableGraphNode>>,
        plan: CompositionGraphEvaluationPlan,
        query_diagnostics: Vec<VariableGraphQueryEvaluationDiagnostic>,
    ) -> Result<CompositionQueryEvaluationResponse> {
        let (current_authorization_fingerprint, _) = self
            .capture_graph_snapshot(
                space_id,
                principal_ids,
                composition_entry_id,
                Some(publication),
            )
            .await?;
        ensure_authorization_fingerprint_unchanged(
            expected_authorization_fingerprint,
            &current_authorization_fingerprint,
        )?;
        Ok(query_evaluation_diagnostics_response(
            evaluation_id,
            draft_fingerprint,
            snapshot_fingerprint,
            publication_fingerprint,
            current_authorization_fingerprint,
            variable_definitions,
            plan,
            query_diagnostics,
        ))
    }
}

fn query_evaluation_diagnostics_response(
    evaluation_id: Uuid,
    draft_fingerprint: Option<String>,
    snapshot_fingerprint: String,
    publication_fingerprint: String,
    authorization_fingerprint: String,
    variable_definitions: Option<Vec<VariableGraphNode>>,
    plan: CompositionGraphEvaluationPlan,
    query_diagnostics: Vec<VariableGraphQueryEvaluationDiagnostic>,
) -> CompositionQueryEvaluationResponse {
    CompositionQueryEvaluationResponse {
        evaluation_id,
        ok: false,
        draft_fingerprint,
        snapshot_fingerprint: Some(snapshot_fingerprint),
        publication_fingerprint: Some(publication_fingerprint),
        authorization_fingerprint: Some(authorization_fingerprint),
        variable_definitions,
        plan: Some(plan),
        // Never expose partial values from a failed evaluation as current data.
        query_results: BTreeMap::new(),
        computed_query_ids: Vec::new(),
        diagnostics: Vec::new(),
        query_diagnostics,
    }
}

fn apply_resolved_query_descriptors(
    graph: &mut VariableGraph,
    queries: &[ResolvedQueryRequest],
) -> Result<()> {
    let expected_ids = graph
        .variables
        .iter()
        .filter(|node| matches!(node.kind, VariableGraphNodeKind::Query { .. }))
        .map(|node| node.id.as_str())
        .collect::<BTreeSet<_>>();
    let observed_ids = queries
        .iter()
        .map(|query| query.variable_id.as_str())
        .collect::<BTreeSet<_>>();
    if !observed_ids.is_subset(&expected_ids) || observed_ids.len() != queries.len() {
        return Err(anyhow!(
            "Composition query descriptor set does not match its graph"
        ));
    }
    let by_id = queries
        .iter()
        .map(|query| (query.variable_id.as_str(), query))
        .collect::<BTreeMap<_, _>>();
    for node in &mut graph.variables {
        let VariableGraphNodeKind::Query { returns, query } = &mut node.kind else {
            continue;
        };
        let Some(descriptor) = by_id.get(node.id.as_str()) else {
            continue;
        };
        *returns = descriptor.returns.clone();
        query.form_bindings = descriptor.form_bindings.clone();
        query.schema_fingerprint = descriptor.schema_fingerprint.clone();
    }
    Ok(())
}

/// The resolver's bound snapshot intentionally omits values explicitly unset
/// by the caller. Remove those input defaults from the execution-only graph as
/// well, so the evaluator cannot silently restore them while resolving inputs.
fn remove_explicitly_unset_input_defaults(graph: &mut VariableGraph, unset_variables: &[String]) {
    let unset = unset_variables
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    for node in &mut graph.variables {
        if !unset.contains(node.id.as_str()) {
            continue;
        }
        if let VariableGraphNodeKind::Input { default, .. } = &mut node.kind {
            *default = None;
        }
    }
}

fn graph_snapshot_fingerprint(
    evaluation_id: Uuid,
    identity: &str,
    publication: &PublicationRef,
    authorization_fingerprint: &str,
    plan: &CompositionGraphEvaluationPlan,
) -> Result<String> {
    let queries = plan
        .queries
        .iter()
        .map(|query| {
            (
                &query.variable_id,
                &query.query_semantic_fingerprint,
                &query.schema_fingerprint,
                &query.form_bindings,
                &query.returns,
            )
        })
        .collect::<Vec<_>>();
    fingerprint_serializable(&(
        evaluation_id,
        identity,
        publication,
        authorization_fingerprint,
        &plan.variable_values,
        queries,
    ))
}

fn query_snapshot_fingerprint(
    graph_snapshot: &str,
    query: &ResolvedQueryRequest,
    request: &SqlQueryRequest,
) -> Result<String> {
    fingerprint_serializable(&(
        graph_snapshot,
        &query.variable_id,
        &query.query_semantic_fingerprint,
        &query.schema_fingerprint,
        &request.sql,
        &request.saved_sql,
        &request.parameters,
        &request.parameter_types,
        request.limit,
    ))
}

fn ensure_authorization_fingerprint_unchanged(expected: &str, current: &str) -> Result<()> {
    if expected != current {
        return Err(AppError::forbidden(
            "authorization changed during Composition evaluation; restart the preview",
        )
        .into());
    }
    Ok(())
}

fn ensure_publication_unchanged(expected: &PublicationRef, current: &PublicationRef) -> Result<()> {
    if expected != current {
        return Err(AppError::new(
            ErrorKind::Conflict,
            ErrorCode::RevisionConflict,
            "Composition Publication changed during descriptor resolution; restart the evaluation",
        )
        .into());
    }
    Ok(())
}

fn ensure_graph_snapshot_unchanged(
    expected_authorization_fingerprint: &str,
    expected_publication: &PublicationRef,
    current_authorization_fingerprint: &str,
    current_publication: &PublicationRef,
) -> Result<()> {
    ensure_authorization_fingerprint_unchanged(
        expected_authorization_fingerprint,
        current_authorization_fingerprint,
    )?;
    ensure_publication_unchanged(expected_publication, current_publication)
}

fn fingerprint_serializable(value: &impl serde::Serialize) -> Result<String> {
    let bytes = serde_json::to_vec(value)?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ugoite_core::composition::{CompositionScalarCardinality, ResolvedQueryRequest};
    use ugoite_core::composition_query::VariableGraphQueryRequestDiagnosticCode;
    use ugoite_core::sql_query::SqlQueryRequest;
    use ugoite_domain::composition::{
        VariableGraphArgumentBinding, VariableGraphLiteral, VariableGraphQuery,
        VariableGraphQueryDefinition, VariableGraphResultType, VariableGraphScalarDescriptor,
        VariableGraphScalarType,
    };
    use ugoite_domain::space_key::SpaceUri;

    fn publication(generation: u64) -> PublicationRef {
        PublicationRef {
            generation,
            publication_uri: SpaceUri::parse(&format!(
                "ugoite://018f6c7e-5f6a-7b8c-9d0e-1f2a3b4c5d6e/publications/{generation}"
            ))
            .expect("publication URI"),
            publication_checksum: format!("{generation:064x}"),
        }
    }

    fn resolved_query() -> ResolvedQueryRequest {
        ResolvedQueryRequest {
            variable_id: "total".to_owned(),
            request: None,
            sql_text: None,
            deferred_arguments: Vec::new(),
            returns: VariableGraphResultType::Scalar {
                value_type: VariableGraphScalarDescriptor {
                    scalar_type: VariableGraphScalarType::Integer,
                    nullable: false,
                },
            },
            scalar_cardinality: Some(CompositionScalarCardinality::ExactlyOneRow),
            result_columns: Vec::new(),
            form_bindings: Vec::new(),
            schema_fingerprint: "schema-v1".to_owned(),
            query_semantic_fingerprint: "query-v1".to_owned(),
        }
    }

    #[test]
    fn table_cursor_context_is_rejected_for_a_different_graph_snapshot() {
        let query = resolved_query();
        let request = SqlQueryRequest {
            sql: "SELECT 1".to_owned(),
            limit: 100,
            execution_id: Some(Uuid::from_u128(1)),
            ..SqlQueryRequest::default()
        };
        let original = query_snapshot_fingerprint("snapshot-v1", &query, &request)
            .expect("original cursor context");
        let changed = query_snapshot_fingerprint("snapshot-v2", &query, &request)
            .expect("changed cursor context");
        assert_ne!(original, changed);
    }

    #[test]
    fn evaluation_rejects_an_authorization_change_before_returning_results() {
        assert!(ensure_authorization_fingerprint_unchanged("acl-v1", "acl-v1").is_ok());
        let error = ensure_authorization_fingerprint_unchanged("acl-v1", "acl-v2")
            .expect_err("changed authorization must invalidate the evaluation");
        assert_eq!(
            error
                .downcast_ref::<AppError>()
                .expect("typed forbidden error")
                .kind(),
            ugoite_core::error::ErrorKind::Forbidden
        );
    }

    #[test]
    fn descriptor_resolution_rejects_a_publication_change() {
        let captured = publication(1);
        assert!(ensure_graph_snapshot_unchanged("acl-v1", &captured, "acl-v1", &captured).is_ok());

        let error = ensure_graph_snapshot_unchanged("acl-v1", &captured, "acl-v1", &publication(2))
            .expect_err("a changed Publication must invalidate descriptor resolution");
        let app_error = error.downcast_ref::<AppError>().expect("typed conflict");
        assert_eq!(app_error.kind(), ErrorKind::Conflict);
        assert_eq!(app_error.code(), ErrorCode::RevisionConflict);

        assert!(ensure_graph_snapshot_unchanged("acl-v1", &captured, "acl-v2", &captured).is_err());
    }

    #[test]
    fn explicitly_unset_input_default_does_not_bind_a_downstream_query_argument() {
        let mut graph = VariableGraph {
            variables: vec![
                VariableGraphNode {
                    id: "threshold".to_owned(),
                    kind: VariableGraphNodeKind::Input {
                        value_type: VariableGraphScalarDescriptor {
                            scalar_type: VariableGraphScalarType::Integer,
                            nullable: false,
                        },
                        required: false,
                        default: Some(VariableGraphLiteral::Integer(7)),
                    },
                },
                VariableGraphNode {
                    id: "result".to_owned(),
                    kind: VariableGraphNodeKind::Query {
                        returns: VariableGraphResultType::Scalar {
                            value_type: VariableGraphScalarDescriptor {
                                scalar_type: VariableGraphScalarType::Integer,
                                nullable: false,
                            },
                        },
                        query: VariableGraphQuery {
                            definition: VariableGraphQueryDefinition::InlineSql {
                                sql: "SELECT $threshold AS value".to_owned(),
                            },
                            arguments: vec![VariableGraphArgumentBinding {
                                name: "threshold".to_owned(),
                                variable: "threshold".to_owned(),
                            }],
                            form_bindings: Vec::new(),
                            schema_fingerprint: String::new(),
                        },
                    },
                },
            ],
        };
        remove_explicitly_unset_input_defaults(&mut graph, &["threshold".to_owned()]);

        let evaluation = VariableGraphQueryEvaluation::new(
            &graph,
            Uuid::from_u128(0x554e534554),
            &BTreeMap::new(),
        )
        .expect("an optional unset input is a valid snapshot value");
        let diagnostics = evaluation
            .compile_request("result")
            .expect_err("the SQL argument must remain unbound after explicit unset");
        assert_eq!(
            diagnostics[0]
                .request_diagnostic
                .as_ref()
                .expect("missing SQL argument diagnostic")
                .code,
            VariableGraphQueryRequestDiagnosticCode::MissingArgumentValue
        );
    }

    #[tokio::test]
    async fn diagnostics_response_rechecks_authorization_after_query_execution() -> Result<()> {
        let service = UgoiteService::new(format!(
            "memory://composition-evaluation-acl-recheck-{}",
            Uuid::now_v7()
        ))?;
        let principal = Uuid::now_v7();
        let space_id = service
            .create_space_for_principal("evaluation-acl-recheck", principal, "Owner")
            .await?
            .to_string();
        let (current_authorization_fingerprint, publication) = service
            .capture_authorized_graph_snapshot(&space_id, &[principal], None, None)
            .await?;
        let stale_authorization_fingerprint = format!("stale-{current_authorization_fingerprint}");
        let diagnostic = VariableGraphQueryEvaluationDiagnostic {
            code: ugoite_core::composition_query::VariableGraphQueryEvaluationDiagnosticCode::ResultValidationFailed,
            variable_id: "total".to_owned(),
            request_diagnostic: None,
            result_diagnostic: None,
        };
        let response = service
            .query_evaluation_diagnostics_response_for_access(
                &space_id,
                Some(&[principal]),
                None,
                &publication,
                &stale_authorization_fingerprint,
                Uuid::now_v7(),
                None,
                "snapshot".to_owned(),
                "publication".to_owned(),
                None,
                CompositionGraphEvaluationPlan {
                    composition_revision: None,
                    draft_fingerprint: None,
                    variable_values: BTreeMap::new(),
                    queries: Vec::new(),
                    component_bindings: Vec::new(),
                    dependencies: CompositionDependencyIndex::default(),
                },
                vec![diagnostic],
            )
            .await;
        let error = response.expect_err("stale ACL snapshot must not return query diagnostics");
        assert_eq!(
            error
                .downcast_ref::<AppError>()
                .expect("typed forbidden error")
                .kind(),
            ugoite_core::error::ErrorKind::Forbidden
        );
        Ok(())
    }
}
