//! Parser-backed capabilities for safe SQL Workspace structure operations.
//!
//! This module describes a deliberately small SQL subset that can be
//! transformed without a second SQL parser in the browser. It does not
//! authorize, plan, or execute queries. The server remains authoritative for
//! read-only SQL validation, Form access, parameter types, and pagination.

use serde::{Deserialize, Serialize};
use sqlparser::ast::{
    BinaryOperator, Distinct, DuplicateTreatment, Expr, Function, FunctionArg, FunctionArgExpr,
    FunctionArgumentList, FunctionArguments, GroupByExpr, Ident, LimitClause, ObjectName,
    ObjectNamePart, OrderBy, OrderByExpr, OrderByKind, OrderByOptions, Query, Select, SelectFlavor,
    SelectItem, SetExpr, Spanned, Statement, TableFactor, Value,
};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;
use sqlparser::tokenizer::Span;
use std::collections::BTreeSet;

use crate::sql_document::{has_comment_token_in_range, source_range_from_span, SqlSourceRange};

/// The parser/capability contract version exposed through the WASM adapter.
pub const SQL_STRUCTURE_CONTRACT_VERSION: u32 = 1;

/// Keep SQL parsing within the same bound as the stateless SQL request.
pub const MAX_SQL_STRUCTURE_QUERY_BYTES: usize = 256 * 1024;

/// Adapter-provided SQL schema information for a selected Form, or for a
/// relation-free scalar query when `relation`, `columns`, and `stable_key` are
/// all empty.
///
/// `stable_key` is supplied by the owning adapter from trusted Form metadata.
/// It is used only to make a proposed sort deterministic; the SQL service
/// continues to validate the query and current authorization on execution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlStructureContext {
    pub relation: String,
    pub columns: Vec<SqlStructureColumn>,
    #[serde(default)]
    pub variables: Vec<SqlStructureVariable>,
    #[serde(default)]
    pub stable_key: Vec<String>,
}

/// A column visible in the selected Form's SQL relation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlStructureColumn {
    pub name: String,
    #[serde(rename = "type")]
    pub column_type: SqlStructureColumnType,
}

/// Portable SQL scalar types that can be used by Workspace filters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlStructureColumnType {
    String,
    Boolean,
    Integer,
    Float,
    Date,
    Timestamp,
    Decimal,
    Json,
    Other,
}

/// A declared typed SQL parameter owned by the current SQL adapter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlStructureVariable {
    pub name: String,
    #[serde(rename = "type")]
    pub variable_type: SqlStructureVariableType,
}

/// Types accepted by the current SQL parameter contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlStructureVariableType {
    String,
    Boolean,
    Integer,
    Float,
    Date,
    Timestamp,
}

impl SqlStructureVariableType {
    fn column_type(self) -> SqlStructureColumnType {
        match self {
            Self::String => SqlStructureColumnType::String,
            Self::Boolean => SqlStructureColumnType::Boolean,
            Self::Integer => SqlStructureColumnType::Integer,
            Self::Float => SqlStructureColumnType::Float,
            Self::Date => SqlStructureColumnType::Date,
            Self::Timestamp => SqlStructureColumnType::Timestamp,
        }
    }
}

/// The supported operations never accept SQL fragments from the caller.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SqlStructureOperation {
    SetRelation {
        relation: String,
    },
    SetColumns {
        columns: Vec<String>,
    },
    SetFilters {
        filters: Vec<SqlStructureFilter>,
    },
    SetSort {
        order_by: Vec<SqlStructureSort>,
    },
    SetAggregates {
        aggregates: Vec<SqlStructureAggregate>,
    },
    SetGroupBy {
        columns: Vec<String>,
    },
    SetDistinct {
        distinct: bool,
    },
}

/// An aggregate projection that the bounded SQL structure parser can round-trip.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlStructureAggregate {
    pub function: SqlStructureAggregateFunction,
    /// `None` represents the supported `COUNT(*)` form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
    pub distinct: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<SqlStructureAlias>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlStructureAggregateFunction {
    Count,
    Sum,
    Avg,
    Min,
    Max,
}

/// Output aliases remain SQL names. The quote flag preserves PostgreSQL's
/// distinction between quoted and folded unquoted identifiers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlStructureAlias {
    pub name: String,
    #[serde(default)]
    pub quoted: bool,
}

/// A typed predicate that references a declared SQL variable by name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlStructureFilter {
    pub column: String,
    pub operator: SqlStructureFilterOperator,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlStructureFilterOperator {
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    IsNull,
    IsNotNull,
}

/// One column and direction in an explicit ORDER BY clause.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlStructureSort {
    pub column: String,
    pub direction: SqlStructureSortDirection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlStructureSortDirection {
    Asc,
    Desc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlStructureStatus {
    Supported,
    InvalidSql,
    Unsupported,
}

/// Stable diagnostic codes for adapters to map to localized accessible text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlStructureReason {
    InvalidContext,
    SqlTooLarge,
    ParseFailed,
    SingleStatementRequired,
    SelectRequired,
    CommonTableExpressionUnsupported,
    SetOperationUnsupported,
    QueryClauseUnsupported,
    RelationUnsupported,
    RelationMismatch,
    JoinUnsupported,
    ProjectionUnsupported,
    FilterUnsupported,
    SortUnsupported,
    UnknownColumn,
    DuplicateColumn,
    EmptyProjection,
    InvalidVariableName,
    UnknownVariable,
    VariableTypeMismatch,
    FilterTypeUnsupported,
    StableKeyUnavailable,
    StableKeyUnorderable,
    StableOrderRequired,
    AggregateUnsupported,
    AggregateTypeUnsupported,
    GroupByUnsupported,
    DistinctUnsupported,
    RoundTripFailed,
}

/// Whether one action can safely operate on the currently parsed query.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlStructureActionCapability {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<SqlStructureReason>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlStructureCapabilities {
    pub columns: SqlStructureActionCapability,
    pub filter: SqlStructureActionCapability,
    pub sort: SqlStructureActionCapability,
    pub aggregate: SqlStructureActionCapability,
    pub group_by: SqlStructureActionCapability,
    pub distinct: SqlStructureActionCapability,
    pub stable_pagination: SqlStructureActionCapability,
}

/// Parsed structure and per-action capability state for the current SQL.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlStructureAnalysis {
    pub contract_version: u32,
    pub status: SqlStructureStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<SqlStructureReason>,
    #[serde(default)]
    pub projection: Vec<String>,
    #[serde(default)]
    pub filters: Vec<SqlStructureFilter>,
    #[serde(default)]
    pub aggregates: Vec<SqlStructureAggregate>,
    #[serde(default)]
    pub group_by: Vec<String>,
    pub distinct: bool,
    #[serde(default)]
    pub order_by: Vec<SqlStructureSort>,
    #[serde(default)]
    pub referenced_variables: Vec<String>,
    pub stable_pagination: bool,
    pub capabilities: SqlStructureCapabilities,
}

/// A non-mutating candidate. The adapter must show the SQL diff and wait for
/// explicit user confirmation before calling its SQL-change callback.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlStructureProposal {
    pub contract_version: u32,
    pub status: SqlStructureStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<SqlStructureReason>,
    pub base_sql: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposed_sql: Option<String>,
    pub requires_confirmation: bool,
    #[serde(default)]
    pub added_stable_tie_breakers: Vec<String>,
}

struct ParsedSqlStructure {
    query: Query,
    projection: Vec<String>,
    filters: Vec<SqlStructureFilter>,
    aggregates: Vec<SqlStructureAggregate>,
    group_by: Vec<String>,
    distinct: bool,
    order_by: Vec<SqlStructureSort>,
    referenced_variables: Vec<String>,
    stable_pagination: bool,
}

/// Analyze SQL using the PostgreSQL dialect shared by the Rust SQL path.
///
/// Unsupported syntax produces disabled capabilities with stable reason codes;
/// the caller can leave the direct editor active without guessing at a rewrite.
pub fn analyze_sql_structure(sql: &str, context: &SqlStructureContext) -> SqlStructureAnalysis {
    match parse_supported_sql(sql, context) {
        Ok(parsed) => supported_analysis(parsed, context),
        Err(reason) => rejected_analysis(reason),
    }
}

/// Produce a proposed SQL rewrite without changing the input or persisting it.
pub fn propose_sql_structure_operation(
    sql: &str,
    context: &SqlStructureContext,
    operation: SqlStructureOperation,
) -> SqlStructureProposal {
    if let SqlStructureOperation::SetRelation { relation } = &operation {
        return propose_sql_relation_selection(sql, context, relation);
    }

    let base_sql = sql.to_owned();
    let mut parsed = match parse_supported_sql(sql, context) {
        Ok(parsed) => parsed,
        Err(reason) => return rejected_proposal(base_sql, reason),
    };

    let mut added_stable_tie_breakers = Vec::new();
    let proposed_sql = match operation {
        SqlStructureOperation::SetRelation { .. } => {
            unreachable!("SetRelation is handled by the Rust relation selection proposal")
        }
        SqlStructureOperation::SetColumns { columns } => {
            match rewrite_projection(&base_sql, &parsed.query, context, columns) {
                Ok(sql) => sql,
                Err(reason) => return rejected_proposal(base_sql, reason),
            }
        }
        operation => {
            let mutation = match operation {
                SqlStructureOperation::SetRelation { .. } => {
                    unreachable!("SetRelation is handled by the Rust relation selection proposal")
                }
                SqlStructureOperation::SetFilters { filters } => {
                    set_filters(&mut parsed.query, context, filters)
                }
                SqlStructureOperation::SetSort { order_by } => set_sort(
                    &mut parsed.query,
                    context,
                    order_by,
                    &mut added_stable_tie_breakers,
                ),
                SqlStructureOperation::SetAggregates { aggregates } => {
                    set_aggregates(&mut parsed.query, context, aggregates)
                }
                SqlStructureOperation::SetGroupBy { columns } => {
                    set_group_by(&mut parsed.query, context, columns)
                }
                SqlStructureOperation::SetDistinct { distinct } => {
                    set_distinct(&mut parsed.query, distinct)
                }
                SqlStructureOperation::SetColumns { .. } => unreachable!(
                    "SetColumns is handled by the source-preserving projection rewrite"
                ),
            };
            if let Err(reason) = mutation {
                return rejected_proposal(base_sql, reason);
            }
            parsed.query.to_string()
        }
    };
    if let Err(reason) = parse_supported_sql(&proposed_sql, context) {
        return rejected_proposal(base_sql, reason);
    }

    let requires_confirmation = proposed_sql != base_sql;
    SqlStructureProposal {
        contract_version: SQL_STRUCTURE_CONTRACT_VERSION,
        status: SqlStructureStatus::Supported,
        reason: None,
        base_sql,
        proposed_sql: Some(proposed_sql),
        requires_confirmation,
        added_stable_tie_breakers,
    }
}

