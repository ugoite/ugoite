use anyhow::{anyhow, Context, Result};
use arrow_array::{
    Array, BooleanArray, Int64Array, ListArray, StringArray, StructArray,
    TimestampMicrosecondArray, TimestampNanosecondArray,
};
use arrow_json::writer::ArrayWriter;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use datafusion::prelude::{array_has, col, lit, Expr};
use datafusion::scalar::ScalarValue;
use opendal::Operator;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use serde_yaml;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use ugoite_domain::form::{sql_column_name, sql_relation_name};
use ugoite_domain::id::FormId;
pub use ugoite_domain::text::compute_word_count;
use uuid::Uuid;

use crate::entry;
use crate::SpaceCheckpoint;
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_core::query::{
    AuthorizedQueryForm, AuthorizedQueryPolicy, EntryScope, QueryLimits, QuerySystemColumn,
};

pub const SQL_SESSION_MAX_ROWS: usize = 1_000;
/// A durable SQL-session policy may carry a sparse ID set only up to the same
/// hard window bound as its rows. Production creation uses `AllExcept`; the
/// public explicit-ID constructor uses `Only` and is bounded identically.
pub const SQL_SESSION_MAX_AUTHORIZATION_SCOPE_IDS: usize = SQL_SESSION_MAX_ROWS;
pub const SQL_SESSION_MAX_MEMORY_BYTES: usize = 64 * 1024 * 1024;
pub const SQL_SESSION_TIMEOUT: Duration = Duration::from_secs(30);

/// Immutable execution inputs for one authorized SQL-session page.
///
/// Keeping the bound parameters, checkpoint, and page together makes the
/// session's reproducible query coordinate explicit at its execution boundary.
pub struct AuthorizedSqlSessionPage {
    pub parameters: HashMap<String, datafusion::scalar::ScalarValue>,
    pub checkpoint: SpaceCheckpoint,
    pub offset: usize,
    pub limit: usize,
}

/// Durable, derived authorization policy for one SQL session. It is stored
/// beside the session metadata rather than in a checkpoint because a
/// checkpoint intentionally contains only storage coordinates.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SqlSessionQueryPolicy {
    pub forms: Vec<SqlSessionQueryForm>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SqlSessionQueryForm {
    pub form_id: FormId,
    pub relation: String,
    pub entry_scope: SqlSessionEntryScope,
    pub columns: BTreeSet<String>,
    pub system_columns: BTreeSet<SqlSessionSystemColumn>,
}

/// Serializable authorization boundary for one frozen SQL-session Form. The
/// `AllExcept` form keeps a sparse set of Entry-level ACL exceptions rather
/// than serializing every readable Entry in a large Form.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlSessionEntryScope {
    AllCurrent,
    Only(BTreeSet<String>),
    AllExcept(BTreeSet<String>),
}

