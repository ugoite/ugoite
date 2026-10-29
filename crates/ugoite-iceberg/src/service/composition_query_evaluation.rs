//! Stateless execution of a typed Composition query graph.
//!
//! Every request resolves the exact Composition target and current query
//! descriptors again. SQL results live only for the duration of this request;
//! table continuations are signed SQL cursors additionally scoped to the
//! exact query snapshot and variable. Unrelated input changes can reuse a
//! cursor; query arguments, authority, publication, and source revisions
//! remain bound to the continuation.

use anyhow::{anyhow, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
#[cfg(debug_assertions)]
use std::sync::{Arc, LazyLock, Mutex};
#[cfg(debug_assertions)]
use tokio::sync::Notify;
use ugoite_core::composition::{
    CompositionDependencyIndex, CompositionDiagnostic, CompositionRevisionRef,
    ResolvedComponentBinding, ResolvedQueryRequest,
};
use ugoite_core::composition_query::{
    VariableGraphQueryEvaluation, VariableGraphQueryEvaluationDiagnostic,
    VariableGraphQueryEvaluationDiagnosticCode,
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

use super::composition_resolver::{CompositionPreviewOptions, CompositionQuerySelection};
use super::{
    effective_actions_for_state, require_nonempty_authorized_principals,
    sql_query_authorization_fingerprint, sql_query_contract_error, validate_storage_id, Authorizer,
    PublicationRef, UgoiteService,
};
use crate::authorization::AuthorizationState;
use crate::iceberg_store;

/// Debug-only rendezvous after graph query execution and immediately before
/// the final authorization reread. This lets service tests deterministically
/// commit an ACL change in the exact interval being checked.
#[cfg(debug_assertions)]
#[doc(hidden)]
#[derive(Debug)]
pub struct TestCompositionFinalAuthorizationGate {
    space_id: String,
    entered: Notify,
    release: Notify,
}

#[cfg(debug_assertions)]
impl TestCompositionFinalAuthorizationGate {
    pub fn new(space_id: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            space_id: space_id.into(),
            entered: Notify::new(),
            release: Notify::new(),
        })
    }

    pub async fn wait_until_entered(&self) {
        self.entered.notified().await;
    }

    pub fn release(&self) {
        self.release.notify_one();
    }

    async fn pause(&self) {
        self.entered.notify_one();
        self.release.notified().await;
    }
}

#[cfg(debug_assertions)]
static TEST_FINAL_AUTHORIZATION_GATE: LazyLock<
    Mutex<Option<Arc<TestCompositionFinalAuthorizationGate>>>,
> = LazyLock::new(|| Mutex::new(None));

#[cfg(debug_assertions)]
#[doc(hidden)]
pub fn install_test_composition_final_authorization_gate(
    gate: Arc<TestCompositionFinalAuthorizationGate>,
) {
    *TEST_FINAL_AUTHORIZATION_GATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(gate);
}

#[cfg(debug_assertions)]
#[doc(hidden)]
pub fn clear_test_composition_final_authorization_gate() {
    *TEST_FINAL_AUTHORIZATION_GATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

#[cfg(debug_assertions)]
async fn wait_at_test_composition_final_authorization_gate(space_id: &str) {
    let gate = TEST_FINAL_AUTHORIZATION_GATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    if let Some(gate) = gate.filter(|gate| gate.space_id == space_id) {
        gate.pause().await;
    }
}

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

/// Current server-owned authority coordinates for a saved Composition
/// revision. This check is read-only and never executes query SQL.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompositionAccessSnapshot {
    pub composition_entry_id: String,
    pub composition_revision_id: String,
    pub publication_fingerprint: String,
    pub authorization_fingerprint: String,
}