/// Propose changing the single Form relation through the Rust SQL AST.
///
/// Server-derived Form SQL relation names are supplied by the owning adapter
/// as `relation`; they are always quoted by this function. A relation-free
/// `SELECT 1` draft is the empty-workspace seed and becomes `SELECT * FROM <Form>`. Existing simple
/// relation queries retain their projection and clauses and are accepted only
/// when the target Form schema validates the resulting SQL.
fn propose_sql_relation_selection(
    sql: &str,
    context: &SqlStructureContext,
    relation: &str,
) -> SqlStructureProposal {
    let base_sql = sql.to_owned();
    if let Err(reason) = validate_context(context) {
        return rejected_proposal(base_sql, reason);
    }
    if context.relation.is_empty() || relation != context.relation {
        return rejected_proposal(base_sql, SqlStructureReason::InvalidContext);
    }
    if sql.len() > MAX_SQL_STRUCTURE_QUERY_BYTES {
        return rejected_proposal(base_sql, SqlStructureReason::SqlTooLarge);
    }

    let mut statements = match Parser::parse_sql(&PostgreSqlDialect {}, sql) {
        Ok(statements) => statements,
        Err(_) => return rejected_proposal(base_sql, SqlStructureReason::ParseFailed),
    };
    if statements.len() != 1 {
        return rejected_proposal(base_sql, SqlStructureReason::SingleStatementRequired);
    }
    let Statement::Query(mut query) = statements.remove(0) else {
        return rejected_proposal(base_sql, SqlStructureReason::SelectRequired);
    };
    if let Err(reason) = validate_query_shape(&query) {
        return rejected_proposal(base_sql, reason);
    }
    let SetExpr::Select(select) = query.body.as_mut() else {
        return rejected_proposal(base_sql, SqlStructureReason::SetOperationUnsupported);
    };
    if let Err(reason) = validate_select_shape(select) {
        return rejected_proposal(base_sql, reason);
    }

    let proposed_sql = if select.from.is_empty() {
        let relation_free_context = SqlStructureContext {
            relation: String::new(),
            columns: Vec::new(),
            variables: context.variables.clone(),
            stable_key: Vec::new(),
        };
        if let Err(reason) = parse_supported_sql(sql, &relation_free_context) {
            return rejected_proposal(base_sql, reason);
        }
        if select.projection.len() != 1
            || !matches!(
                &select.projection[0],
                SelectItem::UnnamedExpr(Expr::Value(value))
                    if matches!(&value.value, Value::Number(number, false) if number == "1")
            )
            || select.selection.is_some()
            || !matches!(&select.group_by, GroupByExpr::Expressions(expressions, modifiers) if expressions.is_empty() && modifiers.is_empty())
            || query.order_by.is_some()
        {
            return rejected_proposal(base_sql, SqlStructureReason::RelationUnsupported);
        }
        let Some(projection_range) = projection_source_range(sql, &query) else {
            return rejected_proposal(base_sql, SqlStructureReason::ProjectionUnsupported);
        };
        let Some(before) = sql.get(..projection_range.start) else {
            return rejected_proposal(base_sql, SqlStructureReason::ProjectionUnsupported);
        };
        let Some(after) = sql.get(projection_range.end..) else {
            return rejected_proposal(base_sql, SqlStructureReason::ProjectionUnsupported);
        };
        let quoted_relation = Ident::with_quote('"', relation.to_owned()).to_string();
        format!("{before}* FROM {quoted_relation}{after}")
    } else {
        if select.from.len() != 1 || !select.from[0].joins.is_empty() {
            return rejected_proposal(base_sql, SqlStructureReason::RelationUnsupported);
        }
        let TableFactor::Table {
            name,
            alias,
            args: None,
            with_hints,
            version: None,
            with_ordinality: false,
            partitions,
            json_path: None,
            sample: None,
            index_hints,
        } = &select.from[0].relation
        else {
            return rejected_proposal(base_sql, SqlStructureReason::RelationUnsupported);
        };
        if !with_hints.is_empty()
            || !partitions.is_empty()
            || !index_hints.is_empty()
            || alias
                .as_ref()
                .is_some_and(|alias| !alias.columns.is_empty() || alias.at.is_some())
        {
            return rejected_proposal(base_sql, SqlStructureReason::RelationUnsupported);
        }
        let Some(ObjectNamePart::Identifier(_)) = name.0.first() else {
            return rejected_proposal(base_sql, SqlStructureReason::RelationUnsupported);
        };
        if name.0.len() != 1 {
            return rejected_proposal(base_sql, SqlStructureReason::RelationUnsupported);
        }
        // Replace only the relation identifier so a supported table alias and
        // surrounding source text stay in place when the selected Form changes.
        let relation_range = source_range_from_span(sql, name.span());
        let Some(relation_range) = relation_range else {
            return rejected_proposal(base_sql, SqlStructureReason::RelationUnsupported);
        };
        if has_comment_token_in_range(sql, relation_range) != Some(false) {
            return rejected_proposal(base_sql, SqlStructureReason::RelationUnsupported);
        }
        let quoted_relation = Ident::with_quote('"', relation.to_owned()).to_string();
        let Some(rewritten) = replace_sql_source_range(sql, relation_range, &quoted_relation)
        else {
            return rejected_proposal(base_sql, SqlStructureReason::RelationUnsupported);
        };
        rewritten
    };

    if let Err(reason) = parse_supported_sql(&proposed_sql, context) {
        return rejected_proposal(base_sql, reason);
    }
    let requires_confirmation = proposed_sql != sql;
    SqlStructureProposal {
        contract_version: SQL_STRUCTURE_CONTRACT_VERSION,
        status: SqlStructureStatus::Supported,
        reason: None,
        base_sql,
        proposed_sql: Some(proposed_sql),
        requires_confirmation,
        added_stable_tie_breakers: Vec::new(),
    }
}