impl SqlSessionEntryScope {
    fn as_query_scope(&self) -> EntryScope {
        match self {
            Self::AllCurrent => EntryScope::AllCurrent,
            Self::Only(entry_ids) => EntryScope::Only(entry_scope_from_ids(entry_ids)),
            Self::AllExcept(entry_ids) => EntryScope::AllExcept(entry_scope_from_ids(entry_ids)),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlSessionSystemColumn {
    ExternalId,
    Title,
    CreatedAt,
    UpdatedAt,
    EntryId,
    EntryVersion,
    CommittedAt,
}

impl SqlSessionQueryPolicy {
    fn authorized_query_policy(
        &self,
        checkpoint: SpaceCheckpoint,
    ) -> Result<AuthorizedQueryPolicy> {
        if self.forms.len() != 1 {
            return Err(anyhow!(
                "SQL session query policy must expose exactly one Form"
            ));
        }
        let mut forms = BTreeMap::new();
        for form in &self.forms {
            if forms.contains_key(&form.form_id) {
                return Err(anyhow!("SQL session query policy repeats a Form ID"));
            }
            forms.insert(
                form.form_id,
                AuthorizedQueryForm {
                    relation: form.relation.clone(),
                    entry_scope: form.entry_scope.as_query_scope(),
                    columns: form.columns.clone(),
                    system_columns: form
                        .system_columns
                        .iter()
                        .copied()
                        .map(SqlSessionSystemColumn::as_query_system_column)
                        .collect(),
                },
            );
        }
        if forms.is_empty() {
            return Err(anyhow!("SQL session query policy exposes no Forms"));
        }
        Ok(AuthorizedQueryPolicy {
            forms,
            checkpoint: Some(checkpoint),
            limits: sql_session_query_limits(),
        })
    }
}

impl SqlSessionSystemColumn {
    fn as_query_system_column(self) -> QuerySystemColumn {
        match self {
            Self::ExternalId => QuerySystemColumn::ExternalId,
            Self::Title => QuerySystemColumn::Title,
            Self::CreatedAt => QuerySystemColumn::CreatedAt,
            Self::UpdatedAt => QuerySystemColumn::UpdatedAt,
            Self::EntryId => QuerySystemColumn::EntryId,
            Self::EntryVersion => QuerySystemColumn::EntryVersion,
            Self::CommittedAt => QuerySystemColumn::CommittedAt,
        }
    }
}

fn sql_session_query_limits() -> QueryLimits {
    QueryLimits {
        max_memory_bytes: SQL_SESSION_MAX_MEMORY_BYTES,
        max_rows: SQL_SESSION_MAX_ROWS,
        timeout: SQL_SESSION_TIMEOUT,
        max_concurrency: 1,
        allowed_functions: BTreeSet::new(),
    }
}

/// Converts transport values to typed DataFusion parameters. A null must carry
/// a declared type, so it can never fall back to string substitution or an
/// untyped SQL NULL.
pub fn datafusion_parameters(
    values: &Map<String, Value>,
    types: &BTreeMap<String, String>,
) -> Result<HashMap<String, datafusion::scalar::ScalarValue>> {
    values
        .iter()
        .map(|(name, value)| {
            let kind = types
                .get(name)
                .ok_or_else(|| anyhow!("SQL parameter {name} has no declared type"))?;
            let scalar = match (kind.as_str(), value) {
                ("string", Value::String(value)) => {
                    datafusion::scalar::ScalarValue::Utf8(Some(value.clone()))
                }
                ("boolean", Value::Bool(value)) => {
                    datafusion::scalar::ScalarValue::Boolean(Some(*value))
                }
                ("integer", Value::Number(value)) => datafusion::scalar::ScalarValue::Int64(
                    value
                        .as_i64()
                        .ok_or_else(|| anyhow!("SQL parameter {name} must be an integer"))?
                        .into(),
                ),
                ("float", Value::Number(value)) => datafusion::scalar::ScalarValue::Float64(
                    value
                        .as_f64()
                        .ok_or_else(|| anyhow!("SQL parameter {name} must be a float"))?
                        .into(),
                ),
                ("timestamp", Value::String(value)) => {
                    let value = DateTime::parse_from_rfc3339(value)
                        .map_err(|_| anyhow!("SQL parameter {name} must be an RFC3339 timestamp"))?
                        .timestamp_micros();
                    datafusion::scalar::ScalarValue::TimestampMicrosecond(Some(value), None)
                }
                ("date", Value::String(value)) => {
                    let value = NaiveDate::parse_from_str(value, "%Y-%m-%d")
                        .map_err(|_| anyhow!("SQL parameter {name} must be an ISO date"))?;
                    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1)
                        .expect("the Unix epoch is a valid date");
                    datafusion::scalar::ScalarValue::Date32(Some((value - epoch).num_days() as i32))
                }
                ("string", Value::Null) => datafusion::scalar::ScalarValue::Utf8(None),
                ("boolean", Value::Null) => datafusion::scalar::ScalarValue::Boolean(None),
                ("integer", Value::Null) => datafusion::scalar::ScalarValue::Int64(None),
                ("float", Value::Null) => datafusion::scalar::ScalarValue::Float64(None),
                ("timestamp", Value::Null) => {
                    datafusion::scalar::ScalarValue::TimestampMicrosecond(None, None)
                }
                ("date", Value::Null) => datafusion::scalar::ScalarValue::Date32(None),
                ("string" | "boolean" | "integer" | "float" | "timestamp" | "date", _) => {
                    return Err(anyhow!(
                        "SQL parameter {name} does not match declared type {kind}"
                    ))
                }
                _ => return Err(anyhow!("SQL parameter {name} has unsupported type {kind}")),
            };
            Ok((name.clone(), scalar))
        })
        .collect()
}

pub async fn datafusion_parameter_names(
    _op: &Operator,
    _ws_path: &str,
    sql: &str,
) -> Result<BTreeSet<String>> {
    use datafusion::sql::parser::{DFParser, Statement};
    use datafusion::sql::sqlparser::ast::{visit_expressions, Expr, Value};
    use std::ops::ControlFlow;

    // Saving SQL validates only DataFusion syntax and native placeholders.
    // Relation resolution remains an execution-time concern: a saved query may
    // legitimately target a Form that an operator creates later.
    let statements = DFParser::parse_sql(sql).context("parse saved SQL with DataFusion")?;
    let mut names = BTreeSet::new();
    for statement in statements {
        if let Statement::Statement(statement) = statement {
            let _ = visit_expressions(statement.as_ref(), |expression| {
                if let Expr::Value(value) = expression {
                    if let Value::Placeholder(name) = &value.value {
                        names.insert(name.clone());
                    }
                }
                ControlFlow::<()>::Continue(())
            });
        }
    }
    Ok(names)
}

pub async fn query_index(op: &Operator, ws_path: &str, query: &str) -> Result<Vec<Value>> {
    let scopes = all_current_form_scopes(op, ws_path).await?;
    query_index_with_form_scopes(op, ws_path, query, &scopes).await
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EntryCandidate {
    pub form_name: String,
    pub entry_id: String,
    pub title: String,
    pub created_at: f64,
    pub updated_at: f64,
}

/// Selects only the bounded, globally ordered current Entry candidates.
pub(crate) async fn query_entry_candidates_authorized(
    op: &Operator,
    ws_path: &str,
    relation_scopes: &BTreeMap<String, EntryScope>,
    form_filter: Option<&str>,
    keyword: Option<&str>,
    limit: usize,
) -> Result<Vec<EntryCandidate>> {
    query_entry_candidates_authorized_after(
        op,
        ws_path,
        relation_scopes,
        form_filter,
        keyword,
        limit,
        None,
    )
    .await
}

pub(crate) async fn query_entry_candidates_authorized_after(
    op: &Operator,
    ws_path: &str,
    relation_scopes: &BTreeMap<String, EntryScope>,
    form_filter: Option<&str>,
    keyword: Option<&str>,
    limit: usize,
    after: Option<(&str, &str, &str)>,
) -> Result<Vec<EntryCandidate>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    if limit > crate::MAX_NORMAL_READ_ROWS {
        return Err(anyhow!(
            "normal Entry reads are limited to {} rows",
            crate::MAX_NORMAL_READ_ROWS
        ));
    }
    let forms = load_forms(op, ws_path).await?;
    let context = datafusion_sql_context_with_limits(
        op,
        ws_path,
        EntryScope::AllCurrent,
        None,
        Some(relation_scopes),
        None,
        BTreeSet::from(["array_to_string".to_string(), "lower".to_string()]),
        crate::MAX_NORMAL_READ_ROWS,
        false,
    )
    .await
    .map_err(map_sql_error)?;
    query_entry_candidates_in_context(
        &context,
        &forms,
        relation_scopes,
        form_filter,
        keyword,
        limit,
        0,
        after,
    )
    .await
}

async fn query_entry_candidates_in_context(
    context: &crate::query_context::AuthorizedQueryContext,
    forms: &HashMap<String, Value>,
    relation_scopes: &BTreeMap<String, EntryScope>,
    form_filter: Option<&str>,
    keyword: Option<&str>,
    limit: usize,
    offset: usize,
    after: Option<(&str, &str, &str)>,
) -> Result<Vec<EntryCandidate>> {
    let normalized_form = form_filter.map(str::trim).filter(|value| !value.is_empty());
    let normalized_keyword = keyword
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_lowercase);
    let mut branches = Vec::new();
    for (form_name, form) in forms {
        if normalized_form.is_some_and(|expected| expected != form_name) {
            continue;
        }
        let relation = form
            .get("sql_relation")
            .and_then(Value::as_str)
            .with_context(|| format!("Form {form_name} is missing its SQL relation"))?;
        if !relation_scopes.contains_key(&form_name.to_ascii_lowercase())
            && !relation_scopes.contains_key(&relation.to_ascii_lowercase())
        {
            continue;
        }
        let keyword_predicate = normalized_keyword
            .as_deref()
            .map(|query| searchable_keyword_predicate(form, form_name, query))
            .transpose()?;
        let where_clause = keyword_predicate
            .map(|predicate| format!(" WHERE {predicate}"))
            .unwrap_or_default();
        branches.push(format!(
            "SELECT \"_ugoite_id\", \"_ugoite_title\", \"_ugoite_created_at\", \"_ugoite_updated_at\", {} AS \"_ugoite_form\" FROM {}{}",
            sql_string_literal(form_name),
            quote_identifier(relation),
            where_clause,
        ));
    }
    if branches.is_empty() {
        return Ok(Vec::new());
    }
    let after_clause = after
        .map(|(title, id, form)| {
            format!(
                " WHERE (\"_ugoite_title\" > {title} OR (\"_ugoite_title\" = {title} AND \"_ugoite_id\" > {id}) OR (\"_ugoite_title\" = {title} AND \"_ugoite_id\" = {id} AND \"_ugoite_form\" > {form}))",
                title = sql_string_literal(title),
                id = sql_string_literal(id),
                form = sql_string_literal(form),
            )
        })
        .unwrap_or_default();
    let sql = format!(
        "SELECT \"_ugoite_id\", \"_ugoite_title\", \"_ugoite_created_at\", \"_ugoite_updated_at\", \"_ugoite_form\" FROM ({}) AS \"_ugoite_entry_candidates\"{} ORDER BY \"_ugoite_title\", \"_ugoite_id\", \"_ugoite_form\" LIMIT {} OFFSET {}",
        branches.join(" UNION ALL "),
        after_clause,
        limit,
        offset,
    );
    let values = record_batches_to_values(&context.execute(&sql).await.map_err(map_sql_error)?)?;
    let mut candidates = values
        .into_iter()
        .map(|value| {
            Ok(EntryCandidate {
                form_name: value
                    .get("_ugoite_form")
                    .and_then(Value::as_str)
                    .context("candidate plan is missing Form name")?
                    .to_string(),
                entry_id: value
                    .get("_ugoite_id")
                    .and_then(Value::as_str)
                    .context("candidate plan is missing Entry ID")?
                    .to_string(),
                title: value
                    .get("_ugoite_title")
                    .and_then(Value::as_str)
                    .context("candidate plan is missing Entry title")?
                    .to_string(),
                created_at: value
                    .get("_ugoite_created_at")
                    .and_then(Value::as_f64)
                    .or_else(|| {
                        value
                            .get("_ugoite_created_at")
                            .and_then(Value::as_i64)
                            .map(|value| value as f64)
                    })
                    .or_else(|| {
                        value
                            .get("_ugoite_created_at")
                            .and_then(Value::as_str)
                            .and_then(|value| {
                                chrono::DateTime::parse_from_rfc3339(value)
                                    .ok()
                                    .map(|value| value.timestamp_millis() as f64 / 1000.0)
                            })
                    })
                    .context("candidate plan is missing Entry creation time")?,
                updated_at: value
                    .get("_ugoite_updated_at")
                    .and_then(Value::as_f64)
                    .or_else(|| {
                        value
                            .get("_ugoite_updated_at")
                            .and_then(Value::as_i64)
                            .map(|value| value as f64)
                    })
                    .or_else(|| {
                        value
                            .get("_ugoite_updated_at")
                            .and_then(Value::as_str)
                            .and_then(|value| {
                                chrono::DateTime::parse_from_rfc3339(value)
                                    .ok()
                                    .map(|value| value.timestamp_millis() as f64 / 1000.0)
                            })
                    })
                    .context("candidate plan is missing Entry update time")?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    candidates.truncate(limit);
    Ok(candidates)
}

/// Reads the selected payload through the same authorized context that chose
/// the candidates. Each Form is projected once, so a list/search response is
/// bounded by Forms rather than by Entry point reads.
pub(crate) async fn query_entry_rows_authorized(
    op: &Operator,
    ws_path: &str,
    relation_scopes: &BTreeMap<String, EntryScope>,
    form_filter: Option<&str>,
    keyword: Option<&str>,
    limit: usize,
    offset: usize,
) -> Result<Vec<(String, entry::EntryRow)>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    if limit > crate::MAX_NORMAL_READ_ROWS.saturating_add(1) {
        return Err(anyhow!(
            "normal Entry reads are limited to {} rows",
            crate::MAX_NORMAL_READ_ROWS
        ));
    }
    let forms = load_forms(op, ws_path).await?;
    let context = datafusion_sql_context_with_limits(
        op,
        ws_path,
        EntryScope::AllCurrent,
        None,
        Some(relation_scopes),
        None,
        BTreeSet::from(["array_to_string".to_string(), "lower".to_string()]),
        crate::MAX_NORMAL_READ_ROWS,
        true,
    )
    .await
    .map_err(map_sql_error)?;
    let candidates = query_entry_candidates_in_context(
        &context,
        &forms,
        relation_scopes,
        form_filter,
        keyword,
        limit,
        offset,
        None,
    )
    .await?;
    let mut by_key = HashMap::<(String, String), entry::EntryRow>::new();
    for form_name in forms.keys() {
        let form_candidates = candidates
            .iter()
            .filter(|candidate| candidate.form_name == *form_name)
            .collect::<Vec<_>>();
        if form_candidates.is_empty() {
            continue;
        }
        let form = forms
            .get(form_name)
            .with_context(|| format!("missing Form definition {form_name}"))?;
        let relation = form
            .get("sql_relation")
            .and_then(Value::as_str)
            .with_context(|| format!("Form {form_name} is missing its SQL relation"))?;
        let ids = form_candidates
            .iter()
            .map(|candidate| lit(candidate.entry_id.as_str()))
            .collect::<Vec<_>>();
        let batches = execute_payload_relation_plan(
            &context,
            relation,
            Vec::new(),
            vec![col("_ugoite_id").in_list(ids, false)],
            form,
            false,
            form_candidates.len(),
        )
        .await?;
        for row in entry_rows_from_batches(form_name, form, &batches)? {
            let entry_id = row.entry_id.clone();
            by_key.insert((form_name.clone(), entry_id.to_string()), row);
        }
    }
    Ok(candidates
        .into_iter()
        .filter_map(|candidate| {
            by_key
                .remove(&(candidate.form_name.clone(), candidate.entry_id.clone()))
                .map(|row| (candidate.form_name, row))
        })
        .collect::<Vec<_>>())
}

/// Reads one Form's current payload projection in a single authorized plan.
/// This is the typed-field path for Form-owned records such as Saved SQL; it
/// does not materialize the Form's full revision history in Rust.
pub(crate) async fn query_form_entry_rows_authorized(
    op: &Operator,
    ws_path: &str,
    form_name: &str,
    entry_scope: EntryScope,
    field_filter: Option<(&str, &Value)>,
    limit: usize,
) -> Result<Vec<entry::EntryRow>> {
    if limit == 0 || limit > crate::MAX_NORMAL_READ_ROWS.saturating_add(1) {
        return Err(anyhow!(
            "normal Entry reads are limited to {} rows",
            crate::MAX_NORMAL_READ_ROWS
        ));
    }
    let forms = load_forms(op, ws_path).await?;
    let form = forms
        .get(form_name)
        .with_context(|| format!("Form {form_name} was not found"))?;
    let relation = form
        .get("sql_relation")
        .and_then(Value::as_str)
        .with_context(|| format!("Form {form_name} is missing its SQL relation"))?;
    let relation_scopes = BTreeMap::from([(form_name.to_ascii_lowercase(), entry_scope)]);
    let context = datafusion_sql_context_with_limits(
        op,
        ws_path,
        EntryScope::AllCurrent,
        None,
        Some(&relation_scopes),
        None,
        BTreeSet::new(),
        crate::MAX_NORMAL_READ_ROWS,
        true,
    )
    .await
    .map_err(map_sql_error)?;
    let predicates: Vec<Expr> = match field_filter {
        None => Vec::new(),
        Some((field_name, expected)) => {
            let definition = form
                .get("fields")
                .and_then(Value::as_object)
                .and_then(|fields| fields.get(field_name))
                .with_context(|| format!("Form {form_name} is missing field {field_name}"))?;
            let column = field_sql_column(definition)?;
            let field_type = definition
                .get("type")
                .and_then(Value::as_str)
                .context("Form field is missing its type")?;
            vec![col(column).eq(filter_literal(expected, field_type)?)]
        }
    };
    let batches = execute_payload_relation_plan(
        &context,
        relation,
        Vec::new(),
        predicates,
        form,
        true,
        limit,
    )
    .await?;
    entry_rows_from_batches(form_name, form, &batches)
}

#[allow(clippy::too_many_arguments)]
async fn execute_payload_relation_plan(
    context: &crate::query_context::AuthorizedQueryContext,
    relation: &str,
    unnest_columns: Vec<(String, String)>,
    predicates: Vec<Expr>,
    form: &Value,
    distinct: bool,
    limit: usize,
) -> Result<Vec<arrow_array::RecordBatch>> {
    let preserved_inputs = unnest_columns
        .iter()
        .map(|(input, _)| input.clone())
        .collect::<BTreeSet<_>>();
    context
        .execute_relation_plan(
            relation,
            &unnest_columns,
            predicates,
            payload_projection(form, &preserved_inputs)?,
            Vec::new(),
            distinct,
            !preserved_inputs.is_empty(),
            limit,
        )
        .await
        .map_err(map_sql_error)
}

fn payload_projection(form: &Value, preserved_inputs: &BTreeSet<String>) -> Result<Vec<Expr>> {
    let mut projection = vec![
        col("_ugoite_id"),
        col("_ugoite_title"),
        col("_ugoite_tags"),
        col("_ugoite_created_at"),
        col("_ugoite_updated_at"),
        col("_ugoite_revision_id"),
        col("_ugoite_parent_revision_id"),
        col("_ugoite_author"),
        col("_ugoite_updated_by"),
        col("_ugoite_deleted_by"),
        col("_ugoite_extra_attributes"),
        col("_ugoite_integrity"),
        col("_ugoite_deleted"),
        col("_ugoite_deleted_at"),
        col("_ugoite_entry_version"),
    ];
    if let Some(fields) = form.get("fields").and_then(Value::as_object) {
        for field in fields.values() {
            let column = field_sql_column(field)?;
            if preserved_inputs.contains(&column) {
                projection.push(
                    col(crate::query_context::preserved_unnest_column(&column)).alias(column),
                );
            } else {
                // Iceberg's current-schema provider owns stable field-ID
                // projection and typed null materialization for older files.
                // Keep every current Form column in the plan; do not infer
                // schema evolution from a relation's physical columns.
                projection.push(col(&column).alias(column));
            }
        }
    }
    Ok(projection)
}

fn entry_rows_from_batches(
    form_name: &str,
    form: &Value,
    batches: &[arrow_array::RecordBatch],
) -> Result<Vec<entry::EntryRow>> {
    let mut rows = Vec::new();
    for batch in batches {
        for row in 0..batch.num_rows() {
            rows.push(entry_row_from_batch(form_name, form, batch, row)?);
        }
    }
    Ok(rows)
}

fn entry_row_from_batch(
    form_name: &str,
    form: &Value,
    batch: &arrow_array::RecordBatch,
    row: usize,
) -> Result<entry::EntryRow> {
    let fields = form
        .get("fields")
        .and_then(Value::as_object)
        .context("Form definition is missing fields")?
        .iter()
        .map(|(name, definition)| {
            let column = field_sql_column(definition)?;
            let field = batch
                .column_by_name(&column)
                .with_context(|| format!("Entry payload is missing field column {column}"))?;
            let field_type: ugoite_domain::form::FieldType = serde_json::from_value(
                definition
                    .get("type")
                    .cloned()
                    .context("Form field is missing its type")?,
            )?;
            let list_item = definition
                .get("items")
                .filter(|items| !items.is_null())
                .cloned()
                .map(serde_json::from_value)
                .transpose()?;
            let field_value =
                crate::field_value_at(field.as_ref(), row, &field_type, list_item.as_ref())?
                    .unwrap_or(ugoite_domain::entry::FieldValue::Null);
            Ok((
                name.clone(),
                serde_json::to_value(field_value).context("encode typed Entry field")?,
            ))
        })
        .collect::<Result<Map<_, _>>>()?;
    Ok(entry::EntryRow {
        entry_id: required_string_column(batch, row, "_ugoite_id", "external ID")?,
        title: required_string_column(batch, row, "_ugoite_title", "title")?,
        form: form_name.to_string(),
        tags: required_string_list_column(batch, row, "_ugoite_tags", "tags")?,
        created_at: required_timestamp_seconds_column(
            batch,
            row,
            "_ugoite_created_at",
            "created_at",
        )?,
        updated_at: required_timestamp_seconds_column(
            batch,
            row,
            "_ugoite_updated_at",
            "updated_at",
        )?,
        fields: Value::Object(fields),
        extra_attributes: extra_attributes_column(batch, row)?,
        revision_id: required_uuid_string_column(batch, row, "_ugoite_revision_id", "revision ID")?,
        parent_revision_id: optional_uuid_string_column(batch, row, "_ugoite_parent_revision_id")?,
        integrity: required_integrity_column(batch, row)?,
        deleted: required_bool_column(batch, row, "_ugoite_deleted", "deleted")?,
        deleted_at: optional_timestamp_seconds_column(batch, row, "_ugoite_deleted_at")?,
        author: required_string_column(batch, row, "_ugoite_author", "author")?,
        updated_by: required_string_column(batch, row, "_ugoite_updated_by", "updated_by")?,
        deleted_by: optional_string_column(batch, row, "_ugoite_deleted_by")?,
        entry_version: required_u64_column(batch, row)?,
    })
}

fn field_sql_column(field: &Value) -> Result<String> {
    if let Some(id) = field.get("id").and_then(Value::as_i64) {
        return Ok(format!("field_{id}"));
    }
    field
        .get("sql_column")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .context("Form field is missing stable id")
}

fn payload_column<'a>(batch: &'a arrow_array::RecordBatch, name: &str) -> Result<&'a dyn Array> {
    batch
        .column_by_name(name)
        .map(|column| column.as_ref())
        .with_context(|| format!("Entry payload is missing system column {name}"))
}

fn required_string_column(
    batch: &arrow_array::RecordBatch,
    row: usize,
    name: &str,
    label: &str,
) -> Result<String> {
    let column = payload_column(batch, name)?;
    let values = column
        .as_any()
        .downcast_ref::<StringArray>()
        .with_context(|| format!("Entry payload has invalid {label} Arrow type"))?;
    if values.is_null(row) {
        return Err(anyhow!("Entry payload is missing {label}"));
    }
    Ok(values.value(row).to_owned())
}

fn optional_string_column(
    batch: &arrow_array::RecordBatch,
    row: usize,
    name: &str,
) -> Result<Option<String>> {
    let column = payload_column(batch, name)?;
    let values = column
        .as_any()
        .downcast_ref::<StringArray>()
        .with_context(|| format!("Entry payload has invalid {name} Arrow type"))?;
    Ok((!values.is_null(row)).then(|| values.value(row).to_owned()))
}

fn required_uuid_string_column(
    batch: &arrow_array::RecordBatch,
    row: usize,
    name: &str,
    label: &str,
) -> Result<String> {
    let column = payload_column(batch, name)?;
    Ok(crate::uuid_value_at(column, row)
        .with_context(|| format!("Entry payload has invalid {label}"))?
        .to_string())
}

fn optional_uuid_string_column(
    batch: &arrow_array::RecordBatch,
    row: usize,
    name: &str,
) -> Result<Option<String>> {
    let column = payload_column(batch, name)?;
    if column.is_null(row) {
        return Ok(None);
    }
    Ok(Some(
        crate::uuid_value_at(column, row)
            .with_context(|| format!("Entry payload has invalid {name}"))?
            .to_string(),
    ))
}

fn required_string_list_column(
    batch: &arrow_array::RecordBatch,
    row: usize,
    name: &str,
    label: &str,
) -> Result<Vec<String>> {
    let column = payload_column(batch, name)?;
    let values = column
        .as_any()
        .downcast_ref::<ListArray>()
        .with_context(|| format!("Entry payload has invalid {label} Arrow type"))?;
    if values.is_null(row) {
        return Err(anyhow!("Entry payload is missing {label}"));
    }
    let items = values.value(row);
    let items = items
        .as_any()
        .downcast_ref::<StringArray>()
        .with_context(|| format!("Entry payload has invalid {label} item Arrow type"))?;
    (0..items.len())
        .map(|index| {
            if items.is_null(index) {
                return Err(anyhow!("Entry payload has a null {label} item"));
            }
            Ok(items.value(index).to_owned())
        })
        .collect::<Result<Vec<_>>>()
}

fn required_timestamp_seconds_column(
    batch: &arrow_array::RecordBatch,
    row: usize,
    name: &str,
    label: &str,
) -> Result<f64> {
    let column = payload_column(batch, name)?;
    if column.is_null(row) {
        return Err(anyhow!("Entry payload is missing {label}"));
    }
    if let Some(values) = column.as_any().downcast_ref::<TimestampMicrosecondArray>() {
        return Ok(values.value(row) as f64 / 1_000_000.0);
    }
    if let Some(values) = column.as_any().downcast_ref::<TimestampNanosecondArray>() {
        return Ok(values.value(row) as f64 / 1_000_000_000.0);
    }
    Err(anyhow!("Entry payload has invalid {label} Arrow type"))
}

fn optional_timestamp_seconds_column(
    batch: &arrow_array::RecordBatch,
    row: usize,
    name: &str,
) -> Result<Option<f64>> {
    let column = payload_column(batch, name)?;
    if column.is_null(row) {
        return Ok(None);
    }
    Ok(Some(required_timestamp_seconds_column(
        batch, row, name, name,
    )?))
}

fn required_bool_column(
    batch: &arrow_array::RecordBatch,
    row: usize,
    name: &str,
    label: &str,
) -> Result<bool> {
    let column = payload_column(batch, name)?;
    let values = column
        .as_any()
        .downcast_ref::<BooleanArray>()
        .with_context(|| format!("Entry payload has invalid {label} Arrow type"))?;
    if values.is_null(row) {
        return Err(anyhow!("Entry payload is missing {label}"));
    }
    Ok(values.value(row))
}

fn required_u64_column(batch: &arrow_array::RecordBatch, row: usize) -> Result<u64> {
    let column = payload_column(batch, "_ugoite_entry_version")?;
    let values = column
        .as_any()
        .downcast_ref::<Int64Array>()
        .context("Entry payload has invalid entry version Arrow type")?;
    if values.is_null(row) {
        return Err(anyhow!("Entry payload is missing entry version"));
    }
    u64::try_from(values.value(row)).context("Entry payload has a negative entry version")
}

fn required_integrity_column(
    batch: &arrow_array::RecordBatch,
    row: usize,
) -> Result<entry::IntegrityPayload> {
    let column = payload_column(batch, "_ugoite_integrity")?;
    let values = column
        .as_any()
        .downcast_ref::<StructArray>()
        .context("Entry payload has invalid integrity Arrow type")?;
    if values.is_null(row) {
        return Err(anyhow!("Entry payload is missing integrity"));
    }
    let checksum = values
        .column_by_name("checksum")
        .and_then(|column| column.as_any().downcast_ref::<StringArray>())
        .filter(|values| !values.is_null(row))
        .map(|values| values.value(row).to_owned())
        .context("Entry payload integrity is missing checksum")?;
    let signature = values
        .column_by_name("signature")
        .and_then(|column| column.as_any().downcast_ref::<StringArray>())
        .filter(|values| !values.is_null(row))
        .map(|values| values.value(row).to_owned())
        .context("Entry payload integrity is missing signature")?;
    Ok(entry::IntegrityPayload {
        checksum,
        signature,
    })
}

fn extra_attributes_column(batch: &arrow_array::RecordBatch, row: usize) -> Result<Value> {
    let column = payload_column(batch, "_ugoite_extra_attributes")?;
    let values = column
        .as_any()
        .downcast_ref::<StringArray>()
        .context("Entry payload has invalid extra_attributes Arrow type")?;
    if values.is_null(row) {
        return Ok(Value::Object(Map::new()));
    }
    let parsed: Value = serde_json::from_str(values.value(row))
        .context("Entry payload has invalid extra_attributes JSON")?;
    if !parsed.is_object() {
        return Err(anyhow!(
            "Entry payload extra_attributes JSON must be an object"
        ));
    }
    Ok(parsed)
}

fn searchable_keyword_predicate(form: &Value, form_name: &str, query: &str) -> Result<String> {
    let pattern = sql_like_literal(query);
    let mut expressions = vec![
        format!(
            "lower(\"_ugoite_id\") LIKE {pattern} ESCAPE {}",
            sql_string_literal("\\")
        ),
        format!(
            "lower(\"_ugoite_title\") LIKE {pattern} ESCAPE {}",
            sql_string_literal("\\")
        ),
        format!(
            "lower(array_to_string(\"_ugoite_tags\", ' ')) LIKE {pattern} ESCAPE {}",
            sql_string_literal("\\")
        ),
        format!(
            "lower({}) LIKE {pattern} ESCAPE {}",
            sql_string_literal(form_name),
            sql_string_literal("\\")
        ),
    ];
    if let Some(fields) = form.get("fields").and_then(Value::as_object) {
        for field in fields.values() {
            let Some(column) = field.get("sql_column").and_then(Value::as_str) else {
                continue;
            };
            let field_type = field
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("string");
            let expression = match field_type {
                "string" | "markdown" | "sql" | "boolean" | "integer" | "long" | "float"
                | "double" | "date" | "time" | "timestamp" | "timestamp_tz" | "timestamp_ns"
                | "timestamp_tz_ns" | "uuid" | "row_reference" => format!(
                    "lower(CAST({} AS VARCHAR)) LIKE {pattern} ESCAPE {}",
                    quote_identifier(column),
                    sql_string_literal("\\")
                ),
                "list" => {
                    let item_type = field
                        .get("items")
                        .and_then(Value::as_object)
                        .and_then(|items| items.get("type"))
                        .and_then(Value::as_str)
                        .unwrap_or("string");
                    if matches!(
                        item_type,
                        "string"
                            | "markdown"
                            | "sql"
                            | "boolean"
                            | "integer"
                            | "long"
                            | "float"
                            | "double"
                            | "date"
                            | "time"
                            | "timestamp"
                            | "timestamp_tz"
                            | "timestamp_ns"
                            | "timestamp_tz_ns"
                            | "uuid"
                            | "row_reference"
                    ) {
                        format!(
                            "lower(array_to_string({}, ' ')) LIKE {pattern} ESCAPE {}",
                            quote_identifier(column),
                            sql_string_literal("\\")
                        )
                    } else {
                        continue;
                    }
                }
                "binary" | "object_list" | "asset_reference" => continue,
                _ => continue,
            };
            expressions.push(expression);
        }
    }
    Ok(expressions.join(" OR "))
}

pub async fn execute_sql_query(
    op: &Operator,
    ws_path: &str,
    sql_query: &str,
) -> Result<Vec<Value>> {
    execute_datafusion_sql(
        op,
        ws_path,
        sql_query,
        EntryScope::AllCurrent,
        None,
        None,
        None,
    )
    .await
}

/// Checks whether an Entry ID is present in a Form's current revision view.
/// The stable external ID is the public Entry reference value; resolving it
/// through this view also excludes deleted and historical revisions.
pub async fn current_entry_exists_in_form(
    op: &Operator,
    ws_path: &str,
    form_name: &str,
    entry_id: &str,
    entry_scope: EntryScope,
) -> Result<bool> {
    let sql = format!(
        "SELECT 1 FROM {} WHERE _ugoite_id = {} LIMIT 1",
        sql_identifier(&form_name.to_ascii_lowercase()),
        sql_string_literal(entry_id)
    );
    Ok(!execute_datafusion_sql_with_functions(
        op,
        ws_path,
        &sql,
        entry_scope,
        None,
        None,
        None,
        BTreeSet::new(),
    )
    .await?
    .is_empty())
}

fn sql_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn sql_string_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn sql_like_literal(value: &str) -> String {
    let mut pattern = String::from("%");
    for character in value.chars() {
        if matches!(character, '\\' | '%' | '_') {
            pattern.push('\\');
        }
        pattern.push(character);
    }
    pattern.push('%');
    sql_string_literal(&pattern)
}

pub async fn execute_sql_query_page(
    op: &Operator,
    ws_path: &str,
    sql_query: &str,
    offset: usize,
    limit: usize,
) -> Result<(Vec<Value>, u64)> {
    execute_sql_query_page_with_parameters(op, ws_path, sql_query, HashMap::new(), offset, limit)
        .await
}

pub async fn execute_sql_query_page_with_parameters(
    op: &Operator,
    ws_path: &str,
    sql_query: &str,
    parameters: HashMap<String, datafusion::scalar::ScalarValue>,
    offset: usize,
    limit: usize,
) -> Result<(Vec<Value>, u64)> {
    execute_datafusion_sql_page(
        op,
        ws_path,
        sql_query,
        EntryScope::AllCurrent,
        None,
        None,
        None,
        offset,
        limit,
        parameters,
    )
    .await
}

pub async fn execute_sql_query_authorized(
    op: &Operator,
    ws_path: &str,
    sql_query: &str,
    readable_entry_ids: &HashSet<String>,
) -> Result<Vec<Value>> {
    execute_datafusion_sql(
        op,
        ws_path,
        sql_query,
        EntryScope::Only(entry_scope(readable_entry_ids)),
        None,
        None,
        None,
    )
    .await
}

/// Production authorization entry point. The service supplies a relation
/// scoped map rather than one global Entry-ID set, so a Form with no readable
/// Entries is never registered and remains unresolvable to DataFusion.
pub async fn execute_sql_query_authorized_by_form(
    op: &Operator,
    ws_path: &str,
    sql_query: &str,
    readable_entries_by_form: &BTreeMap<String, HashSet<String>>,
) -> Result<Vec<Value>> {
    let relation_scopes = readable_entries_by_form
        .iter()
        .map(|(relation, entries)| {
            (
                relation.to_ascii_lowercase(),
                EntryScope::Only(entry_scope(entries)),
            )
        })
        .collect::<BTreeMap<_, _>>();
    execute_datafusion_sql(
        op,
        ws_path,
        sql_query,
        EntryScope::AllCurrent,
        None,
        Some(&relation_scopes),
        None,
    )
    .await
}

pub async fn execute_sql_query_authorized_by_form_scopes(
    op: &Operator,
    ws_path: &str,
    sql_query: &str,
    relation_scopes: &BTreeMap<String, EntryScope>,
) -> Result<Vec<Value>> {
    execute_datafusion_sql(
        op,
        ws_path,
        sql_query,
        EntryScope::AllCurrent,
        None,
        Some(relation_scopes),
        None,
    )
    .await
}

pub async fn execute_sql_query_authorized_by_form_page(
    op: &Operator,
    ws_path: &str,
    sql_query: &str,
    readable_entries_by_form: &BTreeMap<String, HashSet<String>>,
    offset: usize,
    limit: usize,
) -> Result<(Vec<Value>, u64)> {
    execute_sql_query_authorized_by_form_page_with_parameters(
        op,
        ws_path,
        sql_query,
        readable_entries_by_form,
        HashMap::new(),
        offset,
        limit,
    )
    .await
}

pub async fn execute_sql_query_authorized_by_form_page_with_parameters(
    op: &Operator,
    ws_path: &str,
    sql_query: &str,
    readable_entries_by_form: &BTreeMap<String, HashSet<String>>,
    parameters: HashMap<String, datafusion::scalar::ScalarValue>,
    offset: usize,
    limit: usize,
) -> Result<(Vec<Value>, u64)> {
    let relation_scopes = readable_entries_by_form
        .iter()
        .map(|(relation, entries)| {
            (
                relation.to_ascii_lowercase(),
                EntryScope::Only(entry_scope(entries)),
            )
        })
        .collect::<BTreeMap<_, _>>();
    execute_datafusion_sql_page(
        op,
        ws_path,
        sql_query,
        EntryScope::AllCurrent,
        None,
        Some(&relation_scopes),
        None,
        offset,
        limit,
        parameters,
    )
    .await
}

pub async fn execute_sql_query_authorized_by_form_page_at_checkpoint(
    op: &Operator,
    ws_path: &str,
    sql_query: &str,
    policy: &SqlSessionQueryPolicy,
    page: AuthorizedSqlSessionPage,
) -> Result<(Vec<Value>, u64)> {
    let context = datafusion_sql_session_context(op, ws_path, policy, page.checkpoint)
        .await
        .map_err(map_sql_error)?;
    let (batches, count) = context
        .execute_session_page(sql_query, page.parameters, page.offset, page.limit)
        .await
        .map_err(map_sql_error)?;
    Ok((record_batches_to_values(&batches)?, count))
}

/// Executes a count-only session plan. It shares the frozen policy and
/// checkpoint used for pages but never materializes a sentinel page row.
pub async fn execute_sql_query_authorized_by_form_count_at_checkpoint(
    op: &Operator,
    ws_path: &str,
    sql_query: &str,
    policy: &SqlSessionQueryPolicy,
    parameters: HashMap<String, datafusion::scalar::ScalarValue>,
    checkpoint: SpaceCheckpoint,
) -> Result<u64> {
    let context = datafusion_sql_session_context(op, ws_path, policy, checkpoint)
        .await
        .map_err(map_sql_error)?;
    context
        .execute_session_count(sql_query, parameters)
        .await
        .map_err(map_sql_error)
}

/// Validates a SQL session query at creation against only frozen policy and
/// checkpoint inputs. The live Form registry is deliberately absent.
pub async fn validate_sql_session_query_at_checkpoint(
    op: &Operator,
    ws_path: &str,
    sql_query: &str,
    policy: &SqlSessionQueryPolicy,
    parameters: HashMap<String, datafusion::scalar::ScalarValue>,
    checkpoint: SpaceCheckpoint,
) -> Result<()> {
    validate_sql_session_page_shape(sql_query)?;
    let context = datafusion_sql_session_context(op, ws_path, policy, checkpoint)
        .await
        .map_err(map_sql_error)?;
    context
        .validate_session_with_parameters(sql_query, parameters)
        .await
        .map_err(map_sql_error)
}

/// Derives a serializable SQL-session policy from the Form definitions stored
/// in a checkpoint. A persisted session policy is derived metadata, never
/// Catalog or execution authority; callers recreate and compare it at every
/// use before executing with the rebuilt value.
pub async fn sql_session_query_policy_at_checkpoint(
    op: &Operator,
    ws_path: &str,
    relation: &str,
    entry_scope: SqlSessionEntryScope,
    checkpoint: &SpaceCheckpoint,
) -> Result<SqlSessionQueryPolicy> {
    let workspace = crate::iceberg_store::native_workspace(op, ws_path).await?;
    let relation = relation.to_ascii_lowercase();
    let form = workspace.form_at_checkpoint(checkpoint, &relation).await?;
    Ok(SqlSessionQueryPolicy {
        forms: vec![SqlSessionQueryForm {
            form_id: form.id,
            relation,
            entry_scope,
            columns: form
                .fields
                .into_iter()
                .map(|field| sql_column_name(field.id))
                .collect(),
            system_columns: [
                SqlSessionSystemColumn::ExternalId,
                SqlSessionSystemColumn::Title,
                SqlSessionSystemColumn::CreatedAt,
                SqlSessionSystemColumn::UpdatedAt,
            ]
            .into_iter()
            .collect(),
        }],
    })
}

async fn datafusion_sql_session_context(
    op: &Operator,
    ws_path: &str,
    policy: &SqlSessionQueryPolicy,
    checkpoint: SpaceCheckpoint,
) -> Result<crate::query_context::AuthorizedQueryContext> {
    crate::iceberg_store::native_workspace(op, ws_path)
        .await?
        .authorized_query_context(policy.authorized_query_policy(checkpoint)?)
        .await
        .context("create frozen DataFusion SQL session context")
}

/// Validates the deliberately small first SQL-session pagination surface before
/// planning it against the caller's authorized, checkpoint-pinned relations.
///
/// A Form's `_ugoite_id` is unique, so including it in an explicit top-level
/// order makes offset pagination total for one-Form queries. Joins, aggregates,
/// DISTINCT, subqueries, and set operations need a separate proof and are
/// intentionally not part of this initial contract.
pub fn validate_sql_session_page_shape(sql: &str) -> Result<()> {
    sql_session_page_relation(sql).map(|_| ())
}

/// Parses and validates the supported SQL-session subset before any Form or
/// Entry provider is opened. The returned lower-case relation is the only Form
/// that creation may resolve from the checkpoint.
pub fn sql_session_page_relation(sql: &str) -> Result<String> {
    use datafusion::sql::parser::{DFParser, Statement as DataFusionStatement};
    use datafusion::sql::sqlparser::ast::{
        visit_expressions, Expr, GroupByExpr, LimitClause, OrderByKind, SetExpr,
        Statement as SqlStatement, TableFactor,
    };
    use std::ops::ControlFlow;

    let statements = DFParser::parse_sql(sql).context("parse SQL session query with DataFusion")?;
    if statements.len() != 1 {
        return Err(anyhow!(
            "SQL session paging requires exactly one SELECT statement"
        ));
    }
    let statement = statements.front().expect("one statement was checked above");
    let DataFusionStatement::Statement(statement) = statement else {
        return Err(anyhow!("SQL session paging requires a SELECT statement"));
    };
    let SqlStatement::Query(query) = statement.as_ref() else {
        return Err(anyhow!("SQL session paging requires a SELECT statement"));
    };
    let mut has_expression_subquery = false;
    let _ = visit_expressions(statement.as_ref(), |expression| {
        if matches!(
            expression,
            Expr::Exists { .. } | Expr::InSubquery { .. } | Expr::Subquery(_)
        ) {
            has_expression_subquery = true;
        }
        ControlFlow::<()>::Continue(())
    });
    if has_expression_subquery {
        return Err(anyhow!(
            "SQL session paging does not support expression subqueries"
        ));
    }
    if query.with.is_some()
        || query.fetch.is_some()
        || !query.locks.is_empty()
        || query.for_clause.is_some()
        || query.settings.is_some()
        || query.format_clause.is_some()
        || !query.pipe_operators.is_empty()
    {
        return Err(anyhow!(
            "SQL session paging supports only a simple single-Form SELECT"
        ));
    }
    let has_sql_offset = match &query.limit_clause {
        Some(LimitClause::LimitOffset {
            offset, limit_by, ..
        }) => offset.is_some() || !limit_by.is_empty(),
        Some(LimitClause::OffsetCommaLimit { .. }) => true,
        None => false,
    };
    if has_sql_offset {
        return Err(anyhow!(
            "SQL session paging does not support an SQL OFFSET or LIMIT BY clause"
        ));
    }
    let SetExpr::Select(select) = query.body.as_ref() else {
        return Err(anyhow!(
            "SQL session paging does not support set operations or subqueries"
        ));
    };
    if select.distinct.is_some()
        || select.top.is_some()
        || select.into.is_some()
        || !select.lateral_views.is_empty()
        || select.prewhere.is_some()
        || !select.connect_by.is_empty()
        || !matches!(&select.group_by, GroupByExpr::Expressions(expressions, modifiers) if expressions.is_empty() && modifiers.is_empty())
        || !select.cluster_by.is_empty()
        || !select.distribute_by.is_empty()
        || !select.sort_by.is_empty()
        || select.having.is_some()
        || !select.named_window.is_empty()
        || select.qualify.is_some()
    {
        return Err(anyhow!(
            "SQL session paging does not support DISTINCT, aggregation, or window queries"
        ));
    }
    let [from] = select.from.as_slice() else {
        return Err(anyhow!(
            "SQL session paging requires exactly one Form relation"
        ));
    };
    if !from.joins.is_empty() {
        return Err(anyhow!(
            "SQL session paging does not support joins or table functions"
        ));
    }
    let TableFactor::Table {
        name, args: None, ..
    } = &from.relation
    else {
        return Err(anyhow!(
            "SQL session paging does not support joins or table functions"
        ));
    };
    let Some(order_by) = &query.order_by else {
        return Err(anyhow!(
            "SQL session paging requires ORDER BY ending with _ugoite_id"
        ));
    };
    let OrderByKind::Expressions(ordering) = &order_by.kind else {
        return Err(anyhow!(
            "SQL session paging requires ORDER BY ending with _ugoite_id"
        ));
    };
    let Some(last) = ordering.last() else {
        return Err(anyhow!(
            "SQL session paging requires ORDER BY ending with _ugoite_id"
        ));
    };
    let is_entry_id = match &last.expr {
        Expr::Identifier(identifier) => identifier.value.eq_ignore_ascii_case("_ugoite_id"),
        Expr::CompoundIdentifier(identifiers) => identifiers
            .last()
            .is_some_and(|identifier| identifier.value.eq_ignore_ascii_case("_ugoite_id")),
        _ => false,
    };
    if !is_entry_id || last.with_fill.is_some() {
        return Err(anyhow!(
            "SQL session paging requires ORDER BY ending with _ugoite_id"
        ));
    }
    let Some(identifier) = name.0.last() else {
        return Err(anyhow!("SQL session paging requires a Form relation"));
    };
    if name.0.len() != 1 {
        return Err(anyhow!(
            "SQL session paging requires exactly one Form relation"
        ));
    }
    let identifier = identifier
        .as_ident()
        .ok_or_else(|| anyhow!("SQL session paging requires a plain Form relation"))?;
    Ok(identifier.value.to_ascii_lowercase())
}

pub async fn query_index_authorized(
    op: &Operator,
    ws_path: &str,
    query: &str,
    readable_entry_ids: &HashSet<String>,
) -> Result<Vec<Value>> {
    let entry_scope = EntryScope::Only(entry_scope(readable_entry_ids));
    let scopes = all_current_form_scopes(op, ws_path)
        .await?
        .into_keys()
        .map(|relation| (relation, entry_scope.clone()))
        .collect::<BTreeMap<_, _>>();
    query_index_with_form_scopes(op, ws_path, query, &scopes).await
}

pub async fn query_index_authorized_by_form_scopes(
    op: &Operator,
    ws_path: &str,
    query: &str,
    relation_scopes: &BTreeMap<String, EntryScope>,
) -> Result<Vec<Value>> {
    query_index_with_form_scopes(op, ws_path, query, relation_scopes).await
}

async fn query_index_with_form_scopes(
    op: &Operator,
    ws_path: &str,
    query: &str,
    relation_scopes: &BTreeMap<String, EntryScope>,
) -> Result<Vec<Value>> {
    let query_value = if query.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(query).unwrap_or(Value::Null)
    };
    if let Some(sql_query) = extract_sql_query(&query_value) {
        return execute_datafusion_sql(
            op,
            ws_path,
            &sql_query,
            EntryScope::AllCurrent,
            None,
            Some(relation_scopes),
            None,
        )
        .await;
    }
    let forms = load_forms(op, ws_path).await?;
    let filters = query_value.as_object();
    let entries_map = match filters {
        Some(filters) => {
            collect_filtered_entries_with_form_scopes(op, ws_path, &forms, filters, relation_scopes)
                .await?
        }
        None => query_entries_with_form_scopes(op, ws_path, &forms, relation_scopes).await?,
    };
    Ok(entries_map.into_values().collect())
}

pub async fn execute_sql_query_scoped(
    op: &Operator,
    ws_path: &str,
    sql_query: &str,
    readable_forms: &[String],
) -> Result<Vec<Value>> {
    let readable_forms = readable_forms
        .iter()
        .map(|relation| relation.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    execute_datafusion_sql(
        op,
        ws_path,
        sql_query,
        EntryScope::AllCurrent,
        Some(&readable_forms),
        None,
        None,
    )
    .await
}

pub async fn execute_sql_query_scoped_page(
    op: &Operator,
    ws_path: &str,
    sql_query: &str,
    readable_forms: &[String],
    offset: usize,
    limit: usize,
) -> Result<(Vec<Value>, u64)> {
    let readable_forms = readable_forms
        .iter()
        .map(|relation| relation.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    execute_datafusion_sql_page(
        op,
        ws_path,
        sql_query,
        EntryScope::AllCurrent,
        Some(&readable_forms),
        None,
        None,
        offset,
        limit,
        HashMap::new(),
    )
    .await
}

fn entry_scope(entry_ids: &HashSet<String>) -> BTreeSet<ugoite_domain::id::EntryId> {
    let entry_ids = entry_ids.iter().cloned().collect::<BTreeSet<_>>();
    entry_scope_from_ids(&entry_ids)
}

fn entry_scope_from_ids(entry_ids: &BTreeSet<String>) -> BTreeSet<ugoite_domain::id::EntryId> {
    entry_ids
        .iter()
        .map(|entry_id| {
            Uuid::parse_str(entry_id)
                .unwrap_or_else(|_| Uuid::new_v5(&Uuid::NAMESPACE_URL, entry_id.as_bytes()))
                .into()
        })
        .collect()
}

async fn execute_datafusion_sql(
    op: &Operator,
    ws_path: &str,
    sql: &str,
    entry_scope: EntryScope,
    allowed_relations: Option<&HashSet<String>>,
    relation_scopes: Option<&BTreeMap<String, EntryScope>>,
    checkpoint: Option<SpaceCheckpoint>,
) -> Result<Vec<Value>> {
    execute_datafusion_sql_with_functions(
        op,
        ws_path,
        sql,
        entry_scope,
        allowed_relations,
        relation_scopes,
        checkpoint,
        BTreeSet::new(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn execute_datafusion_sql_with_functions(
    op: &Operator,
    ws_path: &str,
    sql: &str,
    entry_scope: EntryScope,
    allowed_relations: Option<&HashSet<String>>,
    relation_scopes: Option<&BTreeMap<String, EntryScope>>,
    checkpoint: Option<SpaceCheckpoint>,
    allowed_functions: BTreeSet<String>,
) -> Result<Vec<Value>> {
    let context = datafusion_sql_context(
        op,
        ws_path,
        entry_scope,
        allowed_relations,
        relation_scopes,
        checkpoint,
        allowed_functions,
    )
    .await
    .map_err(map_sql_error)?;
    let batches = context.execute(sql).await.map_err(map_sql_error)?;
    record_batches_to_values(&batches)
}

#[allow(clippy::too_many_arguments)]
async fn execute_datafusion_sql_page(
    op: &Operator,
    ws_path: &str,
    sql: &str,
    entry_scope: EntryScope,
    allowed_relations: Option<&HashSet<String>>,
    relation_scopes: Option<&BTreeMap<String, EntryScope>>,
    checkpoint: Option<SpaceCheckpoint>,
    offset: usize,
    limit: usize,
    parameters: HashMap<String, datafusion::scalar::ScalarValue>,
) -> Result<(Vec<Value>, u64)> {
    let context = datafusion_sql_context(
        op,
        ws_path,
        entry_scope,
        allowed_relations,
        relation_scopes,
        checkpoint,
        BTreeSet::new(),
    )
    .await
    .map_err(map_sql_error)?;
    let (batches, count) = context
        .execute_page(sql, parameters, offset, limit)
        .await
        .map_err(map_sql_error)?;
    Ok((record_batches_to_values(&batches)?, count))
}

async fn datafusion_sql_context(
    op: &Operator,
    ws_path: &str,
    entry_scope: EntryScope,
    allowed_relations: Option<&HashSet<String>>,
    relation_scopes: Option<&BTreeMap<String, EntryScope>>,
    checkpoint: Option<SpaceCheckpoint>,
    allowed_functions: BTreeSet<String>,
) -> Result<crate::query_context::AuthorizedQueryContext> {
    datafusion_sql_context_with_limits(
        op,
        ws_path,
        entry_scope,
        allowed_relations,
        relation_scopes,
        checkpoint,
        allowed_functions,
        SQL_SESSION_MAX_ROWS,
        false,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn datafusion_sql_context_with_limits(
    op: &Operator,
    ws_path: &str,
    entry_scope: EntryScope,
    allowed_relations: Option<&HashSet<String>>,
    relation_scopes: Option<&BTreeMap<String, EntryScope>>,
    checkpoint: Option<SpaceCheckpoint>,
    allowed_functions: BTreeSet<String>,
    max_rows: usize,
    include_payload: bool,
) -> Result<crate::query_context::AuthorizedQueryContext> {
    let workspace = crate::iceberg_store::native_workspace(op, ws_path).await?;
    let forms = workspace.list_forms().await?;
    let mut policy_forms = BTreeMap::new();
    for form in forms {
        let relation = sql_relation_name(form.id);
        let relation_entry_scope = match relation_scopes {
            Some(scopes) => match scopes
                .get(&relation)
                .or_else(|| scopes.get(&form.name.to_ascii_lowercase()))
            {
                Some(scope) => scope.clone(),
                None => continue,
            },
            None => entry_scope.clone(),
        };
        if allowed_relations.is_some_and(|allowed| !allowed.contains(&relation)) {
            continue;
        }
        let mut system_columns = BTreeSet::from([
            QuerySystemColumn::ExternalId,
            QuerySystemColumn::Title,
            QuerySystemColumn::Tags,
            QuerySystemColumn::CreatedAt,
            QuerySystemColumn::UpdatedAt,
        ]);
        if include_payload {
            system_columns.extend([
                QuerySystemColumn::RevisionId,
                QuerySystemColumn::ParentRevisionId,
                QuerySystemColumn::Author,
                QuerySystemColumn::UpdatedBy,
                QuerySystemColumn::DeletedBy,
                QuerySystemColumn::ExtraAttributes,
                QuerySystemColumn::Integrity,
                QuerySystemColumn::Deleted,
                QuerySystemColumn::DeletedAt,
                QuerySystemColumn::EntryVersion,
            ]);
        }
        policy_forms.insert(
            form.id,
            AuthorizedQueryForm {
                relation,
                entry_scope: relation_entry_scope,
                columns: form
                    .fields
                    .iter()
                    .map(|field| sql_column_name(field.id))
                    .collect(),
                system_columns,
            },
        );
    }
    workspace
        .authorized_query_context(AuthorizedQueryPolicy {
            forms: policy_forms,
            checkpoint,
            limits: QueryLimits {
                max_memory_bytes: SQL_SESSION_MAX_MEMORY_BYTES,
                max_rows,
                timeout: SQL_SESSION_TIMEOUT,
                max_concurrency: 1,
                allowed_functions,
            },
        })
        .await
        .context("create DataFusion SQL context")
}

fn record_batches_to_values(batches: &[arrow_array::RecordBatch]) -> Result<Vec<Value>> {
    let mut writer = ArrayWriter::new(Vec::new());
    writer
        .write_batches(&batches.iter().collect::<Vec<_>>())
        .context("encode DataFusion result rows as JSON")?;
    writer.finish().context("finish DataFusion JSON encoding")?;
    serde_json::from_slice(&writer.into_inner()).context("decode DataFusion result rows")
}

fn map_sql_error(error: anyhow::Error) -> anyhow::Error {
    use crate::query_context::AuthorizedQueryError;

    let message = match error.downcast_ref::<AuthorizedQueryError>() {
        Some(AuthorizedQueryError::InvalidQuery { .. }) => "invalid SQL query",
        Some(AuthorizedQueryError::UnauthorizedQueryFeature { .. }) => {
            "unsupported SQL relation, statement, or function"
        }
        Some(AuthorizedQueryError::ResourceLimitExceeded { .. }) => {
            "SQL query exceeds the configured resource limit"
        }
        Some(AuthorizedQueryError::RevisionInvariantViolation) => {
            "entry revision invariant failed: multiple revisions share a maximum entry_version"
        }
        Some(AuthorizedQueryError::QueryTimedOut) => "SQL query timed out",
        Some(AuthorizedQueryError::QueryExecutionFailed { .. }) => "SQL query execution failed",
        None => return error,
    };
    AppError::invalid_input(ErrorCode::InvalidInput, message).into()
}

fn extract_sql_query(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.trim().is_empty() => Some(text.to_string()),
        Value::Object(map) => map
            .get("$sql")
            .or_else(|| map.get("sql"))
            .and_then(|v| v.as_str())
            .map(|text| text.to_string()),
        _ => None,
    }
}

pub async fn reindex_all(op: &Operator, ws_path: &str) -> Result<()> {
    let _ = op;
    let _ = ws_path;
    Err(AppError::unimplemented(
        ErrorCode::ReindexNotImplemented,
        "reindex is not implemented in this release",
    )
    .into())
}

pub async fn get_space_stats(op: &Operator, ws_path: &str) -> Result<Value> {
    let forms = load_forms(op, ws_path).await?;
    if forms.len() > crate::MAX_NORMAL_READ_ROWS {
        return Err(anyhow!(
            "space statistics are limited to {} Forms",
            crate::MAX_NORMAL_READ_ROWS
        ));
    }
    let relation_scopes = forms
        .keys()
        .map(|form_name| (form_name.to_ascii_lowercase(), EntryScope::AllCurrent))
        .collect::<BTreeMap<_, _>>();
    let context = datafusion_sql_context_with_limits(
        op,
        ws_path,
        EntryScope::AllCurrent,
        None,
        Some(&relation_scopes),
        None,
        BTreeSet::new(),
        crate::MAX_NORMAL_READ_ROWS,
        true,
    )
    .await
    .map_err(map_sql_error)?;
    let mut entry_count = 0u64;
    let mut form_stats = Map::new();
    for (form_name, form) in &forms {
        let relation = form
            .get("sql_relation")
            .and_then(Value::as_str)
            .with_context(|| format!("Form {form_name} is missing its SQL relation"))?;
        let mut aggregate_expr =
            vec![datafusion::functions_aggregate::expr_fn::count(lit(1)).alias("__ugoite_count")];
        let mut field_aliases = Vec::new();
        if let Some(fields) = form.get("fields").and_then(Value::as_object) {
            for (field_name, field) in fields {
                let column = field
                    .get("sql_column")
                    .and_then(Value::as_str)
                    .with_context(|| format!("Form field {field_name} is missing sql_column"))?;
                let alias = format!("__ugoite_field_{field_name}");
                // v1 stats intentionally count non-null typed values. This
                // is the DataFusion `count(column)` semantics, and is a
                // breaking clarification from the removed Rust property-key
                // presence scan.
                aggregate_expr.push(
                    datafusion::functions_aggregate::expr_fn::count(col(column)).alias(&alias),
                );
                field_aliases.push((field_name, alias));
            }
        }
        let values = record_batches_to_values(
            &context
                .execute_relation_aggregate_plan(
                    relation,
                    &[],
                    Vec::new(),
                    Vec::new(),
                    aggregate_expr,
                    1,
                )
                .await
                .map_err(map_sql_error)?,
        )?;
        let value = values
            .first()
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        let count = value
            .get("__ugoite_count")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        entry_count = entry_count.saturating_add(count);
        let mut stats = Map::new();
        stats.insert("count".to_string(), Value::Number(count.into()));
        let mut fields_json = Map::new();
        for (field_name, alias) in field_aliases {
            if let Some(field_count) = value.get(&alias).and_then(Value::as_u64) {
                if field_count > 0 {
                    fields_json.insert(field_name.clone(), Value::Number(field_count.into()));
                }
            }
        }
        if !fields_json.is_empty() {
            stats.insert("fields".to_string(), Value::Object(fields_json));
        }
        form_stats.insert(form_name.clone(), Value::Object(stats));
    }
    let mut tag_relations = forms
        .values()
        .filter_map(|form| form.get("sql_relation").and_then(Value::as_str))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    tag_relations.sort();
    let tag_values = if tag_relations.is_empty() {
        Vec::new()
    } else {
        record_batches_to_values(
            &context
                .execute_union_relation_aggregate_plan(
                    &tag_relations,
                    &[("_ugoite_tags".to_string(), "__ugoite_tag".to_string())],
                    vec![col("__ugoite_tag")],
                    vec![datafusion::functions_aggregate::expr_fn::count(lit(1))
                        .alias("__ugoite_tag_count")],
                    crate::MAX_NORMAL_READ_ROWS.saturating_add(1),
                )
                .await
                .map_err(map_sql_error)?,
        )?
    };
    let mut tag_counts = Map::new();
    for tag_value in tag_values {
        let Some(tag) = tag_value.get("__ugoite_tag").and_then(Value::as_str) else {
            continue;
        };
        let count = tag_value
            .get("__ugoite_tag_count")
            .and_then(Value::as_u64)
            .context("tag aggregate is missing its count")?;
        tag_counts.insert(tag.to_string(), Value::Number(count.into()));
    }
    form_stats.insert(
        "_uncategorized".to_string(),
        serde_json::json!({"count": 0}),
    );
    Ok(serde_json::json!({
        "entry_count": entry_count,
        "form_stats": form_stats,
        "tag_counts": tag_counts,
    }))
}

pub async fn update_entry_index(op: &Operator, ws_path: &str, entry_id: &str) -> Result<()> {
    let _ = op;
    let _ = ws_path;
    let _ = entry_id;
    Err(AppError::unimplemented(
        ErrorCode::ReindexNotImplemented,
        "entry index update is not implemented in this release",
    )
    .into())
}

pub fn extract_properties(markdown: &str) -> Value {
    let mut properties = Map::new();

    let (frontmatter, body) = extract_frontmatter(markdown);
    if let Some(fm) = frontmatter {
        if let Some(obj) = fm.as_object() {
            for (k, v) in obj {
                properties.insert(k.clone(), v.clone());
            }
        }
    }

    let sections = extract_sections(&body);
    for (k, v) in sections {
        if !v.is_empty() {
            properties.insert(k, Value::String(v));
        }
    }

    Value::Object(properties)
}

fn extract_frontmatter(content: &str) -> (Option<Value>, String) {
    let re = Regex::new(r"(?s)^---\s*\n(.*?)\n---\s*\n").unwrap();
    if let Some(caps) = re.captures(content) {
        let yaml_str = caps.get(1).unwrap().as_str();
        let fm_yaml: Option<serde_yaml::Value> = serde_yaml::from_str(yaml_str).ok();
        let fm_json = fm_yaml.and_then(|y| serde_json::to_value(y).ok());
        let end = caps.get(0).unwrap().end();
        return (fm_json, content[end..].to_string());
    }
    (None, content.to_string())
}

fn extract_sections(body: &str) -> Vec<(String, String)> {
    let mut sections = Vec::new();
    let mut current_key: Option<String> = None;
    let mut buffer: Vec<String> = Vec::new();

    let header_re = Regex::new(r"^##\s+(.+)$").unwrap();

    for line in body.lines() {
        if let Some(caps) = header_re.captures(line) {
            if let Some(key) = current_key.take() {
                sections.push((key, buffer.join("\n").trim().to_string()));
            }
            current_key = Some(caps.get(1).unwrap().as_str().trim().to_string());
            buffer.clear();
            continue;
        }

        if line.starts_with('#') {
            if let Some(key) = current_key.take() {
                sections.push((key, buffer.join("\n").trim().to_string()));
            }
            buffer.clear();
            continue;
        }

        if current_key.is_some() {
            buffer.push(line.to_string());
        }
    }

    if let Some(key) = current_key {
        sections.push((key, buffer.join("\n").trim().to_string()));
    }

    sections
}

fn parse_boolean(value: &str) -> Option<bool> {
    match value.trim().to_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

fn parse_wall_timestamp(value: &str) -> Option<NaiveDateTime> {
    ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M"]
        .into_iter()
        .find_map(|format| NaiveDateTime::parse_from_str(value, format).ok())
}

fn parse_zoned_timestamp(value: &str) -> Option<DateTime<chrono::FixedOffset>> {
    DateTime::parse_from_rfc3339(value).ok()
}

fn format_wall_timestamp(timestamp: NaiveDateTime, nanosecond_precision: bool) -> String {
    let base = timestamp.format("%Y-%m-%dT%H:%M:%S").to_string();
    let nanos = if nanosecond_precision {
        timestamp.nanosecond()
    } else {
        (timestamp.nanosecond() / 1_000) * 1_000
    };
    if nanos == 0 {
        return base;
    }
    let fraction = format!("{nanos:09}").trim_end_matches('0').to_string();
    format!("{base}.{fraction}")
}

pub(crate) fn normalize_wall_timestamp(value: &str, nanosecond_precision: bool) -> Option<String> {
    parse_wall_timestamp(value)
        .map(|timestamp| format_wall_timestamp(timestamp, nanosecond_precision))
}

pub(crate) fn normalize_zoned_timestamp(value: &str, nanosecond_precision: bool) -> Option<String> {
    parse_zoned_timestamp(value).map(|timestamp| {
        let timestamp = timestamp.with_timezone(&chrono::Utc);
        if nanosecond_precision {
            timestamp.to_rfc3339_opts(chrono::SecondsFormat::Nanos, false)
        } else {
            timestamp.to_rfc3339()
        }
    })
}

pub(crate) fn normalize_time(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let formats = ["%H:%M:%S%.f", "%H:%M:%S", "%H:%M"];
    for format in formats {
        if let Ok(time) = NaiveTime::parse_from_str(trimmed, format) {
            let micros = time.nanosecond() / 1_000;
            if micros == 0 {
                return Some(time.format("%H:%M:%S").to_string());
            }
            return Some(format!("{}.{:06}", time.format("%H:%M:%S"), micros));
        }
    }
    None
}

pub(crate) fn normalize_binary(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let bytes = if let Some(rest) = trimmed.strip_prefix("base64:") {
        base64::engine::general_purpose::STANDARD
            .decode(rest.trim())
            .ok()?
    } else if let Some(rest) = trimmed.strip_prefix("hex:") {
        hex::decode(rest.trim()).ok()?
    } else if let Some(rest) = trimmed.strip_prefix("0x") {
        hex::decode(rest.trim()).ok()?
    } else {
        base64::engine::general_purpose::STANDARD
            .decode(trimmed)
            .ok()?
    };

    Some(format!(
        "base64:{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

fn parse_markdown_list(value: &str) -> Vec<Value> {
    let mut items = Vec::new();
    for line in value.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let item = if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("- [x] ") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("- [X] ") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("- ") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("* ") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("+ ") {
            rest
        } else {
            trimmed
        };
        if !item.is_empty() {
            items.push(Value::String(item.to_string()));
        }
    }
    items
}

fn parse_object_list(value: &Value) -> Option<Value> {
    let items = match value {
        Value::Array(items) => items.clone(),
        Value::String(raw) => serde_json::from_str::<Value>(raw)
            .ok()
            .and_then(|parsed| parsed.as_array().cloned())?,
        _ => return None,
    };

    let mut normalized = Vec::new();
    for item in items {
        let obj = item.as_object()?;
        let var_type = obj.get("type").and_then(|v| v.as_str())?;
        let name = obj.get("name").and_then(|v| v.as_str())?;
        let description = obj.get("description").and_then(|v| v.as_str())?;
        normalized.push(serde_json::json!({
            "type": var_type,
            "name": name,
            "description": description,
        }));
    }
    Some(Value::Array(normalized))
}

pub fn validate_properties(properties: &Value, entry_form: &Value) -> Result<(Value, Vec<Value>)> {
    let mut warnings = Vec::new();
    let mut casted = properties.clone();

    let fields = entry_form.get("fields");
    let mut field_defs: HashMap<String, Value> = HashMap::new();

    match fields {
        Some(Value::Object(obj)) => {
            for (k, v) in obj {
                field_defs.insert(k.clone(), v.clone());
            }
        }
        Some(Value::Array(arr)) => {
            for item in arr {
                if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                    field_defs.insert(name.to_string(), item.clone());
                }
            }
        }
        _ => {}
    }

    for (field_name, field_def) in field_defs {
        let value = properties.get(&field_name).cloned();
        let field_type = field_def
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("string");
        let required = field_def
            .get("required")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if required && (value.is_none() || value == Some(Value::String(String::new()))) {
            warnings.push(serde_json::json!({
                "code": "missing_field",
                "field": field_name,
                "message": format!("Missing required field: {}", field_name)
            }));
            continue;
        }

        let Some(raw_value) = value else { continue };

        let casted_value = match field_type {
            "number" | "double" => match raw_value {
                Value::Number(_) => Some(raw_value.clone()),
                Value::String(ref s) => s
                    .parse::<f64>()
                    .ok()
                    .and_then(serde_json::Number::from_f64)
                    .map(Value::Number),
                _ => None,
            },
            "float" => match raw_value {
                Value::Number(_) => Some(raw_value.clone()),
                Value::String(ref s) => s
                    .parse::<f32>()
                    .ok()
                    .and_then(|n| serde_json::Number::from_f64(f64::from(n)))
                    .map(Value::Number),
                _ => None,
            },
            "integer" => match raw_value {
                Value::Number(num) => num
                    .as_i64()
                    .and_then(|v| i32::try_from(v).ok())
                    .map(serde_json::Number::from),
                Value::String(ref s) => s.parse::<i32>().ok().map(serde_json::Number::from),
                _ => None,
            }
            .map(Value::Number),
            "long" => match raw_value {
                Value::Number(num) => num.as_i64().map(serde_json::Number::from),
                Value::String(ref s) => s.parse::<i64>().ok().map(serde_json::Number::from),
                _ => None,
            }
            .map(Value::Number),
            "date" => match raw_value {
                Value::String(ref s) => NaiveDate::parse_from_str(s, "%Y-%m-%d")
                    .ok()
                    .map(|d| Value::String(d.format("%Y-%m-%d").to_string())),
                _ => None,
            },
            "time" => match raw_value {
                Value::String(ref s) => normalize_time(s).map(Value::String),
                _ => None,
            },
            "timestamp" => match raw_value {
                Value::String(ref s) => normalize_wall_timestamp(s, false).map(Value::String),
                _ => None,
            },
            "timestamp_tz" => match raw_value {
                Value::String(ref s) => normalize_zoned_timestamp(s, false).map(Value::String),
                _ => None,
            },
            "timestamp_ns" => match raw_value {
                Value::String(ref s) => normalize_wall_timestamp(s, true).map(Value::String),
                _ => None,
            },
            "timestamp_tz_ns" => match raw_value {
                Value::String(ref s) => normalize_zoned_timestamp(s, true).map(Value::String),
                _ => None,
            },
            "uuid" => match raw_value {
                Value::String(ref s) => Uuid::parse_str(s)
                    .ok()
                    .map(|u| Value::String(u.to_string())),
                _ => None,
            },
            "binary" => match raw_value {
                Value::String(ref s) => normalize_binary(s).map(Value::String),
                _ => None,
            },
            "list"
                if field_def
                    .get("items")
                    .and_then(|items| items.get("type"))
                    .and_then(Value::as_str)
                    == Some("asset_reference") =>
            {
                match raw_value {
                    Value::Array(_) => Some(raw_value.clone()),
                    Value::String(ref s) => serde_json::from_str::<Value>(s)
                        .ok()
                        .filter(|value| value.is_array())
                        .or_else(|| Some(Value::Array(parse_markdown_list(s)))),
                    _ => None,
                }
            }
            "list" => match raw_value {
                Value::Array(_) => Some(raw_value.clone()),
                Value::String(ref s) => Some(Value::Array(parse_markdown_list(s))),
                _ => None,
            },
            "object_list" => parse_object_list(&raw_value),
            "asset_reference" => match raw_value {
                Value::Object(_) => Some(raw_value.clone()),
                Value::String(ref s) => serde_json::from_str::<Value>(s)
                    .ok()
                    .filter(|value| value.is_object()),
                _ => None,
            },
            "boolean" => match raw_value {
                Value::Bool(_) => Some(raw_value.clone()),
                Value::String(ref s) => parse_boolean(s).map(Value::Bool),
                _ => None,
            },
            "markdown" | "string" | "row_reference" => Some(raw_value.clone()),
            _ => Some(raw_value.clone()),
        };

        if let Some(value) = casted_value {
            if let Some(obj) = casted.as_object_mut() {
                obj.insert(field_name.clone(), value);
            }
        } else {
            warnings.push(serde_json::json!({
                "code": "invalid_type",
                "field": field_name,
                "message": format!("Field '{}' has invalid type", field_name)
            }));
        }
    }

    Ok((casted, warnings))
}

pub fn aggregate_stats(entries: &Map<String, Value>) -> Value {
    let mut form_stats: HashMap<String, Map<String, Value>> = HashMap::new();
    let mut tag_counts: HashMap<String, usize> = HashMap::new();
    let mut uncategorized = 0usize;

    for record in entries.values() {
        let entry_form = record
            .get("form")
            .or_else(|| record.get("properties").and_then(|v| v.get("form")));

        if let Some(form_name) = entry_form.and_then(|v| v.as_str()) {
            let entry = form_stats.entry(form_name.to_string()).or_default();
            let count = entry.get("count").and_then(|v| v.as_u64()).unwrap_or(0) + 1;
            entry.insert("count".to_string(), Value::Number(count.into()));

            let fields = entry
                .entry("fields".to_string())
                .or_insert_with(|| Value::Object(Map::new()));
            if let Some(field_map) = fields.as_object_mut() {
                if let Some(props) = record.get("properties").and_then(|v| v.as_object()) {
                    for key in props.keys() {
                        let current = field_map.get(key).and_then(|v| v.as_u64()).unwrap_or(0) + 1;
                        field_map.insert(key.to_string(), Value::Number(current.into()));
                    }
                }
            }
        } else {
            uncategorized += 1;
        }

        if let Some(tags) = record.get("tags").and_then(|v| v.as_array()) {
            for tag in tags {
                if let Some(tag_str) = tag.as_str() {
                    *tag_counts.entry(tag_str.to_string()).or_insert(0) += 1;
                }
            }
        }
    }

    let mut form_stats_json: Map<String, Value> = form_stats
        .into_iter()
        .map(|(k, v)| (k, Value::Object(v)))
        .collect();
    form_stats_json.insert(
        "_uncategorized".to_string(),
        Value::Object({
            let mut map = Map::new();
            map.insert("count".to_string(), Value::Number(uncategorized.into()));
            map
        }),
    );

    Value::Object(
        [
            (
                "entry_count".to_string(),
                Value::Number((entries.len() as u64).into()),
            ),
            ("form_stats".to_string(), Value::Object(form_stats_json)),
            (
                "tag_counts".to_string(),
                Value::Object(
                    tag_counts
                        .into_iter()
                        .map(|(k, v)| (k, Value::Number((v as u64).into())))
                        .collect(),
                ),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

async fn load_forms(op: &Operator, ws_path: &str) -> Result<HashMap<String, Value>> {
    let mut forms = HashMap::new();
    for form_name in crate::form::list_form_names(op, ws_path).await? {
        if let Ok(value) = crate::form::get_form(op, ws_path, &form_name).await {
            forms.insert(form_name, value);
        }
    }
    Ok(forms)
}

async fn collect_filtered_entries_with_form_scopes(
    op: &Operator,
    ws_path: &str,
    forms: &HashMap<String, Value>,
    filters: &Map<String, Value>,
    relation_scopes: &BTreeMap<String, EntryScope>,
) -> Result<Map<String, Value>> {
    let mut entries = Map::new();
    let context = datafusion_sql_context_with_limits(
        op,
        ws_path,
        EntryScope::AllCurrent,
        None,
        Some(relation_scopes),
        None,
        BTreeSet::from(["array_has".to_string()]),
        crate::MAX_NORMAL_READ_ROWS,
        true,
    )
    .await
    .map_err(map_sql_error)?;
    let mut rows = Vec::new();
    let mut form_names = forms.keys().cloned().collect::<Vec<_>>();
    form_names.sort();
    for form_name in form_names {
        let form = forms
            .get(&form_name)
            .with_context(|| format!("missing Form definition {form_name}"))?;
        if let Some(expected_form) = filters.get("form") {
            if expected_form.as_str() != Some(form_name.as_str()) {
                continue;
            }
        }
        let relation = form
            .get("sql_relation")
            .and_then(Value::as_str)
            .with_context(|| format!("Form {form_name} is missing its SQL relation"))?;
        if !relation_scopes.contains_key(&form_name.to_ascii_lowercase())
            && !relation_scopes.contains_key(&relation.to_ascii_lowercase())
        {
            continue;
        }
        let FilterPlan {
            predicates,
            struct_list_predicates,
        } = filter_sql(form, filters)?;
        let unnest_columns = struct_list_predicates
            .iter()
            .enumerate()
            .map(|(index, (field, _))| (field.clone(), format!("__ugoite_unnested_item_{index}")))
            .collect::<Vec<_>>();
        let mut predicates = predicates;
        for ((_, asset_id), (_, output_column)) in
            struct_list_predicates.iter().zip(&unnest_columns)
        {
            predicates.push(
                datafusion::functions::core::expr_fn::get_field(col(output_column), "asset_id")
                    .eq(lit(asset_id)),
            );
        }
        let remaining = crate::MAX_NORMAL_READ_ROWS.saturating_sub(rows.len());
        // This endpoint has no paging contract. Request one sentinel row so
        // an over-cap result is rejected instead of depending on Form order
        // and silently returning a partial collection.
        let per_form_bound = remaining.saturating_add(1);
        let batches = execute_payload_relation_plan(
            &context,
            relation,
            unnest_columns,
            predicates,
            form,
            true,
            per_form_bound,
        )
        .await?;
        let rows_for_form = entry_rows_from_batches(&form_name, form, &batches)?;
        if remaining == 0 && !rows_for_form.is_empty() {
            return Err(anyhow!(
                "normal Entry reads are limited to {} current rows",
                crate::MAX_NORMAL_READ_ROWS
            ));
        }
        if rows_for_form.len() > remaining {
            return Err(anyhow!(
                "normal Entry reads are limited to {} current rows",
                crate::MAX_NORMAL_READ_ROWS
            ));
        }
        for row in rows_for_form {
            if let Some(record) = build_record(ws_path, &form_name, &row, forms).await? {
                entries.insert(row.entry_id.clone(), record);
            }
            rows.push((form_name.clone(), row));
        }
    }
    Ok(entries)
}

struct FilterPlan {
    predicates: Vec<Expr>,
    struct_list_predicates: Vec<(String, String)>,
}

fn filter_sql(form: &Value, filters: &Map<String, Value>) -> Result<FilterPlan> {
    let mut predicates = Vec::new();
    let mut struct_list_predicates = Vec::new();
    for (key, expected) in filters {
        if key == "form" {
            continue;
        }
        let (column, field_type, list_item_type) = match key.as_str() {
            "id" => ("_ugoite_id".to_string(), "string", None),
            "title" => ("_ugoite_title".to_string(), "string", None),
            "tag" => ("_ugoite_tags".to_string(), "list", Some("string")),
            field_name => {
                let Some(field) = form
                    .get("fields")
                    .and_then(Value::as_object)
                    .and_then(|fields| fields.get(field_name))
                else {
                    predicates.push(lit(false));
                    continue;
                };
                (
                    field
                        .get("sql_column")
                        .and_then(Value::as_str)
                        .with_context(|| format!("Form field {field_name} is missing sql_column"))?
                        .to_string(),
                    field
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or("string"),
                    field
                        .get("items")
                        .and_then(Value::as_object)
                        .and_then(|items| items.get("type"))
                        .and_then(Value::as_str),
                )
            }
        };
        let column_expr = col(&column);
        let predicate = if field_type == "asset_reference" {
            let asset_id = expected
                .get("asset_id")
                .and_then(Value::as_str)
                .context("asset_reference predicates require asset_id")?;
            datafusion::functions::core::expr_fn::get_field(column_expr, "asset_id")
                .eq(lit(asset_id))
        } else if field_type == "list" {
            let value = expected.get("$contains").unwrap_or(expected);
            if list_item_type == Some("asset_reference") {
                let asset_id = value
                    .get("asset_id")
                    .and_then(Value::as_str)
                    .context("asset_reference list predicates require asset_id")?;
                struct_list_predicates.push((column, asset_id.to_string()));
                continue;
            } else {
                array_has(
                    column_expr,
                    filter_literal(value, list_item_type.unwrap_or("string"))?,
                )
            }
        } else {
            scalar_predicate(column_expr, expected, field_type)?
        };
        predicates.push(predicate);
    }
    Ok(FilterPlan {
        predicates,
        struct_list_predicates,
    })
}

fn scalar_predicate(column: Expr, expected: &Value, field_type: &str) -> Result<Expr> {
    if let Some(value) = expected.get("$eq") {
        return scalar_predicate(column, value, field_type);
    }
    if expected.is_object() {
        return Err(anyhow!(
            "structured predicate is only supported for asset_reference or $eq"
        ));
    }
    if expected.is_null() {
        return Ok(column.is_null());
    }
    Ok(column.eq(filter_literal(expected, field_type)?))
}

fn filter_literal(value: &Value, field_type: &str) -> Result<Expr> {
    let scalar = match (field_type, value) {
        ("boolean", Value::Bool(value)) => ScalarValue::Boolean(Some(*value)),
        ("integer", Value::Number(value)) => ScalarValue::Int32(Some(
            i32::try_from(
                value
                    .as_i64()
                    .context("integer filter value must be an integer")?,
            )
            .context("integer filter value is outside the Int32 range")?,
        )),
        ("long", Value::Number(value)) => ScalarValue::Int64(Some(
            value
                .as_i64()
                .context("long filter value must be an integer")?,
        )),
        ("float", Value::Number(value)) => ScalarValue::Float32(Some(
            value
                .as_f64()
                .context("float filter value must be a number")? as f32,
        )),
        ("double", Value::Number(value)) => ScalarValue::Float64(Some(
            value
                .as_f64()
                .context("double filter value must be a number")?,
        )),
        ("date", Value::String(value)) => ScalarValue::Date32(Some(
            crate::parse_date(value)?.context("date filter value is null")?,
        )),
        ("time", Value::String(value)) => ScalarValue::Time64Microsecond(Some(
            crate::parse_time_micros(value)?.context("time filter value is null")?,
        )),
        ("timestamp", Value::String(value)) => ScalarValue::TimestampMicrosecond(
            Some(crate::parse_wall_timestamp_micros(value)?.context("timestamp is null")?),
            None,
        ),
        ("timestamp_tz", Value::String(value)) => ScalarValue::TimestampMicrosecond(
            Some(crate::parse_zoned_timestamp_micros(value)?.context("timestamp is null")?),
            Some(Arc::from("+00:00")),
        ),
        ("timestamp_ns", Value::String(value)) => ScalarValue::TimestampNanosecond(
            Some(crate::parse_wall_timestamp_nanos(value)?.context("timestamp is null")?),
            None,
        ),
        ("timestamp_tz_ns", Value::String(value)) => ScalarValue::TimestampNanosecond(
            Some(crate::parse_zoned_timestamp_nanos(value)?.context("timestamp is null")?),
            Some(Arc::from("+00:00")),
        ),
        ("uuid", Value::String(value)) => {
            ScalarValue::FixedSizeBinary(16, Some(Uuid::parse_str(value)?.as_bytes().to_vec()))
        }
        ("binary", Value::String(value)) => {
            let encoded = value.strip_prefix("base64:").unwrap_or(value);
            ScalarValue::LargeBinary(Some(BASE64.decode(encoded)?))
        }
        ("string" | "markdown" | "sql" | "row_reference", Value::String(value)) => {
            ScalarValue::Utf8(Some(value.clone()))
        }
        _ => {
            return Err(anyhow!(
                "filter value does not match the typed Form field {field_type}"
            ))
        }
    };
    Ok(lit(scalar))
}

fn quote_identifier(value: &str) -> String {
    sql_identifier(value)
}

async fn query_entries_with_form_scopes(
    op: &Operator,
    ws_path: &str,
    forms: &HashMap<String, Value>,
    relation_scopes: &BTreeMap<String, EntryScope>,
) -> Result<Map<String, Value>> {
    let mut entries = Map::new();
    let rows = query_entry_rows_authorized(
        op,
        ws_path,
        relation_scopes,
        None,
        None,
        crate::MAX_NORMAL_READ_ROWS.saturating_add(1),
        0,
    )
    .await?;
    for (form_name, row) in rows {
        if let Some(record) = build_record(ws_path, &form_name, &row, forms).await? {
            entries.insert(row.entry_id.clone(), record);
        }
    }
    Ok(entries)
}

async fn all_current_form_scopes(
    op: &Operator,
    ws_path: &str,
) -> Result<BTreeMap<String, EntryScope>> {
    Ok(load_forms(op, ws_path)
        .await?
        .into_keys()
        .map(|name| (name.to_ascii_lowercase(), EntryScope::AllCurrent))
        .collect())
}

async fn build_record(
    ws_path: &str,
    form_name: &str,
    row: &entry::EntryRow,
    forms: &HashMap<String, Value>,
) -> Result<Option<Value>> {
    if row.deleted {
        return Ok(None);
    }

    let mut warnings = Vec::new();
    let mut properties = entry::merge_entry_fields(&row.fields, &row.extra_attributes);
    if let Some(form_def) = forms.get(form_name) {
        if let Ok((casted, warns)) = validate_properties(&properties, form_def) {
            properties = casted;
            warnings = warns;
        }
    }

    let word_count = compute_word_count(&serde_json::to_string(&properties)?);
    let record = serde_json::json!({
        "id": row.entry_id,
        "title": row.title,
        "form": form_name,
        "updated_at": row.updated_at,
        "author": row.author,
        "updated_by": row.updated_by,
        "deleted_by": row.deleted_by,
        "space_id": ws_path.split('/').next_back().unwrap_or("").to_string(),
        "properties": properties,
        "word_count": word_count,
        "tags": row.tags,
        "checksum": row.integrity.checksum,
        "validation_warnings": Value::Array(warnings),
    });

    Ok(Some(record))
}

#[cfg(test)]
mod tests {
    use super::{datafusion_parameters, filter_literal, sql_session_page_relation};
    use chrono::DateTime;
    use datafusion::logical_expr::Expr;
    use datafusion::scalar::ScalarValue;
    use serde_json::{Map, Value};
    use std::collections::BTreeMap;

    #[test]
    fn sql_session_relation_parser_uses_identifier_value_without_quotes() {
        assert_eq!(
            sql_session_page_relation(
                r#"SELECT * FROM "form_00000000000000000000000000000001" ORDER BY _ugoite_updated_at DESC, _ugoite_id"#,
            )
            .expect("quoted relation is valid"),
            "form_00000000000000000000000000000001"
        );
    }

    #[test]
    fn native_parameters_keep_date_and_microsecond_timestamp_types() {
        let values = Map::from_iter([
            (
                "when".to_string(),
                Value::String("2025-03-03T23:59:59.999999Z".to_string()),
            ),
            ("day".to_string(), Value::String("2025-03-03".to_string())),
        ]);
        let types = BTreeMap::from_iter([
            ("when".to_string(), "timestamp".to_string()),
            ("day".to_string(), "date".to_string()),
        ]);
        let parameters = datafusion_parameters(&values, &types).expect("typed parameters");
        assert!(matches!(
            parameters.get("when"),
            Some(datafusion::scalar::ScalarValue::TimestampMicrosecond(Some(value), None))
                if *value == DateTime::parse_from_rfc3339("2025-03-03T23:59:59.999999Z")
                    .expect("timestamp")
                    .timestamp_micros()
        ));
        assert!(matches!(
            parameters.get("day"),
            Some(datafusion::scalar::ScalarValue::Date32(Some(value))) if *value == 20150
        ));
    }

    #[test]
    fn form_filter_literals_preserve_physical_arrow_types() {
        let literal = |value: &Value, field_type: &str| {
            let expression = filter_literal(value, field_type).expect("typed filter literal");
            let Expr::Literal(value, _) = expression else {
                panic!("filter literal must remain a DataFusion literal")
            };
            value
        };

        assert!(matches!(
            literal(&Value::Bool(true), "boolean"),
            ScalarValue::Boolean(Some(true))
        ));
        assert!(matches!(
            literal(&serde_json::json!(7), "integer"),
            ScalarValue::Int32(Some(7))
        ));
        assert!(matches!(
            literal(&serde_json::json!(7), "long"),
            ScalarValue::Int64(Some(7))
        ));
        assert!(matches!(
            literal(&serde_json::json!(1.25), "float"),
            ScalarValue::Float32(Some(value)) if (value - 1.25).abs() < f32::EPSILON
        ));
        assert!(matches!(
            literal(&serde_json::json!(1.25), "double"),
            ScalarValue::Float64(Some(value)) if (value - 1.25).abs() < f64::EPSILON
        ));
        assert!(matches!(
            literal(&Value::String("a7f9f5d2-8b7e-4db1-9b0a-0e9a2b3f4c5d".into()), "uuid"),
            ScalarValue::FixedSizeBinary(16, Some(value)) if value.len() == 16
        ));
        assert!(matches!(
            literal(&Value::String("base64:ZGF0YQ==".into()), "binary"),
            ScalarValue::LargeBinary(Some(value)) if value == b"data"
        ));
        assert!(matches!(
            literal(&Value::String("2025-01-02".into()), "date"),
            ScalarValue::Date32(Some(_))
        ));
        assert!(matches!(
            literal(&Value::String("12:34:56.123456".into()), "time"),
            ScalarValue::Time64Microsecond(Some(_))
        ));
        assert!(matches!(
            literal(
                &Value::String("2025-01-02T03:04:05.123456".into()),
                "timestamp"
            ),
            ScalarValue::TimestampMicrosecond(Some(_), None)
        ));
        assert!(matches!(
            literal(
                &Value::String("2025-01-02T03:04:05.123456+00:00".into()),
                "timestamp_tz"
            ),
            ScalarValue::TimestampMicrosecond(Some(_), Some(_))
        ));
        assert!(matches!(
            literal(
                &Value::String("2025-01-02T03:04:05.123456789".into()),
                "timestamp_ns"
            ),
            ScalarValue::TimestampNanosecond(Some(_), None)
        ));
        assert!(matches!(
            literal(
                &Value::String("2025-01-02T03:04:05.123456789+00:00".into()),
                "timestamp_tz_ns"
            ),
            ScalarValue::TimestampNanosecond(Some(_), Some(_))
        ));
    }
}
