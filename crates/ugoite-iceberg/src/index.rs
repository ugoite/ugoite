use anyhow::{anyhow, Context, Result};
use arrow_json::writer::ArrayWriter;
use base64::Engine as _;
use chrono::{DateTime, NaiveDate, NaiveTime, SecondsFormat, Timelike, Utc};
use opendal::Operator;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use serde_yaml;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::time::Duration;
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
    pub entry_ids: BTreeSet<String>,
    pub columns: BTreeSet<String>,
    pub system_columns: BTreeSet<SqlSessionSystemColumn>,
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
        let mut forms = BTreeMap::new();
        for form in &self.forms {
            if forms.contains_key(&form.form_id) {
                return Err(anyhow!("SQL session query policy repeats a Form ID"));
            }
            forms.insert(
                form.form_id,
                AuthorizedQueryForm {
                    relation: form.relation.clone(),
                    entry_scope: EntryScope::Only(entry_scope_from_ids(&form.entry_ids)),
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

    pub fn readable_entry_ids(&self) -> BTreeSet<String> {
        self.forms
            .iter()
            .flat_map(|form| form.entry_ids.iter().cloned())
            .collect()
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
                ("string", Value::Null) => datafusion::scalar::ScalarValue::Utf8(None),
                ("boolean", Value::Null) => datafusion::scalar::ScalarValue::Boolean(None),
                ("integer", Value::Null) => datafusion::scalar::ScalarValue::Int64(None),
                ("float", Value::Null) => datafusion::scalar::ScalarValue::Float64(None),
                ("timestamp", Value::Null) => {
                    datafusion::scalar::ScalarValue::TimestampMicrosecond(None, None)
                }
                ("string" | "boolean" | "integer" | "float" | "timestamp", _) => {
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
            None,
            None,
        )
        .await;
    }

    let forms = load_forms(op, ws_path).await?;
    let entries_map = collect_entries(op, ws_path, &forms).await?;

    let filters: Option<Map<String, Value>> = query_value.as_object().cloned();

    let mut results = Vec::new();
    for entry in entries_map.values() {
        if let Some(filter_obj) = filters.as_ref() {
            if !matches_filters(entry, filter_obj)? {
                continue;
            }
        }
        results.push(entry.clone());
    }

    Ok(results)
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
/// in a checkpoint. A session policy is derived metadata, never Catalog
/// authority; it is replayed only together with this checkpoint.
pub async fn sql_session_query_policy_at_checkpoint(
    op: &Operator,
    ws_path: &str,
    readable_entries_by_form: &BTreeMap<String, HashSet<String>>,
    checkpoint: &SpaceCheckpoint,
) -> Result<SqlSessionQueryPolicy> {
    let workspace = crate::iceberg_store::native_workspace(op, ws_path).await?;
    let forms = workspace.forms_at_checkpoint(checkpoint).await?;
    let mut seen_relations = BTreeSet::new();
    let mut policy_forms = Vec::new();
    for form in forms {
        let relation = form.name.to_ascii_lowercase();
        let Some(entry_ids) = readable_entries_by_form.get(&relation) else {
            continue;
        };
        if !seen_relations.insert(relation.clone()) {
            return Err(anyhow!(
                "checkpoint exposes duplicate SQL relation {relation}"
            ));
        }
        policy_forms.push(SqlSessionQueryForm {
            form_id: form.id,
            relation,
            entry_ids: entry_ids.iter().cloned().collect(),
            columns: form.fields.into_iter().map(|field| field.name).collect(),
            system_columns: [
                SqlSessionSystemColumn::ExternalId,
                SqlSessionSystemColumn::Title,
                SqlSessionSystemColumn::CreatedAt,
                SqlSessionSystemColumn::UpdatedAt,
            ]
            .into_iter()
            .collect(),
        });
    }
    if policy_forms.is_empty() {
        return Err(anyhow!("SQL session has no readable checkpoint Form"));
    }
    policy_forms.sort_by(|left, right| left.relation.cmp(&right.relation));
    Ok(SqlSessionQueryPolicy {
        forms: policy_forms,
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
    if !from.joins.is_empty() || !matches!(&from.relation, TableFactor::Table { args: None, .. }) {
        return Err(anyhow!(
            "SQL session paging does not support joins or table functions"
        ));
    }
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
    Ok(())
}

pub async fn query_index_authorized(
    op: &Operator,
    ws_path: &str,
    query: &str,
    readable_entry_ids: &HashSet<String>,
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
            EntryScope::Only(entry_scope(readable_entry_ids)),
            None,
            None,
            None,
        )
        .await;
    }
    let forms = load_forms(op, ws_path).await?;
    let entries_map: Map<String, Value> = collect_entries(op, ws_path, &forms)
        .await?
        .into_iter()
        .filter(|(entry_id, _)| readable_entry_ids.contains(entry_id))
        .collect();
    let filters = query_value.as_object();
    entries_map
        .into_values()
        .filter_map(|entry| match filters {
            Some(filter) => match matches_filters(&entry, filter) {
                Ok(true) => Some(Ok(entry)),
                Ok(false) => None,
                Err(error) => Some(Err(error)),
            },
            None => Some(Ok(entry)),
        })
        .collect()
}

pub async fn execute_sql_query_scoped(
    op: &Operator,
    ws_path: &str,
    sql_query: &str,
    readable_forms: &[String],
) -> Result<Vec<Value>> {
    let readable_forms = readable_forms
        .iter()
        .map(|form| form.to_ascii_lowercase())
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
        .map(|form| form.to_ascii_lowercase())
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
    let context = datafusion_sql_context(
        op,
        ws_path,
        entry_scope,
        allowed_relations,
        relation_scopes,
        checkpoint,
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
) -> Result<crate::query_context::AuthorizedQueryContext> {
    let workspace = crate::iceberg_store::native_workspace(op, ws_path).await?;
    let forms = workspace.list_forms().await?;
    let mut policy_forms = BTreeMap::new();
    for form in forms {
        let relation = form.name.to_ascii_lowercase();
        let relation_entry_scope = match relation_scopes {
            Some(scopes) => match scopes.get(&relation) {
                Some(scope) => scope.clone(),
                None => continue,
            },
            None => entry_scope.clone(),
        };
        if allowed_relations.is_some_and(|allowed| !allowed.contains(&relation)) {
            continue;
        }
        policy_forms.insert(
            form.id,
            AuthorizedQueryForm {
                relation,
                entry_scope: relation_entry_scope,
                columns: form.fields.iter().map(|field| field.name.clone()).collect(),
                system_columns: [
                    QuerySystemColumn::ExternalId,
                    QuerySystemColumn::Title,
                    QuerySystemColumn::CreatedAt,
                    QuerySystemColumn::UpdatedAt,
                ]
                .into_iter()
                .collect(),
            },
        );
    }
    workspace
        .authorized_query_context(AuthorizedQueryPolicy {
            forms: policy_forms,
            checkpoint,
            limits: QueryLimits {
                max_memory_bytes: SQL_SESSION_MAX_MEMORY_BYTES,
                max_rows: SQL_SESSION_MAX_ROWS,
                timeout: SQL_SESSION_TIMEOUT,
                max_concurrency: 1,
                allowed_functions: BTreeSet::new(),
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

fn matches_filters(entry: &Value, filters: &Map<String, Value>) -> Result<bool> {
    for (key, expected) in filters {
        let mut entry_value = entry.get(key).cloned();
        if entry_value.is_none() {
            entry_value = entry
                .get("properties")
                .and_then(|v| v.as_object())
                .and_then(|props| props.get(key))
                .cloned();
        }

        if expected.is_object() {
            return Err(anyhow!(
                "Structured operators (e.g., $gt) are not implemented for the local query helper yet."
            ));
        }

        if key == "tag" {
            if let Some(tags) = entry.get("tags").and_then(|v| v.as_array()) {
                if !tags.iter().any(|v| v == expected) {
                    return Ok(false);
                }
                continue;
            }
        }

        match entry_value {
            Some(Value::Array(list)) => {
                if !list.iter().any(|v| v == expected) {
                    return Ok(false);
                }
            }
            Some(value) => {
                if value != *expected {
                    return Ok(false);
                }
            }
            None => return Ok(false),
        }
    }
    Ok(true)
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
    let entries = collect_entries(op, ws_path, &forms).await?;
    Ok(aggregate_stats(&entries))
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

fn normalize_timestamp(value: &str) -> Option<String> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&Utc).to_rfc3339())
}

fn normalize_timestamp_ns(value: &str) -> Option<String> {
    DateTime::parse_from_rfc3339(value).ok().map(|dt| {
        dt.with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Nanos, false)
    })
}

fn normalize_time(value: &str) -> Option<String> {
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

fn normalize_binary(value: &str) -> Option<String> {
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
                    .map(|n| Value::Number(serde_json::Number::from_f64(n).unwrap())),
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
                Value::String(ref s) => normalize_timestamp(s).map(Value::String),
                _ => None,
            },
            "timestamp_tz" => match raw_value {
                Value::String(ref s) => normalize_timestamp(s).map(Value::String),
                _ => None,
            },
            "timestamp_ns" => match raw_value {
                Value::String(ref s) => normalize_timestamp_ns(s).map(Value::String),
                _ => None,
            },
            "timestamp_tz_ns" => match raw_value {
                Value::String(ref s) => normalize_timestamp_ns(s).map(Value::String),
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
            "list" => match raw_value {
                Value::Array(_) => Some(raw_value.clone()),
                Value::String(ref s) => Some(Value::Array(parse_markdown_list(s))),
                _ => None,
            },
            "object_list" => parse_object_list(&raw_value),
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

async fn collect_entries(
    op: &Operator,
    ws_path: &str,
    forms: &HashMap<String, Value>,
) -> Result<Map<String, Value>> {
    let mut entries = Map::new();
    let rows = entry::list_entry_rows(op, ws_path).await?;
    for (form_name, row) in rows {
        if let Some(record) = build_record(ws_path, &form_name, &row, forms).await? {
            entries.insert(row.entry_id.clone(), record);
        }
    }
    Ok(entries)
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
        "space_id": ws_path.split('/').next_back().unwrap_or("").to_string(),
        "properties": properties,
        "word_count": word_count,
        "tags": row.tags,
        "links": row.links,
        "assets": row.assets,
        "checksum": row.integrity.checksum,
        "validation_warnings": Value::Array(warnings),
    });

    Ok(Some(record))
}