fn parse_supported_sql(
    sql: &str,
    context: &SqlStructureContext,
) -> Result<ParsedSqlStructure, SqlStructureReason> {
    validate_context(context)?;
    if sql.len() > MAX_SQL_STRUCTURE_QUERY_BYTES {
        return Err(SqlStructureReason::SqlTooLarge);
    }

    let mut statements = Parser::parse_sql(&PostgreSqlDialect {}, sql)
        .map_err(|_| SqlStructureReason::ParseFailed)?;
    if statements.len() != 1 {
        return Err(SqlStructureReason::SingleStatementRequired);
    }
    let Statement::Query(query) = statements.remove(0) else {
        return Err(SqlStructureReason::SelectRequired);
    };
    let query = *query;
    validate_query_shape(&query)?;
    let SetExpr::Select(select) = query.body.as_ref() else {
        return Err(SqlStructureReason::SetOperationUnsupported);
    };
    validate_select_shape(select)?;
    validate_relation(select, context)?;

    let (projection, aggregates) = parse_projection(&select.projection, context)?;
    let group_by = parse_group_by(&select.group_by, context)?;
    validate_grouped_projection(&projection, &aggregates, &group_by)?;
    let distinct = match select.distinct.as_ref() {
        None | Some(Distinct::All) => false,
        Some(Distinct::Distinct) => true,
        Some(Distinct::On(_)) => return Err(SqlStructureReason::DistinctUnsupported),
    };
    let filters = select
        .selection
        .as_ref()
        .map(|selection| parse_filters(selection, context))
        .transpose()?
        .unwrap_or_default();
    let order_by = query
        .order_by
        .as_ref()
        .map(|order_by| parse_order_by(order_by, context))
        .transpose()?
        .unwrap_or_default();
    validate_order_by_semantics(
        query.order_by.as_ref(),
        select,
        &aggregates,
        &group_by,
        distinct,
        context,
    )?;

    let referenced_variables = filters
        .iter()
        .filter_map(|filter| filter.parameter.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let stable_pagination = aggregates.is_empty()
        && group_by.is_empty()
        && !distinct
        && !context.stable_key.is_empty()
        && context
            .stable_key
            .iter()
            .all(|key| order_by.iter().any(|term| names_match(&term.column, key)));

    Ok(ParsedSqlStructure {
        query,
        projection,
        filters,
        aggregates,
        group_by,
        distinct,
        order_by,
        referenced_variables,
        stable_pagination,
    })
}

fn validate_context(context: &SqlStructureContext) -> Result<(), SqlStructureReason> {
    let relation_free = context.relation.is_empty();
    if context.relation.trim() != context.relation
        || context.relation.chars().any(char::is_control)
        || (!relation_free && context.columns.is_empty())
        || (relation_free && (!context.columns.is_empty() || !context.stable_key.is_empty()))
    {
        return Err(SqlStructureReason::InvalidContext);
    }

    let mut column_names = BTreeSet::new();
    for column in &context.columns {
        if !valid_schema_name(&column.name) || !column_names.insert(column.name.as_str()) {
            return Err(SqlStructureReason::InvalidContext);
        }
    }

    let mut variable_names = BTreeSet::new();
    for variable in &context.variables {
        if !valid_variable_name(&variable.name) || !variable_names.insert(variable.name.as_str()) {
            return Err(SqlStructureReason::InvalidContext);
        }
    }

    let mut stable_keys = BTreeSet::new();
    for key in &context.stable_key {
        if find_column(context, key).is_none() || !stable_keys.insert(key.as_str()) {
            return Err(SqlStructureReason::InvalidContext);
        }
    }
    Ok(())
}

fn validate_query_shape(query: &Query) -> Result<(), SqlStructureReason> {
    if query.with.is_some() {
        return Err(SqlStructureReason::CommonTableExpressionUnsupported);
    }
    if query.fetch.is_some()
        || !query.locks.is_empty()
        || query.for_clause.is_some()
        || query.settings.is_some()
        || query.format_clause.is_some()
        || !query.pipe_operators.is_empty()
        || !safe_limit_clause(query.limit_clause.as_ref())
    {
        return Err(SqlStructureReason::QueryClauseUnsupported);
    }
    Ok(())
}

fn safe_limit_clause(limit: Option<&LimitClause>) -> bool {
    match limit {
        None => true,
        Some(LimitClause::LimitOffset {
            limit,
            offset,
            limit_by,
        }) => {
            limit.as_ref().is_none_or(is_nonnegative_integer_literal)
                && offset
                    .as_ref()
                    .is_none_or(|offset| is_nonnegative_integer_literal(&offset.value))
                && limit_by.is_empty()
        }
        Some(LimitClause::OffsetCommaLimit { offset, limit }) => {
            is_nonnegative_integer_literal(offset) && is_nonnegative_integer_literal(limit)
        }
    }
}

fn is_nonnegative_integer_literal(expression: &Expr) -> bool {
    let Expr::Value(value) = expression else {
        return false;
    };
    let Value::Number(number, long) = &value.value else {
        return false;
    };
    !long && !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
}

fn supported_projection_literal(value: &Value) -> bool {
    match value {
        Value::Number(number, long) => !long && number.parse::<i64>().is_ok(),
        Value::SingleQuotedString(_) | Value::Boolean(_) | Value::Null => true,
        _ => false,
    }
}

fn validate_select_shape(select: &Select) -> Result<(), SqlStructureReason> {
    if select.select_modifiers.is_some()
        || select.top.is_some()
        || select.top_before_distinct
        || select.exclude.is_some()
        || select.into.is_some()
        || !select.lateral_views.is_empty()
        || select.prewhere.is_some()
        || !select.connect_by.is_empty()
        || !select.cluster_by.is_empty()
        || !select.distribute_by.is_empty()
        || !select.sort_by.is_empty()
        || select.having.is_some()
        || !select.named_window.is_empty()
        || select.qualify.is_some()
        || select.window_before_qualify
        || select.value_table_mode.is_some()
        || !select.optimizer_hints.is_empty()
        || select.flavor != SelectFlavor::Standard
    {
        return Err(SqlStructureReason::QueryClauseUnsupported);
    }
    Ok(())
}

fn validate_relation(
    select: &Select,
    context: &SqlStructureContext,
) -> Result<(), SqlStructureReason> {
    if select.from.is_empty() {
        return if context.relation.is_empty() {
            Ok(())
        } else {
            Err(SqlStructureReason::RelationUnsupported)
        };
    }
    if context.relation.is_empty() {
        return Err(SqlStructureReason::RelationUnsupported);
    }
    if select.from.len() != 1 {
        return Err(SqlStructureReason::RelationUnsupported);
    }
    let table = &select.from[0];
    if !table.joins.is_empty() {
        return Err(SqlStructureReason::JoinUnsupported);
    }
    let TableFactor::Table {
        name,
        alias,
        args,
        with_hints,
        version,
        with_ordinality,
        partitions,
        json_path,
        sample,
        index_hints,
    } = &table.relation
    else {
        return Err(SqlStructureReason::RelationUnsupported);
    };
    if alias
        .as_ref()
        .is_some_and(|alias| !alias.columns.is_empty() || alias.at.is_some())
        || args.is_some()
        || !with_hints.is_empty()
        || version.is_some()
        || *with_ordinality
        || !partitions.is_empty()
        || json_path.is_some()
        || sample.is_some()
        || !index_hints.is_empty()
        || name.0.len() != 1
    {
        return Err(SqlStructureReason::RelationUnsupported);
    }
    let Some(ObjectNamePart::Identifier(relation)) = name.0.first() else {
        return Err(SqlStructureReason::RelationUnsupported);
    };
    if !identifier_matches(relation, &context.relation) {
        return Err(SqlStructureReason::RelationMismatch);
    }
    Ok(())
}

fn parse_projection(
    projection: &[SelectItem],
    context: &SqlStructureContext,
) -> Result<(Vec<String>, Vec<SqlStructureAggregate>), SqlStructureReason> {
    if projection.is_empty() || projection.len() > 384 {
        return Err(SqlStructureReason::ProjectionUnsupported);
    }
    if projection.len() == 1 {
        if let SelectItem::Wildcard(options) = &projection[0] {
            if options.opt_ilike.is_none()
                && options.opt_exclude.is_none()
                && options.opt_except.is_none()
                && options.opt_replace.is_none()
                && options.opt_rename.is_none()
                && options.opt_alias.is_none()
            {
                if context.relation.is_empty() {
                    return Err(SqlStructureReason::ProjectionUnsupported);
                }
                return Ok((vec!["*".to_owned()], Vec::new()));
            }
        }
    }

    let mut columns = Vec::new();
    let mut aggregates = Vec::new();
    for item in projection {
        match item {
            SelectItem::UnnamedExpr(Expr::Identifier(identifier)) => {
                let column = find_column_by_ident(context, identifier)
                    .ok_or(SqlStructureReason::UnknownColumn)?;
                columns.push(column.name.clone());
            }
            SelectItem::UnnamedExpr(Expr::Value(value))
                if context.relation.is_empty() && supported_projection_literal(&value.value) =>
            {
                columns.push(value.value.to_string());
            }
            SelectItem::UnnamedExpr(Expr::Function(function)) if !context.relation.is_empty() => {
                aggregates.push(parse_aggregate(function, None, context)?);
            }
            SelectItem::ExprWithAlias {
                expr: Expr::Function(function),
                alias,
            } if !context.relation.is_empty() => {
                aggregates.push(parse_aggregate(
                    function,
                    Some(SqlStructureAlias {
                        name: alias.value.clone(),
                        quoted: alias.quote_style.is_some(),
                    }),
                    context,
                )?);
            }
            _ => return Err(SqlStructureReason::ProjectionUnsupported),
        }
    }
    if columns.len() > 256 || aggregates.len() > 128 {
        return Err(SqlStructureReason::ProjectionUnsupported);
    }
    if columns.is_empty() && aggregates.is_empty() {
        return Err(SqlStructureReason::ProjectionUnsupported);
    }
    Ok((columns, aggregates))
}

fn parse_aggregate(
    function: &Function,
    alias: Option<SqlStructureAlias>,
    context: &SqlStructureContext,
) -> Result<SqlStructureAggregate, SqlStructureReason> {
    if function.uses_odbc_syntax
        || function.parameters != FunctionArguments::None
        || function.filter.is_some()
        || function.null_treatment.is_some()
        || function.over.is_some()
        || !function.within_group.is_empty()
        || function.name.0.len() != 1
    {
        return Err(SqlStructureReason::AggregateUnsupported);
    }
    let Some(ObjectNamePart::Identifier(name)) = function.name.0.first() else {
        return Err(SqlStructureReason::AggregateUnsupported);
    };
    if name.quote_style.is_some() {
        return Err(SqlStructureReason::AggregateUnsupported);
    }
    let aggregate_function = match name.value.to_ascii_lowercase().as_str() {
        "count" => SqlStructureAggregateFunction::Count,
        "sum" => SqlStructureAggregateFunction::Sum,
        "avg" => SqlStructureAggregateFunction::Avg,
        "min" => SqlStructureAggregateFunction::Min,
        "max" => SqlStructureAggregateFunction::Max,
        _ => return Err(SqlStructureReason::AggregateUnsupported),
    };
    let FunctionArguments::List(arguments) = &function.args else {
        return Err(SqlStructureReason::AggregateUnsupported);
    };
    if !arguments.clauses.is_empty() || arguments.args.len() != 1 {
        return Err(SqlStructureReason::AggregateUnsupported);
    }
    let distinct = match arguments.duplicate_treatment {
        None | Some(DuplicateTreatment::All) => false,
        Some(DuplicateTreatment::Distinct) => true,
    };
    let FunctionArg::Unnamed(argument) = &arguments.args[0] else {
        return Err(SqlStructureReason::AggregateUnsupported);
    };
    let column = match argument {
        FunctionArgExpr::Wildcard if aggregate_function == SqlStructureAggregateFunction::Count => {
            if distinct {
                return Err(SqlStructureReason::AggregateUnsupported);
            }
            None
        }
        FunctionArgExpr::Expr(Expr::Identifier(identifier)) => {
            let column = find_column_by_ident(context, identifier)
                .ok_or(SqlStructureReason::UnknownColumn)?;
            if distinct && !column_type_supports_distinct_aggregate(column.column_type) {
                return Err(SqlStructureReason::AggregateTypeUnsupported);
            }
            if matches!(
                aggregate_function,
                SqlStructureAggregateFunction::Sum | SqlStructureAggregateFunction::Avg
            ) && !column_type_supports_numeric_aggregate(column.column_type)
            {
                return Err(SqlStructureReason::AggregateTypeUnsupported);
            }
            if matches!(
                aggregate_function,
                SqlStructureAggregateFunction::Min | SqlStructureAggregateFunction::Max
            ) && !column_type_supports_sort(column.column_type)
            {
                return Err(SqlStructureReason::AggregateTypeUnsupported);
            }
            Some(column.name.clone())
        }
        _ => return Err(SqlStructureReason::AggregateUnsupported),
    };
    if alias.as_ref().is_some_and(|alias| {
        alias.name.is_empty()
            || alias.name.len() > 1024
            || alias.name.chars().any(char::is_control)
            || (!alias.quoted && !valid_variable_name(&alias.name))
    }) {
        return Err(SqlStructureReason::AggregateUnsupported);
    }
    Ok(SqlStructureAggregate {
        function: aggregate_function,
        column,
        distinct,
        alias,
    })
}

fn parse_group_by(
    group_by: &GroupByExpr,
    context: &SqlStructureContext,
) -> Result<Vec<String>, SqlStructureReason> {
    let GroupByExpr::Expressions(expressions, modifiers) = group_by else {
        return Err(SqlStructureReason::GroupByUnsupported);
    };
    if !modifiers.is_empty() || expressions.len() > 256 {
        return Err(SqlStructureReason::GroupByUnsupported);
    }
    let mut columns = Vec::with_capacity(expressions.len());
    let mut seen = BTreeSet::new();
    for expression in expressions {
        let Expr::Identifier(identifier) = expression else {
            return Err(SqlStructureReason::GroupByUnsupported);
        };
        let column =
            find_column_by_ident(context, identifier).ok_or(SqlStructureReason::UnknownColumn)?;
        if !seen.insert(column.name.as_str()) {
            return Err(SqlStructureReason::GroupByUnsupported);
        }
        columns.push(column.name.clone());
    }
    Ok(columns)
}

fn validate_grouped_projection(
    projection: &[String],
    aggregates: &[SqlStructureAggregate],
    group_by: &[String],
) -> Result<(), SqlStructureReason> {
    if projection.len() == 1 && projection[0] == "*" {
        if !aggregates.is_empty() || !group_by.is_empty() {
            return Err(SqlStructureReason::GroupByUnsupported);
        }
        return Ok(());
    }
    if (!aggregates.is_empty() || !group_by.is_empty())
        && projection
            .iter()
            .any(|column| !group_by.iter().any(|group| names_match(column, group)))
    {
        return Err(SqlStructureReason::GroupByUnsupported);
    }
    Ok(())
}

fn parse_filters(
    expression: &Expr,
    context: &SqlStructureContext,
) -> Result<Vec<SqlStructureFilter>, SqlStructureReason> {
    let mut filters = Vec::new();
    flatten_filters(expression, context, &mut filters)?;
    Ok(filters)
}

fn flatten_filters(
    expression: &Expr,
    context: &SqlStructureContext,
    filters: &mut Vec<SqlStructureFilter>,
) -> Result<(), SqlStructureReason> {
    match expression {
        Expr::Nested(inner) => flatten_filters(inner, context, filters),
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And,
            right,
        } => {
            flatten_filters(left, context, filters)?;
            flatten_filters(right, context, filters)
        }
        Expr::BinaryOp { left, op, right } => {
            let Expr::Identifier(identifier) = left.as_ref() else {
                return Err(SqlStructureReason::FilterUnsupported);
            };
            let column = find_column_by_ident(context, identifier)
                .ok_or(SqlStructureReason::UnknownColumn)?;
            let operator =
                filter_operator_from_binary(op).ok_or(SqlStructureReason::FilterUnsupported)?;
            let parameter =
                placeholder_name(right.as_ref()).ok_or(SqlStructureReason::FilterUnsupported)?;
            let filter = SqlStructureFilter {
                column: column.name.clone(),
                operator,
                parameter: Some(parameter),
            };
            validate_filter(context, &filter)?;
            filters.push(filter);
            Ok(())
        }
        Expr::IsNull(inner) | Expr::IsNotNull(inner) => {
            let Expr::Identifier(identifier) = inner.as_ref() else {
                return Err(SqlStructureReason::FilterUnsupported);
            };
            let column = find_column_by_ident(context, identifier)
                .ok_or(SqlStructureReason::UnknownColumn)?;
            let operator = if matches!(expression, Expr::IsNull(_)) {
                SqlStructureFilterOperator::IsNull
            } else {
                SqlStructureFilterOperator::IsNotNull
            };
            filters.push(SqlStructureFilter {
                column: column.name.clone(),
                operator,
                parameter: None,
            });
            Ok(())
        }
        _ => Err(SqlStructureReason::FilterUnsupported),
    }
}

fn parse_order_by(
    order_by: &OrderBy,
    context: &SqlStructureContext,
) -> Result<Vec<SqlStructureSort>, SqlStructureReason> {
    let OrderByKind::Expressions(expressions) = &order_by.kind else {
        return Err(SqlStructureReason::SortUnsupported);
    };
    if order_by.interpolate.is_some() {
        return Err(SqlStructureReason::SortUnsupported);
    }
    let mut seen = BTreeSet::new();
    expressions
        .iter()
        .map(|expression| {
            let Expr::Identifier(identifier) = &expression.expr else {
                return Err(SqlStructureReason::SortUnsupported);
            };
            if expression.with_fill.is_some() || expression.options.nulls_first.is_some() {
                return Err(SqlStructureReason::SortUnsupported);
            }
            let column = find_column_by_ident(context, identifier)
                .ok_or(SqlStructureReason::UnknownColumn)?;
            if !column_type_supports_sort(column.column_type) {
                return Err(SqlStructureReason::SortUnsupported);
            }
            if !seen.insert(column.name.as_str()) {
                return Err(SqlStructureReason::DuplicateColumn);
            }
            Ok(SqlStructureSort {
                column: column.name.clone(),
                direction: if expression.options.asc == Some(false) {
                    SqlStructureSortDirection::Desc
                } else {
                    SqlStructureSortDirection::Asc
                },
            })
        })
        .collect()
}