impl UgoiteService {
    /// Recheck the authority snapshot for an exact saved Composition without
    /// resolving or executing its query variables. A caller may retain prior
    /// transient results only while both returned fingerprints remain equal
    /// to the fingerprints from the last successful full evaluation.
    pub async fn check_composition_access_snapshot_for_principals(
        &self,
        space_id: &str,
        composition_id: &str,
        revision_id: &str,
        principal_ids: &[Uuid],
    ) -> Result<CompositionAccessSnapshot> {
        let before = self
            .capture_graph_snapshot(
                space_id,
                Some(principal_ids),
                Some(composition_id),
                None,
                None,
            )
            .await?;
        let raw = self
            .get_composition_raw_revision_authorized_for_principals(
                space_id,
                composition_id,
                revision_id,
                principal_ids,
            )
            .await?;
        // The stored revision is immutable. Strictly parse it here so this
        // operation cannot turn an old or malformed graph into an authorized
        // current Composition snapshot.
        let spec = raw
            .fields
            .get("spec")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("Composition revision has no spec"))?;
        ugoite_domain::composition::parse_composition_yaml(spec)
            .map_err(|_| anyhow!("Composition revision is not a supported graph"))?;
        if raw.revision.entry_id.to_string() != composition_id
            || raw.revision.revision_id.to_string() != revision_id
        {
            return Err(anyhow!(
                "Composition snapshot revision does not match the requested revision"
            ));
        }
        let after = self
            .capture_graph_snapshot(
                space_id,
                Some(principal_ids),
                Some(composition_id),
                None,
                None,
            )
            .await?;
        ensure_graph_snapshot_unchanged(&before.0, &before.1, &after.0, &after.1)?;
        Ok(CompositionAccessSnapshot {
            composition_entry_id: composition_id.to_owned(),
            composition_revision_id: revision_id.to_owned(),
            publication_fingerprint: fingerprint_serializable(&after.1)?,
            authorization_fingerprint: after.0,
        })
    }

    pub(super) async fn preflight_composition_save_authorized(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        yaml: &str,
        variables: &BTreeMap<String, Value>,
        unset_variables: &[String],
        authorization_state: &AuthorizationState,
    ) -> Result<crate::ExactCatalogHeadProof> {
        self.preflight_composition_save_with_access(
            space_id,
            Some(principal_ids),
            yaml,
            variables,
            unset_variables,
            Some(authorization_state),
        )
        .await
    }

    pub(super) async fn preflight_composition_save_local(
        &self,
        space_id: &str,
        yaml: &str,
        variables: &BTreeMap<String, Value>,
        unset_variables: &[String],
    ) -> Result<crate::ExactCatalogHeadProof> {
        self.preflight_composition_save_with_access(
            space_id,
            None,
            yaml,
            variables,
            unset_variables,
            None,
        )
        .await
    }

    async fn preflight_composition_save_with_access(
        &self,
        space_id: &str,
        principal_ids: Option<&[Uuid]>,
        yaml: &str,
        variables: &BTreeMap<String, Value>,
        unset_variables: &[String],
        authorization_state: Option<&AuthorizationState>,
    ) -> Result<crate::ExactCatalogHeadProof> {
        let workspace = iceberg_store::native_workspace_read_only(
            &self.operator,
            &self.workspace_path(space_id),
        )
        .await?;
        let (bound_workspace, proof) = workspace.bind_exact_catalog_head().await?;
        let publication = bound_workspace.bound_publication().await?;
        if publication.is_none() {
            self.preflight_unpublished_composition_queries(
                space_id,
                principal_ids,
                yaml,
                variables,
                unset_variables,
                authorization_state,
                &bound_workspace,
            )
            .await?;
            return Ok(proof);
        }
        let response = self
            .evaluate_composition_graph(
                space_id,
                principal_ids,
                CompositionQueryEvaluationTarget::Draft {
                    yaml: yaml.to_owned(),
                },
                Uuid::new_v4(),
                None,
                None,
                variables,
                unset_variables,
                None,
                None,
                authorization_state,
                publication,
            )
            .await?;
        let plan = response.plan.as_ref();
        if !response.ok
            || plan.is_none()
            || response.query_results.len() != plan.map_or(0, |plan| plan.queries.len())
            || response.computed_query_ids.len() != plan.map_or(0, |plan| plan.queries.len())
        {
            let detail = response
                .diagnostics
                .iter()
                .map(|diagnostic| format!("{:?} at {:?}", diagnostic.code, diagnostic.variable_id))
                .chain(response.query_diagnostics.iter().map(|diagnostic| {
                    format!("{:?} at {}", diagnostic.code, diagnostic.variable_id)
                }))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(AppError::invalid_input(
                ErrorCode::InvalidInput,
                format!("Composition save preflight failed: {detail}"),
            )
            .into());
        }
        let plan = response.plan.as_ref().expect("validated preflight plan");
        Self::ensure_declared_query_returns_match(yaml, &plan.queries)?;
        Ok(proof)
    }

    // This boundary threads one complete save-evaluation context into the
    // resolver without reconstructing authorization or Work state.
    #[allow(clippy::too_many_arguments)]
    async fn preflight_unpublished_composition_queries(
        &self,
        space_id: &str,
        principal_ids: Option<&[Uuid]>,
        yaml: &str,
        variables: &BTreeMap<String, Value>,
        unset_variables: &[String],
        authorization_state: Option<&AuthorizationState>,
        workspace: &crate::IcebergWorkspace,
    ) -> Result<()> {
        let preview = self
            .preview_composition_for_save_with_access(
                space_id,
                yaml,
                variables,
                unset_variables,
                principal_ids,
                authorization_state,
                None,
            )
            .await?;
        if !preview.diagnostics.is_empty() || preview.plan.is_none() {
            let detail = preview
                .diagnostics
                .iter()
                .map(|diagnostic| format!("{:?} at {:?}", diagnostic.code, diagnostic.variable_id))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(AppError::invalid_input(
                ErrorCode::InvalidInput,
                format!("Composition save preflight failed: {detail}"),
            )
            .into());
        }
        let plan = preview.plan.expect("checked Composition preview plan");
        Self::ensure_declared_query_returns_match(yaml, &plan.queries)?;
        let definitions = preview
            .variable_definitions
            .ok_or_else(|| anyhow!("Composition preview did not return variable definitions"))?;
        let mut graph = VariableGraph {
            variables: definitions,
        };
        apply_resolved_query_descriptors(&mut graph, &plan.queries)?;
        remove_explicitly_unset_input_defaults(&mut graph, unset_variables);
        for node in &graph.variables {
            let VariableGraphNodeKind::Query { query, .. } = &node.kind else {
                continue;
            };
            let ugoite_domain::composition::VariableGraphQueryDefinition::InlineSql { sql } =
                &query.definition
            else {
                return Err(AppError::invalid_input(
                    ErrorCode::InvalidInput,
                    format!("Query {} requires a published Space snapshot", node.id),
                )
                .into());
            };
            if !crate::index::quoted_form_name_references(sql)?.is_empty()
                || !crate::index::quoted_form_relation_names(sql)?.is_empty()
            {
                return Err(AppError::invalid_input(
                    ErrorCode::InvalidInput,
                    format!("Query {} requires a published Form snapshot", node.id),
                )
                .into());
            }
        }
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
        let evaluation_id = Uuid::new_v4();
        let mut evaluation = VariableGraphQueryEvaluation::new_for_queries(
            &graph,
            evaluation_id,
            &input_values,
            None,
        )
        .map_err(|diagnostics| {
            AppError::invalid_input(
                ErrorCode::InvalidInput,
                format!("Composition query planning failed: {diagnostics:?}"),
            )
        })?;
        let context = workspace
            .authorized_query_context(ugoite_core::query::AuthorizedQueryPolicy {
                forms: BTreeMap::new(),
                checkpoint: None,
                limits: ugoite_core::query::QueryLimits {
                    max_memory_bytes: crate::index::SQL_MAX_MEMORY_BYTES,
                    max_rows: crate::index::SQL_MAX_OUTPUT_ROWS,
                    timeout: std::time::Duration::from_secs(30),
                    max_concurrency: 1,
                    function_policy: ugoite_core::query::SqlFunctionPolicy::ReadOnlyAggregateV1,
                },
            })
            .await?;
        for variable_id in evaluation.selected_query_ids().to_vec() {
            let request = evaluation
                .compile_page_request(&variable_id, None)
                .map_err(|diagnostics| {
                    AppError::invalid_input(
                        ErrorCode::InvalidInput,
                        format!("Query {variable_id} request is invalid: {diagnostics:?}"),
                    )
                })?;
            let prepared = super::sql_execution::prepare_sql_request(
                self,
                space_id,
                &request,
                match (principal_ids, authorization_state) {
                    (Some(principal_ids), Some(state)) => {
                        Some(super::sql_execution::SqlSourceAuthorization {
                            state,
                            principal_ids,
                        })
                    }
                    _ => None,
                },
            )
            .await?;
            let result_schema = context
                .stateless_result_schema(&prepared.normalized_sql, prepared.parameters.clone())
                .await?;
            let stateless_page = context
                .execute_stateless_page(
                    &prepared.normalized_sql,
                    prepared.parameters,
                    0,
                    request.limit.saturating_add(1),
                )
                .await?;
            let (mut rows, mut exact_integer_rows) =
                crate::index::serialize_stateless_sql_page_for_validation(
                    &stateless_page,
                    request.include_exact_integer_values,
                )?;
            let has_more = rows.len() > request.limit;
            rows.truncate(request.limit);
            if let Some(exact_rows) = exact_integer_rows.as_mut() {
                exact_rows.truncate(request.limit);
            }
            let page = SqlQueryPage {
                columns: stateless_page.columns,
                rows,
                has_more,
                next: has_more.then(|| "strict-save-more".to_owned()),
                execution_id: request.execution_id,
                request_fingerprint: Some(
                    request
                        .execution_fingerprint()
                        .map_err(sql_query_contract_error)?,
                ),
                result_schema: Some(result_schema),
                exact_integer_rows,
            };
            evaluation
                .accept_page(&variable_id, &request, &page)
                .map_err(|diagnostics| {
                    AppError::invalid_input(
                        ErrorCode::InvalidInput,
                        format!("Query {variable_id} result is invalid: {diagnostics:?}"),
                    )
                })?;
        }
        Ok(())
    }

    fn ensure_declared_query_returns_match(
        yaml: &str,
        queries: &[ResolvedQueryRequest],
    ) -> Result<()> {
        let document = ugoite_domain::composition::parse_composition_yaml(yaml)
            .map_err(|diagnostic| anyhow!(diagnostic.as_str()))?;
        let declared = document
            .spec
            .variables
            .iter()
            .filter_map(|node| match &node.kind {
                VariableGraphNodeKind::Query { returns, .. } => Some((node.id.as_str(), returns)),
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();
        if queries.len() != declared.len() {
            return Err(AppError::invalid_input(
                ErrorCode::InvalidInput,
                "Composition save preflight did not resolve every declared query",
            )
            .into());
        }
        for query in queries {
            let Some(expected) = declared.get(query.variable_id.as_str()) else {
                return Err(AppError::invalid_input(
                    ErrorCode::InvalidInput,
                    format!(
                        "Composition save preflight resolved unknown query {}",
                        query.variable_id
                    ),
                )
                .into());
            };
            if **expected != query.returns {
                return Err(AppError::invalid_input(
                    ErrorCode::InvalidInput,
                    format!(
                        "Composition query {} return descriptor does not match its resolved SQL schema",
                        query.variable_id
                    ),
                )
                .into());
            }
        }
        Ok(())
    }

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
            None,
            None,
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
            None,
            None,
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
        authorization_state: Option<&AuthorizationState>,
        pinned_publication_override: Option<PublicationRef>,
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
                        "Composition page cursor is missing its query snapshot binding",
                    )
                    .into());
                }
                Some(cursor)
            }
            None => None,
        };
        let pinned_publication = decoded_cursor
            .as_ref()
            .map(|cursor| cursor.publication.clone())
            .or(pinned_publication_override);
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
                authorization_state,
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
                            CompositionQuerySelection {
                                variables,
                                unset_variables,
                                principal_ids,
                                requested_queries,
                            },
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
                            CompositionPreviewOptions {
                                variables,
                                unset_variables,
                                principal_ids: Some(principal_ids),
                                base,
                                requested_queries,
                                allow_unset_inputs: false,
                                authorization_state,
                                publication_override: Some(&publication),
                            },
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
            authorization_state,
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

        let mut query_diagnostics = Vec::new();
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
                Err(diagnostics) => {
                    query_diagnostics.extend(diagnostics);
                    continue;
                }
            };
            let query_context = query_snapshot_fingerprint(
                &identity,
                &publication,
                &authorization_fingerprint,
                query,
                &request,
            )?;
            let page_result = if matches!(query.returns, VariableGraphResultType::Scalar { .. }) {
                let probe = match principal_ids {
                    Some(principal_ids) => {
                        if let Some(state) = authorization_state {
                            self.probe_composition_sql_scalar_authorized_at_publication_with_state(
                                space_id,
                                principal_ids,
                                composition_entry_id.as_deref(),
                                request.clone(),
                                &publication,
                                state,
                            )
                            .await?
                        } else {
                            self.probe_composition_sql_scalar_authorized_at_publication(
                                space_id,
                                principal_ids,
                                composition_entry_id.as_deref(),
                                request.clone(),
                                &publication,
                            )
                            .await?
                        }
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
                        if let Some(state) = authorization_state {
                            self.query_sql_authorized_for_composition_with_state(
                                space_id,
                                principal_ids,
                                request.clone(),
                                composition_entry_id.as_deref(),
                                &publication,
                                &query_context,
                                state,
                            )
                            .await?
                        } else {
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
            if let Err(diagnostics) = evaluation.accept_page(variable_id, &request, &page_result) {
                query_diagnostics.extend(diagnostics);
            }
        }

        // One selected dependency closure is the result boundary of a run.
        // Keep every staged value/cursor private when any selected query fails.
        let preview = if query_diagnostics.is_empty() {
            match evaluation.preview() {
                Ok(preview) => Some(preview),
                Err(diagnostics) => {
                    query_diagnostics = diagnostics;
                    None
                }
            }
        } else {
            None
        };
        if preview.is_none() {
            let failed_ids = query_diagnostics
                .iter()
                .map(|diagnostic| diagnostic.variable_id.clone())
                .collect::<BTreeSet<_>>();
            for variable_id in &computed_query_ids {
                if !failed_ids.contains(variable_id) {
                    query_diagnostics.push(VariableGraphQueryEvaluationDiagnostic::for_variable(
                        VariableGraphQueryEvaluationDiagnosticCode::EvaluationAborted,
                        variable_id,
                    ));
                }
            }
        }
        query_diagnostics.sort();
        query_diagnostics.dedup();
        #[cfg(debug_assertions)]
        if preview.is_some() {
            wait_at_test_composition_final_authorization_gate(space_id).await;
        }
        let (current_authorization_fingerprint, _) = self
            .capture_graph_snapshot(
                space_id,
                principal_ids,
                base_acl_entry.as_deref(),
                Some(&publication),
                authorization_state,
            )
            .await?;
        ensure_authorization_fingerprint_unchanged(
            &authorization_fingerprint,
            &current_authorization_fingerprint,
        )?;
        let query_results = preview
            .map(|preview| preview.query_results)
            .unwrap_or_default()
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
            ok: query_diagnostics.is_empty(),
            draft_fingerprint,
            snapshot_fingerprint: Some(snapshot_fingerprint),
            publication_fingerprint: Some(publication_fingerprint),
            authorization_fingerprint: Some(current_authorization_fingerprint),
            variable_definitions,
            plan: Some(plan),
            query_results,
            computed_query_ids,
            diagnostics: Vec::new(),
            query_diagnostics,
        })
    }

    async fn capture_graph_snapshot(
        &self,
        space_id: &str,
        principal_ids: Option<&[Uuid]>,
        composition_entry_id: Option<&str>,
        pinned_publication: Option<&PublicationRef>,
        authorization_state: Option<&AuthorizationState>,
    ) -> Result<(String, PublicationRef)> {
        match principal_ids {
            Some(principal_ids) => {
                self.capture_authorized_graph_snapshot(
                    space_id,
                    principal_ids,
                    composition_entry_id,
                    pinned_publication,
                    authorization_state,
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
        authorization_state: Option<&AuthorizationState>,
    ) -> Result<(String, PublicationRef)> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        let owned_state;
        let state = if let Some(state) = authorization_state {
            state
        } else {
            owned_state = Authorizer::new(self.operator.clone())
                .state(space_id)
                .await?;
            &owned_state
        };
        for principal_id in principal_ids {
            if !effective_actions_for_state(state, *principal_id, None)?.contains(&Action::Read) {
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
            let scopes = Self::authorized_form_entry_scopes_for_forms(state, principal_ids, forms)?;
            super::require_composition_entry_read_scope(
                &scopes,
                super::parse_entry_id(composition_entry_id)?,
                composition_entry_id,
            )?;
        }
        let authorization_fingerprint = sql_query_authorization_fingerprint(state, principal_ids)?;
        Ok((authorization_fingerprint, publication))
    }

    async fn ensure_graph_resolution_snapshot_unchanged(
        &self,
        space_id: &str,
        principal_ids: Option<&[Uuid]>,
        composition_entry_id: Option<&str>,
        expected_authorization_fingerprint: &str,
        expected_publication: &PublicationRef,
        authorization_state: Option<&AuthorizationState>,
    ) -> Result<()> {
        let (current_authorization_fingerprint, current_publication) = self
            .capture_graph_snapshot(
                space_id,
                principal_ids,
                composition_entry_id,
                None,
                authorization_state,
            )
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
                None,
            )
            .await?;
        ensure_authorization_fingerprint_unchanged(
            expected_authorization_fingerprint,
            &current_authorization_fingerprint,
        )?;
        Ok(query_evaluation_diagnostics_response(
            QueryEvaluationDiagnosticsResponse {
                evaluation_id,
                draft_fingerprint,
                snapshot_fingerprint,
                publication_fingerprint,
                authorization_fingerprint: current_authorization_fingerprint,
                variable_definitions,
                plan,
                query_diagnostics,
            },
        ))
    }
}

struct QueryEvaluationDiagnosticsResponse {
    evaluation_id: Uuid,
    draft_fingerprint: Option<String>,
    snapshot_fingerprint: String,
    publication_fingerprint: String,
    authorization_fingerprint: String,
    variable_definitions: Option<Vec<VariableGraphNode>>,
    plan: CompositionGraphEvaluationPlan,
    query_diagnostics: Vec<VariableGraphQueryEvaluationDiagnostic>,
}

fn query_evaluation_diagnostics_response(
    response: QueryEvaluationDiagnosticsResponse,
) -> CompositionQueryEvaluationResponse {
    CompositionQueryEvaluationResponse {
        evaluation_id: response.evaluation_id,
        ok: false,
        draft_fingerprint: response.draft_fingerprint,
        snapshot_fingerprint: Some(response.snapshot_fingerprint),
        publication_fingerprint: Some(response.publication_fingerprint),
        authorization_fingerprint: Some(response.authorization_fingerprint),
        variable_definitions: response.variable_definitions,
        plan: Some(response.plan),
        // This path handles graph/input failures before query execution starts.
        query_results: BTreeMap::new(),
        computed_query_ids: Vec::new(),
        diagnostics: Vec::new(),
        query_diagnostics: response.query_diagnostics,
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
    composition_identity: &str,
    publication: &PublicationRef,
    authorization_fingerprint: &str,
    query: &ResolvedQueryRequest,
    request: &SqlQueryRequest,
) -> Result<String> {
    fingerprint_serializable(&(
        "composition-query-page-v1",
        composition_identity,
        publication,
        authorization_fingerprint,
        &query.variable_id,
        &query.query_semantic_fingerprint,
        &query.schema_fingerprint,
        &query.form_bindings,
        &query.returns,
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
            portable_sql: None,
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
    fn table_cursor_context_is_scoped_to_the_query_snapshot() {
        let query = resolved_query();
        let request = SqlQueryRequest {
            form_binding_guards: Vec::new(),
            sql: "SELECT 1".to_owned(),
            limit: 100,
            execution_id: Some(Uuid::from_u128(1)),
            ..SqlQueryRequest::default()
        };
        let pinned_publication = publication(1);
        let original = query_snapshot_fingerprint(
            "composition/revision-v1",
            &pinned_publication,
            "authorization-v1",
            &query,
            &request,
        )
        .expect("original query cursor context");
        let same_query = query_snapshot_fingerprint(
            "composition/revision-v1",
            &pinned_publication,
            "authorization-v1",
            &query,
            &request,
        )
        .expect("same query cursor context");
        assert_eq!(original, same_query);

        let mut changed_request = request.clone();
        changed_request
            .parameters
            .insert("minimum".to_owned(), serde_json::Value::from(2));
        let changed_parameters = query_snapshot_fingerprint(
            "composition/revision-v1",
            &pinned_publication,
            "authorization-v1",
            &query,
            &changed_request,
        )
        .expect("changed query parameters");
        assert_ne!(original, changed_parameters);

        let changed_revision = query_snapshot_fingerprint(
            "composition/revision-v2",
            &pinned_publication,
            "authorization-v1",
            &query,
            &request,
        )
        .expect("changed composition revision");
        assert_ne!(original, changed_revision);

        let changed_publication = query_snapshot_fingerprint(
            "composition/revision-v1",
            &publication(2),
            "authorization-v1",
            &query,
            &request,
        )
        .expect("changed publication");
        assert_ne!(original, changed_publication);

        let changed_authorization = query_snapshot_fingerprint(
            "composition/revision-v1",
            &pinned_publication,
            "authorization-v2",
            &query,
            &request,
        )
        .expect("changed authorization");
        assert_ne!(original, changed_authorization);
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
            .capture_authorized_graph_snapshot(&space_id, &[principal], None, None, None)
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
