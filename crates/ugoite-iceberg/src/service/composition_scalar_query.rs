//! Stateless scalar SQL probes used by inline Composition queries and the
//! standalone Saved SQL API.

use super::*;
use crate::index;
use ugoite_core::query::QueryLimits;
use ugoite_core::sql_query::{SqlQueryRequest, SqlScalarProbeResult};

struct AuthorizedScalarProbeOptions<'a> {
    query_limits_override: Option<&'a QueryLimits>,
    allow_inline_sql: bool,
    publication_override: Option<&'a PublicationRef>,
    authorization_state: Option<&'a AuthorizationState>,
}

impl UgoiteService {
    /// Probe an exact Saved SQL revision through the local operator boundary.
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

    /// Probe inline SQL at a selected published Space snapshot.
    pub(super) async fn probe_composition_sql_scalar_local_at_publication(
        &self,
        space_id: &str,
        request: SqlQueryRequest,
        publication_override: Option<&PublicationRef>,
    ) -> Result<SqlScalarProbeResult> {
        validate_scalar_probe_request_with_inline(&request, true)?;
        let prepared = sql_execution::prepare_sql_request(self, space_id, &request, None).await?;
        self.validate_complete_space(space_id).await?;
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        let publication = match publication_override {
            Some(publication) => publication.clone(),
            None => workspace.current_publication().await?,
        };
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
            AuthorizedScalarProbeOptions {
                query_limits_override: None,
                allow_inline_sql: false,
                publication_override: None,
                authorization_state: None,
            },
        )
        .await
    }

    /// Probe inline SQL for a Composition while checking the current access
    /// state and executing against the requested publication.
    pub(super) async fn probe_composition_sql_scalar_authorized_at_publication(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        composition_id: Option<&str>,
        request: SqlQueryRequest,
        publication: &PublicationRef,
    ) -> Result<SqlScalarProbeResult> {
        self.probe_sql_scalar_authorized_with_composition_access(
            space_id,
            principal_ids,
            composition_id,
            request,
            AuthorizedScalarProbeOptions {
                query_limits_override: None,
                allow_inline_sql: true,
                publication_override: Some(publication),
                authorization_state: None,
            },
        )
        .await
    }

    /// Probe inline SQL using a caller-held authorization snapshot.
    pub(super) async fn probe_composition_sql_scalar_authorized_at_publication_with_state(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        composition_id: Option<&str>,
        request: SqlQueryRequest,
        publication: &PublicationRef,
        authorization_state: &AuthorizationState,
    ) -> Result<SqlScalarProbeResult> {
        self.probe_sql_scalar_authorized_with_composition_access(
            space_id,
            principal_ids,
            composition_id,
            request,
            AuthorizedScalarProbeOptions {
                query_limits_override: None,
                allow_inline_sql: true,
                publication_override: Some(publication),
                authorization_state: Some(authorization_state),
            },
        )
        .await
    }

    async fn probe_sql_scalar_authorized_with_composition_access(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        composition_id: Option<&str>,
        request: SqlQueryRequest,
        options: AuthorizedScalarProbeOptions<'_>,
    ) -> Result<SqlScalarProbeResult> {
        validate_scalar_probe_request_with_inline(&request, options.allow_inline_sql)?;
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        let (owned_state, _authorization_lease) = match options.authorization_state {
            Some(_) => (None, None),
            None => {
                let (state, lease) = Authorizer::new(self.operator.clone())
                    .acquire_state_lease(space_id)
                    .await?;
                (Some(state), Some(lease))
            }
        };
        let state = options
            .authorization_state
            .or(owned_state.as_ref())
            .expect("authorized scalar probe has authorization state");
        for principal_id in principal_ids {
            if !effective_actions_for_state(state, *principal_id, None)?.contains(&Action::Read) {
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
                state,
                principal_ids,
            }),
        )
        .await?;
        self.validate_complete_space(space_id).await?;
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        let publication = match options.publication_override {
            Some(publication) => publication.clone(),
            None => workspace.current_publication().await?,
        };
        let checkpoint = workspace.resolve_publication(&publication).await?;
        let forms = workspace.forms_at_checkpoint(&checkpoint).await?;
        if let Some(composition_id) = composition_id {
            let composition_scopes =
                Self::authorized_form_entry_scopes_for_forms(state, principal_ids, forms.clone())?;
            require_composition_entry_read_scope(
                &composition_scopes,
                parse_entry_id(composition_id)?,
                composition_id,
            )?;
        }
        let relation_scopes =
            sql_execution::authorized_relation_scopes(state, principal_ids, &forms)?;
        let execution_sql = sql_execution::bind_sql_to_forms(&prepared, &forms, &relation_scopes)?;
        let parameters = prepared.parameters;
        let (columns, result_schema, rows) = if let Some(query_limits) =
            options.query_limits_override
        {
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