fn validate_order_by_semantics(
    order_by: Option<&OrderBy>,
    select: &Select,
    aggregates: &[SqlStructureAggregate],
    group_by: &[String],
    distinct: bool,
    context: &SqlStructureContext,
) -> Result<(), SqlStructureReason> {
    let Some(order_by) = order_by else {
        return Ok(());
    };
    let OrderByKind::Expressions(expressions) = &order_by.kind else {
        return Err(SqlStructureReason::SortUnsupported);
    };

    let wildcard_projection = select.projection.len() == 1
        && matches!(&select.projection[0], SelectItem::Wildcard(options)
            if options.opt_ilike.is_none()
                && options.opt_exclude.is_none()
                && options.opt_except.is_none()
                && options.opt_replace.is_none()
                && options.opt_rename.is_none()
                && options.opt_alias.is_none());

    for expression in expressions {
        let Expr::Identifier(identifier) = &expression.expr else {
            return Err(SqlStructureReason::SortUnsupported);
        };
        let is_aggregate_alias = aggregates.iter().any(|aggregate| {
            aggregate
                .alias
                .as_ref()
                .is_some_and(|alias| identifier_matches_alias(identifier, alias))
        });
        if distinct {
            let selected = wildcard_projection
                || is_aggregate_alias
                || select.projection.iter().any(|item| match item {
                    SelectItem::UnnamedExpr(Expr::Identifier(projected)) => {
                        identifiers_resolve_same(identifier, projected)
                    }
                    SelectItem::ExprWithAlias { alias, .. } => {
                        identifiers_resolve_same(identifier, alias)
                    }
                    _ => false,
                });
            if !selected {
                return Err(SqlStructureReason::DistinctUnsupported);
            }
        } else if !aggregates.is_empty() || !group_by.is_empty() {
            if is_aggregate_alias {
                continue;
            }
            let column = find_column_by_ident(context, identifier)
                .ok_or(SqlStructureReason::UnknownColumn)?;
            if !group_by.iter().any(|group| group == &column.name) {
                return Err(SqlStructureReason::SortUnsupported);
            }
        }
    }
    Ok(())
}

fn identifier_matches_alias(identifier: &Ident, alias: &SqlStructureAlias) -> bool {
    let alias = if alias.quoted {
        Ident::with_quote('"', alias.name.clone())
    } else {
        Ident::new(alias.name.clone())
    };
    identifiers_resolve_same(identifier, &alias)
}

fn identifiers_resolve_same(left: &Ident, right: &Ident) -> bool {
    let resolved_name = |identifier: &Ident| {
        if identifier.quote_style.is_some() {
            identifier.value.clone()
        } else {
            identifier.value.to_ascii_lowercase()
        }
    };
    resolved_name(left) == resolved_name(right)
}

fn filter_operator_from_binary(operator: &BinaryOperator) -> Option<SqlStructureFilterOperator> {
    match operator {
        BinaryOperator::Eq => Some(SqlStructureFilterOperator::Eq),
        BinaryOperator::NotEq => Some(SqlStructureFilterOperator::NotEq),
        BinaryOperator::Lt => Some(SqlStructureFilterOperator::Lt),
        BinaryOperator::LtEq => Some(SqlStructureFilterOperator::LtEq),
        BinaryOperator::Gt => Some(SqlStructureFilterOperator::Gt),
        BinaryOperator::GtEq => Some(SqlStructureFilterOperator::GtEq),
        _ => None,
    }
}

fn binary_from_filter_operator(operator: SqlStructureFilterOperator) -> Option<BinaryOperator> {
    match operator {
        SqlStructureFilterOperator::Eq => Some(BinaryOperator::Eq),
        SqlStructureFilterOperator::NotEq => Some(BinaryOperator::NotEq),
        SqlStructureFilterOperator::Lt => Some(BinaryOperator::Lt),
        SqlStructureFilterOperator::LtEq => Some(BinaryOperator::LtEq),
        SqlStructureFilterOperator::Gt => Some(BinaryOperator::Gt),
        SqlStructureFilterOperator::GtEq => Some(BinaryOperator::GtEq),
        SqlStructureFilterOperator::IsNull | SqlStructureFilterOperator::IsNotNull => None,
    }
}

fn placeholder_name(expression: &Expr) -> Option<String> {
    let Expr::Value(value) = expression else {
        return None;
    };
    let Value::Placeholder(placeholder) = &value.value else {
        return None;
    };
    let name = placeholder.strip_prefix('$')?;
    valid_variable_name(name).then(|| name.to_owned())
}

fn validate_filter(
    context: &SqlStructureContext,
    filter: &SqlStructureFilter,
) -> Result<(), SqlStructureReason> {
    let column = find_column(context, &filter.column).ok_or(SqlStructureReason::UnknownColumn)?;
    match filter.operator {
        SqlStructureFilterOperator::IsNull | SqlStructureFilterOperator::IsNotNull => {
            if filter.parameter.is_some() {
                return Err(SqlStructureReason::FilterUnsupported);
            }
            return Ok(());
        }
        _ => {}
    }

    let parameter = filter
        .parameter
        .as_deref()
        .filter(|name| valid_variable_name(name))
        .ok_or(SqlStructureReason::InvalidVariableName)?;
    let variable = context
        .variables
        .iter()
        .find(|variable| variable.name == parameter)
        .ok_or(SqlStructureReason::UnknownVariable)?;
    if variable.variable_type.column_type() != column.column_type {
        return Err(SqlStructureReason::VariableTypeMismatch);
    }
    if !column_type_supports_operator(column.column_type, filter.operator) {
        return Err(SqlStructureReason::FilterTypeUnsupported);
    }
    Ok(())
}

fn column_type_supports_operator(
    column_type: SqlStructureColumnType,
    operator: SqlStructureFilterOperator,
) -> bool {
    match column_type {
        SqlStructureColumnType::String
        | SqlStructureColumnType::Integer
        | SqlStructureColumnType::Float
        | SqlStructureColumnType::Date
        | SqlStructureColumnType::Timestamp
        | SqlStructureColumnType::Decimal => true,
        SqlStructureColumnType::Boolean => matches!(
            operator,
            SqlStructureFilterOperator::Eq | SqlStructureFilterOperator::NotEq
        ),
        SqlStructureColumnType::Json | SqlStructureColumnType::Other => false,
    }
}

fn validate_schema_columns(
    context: &SqlStructureContext,
    columns: &[String],
) -> Result<Vec<String>, SqlStructureReason> {
    if columns.is_empty() {
        return Err(SqlStructureReason::EmptyProjection);
    }
    if columns.len() > 256 {
        return Err(SqlStructureReason::ProjectionUnsupported);
    }
    validate_schema_columns_allow_empty(context, columns)
}

fn validate_schema_columns_allow_empty(
    context: &SqlStructureContext,
    columns: &[String],
) -> Result<Vec<String>, SqlStructureReason> {
    if columns.len() > 256 {
        return Err(SqlStructureReason::ProjectionUnsupported);
    }
    let mut selected = Vec::with_capacity(columns.len());
    let mut seen = BTreeSet::new();
    for name in columns {
        let column = find_column(context, name).ok_or(SqlStructureReason::UnknownColumn)?;
        if !seen.insert(column.name.as_str()) {
            return Err(SqlStructureReason::DuplicateColumn);
        }
        selected.push(column.name.clone());
    }
    Ok(selected)
}

fn rewrite_projection(
    sql: &str,
    query: &Query,
    context: &SqlStructureContext,
    columns: Vec<String>,
) -> Result<String, SqlStructureReason> {
    let columns = validate_schema_columns(context, &columns)?;
    let SetExpr::Select(select) = query.body.as_ref() else {
        return Err(SqlStructureReason::SetOperationUnsupported);
    };
    if select.projection.iter().any(select_item_is_aggregate) {
        return Err(SqlStructureReason::ProjectionUnsupported);
    }
    let source_range =
        projection_source_range(sql, query).ok_or(SqlStructureReason::ProjectionUnsupported)?;
    let replacement = columns
        .into_iter()
        .map(|column| Ident::with_quote('"', column).to_string())
        .collect::<Vec<_>>()
        .join(", ");
    replace_sql_source_range(sql, source_range, &replacement)
        .ok_or(SqlStructureReason::ProjectionUnsupported)
}

fn projection_source_range(sql: &str, query: &Query) -> Option<SqlSourceRange> {
    let SetExpr::Select(select) = query.body.as_ref() else {
        return None;
    };
    let first_span = select.projection.first()?.span();
    let last_span = select.projection.last()?.span();
    if first_span == Span::empty() || last_span == Span::empty() {
        return None;
    }

    let range = source_range_from_span(sql, Span::new(first_span.start, last_span.end))?;
    if range.start >= range.end {
        return None;
    }
    if has_comment_token_in_range(sql, range)? {
        return None;
    }

    let mut previous_end = range.start;
    for item in &select.projection {
        let item_span = item.span();
        if item_span == Span::empty() {
            return None;
        }
        let item_range = source_range_from_span(sql, item_span)?;
        if item_range.start < previous_end
            || item_range.start >= item_range.end
            || item_range.end > range.end
        {
            return None;
        }
        previous_end = item_range.end;
    }
    Some(range)
}

fn replace_sql_source_range(sql: &str, range: SqlSourceRange, replacement: &str) -> Option<String> {
    if range.start >= range.end {
        return None;
    }
    let before = sql.get(..range.start)?;
    let after = sql.get(range.end..)?;
    Some(format!("{before}{replacement}{after}"))
}

fn set_filters(
    query: &mut Query,
    context: &SqlStructureContext,
    filters: Vec<SqlStructureFilter>,
) -> Result<(), SqlStructureReason> {
    if filters.len() > 128 {
        return Err(SqlStructureReason::FilterUnsupported);
    }
    let mut expressions = Vec::with_capacity(filters.len());
    for filter in filters {
        validate_filter(context, &filter)?;
        let column = Expr::Identifier(Ident::with_quote('"', filter.column));
        let expression = match filter.operator {
            SqlStructureFilterOperator::IsNull => Expr::IsNull(Box::new(column)),
            SqlStructureFilterOperator::IsNotNull => Expr::IsNotNull(Box::new(column)),
            operator => {
                let sql_operator = binary_from_filter_operator(operator)
                    .ok_or(SqlStructureReason::FilterUnsupported)?;
                let parameter = filter
                    .parameter
                    .ok_or(SqlStructureReason::InvalidVariableName)?;
                Expr::BinaryOp {
                    left: Box::new(column),
                    op: sql_operator,
                    right: Box::new(Expr::value(Value::Placeholder(format!("${parameter}")))),
                }
            }
        };
        expressions.push(expression);
    }

    let selection = expressions
        .into_iter()
        .reduce(|left, right| Expr::BinaryOp {
            left: Box::new(left),
            op: BinaryOperator::And,
            right: Box::new(right),
        });
    select_mut(query)?.selection = selection;
    Ok(())
}

fn set_sort(
    query: &mut Query,
    context: &SqlStructureContext,
    order_by: Vec<SqlStructureSort>,
    added_tie_breakers: &mut Vec<String>,
) -> Result<(), SqlStructureReason> {
    let select = select_mut(query)?;
    if select.projection.iter().any(select_item_is_aggregate)
        || !matches!(&select.group_by, GroupByExpr::Expressions(columns, modifiers) if columns.is_empty() && modifiers.is_empty())
        || matches!(select.distinct, Some(Distinct::Distinct | Distinct::On(_)))
    {
        return Err(SqlStructureReason::SortUnsupported);
    }
    if context.stable_key.is_empty() {
        return Err(SqlStructureReason::StableKeyUnavailable);
    }
    if !stable_key_is_orderable(context) {
        return Err(SqlStructureReason::StableKeyUnorderable);
    }
    if order_by.len() > 256 {
        return Err(SqlStructureReason::SortUnsupported);
    }
    let mut validated = validate_sort_columns(context, order_by)?;
    for key in &context.stable_key {
        if !validated.iter().any(|sort| names_match(&sort.column, key)) {
            validated.push(SqlStructureSort {
                column: key.clone(),
                direction: SqlStructureSortDirection::Asc,
            });
            added_tie_breakers.push(key.clone());
        }
    }
    query.order_by = Some(OrderBy {
        kind: OrderByKind::Expressions(
            validated
                .into_iter()
                .map(|sort| OrderByExpr {
                    expr: Expr::Identifier(Ident::with_quote('"', sort.column)),
                    options: OrderByOptions {
                        asc: Some(sort.direction == SqlStructureSortDirection::Asc),
                        nulls_first: None,
                    },
                    with_fill: None,
                })
                .collect(),
        ),
        interpolate: None,
    });
    Ok(())
}

