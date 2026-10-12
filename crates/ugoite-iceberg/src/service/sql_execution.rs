//! Shared preparation for ordinary Saved SQL execution adapters.

use super::*;
use datafusion::scalar::ScalarValue;
use std::collections::HashMap;
use ugoite_domain::form::FormDefinition;

pub(super) struct PreparedSqlRequest {
    pub(super) saved_revision: Option<saved_sql::ResolvedSavedSqlRevision>,
    pub(super) normalized_sql: String,
    pub(super) parameter_types: BTreeMap<String, String>,
    pub(super) parameters: HashMap<String, ScalarValue>,
}

#[derive(Clone, Copy)]
pub(super) struct SqlSourceAuthorization<'a> {
    pub(super) state: &'a AuthorizationState,
    pub(super) principal_ids: &'a [Uuid],
}

/// Resolve and validate one exact Saved SQL revision before an adapter selects
/// its publication, result shape, and transport-specific execution limits.
pub(super) async fn prepare_sql_request(
    service: &UgoiteService,
    space_id: &str,
    request: &SqlQueryRequest,
    authorization: Option<SqlSourceAuthorization<'_>>,
) -> Result<PreparedSqlRequest> {
    if let (Some(authorization), Some(source)) = (authorization, request.saved_sql.as_ref()) {
        ensure_saved_sql_access(authorization.state, authorization.principal_ids, source)?;
    }

    let saved_revision = service
        .read_saved_sql_query_revision(space_id, request.saved_sql.as_ref())
        .await?;
    let query_sql = saved_revision
        .as_ref()
        .map_or(request.sql.as_str(), |revision| revision.sql.as_str());
    let normalized_sql = index::normalize_sql_template(query_sql)?;
    validate_saved_sql_client_sql(saved_revision.is_some(), &request.sql, &normalized_sql)?;
    index::validate_read_only_sql(&normalized_sql)?;

    let parameter_types = effective_sql_parameter_types(
        &request.parameters,
        &saved_sql_parameter_types(saved_revision.as_ref(), &request.parameter_types)?,
    )?;
    let parameters = index::datafusion_parameters(&request.parameters, &parameter_types)
        .map_err(|error| AppError::invalid_input(ErrorCode::InvalidInput, error.to_string()))?;

    Ok(PreparedSqlRequest {
        saved_revision,
        normalized_sql,
        parameter_types,
        parameters,
    })
}

pub(super) fn all_current_relation_scopes(
    forms: &[FormDefinition],
) -> BTreeMap<String, EntryScope> {
    forms
        .iter()
        .map(|form| (sql_relation_name(form.id), EntryScope::AllCurrent))
        .collect()
}

pub(super) fn authorized_relation_scopes(
    state: &AuthorizationState,
    principal_ids: &[Uuid],
    forms: &[FormDefinition],
) -> Result<BTreeMap<String, EntryScope>> {
    let named_scopes = UgoiteService::authorized_form_entry_scopes_for_forms(
        state,
        principal_ids,
        forms.to_vec(),
    )?;
    Ok(forms
        .iter()
        .filter_map(|form| {
            named_scopes
                .get(&form.name.to_ascii_lowercase())
                .cloned()
                .map(|scope| (sql_relation_name(form.id), scope))
        })
        .collect())
}

pub(super) fn bind_sql_to_forms(
    prepared: &PreparedSqlRequest,
    forms: &[FormDefinition],
    relation_scopes: &BTreeMap<String, EntryScope>,
) -> Result<String> {
    match prepared.saved_revision.as_ref() {
        Some(revision) => {
            bound_sql_for_revision(revision, forms, relation_scopes, &prepared.normalized_sql)
        }
        None => Ok(prepared.normalized_sql.clone()),
    }
}
