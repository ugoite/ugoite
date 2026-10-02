use crate::entry;
use crate::form;
use crate::index;
use crate::integrity::IntegrityProvider;
use anyhow::{Context, Result};
use opendal::Operator;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_core::query::EntryScope;
use uuid::Uuid;

const SQL_FORM_NAME: &str = "SQL";
pub(crate) const SQL_FORM_NAME_FOR_AUDIT: &str = SQL_FORM_NAME;
const SQL_VALIDATION_PREFIX: &str = "UGOITE_SQL_VALIDATION";

fn validation_error(message: impl std::fmt::Display) -> anyhow::Error {
    AppError::invalid_input(
        ErrorCode::InvalidInput,
        format!("{SQL_VALIDATION_PREFIX}: {message}"),
    )
    .into()
}

fn sql_entry_not_found(sql_id: &str) -> anyhow::Error {
    AppError::not_found(
        ErrorCode::EntryNotFound,
        format!("Entry not found: {sql_id}"),
    )
    .into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqlVariable {
    #[serde(rename = "type")]
    pub var_type: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SqlKind {
    UserQuery,
    SearchHistory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchHistoryOperator {
    Equals,
    Contains,
    Lt,
    Lte,
    Gt,
    Gte,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct SearchHistoryFieldCondition {
    pub field: String,
    pub operator: SearchHistoryOperator,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct SearchHistoryCriteria {
    pub form_name: String,
    pub tags: Vec<String>,
    pub updated_from: String,
    pub updated_to: String,
    pub field_conditions: Vec<SearchHistoryFieldCondition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SqlGeneratedName {
    Untitled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct SqlMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_criteria: Option<SearchHistoryCriteria>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_name: Option<SqlGeneratedName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binding_version: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub form_bindings: Option<Vec<SqlFormBinding>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SqlFormBinding {
    pub name: String,
    pub form_id: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedSavedSqlRevision {
    pub sql: String,
    pub variables: Value,
    pub bindings: Vec<SqlFormBinding>,
    pub binding_version: Option<u32>,
}

/// Current-authorization-safe metadata for one exact immutable Saved SQL
/// revision. SQL text, variable descriptions, and Form bindings are omitted.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SavedSqlRevisionDescriptor {
    pub id: String,
    pub revision_id: String,
    pub variables: BTreeMap<String, SavedSqlVariableDescriptor>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SavedSqlVariableDescriptor {
    pub var_type: String,
}

fn saved_sql_binding_metadata(
    metadata: Option<SqlMetadata>,
) -> Result<(Option<u32>, Vec<SqlFormBinding>)> {
    match metadata {
        Some(metadata) => match (metadata.binding_version, metadata.form_bindings) {
            (Some(1), Some(bindings)) => Ok((Some(1), bindings)),
            (None, None) => Ok((None, Vec::new())),
            _ => anyhow::bail!("unsupported or incomplete Saved SQL Form binding metadata"),
        },
        None => Ok((None, Vec::new())),
    }
}

pub(crate) struct SqlUpdateContext<'a, I: IntegrityProvider> {
    pub authorized_forms: &'a BTreeMap<String, ugoite_domain::id::FormId>,
    pub parent_revision_id: &'a str,
    pub author: &'a str,
    pub integrity: &'a I,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlPayload {
    pub name: Option<String>,
    pub kind: SqlKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SqlMetadata>,
    pub sql: String,
    #[serde(default)]
    pub variables: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlUpdatePayload {
    pub name: Option<String>,
    pub kind: SqlKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SqlMetadata>,
    pub sql: String,
    #[serde(default)]
    pub variables: Value,
    pub parent_revision_id: String,
}

impl SqlUpdatePayload {
    pub fn into_sql_payload(self) -> SqlPayload {
        SqlPayload {
            name: self.name,
            kind: self.kind,
            metadata: self.metadata,
            sql: self.sql,
            variables: self.variables,
        }
    }
}

fn validate_sql_metadata(payload: &SqlPayload) -> Result<()> {
    if payload
        .name
        .as_ref()
        .is_some_and(|name| name.trim().is_empty())
    {
        return Err(validation_error("name must be null or a non-blank string"));
    }

    match payload.kind {
        SqlKind::UserQuery => match (&payload.name, &payload.metadata) {
            (Some(_), None) => {}
            (Some(_), Some(metadata))
                if metadata.generated_name.is_none()
                    && metadata.search_criteria.is_none()
                    && (metadata.binding_version.is_some() || metadata.form_bindings.is_some()) => {
            }
            (Some(_), Some(metadata))
                if metadata.generated_name.is_none() && metadata.search_criteria.is_none() =>
            {
                return Err(validation_error(
                    "user-query metadata must be omitted for named queries",
                ));
            }
            (Some(_), Some(_)) => {
                return Err(validation_error(
                    "named user-query cannot declare generated metadata",
                ));
            }
            (None, Some(metadata))
                if metadata.search_criteria.is_none()
                    && matches!(metadata.generated_name, Some(SqlGeneratedName::Untitled)) => {}
            (None, _) => {
                return Err(validation_error(
                    "unnamed user-query must declare generated_name=untitled",
                ));
            }
        },
        SqlKind::SearchHistory => {
            if payload.name.is_some() {
                return Err(validation_error(
                    "search-history requires a structured search_criteria and no name",
                ));
            }
            match payload.metadata.as_ref() {
                Some(metadata)
                    if metadata.search_criteria.is_some() && metadata.generated_name.is_none() => {}
                Some(_) => {
                    return Err(validation_error(
                        "search-history metadata must contain only search_criteria",
                    ));
                }
                None => {
                    return Err(validation_error(
                        "search-history requires a structured search_criteria and no name",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn sql_form_definition() -> Value {
    serde_json::json!({
        "name": SQL_FORM_NAME,
        "version": 1,
        "fields": {
            // The saved-SQL display name is a normal optional Form field.
            // Older records without this field remain valid; no table rewrite
            // is performed.
            "name": {"type": "string", "required": false},
            "sql": {"type": "sql", "required": true},
            "variables": {"type": "object_list", "required": false}
        },
        "allow_extra_attributes": "allow_json"
    })
}

async fn ensure_sql_form(op: &Operator, ws_path: &str) -> Result<Value> {
    let form_def = sql_form_definition();
    form::upsert_metadata_form(op, ws_path, &form_def).await?;
    form::read_form_definition(op, ws_path, SQL_FORM_NAME).await
}

async fn sql_form_exists_read_only(op: &Operator, ws_path: &str) -> Result<bool> {
    Ok(form::list_forms_read_only(op, ws_path)
        .await?
        .iter()
        .any(|definition| definition.get("name").and_then(Value::as_str) == Some(SQL_FORM_NAME)))
}

fn normalize_sql_variables(value: Option<&Value>) -> Result<Value> {
    let items = match value {
        None => Vec::new(),
        Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items.clone(),
        Some(_) => return Err(validation_error("variables must be an array")),
    };

    let mut normalized = Vec::new();
    for item in items {
        let obj = item
            .as_object()
            .ok_or_else(|| validation_error("variables items must be objects"))?;
        let var_type = obj
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| validation_error("variables.type must be a string"))?;
        if !matches!(
            var_type,
            "string" | "boolean" | "integer" | "float" | "timestamp" | "date"
        ) {
            return Err(validation_error(format!(
                "variables.type is unsupported: {var_type}"
            )));
        }
        let name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| validation_error("variables.name must be a string"))?;
        if !is_sql_variable_name(name) {
            return Err(validation_error(
                "variables.name must match [A-Za-z_][A-Za-z0-9_]*",
            ));
        }
        let description = obj
            .get("description")
            .and_then(|v| v.as_str())
            .ok_or_else(|| validation_error("variables.description must be a string"))?;
        normalized.push(serde_json::json!({
            "type": var_type,
            "name": name,
            "description": description,
        }));
    }
    Ok(Value::Array(normalized))
}

async fn validate_sql_payload(
    op: &Operator,
    ws_path: &str,
    sql_text: &str,
    variables: &Value,
) -> Result<()> {
    let items = variables
        .as_array()
        .ok_or_else(|| validation_error("variables must be an array"))?;
    let mut var_names = BTreeSet::new();
    for item in items {
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if name.is_empty() {
            return Err(validation_error(
                "variables.name must be a non-empty string",
            ));
        }
        var_names.insert(name.to_string());
    }

    let embedded_names = crate::index::datafusion_parameter_names(op, ws_path, sql_text)
        .await
        .map_err(validation_error)?
        .into_iter()
        .map(|name| name.trim_start_matches('$').to_string())
        .collect::<BTreeSet<_>>();

    for name in &var_names {
        if !embedded_names.contains(name) {
            return Err(validation_error(format!(
                "variables must be embedded in SQL as ${name}"
            )));
        }
    }

    for name in &embedded_names {
        if !var_names.contains(name) {
            return Err(validation_error(format!(
                "sql contains undefined variables: {name}",
            )));
        }
    }

    Ok(())
}

fn is_sql_variable_name(name: &str) -> bool {
    let mut characters = name.chars();
    matches!(characters.next(), Some(first) if first == '_' || first.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn sql_integrity_payload(
    integrity: &dyn IntegrityProvider,
    payload: &SqlPayload,
    variables: &Value,
) -> entry::IntegrityPayload {
    let payload = serde_json::json!({
        "name": payload.name,
        "kind": payload.kind,
        "metadata": payload.metadata,
        "sql": payload.sql,
        "variables": variables,
    });
    let serialized = serde_json::to_string(&payload).unwrap_or_default();
    entry::IntegrityPayload {
        checksum: integrity.checksum(&serialized),
        signature: integrity.signature(&serialized),
    }
}

/// Recomputes the content integrity stored on one Saved SQL history row.
/// Saved SQL has its own canonical payload and must not be checked as
/// Markdown-backed Entry content.
pub(crate) fn verify_revision_integrity(
    row: &entry::RevisionRow,
    integrity: &dyn IntegrityProvider,
) -> Result<()> {
    let mut fields = row.fields.clone();
    let mut extra_attributes = row.extra_attributes.clone();
    if row.operation == "delete" {
        let state = row
            .state
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Saved SQL tombstone state is missing"))?;
        if !state.deleted
            || state.entry_id != row.entry_id
            || state.revision_id != row.revision_id
            || state.parent_revision_id != row.parent_revision_id
            || state.integrity.checksum != row.integrity.checksum
            || state.integrity.signature != row.integrity.signature
            || !row.fields.as_object().is_some_and(Map::is_empty)
            || !row.extra_attributes.as_object().is_some_and(Map::is_empty)
        {
            anyhow::bail!("Saved SQL tombstone state does not match its revision");
        }
        fields = state.fields.clone();
        extra_attributes = state.extra_attributes.clone();
    }
    if let Some(state) = &row.state {
        let mut state = state.clone();
        apply_sql_name_compat(&mut state);
        if let (Some(fields), Some(state_fields)) =
            (fields.as_object_mut(), state.fields.as_object())
        {
            if let Some(name) = state_fields.get("name") {
                fields.entry("name").or_insert_with(|| name.clone());
            }
        }
    }

    let name = fields
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned);
    let sql = fields
        .get("sql")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("SQL text is missing"))?
        .to_owned();
    let variables = normalize_sql_variables(fields.get("variables"))?;
    let kind: SqlKind = serde_json::from_value(
        extra_attributes
            .get("kind")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("SQL kind is missing"))?,
    )
    .context("SQL kind is invalid")?;
    let metadata = extra_attributes
        .get("metadata")
        .cloned()
        .filter(|value| !value.is_null())
        .map(serde_json::from_value::<SqlMetadata>)
        .transpose()
        .context("SQL metadata is invalid")?;
    let payload = SqlPayload {
        name,
        kind,
        metadata,
        sql,
        variables: variables.clone(),
    };
    let expected = sql_integrity_payload(integrity, &payload, &variables);
    if expected.checksum != row.integrity.checksum || expected.signature != row.integrity.signature
    {
        anyhow::bail!("Saved SQL integrity mismatch");
    }
    Ok(())
}

fn sql_extra_attributes(payload: &SqlPayload) -> Value {
    serde_json::json!({
        "kind": payload.kind,
        "metadata": payload.metadata,
    })
}

fn with_server_bindings(payload: &mut SqlPayload, bindings: Vec<SqlFormBinding>) {
    let mut metadata = payload.metadata.clone().unwrap_or(SqlMetadata {
        search_criteria: None,
        generated_name: None,
        binding_version: None,
        form_bindings: None,
    });
    metadata.binding_version = Some(1);
    metadata.form_bindings = Some(bindings);
    payload.metadata = Some(metadata);
}

fn derive_form_bindings(
    sql: &str,
    authorized_forms: &BTreeMap<String, ugoite_domain::id::FormId>,
) -> Result<Vec<SqlFormBinding>> {
    let references = index::quoted_form_name_references(sql).map_err(validation_error)?;
    for name in &references {
        if !authorized_forms.contains_key(name) {
            return Err(validation_error(format!(
                "SQL Form name {name} is not available to bind in this revision"
            )));
        }
    }
    let aliases = authorized_forms
        .iter()
        .map(|(name, id)| (name.clone(), ugoite_domain::form::sql_relation_name(*id)))
        .collect::<BTreeMap<_, _>>();
    let (_, used_names) =
        index::resolve_saved_sql_bindings(sql, &aliases).map_err(validation_error)?;
    Ok(used_names
        .into_iter()
        .filter_map(|name| {
            authorized_forms.get(&name).map(|id| SqlFormBinding {
                name,
                form_id: id.to_string(),
            })
        })
        .collect())
}

/// Storage-compatibility boundary: folds the legacy physical
/// `saved_query_name` carrier (decoded as `legacy_saved_query_name`) into
/// the in-memory `fields["name"]` for old Saved SQL rows that predate the
/// canonical SQL Form name field. New rows always carry an empty legacy
/// value; product logic below must never reference the legacy carrier.
fn apply_sql_name_compat(row: &mut entry::EntryRow) {
    let has_name = row
        .fields
        .get("name")
        .and_then(|value| value.as_str())
        .is_some_and(|name| !name.trim().is_empty());
    if has_name || row.legacy_saved_query_name.trim().is_empty() {
        return;
    }
    if let Some(fields) = row.fields.as_object_mut() {
        fields.insert(
            "name".to_string(),
            Value::String(row.legacy_saved_query_name.clone()),
        );
    }
}

fn sql_entry_from_row(row: &entry::EntryRow) -> Result<Value> {
    let fields = row
        .fields
        .as_object()
        .context("SQL row fields must be an object")?;
    let sql_value = fields.get("sql").and_then(|v| v.as_str()).unwrap_or("");
    let variables = fields
        .get("variables")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let extra_attributes = row
        .extra_attributes
        .as_object()
        .context("SQL row extra_attributes must be an object")?;
    let kind = extra_attributes
        .get("kind")
        .cloned()
        .context("SQL row kind is missing")?;
    let kind: SqlKind = serde_json::from_value(kind).context("SQL row kind is invalid")?;
    let metadata = extra_attributes
        .get("metadata")
        .cloned()
        .unwrap_or(Value::Null);
    let metadata = if metadata.is_null() {
        None
    } else {
        Some(
            serde_json::from_value::<SqlMetadata>(metadata)
                .context("SQL row metadata is invalid")?,
        )
    };
    // Canonical SQL Form `fields["name"]` is the single authority. Older
    // nameless records remain valid.
    let name = fields
        .get("name")
        .and_then(|value| value.as_str())
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned);

    Ok(serde_json::json!({
        "id": row.entry_id,
        "name": name.map(Value::String).unwrap_or(Value::Null),
        "kind": kind,
        "metadata": metadata,
        "sql": sql_value,
        "variables": variables,
        "created_at": row.created_at,
        "updated_at": row.updated_at,
        "author": row.author,
        "updated_by": row.updated_by,
        "deleted_by": row.deleted_by,
        "revision_id": row.revision_id,
    }))
}

pub async fn list_sql(op: &Operator, ws_path: &str, entry_scope: EntryScope) -> Result<Vec<Value>> {
    if !sql_form_exists_read_only(op, ws_path).await? {
        return Ok(Vec::new());
    }
    let rows = index::query_form_entry_rows_authorized(
        op,
        ws_path,
        SQL_FORM_NAME,
        entry_scope,
        None,
        crate::MAX_NORMAL_READ_ROWS.saturating_add(1),
    )
    .await?;
    let mut entries = Vec::new();
    for mut row in rows {
        if row.deleted {
            continue;
        }
        apply_sql_name_compat(&mut row);
        entries.push(sql_entry_from_row(&row)?);
    }
    Ok(entries)
}

pub async fn get_sql(op: &Operator, ws_path: &str, sql_id: &str) -> Result<Value> {
    if !sql_form_exists_read_only(op, ws_path).await? {
        return Err(sql_entry_not_found(sql_id));
    }
    let mut row = entry::read_entry_row(op, ws_path, SQL_FORM_NAME, sql_id).await?;
    if row.deleted {
        return Err(sql_entry_not_found(sql_id));
    }
    apply_sql_name_compat(&mut row);
    sql_entry_from_row(&row)
}

pub(crate) async fn read_sql_revision(
    op: &Operator,
    ws_path: &str,
    sql_id: &str,
    revision_id: &str,
    integrity: &(dyn IntegrityProvider + Send + Sync),
) -> Result<ResolvedSavedSqlRevision> {
    if !sql_form_exists_read_only(op, ws_path).await? {
        return Err(AppError::not_found(
            ErrorCode::EntryNotFound,
            format!("Saved SQL revision not found: {sql_id}@{revision_id}"),
        )
        .into());
    }
    let (_, _, revisions) =
        entry::revision_rows_for_form_read_only(op, ws_path, SQL_FORM_NAME).await?;
    let row = revisions
        .into_iter()
        .find(|row| row.entry_id == sql_id && row.revision_id == revision_id)
        .ok_or_else(|| {
            AppError::not_found(
                ErrorCode::EntryNotFound,
                format!("Saved SQL revision not found: {sql_id}@{revision_id}"),
            )
        })?;
    if row.operation == "delete" {
        return Err(sql_entry_not_found(sql_id));
    }
    verify_revision_integrity(&row, integrity)?;
    let fields = row
        .fields
        .as_object()
        .context("SQL revision fields must be an object")?;
    let extra = row
        .extra_attributes
        .as_object()
        .context("SQL revision metadata must be an object")?;
    let sql = fields
        .get("sql")
        .and_then(Value::as_str)
        .context("SQL revision text is missing")?
        .to_owned();
    let variables = normalize_sql_variables(fields.get("variables"))?;
    let metadata = extra
        .get("metadata")
        .filter(|value| !value.is_null())
        .cloned()
        .map(serde_json::from_value::<SqlMetadata>)
        .transpose()
        .context("SQL revision metadata is invalid")?;
    let (binding_version, bindings) = saved_sql_binding_metadata(metadata)?;
    if binding_version.is_some() {
        let references = index::quoted_form_name_references(&sql)?;
        let bound_names = bindings
            .iter()
            .map(|binding| binding.name.clone())
            .collect::<BTreeSet<_>>();
        if references != bound_names {
            anyhow::bail!("Saved SQL Form bindings do not match the SQL Form-name references");
        }
    }
    Ok(ResolvedSavedSqlRevision {
        sql,
        variables,
        bindings,
        binding_version,
    })
}

/// Reads the committed saved-SQL revision identity for audit reconciliation,
/// including tombstones. Returns
/// `(revision_id, parent_revision_id, deleted, committed_actor)` or `None`
/// when no row was ever committed. Only identity fields cross this boundary;
/// SQL text and variables never leave storage here.
pub(crate) async fn read_sql_row_for_audit(
    op: &Operator,
    ws_path: &str,
    sql_id: &str,
) -> Result<Option<(String, Option<String>, bool, String)>> {
    // Read-only presence check: reconciliation must never bootstrap the SQL
    // form as a side effect. A missing form means no row was ever committed.
    // Typed missing-target only; corrupt Forms and storage failures
    // propagate fail-closed.
    match crate::form::read_form_definition(op, ws_path, SQL_FORM_NAME).await {
        Ok(_) => {}
        Err(error) if crate::audit::is_missing_audit_target(&error) => {
            return Ok(None);
        }
        Err(error) => return Err(error),
    }
    let row = match entry::read_entry_row(op, ws_path, SQL_FORM_NAME, sql_id).await {
        Ok(row) => row,
        Err(error)
            if error
                .downcast_ref::<AppError>()
                .is_some_and(|app| app.code() == ErrorCode::EntryNotFound)
                || crate::audit::is_missing_audit_target(&error) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    // Committed actor is the authority for reconciliation: prefer the
    // revision updater, then the original author, exactly like Entry
    // history attribution.
    let committed_actor = if row.updated_by.trim().is_empty() {
        row.author.clone()
    } else {
        row.updated_by.clone()
    };
    Ok(Some((
        row.revision_id,
        row.parent_revision_id,
        row.deleted,
        committed_actor,
    )))
}

/// Lists committed saved-SQL IDs for audit reconciliation, tombstones
/// included. Deleted rows have no listable state but their delete evidence
/// may still be missing, so reconciliation enumerates them too. Returns an
/// empty list when the SQL form was never created; enumeration never creates
/// storage state.
pub(crate) async fn list_sql_ids_for_audit(op: &Operator, ws_path: &str) -> Result<Vec<String>> {
    crate::entry::list_form_entry_ids_for_audit(op, ws_path, SQL_FORM_NAME).await
}

pub async fn find_sql_id_by_text(
    op: &Operator,
    ws_path: &str,
    sql_text: &str,
    entry_scope: EntryScope,
) -> Result<Option<String>> {
    ensure_sql_form(op, ws_path).await?;
    let expected = Value::String(sql_text.to_owned());
    Ok(index::query_form_entry_rows_authorized(
        op,
        ws_path,
        SQL_FORM_NAME,
        entry_scope,
        Some(("sql", &expected)),
        1,
    )
    .await?
    .into_iter()
    .next()
    .map(|row| row.entry_id))
}

pub async fn create_sql<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    sql_id: &str,
    payload: &SqlPayload,
    author: &str,
    integrity: &I,
) -> Result<Value> {
    let workspace = crate::iceberg_store::native_workspace(op, ws_path).await?;
    let publication = workspace.current_publication().await?;
    let checkpoint = workspace.resolve_publication(&publication).await?;
    let forms = workspace
        .forms_at_checkpoint(&checkpoint)
        .await?
        .into_iter()
        .map(|form| (form.name, form.id))
        .collect::<BTreeMap<_, _>>();
    create_sql_with_bindings(op, ws_path, sql_id, payload, &forms, author, integrity).await
}

pub(crate) async fn create_sql_with_bindings<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    sql_id: &str,
    payload: &SqlPayload,
    authorized_forms: &BTreeMap<String, ugoite_domain::id::FormId>,
    author: &str,
    integrity: &I,
) -> Result<Value> {
    crate::authorization::Authorizer::new(op.clone()).ensure_authoritative_mutation_contract()?;
    let form_def = ensure_sql_form(op, ws_path).await?;
    let mut normalized_payload = payload.clone();
    normalized_payload.sql =
        index::normalize_sql_template(&payload.sql).map_err(validation_error)?;
    validate_sql_metadata(&normalized_payload)?;
    let variables = normalize_sql_variables(Some(&payload.variables))?;
    validate_sql_payload(op, ws_path, &normalized_payload.sql, &variables).await?;
    let bindings = derive_form_bindings(&normalized_payload.sql, authorized_forms)?;
    with_server_bindings(&mut normalized_payload, bindings);

    let timestamp = entry::now_ts();
    let revision_id = Uuid::new_v4().to_string();
    let integrity_payload = sql_integrity_payload(integrity, &normalized_payload, &variables);

    let mut fields = Map::new();
    if let Some(name) = normalized_payload.name.as_deref() {
        fields.insert("name".to_string(), Value::String(name.to_string()));
    }
    fields.insert(
        "sql".to_string(),
        Value::String(normalized_payload.sql.to_string()),
    );
    fields.insert("variables".to_string(), variables.clone());
    let extra_attributes = sql_extra_attributes(&normalized_payload);

    let row = entry::EntryRow {
        entry_id: sql_id.to_string(),
        // Canonical SQL Form `fields["name"]` is the single authority; the
        // legacy carrier is always empty for new rows.
        legacy_saved_query_name: String::new(),
        form: SQL_FORM_NAME.to_string(),
        tags: Vec::new(),
        created_at: timestamp,
        updated_at: timestamp,
        fields: Value::Object(fields),
        extra_attributes,
        revision_id: revision_id.clone(),
        parent_revision_id: None,
        integrity: integrity_payload.clone(),
        deleted: false,
        deleted_at: None,
        author: author.to_string(),
        updated_by: author.to_string(),
        deleted_by: None,
        entry_version: 1,
        legacy_columns: BTreeMap::new(),
    };

    let revision = entry::RevisionRow {
        revision_id: revision_id.clone(),
        change_id: revision_id.clone(),
        entry_id: sql_id.to_string(),
        parent_revision_id: None,
        timestamp,
        author: author.to_string(),
        updated_by: author.to_string(),
        deleted_by: None,
        fields: row.fields.clone(),
        extra_attributes: row.extra_attributes.clone(),
        markdown_checksum: integrity_payload.checksum.clone(),
        integrity: integrity_payload,
        restored_from: None,
        form_version: form_def.get("version").and_then(Value::as_u64).unwrap_or(1) as u32,
        state: Some(row.clone()),
        entry_version: row.entry_version,
        operation: "upsert".to_string(),
        source_kind: "api".to_string(),
        source_id: None,
        extension_metadata: Value::Object(Map::new()),
    };
    entry::append_revision_row_for_form(op, ws_path, SQL_FORM_NAME, &revision, &form_def).await?;

    sql_entry_from_row(&row)
}

pub async fn update_sql<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    sql_id: &str,
    payload: &SqlPayload,
    parent_revision_id: &str,
    author: &str,
    integrity: &I,
) -> Result<Value> {
    let workspace = crate::iceberg_store::native_workspace(op, ws_path).await?;
    let publication = workspace.current_publication().await?;
    let checkpoint = workspace.resolve_publication(&publication).await?;
    let forms = workspace
        .forms_at_checkpoint(&checkpoint)
        .await?
        .into_iter()
        .map(|form| (form.name, form.id))
        .collect::<BTreeMap<_, _>>();
    update_sql_with_bindings(
        op,
        ws_path,
        sql_id,
        payload,
        SqlUpdateContext {
            authorized_forms: &forms,
            parent_revision_id,
            author,
            integrity,
        },
    )
    .await
}

pub(crate) async fn update_sql_with_bindings<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    sql_id: &str,
    payload: &SqlPayload,
    context: SqlUpdateContext<'_, I>,
) -> Result<Value> {
    let SqlUpdateContext {
        authorized_forms,
        parent_revision_id,
        author,
        integrity,
    } = context;
    crate::authorization::Authorizer::new(op.clone()).ensure_authoritative_mutation_contract()?;
    ensure_sql_form(op, ws_path).await?;
    let form_def = form::read_form_definition(op, ws_path, SQL_FORM_NAME).await?;
    let mut row = entry::read_entry_row(op, ws_path, SQL_FORM_NAME, sql_id).await?;
    if row.deleted {
        return Err(sql_entry_not_found(sql_id));
    }

    if parent_revision_id.trim().is_empty() {
        return Err(AppError::invalid_input(
            ErrorCode::InvalidInput,
            "parent_revision_id must not be blank",
        )
        .into());
    }
    if row.revision_id != parent_revision_id {
        return Err(AppError::conflict(
            ErrorCode::RevisionConflict,
            format!(
                "Revision conflict: expected {}, got {}",
                parent_revision_id, row.revision_id
            ),
        )
        .into());
    }

    let mut normalized_payload = payload.clone();
    normalized_payload.sql =
        index::normalize_sql_template(&payload.sql).map_err(validation_error)?;
    let variables = normalize_sql_variables(Some(&normalized_payload.variables))?;
    validate_sql_metadata(&normalized_payload)?;
    validate_sql_payload(op, ws_path, &normalized_payload.sql, &variables).await?;
    let bindings = derive_form_bindings(&normalized_payload.sql, authorized_forms)?;
    with_server_bindings(&mut normalized_payload, bindings);
    let mut timestamp = entry::now_ts();
    if timestamp <= row.updated_at {
        timestamp = row.updated_at + 0.001;
    }
    let revision_id = Uuid::new_v4().to_string();
    let integrity_payload = sql_integrity_payload(integrity, &normalized_payload, &variables);

    let mut fields = Map::new();
    if let Some(name) = normalized_payload.name.as_deref() {
        fields.insert("name".to_string(), Value::String(name.to_string()));
    }
    fields.insert(
        "sql".to_string(),
        Value::String(normalized_payload.sql.to_string()),
    );
    fields.insert("variables".to_string(), variables.clone());
    let extra_attributes = sql_extra_attributes(&normalized_payload);

    row.updated_at = timestamp;
    row.fields = Value::Object(fields);
    row.extra_attributes = extra_attributes;
    // The legacy carrier is never authority and never written; clear it
    // defensively so updated rows cannot carry a stale value forward.
    row.legacy_saved_query_name = String::new();
    row.parent_revision_id = Some(row.revision_id.clone());
    row.revision_id = revision_id.clone();
    row.entry_version = row.entry_version.saturating_add(1);
    row.updated_by = author.to_string();
    row.deleted_by = None;
    row.integrity = integrity_payload.clone();

    let revision = entry::RevisionRow {
        revision_id: revision_id.clone(),
        change_id: revision_id.clone(),
        entry_id: sql_id.to_string(),
        parent_revision_id: row.parent_revision_id.clone(),
        timestamp,
        author: row.author.clone(),
        updated_by: author.to_string(),
        deleted_by: None,
        fields: row.fields.clone(),
        extra_attributes: row.extra_attributes.clone(),
        markdown_checksum: integrity_payload.checksum.clone(),
        integrity: integrity_payload,
        restored_from: None,
        form_version: form_def.get("version").and_then(Value::as_u64).unwrap_or(1) as u32,
        state: Some(row.clone()),
        entry_version: row.entry_version,
        operation: "upsert".to_string(),
        source_kind: "api".to_string(),
        source_id: None,
        extension_metadata: Value::Object(Map::new()),
    };
    entry::append_revision_row_for_form(op, ws_path, SQL_FORM_NAME, &revision, &form_def).await?;

    sql_entry_from_row(&row)
}

pub async fn delete_sql(op: &Operator, ws_path: &str, sql_id: &str, actor: &str) -> Result<()> {
    crate::authorization::Authorizer::new(op.clone()).ensure_authoritative_mutation_contract()?;
    ensure_sql_form(op, ws_path).await?;
    let form_def = form::read_form_definition(op, ws_path, SQL_FORM_NAME).await?;
    let mut row = entry::read_entry_row(op, ws_path, SQL_FORM_NAME, sql_id).await?;
    if row.deleted {
        return Err(sql_entry_not_found(sql_id));
    }

    let mut delete_ts = entry::now_ts();
    if delete_ts <= row.updated_at {
        delete_ts = row.updated_at + 0.001;
    }
    row.deleted = true;
    row.deleted_at = Some(delete_ts);
    row.updated_at = delete_ts;
    row.parent_revision_id = Some(row.revision_id.clone());
    row.revision_id = Uuid::new_v4().to_string();
    row.entry_version = row.entry_version.saturating_add(1);
    row.updated_by = actor.to_string();
    row.deleted_by = Some(actor.to_string());
    let entry_version = row.entry_version;
    let tombstone = entry::RevisionRow {
        revision_id: row.revision_id.clone(),
        change_id: row.revision_id.clone(),
        entry_id: sql_id.to_string(),
        parent_revision_id: row.parent_revision_id.clone(),
        timestamp: delete_ts,
        author: row.author.clone(),
        updated_by: actor.to_string(),
        deleted_by: Some(actor.to_string()),
        fields: Value::Object(Map::new()),
        extra_attributes: Value::Object(Map::new()),
        markdown_checksum: row.integrity.checksum.clone(),
        integrity: row.integrity.clone(),
        restored_from: None,
        form_version: form_def.get("version").and_then(Value::as_u64).unwrap_or(1) as u32,
        state: Some(row),
        entry_version,
        operation: "delete".to_string(),
        source_kind: "api".to_string(),
        source_id: None,
        extension_metadata: Value::Object(Map::new()),
    };
    entry::append_revision_row_for_form(op, ws_path, SQL_FORM_NAME, &tombstone, &form_def).await?;
    Ok(())
}

#[cfg(test)]
mod name_field_tests {
    use super::*;
    use crate::integrity::FakeIntegrityProvider;

    #[test]
    fn saved_sql_metadata_reader_accepts_versioned_form_bindings() {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct LegacySqlMetadata {
            search_criteria: Option<Value>,
            generated_name: Option<Value>,
        }
        let legacy: SqlMetadata = serde_json::from_value(serde_json::json!({
            "searchCriteria": null,
            "generatedName": "untitled",
        }))
        .expect("legacy metadata remains readable");
        assert!(legacy.search_criteria.is_none());

        let bound = serde_json::json!({
            "searchCriteria": null,
            "generatedName": "untitled",
            "bindingVersion": 1,
            "formBindings": [],
        });
        let bound = serde_json::from_value::<SqlMetadata>(bound).expect("supported metadata");
        assert_eq!(bound.binding_version, Some(1));
        assert_eq!(bound.form_bindings.unwrap().len(), 0);

        let legacy_reader: LegacySqlMetadata = serde_json::from_value(serde_json::json!({
            "searchCriteria": null,
            "generatedName": "untitled",
        }))
        .expect("legacy reader can read legacy metadata");
        assert!(legacy_reader.search_criteria.is_none());
        assert!(legacy_reader.generated_name.is_some());
        assert!(
            serde_json::from_value::<LegacySqlMetadata>(serde_json::json!({
                "searchCriteria": null,
                "generatedName": "untitled",
                "bindingVersion": 1,
                "formBindings": []
            }))
            .is_err(),
            "old readers fail closed instead of dropping binding metadata"
        );

        let legacy = serde_json::from_value::<SqlMetadata>(serde_json::json!({
            "searchCriteria": null,
            "generatedName": null,
        }))
        .expect("pre-binding metadata decodes");
        assert_eq!(
            saved_sql_binding_metadata(Some(legacy)).unwrap(),
            (None, Vec::new())
        );
        for partial in [
            serde_json::json!({"bindingVersion": 1}),
            serde_json::json!({"formBindings": []}),
            serde_json::json!({"bindingVersion": 2, "formBindings": []}),
        ] {
            let metadata = serde_json::from_value::<SqlMetadata>(partial)
                .expect("known fields decode before semantic version validation");
            assert!(saved_sql_binding_metadata(Some(metadata)).is_err());
        }
    }

    #[test]
    fn create_binding_derivation_is_server_side_and_name_scoped() {
        let form_id = Uuid::parse_str("01900000-0000-7000-8000-000000000001")
            .expect("uuid")
            .into();
        let forms = BTreeMap::from([("Expense".to_string(), form_id)]);
        let bindings = derive_form_bindings("SELECT * FROM \"Expense\"", &forms).unwrap();
        assert_eq!(
            bindings,
            vec![SqlFormBinding {
                name: "Expense".into(),
                form_id: form_id.to_string()
            }]
        );
        assert!(derive_form_bindings("SELECT * FROM \"Missing\"", &forms).is_err());
        assert!(derive_form_bindings("SELECT * FROM Expense", &forms).is_err());
    }

    #[test]
    fn client_binding_metadata_is_replaced_by_server_derived_bindings() {
        let mut payload = SqlPayload {
            name: Some("query".into()),
            kind: SqlKind::UserQuery,
            metadata: Some(SqlMetadata {
                search_criteria: None,
                generated_name: None,
                binding_version: Some(999),
                form_bindings: Some(vec![SqlFormBinding {
                    name: "Secret".into(),
                    form_id: "client-controlled".into(),
                }]),
            }),
            sql: "SELECT 1".into(),
            variables: Value::Array(Vec::new()),
        };
        let server = vec![SqlFormBinding {
            name: "Task".into(),
            form_id: "01900000-0000-7000-8000-000000000001".into(),
        }];
        with_server_bindings(&mut payload, server.clone());
        let metadata = payload.metadata.expect("metadata");
        assert_eq!(metadata.binding_version, Some(1));
        assert_eq!(metadata.form_bindings, Some(server));
    }

    #[test]
    fn named_query_accepts_binding_echo_but_rejects_generated_metadata() {
        let mut payload = SqlPayload {
            name: Some("query".into()),
            kind: SqlKind::UserQuery,
            metadata: Some(SqlMetadata {
                search_criteria: None,
                generated_name: None,
                binding_version: Some(999),
                form_bindings: Some(vec![SqlFormBinding {
                    name: "Task".into(),
                    form_id: "client-controlled".into(),
                }]),
            }),
            sql: "SELECT * FROM \"Task\"".into(),
            variables: Value::Array(Vec::new()),
        };
        assert!(validate_sql_metadata(&payload).is_ok());

        let server_bindings = vec![SqlFormBinding {
            name: "Task".into(),
            form_id: "01900000-0000-7000-8000-000000000001".into(),
        }];
        with_server_bindings(&mut payload, server_bindings.clone());
        assert_eq!(
            payload.metadata.as_ref().unwrap().form_bindings.as_ref(),
            Some(&server_bindings)
        );

        let mut invalid = payload;
        let metadata = invalid.metadata.as_mut().expect("metadata");
        metadata.generated_name = Some(SqlGeneratedName::Untitled);
        assert!(validate_sql_metadata(&invalid).is_err());
    }

    fn row_with_saved_query_name(name: &str, fields: Value) -> entry::EntryRow {
        entry::EntryRow {
            entry_id: "sql-legacy".to_string(),
            legacy_saved_query_name: name.to_string(),
            form: SQL_FORM_NAME.to_string(),
            tags: Vec::new(),
            created_at: 1.0,
            updated_at: 1.0,
            fields,
            extra_attributes: serde_json::json!({
                "kind": "user-query",
                "metadata": null,
            }),
            revision_id: "rev-1".to_string(),
            parent_revision_id: None,
            integrity: entry::IntegrityPayload {
                checksum: String::new(),
                signature: String::new(),
            },
            deleted: false,
            deleted_at: None,
            author: "author".to_string(),
            updated_by: "author".to_string(),
            deleted_by: None,
            entry_version: 1,
            legacy_columns: BTreeMap::new(),
        }
    }

    fn sql_fields(name: Option<&str>) -> Value {
        let mut fields = Map::new();
        if let Some(name) = name {
            fields.insert("name".to_string(), Value::String(name.to_string()));
        }
        fields.insert("sql".to_string(), Value::String("SELECT 1".to_string()));
        fields.insert("variables".to_string(), Value::Array(Vec::new()));
        Value::Object(fields)
    }

    #[test]
    fn revision_integrity_uses_saved_sql_payload_and_rejects_tampering() {
        let provider = FakeIntegrityProvider;
        let variables = Value::Array(Vec::new());
        let payload = SqlPayload {
            name: Some("portable-recovery".into()),
            kind: SqlKind::UserQuery,
            metadata: Some(SqlMetadata {
                search_criteria: None,
                generated_name: None,
                binding_version: Some(1),
                form_bindings: Some(vec![SqlFormBinding {
                    name: "Task".into(),
                    form_id: "01900000-0000-7000-8000-000000000001".into(),
                }]),
            }),
            sql: "SELECT 1 AS recovery_check".into(),
            variables: variables.clone(),
        };
        let integrity = sql_integrity_payload(&provider, &payload, &variables);
        let mut fields = Map::new();
        fields.insert("name".into(), Value::String(payload.name.clone().unwrap()));
        fields.insert("sql".into(), Value::String(payload.sql.clone()));
        fields.insert("variables".into(), variables);
        let mut row = entry::RevisionRow {
            revision_id: "revision-1".into(),
            change_id: "change-1".into(),
            entry_id: "saved-sql-1".into(),
            parent_revision_id: None,
            timestamp: 1.0,
            author: "test".into(),
            updated_by: "test".into(),
            deleted_by: None,
            fields: Value::Object(fields),
            extra_attributes: serde_json::json!({"kind": "user-query", "metadata": payload.metadata.clone()}),
            markdown_checksum: integrity.checksum.clone(),
            integrity,
            restored_from: None,
            form_version: 1,
            state: None,
            entry_version: 1,
            operation: "upsert".into(),
            source_kind: "test".into(),
            source_id: None,
            extension_metadata: Value::Object(Map::new()),
        };

        verify_revision_integrity(&row, &provider).expect("valid Saved SQL integrity");
        row.fields["sql"] = Value::String("SELECT 200 AS recovery_check".into());
        assert!(verify_revision_integrity(&row, &provider).is_err());

        row.fields["sql"] = Value::String(payload.sql.clone());
        row.extra_attributes["metadata"]["formBindings"][0]["formId"] =
            Value::String("tampered-form-id-with-a-different-length".into());
        assert!(
            verify_revision_integrity(&row, &provider).is_err(),
            "Form bindings are covered by revision integrity"
        );
        row.extra_attributes["metadata"] = serde_json::json!(payload.metadata);
        let state = entry::EntryRow {
            entry_id: row.entry_id.clone(),
            legacy_saved_query_name: String::new(),
            form: SQL_FORM_NAME.into(),
            tags: Vec::new(),
            created_at: row.timestamp,
            updated_at: row.timestamp + 1.0,
            fields: row.fields.clone(),
            extra_attributes: row.extra_attributes.clone(),
            revision_id: "revision-delete".into(),
            parent_revision_id: Some(row.revision_id.clone()),
            integrity: row.integrity.clone(),
            deleted: true,
            deleted_at: Some(row.timestamp + 1.0),
            author: row.author.clone(),
            updated_by: "test".into(),
            deleted_by: Some("test".into()),
            entry_version: 2,
            legacy_columns: BTreeMap::new(),
        };
        row.revision_id = state.revision_id.clone();
        row.parent_revision_id = state.parent_revision_id.clone();
        row.fields = Value::Object(Map::new());
        row.extra_attributes = Value::Object(Map::new());
        row.operation = "delete".into();
        row.updated_by = "test".into();
        row.deleted_by = Some("test".into());
        row.entry_version = 2;
        row.state = Some(state);
        verify_revision_integrity(&row, &provider)
            .expect("valid Saved SQL tombstone uses its retained state");

        row.state.as_mut().unwrap().fields["sql"] =
            Value::String("SELECT 200 AS recovery_check".into());
        assert!(verify_revision_integrity(&row, &provider).is_err());
    }

    #[test]
    fn name_read_uses_field_then_null() {
        let entry = sql_entry_from_row(&row_with_saved_query_name(
            "Saved Query Name",
            sql_fields(Some("Field Name")),
        ))
        .expect("field name must win");
        assert_eq!(entry["name"], Value::String("Field Name".to_string()));

        let entry = sql_entry_from_row(&row_with_saved_query_name(
            "Saved Query Name",
            sql_fields(None),
        ))
        .expect("legacy carrier must be ignored by product logic");
        assert!(entry["name"].is_null());

        let entry = sql_entry_from_row(&row_with_saved_query_name("", sql_fields(None)))
            .expect("nameless records stay valid");
        assert!(entry["name"].is_null());
    }
}