fn set_aggregates(
    query: &mut Query,
    context: &SqlStructureContext,
    aggregates: Vec<SqlStructureAggregate>,
) -> Result<(), SqlStructureReason> {
    if aggregates.len() > 128 {
        return Err(SqlStructureReason::AggregateUnsupported);
    }
    for aggregate in &aggregates {
        validate_aggregate(context, aggregate)?;
    }

    let select = select_mut(query)?;
    let mut remaining = aggregates.into_iter();
    let mut projection = Vec::with_capacity(select.projection.len());
    let was_wildcard = select.projection.len() == 1
        && matches!(&select.projection[0], SelectItem::Wildcard(options)
            if options.opt_ilike.is_none()
                && options.opt_exclude.is_none()
                && options.opt_except.is_none()
                && options.opt_replace.is_none()
                && options.opt_rename.is_none()
                && options.opt_alias.is_none());

    if was_wildcard {
        projection.extend(remaining.map(aggregate_to_select_item));
    } else {
        for item in &select.projection {
            if select_item_is_aggregate(item) {
                if let Some(mut aggregate) = remaining.next() {
                    if aggregate.alias.is_none() {
                        aggregate.alias = select_item_alias(item);
                    }
                    projection.push(aggregate_to_select_item(aggregate));
                }
            } else {
                projection.push(item.clone());
            }
        }
        projection.extend(remaining.map(aggregate_to_select_item));
    }

    if projection.is_empty() {
        let group_by = parse_group_by(&select.group_by, context)?;
        if group_by.is_empty() {
            projection.push(SelectItem::Wildcard(Default::default()));
        } else {
            projection.extend(group_by.into_iter().map(|column| {
                SelectItem::UnnamedExpr(Expr::Identifier(Ident::with_quote('"', column)))
            }));
        }
    }
    select.projection = projection;
    Ok(())
}

fn set_group_by(
    query: &mut Query,
    context: &SqlStructureContext,
    columns: Vec<String>,
) -> Result<(), SqlStructureReason> {
    if columns.len() > 256 {
        return Err(SqlStructureReason::GroupByUnsupported);
    }
    let columns = validate_schema_columns_allow_empty(context, &columns)?;
    let select = select_mut(query)?;
    select.group_by = GroupByExpr::Expressions(
        columns
            .into_iter()
            .map(|column| Expr::Identifier(Ident::with_quote('"', column)))
            .collect(),
        Vec::new(),
    );
    Ok(())
}

fn set_distinct(query: &mut Query, distinct: bool) -> Result<(), SqlStructureReason> {
    select_mut(query)?.distinct = distinct.then_some(Distinct::Distinct);
    Ok(())
}

fn validate_aggregate(
    context: &SqlStructureContext,
    aggregate: &SqlStructureAggregate,
) -> Result<(), SqlStructureReason> {
    match (&aggregate.function, aggregate.column.as_deref()) {
        (SqlStructureAggregateFunction::Count, None) if !aggregate.distinct => {}
        (SqlStructureAggregateFunction::Count, Some(column)) => {
            find_column(context, column).ok_or(SqlStructureReason::UnknownColumn)?;
        }
        (SqlStructureAggregateFunction::Sum | SqlStructureAggregateFunction::Avg, Some(column)) => {
            let column = find_column(context, column).ok_or(SqlStructureReason::UnknownColumn)?;
            if !column_type_supports_numeric_aggregate(column.column_type) {
                return Err(SqlStructureReason::AggregateTypeUnsupported);
            }
        }
        (SqlStructureAggregateFunction::Min | SqlStructureAggregateFunction::Max, Some(column)) => {
            let column = find_column(context, column).ok_or(SqlStructureReason::UnknownColumn)?;
            if !column_type_supports_sort(column.column_type) {
                return Err(SqlStructureReason::AggregateTypeUnsupported);
            }
        }
        _ => return Err(SqlStructureReason::AggregateUnsupported),
    }
    if aggregate.alias.as_ref().is_some_and(|alias| {
        alias.name.is_empty()
            || alias.name.len() > 1024
            || alias.name.chars().any(char::is_control)
            || (!alias.quoted && !valid_variable_name(&alias.name))
    }) {
        return Err(SqlStructureReason::AggregateUnsupported);
    }
    Ok(())
}

fn aggregate_to_select_item(aggregate: SqlStructureAggregate) -> SelectItem {
    let function_name = match aggregate.function {
        SqlStructureAggregateFunction::Count => "COUNT",
        SqlStructureAggregateFunction::Sum => "SUM",
        SqlStructureAggregateFunction::Avg => "AVG",
        SqlStructureAggregateFunction::Min => "MIN",
        SqlStructureAggregateFunction::Max => "MAX",
    };
    let argument = match aggregate.column {
        Some(column) => FunctionArgExpr::Expr(Expr::Identifier(Ident::with_quote('"', column))),
        None => FunctionArgExpr::Wildcard,
    };
    let function = Expr::Function(Function {
        name: ObjectName(vec![ObjectNamePart::Identifier(Ident::new(function_name))]),
        uses_odbc_syntax: false,
        parameters: FunctionArguments::None,
        args: FunctionArguments::List(FunctionArgumentList {
            duplicate_treatment: aggregate.distinct.then_some(DuplicateTreatment::Distinct),
            args: vec![FunctionArg::Unnamed(argument)],
            clauses: Vec::new(),
        }),
        filter: None,
        null_treatment: None,
        over: None,
        within_group: Vec::new(),
    });
    match aggregate.alias {
        Some(alias) => SelectItem::ExprWithAlias {
            expr: function,
            alias: if alias.quoted {
                Ident::with_quote('"', alias.name)
            } else {
                Ident::new(alias.name)
            },
        },
        None => SelectItem::UnnamedExpr(function),
    }
}

fn select_item_is_aggregate(item: &SelectItem) -> bool {
    matches!(
        item,
        SelectItem::UnnamedExpr(Expr::Function(_))
            | SelectItem::ExprWithAlias {
                expr: Expr::Function(_),
                ..
            }
    )
}

fn select_item_alias(item: &SelectItem) -> Option<SqlStructureAlias> {
    let SelectItem::ExprWithAlias {
        expr: Expr::Function(_),
        alias,
    } = item
    else {
        return None;
    };
    Some(SqlStructureAlias {
        name: alias.value.clone(),
        quoted: alias.quote_style.is_some(),
    })
}

fn validate_sort_columns(
    context: &SqlStructureContext,
    order_by: Vec<SqlStructureSort>,
) -> Result<Vec<SqlStructureSort>, SqlStructureReason> {
    let mut validated = Vec::with_capacity(order_by.len());
    let mut seen = BTreeSet::new();
    for sort in order_by {
        let column = find_column(context, &sort.column).ok_or(SqlStructureReason::UnknownColumn)?;
        if !column_type_supports_sort(column.column_type) {
            return Err(SqlStructureReason::SortUnsupported);
        }
        if !seen.insert(column.name.as_str()) {
            return Err(SqlStructureReason::DuplicateColumn);
        }
        validated.push(SqlStructureSort {
            column: column.name.clone(),
            direction: sort.direction,
        });
    }
    Ok(validated)
}

fn select_mut(query: &mut Query) -> Result<&mut Select, SqlStructureReason> {
    match query.body.as_mut() {
        SetExpr::Select(select) => Ok(select.as_mut()),
        _ => Err(SqlStructureReason::SetOperationUnsupported),
    }
}

fn supported_analysis(
    parsed: ParsedSqlStructure,
    context: &SqlStructureContext,
) -> SqlStructureAnalysis {
    let columns_available = !context.columns.is_empty();
    let structured_projection = parsed.aggregates.is_empty() && parsed.group_by.is_empty();
    let columns_enabled = columns_available && structured_projection;
    let literal_projection = context.relation.is_empty()
        && !parsed.projection.is_empty()
        && parsed.aggregates.is_empty()
        && parsed.group_by.is_empty();
    let distinct_enabled = columns_available || literal_projection;
    let sort_available =
        structured_projection && !parsed.distinct && stable_key_is_orderable(context);
    let pagination_reason = if parsed.stable_pagination {
        None
    } else if !structured_projection || parsed.distinct {
        Some(SqlStructureReason::SortUnsupported)
    } else if sort_available {
        Some(SqlStructureReason::StableOrderRequired)
    } else if context.stable_key.is_empty() {
        Some(SqlStructureReason::StableKeyUnavailable)
    } else {
        Some(SqlStructureReason::StableKeyUnorderable)
    };
    SqlStructureAnalysis {
        contract_version: SQL_STRUCTURE_CONTRACT_VERSION,
        status: SqlStructureStatus::Supported,
        reason: None,
        projection: parsed.projection,
        filters: parsed.filters,
        aggregates: parsed.aggregates,
        group_by: parsed.group_by,
        distinct: parsed.distinct,
        order_by: parsed.order_by,
        referenced_variables: parsed.referenced_variables,
        stable_pagination: parsed.stable_pagination,
        capabilities: SqlStructureCapabilities {
            columns: capability(
                columns_enabled,
                (!columns_available)
                    .then_some(SqlStructureReason::InvalidContext)
                    .or_else(|| {
                        (!structured_projection)
                            .then_some(SqlStructureReason::ProjectionUnsupported)
                    }),
            ),
            filter: capability(
                columns_available,
                (!columns_available).then_some(SqlStructureReason::InvalidContext),
            ),
            sort: capability(
                sort_available,
                (!sort_available).then_some(if !structured_projection || parsed.distinct {
                    SqlStructureReason::SortUnsupported
                } else if context.stable_key.is_empty() {
                    SqlStructureReason::StableKeyUnavailable
                } else {
                    SqlStructureReason::StableKeyUnorderable
                }),
            ),
            aggregate: capability(columns_available, None),
            group_by: capability(columns_available, None),
            distinct: capability(distinct_enabled, None),
            stable_pagination: capability(parsed.stable_pagination, pagination_reason),
        },
    }
}

fn stable_key_is_orderable(context: &SqlStructureContext) -> bool {
    !context.stable_key.is_empty()
        && context.stable_key.iter().all(|key| {
            find_column(context, key)
                .is_some_and(|column| column_type_supports_sort(column.column_type))
        })
}

fn column_type_supports_sort(column_type: SqlStructureColumnType) -> bool {
    matches!(
        column_type,
        SqlStructureColumnType::String
            | SqlStructureColumnType::Boolean
            | SqlStructureColumnType::Integer
            | SqlStructureColumnType::Float
            | SqlStructureColumnType::Date
            | SqlStructureColumnType::Timestamp
            | SqlStructureColumnType::Decimal
    )
}

fn column_type_supports_numeric_aggregate(column_type: SqlStructureColumnType) -> bool {
    matches!(
        column_type,
        SqlStructureColumnType::Integer
            | SqlStructureColumnType::Float
            | SqlStructureColumnType::Decimal
    )
}

fn column_type_supports_distinct_aggregate(column_type: SqlStructureColumnType) -> bool {
    !matches!(
        column_type,
        SqlStructureColumnType::Json | SqlStructureColumnType::Other
    )
}

fn rejected_analysis(reason: SqlStructureReason) -> SqlStructureAnalysis {
    let status = match reason {
        SqlStructureReason::SqlTooLarge
        | SqlStructureReason::ParseFailed
        | SqlStructureReason::SingleStatementRequired
        | SqlStructureReason::SelectRequired => SqlStructureStatus::InvalidSql,
        _ => SqlStructureStatus::Unsupported,
    };
    let disabled = capability(false, Some(reason));
    SqlStructureAnalysis {
        contract_version: SQL_STRUCTURE_CONTRACT_VERSION,
        status,
        reason: Some(reason),
        projection: Vec::new(),
        filters: Vec::new(),
        aggregates: Vec::new(),
        group_by: Vec::new(),
        distinct: false,
        order_by: Vec::new(),
        referenced_variables: Vec::new(),
        stable_pagination: false,
        capabilities: SqlStructureCapabilities {
            columns: disabled.clone(),
            filter: disabled.clone(),
            sort: disabled.clone(),
            aggregate: disabled.clone(),
            group_by: disabled.clone(),
            distinct: disabled.clone(),
            stable_pagination: disabled,
        },
    }
}

fn rejected_proposal(base_sql: String, reason: SqlStructureReason) -> SqlStructureProposal {
    let analysis = rejected_analysis(reason);
    SqlStructureProposal {
        contract_version: SQL_STRUCTURE_CONTRACT_VERSION,
        status: analysis.status,
        reason: Some(reason),
        base_sql,
        proposed_sql: None,
        requires_confirmation: false,
        added_stable_tie_breakers: Vec::new(),
    }
}

