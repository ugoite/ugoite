//! Shared preparation for ordinary Saved SQL execution adapters.

use super::*;
use datafusion::scalar::ScalarValue;
use std::collections::HashMap;
use ugoite_domain::form::FormDefinition;

pub(super) struct PreparedSqlRequest {
    pub(super) saved_revision: Option<saved_sql::ResolvedSavedSqlRevision>,
    pub(super) binding_base: Option<saved_sql::ResolvedSavedSqlRevision>,
    pub(super) normalized_sql: String,
    pub(super) parameter_types: BTreeMap<String, String>,
    pub(super) parameters: HashMap<String, ScalarValue>,
    pub(super) form_binding_guards: Vec<ugoite_core::sql_query::SqlFormBindingGuard>,
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
    if let (Some(authorization), Some(source)) = (authorization, request.binding_base.as_ref()) {
        ensure_saved_sql_access(authorization.state, authorization.principal_ids, source)?;
    }

    let saved_revision = service
        .read_saved_sql_query_revision(space_id, request.saved_sql.as_ref())
        .await?;
    let binding_base = service
        .read_saved_sql_query_revision(space_id, request.binding_base.as_ref())
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
        binding_base,
        normalized_sql,
        parameter_types,
        parameters,
        form_binding_guards: request.form_binding_guards.clone(),
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
        None => bind_human_form_names(
            &prepared.normalized_sql,
            &prepared.form_binding_guards,
            prepared
                .binding_base
                .as_ref()
                .map_or(&[], |base| base.bindings.as_slice()),
            forms,
            relation_scopes,
        ),
    }
}

pub(super) fn bind_human_form_names(
    sql: &str,
    guards: &[ugoite_core::sql_query::SqlFormBindingGuard],
    base_bindings: &[saved_sql::SqlFormBinding],
    forms: &[FormDefinition],
    relation_scopes: &BTreeMap<String, EntryScope>,
) -> Result<String> {
    let base_by_name = base_bindings
        .iter()
        .map(|binding| (binding.name.as_str(), binding))
        .collect::<BTreeMap<_, _>>();
    let binding_names = guards
        .iter()
        .map(|guard| guard.name.clone())
        .chain(base_bindings.iter().map(|binding| binding.name.clone()))
        .collect::<BTreeSet<_>>();
    let references = index::quoted_form_name_references_with_bindings(sql, &binding_names)?;
    let mut aliases = BTreeMap::new();
    for guard in guards {
        if !references.contains(&guard.name) {
            return Err(AppError::invalid_input(
                ErrorCode::InvalidInput,
                "SQL Form binding guard does not match a referenced Form name",
            )
            .into());
        }
    }
    for name in &references {
        if aliases.contains_key(name) {
            continue;
        }
        let guard = guards.iter().find(|guard| guard.name == *name);
        let base_binding = base_by_name.get(name.as_str());
        let inherited_guard = guard
            .zip(base_binding)
            .filter(|(guard, base)| guard.form_id == base.form_id);
        if let Some((_, binding)) = inherited_guard {
            let Some(form) = forms.iter().find(|form| {
                form.id.to_string() == binding.form_id
                    && relation_scopes.contains_key(&sql_relation_name(form.id))
            }) else {
                return Err(AppError::invalid_input(
                    ErrorCode::InvalidInput,
                    "SQL query Form binding is unavailable",
                )
                .into());
            };
            aliases.insert(name.clone(), sql_relation_name(form.id));
        } else if let Some(guard) = guard {
            let matches = forms
                .iter()
                .filter(|form| {
                    form.name == guard.name
                        && relation_scopes.contains_key(&sql_relation_name(form.id))
                })
                .collect::<Vec<_>>();
            let Some(form) = (matches.len() == 1).then(|| matches[0]) else {
                return Err(AppError::invalid_input(
                    ErrorCode::InvalidInput,
                    "SQL query Form binding is unavailable",
                )
                .into());
            };
            if form.id.to_string() != guard.form_id {
                return Err(AppError::invalid_input(
                    ErrorCode::InvalidInput,
                    "SQL Form binding guard no longer matches the selected Form",
                )
                .into());
            }
            aliases.insert(name.clone(), sql_relation_name(form.id));
        } else if let Some(binding) = base_binding {
            let Some(form) = forms.iter().find(|form| {
                form.id.to_string() == binding.form_id
                    && relation_scopes.contains_key(&sql_relation_name(form.id))
            }) else {
                return Err(AppError::invalid_input(
                    ErrorCode::InvalidInput,
                    "SQL query Form binding is unavailable",
                )
                .into());
            };
            aliases.insert(name.clone(), sql_relation_name(form.id));
        } else {
            let matches = forms
                .iter()
                .filter(|form| {
                    form.name == *name && relation_scopes.contains_key(&sql_relation_name(form.id))
                })
                .collect::<Vec<_>>();
            if matches.len() != 1 {
                return Err(AppError::invalid_input(
                    ErrorCode::InvalidInput,
                    "SQL query Form binding is unavailable",
                )
                .into());
            }
            aliases.insert(name.clone(), sql_relation_name(matches[0].id));
        }
    }
    let (bound, used) = index::resolve_saved_sql_bindings(sql, &aliases)?;
    if used.len() != references.len() {
        return Err(AppError::invalid_input(
            ErrorCode::InvalidInput,
            "SQL query Form binding is unavailable",
        )
        .into());
    }
    Ok(bound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ugoite_core::sql_query::SqlFormBindingGuard;
    use ugoite_domain::form::{sql_relation_name, FormVersion};
    use ugoite_domain::id::FormId;

    #[test]
    fn explicit_guard_binds_a_legacy_shaped_human_form_name() {
        let legacy_id =
            FormId::from_uuid(Uuid::parse_str("0123456789abcdef0123456789abcdef").unwrap());
        let selected_id =
            FormId::from_uuid(Uuid::parse_str("01900000-0000-7000-8000-000000000031").unwrap());
        let relation = sql_relation_name(legacy_id);
        let forms = vec![
            FormDefinition {
                id: legacy_id,
                version: FormVersion::new(1).unwrap(),
                name: "Original".to_owned(),
                description: None,
                fields: Vec::new(),
                allow_extra_attributes: false,
                extension_metadata: BTreeMap::new(),
            },
            FormDefinition {
                id: selected_id,
                version: FormVersion::new(1).unwrap(),
                name: relation.clone(),
                description: None,
                fields: Vec::new(),
                allow_extra_attributes: false,
                extension_metadata: BTreeMap::new(),
            },
        ];
        let relation_scopes = forms
            .iter()
            .map(|form| (sql_relation_name(form.id), EntryScope::AllCurrent))
            .collect();
        let sql = format!("SELECT * FROM \"{relation}\"");

        let bound = bind_human_form_names(
            &sql,
            &[SqlFormBindingGuard {
                name: relation.clone(),
                form_id: selected_id.to_string(),
            }],
            &[],
            &forms,
            &relation_scopes,
        )
        .expect("picker guard binds the selected Form");

        assert_eq!(
            bound,
            format!(
                "SELECT * FROM {} AS \"{relation}\"",
                sql_relation_name(selected_id)
            ),
        );
    }
}