fn capability(enabled: bool, reason: Option<SqlStructureReason>) -> SqlStructureActionCapability {
    SqlStructureActionCapability { enabled, reason }
}

fn find_column<'a>(context: &'a SqlStructureContext, name: &str) -> Option<&'a SqlStructureColumn> {
    context.columns.iter().find(|column| column.name == name)
}

fn find_column_by_ident<'a>(
    context: &'a SqlStructureContext,
    identifier: &Ident,
) -> Option<&'a SqlStructureColumn> {
    context
        .columns
        .iter()
        .find(|column| identifier_matches(identifier, &column.name))
}

fn identifier_matches(identifier: &Ident, expected: &str) -> bool {
    if identifier.quote_style.is_some() {
        identifier.value == expected
    } else {
        identifier.value.eq_ignore_ascii_case(expected)
    }
}

fn names_match(left: &str, right: &str) -> bool {
    left == right
}

fn valid_schema_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 1024 && !name.chars().any(char::is_control)
}

fn valid_variable_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    name.len() <= 128
        && (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> SqlStructureContext {
        SqlStructureContext {
            relation: "Task".to_owned(),
            columns: vec![
                SqlStructureColumn {
                    name: "_ugoite_id".to_owned(),
                    column_type: SqlStructureColumnType::String,
                },
                SqlStructureColumn {
                    name: "field_101".to_owned(),
                    column_type: SqlStructureColumnType::String,
                },
                SqlStructureColumn {
                    name: "field_102".to_owned(),
                    column_type: SqlStructureColumnType::Integer,
                },
                SqlStructureColumn {
                    name: "opaque_json".to_owned(),
                    column_type: SqlStructureColumnType::Json,
                },
            ],
            variables: vec![
                SqlStructureVariable {
                    name: "status".to_owned(),
                    variable_type: SqlStructureVariableType::String,
                },
                SqlStructureVariable {
                    name: "priority".to_owned(),
                    variable_type: SqlStructureVariableType::Integer,
                },
            ],
            stable_key: vec!["_ugoite_id".to_owned()],
        }
    }

    #[test]
    fn shared_sql_feature_matrix_matches_structure_protocol_and_aggregate_types() {
        let matrix: serde_json::Value =
            serde_json::from_str(include_str!("../../../shared/sql/ugoite-sql-rules.json"))
                .expect("shared SQL rules must remain valid JSON");
        let editor = &matrix["feature_matrix"]["editor"];
        let relation_limits = editor["constructs"]
            .as_array()
            .expect("editor constructs must be an array")
            .iter()
            .find(|construct| construct["id"] == "from_relation")
            .expect("FROM relation coverage must be declared")["limits"]
            .as_array()
            .expect("FROM relation limits must be an array")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<BTreeSet<_>>();
        assert!(relation_limits.contains(
            &"plain aliases on a single relation are supported and preserved; edited column references remain unqualified"
        ));
        assert!(relation_limits
            .contains(&"joins, derived relations, and table alias column lists are not supported"));
        let declared_operations = editor["operations"]
            .as_array()
            .expect("feature matrix operations must be an array")
            .iter()
            .map(|operation| {
                operation["protocol"]
                    .as_str()
                    .expect("each operation must name its protocol kind")
                    .to_owned()
            })
            .collect::<BTreeSet<_>>();
        let protocol_operations = [
            SqlStructureOperation::SetRelation {
                relation: "Task".to_owned(),
            },
            SqlStructureOperation::SetColumns { columns: vec![] },
            SqlStructureOperation::SetFilters { filters: vec![] },
            SqlStructureOperation::SetSort { order_by: vec![] },
            SqlStructureOperation::SetAggregates { aggregates: vec![] },
            SqlStructureOperation::SetGroupBy { columns: vec![] },
            SqlStructureOperation::SetDistinct { distinct: false },
        ]
        .into_iter()
        .map(|operation| {
            serde_json::to_value(operation).expect("structure operation must serialize")["kind"]
                .as_str()
                .expect("structure operation must have a kind")
                .to_owned()
        })
        .collect::<BTreeSet<_>>();
        assert_eq!(declared_operations, protocol_operations);

        let aggregate_function_names = [
            SqlStructureAggregateFunction::Count,
            SqlStructureAggregateFunction::Sum,
            SqlStructureAggregateFunction::Avg,
            SqlStructureAggregateFunction::Min,
            SqlStructureAggregateFunction::Max,
        ]
        .into_iter()
        .map(|function| {
            serde_json::to_value(function)
                .expect("aggregate function must serialize")
                .as_str()
                .expect("aggregate function must serialize as a name")
                .to_owned()
        })
        .collect::<BTreeSet<_>>();
        let backend_aggregates = matrix["feature_matrix"]["backend"]["functions"]["aggregate"]
            .as_array()
            .expect("backend aggregate function list must be an array")
            .iter()
            .map(|function| {
                function
                    .as_str()
                    .expect("function name must be a string")
                    .to_owned()
            })
            .collect::<BTreeSet<_>>();
        let aggregate_operation = editor["operations"]
            .as_array()
            .expect("feature matrix operations must be an array")
            .iter()
            .find(|operation| operation["action"] == "aggregate")
            .expect("aggregate operation must be present");
        let editor_aggregates = aggregate_operation["functions"]
            .as_array()
            .expect("aggregate operation must list its functions")
            .iter()
            .map(|function| {
                function
                    .as_str()
                    .expect("function name must be a string")
                    .to_owned()
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(backend_aggregates, aggregate_function_names);
        assert_eq!(editor_aggregates, aggregate_function_names);
    }

    #[test]
    fn relation_selection_builds_a_form_query_and_preserves_source_trivia() {
        let target = context();
        let seed = "-- draft\nSELECT 1 -- keep this comment\n";
        let proposal = propose_sql_structure_operation(
            seed,
            &target,
            SqlStructureOperation::SetRelation {
                relation: "Task".to_owned(),
            },
        );

        assert_eq!(proposal.status, SqlStructureStatus::Supported);
        assert!(proposal.requires_confirmation);
        assert_eq!(
            proposal.proposed_sql.as_deref(),
            Some("-- draft\nSELECT * FROM \"Task\" -- keep this comment\n")
        );
        assert_eq!(
            analyze_sql_structure(proposal.proposed_sql.as_deref().unwrap(), &target).status,
            SqlStructureStatus::Supported
        );
    }

    #[test]
    fn relation_selection_replaces_only_a_simple_relation_and_checks_target_schema() {
        let target = context();
        let proposal = propose_sql_structure_operation(
            "SELECT /* keep */ \"field_102\" FROM \"Old\"",
            &target,
            SqlStructureOperation::SetRelation {
                relation: "Task".to_owned(),
            },
        );

        assert_eq!(proposal.status, SqlStructureStatus::Supported);
        assert_eq!(
            proposal.proposed_sql.as_deref(),
            Some("SELECT /* keep */ \"field_102\" FROM \"Task\"")
        );

        let invalid = propose_sql_structure_operation(
            "SELECT missing FROM \"Old\"",
            &target,
            SqlStructureOperation::SetRelation {
                relation: "Task".to_owned(),
            },
        );
        assert_eq!(invalid.status, SqlStructureStatus::Unsupported);
        assert_eq!(invalid.reason, Some(SqlStructureReason::UnknownColumn));
    }

    #[test]
    fn single_relation_aliases_remain_editable_and_survive_structure_changes() {
        let sql = "SELECT \"_ugoite_id\" FROM \"Task\" AS entries ORDER BY \"_ugoite_id\"";
        let analysis = analyze_sql_structure(sql, &context());
        assert_eq!(analysis.status, SqlStructureStatus::Supported);
        assert!(analysis.capabilities.filter.enabled);
        assert!(analysis.capabilities.aggregate.enabled);
        assert!(analysis.capabilities.group_by.enabled);

        let filter = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetFilters {
                filters: vec![SqlStructureFilter {
                    column: "field_101".to_owned(),
                    operator: SqlStructureFilterOperator::Eq,
                    parameter: Some("status".to_owned()),
                }],
            },
        );
        assert_eq!(filter.status, SqlStructureStatus::Supported);
        let filter_sql = filter.proposed_sql.expect("filter proposal");
        assert!(filter_sql.contains("FROM \"Task\" AS entries"));
        assert!(filter_sql.contains("WHERE \"field_101\" = $status"));

        let grouped = propose_sql_structure_operation(
            &filter_sql,
            &context(),
            SqlStructureOperation::SetGroupBy {
                columns: vec!["_ugoite_id".to_owned()],
            },
        );
        assert_eq!(grouped.status, SqlStructureStatus::Supported);
        let grouped_sql = grouped.proposed_sql.expect("grouping proposal");
        assert!(grouped_sql.contains("FROM \"Task\" AS entries"));
        assert!(grouped_sql.contains("GROUP BY \"_ugoite_id\""));

        let aggregate = propose_sql_structure_operation(
            &grouped_sql,
            &context(),
            SqlStructureOperation::SetAggregates {
                aggregates: vec![SqlStructureAggregate {
                    function: SqlStructureAggregateFunction::Count,
                    column: None,
                    distinct: false,
                    alias: None,
                }],
            },
        );
        assert_eq!(aggregate.status, SqlStructureStatus::Supported);
        let aggregate_sql = aggregate.proposed_sql.expect("aggregate proposal");
        assert!(aggregate_sql.contains("FROM \"Task\" AS entries"));
        assert!(aggregate_sql.contains("COUNT(*)"));
        assert!(aggregate_sql.contains("GROUP BY \"_ugoite_id\""));
    }

    #[test]
    fn changing_form_relation_preserves_a_plain_table_alias() {
        let sql = "SELECT \"_ugoite_id\" FROM \"Old\" AS entries ORDER BY \"_ugoite_id\"";
        let proposal = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetRelation {
                relation: "Task".to_owned(),
            },
        );

        assert_eq!(proposal.status, SqlStructureStatus::Supported);
        assert_eq!(
            proposal.proposed_sql.as_deref(),
            Some("SELECT \"_ugoite_id\" FROM \"Task\" AS entries ORDER BY \"_ugoite_id\"")
        );
    }

    #[test]
    fn rejects_single_relation_aliases_that_rename_columns() {
        let result =
            analyze_sql_structure("SELECT * FROM \"Task\" AS entries (entry_id)", &context());
        assert_eq!(result.status, SqlStructureStatus::Unsupported);
        assert_eq!(result.reason, Some(SqlStructureReason::RelationUnsupported));
    }

    #[test]
    fn analyzes_simple_select_filters_and_stable_order() {
        let result = analyze_sql_structure(
            "SELECT * FROM \"Task\" WHERE \"field_101\" = $status ORDER BY \"_ugoite_id\"",
            &context(),
        );

        assert_eq!(result.status, SqlStructureStatus::Supported);
        assert_eq!(result.projection, vec!["*"]);
        assert_eq!(result.filters.len(), 1);
        assert_eq!(result.filters[0].parameter.as_deref(), Some("status"));
        assert_eq!(result.referenced_variables, vec!["status"]);
        assert!(result.stable_pagination);
        assert!(result.capabilities.columns.enabled);
        assert!(result.capabilities.filter.enabled);
        assert!(result.capabilities.sort.enabled);
    }

    #[test]
    fn analyzes_and_proposes_distinct_for_a_relation_free_literal_select() {
        let relation_free_context = SqlStructureContext {
            relation: String::new(),
            columns: Vec::new(),
            variables: Vec::new(),
            stable_key: Vec::new(),
        };
        let sql = "SELECT 1";
        let analysis = analyze_sql_structure(sql, &relation_free_context);

        assert_eq!(analysis.status, SqlStructureStatus::Supported);
        assert_eq!(analysis.projection, vec!["1"]);
        assert!(analysis.capabilities.distinct.enabled);
        assert!(!analysis.capabilities.columns.enabled);
        assert!(!analysis.capabilities.filter.enabled);
        assert!(!analysis.capabilities.sort.enabled);
        assert!(!analysis.capabilities.aggregate.enabled);
        assert!(!analysis.capabilities.group_by.enabled);

        let proposal = propose_sql_structure_operation(
            sql,
            &relation_free_context,
            SqlStructureOperation::SetDistinct { distinct: true },
        );
        assert_eq!(proposal.status, SqlStructureStatus::Supported);
        assert!(proposal.requires_confirmation);
        assert_eq!(proposal.proposed_sql.as_deref(), Some("SELECT DISTINCT 1"));
        assert_eq!(sql, "SELECT 1");
    }

    #[test]
    fn relation_free_analysis_rejects_unmodeled_or_relation_dependent_projections() {
        let relation_free_context = SqlStructureContext {
            relation: String::new(),
            columns: Vec::new(),
            variables: Vec::new(),
            stable_key: Vec::new(),
        };

        for sql in [
            "SELECT *",
            "SELECT $value",
            "SELECT COUNT(*)",
            "SELECT COUNT(*) AS n",
            "SELECT 1 FROM \"Task\"",
        ] {
            let analysis = analyze_sql_structure(sql, &relation_free_context);
            assert_eq!(analysis.status, SqlStructureStatus::Unsupported, "{sql}");
        }

        let mut form_context = context();
        let analysis = analyze_sql_structure("SELECT 1", &form_context);
        assert_eq!(analysis.status, SqlStructureStatus::Unsupported);
        form_context.columns.clear();
        assert_eq!(
            analyze_sql_structure("SELECT 1", &form_context).reason,
            Some(SqlStructureReason::InvalidContext)
        );
    }

    #[test]
    fn proposes_columns_filters_and_sort_without_mutating_source_sql() {
        let sql = "SELECT * FROM \"Task\"";
        let set_columns = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetColumns {
                columns: vec!["field_101".to_owned(), "field_102".to_owned()],
            },
        );
        assert_eq!(sql, "SELECT * FROM \"Task\"");
        assert!(set_columns.requires_confirmation);
        assert!(set_columns
            .proposed_sql
            .as_deref()
            .is_some_and(|query| query.contains("\"field_101\", \"field_102\"")));

        let set_filters = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetFilters {
                filters: vec![SqlStructureFilter {
                    column: "field_102".to_owned(),
                    operator: SqlStructureFilterOperator::GtEq,
                    parameter: Some("priority".to_owned()),
                }],
            },
        );
        let filtered_sql = set_filters.proposed_sql.as_deref().unwrap();
        assert!(filtered_sql.contains("\"field_102\" >= $priority"));
        assert!(!filtered_sql.contains(">= 10"));
        assert!(set_filters.requires_confirmation);

        let set_sort = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetSort {
                order_by: vec![SqlStructureSort {
                    column: "field_102".to_owned(),
                    direction: SqlStructureSortDirection::Desc,
                }],
            },
        );
        let sorted_sql = set_sort.proposed_sql.as_deref().unwrap();
        assert!(sorted_sql.contains("\"field_102\" DESC"));
        assert!(sorted_sql.contains("\"_ugoite_id\" ASC"));
        assert_eq!(set_sort.added_stable_tie_breakers, vec!["_ugoite_id"]);
        assert!(set_sort.requires_confirmation);
    }

    #[test]
    fn set_columns_replaces_only_the_projection_source_span() {
        let columns = vec!["field_101".to_owned(), "field_102".to_owned()];
        let simple_sql = "SELECT * FROM \"G\"";
        let mut simple_context = context();
        simple_context.relation = "G".to_owned();
        let commented_sql = "-- 東京\nSELECT /* 列 */ * FROM \"Task\" /* relation */ WHERE \"field_101\" = $status ORDER BY \"_ugoite_id\" DESC LIMIT 50 /* 終了 */";

        for (sql, context) in [(simple_sql, &simple_context), (commented_sql, &context())] {
            let projection_start = sql.find("* FROM").expect("wildcard projection");
            let projection_end = projection_start + '*'.len_utf8();
            let expected = format!(
                "{}\"field_101\", \"field_102\"{}",
                &sql[..projection_start],
                &sql[projection_end..]
            );
            let proposal = propose_sql_structure_operation(
                sql,
                context,
                SqlStructureOperation::SetColumns {
                    columns: columns.clone(),
                },
            );

            assert_eq!(proposal.status, SqlStructureStatus::Supported, "{sql}");
            let proposed_sql = proposal.proposed_sql.expect("column proposal");
            assert_eq!(proposed_sql, expected, "{sql}");
            let analysis = analyze_sql_structure(&proposed_sql, context);
            assert_eq!(analysis.status, SqlStructureStatus::Supported, "{sql}");
            assert_eq!(analysis.projection, columns, "{sql}");
        }
    }

    #[test]
    fn set_columns_rejects_projection_spans_that_contain_comments() {
        let sql = "SELECT \"field_101\" /* keep */ , \"field_102\" FROM \"Task\"";
        let document = crate::sql_document::parse_sql_document(sql);
        assert_eq!(
            document.status,
            crate::sql_document::SqlDocumentStatus::Parsed
        );
        assert!(document.tokens.iter().any(|token| {
            token.kind == crate::sql_document::SqlDocumentTokenKind::Comment
                && document.source_slice(token.range) == Some("/* keep */")
        }));

        let proposal = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetColumns {
                columns: vec!["field_102".to_owned()],
            },
        );

        assert_eq!(proposal.status, SqlStructureStatus::Unsupported);
        assert_eq!(
            proposal.reason,
            Some(SqlStructureReason::ProjectionUnsupported)
        );
        assert_eq!(proposal.base_sql, sql);
        assert!(proposal.proposed_sql.is_none());
    }

    #[test]
    fn source_range_replacement_fails_closed_for_invalid_utf8_boundaries() {
        assert_eq!(
            replace_sql_source_range("東京", SqlSourceRange { start: 1, end: 2 }, "replacement"),
            None
        );
        assert_eq!(
            replace_sql_source_range("東京", SqlSourceRange { start: 0, end: 99 }, "replacement"),
            None
        );
    }

    #[test]
    fn preserves_explicit_literal_limit_while_proposing_sort() {
        let proposal = propose_sql_structure_operation(
            "SELECT * FROM \"Task\" LIMIT 50",
            &context(),
            SqlStructureOperation::SetSort { order_by: vec![] },
        );

        let proposed = proposal.proposed_sql.expect("valid proposal");
        assert!(proposed.contains("LIMIT 50"));
        assert!(proposed.contains("ORDER BY \"_ugoite_id\" ASC"));
    }

    #[test]
    fn rejects_unsupported_sql_with_accessible_reason_codes() {
        for (sql, reason) in [
            (
                "WITH current AS (SELECT * FROM \"Task\") SELECT * FROM current",
                SqlStructureReason::CommonTableExpressionUnsupported,
            ),
            (
                "SELECT * FROM \"Task\" JOIN \"Task\" AS other ON true",
                SqlStructureReason::JoinUnsupported,
            ),
            (
                "SELECT * FROM \"Task\" WHERE \"field_101\" = $status OR \"field_102\" = $priority",
                SqlStructureReason::FilterUnsupported,
            ),
        ] {
            let result = analyze_sql_structure(sql, &context());
            assert_eq!(result.status, SqlStructureStatus::Unsupported, "{sql}");
            assert_eq!(result.reason, Some(reason), "{sql}");
            assert!(!result.capabilities.columns.enabled, "{sql}");
            assert!(!result.capabilities.filter.enabled, "{sql}");
            assert!(!result.capabilities.sort.enabled, "{sql}");
        }
    }

    #[test]
    fn analyzes_supported_aggregates_grouping_and_distinct_without_losing_aliases() {
        let sql = "SELECT DISTINCT \"field_101\", COUNT(DISTINCT \"field_102\") AS unique_count, COUNT(*) AS row_count, SUM(\"field_102\") AS total, AVG(\"field_102\"), MIN(\"field_102\"), MAX(\"field_102\") FROM \"Task\" GROUP BY \"field_101\"";
        let analysis = analyze_sql_structure(sql, &context());

        assert_eq!(analysis.status, SqlStructureStatus::Supported);
        assert_eq!(analysis.projection, vec!["field_101"]);
        assert_eq!(analysis.group_by, vec!["field_101"]);
        assert!(analysis.distinct);
        assert_eq!(analysis.aggregates.len(), 6);
        assert_eq!(
            analysis.aggregates[0],
            SqlStructureAggregate {
                function: SqlStructureAggregateFunction::Count,
                column: Some("field_102".to_owned()),
                distinct: true,
                alias: Some(SqlStructureAlias {
                    name: "unique_count".to_owned(),
                    quoted: false,
                }),
            }
        );
        assert_eq!(
            analysis.aggregates[1].function,
            SqlStructureAggregateFunction::Count
        );
        assert_eq!(analysis.aggregates[1].column, None);
        assert_eq!(
            analysis
                .aggregates
                .iter()
                .map(|aggregate| aggregate.function)
                .collect::<Vec<_>>(),
            vec![
                SqlStructureAggregateFunction::Count,
                SqlStructureAggregateFunction::Count,
                SqlStructureAggregateFunction::Sum,
                SqlStructureAggregateFunction::Avg,
                SqlStructureAggregateFunction::Min,
                SqlStructureAggregateFunction::Max,
            ]
        );
        assert!(!analysis.capabilities.columns.enabled);
        assert!(!analysis.capabilities.sort.enabled);
        assert!(analysis.capabilities.aggregate.enabled);
        assert!(analysis.capabilities.group_by.enabled);
        assert!(analysis.capabilities.distinct.enabled);
        assert!(!analysis.capabilities.stable_pagination.enabled);
    }

    #[test]
    fn proposes_aggregate_group_by_and_select_distinct_operations_as_ast_rewrites() {
        let sql = "SELECT \"field_101\" FROM \"Task\"";
        let grouped = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetGroupBy {
                columns: vec!["field_101".to_owned()],
            },
        );
        assert_eq!(grouped.status, SqlStructureStatus::Supported);
        assert!(grouped.requires_confirmation);
        let grouped_sql = grouped.proposed_sql.expect("grouping proposal");
        assert!(grouped_sql.contains("GROUP BY \"field_101\""));

        let aggregate = propose_sql_structure_operation(
            &grouped_sql,
            &context(),
            SqlStructureOperation::SetAggregates {
                aggregates: vec![SqlStructureAggregate {
                    function: SqlStructureAggregateFunction::Sum,
                    column: Some("field_102".to_owned()),
                    distinct: true,
                    alias: Some(SqlStructureAlias {
                        name: "total".to_owned(),
                        quoted: true,
                    }),
                }],
            },
        );
        assert_eq!(aggregate.status, SqlStructureStatus::Supported);
        assert!(aggregate.requires_confirmation);
        let aggregate_sql = aggregate.proposed_sql.expect("aggregate proposal");
        assert!(aggregate_sql.contains("SUM(DISTINCT \"field_102\") AS \"total\""));
        assert!(aggregate_sql.contains("GROUP BY \"field_101\""));

        let distinct = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetDistinct { distinct: true },
        );
        assert_eq!(distinct.status, SqlStructureStatus::Supported);
        assert!(distinct
            .proposed_sql
            .as_deref()
            .is_some_and(|sql| sql.starts_with("SELECT DISTINCT")));
        assert!(distinct.requires_confirmation);
        assert_eq!(sql, "SELECT \"field_101\" FROM \"Task\"");
    }

    #[test]
    fn aggregates_replace_wildcard_projection_and_retain_existing_alias_slots() {
        let count = propose_sql_structure_operation(
            "SELECT * FROM \"Task\"",
            &context(),
            SqlStructureOperation::SetAggregates {
                aggregates: vec![SqlStructureAggregate {
                    function: SqlStructureAggregateFunction::Count,
                    column: None,
                    distinct: false,
                    alias: Some(SqlStructureAlias {
                        name: "total".to_owned(),
                        quoted: false,
                    }),
                }],
            },
        );
        let sql = count.proposed_sql.expect("COUNT proposal");
        assert!(sql.contains("COUNT(*) AS total"));
        assert!(!sql.contains("SELECT *,"));

        let changed = propose_sql_structure_operation(
            "SELECT COUNT(*) AS total, \"field_101\" FROM \"Task\" GROUP BY \"field_101\"",
            &context(),
            SqlStructureOperation::SetAggregates {
                aggregates: vec![SqlStructureAggregate {
                    function: SqlStructureAggregateFunction::Max,
                    column: Some("field_102".to_owned()),
                    distinct: false,
                    alias: None,
                }],
            },
        );
        let sql = changed.proposed_sql.expect("MAX proposal");
        assert!(sql.contains("MAX(\"field_102\") AS total"));
        assert!(sql.contains("\"field_101\""));
    }

    #[test]
    fn aggregate_operation_serializes_every_allowlisted_function() {
        let proposal = propose_sql_structure_operation(
            "SELECT * FROM \"Task\"",
            &context(),
            SqlStructureOperation::SetAggregates {
                aggregates: vec![
                    SqlStructureAggregate {
                        function: SqlStructureAggregateFunction::Count,
                        column: None,
                        distinct: false,
                        alias: None,
                    },
                    SqlStructureAggregate {
                        function: SqlStructureAggregateFunction::Count,
                        column: Some("field_101".to_owned()),
                        distinct: true,
                        alias: None,
                    },
                    SqlStructureAggregate {
                        function: SqlStructureAggregateFunction::Sum,
                        column: Some("field_102".to_owned()),
                        distinct: false,
                        alias: None,
                    },
                    SqlStructureAggregate {
                        function: SqlStructureAggregateFunction::Avg,
                        column: Some("field_102".to_owned()),
                        distinct: false,
                        alias: None,
                    },
                    SqlStructureAggregate {
                        function: SqlStructureAggregateFunction::Min,
                        column: Some("field_102".to_owned()),
                        distinct: false,
                        alias: None,
                    },
                    SqlStructureAggregate {
                        function: SqlStructureAggregateFunction::Max,
                        column: Some("field_102".to_owned()),
                        distinct: false,
                        alias: None,
                    },
                ],
            },
        );
        assert_eq!(proposal.status, SqlStructureStatus::Supported);
        let sql = proposal.proposed_sql.expect("aggregate proposal");
        for expected in [
            "COUNT(*)",
            "COUNT(DISTINCT \"field_101\")",
            "SUM(\"field_102\")",
            "AVG(\"field_102\")",
            "MIN(\"field_102\")",
            "MAX(\"field_102\")",
        ] {
            assert!(sql.contains(expected), "{expected} missing from {sql}");
        }
    }

    #[test]
    fn aggregate_and_grouping_rewrites_fail_closed_on_unsafe_shapes() {
        for (sql, reason) in [
            (
                "SELECT SUM(\"field_101\") FROM \"Task\"",
                SqlStructureReason::AggregateTypeUnsupported,
            ),
            (
                "SELECT COUNT(DISTINCT \"opaque_json\") FROM \"Task\"",
                SqlStructureReason::AggregateTypeUnsupported,
            ),
            (
                "SELECT COUNT(DISTINCT *) FROM \"Task\"",
                SqlStructureReason::AggregateUnsupported,
            ),
            (
                "SELECT COUNT(*) FILTER (WHERE \"field_102\" > 0) FROM \"Task\"",
                SqlStructureReason::AggregateUnsupported,
            ),
            (
                "SELECT \"field_101\", COUNT(*) FROM \"Task\"",
                SqlStructureReason::GroupByUnsupported,
            ),
            (
                "SELECT \"field_101\" FROM \"Task\" GROUP BY ROLLUP (\"field_101\")",
                SqlStructureReason::GroupByUnsupported,
            ),
            (
                "SELECT DISTINCT ON (\"field_101\") \"field_101\" FROM \"Task\"",
                SqlStructureReason::DistinctUnsupported,
            ),
            (
                "SELECT DISTINCT \"field_101\" FROM \"Task\" ORDER BY \"_ugoite_id\"",
                SqlStructureReason::DistinctUnsupported,
            ),
            (
                "SELECT \"field_101\", COUNT(*) FROM \"Task\" GROUP BY \"field_101\" ORDER BY \"field_102\"",
                SqlStructureReason::SortUnsupported,
            ),
            (
                "SELECT SUM(\"field_102\") FROM \"Task\" ORDER BY \"field_101\"",
                SqlStructureReason::SortUnsupported,
            ),
        ] {
            let result = analyze_sql_structure(sql, &context());
            assert_eq!(result.status, SqlStructureStatus::Unsupported, "{sql}");
            assert_eq!(result.reason, Some(reason), "{sql}");
            assert!(!result.capabilities.aggregate.enabled, "{sql}");
            assert!(!result.capabilities.group_by.enabled, "{sql}");
            assert!(!result.capabilities.distinct.enabled, "{sql}");
        }

        let invalid_group = propose_sql_structure_operation(
            "SELECT \"field_101\", SUM(\"field_102\") FROM \"Task\" GROUP BY \"field_101\"",
            &context(),
            SqlStructureOperation::SetGroupBy {
                columns: vec!["field_102".to_owned()],
            },
        );
        assert_eq!(
            invalid_group.reason,
            Some(SqlStructureReason::GroupByUnsupported)
        );
        assert!(invalid_group.proposed_sql.is_none());

        let unsupported_sql = "SELECT COUNT(DISTINCT *) FROM \"Task\"";
        let rejected = propose_sql_structure_operation(
            unsupported_sql,
            &context(),
            SqlStructureOperation::SetDistinct { distinct: true },
        );
        assert_eq!(rejected.status, SqlStructureStatus::Unsupported);
        assert_eq!(rejected.base_sql, unsupported_sql);
        assert!(rejected.proposed_sql.is_none());

        let incompatible_distinct = "SELECT \"field_101\" FROM \"Task\" ORDER BY \"_ugoite_id\"";
        let rejected = propose_sql_structure_operation(
            incompatible_distinct,
            &context(),
            SqlStructureOperation::SetDistinct { distinct: true },
        );
        assert_eq!(rejected.status, SqlStructureStatus::Unsupported);
        assert_eq!(
            rejected.reason,
            Some(SqlStructureReason::DistinctUnsupported)
        );
        assert_eq!(rejected.base_sql, incompatible_distinct);
        assert!(rejected.proposed_sql.is_none());

        let incompatible_grouping = "SELECT \"field_101\" FROM \"Task\" ORDER BY \"field_102\"";
        let rejected = propose_sql_structure_operation(
            incompatible_grouping,
            &context(),
            SqlStructureOperation::SetGroupBy {
                columns: vec!["field_101".to_owned()],
            },
        );
        assert_eq!(rejected.status, SqlStructureStatus::Unsupported);
        assert_eq!(rejected.reason, Some(SqlStructureReason::SortUnsupported));
        assert_eq!(rejected.base_sql, incompatible_grouping);
        assert!(rejected.proposed_sql.is_none());

        let incompatible_aggregate = "SELECT * FROM \"Task\" ORDER BY \"field_101\"";
        let rejected = propose_sql_structure_operation(
            incompatible_aggregate,
            &context(),
            SqlStructureOperation::SetAggregates {
                aggregates: vec![SqlStructureAggregate {
                    function: SqlStructureAggregateFunction::Count,
                    column: None,
                    distinct: false,
                    alias: None,
                }],
            },
        );
        assert_eq!(rejected.status, SqlStructureStatus::Unsupported);
        assert_eq!(rejected.reason, Some(SqlStructureReason::SortUnsupported));
        assert_eq!(rejected.base_sql, incompatible_aggregate);
        assert!(rejected.proposed_sql.is_none());
    }

    #[test]
    fn rejects_unknown_columns_and_mismatched_typed_parameters() {
        let unknown_column = propose_sql_structure_operation(
            "SELECT * FROM \"Task\"",
            &context(),
            SqlStructureOperation::SetColumns {
                columns: vec!["unknown".to_owned()],
            },
        );
        assert_eq!(
            unknown_column.reason,
            Some(SqlStructureReason::UnknownColumn)
        );
        assert!(unknown_column.proposed_sql.is_none());

        let mismatch = propose_sql_structure_operation(
            "SELECT * FROM \"Task\"",
            &context(),
            SqlStructureOperation::SetFilters {
                filters: vec![SqlStructureFilter {
                    column: "field_102".to_owned(),
                    operator: SqlStructureFilterOperator::Eq,
                    parameter: Some("status".to_owned()),
                }],
            },
        );
        assert_eq!(
            mismatch.reason,
            Some(SqlStructureReason::VariableTypeMismatch)
        );
        assert!(mismatch.proposed_sql.is_none());

        let unknown_sort_column = analyze_sql_structure(
            "SELECT * FROM \"Task\" ORDER BY \"not_in_form\"",
            &context(),
        );
        assert_eq!(
            unknown_sort_column.reason,
            Some(SqlStructureReason::UnknownColumn)
        );
    }

    #[test]
    fn reports_when_current_order_needs_a_stable_tie_breaker() {
        let result = analyze_sql_structure(
            "SELECT * FROM \"Task\" ORDER BY \"field_102\" DESC",
            &context(),
        );

        assert_eq!(result.status, SqlStructureStatus::Supported);
        assert!(!result.stable_pagination);
        assert_eq!(
            result.capabilities.stable_pagination.reason,
            Some(SqlStructureReason::StableOrderRequired)
        );
        assert!(result.capabilities.sort.enabled);
    }

    #[test]
    fn sort_requires_a_stable_key_and_proposals_always_keep_the_base_sql() {
        let mut without_key = context();
        without_key.stable_key.clear();
        let sql = "SELECT * FROM \"Task\"";
        let proposal = propose_sql_structure_operation(
            sql,
            &without_key,
            SqlStructureOperation::SetSort {
                order_by: vec![SqlStructureSort {
                    column: "field_101".to_owned(),
                    direction: SqlStructureSortDirection::Asc,
                }],
            },
        );
        assert_eq!(proposal.base_sql, sql);
        assert_eq!(
            proposal.reason,
            Some(SqlStructureReason::StableKeyUnavailable)
        );
        assert!(proposal.proposed_sql.is_none());
    }

    #[test]
    fn rejects_sorting_opaque_columns_and_disables_sort_for_unorderable_stable_keys() {
        let mut with_json_column = context();
        with_json_column.columns.push(SqlStructureColumn {
            name: "payload".to_owned(),
            column_type: SqlStructureColumnType::Json,
        });
        let opaque_order = analyze_sql_structure(
            "SELECT * FROM \"Task\" ORDER BY \"payload\"",
            &with_json_column,
        );
        assert_eq!(opaque_order.status, SqlStructureStatus::Unsupported);
        assert_eq!(
            opaque_order.reason,
            Some(SqlStructureReason::SortUnsupported)
        );

        let mut unorderable_stable_key = context();
        unorderable_stable_key.columns[0].column_type = SqlStructureColumnType::Json;
        let sql = "SELECT * FROM \"Task\" ORDER BY \"field_102\"";
        let analysis = analyze_sql_structure(sql, &unorderable_stable_key);
        assert_eq!(analysis.status, SqlStructureStatus::Supported);
        assert!(!analysis.stable_pagination);
        assert!(!analysis.capabilities.sort.enabled);
        assert_eq!(
            analysis.capabilities.sort.reason,
            Some(SqlStructureReason::StableKeyUnorderable)
        );
        assert_eq!(
            analysis.capabilities.stable_pagination.reason,
            Some(SqlStructureReason::StableKeyUnorderable)
        );

        let proposal = propose_sql_structure_operation(
            sql,
            &unorderable_stable_key,
            SqlStructureOperation::SetSort {
                order_by: vec![SqlStructureSort {
                    column: "field_102".to_owned(),
                    direction: SqlStructureSortDirection::Desc,
                }],
            },
        );
        assert_eq!(
            proposal.reason,
            Some(SqlStructureReason::StableKeyUnorderable)
        );
        assert!(proposal.proposed_sql.is_none());
    }
}
