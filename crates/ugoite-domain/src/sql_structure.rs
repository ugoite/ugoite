//! Parser-backed capabilities for safe SQL Workspace structure operations.
//!
//! This module describes a deliberately small SQL subset that can be
//! transformed without a second SQL parser in the browser. It does not
//! authorize, plan, or execute queries. The server remains authoritative for
//! read-only SQL validation, Form access, parameter types, and pagination.

use crate::sql_functions::{user_sql_aggregate, SqlAggregateDistinct, SqlAggregateWildcard};
use serde::{Deserialize, Serialize};
use sqlparser::ast::{
    BinaryOperator, DataType, Distinct, DuplicateTreatment, Expr, Function, FunctionArg,
    FunctionArgExpr, FunctionArgumentList, FunctionArguments, GroupByExpr, Ident, JoinConstraint,
    JoinOperator, LimitClause, ObjectName, ObjectNamePart, OrderBy, OrderByExpr, OrderByKind,
    OrderByOptions, OrderBySort, Query, Select, SelectFlavor, SelectItem, SetExpr, Spanned,
    Statement, TableFactor, TableWithJoins, TypedString, UnaryOperator, Value, ValueWithSpan,
};
use sqlparser::tokenizer::Span;
use std::collections::BTreeSet;

use crate::sql_document::{
    apply_sql_document_node_edit_transaction, has_comment_token_in_range,
    parse_generic_sql_statements, parse_sql_document, source_range_from_span, SqlDocumentNodeEdit,
    SqlDocumentNodeEditTransaction, SqlDocumentNodeKind, SqlDocumentStatus, SqlDocumentTokenKind,
    SqlSourceRange, SQL_DOCUMENT_CONTRACT_VERSION,
};

/// The parser/capability contract version exposed through the WASM adapter.
pub const SQL_STRUCTURE_CONTRACT_VERSION: u32 = 7;

/// Keep SQL parsing within the same bound as the stateless SQL request.
pub const MAX_SQL_STRUCTURE_QUERY_BYTES: usize = 256 * 1024;
const MAX_FILTER_EXPRESSION_DEPTH: usize = 32;
const MAX_FILTER_EXPRESSION_NODES: usize = 128;
// sqlparser represents a flat N-leaf AND/OR chain as N predicates plus N-1
// binary nodes. The public limit counts the normalized editor tree instead.
const MAX_FILTER_EXPRESSION_PARSER_DEPTH: usize = MAX_FILTER_EXPRESSION_NODES * 2;
const MAX_FILTER_EXPRESSION_PARSER_NODES: usize = MAX_FILTER_EXPRESSION_NODES * 2;

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
    SetJoin {
        #[serde(default)]
        join_kind: SqlStructureJoinKind,
        left_column: String,
        right_column: String,
    },
    SetColumns {
        columns: Vec<String>,
        /// Optional aliases aligned with `columns`. An omitted/empty list
        /// keeps aliases on retained columns; a non-empty list replaces the
        /// alias state for every selected projection.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        aliases: Vec<Option<SqlStructureAlias>>,
    },
    SetLiteralProjection {
        values: Vec<SqlStructureLiteral>,
    },
    SetProjectionSlots {
        slots: Vec<SqlStructureProjectionSlotEdit>,
    },
    SetFilters {
        filters: Vec<SqlStructureFilter>,
    },
    SetFilterExpression {
        #[serde(deserialize_with = "deserialize_required_nullable")]
        expression: Option<SqlStructureFilterExpression>,
    },
    SetHaving {
        #[serde(deserialize_with = "deserialize_required_nullable")]
        expression: Option<SqlStructureHavingExpression>,
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
    SetRowLimit {
        #[serde(deserialize_with = "deserialize_required_nullable")]
        limit: Option<String>,
        #[serde(deserialize_with = "deserialize_required_nullable")]
        offset: Option<String>,
    },
}

/// Preserve explicit `null` as the request to clear a filter tree while
/// rejecting a missing `expression` field. With a plain `Option<T>`, Serde
/// otherwise treats an omitted field as `None`, which could clear WHERE.
fn deserialize_required_nullable<'de, D, T>(
    deserializer: D,
) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

/// A relation-free scalar literal that the SQL structure editor can rewrite.
/// Integer values use decimal strings so the WASM protocol preserves all i64
/// digits without passing through JavaScript floating point.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SqlStructureLiteral {
    Integer(String),
    String(String),
    Boolean(bool),
    Null,
}

/// An aggregate projection that the bounded SQL structure parser can round-trip.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlStructureAggregate {
    pub function: SqlStructureAggregateFunction,
    /// `None` represents `COUNT(*)` and remains the original wire representation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
    /// Additional identifiers after `column` for variadic COUNT. It is absent
    /// for the original single-column and wildcard forms so their wire contract
    /// stays stable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<String>,
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
    Mean,
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

/// Alias state for a simple Form-column projection, in SELECT-list order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlStructureProjectionAlias {
    pub column: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<SqlStructureAlias>,
}

/// Requested projection state. `id` refers to an existing slot in the exact
/// base SQL snapshot; omitting it creates a new slot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlStructureProjectionSlotEdit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub column: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<SqlStructureAlias>,
}

/// A typed predicate that references a declared SQL variable by name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlStructureFilter {
    pub column: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qualifier: Option<SqlStructureAlias>,
    pub operator: SqlStructureFilterOperator,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_parameter: Option<String>,
}

/// A typed column reference used by GUI-constructed SQL predicates.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlStructureColumnReference {
    pub column: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qualifier: Option<SqlStructureAlias>,
}

/// A bounded boolean expression tree used by the shared SQL filter editor.
/// Predicate leaves keep the same typed validation contract as `SetFilters`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SqlStructureFilterExpression {
    And {
        expressions: Vec<SqlStructureFilterExpression>,
    },
    Or {
        expressions: Vec<SqlStructureFilterExpression>,
    },
    Not {
        expression: Box<SqlStructureFilterExpression>,
    },
    Predicate {
        column: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        qualifier: Option<SqlStructureAlias>,
        operator: SqlStructureFilterOperator,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameter: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        second_parameter: Option<String>,
    },
    ColumnComparison {
        left: SqlStructureColumnReference,
        operator: SqlStructureFilterOperator,
        right: SqlStructureColumnReference,
    },
}

/// A bounded boolean tree for predicates over an allowlisted aggregate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SqlStructureHavingExpression {
    And {
        expressions: Vec<SqlStructureHavingExpression>,
    },
    Or {
        expressions: Vec<SqlStructureHavingExpression>,
    },
    Not {
        expression: Box<SqlStructureHavingExpression>,
    },
    Predicate {
        aggregate: SqlStructureAggregate,
        operator: SqlStructureHavingOperator,
        value: SqlStructureHavingValue,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlStructureHavingOperator {
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
}

/// Typed non-SQL-fragment values accepted on the right side of HAVING
/// aggregate comparisons.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SqlStructureHavingValue {
    Integer(String),
    Numeric(String),
    String(String),
    Date(String),
    Timestamp(String),
    Boolean(bool),
    Variable(String),
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
    In,
    NotIn,
    Between,
    NotBetween,
    Like,
    NotLike,
    Ilike,
    NotIlike,
    IsNull,
    IsNotNull,
}

/// Literal row bounds from a query-level LIMIT/OFFSET clause.
///
/// Decimal strings preserve the full SQL integer spelling without passing
/// through JavaScript number precision. `None` means that term is absent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlStructureRowLimit {
    pub limit: Option<String>,
    pub offset: Option<String>,
}

/// One column and direction in an explicit ORDER BY clause.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlStructureSort {
    pub column: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qualifier: Option<SqlStructureAlias>,
    /// Set when this term resolves to a simple SELECT projection alias. The
    /// column remains the projected source column for type checks and UI
    /// capabilities; SQL rewrites preserve the alias as the ORDER BY target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection_alias: Option<SqlStructureAlias>,
    pub direction: SqlStructureSortDirection,
    /// Explicit NULL placement. `None` leaves ordering to the SQL dialect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nulls_first: Option<bool>,
}

/// The bounded relation shape recognized by the structure editor. `relation`
/// is a self-join of the selected Form; the runtime SQL planner remains the
/// authority for whether the query can execute.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlStructureJoin {
    pub relation: String,
    #[serde(default)]
    pub join_kind: SqlStructureJoinKind,
    pub left_alias: SqlStructureAlias,
    pub right_alias: SqlStructureAlias,
    pub left_column: String,
    pub right_column: String,
}

/// The closed set of join operators editable by the bounded self-join editor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlStructureJoinKind {
    #[default]
    Inner,
    Left,
    Right,
    Full,
}

impl SqlStructureJoinKind {
    const fn keyword(self) -> &'static str {
        match self {
            Self::Inner => "INNER",
            Self::Left => "LEFT",
            Self::Right => "RIGHT",
            Self::Full => "FULL",
        }
    }
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
    HavingUnsupported,
    GroupByUnsupported,
    DistinctUnsupported,
    RowLimitUnsupported,
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
    pub join: SqlStructureActionCapability,
    pub columns: SqlStructureActionCapability,
    /// Flat-conjunction capability retained for older adapters.
    pub filter: SqlStructureActionCapability,
    /// Boolean-tree capability exposed for the shared Workspace protocol.
    pub filter_expression: SqlStructureActionCapability,
    pub sort: SqlStructureActionCapability,
    pub aggregate: SqlStructureActionCapability,
    pub group_by: SqlStructureActionCapability,
    pub having: SqlStructureActionCapability,
    pub distinct: SqlStructureActionCapability,
    pub row_limit: SqlStructureActionCapability,
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
    pub literal_projection: Vec<SqlStructureLiteral>,
    #[serde(default)]
    pub projection_aliases: Vec<SqlStructureProjectionAlias>,
    #[serde(default)]
    pub filters: Vec<SqlStructureFilter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter_expression: Option<SqlStructureFilterExpression>,
    #[serde(default)]
    pub aggregates: Vec<SqlStructureAggregate>,
    #[serde(default)]
    pub group_by: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub having_expression: Option<SqlStructureHavingExpression>,
    pub distinct: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_limit: Option<SqlStructureRowLimit>,
    #[serde(default)]
    pub order_by: Vec<SqlStructureSort>,
    #[serde(default)]
    pub joins: Vec<SqlStructureJoin>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_node_edit: Option<SqlDocumentNodeEditTransaction>,
    pub requires_confirmation: bool,
    #[serde(default)]
    pub added_stable_tie_breakers: Vec<String>,
}

struct ParsedSqlStructure {
    query: Query,
    projection: Vec<String>,
    literal_projection: Vec<SqlStructureLiteral>,
    projection_aliases: Vec<SqlStructureProjectionAlias>,
    filters: Vec<SqlStructureFilter>,
    filter_expression: Option<SqlStructureFilterExpression>,
    aggregates: Vec<SqlStructureAggregate>,
    group_by: Vec<String>,
    having_expression: Option<SqlStructureHavingExpression>,
    distinct: bool,
    row_limit: Option<SqlStructureRowLimit>,
    order_by: Vec<SqlStructureSort>,
    joins: Vec<SqlStructureJoin>,
    can_add_join: bool,
    referenced_variables: Vec<String>,
    stable_pagination: bool,
}

/// Analyze SQL using the same GenericDialect as DataFusion's SQL parser.
///
/// Unsupported syntax produces disabled capabilities with stable reason codes;
/// the caller can leave the direct editor active without guessing at a rewrite.
pub fn analyze_sql_structure(sql: &str, context: &SqlStructureContext) -> SqlStructureAnalysis {
    match parse_supported_sql(sql, context) {
        Ok(parsed) => supported_analysis(parsed, context),
        Err(reason) => partial_query_clause_analysis(sql, context, reason)
            .unwrap_or_else(|| rejected_analysis(reason)),
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
    if let SqlStructureOperation::SetJoin {
        join_kind,
        left_column,
        right_column,
    } = &operation
    {
        return propose_sql_join_edit(sql, context, *join_kind, left_column, right_column);
    }
    if let SqlStructureOperation::SetRowLimit { limit, offset } = &operation {
        return propose_sql_row_limit_edit(sql, context, limit.as_deref(), offset.as_deref());
    }

    let edit_target = structure_edit_target(&operation);
    let base_sql = sql.to_owned();
    let mut parsed = match parse_supported_sql(sql, context) {
        Ok(parsed) => parsed,
        Err(reason) => return rejected_proposal(base_sql, reason),
    };

    let mut added_stable_tie_breakers = Vec::new();
    let original_query = parsed.query.clone();
    let proposed_sql = match operation {
        SqlStructureOperation::SetFilterExpression { expression } => {
            match rewrite_filter_expression(
                &base_sql,
                &original_query,
                context,
                &parsed.joins,
                expression,
            ) {
                Ok(sql) => sql,
                Err(reason) => return rejected_proposal(base_sql, reason),
            }
        }
        SqlStructureOperation::SetHaving { expression } => {
            if let Err(reason) = set_having(&mut parsed.query, context, expression) {
                return rejected_proposal(base_sql, reason);
            }
            match rewrite_having_clause(&base_sql, &original_query, &parsed.query) {
                Ok(sql) => sql,
                Err(reason) => return rejected_proposal(base_sql, reason),
            }
        }
        SqlStructureOperation::SetRelation { .. } => {
            unreachable!("SetRelation is handled by the Rust relation selection proposal")
        }
        SqlStructureOperation::SetJoin { .. } => {
            unreachable!("JOIN edits are handled by the source-preserving proposal")
        }
        SqlStructureOperation::SetColumns { columns, aliases } => {
            if !projection_sources_are_unique(&parsed.projection) {
                return rejected_proposal(base_sql, SqlStructureReason::ProjectionUnsupported);
            }
            match rewrite_projection(
                &base_sql,
                &parsed.query,
                context,
                columns,
                aliases,
                &parsed.projection_aliases,
            ) {
                Ok(sql) => sql,
                Err(reason) => return rejected_proposal(base_sql, reason),
            }
        }
        SqlStructureOperation::SetLiteralProjection { values } => {
            match rewrite_literal_projection(&base_sql, &parsed.query, context, values) {
                Ok(sql) => sql,
                Err(reason) => return rejected_proposal(base_sql, reason),
            }
        }
        SqlStructureOperation::SetProjectionSlots { slots } => match rewrite_projection_slots(
            &base_sql,
            &parsed.query,
            context,
            slots,
            &parsed.projection_aliases,
        ) {
            Ok(sql) => sql,
            Err(reason) => return rejected_proposal(base_sql, reason),
        },
        SqlStructureOperation::SetFilters { filters } => {
            if let Err(reason) = set_filters(&mut parsed.query, context, &parsed.joins, filters) {
                return rejected_proposal(base_sql, reason);
            }
            let replacement = match select_ref(&parsed.query) {
                Ok(select) => select.selection.as_ref().map(ToString::to_string),
                Err(reason) => return rejected_proposal(base_sql, reason),
            };
            match rewrite_where_clause(&base_sql, &original_query, replacement.as_deref()) {
                Ok(sql) => sql,
                Err(reason) => return rejected_proposal(base_sql, reason),
            }
        }
        SqlStructureOperation::SetSort { order_by } => {
            if let Err(reason) = set_sort(
                &mut parsed.query,
                context,
                &parsed.joins,
                order_by,
                &mut added_stable_tie_breakers,
            ) {
                return rejected_proposal(base_sql, reason);
            }
            match rewrite_order_by_clause(&base_sql, &original_query, &parsed.query) {
                Ok(sql) => sql,
                Err(reason) => return rejected_proposal(base_sql, reason),
            }
        }
        SqlStructureOperation::SetAggregates { aggregates } => {
            if let Err(reason) = set_aggregates(&mut parsed.query, context, aggregates) {
                return rejected_proposal(base_sql, reason);
            }
            match rewrite_projection_from_ast(&base_sql, &original_query, &parsed.query) {
                Ok(sql) => sql,
                Err(reason) => return rejected_proposal(base_sql, reason),
            }
        }
        SqlStructureOperation::SetGroupBy { columns } => {
            if let Err(reason) = set_group_by(&mut parsed.query, context, columns) {
                return rejected_proposal(base_sql, reason);
            }
            match rewrite_group_by_clause(&base_sql, &original_query, &parsed.query) {
                Ok(sql) => sql,
                Err(reason) => return rejected_proposal(base_sql, reason),
            }
        }
        SqlStructureOperation::SetDistinct { distinct } => {
            if select_ref(&original_query)
                .ok()
                .and_then(|select| select.distinct.as_ref())
                .is_some_and(|distinct| matches!(distinct, Distinct::On(_)))
            {
                return rejected_proposal(base_sql, SqlStructureReason::DistinctUnsupported);
            }
            if let Err(reason) = set_distinct(&mut parsed.query, distinct) {
                return rejected_proposal(base_sql, reason);
            }
            match rewrite_distinct(&base_sql, &original_query, distinct) {
                Ok(sql) => sql,
                Err(reason) => return rejected_proposal(base_sql, reason),
            }
        }
        SqlStructureOperation::SetRowLimit { limit, offset } => {
            let replacement = match render_row_limit(limit.as_deref(), offset.as_deref()) {
                Ok(replacement) => replacement,
                Err(reason) => return rejected_proposal(base_sql, reason),
            };
            let select = match select_ref(&original_query) {
                Ok(select) => select,
                Err(reason) => return rejected_proposal(base_sql, reason),
            };
            match rewrite_query_clause(
                &base_sql,
                &original_query,
                select,
                SqlDocumentNodeKind::LimitClause,
                false,
                replacement.as_deref(),
                &[],
                SqlStructureReason::RowLimitUnsupported,
            ) {
                Ok(sql) => sql,
                Err(reason) => return rejected_proposal(base_sql, reason),
            }
        }
    };
    if let Err(reason) = parse_supported_sql(&proposed_sql, context) {
        return rejected_proposal(base_sql, reason);
    }

    let requires_confirmation = proposed_sql != base_sql;
    let document_node_edit = if requires_confirmation {
        match derive_structure_node_edit(&base_sql, &proposed_sql, edit_target) {
            Some(edit) => Some(edit),
            None => return rejected_proposal(base_sql, SqlStructureReason::RoundTripFailed),
        }
    } else {
        None
    };
    SqlStructureProposal {
        contract_version: SQL_STRUCTURE_CONTRACT_VERSION,
        status: SqlStructureStatus::Supported,
        reason: None,
        base_sql,
        proposed_sql: Some(proposed_sql),
        document_node_edit,
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

    let mut statements = match parse_generic_sql_statements(sql) {
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
        if select.from.len() != 1 {
            return rejected_proposal(base_sql, SqlStructureReason::RelationUnsupported);
        }
        let table = &select.from[0];
        if table.joins.is_empty() {
            let Some(_) = simple_table_name(&table.relation) else {
                return rejected_proposal(base_sql, SqlStructureReason::RelationUnsupported);
            };
            let TableFactor::Table { name, .. } = &table.relation else {
                return rejected_proposal(base_sql, SqlStructureReason::RelationUnsupported);
            };
            let Some(relation_range) = source_range_from_span(sql, name.span()) else {
                return rejected_proposal(base_sql, SqlStructureReason::RelationUnsupported);
            };
            if has_comment_token_in_range(sql, relation_range) != Some(false) {
                return rejected_proposal(base_sql, SqlStructureReason::RelationUnsupported);
            }
            let quoted_relation = quote_ident(relation);
            let Some(rewritten) = replace_sql_source_range(sql, relation_range, &quoted_relation)
            else {
                return rejected_proposal(base_sql, SqlStructureReason::RelationUnsupported);
            };
            rewritten
        } else {
            let Some(rewritten) = rewrite_join_relation(sql, table, relation, context) else {
                return rejected_proposal(base_sql, SqlStructureReason::RelationUnsupported);
            };
            rewritten
        }
    };

    if let Err(reason) = parse_supported_sql(&proposed_sql, context) {
        return rejected_proposal(base_sql, reason);
    }
    let requires_confirmation = proposed_sql != sql;
    let document_node_edit = if requires_confirmation {
        match derive_structure_node_edit(&base_sql, &proposed_sql, StructureEditTarget::Relation) {
            Some(edit) => Some(edit),
            None => return rejected_proposal(base_sql, SqlStructureReason::RoundTripFailed),
        }
    } else {
        None
    };
    SqlStructureProposal {
        contract_version: SQL_STRUCTURE_CONTRACT_VERSION,
        status: SqlStructureStatus::Supported,
        reason: None,
        base_sql,
        proposed_sql: Some(proposed_sql),
        document_node_edit,
        requires_confirmation,
        added_stable_tie_breakers: Vec::new(),
    }
}

fn propose_sql_join_edit(
    sql: &str,
    context: &SqlStructureContext,
    join_kind: SqlStructureJoinKind,
    left_column: &str,
    right_column: &str,
) -> SqlStructureProposal {
    let base_sql = sql.to_owned();
    let parsed = match parse_supported_sql(sql, context) {
        Ok(parsed) => parsed,
        Err(reason) => return rejected_proposal(base_sql, reason),
    };
    if let Err(reason) = validate_join_columns(context, left_column, right_column) {
        return rejected_proposal(base_sql, reason);
    }
    let SetExpr::Select(select) = parsed.query.body.as_ref() else {
        return rejected_proposal(base_sql, SqlStructureReason::SetOperationUnsupported);
    };
    let Some(table) = select.from.first() else {
        return rejected_proposal(base_sql, SqlStructureReason::RelationUnsupported);
    };

    let proposed_sql = if let Some(join) = parsed.joins.first() {
        let Some(ast_join) = table.joins.first() else {
            return rejected_proposal(base_sql, SqlStructureReason::JoinUnsupported);
        };
        let (JoinOperator::Join(JoinConstraint::On(expression))
        | JoinOperator::Inner(JoinConstraint::On(expression))
        | JoinOperator::Left(JoinConstraint::On(expression))
        | JoinOperator::LeftOuter(JoinConstraint::On(expression))
        | JoinOperator::Right(JoinConstraint::On(expression))
        | JoinOperator::RightOuter(JoinConstraint::On(expression))
        | JoinOperator::FullOuter(JoinConstraint::On(expression))) = &ast_join.join_operator
        else {
            return rejected_proposal(base_sql, SqlStructureReason::JoinUnsupported);
        };
        let Some(source_edits) =
            join_kind_source_edits(sql, table, ast_join, join.join_kind, join_kind)
        else {
            return rejected_proposal(base_sql, SqlStructureReason::JoinUnsupported);
        };
        let Some(range) = source_range_from_span(sql, expression.span()) else {
            return rejected_proposal(base_sql, SqlStructureReason::JoinUnsupported);
        };
        if has_comment_token_in_range(sql, range) != Some(false) {
            return rejected_proposal(base_sql, SqlStructureReason::JoinUnsupported);
        }
        let replacement = join_equality_sql(join, left_column, right_column);
        let mut source_edits = source_edits;
        source_edits.push((range, replacement));
        let Some(rewritten) = apply_sql_source_edits(sql, source_edits) else {
            return rejected_proposal(base_sql, SqlStructureReason::JoinUnsupported);
        };
        rewritten
    } else if parsed.can_add_join {
        let Some(range) = source_range_from_span(sql, table.relation.span()) else {
            return rejected_proposal(base_sql, SqlStructureReason::JoinUnsupported);
        };
        if has_comment_token_in_range(sql, range) != Some(false) {
            return rejected_proposal(base_sql, SqlStructureReason::JoinUnsupported);
        }
        let Some(source_relation) = sql.get(range.start..range.end) else {
            return rejected_proposal(base_sql, SqlStructureReason::JoinUnsupported);
        };
        let (_, alias) = match parse_form_table(&table.relation, context) {
            Ok(table) => table,
            Err(reason) => return rejected_proposal(base_sql, reason),
        };
        let had_alias = alias.is_some();
        let left_alias = alias.unwrap_or_else(|| SqlStructureAlias {
            name: "_ugoite_left".to_owned(),
            quoted: true,
        });
        let mut right_name = "_ugoite_right".to_owned();
        let mut suffix = 2;
        while right_name.eq_ignore_ascii_case(&left_alias.name) {
            right_name = format!("_ugoite_right_{suffix}");
            suffix += 1;
        }
        let right_alias = SqlStructureAlias {
            name: right_name,
            quoted: true,
        };
        let qualifying_alias = left_alias.clone();
        let left_source = if had_alias {
            source_relation.to_owned()
        } else {
            format!("{source_relation} AS {}", quote_ident(&left_alias.name))
        };
        let relation = quote_ident(&context.relation);
        let replacement = format!(
            "{left_source} {} JOIN {relation} AS {} ON {}",
            join_kind.keyword(),
            quote_ident(&right_alias.name),
            join_equality_sql(
                &SqlStructureJoin {
                    relation: context.relation.clone(),
                    join_kind,
                    left_alias,
                    right_alias,
                    left_column: left_column.to_owned(),
                    right_column: right_column.to_owned(),
                },
                left_column,
                right_column
            ),
        );
        let Some(rewritten) = qualify_join_input_references(
            sql,
            &parsed.query,
            select,
            context,
            &qualifying_alias,
            range,
            &replacement,
        ) else {
            return rejected_proposal(base_sql, SqlStructureReason::JoinUnsupported);
        };
        rewritten
    } else {
        return rejected_proposal(base_sql, SqlStructureReason::JoinUnsupported);
    };

    finish_structure_proposal(base_sql, proposed_sql, context, StructureEditTarget::Join)
}

fn join_kind_source_edits(
    sql: &str,
    table: &TableWithJoins,
    join: &sqlparser::ast::Join,
    current_kind: SqlStructureJoinKind,
    requested_kind: SqlStructureJoinKind,
) -> Option<Vec<(SqlSourceRange, String)>> {
    let left_relation = source_range_from_span(sql, table.relation.span())?;
    let right_relation = source_range_from_span(sql, join.relation.span())?;
    if left_relation.end > right_relation.start {
        return None;
    }
    let document = parse_sql_document(sql);
    if document.status != SqlDocumentStatus::Parsed {
        return None;
    }
    let words = document
        .tokens
        .iter()
        .filter(|token| {
            token.kind == SqlDocumentTokenKind::Word
                && token.range.start >= left_relation.end
                && token.range.end <= right_relation.start
        })
        .map(|token| {
            Some((
                token.range,
                sql.get(token.range.start..token.range.end)?
                    .to_ascii_uppercase(),
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    let (join_token, prefix_tokens) = words.split_last()?;
    if join_token.1 != "JOIN" {
        return None;
    }
    let prefix = prefix_tokens
        .iter()
        .map(|(_, word)| word.as_str())
        .collect::<Vec<_>>();
    let prefix_matches_current = match (current_kind, prefix.as_slice()) {
        (SqlStructureJoinKind::Inner, [])
        | (SqlStructureJoinKind::Inner, ["INNER"])
        | (SqlStructureJoinKind::Left, ["LEFT"])
        | (SqlStructureJoinKind::Left, ["LEFT", "OUTER"])
        | (SqlStructureJoinKind::Right, ["RIGHT"])
        | (SqlStructureJoinKind::Right, ["RIGHT", "OUTER"])
        | (SqlStructureJoinKind::Full, ["FULL"])
        | (SqlStructureJoinKind::Full, ["FULL", "OUTER"]) => true,
        _ => false,
    };
    if !prefix_matches_current {
        return None;
    }
    if current_kind == requested_kind {
        return Some(Vec::new());
    }

    if requested_kind == SqlStructureJoinKind::Inner {
        return Some(
            prefix_tokens
                .iter()
                .map(|(range, _)| (*range, String::new()))
                .collect(),
        );
    }
    if let Some((first_prefix, _)) = prefix_tokens.first() {
        Some(
            std::iter::once((*first_prefix, requested_kind.keyword().to_owned()))
                .chain(
                    prefix_tokens
                        .iter()
                        .skip(1)
                        .map(|(range, _)| (*range, String::new())),
                )
                .collect(),
        )
    } else {
        Some(vec![(
            SqlSourceRange {
                start: join_token.0.start,
                end: join_token.0.start,
            },
            format!("{} ", requested_kind.keyword()),
        )])
    }
}

fn finish_structure_proposal(
    base_sql: String,
    proposed_sql: String,
    context: &SqlStructureContext,
    edit_target: StructureEditTarget,
) -> SqlStructureProposal {
    if let Err(reason) = parse_supported_sql(&proposed_sql, context) {
        return rejected_proposal(base_sql, reason);
    }
    let requires_confirmation = proposed_sql != base_sql;
    let document_node_edit = if requires_confirmation {
        match derive_structure_node_edit(&base_sql, &proposed_sql, edit_target) {
            Some(edit) => Some(edit),
            None => return rejected_proposal(base_sql, SqlStructureReason::RoundTripFailed),
        }
    } else {
        None
    };
    SqlStructureProposal {
        contract_version: SQL_STRUCTURE_CONTRACT_VERSION,
        status: SqlStructureStatus::Supported,
        reason: None,
        base_sql,
        proposed_sql: Some(proposed_sql),
        document_node_edit,
        requires_confirmation,
        added_stable_tie_breakers: Vec::new(),
    }
}

#[derive(Clone, Copy)]
enum StructureEditTarget {
    Relation,
    Join,
    Projection,
    Filter,
    Sort,
    GroupBy,
    Having,
    RowLimit,
}

fn structure_edit_target(operation: &SqlStructureOperation) -> StructureEditTarget {
    match operation {
        SqlStructureOperation::SetRelation { .. } => StructureEditTarget::Relation,
        SqlStructureOperation::SetJoin { .. } => StructureEditTarget::Join,
        SqlStructureOperation::SetColumns { .. }
        | SqlStructureOperation::SetLiteralProjection { .. }
        | SqlStructureOperation::SetProjectionSlots { .. }
        | SqlStructureOperation::SetAggregates { .. }
        | SqlStructureOperation::SetDistinct { .. } => StructureEditTarget::Projection,
        SqlStructureOperation::SetFilters { .. }
        | SqlStructureOperation::SetFilterExpression { .. } => StructureEditTarget::Filter,
        SqlStructureOperation::SetHaving { .. } => StructureEditTarget::Having,
        SqlStructureOperation::SetSort { .. } => StructureEditTarget::Sort,
        SqlStructureOperation::SetGroupBy { .. } => StructureEditTarget::GroupBy,
        SqlStructureOperation::SetRowLimit { .. } => StructureEditTarget::RowLimit,
    }
}

fn structure_edit_node_kinds(target: StructureEditTarget) -> &'static [SqlDocumentNodeKind] {
    use SqlDocumentNodeKind as Node;

    match target {
        // `SELECT 1` relation selection changes both the projection and FROM
        // clause, so Select is the smallest safe scope. Existing table changes
        // resolve to the narrower Relation node.
        StructureEditTarget::Relation => &[
            Node::Relation,
            Node::TableFactor,
            Node::RelationTree,
            Node::FromClause,
            Node::Select,
            Node::Query,
        ],
        StructureEditTarget::Join => &[
            Node::Join,
            Node::RelationTree,
            Node::FromClause,
            Node::Select,
            Node::Query,
        ],
        StructureEditTarget::Projection => &[Node::SelectList, Node::Select, Node::Query],
        StructureEditTarget::Filter => &[Node::WhereClause, Node::Select, Node::Query],
        StructureEditTarget::Sort => &[Node::OrderByClause, Node::Query, Node::Select],
        StructureEditTarget::GroupBy => &[Node::GroupByClause, Node::Select, Node::Query],
        StructureEditTarget::Having => &[Node::HavingClause, Node::Select, Node::Query],
        StructureEditTarget::RowLimit => &[Node::LimitClause, Node::Query],
    }
}

/// Turn a Rust-derived structure proposal into exactly one guarded syntax-node
/// replacement. The candidate node is accepted only when replacing its exact
/// source range reproduces the preview byte-for-byte; otherwise the caller
/// fails closed instead of falling back to a whole-document edit.
fn derive_structure_node_edit(
    base_sql: &str,
    proposed_sql: &str,
    target: StructureEditTarget,
) -> Option<SqlDocumentNodeEditTransaction> {
    if base_sql == proposed_sql {
        return None;
    }

    let document = parse_sql_document(base_sql);
    if document.status != SqlDocumentStatus::Parsed {
        return None;
    }
    let (changed_start, changed_end) = changed_source_range(base_sql, proposed_sql)?;
    let allowed_kinds = structure_edit_node_kinds(target);
    let mut candidates = document
        .nodes
        .iter()
        .filter_map(|node| {
            let kind_rank = allowed_kinds.iter().position(|kind| *kind == node.kind)?;
            let range = node.range?;
            let contains_change = if changed_start == changed_end {
                range.start <= changed_start && changed_start <= range.end
            } else {
                range.start <= changed_start && changed_end <= range.end
            };
            contains_change.then_some((
                range.end - range.start,
                kind_rank,
                node.id,
                node.kind,
                range,
            ))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(length, kind_rank, node_id, _, _)| (*length, *kind_rank, *node_id));

    for (_, _, node_id, expected_kind, range) in candidates {
        let Some(new_end) = shifted_offset(range.end, base_sql.len(), proposed_sql.len()) else {
            continue;
        };
        if range.start > new_end
            || proposed_sql.get(..range.start) != base_sql.get(..range.start)
            || proposed_sql.get(new_end..) != base_sql.get(range.end..)
        {
            continue;
        }
        let Some(replacement) = proposed_sql.get(range.start..new_end) else {
            continue;
        };
        let transaction = SqlDocumentNodeEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: base_sql.to_owned(),
            edits: vec![SqlDocumentNodeEdit {
                node_id,
                expected_kind,
                replacement: replacement.to_owned(),
            }],
        };
        if apply_sql_document_node_edit_transaction(base_sql, &transaction)
            .is_ok_and(|result| result.document.source == proposed_sql)
        {
            return Some(transaction);
        }
    }
    None
}

fn changed_source_range(base_sql: &str, proposed_sql: &str) -> Option<(usize, usize)> {
    let mut prefix = 0;
    for (base_char, proposed_char) in base_sql.chars().zip(proposed_sql.chars()) {
        if base_char != proposed_char {
            break;
        }
        prefix += base_char.len_utf8();
    }

    let mut suffix = 0;
    for (base_char, proposed_char) in base_sql[prefix..]
        .chars()
        .rev()
        .zip(proposed_sql[prefix..].chars().rev())
    {
        if base_char != proposed_char {
            break;
        }
        suffix += base_char.len_utf8();
    }

    Some((prefix, base_sql.len().checked_sub(suffix)?))
}

fn shifted_offset(offset: usize, old_len: usize, new_len: usize) -> Option<usize> {
    if new_len >= old_len {
        offset.checked_add(new_len - old_len)
    } else {
        offset.checked_sub(old_len - new_len)
    }
}

fn join_equality_sql(join: &SqlStructureJoin, left_column: &str, right_column: &str) -> String {
    format!(
        "{}.{} = {}.{}",
        alias_ident(&join.left_alias),
        quote_ident(left_column),
        alias_ident(&join.right_alias),
        quote_ident(right_column),
    )
}

fn alias_ident(alias: &SqlStructureAlias) -> String {
    if alias.quoted {
        quote_ident(&alias.name)
    } else {
        Ident::new(alias.name.clone()).to_string()
    }
}

fn quote_ident(value: &str) -> String {
    Ident::with_quote('"', value.to_owned()).to_string()
}

fn qualify_join_input_references(
    sql: &str,
    query: &Query,
    select: &Select,
    context: &SqlStructureContext,
    alias: &SqlStructureAlias,
    relation_range: SqlSourceRange,
    relation_replacement: &str,
) -> Option<String> {
    let mut identifiers = Vec::new();
    if let Some(selection) = &select.selection {
        if !collect_filter_identifiers(selection, &mut identifiers) {
            return None;
        }
    }
    if let Some(order_by) = &query.order_by {
        let OrderByKind::Expressions(expressions) = &order_by.kind else {
            return None;
        };
        for expression in expressions {
            let Expr::Identifier(identifier) = &expression.expr else {
                return None;
            };
            identifiers.push(identifier.clone());
        }
    }

    let mut replacements = vec![(relation_range, relation_replacement.to_owned())];
    for identifier in identifiers {
        let column = find_column_by_ident(context, &identifier)?;
        let range = source_range_from_span(sql, identifier.span)?;
        let replacement = format!("{}.{}", alias_ident(alias), quote_ident(&column.name));
        replacements.push((range, replacement));
    }
    replacements.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));

    let mut rewritten = sql.to_owned();
    let mut previous_start = sql.len();
    for (range, replacement) in replacements {
        if range.end > previous_start {
            return None;
        }
        rewritten = replace_sql_source_range(&rewritten, range, &replacement)?;
        previous_start = range.start;
    }
    Some(rewritten)
}

fn collect_filter_identifiers(expression: &Expr, identifiers: &mut Vec<Ident>) -> bool {
    match expression {
        Expr::Nested(inner) => collect_filter_identifiers(inner, identifiers),
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And | BinaryOperator::Or,
            right,
        } => {
            collect_filter_identifiers(left, identifiers)
                && collect_filter_identifiers(right, identifiers)
        }
        Expr::UnaryOp {
            op: UnaryOperator::Not,
            expr,
        } => collect_filter_identifiers(expr, identifiers),
        Expr::BinaryOp { left, op, .. } if filter_operator_from_binary(op).is_some() => {
            let Expr::Identifier(identifier) = left.as_ref() else {
                return false;
            };
            identifiers.push(identifier.clone());
            true
        }
        Expr::IsNull(inner) | Expr::IsNotNull(inner) => {
            let Expr::Identifier(identifier) = inner.as_ref() else {
                return false;
            };
            identifiers.push(identifier.clone());
            true
        }
        _ => false,
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

    let mut statements =
        parse_generic_sql_statements(sql).map_err(|_| SqlStructureReason::ParseFailed)?;
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
    let joins = validate_relation(select, context)?;

    let (projection, projection_aliases, aggregates) =
        parse_projection(&select.projection, context)?;
    let group_by = parse_group_by(&select.group_by, context)?;
    let having_expression = select
        .having
        .as_ref()
        .map(|having| parse_having_expression(having, context))
        .transpose()?;
    validate_grouped_projection(&projection, &aggregates, &group_by)?;
    let distinct = match select.distinct.as_ref() {
        None | Some(Distinct::All) => false,
        Some(Distinct::Distinct) => true,
        Some(Distinct::On(_)) => return Err(SqlStructureReason::DistinctUnsupported),
    };
    let filter_expression = select
        .selection
        .as_ref()
        .map(|selection| parse_filter_expression(selection, context, &joins))
        .transpose()?;
    let row_limit = parse_row_limit(query.limit_clause.as_ref())?;
    // Preserve the legacy flat projection only when the WHERE expression is
    // semantically a conjunction of predicates. Returning an empty list for
    // OR/NOT avoids exposing a lossy representation to existing consumers.
    let filters = filter_expression
        .as_ref()
        .and_then(flat_conjunctive_filters)
        .unwrap_or_default();
    let order_by = query
        .order_by
        .as_ref()
        .map(|order_by| parse_order_by(order_by, select, context, &joins))
        .transpose()?
        .unwrap_or_default();
    let can_add_join = joins.is_empty()
        && is_wildcard_projection(&select.projection)
        && matches!(&select.group_by, GroupByExpr::Expressions(expressions, modifiers) if expressions.is_empty() && modifiers.is_empty())
        && select.distinct.is_none()
        && select.from.len() == 1;
    if joins.is_empty() {
        validate_order_by_semantics(
            query.order_by.as_ref(),
            select,
            &aggregates,
            &group_by,
            distinct,
            context,
        )?;
    }

    let literal_projection = if context.relation.is_empty() {
        parse_literal_projection(&select.projection)?
    } else {
        Vec::new()
    };

    let referenced_variables = filter_expression
        .as_ref()
        .map(filter_expression_variables)
        .unwrap_or_default()
        .into_iter()
        .chain(
            having_expression
                .as_ref()
                .map(having_expression_variables)
                .unwrap_or_default(),
        )
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let stable_key_ordered =
        if let Some(join) = joins.first() {
            context.stable_key.iter().all(|key| {
                order_by.iter().any(|sort| {
                    names_match(&sort.column, key)
                        && sort.qualifier.as_ref().is_some_and(|qualifier| {
                            aliases_resolve_same(qualifier, &join.left_alias)
                        })
                }) && order_by.iter().any(|sort| {
                    names_match(&sort.column, key)
                        && sort.qualifier.as_ref().is_some_and(|qualifier| {
                            aliases_resolve_same(qualifier, &join.right_alias)
                        })
                })
            })
        } else {
            context.stable_key.iter().all(|key| {
                order_by
                    .iter()
                    .any(|term| term.qualifier.is_none() && names_match(&term.column, key))
            })
        };
    let stable_pagination = aggregates.is_empty()
        && group_by.is_empty()
        && !distinct
        && !context.stable_key.is_empty()
        && stable_key_ordered;

    Ok(ParsedSqlStructure {
        query,
        projection,
        literal_projection,
        projection_aliases,
        filters,
        filter_expression,
        aggregates,
        group_by,
        having_expression,
        distinct,
        row_limit,
        order_by,
        joins,
        can_add_join,
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

fn parse_row_limit(
    limit_clause: Option<&LimitClause>,
) -> Result<Option<SqlStructureRowLimit>, SqlStructureReason> {
    let parsed = match limit_clause {
        None => return Ok(None),
        Some(LimitClause::LimitOffset {
            limit,
            offset,
            limit_by,
        }) if limit_by.is_empty() => SqlStructureRowLimit {
            limit: limit.as_ref().and_then(nonnegative_integer_spelling),
            offset: offset
                .as_ref()
                .and_then(|offset| nonnegative_integer_spelling(&offset.value)),
        },
        Some(LimitClause::OffsetCommaLimit { offset, limit }) => SqlStructureRowLimit {
            limit: nonnegative_integer_spelling(limit),
            offset: nonnegative_integer_spelling(offset),
        },
        _ => return Err(SqlStructureReason::RowLimitUnsupported),
    };
    if (parsed.limit.is_none() && parsed.offset.is_none())
        || (parsed.limit.is_none()
            && limit_clause.is_some_and(|clause| {
                matches!(clause, LimitClause::LimitOffset { offset: None, .. })
            }))
    {
        return Err(SqlStructureReason::RowLimitUnsupported);
    }
    Ok(Some(parsed))
}

fn partial_query_clause_analysis(
    sql: &str,
    context: &SqlStructureContext,
    reason: SqlStructureReason,
) -> Option<SqlStructureAnalysis> {
    if matches!(
        reason,
        SqlStructureReason::InvalidContext
            | SqlStructureReason::SqlTooLarge
            | SqlStructureReason::ParseFailed
            | SqlStructureReason::SingleStatementRequired
            | SqlStructureReason::SelectRequired
    ) || validate_context(context).is_err()
    {
        return None;
    }
    if sql.len() > MAX_SQL_STRUCTURE_QUERY_BYTES {
        return None;
    }
    let mut statements = parse_generic_sql_statements(sql).ok()?;
    if statements.len() != 1 {
        return None;
    }
    let Statement::Query(query) = statements.remove(0) else {
        return None;
    };
    if query.fetch.is_some()
        || !query.locks.is_empty()
        || query.for_clause.is_some()
        || query.settings.is_some()
        || query.format_clause.is_some()
        || !query.pipe_operators.is_empty()
    {
        return None;
    }
    let row_limit = parse_row_limit(query.limit_clause.as_ref()).ok()?;
    let mut analysis = rejected_analysis(reason);
    analysis.row_limit = row_limit;
    analysis.capabilities.row_limit = capability(true, None);
    Some(analysis)
}

fn propose_sql_row_limit_edit(
    sql: &str,
    context: &SqlStructureContext,
    limit: Option<&str>,
    offset: Option<&str>,
) -> SqlStructureProposal {
    let base_sql = sql.to_owned();
    if let Err(reason) = validate_context(context) {
        return rejected_proposal(base_sql, reason);
    }
    if sql.len() > MAX_SQL_STRUCTURE_QUERY_BYTES {
        return rejected_proposal(base_sql, SqlStructureReason::SqlTooLarge);
    }
    let replacement = match render_row_limit(limit, offset) {
        Ok(replacement) => replacement,
        Err(reason) => return rejected_proposal(base_sql, reason),
    };
    let mut statements = match parse_generic_sql_statements(sql) {
        Ok(statements) => statements,
        Err(_) => return rejected_proposal(base_sql, SqlStructureReason::ParseFailed),
    };
    if statements.len() != 1 {
        return rejected_proposal(base_sql, SqlStructureReason::SingleStatementRequired);
    }
    let Statement::Query(query) = statements.remove(0) else {
        return rejected_proposal(base_sql, SqlStructureReason::SelectRequired);
    };
    if query.fetch.is_some()
        || !query.locks.is_empty()
        || query.for_clause.is_some()
        || query.settings.is_some()
        || query.format_clause.is_some()
        || !query.pipe_operators.is_empty()
        || parse_row_limit(query.limit_clause.as_ref()).is_err()
    {
        return rejected_proposal(base_sql, SqlStructureReason::RowLimitUnsupported);
    }
    let proposed_sql = match rewrite_top_level_query_clause(
        &base_sql,
        &query,
        SqlDocumentNodeKind::LimitClause,
        replacement.as_deref(),
        SqlStructureReason::RowLimitUnsupported,
    ) {
        Ok(sql) => sql,
        Err(reason) => return rejected_proposal(base_sql, reason),
    };
    if parse_sql_document(&proposed_sql).status != SqlDocumentStatus::Parsed {
        return rejected_proposal(base_sql, SqlStructureReason::RoundTripFailed);
    }
    let requires_confirmation = proposed_sql != base_sql;
    let document_node_edit = if requires_confirmation {
        match derive_structure_node_edit(&base_sql, &proposed_sql, StructureEditTarget::RowLimit) {
            Some(edit) => Some(edit),
            None => return rejected_proposal(base_sql, SqlStructureReason::RoundTripFailed),
        }
    } else {
        None
    };
    SqlStructureProposal {
        contract_version: SQL_STRUCTURE_CONTRACT_VERSION,
        status: SqlStructureStatus::Supported,
        reason: None,
        base_sql,
        proposed_sql: Some(proposed_sql),
        document_node_edit,
        requires_confirmation,
        added_stable_tie_breakers: Vec::new(),
    }
}

fn rewrite_top_level_query_clause(
    sql: &str,
    query: &Query,
    kind: SqlDocumentNodeKind,
    replacement: Option<&str>,
    reason: SqlStructureReason,
) -> Result<String, SqlStructureReason> {
    let document = parse_sql_document(sql);
    if document.status != SqlDocumentStatus::Parsed {
        return Err(reason);
    }
    let query_range = source_range_from_span(sql, query.span()).ok_or(reason)?;
    let query_id = document
        .nodes
        .iter()
        .find(|node| node.kind == SqlDocumentNodeKind::Query && node.range == Some(query_range))
        .map(|node| node.id)
        .ok_or(reason)?;
    if let Some(existing) = document
        .nodes
        .iter()
        .find(|node| node.kind == kind && node.parent == Some(query_id))
        .and_then(|node| node.range)
    {
        if has_comment_token_in_range(sql, existing) != Some(false) {
            return Err(reason);
        }
        return replace_sql_source_range(sql, existing, replacement.unwrap_or("")).ok_or(reason);
    }
    let Some(replacement) = replacement else {
        return Ok(sql.to_owned());
    };
    let insertion = if sql
        .get(..query_range.end)
        .is_some_and(|prefix| prefix.ends_with(';'))
    {
        query_range.end.checked_sub(1).ok_or(reason)?
    } else {
        query_range.end
    };
    insert_sql_source_offset(sql, insertion, &format!(" {replacement}")).ok_or(reason)
}

fn nonnegative_integer_spelling(expression: &Expr) -> Option<String> {
    let Expr::Value(value) = expression else {
        return None;
    };
    let Value::Number(number, long) = &value.value else {
        return None;
    };
    (!long && number.bytes().all(|byte| byte.is_ascii_digit()) && number.parse::<i64>().is_ok())
        .then(|| number.clone())
}

fn render_row_limit(
    limit: Option<&str>,
    offset: Option<&str>,
) -> Result<Option<String>, SqlStructureReason> {
    let is_supported_limit = |value: &str| {
        value.bytes().all(|byte| byte.is_ascii_digit()) && value.parse::<i64>().is_ok()
    };
    if limit.is_some_and(|value| !is_supported_limit(value))
        || offset.is_some_and(|value| !is_supported_limit(value))
    {
        return Err(SqlStructureReason::RowLimitUnsupported);
    }
    match (limit, offset) {
        (None, None) => Ok(None),
        (Some(limit), None) => Ok(Some(format!("LIMIT {limit}"))),
        (None, Some(offset)) => Ok(Some(format!("OFFSET {offset}"))),
        (Some(limit), Some(offset)) => Ok(Some(format!("LIMIT {limit} OFFSET {offset}"))),
    }
}

fn is_nonnegative_integer_literal(expression: &Expr) -> bool {
    let Expr::Value(value) = expression else {
        return false;
    };
    let Value::Number(number, long) = &value.value else {
        return false;
    };
    !long && number.bytes().all(|byte| byte.is_ascii_digit()) && number.parse::<i64>().is_ok()
}

fn supported_projection_literal(expression: &Expr) -> Option<SqlStructureLiteral> {
    match expression {
        Expr::Value(value) => match &value.value {
            Value::Number(number, long) if !long && number.parse::<i64>().is_ok() => {
                Some(SqlStructureLiteral::Integer(number.clone()))
            }
            Value::SingleQuotedString(value) => Some(SqlStructureLiteral::String(value.clone())),
            Value::Boolean(value) => Some(SqlStructureLiteral::Boolean(*value)),
            Value::Null => Some(SqlStructureLiteral::Null),
            _ => None,
        },
        Expr::UnaryOp { op, expr } if matches!(*op, UnaryOperator::Minus | UnaryOperator::Plus) => {
            let Expr::Value(value) = expr.as_ref() else {
                return None;
            };
            let Value::Number(number, long) = &value.value else {
                return None;
            };
            let signed = if *op == UnaryOperator::Minus {
                format!("-{number}")
            } else {
                format!("+{number}")
            };
            (!long && signed.parse::<i64>().is_ok()).then_some(SqlStructureLiteral::Integer(signed))
        }
        _ => None,
    }
}

fn parse_literal_projection(
    projection: &[SelectItem],
) -> Result<Vec<SqlStructureLiteral>, SqlStructureReason> {
    projection
        .iter()
        .map(|item| {
            let SelectItem::UnnamedExpr(expression) = item else {
                return Err(SqlStructureReason::ProjectionUnsupported);
            };
            supported_projection_literal(expression)
                .ok_or(SqlStructureReason::ProjectionUnsupported)
        })
        .collect()
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
) -> Result<Vec<SqlStructureJoin>, SqlStructureReason> {
    if select.from.is_empty() {
        return if context.relation.is_empty() {
            Ok(Vec::new())
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
    let (left_relation, left_alias) = parse_form_table(&table.relation, context)?;
    if table.joins.is_empty() {
        return Ok(Vec::new());
    }
    if table.joins.len() != 1
        || !is_wildcard_projection(&select.projection)
        || !matches!(&select.group_by, GroupByExpr::Expressions(expressions, modifiers) if expressions.is_empty() && modifiers.is_empty())
        || select.distinct.is_some()
    {
        return Err(SqlStructureReason::JoinUnsupported);
    }
    let Some(left_alias) = left_alias else {
        return Err(SqlStructureReason::JoinUnsupported);
    };
    let join = &table.joins[0];
    if join.global {
        return Err(SqlStructureReason::JoinUnsupported);
    }
    let (right_relation, right_alias) = parse_form_table(&join.relation, context)?;
    let Some(right_alias) = right_alias else {
        return Err(SqlStructureReason::JoinUnsupported);
    };
    if !identifier_matches(left_relation, context.relation.as_str())
        || !identifier_matches(right_relation, context.relation.as_str())
        || aliases_resolve_same(&left_alias, &right_alias)
    {
        return Err(SqlStructureReason::JoinUnsupported);
    }
    let (join_kind, constraint) = match &join.join_operator {
        JoinOperator::Join(JoinConstraint::On(expression))
        | JoinOperator::Inner(JoinConstraint::On(expression)) => {
            (SqlStructureJoinKind::Inner, expression)
        }
        JoinOperator::Left(JoinConstraint::On(expression))
        | JoinOperator::LeftOuter(JoinConstraint::On(expression)) => {
            (SqlStructureJoinKind::Left, expression)
        }
        JoinOperator::Right(JoinConstraint::On(expression))
        | JoinOperator::RightOuter(JoinConstraint::On(expression)) => {
            (SqlStructureJoinKind::Right, expression)
        }
        JoinOperator::FullOuter(JoinConstraint::On(expression)) => {
            (SqlStructureJoinKind::Full, expression)
        }
        _ => return Err(SqlStructureReason::JoinUnsupported),
    };
    let Expr::BinaryOp {
        left,
        op: BinaryOperator::Eq,
        right,
    } = constraint
    else {
        return Err(SqlStructureReason::JoinUnsupported);
    };
    let Some((left_column, right_column)) =
        parse_join_columns(left, right, &left_alias, &right_alias, context)
    else {
        return Err(SqlStructureReason::JoinUnsupported);
    };
    validate_join_columns(context, &left_column, &right_column)
        .map_err(|_| SqlStructureReason::JoinUnsupported)?;
    Ok(vec![SqlStructureJoin {
        relation: context.relation.clone(),
        join_kind,
        left_alias,
        right_alias,
        left_column,
        right_column,
    }])
}

fn parse_form_table<'a>(
    factor: &'a TableFactor,
    context: &SqlStructureContext,
) -> Result<(&'a Ident, Option<SqlStructureAlias>), SqlStructureReason> {
    let relation = simple_table_name(factor).ok_or(SqlStructureReason::RelationUnsupported)?;
    if !identifier_matches(relation, &context.relation) {
        return Err(SqlStructureReason::RelationMismatch);
    }
    let alias = match factor {
        TableFactor::Table { alias, .. } => alias.as_ref().map(|alias| SqlStructureAlias {
            name: alias.name.value.clone(),
            quoted: alias.name.quote_style.is_some(),
        }),
        _ => None,
    };
    Ok((relation, alias))
}

fn simple_table_name(factor: &TableFactor) -> Option<&Ident> {
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
    } = factor
    else {
        return None;
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
        return None;
    }
    let Some(ObjectNamePart::Identifier(relation)) = name.0.first() else {
        return None;
    };
    (name.0.len() == 1).then_some(relation)
}

fn rewrite_join_relation(
    sql: &str,
    table: &TableWithJoins,
    relation: &str,
    context: &SqlStructureContext,
) -> Option<String> {
    if table.joins.len() != 1 {
        return None;
    }
    let left_relation = simple_table_name(&table.relation)?;
    let right = &table.joins[0];
    if right.global {
        return None;
    }
    let right_relation = simple_table_name(&right.relation)?;
    if !identifiers_resolve_same(left_relation, right_relation) {
        return None;
    }
    let (left_alias, right_alias) = match (&table.relation, &right.relation) {
        (
            TableFactor::Table {
                alias: Some(left), ..
            },
            TableFactor::Table {
                alias: Some(right), ..
            },
        ) => (
            SqlStructureAlias {
                name: left.name.value.clone(),
                quoted: left.name.quote_style.is_some(),
            },
            SqlStructureAlias {
                name: right.name.value.clone(),
                quoted: right.name.quote_style.is_some(),
            },
        ),
        _ => return None,
    };
    let constraint = match &right.join_operator {
        JoinOperator::Join(JoinConstraint::On(expression))
        | JoinOperator::Inner(JoinConstraint::On(expression))
        | JoinOperator::Left(JoinConstraint::On(expression))
        | JoinOperator::LeftOuter(JoinConstraint::On(expression))
        | JoinOperator::Right(JoinConstraint::On(expression))
        | JoinOperator::RightOuter(JoinConstraint::On(expression))
        | JoinOperator::FullOuter(JoinConstraint::On(expression)) => expression,
        _ => return None,
    };
    let Expr::BinaryOp {
        left,
        op: BinaryOperator::Eq,
        right,
    } = constraint
    else {
        return None;
    };
    let (left_column, right_column) =
        parse_join_columns(left, right, &left_alias, &right_alias, context)?;
    validate_join_columns(context, &left_column, &right_column).ok()?;

    let left_range = source_range_from_span(sql, left_relation.span)?;
    let right_range = source_range_from_span(sql, right_relation.span)?;
    if has_comment_token_in_range(sql, left_range)? || has_comment_token_in_range(sql, right_range)?
    {
        return None;
    }
    let replacement = quote_ident(relation);
    let mut replacements = [
        (left_range, replacement.as_str()),
        (right_range, replacement.as_str()),
    ];
    replacements.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let mut rewritten = sql.to_owned();
    for (range, value) in replacements {
        rewritten = replace_sql_source_range(&rewritten, range, value)?;
    }
    Some(rewritten)
}

fn parse_join_columns(
    left: &Expr,
    right: &Expr,
    left_alias: &SqlStructureAlias,
    right_alias: &SqlStructureAlias,
    context: &SqlStructureContext,
) -> Option<(String, String)> {
    let left = qualified_column(left)?;
    let right = qualified_column(right)?;
    if !identifier_matches_alias(left.0, left_alias)
        || !identifier_matches_alias(right.0, right_alias)
    {
        return None;
    }
    let left_column = find_column_by_ident(context, left.1)?;
    let right_column = find_column_by_ident(context, right.1)?;
    Some((left_column.name.clone(), right_column.name.clone()))
}

fn resolve_column_reference(
    expression: &Expr,
    context: &SqlStructureContext,
    joins: &[SqlStructureJoin],
    unsupported_reason: SqlStructureReason,
) -> Result<(String, Option<SqlStructureAlias>), SqlStructureReason> {
    if joins.is_empty() {
        let Expr::Identifier(identifier) = expression else {
            return Err(unsupported_reason);
        };
        let column =
            find_column_by_ident(context, identifier).ok_or(SqlStructureReason::UnknownColumn)?;
        return Ok((column.name.clone(), None));
    }

    let [join] = joins else {
        return Err(unsupported_reason);
    };
    let Expr::CompoundIdentifier(parts) = expression else {
        return Err(unsupported_reason);
    };
    let [qualifier, identifier] = parts.as_slice() else {
        return Err(unsupported_reason);
    };
    let qualifier = if identifier_matches_alias(qualifier, &join.left_alias) {
        join.left_alias.clone()
    } else if identifier_matches_alias(qualifier, &join.right_alias) {
        join.right_alias.clone()
    } else {
        return Err(unsupported_reason);
    };
    let column =
        find_column_by_ident(context, identifier).ok_or(SqlStructureReason::UnknownColumn)?;
    Ok((column.name.clone(), Some(qualifier)))
}

fn alias_resolution_key(alias: &SqlStructureAlias) -> String {
    if alias.quoted {
        alias.name.clone()
    } else {
        alias.name.to_ascii_lowercase()
    }
}

fn aliases_resolve_same(left: &SqlStructureAlias, right: &SqlStructureAlias) -> bool {
    let to_ident = |alias: &SqlStructureAlias| {
        if alias.quoted {
            Ident::with_quote('"', alias.name.clone())
        } else {
            Ident::new(alias.name.clone())
        }
    };
    identifiers_resolve_same(&to_ident(left), &to_ident(right))
}

fn qualified_column(expression: &Expr) -> Option<(&Ident, &Ident)> {
    let Expr::CompoundIdentifier(parts) = expression else {
        return None;
    };
    match parts.as_slice() {
        [alias, column] => Some((alias, column)),
        _ => None,
    }
}

fn validate_join_columns(
    context: &SqlStructureContext,
    left_column: &str,
    right_column: &str,
) -> Result<(), SqlStructureReason> {
    let left = find_column(context, left_column).ok_or(SqlStructureReason::UnknownColumn)?;
    let right = find_column(context, right_column).ok_or(SqlStructureReason::UnknownColumn)?;
    if left.column_type != right.column_type
        || matches!(
            left.column_type,
            SqlStructureColumnType::Json | SqlStructureColumnType::Other
        )
    {
        return Err(SqlStructureReason::JoinUnsupported);
    }
    Ok(())
}

fn is_wildcard_projection(projection: &[SelectItem]) -> bool {
    matches!(
        projection,
        [SelectItem::Wildcard(options)]
            if options.opt_ilike.is_none()
                && options.opt_exclude.is_none()
                && options.opt_except.is_none()
                && options.opt_replace.is_none()
                && options.opt_rename.is_none()
                && options.opt_alias.is_none()
    )
}

type ParsedProjection = (
    Vec<String>,
    Vec<SqlStructureProjectionAlias>,
    Vec<SqlStructureAggregate>,
);

fn parse_projection(
    projection: &[SelectItem],
    context: &SqlStructureContext,
) -> Result<ParsedProjection, SqlStructureReason> {
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
                return Ok((vec!["*".to_owned()], Vec::new(), Vec::new()));
            }
        }
    }

    let mut columns = Vec::new();
    let mut projection_aliases = Vec::new();
    let mut aggregates = Vec::new();
    for item in projection {
        match item {
            SelectItem::UnnamedExpr(Expr::Identifier(identifier)) => {
                let column = find_column_by_ident(context, identifier)
                    .ok_or(SqlStructureReason::UnknownColumn)?;
                columns.push(column.name.clone());
                if !context.relation.is_empty() {
                    projection_aliases.push(SqlStructureProjectionAlias {
                        column: column.name.clone(),
                        alias: None,
                    });
                }
            }
            SelectItem::ExprWithAlias {
                expr: Expr::Identifier(identifier),
                alias,
            } if !context.relation.is_empty() => {
                let column = find_column_by_ident(context, identifier)
                    .ok_or(SqlStructureReason::UnknownColumn)?;
                let alias = SqlStructureAlias {
                    name: alias.value.clone(),
                    quoted: alias.quote_style.is_some(),
                };
                if !valid_projection_alias(&alias) {
                    return Err(SqlStructureReason::ProjectionUnsupported);
                }
                columns.push(column.name.clone());
                projection_aliases.push(SqlStructureProjectionAlias {
                    column: column.name.clone(),
                    alias: Some(alias),
                });
            }
            SelectItem::UnnamedExpr(expression) if context.relation.is_empty() => {
                if supported_projection_literal(expression).is_none() {
                    return Err(SqlStructureReason::ProjectionUnsupported);
                }
                columns.push(expression.to_string());
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
    if !aggregates.is_empty() || projection_aliases.len() != columns.len() {
        projection_aliases.clear();
    }
    Ok((columns, projection_aliases, aggregates))
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
    let descriptor =
        user_sql_aggregate(&name.value).ok_or(SqlStructureReason::AggregateUnsupported)?;
    let aggregate_function = if name.value.eq_ignore_ascii_case("mean") {
        SqlStructureAggregateFunction::Mean
    } else {
        match descriptor.name {
            "count" => SqlStructureAggregateFunction::Count,
            "sum" => SqlStructureAggregateFunction::Sum,
            "avg" => SqlStructureAggregateFunction::Avg,
            "min" => SqlStructureAggregateFunction::Min,
            "max" => SqlStructureAggregateFunction::Max,
            _ => return Err(SqlStructureReason::AggregateUnsupported),
        }
    };
    let (argument_nodes, clauses, duplicate_treatment) = match &function.args {
        FunctionArguments::None => (&[][..], &[][..], None),
        FunctionArguments::List(arguments) => (
            arguments.args.as_slice(),
            arguments.clauses.as_slice(),
            arguments.duplicate_treatment,
        ),
        FunctionArguments::Subquery(_) => return Err(SqlStructureReason::AggregateUnsupported),
    };
    if !clauses.is_empty() || !descriptor.arity.accepts(argument_nodes.len()) {
        return Err(SqlStructureReason::AggregateUnsupported);
    }
    let distinct = match duplicate_treatment {
        None | Some(DuplicateTreatment::All) => false,
        Some(DuplicateTreatment::Distinct) => true,
    };
    if distinct && descriptor.distinct == SqlAggregateDistinct::Unsupported {
        return Err(SqlStructureReason::AggregateUnsupported);
    }
    if distinct
        && descriptor.distinct == SqlAggregateDistinct::SingleArgument
        && argument_nodes.len() != 1
    {
        return Err(SqlStructureReason::AggregateUnsupported);
    }
    let mut parsed_columns = Vec::with_capacity(argument_nodes.len());
    let mut wildcard = false;
    for argument_node in argument_nodes {
        let FunctionArg::Unnamed(argument) = argument_node else {
            return Err(SqlStructureReason::AggregateUnsupported);
        };
        match argument {
            FunctionArgExpr::Wildcard
                if descriptor.wildcard != SqlAggregateWildcard::Unsupported
                    && argument_nodes.len() == 1 =>
            {
                wildcard = true;
            }
            FunctionArgExpr::Expr(Expr::Identifier(identifier)) => {
                let column = find_column_by_ident(context, identifier)
                    .ok_or(SqlStructureReason::UnknownColumn)?;
                if distinct && !column_type_supports_distinct_aggregate(column.column_type) {
                    return Err(SqlStructureReason::AggregateTypeUnsupported);
                }
                if matches!(
                    descriptor.input,
                    crate::sql_functions::SqlAggregateInput::Numeric
                ) && !column_type_supports_numeric_aggregate(column.column_type)
                {
                    return Err(SqlStructureReason::AggregateTypeUnsupported);
                }
                if matches!(
                    descriptor.input,
                    crate::sql_functions::SqlAggregateInput::Orderable
                ) && !column_type_supports_sort(column.column_type)
                {
                    return Err(SqlStructureReason::AggregateTypeUnsupported);
                }
                parsed_columns.push(column.name.clone());
            }
            _ => return Err(SqlStructureReason::AggregateUnsupported),
        }
    }
    let (column, arguments) = if wildcard {
        (None, Vec::new())
    } else {
        match parsed_columns.as_slice() {
            [] => (None, Vec::new()),
            [column] => (Some(column.clone()), Vec::new()),
            columns if descriptor.name == "count" && !distinct => {
                (Some(columns[0].clone()), columns[1..].to_vec())
            }
            _ => return Err(SqlStructureReason::AggregateUnsupported),
        }
    };
    if alias
        .as_ref()
        .is_some_and(|alias| !valid_projection_alias(alias))
    {
        return Err(SqlStructureReason::AggregateUnsupported);
    }
    Ok(SqlStructureAggregate {
        function: aggregate_function,
        column,
        arguments,
        distinct,
        alias,
    })
}

fn valid_projection_alias(alias: &SqlStructureAlias) -> bool {
    !alias.name.is_empty()
        && alias.name.len() <= 1024
        && !alias.name.chars().any(char::is_control)
        && (alias.quoted || valid_variable_name(&alias.name))
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

fn parse_filter_expression(
    expression: &Expr,
    context: &SqlStructureContext,
    joins: &[SqlStructureJoin],
) -> Result<SqlStructureFilterExpression, SqlStructureReason> {
    let mut nodes = 0;
    let expression = parse_filter_expression_inner(expression, context, joins, 0, &mut nodes)?;
    let mut tree_nodes = 0;
    validate_filter_expression_shape(&expression, 0, &mut tree_nodes)?;
    Ok(expression)
}

fn parse_filter_expression_inner(
    expression: &Expr,
    context: &SqlStructureContext,
    joins: &[SqlStructureJoin],
    depth: usize,
    nodes: &mut usize,
) -> Result<SqlStructureFilterExpression, SqlStructureReason> {
    // The parser AST is left associative, so a wide flat AND/OR chain can be
    // deep even though its editable boolean tree is shallow. Bound traversal
    // independently, then enforce the semantic tree depth after normalization.
    if depth > MAX_FILTER_EXPRESSION_PARSER_DEPTH {
        return Err(SqlStructureReason::FilterUnsupported);
    }
    match expression {
        Expr::Nested(inner) => {
            parse_filter_expression_inner(inner, context, joins, depth + 1, nodes)
        }
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And,
            right,
        } => {
            increment_filter_parser_node(nodes)?;
            let left = parse_filter_expression_inner(left, context, joins, depth + 1, nodes)?;
            let right = parse_filter_expression_inner(right, context, joins, depth + 1, nodes)?;
            Ok(combine_filter_expressions(true, left, right))
        }
        Expr::BinaryOp {
            left,
            op: BinaryOperator::Or,
            right,
        } => {
            increment_filter_parser_node(nodes)?;
            let left = parse_filter_expression_inner(left, context, joins, depth + 1, nodes)?;
            let right = parse_filter_expression_inner(right, context, joins, depth + 1, nodes)?;
            Ok(combine_filter_expressions(false, left, right))
        }
        Expr::UnaryOp {
            op: UnaryOperator::Not,
            expr,
        } => {
            increment_filter_parser_node(nodes)?;
            Ok(SqlStructureFilterExpression::Not {
                expression: Box::new(parse_filter_expression_inner(
                    expr,
                    context,
                    joins,
                    depth + 1,
                    nodes,
                )?),
            })
        }
        Expr::BinaryOp { left, op, right } => {
            let (column, qualifier) = resolve_column_reference(
                left,
                context,
                joins,
                SqlStructureReason::FilterUnsupported,
            )?;
            let operator =
                filter_operator_from_binary(op).ok_or(SqlStructureReason::FilterUnsupported)?;
            if let Some(parameter) = placeholder_name(right.as_ref()) {
                let filter = SqlStructureFilter {
                    column,
                    qualifier,
                    operator,
                    parameter: Some(parameter),
                    second_parameter: None,
                };
                validate_filter(context, &filter)?;
                increment_filter_parser_node(nodes)?;
                Ok(filter_expression_from_filter(filter))
            } else {
                let (right_column, right_qualifier) = resolve_column_reference(
                    right,
                    context,
                    joins,
                    SqlStructureReason::FilterUnsupported,
                )?;
                let left = SqlStructureColumnReference { column, qualifier };
                let right = SqlStructureColumnReference {
                    column: right_column,
                    qualifier: right_qualifier,
                };
                validate_column_comparison(context, joins, &left, operator, &right)?;
                increment_filter_parser_node(nodes)?;
                Ok(SqlStructureFilterExpression::ColumnComparison {
                    left,
                    operator,
                    right,
                })
            }
        }
        Expr::InList {
            expr,
            list,
            negated,
        } if list.len() == 1 => {
            let (column, qualifier) = resolve_column_reference(
                expr,
                context,
                joins,
                SqlStructureReason::FilterUnsupported,
            )?;
            let filter = SqlStructureFilter {
                column,
                qualifier,
                operator: if *negated {
                    SqlStructureFilterOperator::NotIn
                } else {
                    SqlStructureFilterOperator::In
                },
                parameter: Some(
                    placeholder_name(&list[0]).ok_or(SqlStructureReason::FilterUnsupported)?,
                ),
                second_parameter: None,
            };
            validate_filter(context, &filter)?;
            increment_filter_parser_node(nodes)?;
            Ok(filter_expression_from_filter(filter))
        }
        Expr::Between {
            expr,
            negated,
            low,
            high,
        } => {
            let (column, qualifier) = resolve_column_reference(
                expr,
                context,
                joins,
                SqlStructureReason::FilterUnsupported,
            )?;
            let low_parameter =
                placeholder_name(low).ok_or(SqlStructureReason::FilterUnsupported)?;
            let high_parameter =
                placeholder_name(high).ok_or(SqlStructureReason::FilterUnsupported)?;
            let filter = SqlStructureFilter {
                column,
                qualifier,
                operator: if *negated {
                    SqlStructureFilterOperator::NotBetween
                } else {
                    SqlStructureFilterOperator::Between
                },
                parameter: Some(low_parameter),
                second_parameter: Some(high_parameter),
            };
            validate_filter(context, &filter)?;
            increment_filter_parser_node(nodes)?;
            Ok(filter_expression_from_filter(filter))
        }
        Expr::Like {
            expr,
            pattern,
            negated,
            any: false,
            escape_char: None,
        }
        | Expr::ILike {
            expr,
            pattern,
            negated,
            any: false,
            escape_char: None,
        } => {
            let is_ilike = matches!(expression, Expr::ILike { .. });
            let (column, qualifier) = resolve_column_reference(
                expr,
                context,
                joins,
                SqlStructureReason::FilterUnsupported,
            )?;
            let filter = SqlStructureFilter {
                column,
                qualifier,
                operator: match (is_ilike, *negated) {
                    (false, false) => SqlStructureFilterOperator::Like,
                    (false, true) => SqlStructureFilterOperator::NotLike,
                    (true, false) => SqlStructureFilterOperator::Ilike,
                    (true, true) => SqlStructureFilterOperator::NotIlike,
                },
                parameter: Some(
                    placeholder_name(pattern).ok_or(SqlStructureReason::FilterUnsupported)?,
                ),
                second_parameter: None,
            };
            validate_filter(context, &filter)?;
            increment_filter_parser_node(nodes)?;
            Ok(filter_expression_from_filter(filter))
        }
        Expr::IsNull(inner) | Expr::IsNotNull(inner) => {
            let (column, qualifier) = resolve_column_reference(
                inner,
                context,
                joins,
                SqlStructureReason::FilterUnsupported,
            )?;
            let operator = if matches!(expression, Expr::IsNull(_)) {
                SqlStructureFilterOperator::IsNull
            } else {
                SqlStructureFilterOperator::IsNotNull
            };
            let filter = SqlStructureFilter {
                column,
                qualifier,
                operator,
                parameter: None,
                second_parameter: None,
            };
            validate_filter(context, &filter)?;
            increment_filter_parser_node(nodes)?;
            Ok(filter_expression_from_filter(filter))
        }
        _ => Err(SqlStructureReason::FilterUnsupported),
    }
}

fn increment_filter_node(nodes: &mut usize) -> Result<(), SqlStructureReason> {
    *nodes += 1;
    if *nodes > MAX_FILTER_EXPRESSION_NODES {
        return Err(SqlStructureReason::FilterUnsupported);
    }
    Ok(())
}

fn increment_filter_parser_node(nodes: &mut usize) -> Result<(), SqlStructureReason> {
    *nodes += 1;
    if *nodes > MAX_FILTER_EXPRESSION_PARSER_NODES {
        return Err(SqlStructureReason::FilterUnsupported);
    }
    Ok(())
}

fn combine_filter_expressions(
    conjunction: bool,
    left: SqlStructureFilterExpression,
    right: SqlStructureFilterExpression,
) -> SqlStructureFilterExpression {
    if conjunction {
        let mut expressions = match left {
            SqlStructureFilterExpression::And { expressions } => expressions,
            expression => vec![expression],
        };
        match right {
            SqlStructureFilterExpression::And {
                expressions: right_expressions,
            } => expressions.extend(right_expressions),
            expression => expressions.push(expression),
        }
        SqlStructureFilterExpression::And { expressions }
    } else {
        let mut expressions = match left {
            SqlStructureFilterExpression::Or { expressions } => expressions,
            expression => vec![expression],
        };
        match right {
            SqlStructureFilterExpression::Or {
                expressions: right_expressions,
            } => expressions.extend(right_expressions),
            expression => expressions.push(expression),
        }
        SqlStructureFilterExpression::Or { expressions }
    }
}

fn validate_filter_expression_shape(
    expression: &SqlStructureFilterExpression,
    depth: usize,
    nodes: &mut usize,
) -> Result<(), SqlStructureReason> {
    if depth > MAX_FILTER_EXPRESSION_DEPTH {
        return Err(SqlStructureReason::FilterUnsupported);
    }
    increment_filter_node(nodes)?;
    match expression {
        SqlStructureFilterExpression::And { expressions }
        | SqlStructureFilterExpression::Or { expressions } => {
            if !(2..=MAX_FILTER_EXPRESSION_NODES).contains(&expressions.len()) {
                return Err(SqlStructureReason::FilterUnsupported);
            }
            for expression in expressions {
                validate_filter_expression_shape(expression, depth + 1, nodes)?;
            }
        }
        SqlStructureFilterExpression::Not { expression } => {
            validate_filter_expression_shape(expression, depth + 1, nodes)?;
        }
        SqlStructureFilterExpression::Predicate { .. }
        | SqlStructureFilterExpression::ColumnComparison { .. } => {}
    }
    Ok(())
}

fn parse_having_expression(
    expression: &Expr,
    context: &SqlStructureContext,
) -> Result<SqlStructureHavingExpression, SqlStructureReason> {
    let mut parser_nodes = 0;
    let expression = parse_having_expression_inner(expression, context, 0, &mut parser_nodes)?;
    let mut tree_nodes = 0;
    validate_having_expression_shape(&expression, 0, &mut tree_nodes)?;
    Ok(expression)
}

fn parse_having_expression_inner(
    expression: &Expr,
    context: &SqlStructureContext,
    depth: usize,
    nodes: &mut usize,
) -> Result<SqlStructureHavingExpression, SqlStructureReason> {
    if depth > MAX_FILTER_EXPRESSION_PARSER_DEPTH {
        return Err(SqlStructureReason::HavingUnsupported);
    }
    match expression {
        Expr::Nested(inner) => parse_having_expression_inner(inner, context, depth + 1, nodes),
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And,
            right,
        } => {
            increment_having_parser_node(nodes)?;
            let left = parse_having_expression_inner(left, context, depth + 1, nodes)?;
            let right = parse_having_expression_inner(right, context, depth + 1, nodes)?;
            Ok(combine_having_expressions(true, left, right))
        }
        Expr::BinaryOp {
            left,
            op: BinaryOperator::Or,
            right,
        } => {
            increment_having_parser_node(nodes)?;
            let left = parse_having_expression_inner(left, context, depth + 1, nodes)?;
            let right = parse_having_expression_inner(right, context, depth + 1, nodes)?;
            Ok(combine_having_expressions(false, left, right))
        }
        Expr::UnaryOp {
            op: UnaryOperator::Not,
            expr,
        } => {
            increment_having_parser_node(nodes)?;
            Ok(SqlStructureHavingExpression::Not {
                expression: Box::new(parse_having_expression_inner(
                    expr,
                    context,
                    depth + 1,
                    nodes,
                )?),
            })
        }
        Expr::BinaryOp { left, op, right } => {
            let operator =
                having_operator_from_binary(op).ok_or(SqlStructureReason::HavingUnsupported)?;
            let Expr::Function(function) = left.as_ref() else {
                return Err(SqlStructureReason::HavingUnsupported);
            };
            let aggregate =
                parse_aggregate(function, None, context).map_err(|reason| match reason {
                    SqlStructureReason::UnknownColumn
                    | SqlStructureReason::AggregateTypeUnsupported => reason,
                    _ => SqlStructureReason::HavingUnsupported,
                })?;
            let value = parse_having_value(right)?;
            validate_having_predicate(context, &aggregate, operator, &value)?;
            increment_having_parser_node(nodes)?;
            Ok(SqlStructureHavingExpression::Predicate {
                aggregate,
                operator,
                value,
            })
        }
        _ => Err(SqlStructureReason::HavingUnsupported),
    }
}

fn parse_having_value(expression: &Expr) -> Result<SqlStructureHavingValue, SqlStructureReason> {
    match expression {
        Expr::Value(value) => match &value.value {
            Value::Number(number, long) if !long => {
                if number.parse::<i64>().is_ok() {
                    Ok(SqlStructureHavingValue::Integer(number.clone()))
                } else if valid_numeric_spelling(number) {
                    Ok(SqlStructureHavingValue::Numeric(number.clone()))
                } else {
                    Err(SqlStructureReason::HavingUnsupported)
                }
            }
            Value::SingleQuotedString(value) => Ok(SqlStructureHavingValue::String(value.clone())),
            Value::Boolean(value) => Ok(SqlStructureHavingValue::Boolean(*value)),
            Value::Placeholder(_) => placeholder_name(expression)
                .map(SqlStructureHavingValue::Variable)
                .ok_or(SqlStructureReason::InvalidVariableName),
            _ => Err(SqlStructureReason::HavingUnsupported),
        },
        Expr::TypedString(value) => {
            let Value::SingleQuotedString(value_text) = &value.value.value else {
                return Err(SqlStructureReason::HavingUnsupported);
            };
            match value.data_type {
                DataType::Date => Ok(SqlStructureHavingValue::Date(value_text.clone())),
                DataType::Timestamp(..) | DataType::TimestampNtz(..) => {
                    Ok(SqlStructureHavingValue::Timestamp(value_text.clone()))
                }
                _ => Err(SqlStructureReason::HavingUnsupported),
            }
        }
        Expr::UnaryOp { op, expr } if matches!(*op, UnaryOperator::Minus | UnaryOperator::Plus) => {
            let Expr::Value(value) = expr.as_ref() else {
                return Err(SqlStructureReason::HavingUnsupported);
            };
            let Value::Number(number, long) = &value.value else {
                return Err(SqlStructureReason::HavingUnsupported);
            };
            if *long {
                return Err(SqlStructureReason::HavingUnsupported);
            }
            let signed = if *op == UnaryOperator::Minus {
                format!("-{number}")
            } else {
                format!("+{number}")
            };
            if signed.parse::<i64>().is_ok() {
                Ok(SqlStructureHavingValue::Integer(signed))
            } else if valid_numeric_spelling(&signed) {
                Ok(SqlStructureHavingValue::Numeric(signed))
            } else {
                Err(SqlStructureReason::HavingUnsupported)
            }
        }
        _ => Err(SqlStructureReason::HavingUnsupported),
    }
}

fn valid_numeric_spelling(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let mut mantissa_digits = 0usize;
    while bytes.get(index).is_some_and(u8::is_ascii_digit) {
        mantissa_digits += 1;
        index += 1;
    }
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            mantissa_digits += 1;
            index += 1;
        }
    }
    if mantissa_digits == 0 {
        return false;
    }
    if bytes
        .get(index)
        .is_some_and(|byte| matches!(byte, b'e' | b'E'))
    {
        index += 1;
        if bytes
            .get(index)
            .is_some_and(|byte| matches!(byte, b'+' | b'-'))
        {
            index += 1;
        }
        let exponent_start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == exponent_start {
            return false;
        }
    }
    index == bytes.len()
}

fn having_operator_from_binary(operator: &BinaryOperator) -> Option<SqlStructureHavingOperator> {
    match operator {
        BinaryOperator::Eq => Some(SqlStructureHavingOperator::Eq),
        BinaryOperator::NotEq => Some(SqlStructureHavingOperator::NotEq),
        BinaryOperator::Lt => Some(SqlStructureHavingOperator::Lt),
        BinaryOperator::LtEq => Some(SqlStructureHavingOperator::LtEq),
        BinaryOperator::Gt => Some(SqlStructureHavingOperator::Gt),
        BinaryOperator::GtEq => Some(SqlStructureHavingOperator::GtEq),
        _ => None,
    }
}

fn binary_from_having_operator(operator: SqlStructureHavingOperator) -> BinaryOperator {
    match operator {
        SqlStructureHavingOperator::Eq => BinaryOperator::Eq,
        SqlStructureHavingOperator::NotEq => BinaryOperator::NotEq,
        SqlStructureHavingOperator::Lt => BinaryOperator::Lt,
        SqlStructureHavingOperator::LtEq => BinaryOperator::LtEq,
        SqlStructureHavingOperator::Gt => BinaryOperator::Gt,
        SqlStructureHavingOperator::GtEq => BinaryOperator::GtEq,
    }
}

fn combine_having_expressions(
    conjunction: bool,
    left: SqlStructureHavingExpression,
    right: SqlStructureHavingExpression,
) -> SqlStructureHavingExpression {
    if conjunction {
        let mut expressions = match left {
            SqlStructureHavingExpression::And { expressions } => expressions,
            expression => vec![expression],
        };
        match right {
            SqlStructureHavingExpression::And {
                expressions: right_expressions,
            } => expressions.extend(right_expressions),
            expression => expressions.push(expression),
        }
        SqlStructureHavingExpression::And { expressions }
    } else {
        let mut expressions = match left {
            SqlStructureHavingExpression::Or { expressions } => expressions,
            expression => vec![expression],
        };
        match right {
            SqlStructureHavingExpression::Or {
                expressions: right_expressions,
            } => expressions.extend(right_expressions),
            expression => expressions.push(expression),
        }
        SqlStructureHavingExpression::Or { expressions }
    }
}

fn increment_having_parser_node(nodes: &mut usize) -> Result<(), SqlStructureReason> {
    *nodes += 1;
    if *nodes > MAX_FILTER_EXPRESSION_PARSER_NODES {
        return Err(SqlStructureReason::HavingUnsupported);
    }
    Ok(())
}

fn increment_having_node(nodes: &mut usize) -> Result<(), SqlStructureReason> {
    *nodes += 1;
    if *nodes > MAX_FILTER_EXPRESSION_NODES {
        return Err(SqlStructureReason::HavingUnsupported);
    }
    Ok(())
}

fn validate_having_expression_shape(
    expression: &SqlStructureHavingExpression,
    depth: usize,
    nodes: &mut usize,
) -> Result<(), SqlStructureReason> {
    if depth > MAX_FILTER_EXPRESSION_DEPTH {
        return Err(SqlStructureReason::HavingUnsupported);
    }
    increment_having_node(nodes)?;
    match expression {
        SqlStructureHavingExpression::And { expressions }
        | SqlStructureHavingExpression::Or { expressions } => {
            if !(2..=MAX_FILTER_EXPRESSION_NODES).contains(&expressions.len()) {
                return Err(SqlStructureReason::HavingUnsupported);
            }
            for expression in expressions {
                validate_having_expression_shape(expression, depth + 1, nodes)?;
            }
        }
        SqlStructureHavingExpression::Not { expression } => {
            validate_having_expression_shape(expression, depth + 1, nodes)?;
        }
        SqlStructureHavingExpression::Predicate { .. } => {}
    }
    Ok(())
}

fn having_expression_variables(expression: &SqlStructureHavingExpression) -> Vec<String> {
    fn collect(expression: &SqlStructureHavingExpression, variables: &mut BTreeSet<String>) {
        match expression {
            SqlStructureHavingExpression::And { expressions }
            | SqlStructureHavingExpression::Or { expressions } => {
                for expression in expressions {
                    collect(expression, variables);
                }
            }
            SqlStructureHavingExpression::Not { expression } => collect(expression, variables),
            SqlStructureHavingExpression::Predicate {
                value: SqlStructureHavingValue::Variable(name),
                ..
            } => {
                variables.insert(name.clone());
            }
            SqlStructureHavingExpression::Predicate { .. } => {}
        }
    }

    let mut variables = BTreeSet::new();
    collect(expression, &mut variables);
    variables.into_iter().collect()
}

fn filter_expression_from_filter(filter: SqlStructureFilter) -> SqlStructureFilterExpression {
    SqlStructureFilterExpression::Predicate {
        column: filter.column,
        qualifier: filter.qualifier,
        operator: filter.operator,
        parameter: filter.parameter,
        second_parameter: filter.second_parameter,
    }
}

fn filter_from_expression(expression: &SqlStructureFilterExpression) -> Option<SqlStructureFilter> {
    match expression {
        SqlStructureFilterExpression::Predicate {
            column,
            qualifier,
            operator,
            parameter,
            second_parameter,
        } => Some(SqlStructureFilter {
            column: column.clone(),
            qualifier: qualifier.clone(),
            operator: *operator,
            parameter: parameter.clone(),
            second_parameter: second_parameter.clone(),
        }),
        _ => None,
    }
}

fn flat_conjunctive_filters(
    expression: &SqlStructureFilterExpression,
) -> Option<Vec<SqlStructureFilter>> {
    match expression {
        SqlStructureFilterExpression::And { expressions } => {
            let mut filters = Vec::new();
            for expression in expressions {
                filters.extend(flat_conjunctive_filters(expression)?);
            }
            Some(filters)
        }
        SqlStructureFilterExpression::Predicate { .. } => {
            filter_from_expression(expression).map(|filter| vec![filter])
        }
        SqlStructureFilterExpression::Or { .. }
        | SqlStructureFilterExpression::Not { .. }
        | SqlStructureFilterExpression::ColumnComparison { .. } => None,
    }
}

fn filter_expression_variables(expression: &SqlStructureFilterExpression) -> Vec<String> {
    fn collect(expression: &SqlStructureFilterExpression, variables: &mut BTreeSet<String>) {
        match expression {
            SqlStructureFilterExpression::And { expressions }
            | SqlStructureFilterExpression::Or { expressions } => {
                for expression in expressions {
                    collect(expression, variables);
                }
            }
            SqlStructureFilterExpression::Not { expression } => collect(expression, variables),
            SqlStructureFilterExpression::Predicate {
                parameter: Some(parameter),
                second_parameter,
                ..
            } => {
                variables.insert(parameter.clone());
                if let Some(second_parameter) = second_parameter {
                    variables.insert(second_parameter.clone());
                }
            }
            SqlStructureFilterExpression::Predicate {
                parameter: None, ..
            }
            | SqlStructureFilterExpression::ColumnComparison { .. } => {}
        }
    }

    let mut variables = BTreeSet::new();
    collect(expression, &mut variables);
    variables.into_iter().collect()
}

fn parse_order_by(
    order_by: &OrderBy,
    select: &Select,
    context: &SqlStructureContext,
    joins: &[SqlStructureJoin],
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
            if expression.with_fill.is_some() {
                return Err(SqlStructureReason::SortUnsupported);
            }
            if matches!(
                expression.options.sort.as_ref(),
                Some(OrderBySort::Using(_))
            ) {
                return Err(SqlStructureReason::SortUnsupported);
            }
            let (column, qualifier, projection_alias) = if joins.is_empty() {
                if let Expr::Identifier(identifier) = &expression.expr {
                    if let Some((column, alias)) =
                        projection_alias_binding(select, identifier, context)?
                    {
                        (column, None, Some(alias))
                    } else {
                        let (column, qualifier) = resolve_column_reference(
                            &expression.expr,
                            context,
                            joins,
                            SqlStructureReason::SortUnsupported,
                        )?;
                        (column, qualifier, None)
                    }
                } else {
                    let (column, qualifier) = resolve_column_reference(
                        &expression.expr,
                        context,
                        joins,
                        SqlStructureReason::SortUnsupported,
                    )?;
                    (column, qualifier, None)
                }
            } else {
                let (column, qualifier) = resolve_column_reference(
                    &expression.expr,
                    context,
                    joins,
                    SqlStructureReason::SortUnsupported,
                )?;
                (column, qualifier, None)
            };
            let key = (qualifier.as_ref().map(alias_resolution_key), column.clone());
            if !seen.insert(key) {
                return Err(SqlStructureReason::DuplicateColumn);
            }
            let column = find_column(context, &column).ok_or(SqlStructureReason::UnknownColumn)?;
            if !column_type_supports_sort(column.column_type) {
                return Err(SqlStructureReason::SortUnsupported);
            }
            Ok(SqlStructureSort {
                column: column.name.clone(),
                qualifier,
                projection_alias,
                direction: if expression.options.sort.as_ref() == Some(&OrderBySort::Desc) {
                    SqlStructureSortDirection::Desc
                } else {
                    SqlStructureSortDirection::Asc
                },
                nulls_first: expression.options.nulls_first,
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
            let column = projection_alias_binding(select, identifier, context)?
                .map(|(column, _)| find_column(context, &column))
                .unwrap_or_else(|| find_column_by_ident(context, identifier))
                .ok_or(SqlStructureReason::UnknownColumn)?;
            if !group_by.iter().any(|group| group == &column.name) {
                return Err(SqlStructureReason::SortUnsupported);
            }
        }
    }
    Ok(())
}

/// Resolve an unqualified ORDER BY name against explicit simple projection
/// aliases before looking for an input Form column. SQL gives output names
/// precedence here, including when an alias shadows a source column.
fn projection_alias_binding(
    select: &Select,
    identifier: &Ident,
    context: &SqlStructureContext,
) -> Result<Option<(String, SqlStructureAlias)>, SqlStructureReason> {
    let mut binding = None;
    for item in &select.projection {
        let SelectItem::ExprWithAlias {
            expr: Expr::Identifier(source),
            alias,
        } = item
        else {
            continue;
        };
        let alias = SqlStructureAlias {
            name: alias.value.clone(),
            quoted: alias.quote_style.is_some(),
        };
        if !identifier_matches_alias(identifier, &alias) {
            continue;
        }
        if binding.is_some() {
            return Err(SqlStructureReason::SortUnsupported);
        }
        let column =
            find_column_by_ident(context, source).ok_or(SqlStructureReason::UnknownColumn)?;
        binding = Some((column.name.clone(), alias));
    }
    Ok(binding)
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
        SqlStructureFilterOperator::In
        | SqlStructureFilterOperator::NotIn
        | SqlStructureFilterOperator::Between
        | SqlStructureFilterOperator::NotBetween
        | SqlStructureFilterOperator::Like
        | SqlStructureFilterOperator::NotLike
        | SqlStructureFilterOperator::Ilike
        | SqlStructureFilterOperator::NotIlike
        | SqlStructureFilterOperator::IsNull
        | SqlStructureFilterOperator::IsNotNull => None,
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
            if filter.parameter.is_some() || filter.second_parameter.is_some() {
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
    let is_between = matches!(
        filter.operator,
        SqlStructureFilterOperator::Between | SqlStructureFilterOperator::NotBetween
    );
    if is_between {
        let second_parameter = filter
            .second_parameter
            .as_deref()
            .filter(|name| valid_variable_name(name))
            .ok_or(SqlStructureReason::InvalidVariableName)?;
        let second_variable = context
            .variables
            .iter()
            .find(|variable| variable.name == second_parameter)
            .ok_or(SqlStructureReason::UnknownVariable)?;
        if second_variable.variable_type.column_type() != column.column_type {
            return Err(SqlStructureReason::VariableTypeMismatch);
        }
    } else if filter.second_parameter.is_some() {
        return Err(SqlStructureReason::FilterUnsupported);
    }
    if !column_type_supports_operator(column.column_type, filter.operator) {
        return Err(SqlStructureReason::FilterTypeUnsupported);
    }
    Ok(())
}

fn validate_column_comparison(
    context: &SqlStructureContext,
    joins: &[SqlStructureJoin],
    left: &SqlStructureColumnReference,
    operator: SqlStructureFilterOperator,
    right: &SqlStructureColumnReference,
) -> Result<(), SqlStructureReason> {
    if !matches!(
        operator,
        SqlStructureFilterOperator::Eq
            | SqlStructureFilterOperator::NotEq
            | SqlStructureFilterOperator::Lt
            | SqlStructureFilterOperator::LtEq
            | SqlStructureFilterOperator::Gt
            | SqlStructureFilterOperator::GtEq
    ) {
        return Err(SqlStructureReason::FilterUnsupported);
    }
    resolve_filter_qualifier(left.qualifier.as_ref(), joins)?;
    resolve_filter_qualifier(right.qualifier.as_ref(), joins)?;
    let left_column =
        find_column(context, &left.column).ok_or(SqlStructureReason::UnknownColumn)?;
    let right_column =
        find_column(context, &right.column).ok_or(SqlStructureReason::UnknownColumn)?;
    if left_column.column_type != right_column.column_type
        || matches!(
            left_column.column_type,
            SqlStructureColumnType::Json | SqlStructureColumnType::Other
        )
    {
        return Err(SqlStructureReason::VariableTypeMismatch);
    }
    if !column_type_supports_operator(left_column.column_type, operator) {
        return Err(SqlStructureReason::FilterTypeUnsupported);
    }
    Ok(())
}

fn column_type_supports_operator(
    column_type: SqlStructureColumnType,
    operator: SqlStructureFilterOperator,
) -> bool {
    let is_like = matches!(
        operator,
        SqlStructureFilterOperator::Like
            | SqlStructureFilterOperator::NotLike
            | SqlStructureFilterOperator::Ilike
            | SqlStructureFilterOperator::NotIlike
    );
    let is_membership = matches!(
        operator,
        SqlStructureFilterOperator::In | SqlStructureFilterOperator::NotIn
    );
    let is_between = matches!(
        operator,
        SqlStructureFilterOperator::Between | SqlStructureFilterOperator::NotBetween
    );
    if is_like {
        return column_type == SqlStructureColumnType::String;
    }
    match column_type {
        SqlStructureColumnType::String => true,
        SqlStructureColumnType::Integer
        | SqlStructureColumnType::Float
        | SqlStructureColumnType::Date
        | SqlStructureColumnType::Timestamp
        | SqlStructureColumnType::Decimal => true,
        SqlStructureColumnType::Boolean => {
            is_membership
                || (!is_between
                    && matches!(
                        operator,
                        SqlStructureFilterOperator::Eq | SqlStructureFilterOperator::NotEq
                    ))
        }
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
    aliases: Vec<Option<SqlStructureAlias>>,
    existing_aliases: &[SqlStructureProjectionAlias],
) -> Result<String, SqlStructureReason> {
    let columns = validate_schema_columns(context, &columns)?;
    let SetExpr::Select(select) = query.body.as_ref() else {
        return Err(SqlStructureReason::SetOperationUnsupported);
    };
    if select
        .from
        .iter()
        .any(|relation| !relation.joins.is_empty())
    {
        return Err(SqlStructureReason::JoinUnsupported);
    }
    if select.projection.iter().any(select_item_is_aggregate) {
        return Err(SqlStructureReason::ProjectionUnsupported);
    }

    let aliases = if aliases.is_empty() {
        columns
            .iter()
            .map(|column| {
                let matching = existing_aliases
                    .iter()
                    .filter(|projection| projection.column == *column)
                    .collect::<Vec<_>>();
                (matching.len() == 1)
                    .then(|| matching[0].alias.clone())
                    .flatten()
            })
            .collect::<Vec<_>>()
    } else if aliases.len() == columns.len() {
        aliases
    } else {
        return Err(SqlStructureReason::ProjectionUnsupported);
    };
    if aliases
        .iter()
        .flatten()
        .any(|alias| !valid_projection_alias(alias))
    {
        return Err(SqlStructureReason::ProjectionUnsupported);
    }

    ensure_projection_alias_sort_binding_is_preserved(
        query,
        context,
        &select.projection,
        &columns,
        &aliases,
    )?;

    let current_columns = existing_aliases
        .iter()
        .map(|projection| projection.column.as_str())
        .collect::<Vec<_>>();
    if current_columns == columns.iter().map(String::as_str).collect::<Vec<_>>()
        && existing_aliases.len() == select.projection.len()
    {
        return rewrite_projection_aliases(sql, &select.projection, &aliases);
    }

    let source_range =
        projection_source_range(sql, query).ok_or(SqlStructureReason::ProjectionUnsupported)?;
    let replacement = columns
        .into_iter()
        .zip(aliases)
        .map(|(column, alias)| {
            let column = Ident::with_quote('"', column).to_string();
            match alias {
                Some(alias) => format!("{column} AS {}", sql_alias_to_string(alias)),
                None => column,
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    replace_sql_source_range(sql, source_range, &replacement)
        .ok_or(SqlStructureReason::ProjectionUnsupported)
}

fn rewrite_literal_projection(
    sql: &str,
    query: &Query,
    context: &SqlStructureContext,
    values: Vec<SqlStructureLiteral>,
) -> Result<String, SqlStructureReason> {
    if !context.relation.is_empty() || values.is_empty() || values.len() > 256 {
        return Err(SqlStructureReason::ProjectionUnsupported);
    }
    let SetExpr::Select(select) = query.body.as_ref() else {
        return Err(SqlStructureReason::SetOperationUnsupported);
    };
    if !select.from.is_empty() {
        return Err(SqlStructureReason::RelationUnsupported);
    }
    let source_range = literal_projection_source_range(sql, query)
        .ok_or(SqlStructureReason::ProjectionUnsupported)?;
    let replacement = values
        .into_iter()
        .map(|value| {
            let sql_value = match value {
                SqlStructureLiteral::Integer(value) => {
                    if value.parse::<i64>().is_err() {
                        return Err(SqlStructureReason::ProjectionUnsupported);
                    }
                    Value::Number(value, false)
                }
                SqlStructureLiteral::String(value) => Value::SingleQuotedString(value),
                SqlStructureLiteral::Boolean(value) => Value::Boolean(value),
                SqlStructureLiteral::Null => Value::Null,
            };
            Ok(sql_value.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    replace_sql_source_range(sql, source_range, &replacement)
        .ok_or(SqlStructureReason::ProjectionUnsupported)
}

fn literal_projection_source_range(sql: &str, query: &Query) -> Option<SqlSourceRange> {
    let mut range = projection_source_range(sql, query)?;
    let SetExpr::Select(select) = query.body.as_ref() else {
        return None;
    };
    let SelectItem::UnnamedExpr(Expr::UnaryOp { op, .. }) = select.projection.first()? else {
        return Some(range);
    };
    if !matches!(op, UnaryOperator::Minus | UnaryOperator::Plus) {
        return Some(range);
    }

    // sqlparser's unary-expression span starts at the operand, so include the
    // leading sign when replacing a projection such as `SELECT -1`.
    let prefix = sql.get(..range.start)?;
    let sign = prefix
        .char_indices()
        .rev()
        .find(|(_, ch)| !ch.is_ascii_whitespace());
    let (sign_start, sign_char) = sign?;
    let expected = if *op == UnaryOperator::Minus {
        '-'
    } else {
        '+'
    };
    if sign_char != expected {
        return None;
    }
    range.start = sign_start;
    Some(range)
}

fn rewrite_projection_slots(
    sql: &str,
    query: &Query,
    context: &SqlStructureContext,
    slots: Vec<SqlStructureProjectionSlotEdit>,
    existing_slots: &[SqlStructureProjectionAlias],
) -> Result<String, SqlStructureReason> {
    if slots.is_empty() {
        return Err(SqlStructureReason::EmptyProjection);
    }
    if slots.len() > 256 {
        return Err(SqlStructureReason::ProjectionUnsupported);
    }
    let SetExpr::Select(select) = query.body.as_ref() else {
        return Err(SqlStructureReason::SetOperationUnsupported);
    };
    if select
        .from
        .iter()
        .any(|relation| !relation.joins.is_empty())
    {
        return Err(SqlStructureReason::JoinUnsupported);
    }
    let wildcard_projection = select.projection.len() == 1
        && matches!(
            &select.projection[0],
            SelectItem::Wildcard(_) | SelectItem::QualifiedWildcard(_, _)
        );
    if select.projection.iter().any(select_item_is_aggregate)
        || (!wildcard_projection && select.projection.len() != existing_slots.len())
        || (wildcard_projection && !existing_slots.is_empty())
    {
        return Err(SqlStructureReason::ProjectionUnsupported);
    }

    let mut used_source_ids = BTreeSet::new();
    let mut columns = Vec::with_capacity(slots.len());
    let mut aliases = Vec::with_capacity(slots.len());
    let mut preserves_order = slots.len() == existing_slots.len();
    for (output_index, slot) in slots.into_iter().enumerate() {
        if let Some(id) = &slot.id {
            if wildcard_projection {
                return Err(SqlStructureReason::ProjectionUnsupported);
            }
            let Some(source_index) = existing_slots
                .iter()
                .enumerate()
                .find_map(|(index, _)| (projection_slot_id(index) == *id).then_some(index))
            else {
                return Err(SqlStructureReason::ProjectionUnsupported);
            };
            if !used_source_ids.insert(source_index) {
                return Err(SqlStructureReason::ProjectionUnsupported);
            }
            preserves_order &= source_index == output_index;
        } else {
            preserves_order = false;
        }
        let column = find_column(context, &slot.column).ok_or(SqlStructureReason::UnknownColumn)?;
        if slot
            .alias
            .as_ref()
            .is_some_and(|alias| !valid_projection_alias(alias))
        {
            return Err(SqlStructureReason::ProjectionUnsupported);
        }
        columns.push(column.name.clone());
        aliases.push(slot.alias);
    }

    ensure_projection_alias_sort_binding_is_preserved(
        query,
        context,
        &select.projection,
        &columns,
        &aliases,
    )?;

    if preserves_order
        && existing_slots
            .iter()
            .zip(&columns)
            .all(|(existing, column)| existing.column == *column)
    {
        return rewrite_projection_aliases(sql, &select.projection, &aliases);
    }

    let source_range =
        projection_source_range(sql, query).ok_or(SqlStructureReason::ProjectionUnsupported)?;
    let replacement = columns
        .into_iter()
        .zip(aliases)
        .map(|(column, alias)| {
            let column = Ident::with_quote('"', column).to_string();
            match alias {
                Some(alias) => format!("{column} AS {}", sql_alias_to_string(alias)),
                None => column,
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    replace_sql_source_range(sql, source_range, &replacement)
        .ok_or(SqlStructureReason::ProjectionUnsupported)
}

fn projection_slot_id(index: usize) -> String {
    format!("projection-{index}")
}

fn rewrite_projection_aliases(
    sql: &str,
    projection: &[SelectItem],
    aliases: &[Option<SqlStructureAlias>],
) -> Result<String, SqlStructureReason> {
    if projection.is_empty() || projection.len() != aliases.len() {
        return Err(SqlStructureReason::ProjectionUnsupported);
    }
    let mut edits = Vec::new();
    for (item, requested_alias) in projection.iter().zip(aliases) {
        let (expression, current_alias) = match item {
            SelectItem::UnnamedExpr(Expr::Identifier(expression)) => (expression, None),
            SelectItem::ExprWithAlias {
                expr: Expr::Identifier(expression),
                alias,
            } => (expression, Some(alias)),
            _ => return Err(SqlStructureReason::ProjectionUnsupported),
        };
        if requested_alias
            .as_ref()
            .is_some_and(|alias| !valid_projection_alias(alias))
        {
            return Err(SqlStructureReason::ProjectionUnsupported);
        }
        let current_alias = current_alias.map(|alias| SqlStructureAlias {
            name: alias.value.clone(),
            quoted: alias.quote_style.is_some(),
        });
        if current_alias.as_ref() == requested_alias.as_ref() {
            continue;
        }

        let expression_range = source_range_from_span(sql, expression.span)
            .ok_or(SqlStructureReason::ProjectionUnsupported)?;
        if let Some(current_alias_ast) = match item {
            SelectItem::ExprWithAlias {
                expr: Expr::Identifier(_),
                alias,
            } => Some(alias),
            _ => None,
        } {
            let alias_range = source_range_from_span(sql, current_alias_ast.span)
                .ok_or(SqlStructureReason::ProjectionUnsupported)?;
            if alias_range.start < expression_range.end
                || has_comment_token_in_range(
                    sql,
                    SqlSourceRange {
                        start: expression_range.end,
                        end: alias_range.start,
                    },
                ) == Some(true)
            {
                return Err(SqlStructureReason::ProjectionUnsupported);
            }
            match requested_alias {
                Some(alias) => {
                    if has_comment_token_in_range(sql, alias_range) != Some(false) {
                        return Err(SqlStructureReason::ProjectionUnsupported);
                    }
                    edits.push((alias_range, sql_alias_to_string(alias.clone())));
                }
                None => {
                    let range = SqlSourceRange {
                        start: expression_range.end,
                        end: alias_range.end,
                    };
                    if has_comment_token_in_range(sql, range) != Some(false) {
                        return Err(SqlStructureReason::ProjectionUnsupported);
                    }
                    edits.push((range, String::new()));
                }
            }
        } else if let Some(alias) = requested_alias {
            if comment_follows_source_offset(sql, expression_range.end) != Some(false) {
                return Err(SqlStructureReason::ProjectionUnsupported);
            }
            edits.push((
                SqlSourceRange {
                    start: expression_range.end,
                    end: expression_range.end,
                },
                format!(" AS {}", sql_alias_to_string(alias.clone())),
            ));
        }
    }
    apply_sql_source_edits(sql, edits).ok_or(SqlStructureReason::ProjectionUnsupported)
}

/// Reject a projection edit when an ORDER BY identifier would resolve to a
/// different selected Form column after output aliases change. SQL gives a
/// matching output alias precedence over a same-named input column in ORDER
/// BY, so resolving the identifier only against Form metadata is insufficient.
fn ensure_projection_alias_sort_binding_is_preserved(
    query: &Query,
    context: &SqlStructureContext,
    projection: &[SelectItem],
    requested_columns: &[String],
    requested_aliases: &[Option<SqlStructureAlias>],
) -> Result<(), SqlStructureReason> {
    let Some(order_by) = &query.order_by else {
        return Ok(());
    };
    let OrderByKind::Expressions(expressions) = &order_by.kind else {
        return Err(SqlStructureReason::SortUnsupported);
    };
    if requested_columns.len() != requested_aliases.len() {
        return Err(SqlStructureReason::ProjectionUnsupported);
    }

    let (current_projected_columns, current_aliases) = if projection.len() == 1
        && matches!(&projection[0], SelectItem::Wildcard(options)
            if options.opt_ilike.is_none()
                && options.opt_exclude.is_none()
                && options.opt_except.is_none()
                && options.opt_replace.is_none()
                && options.opt_rename.is_none()
                && options.opt_alias.is_none())
    {
        // SELECT * has no editable output aliases. Its ORDER BY identifiers
        // resolve to the corresponding Form columns before projection editing.
        (Vec::new(), Vec::new())
    } else {
        let mut projected_columns = Vec::with_capacity(projection.len());
        let mut aliases = Vec::with_capacity(projection.len());
        for item in projection {
            let (expression, alias) = match item {
                SelectItem::UnnamedExpr(Expr::Identifier(expression)) => (expression, None),
                SelectItem::ExprWithAlias {
                    expr: Expr::Identifier(expression),
                    alias,
                } => (expression, Some(alias)),
                _ => return Err(SqlStructureReason::ProjectionUnsupported),
            };
            let column = find_column_by_ident(context, expression)
                .ok_or(SqlStructureReason::UnknownColumn)?;
            projected_columns.push(column.name.clone());
            aliases.push(alias.map(|alias| SqlStructureAlias {
                name: alias.value.clone(),
                quoted: alias.quote_style.is_some(),
            }));
        }
        (projected_columns, aliases)
    };

    for order in expressions {
        let Expr::Identifier(identifier) = &order.expr else {
            // The bounded editor accepts simple column names here; other
            // supported ordering forms cannot be affected by output aliases.
            continue;
        };
        let resolve = |aliases: &[Option<SqlStructureAlias>], columns: &[String]| {
            let matches = aliases
                .iter()
                .enumerate()
                .filter_map(|(index, alias)| {
                    alias
                        .as_ref()
                        .filter(|alias| identifier_matches_alias(identifier, alias))
                        .map(|_| index)
                })
                .collect::<Vec<_>>();
            match matches.as_slice() {
                [index] => columns
                    .get(*index)
                    .cloned()
                    .ok_or(SqlStructureReason::SortUnsupported),
                [] => find_column_by_ident(context, identifier)
                    .map(|column| column.name.clone())
                    .ok_or(SqlStructureReason::SortUnsupported),
                _ => Err(SqlStructureReason::SortUnsupported),
            }
        };
        if resolve(&current_aliases, &current_projected_columns)?
            != resolve(requested_aliases, requested_columns)?
        {
            return Err(SqlStructureReason::SortUnsupported);
        }
    }
    Ok(())
}

fn sql_alias_to_string(alias: SqlStructureAlias) -> String {
    if alias.quoted {
        Ident::with_quote('"', alias.name).to_string()
    } else {
        Ident::new(alias.name).to_string()
    }
}

fn comment_follows_source_offset(sql: &str, offset: usize) -> Option<bool> {
    let document = crate::sql_document::parse_sql_document(sql);
    if document.status != crate::sql_document::SqlDocumentStatus::Parsed {
        return None;
    }
    for token in document
        .tokens
        .iter()
        .filter(|token| token.range.start >= offset)
    {
        match token.kind {
            crate::sql_document::SqlDocumentTokenKind::Trivia => continue,
            crate::sql_document::SqlDocumentTokenKind::Comment => return Some(true),
            _ => return Some(false),
        }
    }
    Some(false)
}

fn apply_sql_source_edits(sql: &str, mut edits: Vec<(SqlSourceRange, String)>) -> Option<String> {
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let mut result = sql.to_owned();
    let mut previous_start = sql.len().checked_add(1)?;
    for (range, replacement) in edits {
        if range.start > range.end || range.end > sql.len() || range.end > previous_start {
            return None;
        }
        let before = result.get(..range.start)?;
        let after = result.get(range.end..)?;
        result = format!("{before}{replacement}{after}");
        previous_start = range.start;
    }
    Some(result)
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

fn resolve_filter_qualifier(
    qualifier: Option<&SqlStructureAlias>,
    joins: &[SqlStructureJoin],
) -> Result<Option<SqlStructureAlias>, SqlStructureReason> {
    match (qualifier, joins) {
        (None, []) => Ok(None),
        (Some(qualifier), [join]) if aliases_resolve_same(qualifier, &join.left_alias) => {
            Ok(Some(join.left_alias.clone()))
        }
        (Some(qualifier), [join]) if aliases_resolve_same(qualifier, &join.right_alias) => {
            Ok(Some(join.right_alias.clone()))
        }
        _ => Err(SqlStructureReason::JoinUnsupported),
    }
}

fn column_reference_expr(column: &str, qualifier: Option<&SqlStructureAlias>) -> Expr {
    let column = Ident::with_quote('"', column.to_owned());
    match qualifier {
        Some(alias) => Expr::CompoundIdentifier(vec![alias_ast_ident(alias), column]),
        None => Expr::Identifier(column),
    }
}

fn alias_reference_expr(alias: &SqlStructureAlias) -> Expr {
    Expr::Identifier(if alias.quoted {
        Ident::with_quote('"', alias.name.clone())
    } else {
        Ident::new(alias.name.clone())
    })
}

fn alias_ast_ident(alias: &SqlStructureAlias) -> Ident {
    if alias.quoted {
        Ident::with_quote('"', alias.name.clone())
    } else {
        Ident::new(alias.name.clone())
    }
}

fn select_ref(query: &Query) -> Result<&Select, SqlStructureReason> {
    match query.body.as_ref() {
        SetExpr::Select(select) => Ok(select),
        _ => Err(SqlStructureReason::SetOperationUnsupported),
    }
}

fn document_query_and_select_ids(
    sql: &str,
    query: &Query,
    select: &Select,
) -> Option<(crate::sql_document::SqlDocument, u32, u32)> {
    let document = parse_sql_document(sql);
    if document.status != SqlDocumentStatus::Parsed {
        return None;
    }
    let query_range = source_range_from_span(sql, query.span())?;
    let select_range = source_range_from_span(sql, select.span())?;
    let query_id = document
        .nodes
        .iter()
        .find(|node| node.kind == SqlDocumentNodeKind::Query && node.range == Some(query_range))?
        .id;
    let select_id = document
        .nodes
        .iter()
        .find(|node| node.kind == SqlDocumentNodeKind::Select && node.range == Some(select_range))?
        .id;
    Some((document, query_id, select_id))
}

fn rewrite_query_clause(
    sql: &str,
    query: &Query,
    select: &Select,
    kind: SqlDocumentNodeKind,
    belongs_to_select: bool,
    replacement: Option<&str>,
    insert_before: &[SqlDocumentNodeKind],
    reason: SqlStructureReason,
) -> Result<String, SqlStructureReason> {
    let (document, query_id, select_id) =
        document_query_and_select_ids(sql, query, select).ok_or(reason)?;
    let parent_id = if belongs_to_select {
        select_id
    } else {
        query_id
    };
    let existing = document
        .nodes
        .iter()
        .find(|node| node.kind == kind && node.parent == Some(parent_id))
        .and_then(|node| node.range);

    if let Some(existing) = existing {
        if has_comment_token_in_range(sql, existing) != Some(false) {
            return Err(reason);
        }
        return match replacement {
            Some(replacement) => replace_sql_source_range(sql, existing, replacement).ok_or(reason),
            None => replace_sql_source_range(sql, existing, "").ok_or(reason),
        };
    }

    let Some(replacement) = replacement else {
        return Ok(sql.to_owned());
    };
    let query_range = source_range_from_span(sql, query.span()).ok_or(reason)?;
    let later_clause_start = document
        .nodes
        .iter()
        .filter(|node| {
            insert_before.contains(&node.kind)
                && (node.parent == Some(query_id) || node.parent == Some(select_id))
        })
        .filter_map(|node| node.range.map(|range| range.start))
        .filter(|start| *start >= query_range.start && *start < query_range.end)
        .min();
    let insertion = later_clause_start.unwrap_or(query_range.end);
    let replacement = if later_clause_start.is_some() {
        format!("{replacement} ")
    } else {
        format!(" {replacement}")
    };
    insert_sql_source_offset(sql, insertion, &replacement).ok_or(reason)
}

fn rewrite_where_clause(
    sql: &str,
    query: &Query,
    replacement: Option<&str>,
) -> Result<String, SqlStructureReason> {
    let select = select_ref(query)?;
    match (select.selection.as_ref(), replacement) {
        (Some(selection), Some(replacement)) => {
            let range = source_range_from_span(sql, selection.span())
                .ok_or(SqlStructureReason::FilterUnsupported)?;
            if has_comment_token_in_range(sql, range) != Some(false) {
                return Err(SqlStructureReason::FilterUnsupported);
            }
            replace_sql_source_range(sql, range, replacement)
                .ok_or(SqlStructureReason::FilterUnsupported)
        }
        (Some(_), None) => rewrite_query_clause(
            sql,
            query,
            select,
            SqlDocumentNodeKind::WhereClause,
            true,
            None,
            &[],
            SqlStructureReason::FilterUnsupported,
        ),
        (None, Some(replacement)) => {
            let anchor = select
                .from
                .last()
                .map(Spanned::span)
                .or_else(|| select.projection.last().map(Spanned::span))
                .ok_or(SqlStructureReason::FilterUnsupported)?;
            let range =
                source_range_from_span(sql, anchor).ok_or(SqlStructureReason::FilterUnsupported)?;
            insert_sql_source_offset(sql, range.end, &format!(" WHERE {replacement}"))
                .ok_or(SqlStructureReason::FilterUnsupported)
        }
        (None, None) => Ok(sql.to_owned()),
    }
}

fn rewrite_order_by_clause(
    sql: &str,
    original_query: &Query,
    proposed_query: &Query,
) -> Result<String, SqlStructureReason> {
    let original_select = select_ref(original_query)?;
    let proposed = proposed_query.order_by.as_ref().map(ToString::to_string);
    rewrite_query_clause(
        sql,
        original_query,
        original_select,
        SqlDocumentNodeKind::OrderByClause,
        false,
        proposed.as_deref(),
        &[SqlDocumentNodeKind::LimitClause],
        SqlStructureReason::SortUnsupported,
    )
}

fn rewrite_group_by_clause(
    sql: &str,
    original_query: &Query,
    proposed_query: &Query,
) -> Result<String, SqlStructureReason> {
    let original_select = select_ref(original_query)?;
    let proposed_select = select_ref(proposed_query)?;
    let replacement = match &proposed_select.group_by {
        GroupByExpr::Expressions(columns, modifiers)
            if columns.is_empty() && modifiers.is_empty() =>
        {
            None
        }
        group_by => Some(group_by.to_string()),
    };
    rewrite_query_clause(
        sql,
        original_query,
        original_select,
        SqlDocumentNodeKind::GroupByClause,
        true,
        replacement.as_deref(),
        &[
            SqlDocumentNodeKind::HavingClause,
            SqlDocumentNodeKind::OrderByClause,
            SqlDocumentNodeKind::LimitClause,
        ],
        SqlStructureReason::GroupByUnsupported,
    )
}

fn rewrite_having_clause(
    sql: &str,
    original_query: &Query,
    proposed_query: &Query,
) -> Result<String, SqlStructureReason> {
    let original_select = select_ref(original_query)?;
    let proposed_select = select_ref(proposed_query)?;
    let replacement = proposed_select
        .having
        .as_ref()
        .map(|expression| format!("HAVING {expression}"));
    let Some(range) = having_clause_source_range(sql, original_query, original_select)? else {
        return rewrite_query_clause(
            sql,
            original_query,
            original_select,
            SqlDocumentNodeKind::HavingClause,
            true,
            replacement.as_deref(),
            &[
                SqlDocumentNodeKind::OrderByClause,
                SqlDocumentNodeKind::LimitClause,
            ],
            SqlStructureReason::HavingUnsupported,
        );
    };
    if has_comment_token_in_range(sql, range) != Some(false) {
        return Err(SqlStructureReason::HavingUnsupported);
    }
    replace_sql_source_range(sql, range, replacement.as_deref().unwrap_or(""))
        .ok_or(SqlStructureReason::HavingUnsupported)
}

/// The SQL parser may put a nested expression's span inside its surrounding
/// parentheses. Extend the parser-derived HAVING node through only the
/// unmatched closing delimiters after that span, so clearing/replacing the
/// clause cannot leave a dangling `)` in source SQL.
fn having_clause_source_range(
    sql: &str,
    query: &Query,
    select: &Select,
) -> Result<Option<SqlSourceRange>, SqlStructureReason> {
    if select.having.is_none() {
        return Ok(None);
    }
    let Some((document, _query_id, select_id)) = document_query_and_select_ids(sql, query, select)
    else {
        return Err(SqlStructureReason::HavingUnsupported);
    };
    let Some(mut range) = document
        .nodes
        .iter()
        .find(|node| {
            node.kind == SqlDocumentNodeKind::HavingClause && node.parent == Some(select_id)
        })
        .and_then(|node| node.range)
    else {
        return Err(SqlStructureReason::HavingUnsupported);
    };

    let mut open_parentheses = 0usize;
    for token in &document.tokens {
        if token.range.start < range.start || token.range.end > range.end {
            continue;
        }
        if token.kind != SqlDocumentTokenKind::Punctuation {
            continue;
        }
        match sql.get(token.range.start..token.range.end) {
            Some("(") => open_parentheses += 1,
            Some(")") => {
                open_parentheses = open_parentheses
                    .checked_sub(1)
                    .ok_or(SqlStructureReason::HavingUnsupported)?;
            }
            _ => {}
        }
    }

    let span_end = range.end;
    let mut next_token = document
        .tokens
        .iter()
        .filter(|token| token.range.start >= span_end);
    while open_parentheses > 0 {
        let token = next_token
            .find(|token| {
                !matches!(
                    token.kind,
                    SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
                )
            })
            .ok_or(SqlStructureReason::HavingUnsupported)?;
        if token.kind != SqlDocumentTokenKind::Punctuation
            || sql.get(token.range.start..token.range.end) != Some(")")
        {
            return Err(SqlStructureReason::HavingUnsupported);
        }
        range.end = token.range.end;
        open_parentheses -= 1;
    }
    Ok(Some(range))
}

fn rewrite_projection_from_ast(
    sql: &str,
    original_query: &Query,
    proposed_query: &Query,
) -> Result<String, SqlStructureReason> {
    let proposed_select = select_ref(proposed_query)?;
    let (range, _) = document_projection_ranges(sql, original_query)
        .ok_or(SqlStructureReason::AggregateUnsupported)?;
    let mut projection = Vec::with_capacity(proposed_select.projection.len());
    for item in &proposed_select.projection {
        let item_span = item.span();
        let item_range = (item_span != Span::empty())
            .then(|| source_range_from_span(sql, item_span))
            .flatten();
        let original = item_range
            .filter(|item_range| range.start <= item_range.start && item_range.end <= range.end)
            .and_then(|item_range| sql.get(item_range.start..item_range.end));
        projection.push(original.map_or_else(|| item.to_string(), str::to_owned));
    }
    if projection.is_empty() {
        return Err(SqlStructureReason::EmptyProjection);
    }
    replace_sql_source_range(sql, range, &projection.join(", "))
        .ok_or(SqlStructureReason::AggregateUnsupported)
}

fn document_projection_ranges(
    sql: &str,
    query: &Query,
) -> Option<(SqlSourceRange, Vec<SqlSourceRange>)> {
    let select = select_ref(query).ok()?;
    let (document, _, select_id) = document_query_and_select_ids(sql, query, select)?;
    let select_list_id = document
        .nodes
        .iter()
        .find(|node| {
            node.kind == SqlDocumentNodeKind::SelectList && node.parent == Some(select_id)
        })?
        .id;
    let mut ranges = document
        .nodes
        .iter()
        .filter(|node| {
            node.kind == SqlDocumentNodeKind::SelectItem && node.parent == Some(select_list_id)
        })
        .filter_map(|node| node.range)
        .collect::<Vec<_>>();
    ranges.sort_by_key(|range| range.start);
    if ranges.len() != select.projection.len() {
        return None;
    }
    let range = SqlSourceRange {
        start: ranges.first()?.start,
        end: ranges.last()?.end,
    };
    if has_comment_token_in_range(sql, range)? {
        return None;
    }
    Some((range, ranges))
}

fn rewrite_distinct(
    sql: &str,
    query: &Query,
    distinct: bool,
) -> Result<String, SqlStructureReason> {
    let select = select_ref(query)?;
    let select_keyword = source_range_from_span(sql, select.select_token.0.span)
        .ok_or(SqlStructureReason::DistinctUnsupported)?;
    match (select.distinct.as_ref(), distinct) {
        (Some(Distinct::On(_)), _) => Err(SqlStructureReason::DistinctUnsupported),
        (Some(Distinct::Distinct), true) | (Some(Distinct::All), false) | (None, false) => {
            Ok(sql.to_owned())
        }
        (None, true) => insert_sql_source_offset(sql, select_keyword.end, " DISTINCT")
            .ok_or(SqlStructureReason::DistinctUnsupported),
        (Some(Distinct::Distinct), false) | (Some(Distinct::All), true) => {
            let document = parse_sql_document(sql);
            if document.status != SqlDocumentStatus::Parsed {
                return Err(SqlStructureReason::DistinctUnsupported);
            }
            let projection_start = select
                .projection
                .first()
                .and_then(|item| source_range_from_span(sql, item.span()))
                .map(|range| range.start)
                .ok_or(SqlStructureReason::DistinctUnsupported)?;
            let modifier = if matches!(select.distinct.as_ref(), Some(Distinct::All)) {
                "ALL"
            } else {
                "DISTINCT"
            };
            let distinct_range = document
                .tokens
                .iter()
                .find(|token| {
                    token.kind == SqlDocumentTokenKind::Word
                        && token.range.start >= select_keyword.end
                        && token.range.end <= projection_start
                        && sql
                            .get(token.range.start..token.range.end)
                            .is_some_and(|word| word.eq_ignore_ascii_case(modifier))
                })
                .map(|token| token.range)
                .ok_or(SqlStructureReason::DistinctUnsupported)?;
            let replacement = if distinct { "DISTINCT" } else { "" };
            replace_sql_source_range(sql, distinct_range, replacement)
                .ok_or(SqlStructureReason::DistinctUnsupported)
        }
    }
}

fn insert_sql_source_offset(sql: &str, offset: usize, insertion: &str) -> Option<String> {
    if offset > sql.len() || !sql.is_char_boundary(offset) {
        return None;
    }
    let before = sql.get(..offset)?;
    let after = sql.get(offset..)?;
    Some(format!("{before}{insertion}{after}"))
}

fn rewrite_filter_expression(
    sql: &str,
    query: &Query,
    context: &SqlStructureContext,
    joins: &[SqlStructureJoin],
    expression: Option<SqlStructureFilterExpression>,
) -> Result<String, SqlStructureReason> {
    let replacement = expression
        .as_ref()
        .map(|expression| {
            let mut nodes = 0;
            filter_expression_to_ast(expression, context, joins, 0, &mut nodes)
        })
        .transpose()?
        .map(|expression| expression.to_string());
    rewrite_where_clause(sql, query, replacement.as_deref())
}

fn set_filters(
    query: &mut Query,
    context: &SqlStructureContext,
    joins: &[SqlStructureJoin],
    filters: Vec<SqlStructureFilter>,
) -> Result<(), SqlStructureReason> {
    if filters.len() > 128 {
        return Err(SqlStructureReason::FilterUnsupported);
    }
    let select = match query.body.as_ref() {
        SetExpr::Select(select) => select,
        _ => return Err(SqlStructureReason::SetOperationUnsupported),
    };
    if let Some(selection) = select.selection.as_ref() {
        let current = parse_filter_expression(selection, context, joins)?;
        if flat_conjunctive_filters(&current).is_none() {
            return Err(SqlStructureReason::FilterUnsupported);
        }
    }
    let mut expressions = Vec::with_capacity(filters.len());
    for filter in filters {
        expressions.push(filter_to_ast(&filter, context, joins)?);
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

fn filter_to_ast(
    filter: &SqlStructureFilter,
    context: &SqlStructureContext,
    joins: &[SqlStructureJoin],
) -> Result<Expr, SqlStructureReason> {
    validate_filter(context, filter)?;
    let qualifier = resolve_filter_qualifier(filter.qualifier.as_ref(), joins)?;
    let column = column_reference_expr(&filter.column, qualifier.as_ref());
    let parameter = filter.parameter.as_deref();
    let placeholder = |name: Option<&str>| {
        name.map(|name| Expr::value(Value::Placeholder(format!("${name}"))))
            .ok_or(SqlStructureReason::InvalidVariableName)
    };
    Ok(match filter.operator {
        SqlStructureFilterOperator::IsNull => Expr::IsNull(Box::new(column)),
        SqlStructureFilterOperator::IsNotNull => Expr::IsNotNull(Box::new(column)),
        SqlStructureFilterOperator::In | SqlStructureFilterOperator::NotIn => Expr::InList {
            expr: Box::new(column),
            list: vec![placeholder(parameter)?],
            negated: filter.operator == SqlStructureFilterOperator::NotIn,
        },
        SqlStructureFilterOperator::Between | SqlStructureFilterOperator::NotBetween => {
            Expr::Between {
                expr: Box::new(column),
                negated: filter.operator == SqlStructureFilterOperator::NotBetween,
                low: Box::new(placeholder(parameter)?),
                high: Box::new(placeholder(filter.second_parameter.as_deref())?),
            }
        }
        SqlStructureFilterOperator::Like | SqlStructureFilterOperator::NotLike => Expr::Like {
            negated: filter.operator == SqlStructureFilterOperator::NotLike,
            any: false,
            expr: Box::new(column),
            pattern: Box::new(placeholder(parameter)?),
            escape_char: None,
        },
        SqlStructureFilterOperator::Ilike | SqlStructureFilterOperator::NotIlike => Expr::ILike {
            negated: filter.operator == SqlStructureFilterOperator::NotIlike,
            any: false,
            expr: Box::new(column),
            pattern: Box::new(placeholder(parameter)?),
            escape_char: None,
        },
        operator => {
            let sql_operator = binary_from_filter_operator(operator)
                .ok_or(SqlStructureReason::FilterUnsupported)?;
            Expr::BinaryOp {
                left: Box::new(column),
                op: sql_operator,
                right: Box::new(placeholder(parameter)?),
            }
        }
    })
}

fn filter_expression_to_ast(
    expression: &SqlStructureFilterExpression,
    context: &SqlStructureContext,
    joins: &[SqlStructureJoin],
    depth: usize,
    nodes: &mut usize,
) -> Result<Expr, SqlStructureReason> {
    if depth > MAX_FILTER_EXPRESSION_DEPTH {
        return Err(SqlStructureReason::FilterUnsupported);
    }
    increment_filter_node(nodes)?;
    match expression {
        SqlStructureFilterExpression::And { expressions }
        | SqlStructureFilterExpression::Or { expressions } => {
            if !(2..=MAX_FILTER_EXPRESSION_NODES).contains(&expressions.len()) {
                return Err(SqlStructureReason::FilterUnsupported);
            }
            let operator = if matches!(expression, SqlStructureFilterExpression::And { .. }) {
                BinaryOperator::And
            } else {
                BinaryOperator::Or
            };
            let mut expressions = expressions
                .iter()
                .map(|expression| {
                    filter_expression_to_ast(expression, context, joins, depth + 1, nodes)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let first = expressions
                .drain(..1)
                .next()
                .ok_or(SqlStructureReason::FilterUnsupported)?;
            Ok(Expr::Nested(Box::new(expressions.into_iter().fold(
                first,
                |left, right| Expr::BinaryOp {
                    left: Box::new(left),
                    op: operator.clone(),
                    right: Box::new(right),
                },
            ))))
        }
        SqlStructureFilterExpression::Not { expression } => Ok(Expr::UnaryOp {
            op: UnaryOperator::Not,
            expr: Box::new(filter_expression_to_ast(
                expression,
                context,
                joins,
                depth + 1,
                nodes,
            )?),
        }),
        SqlStructureFilterExpression::Predicate {
            column,
            qualifier,
            operator,
            parameter,
            second_parameter,
        } => {
            let filter = SqlStructureFilter {
                column: column.clone(),
                qualifier: qualifier.clone(),
                operator: *operator,
                parameter: parameter.clone(),
                second_parameter: second_parameter.clone(),
            };
            filter_to_ast(&filter, context, joins)
        }
        SqlStructureFilterExpression::ColumnComparison {
            left,
            operator,
            right,
        } => {
            validate_column_comparison(context, joins, left, *operator, right)?;
            let operator = binary_from_filter_operator(*operator)
                .ok_or(SqlStructureReason::FilterUnsupported)?;
            let left_qualifier = resolve_filter_qualifier(left.qualifier.as_ref(), joins)?;
            let right_qualifier = resolve_filter_qualifier(right.qualifier.as_ref(), joins)?;
            Ok(Expr::BinaryOp {
                left: Box::new(column_reference_expr(&left.column, left_qualifier.as_ref())),
                op: operator,
                right: Box::new(column_reference_expr(
                    &right.column,
                    right_qualifier.as_ref(),
                )),
            })
        }
    }
}

fn having_expression_to_ast(
    expression: &SqlStructureHavingExpression,
    context: &SqlStructureContext,
    depth: usize,
    nodes: &mut usize,
) -> Result<Expr, SqlStructureReason> {
    if depth > MAX_FILTER_EXPRESSION_DEPTH {
        return Err(SqlStructureReason::HavingUnsupported);
    }
    increment_having_node(nodes)?;
    match expression {
        SqlStructureHavingExpression::And { expressions }
        | SqlStructureHavingExpression::Or { expressions } => {
            if !(2..=MAX_FILTER_EXPRESSION_NODES).contains(&expressions.len()) {
                return Err(SqlStructureReason::HavingUnsupported);
            }
            let operator = if matches!(expression, SqlStructureHavingExpression::And { .. }) {
                BinaryOperator::And
            } else {
                BinaryOperator::Or
            };
            let mut expressions = expressions
                .iter()
                .map(|expression| having_expression_to_ast(expression, context, depth + 1, nodes))
                .collect::<Result<Vec<_>, _>>()?;
            let first = expressions
                .drain(..1)
                .next()
                .ok_or(SqlStructureReason::HavingUnsupported)?;
            Ok(Expr::Nested(Box::new(expressions.into_iter().fold(
                first,
                |left, right| Expr::BinaryOp {
                    left: Box::new(left),
                    op: operator.clone(),
                    right: Box::new(right),
                },
            ))))
        }
        SqlStructureHavingExpression::Not { expression } => Ok(Expr::UnaryOp {
            op: UnaryOperator::Not,
            expr: Box::new(Expr::Nested(Box::new(having_expression_to_ast(
                expression,
                context,
                depth + 1,
                nodes,
            )?))),
        }),
        SqlStructureHavingExpression::Predicate {
            aggregate,
            operator,
            value,
        } => {
            validate_having_predicate(context, aggregate, *operator, value)?;
            let SelectItem::UnnamedExpr(aggregate_expression) =
                aggregate_to_select_item(aggregate.clone())
            else {
                return Err(SqlStructureReason::HavingUnsupported);
            };
            Ok(Expr::BinaryOp {
                left: Box::new(aggregate_expression),
                op: binary_from_having_operator(*operator),
                right: Box::new(having_value_to_ast(value)?),
            })
        }
    }
}

fn having_value_to_ast(value: &SqlStructureHavingValue) -> Result<Expr, SqlStructureReason> {
    let typed_literal = match value {
        SqlStructureHavingValue::Date(value) => Some((DataType::Date, value)),
        SqlStructureHavingValue::Timestamp(value) => Some((
            DataType::Timestamp(None, sqlparser::ast::TimezoneInfo::None),
            value,
        )),
        _ => None,
    };
    if let Some((data_type, value)) = typed_literal {
        return Ok(Expr::TypedString(TypedString {
            data_type,
            value: ValueWithSpan {
                value: Value::SingleQuotedString(value.clone()),
                span: Span::empty(),
            },
            uses_odbc_syntax: false,
        }));
    }
    let ast_value = match value {
        SqlStructureHavingValue::Integer(value) if value.parse::<i64>().is_ok() => {
            Value::Number(value.clone(), false)
        }
        SqlStructureHavingValue::Numeric(value) if valid_numeric_spelling(value) => {
            Value::Number(value.clone(), false)
        }
        SqlStructureHavingValue::String(value) => Value::SingleQuotedString(value.clone()),
        SqlStructureHavingValue::Boolean(value) => Value::Boolean(*value),
        SqlStructureHavingValue::Variable(value) if valid_variable_name(value) => {
            Value::Placeholder(format!("${value}"))
        }
        _ => return Err(SqlStructureReason::HavingUnsupported),
    };
    Ok(Expr::value(ast_value))
}

fn set_having(
    query: &mut Query,
    context: &SqlStructureContext,
    expression: Option<SqlStructureHavingExpression>,
) -> Result<(), SqlStructureReason> {
    let having = expression
        .as_ref()
        .map(|expression| {
            let mut nodes = 0;
            having_expression_to_ast(expression, context, 0, &mut nodes)
        })
        .transpose()?;
    select_mut(query)?.having = having;
    Ok(())
}

fn set_sort(
    query: &mut Query,
    context: &SqlStructureContext,
    joins: &[SqlStructureJoin],
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
    let mut validated = validate_sort_columns(context, order_by, joins, select)?;
    // An unqualified stable-key name would resolve to this output alias and
    // sort by a different source column. There is no safe unqualified rewrite
    // for the hidden source key, so reject this edit instead of claiming a
    // deterministic order.
    for key in &context.stable_key {
        let identifier = Ident::with_quote('"', key.clone());
        if projection_alias_binding(select, &identifier, context)?
            .is_some_and(|(source, _)| !names_match(&source, key))
        {
            return Err(SqlStructureReason::SortUnsupported);
        }
    }
    if let Some(join) = joins.first() {
        for alias in [&join.left_alias, &join.right_alias] {
            for key in &context.stable_key {
                let exists = validated.iter().any(|sort| {
                    names_match(&sort.column, key)
                        && sort
                            .qualifier
                            .as_ref()
                            .is_some_and(|qualifier| aliases_resolve_same(qualifier, alias))
                });
                if !exists {
                    validated.push(SqlStructureSort {
                        column: key.clone(),
                        qualifier: Some(alias.clone()),
                        projection_alias: None,
                        direction: SqlStructureSortDirection::Asc,
                        nulls_first: None,
                    });
                    added_tie_breakers.push(format!("{}.{}", alias.name, key));
                }
            }
        }
    } else {
        for key in &context.stable_key {
            if !validated
                .iter()
                .any(|sort| sort.qualifier.is_none() && names_match(&sort.column, key))
            {
                validated.push(SqlStructureSort {
                    column: key.clone(),
                    qualifier: None,
                    projection_alias: None,
                    direction: SqlStructureSortDirection::Asc,
                    nulls_first: None,
                });
                added_tie_breakers.push(key.clone());
            }
        }
    }
    query.order_by = Some(OrderBy {
        kind: OrderByKind::Expressions(
            validated
                .into_iter()
                .map(|sort| OrderByExpr {
                    expr: sort.projection_alias.as_ref().map_or_else(
                        || column_reference_expr(&sort.column, sort.qualifier.as_ref()),
                        alias_reference_expr,
                    ),
                    options: OrderByOptions {
                        sort: Some(match sort.direction {
                            SqlStructureSortDirection::Asc => OrderBySort::Asc,
                            SqlStructureSortDirection::Desc => OrderBySort::Desc,
                        }),
                        nulls_first: sort.nulls_first,
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
    let function_name = match aggregate.function {
        SqlStructureAggregateFunction::Count => "count",
        SqlStructureAggregateFunction::Sum => "sum",
        SqlStructureAggregateFunction::Avg => "avg",
        SqlStructureAggregateFunction::Mean => "mean",
        SqlStructureAggregateFunction::Min => "min",
        SqlStructureAggregateFunction::Max => "max",
    };
    let descriptor =
        user_sql_aggregate(function_name).ok_or(SqlStructureReason::AggregateUnsupported)?;
    let argument_count = if aggregate.column.is_some() {
        1 + aggregate.arguments.len()
    } else if aggregate.distinct
        && aggregate.arguments.is_empty()
        && descriptor.wildcard != SqlAggregateWildcard::Unsupported
    {
        1
    } else {
        aggregate.arguments.len()
    };
    if !descriptor.arity.accepts(argument_count) {
        return Err(SqlStructureReason::AggregateUnsupported);
    }
    if aggregate.distinct
        && (descriptor.distinct == SqlAggregateDistinct::Unsupported
            || (descriptor.distinct == SqlAggregateDistinct::SingleArgument && argument_count != 1))
    {
        return Err(SqlStructureReason::AggregateUnsupported);
    }
    if !aggregate.arguments.is_empty()
        && (descriptor.name != "count"
            || aggregate.column.is_none()
            || aggregate.distinct
            || aggregate.arguments.is_empty())
    {
        return Err(SqlStructureReason::AggregateUnsupported);
    }
    if aggregate.column.is_none()
        && aggregate.arguments.is_empty()
        && descriptor.wildcard == SqlAggregateWildcard::Unsupported
    {
        return Err(SqlStructureReason::AggregateUnsupported);
    }
    for column_name in aggregate.column.iter().chain(aggregate.arguments.iter()) {
        let column = find_column(context, column_name).ok_or(SqlStructureReason::UnknownColumn)?;
        if aggregate.distinct && !column_type_supports_distinct_aggregate(column.column_type) {
            return Err(SqlStructureReason::AggregateTypeUnsupported);
        }
        if matches!(
            descriptor.input,
            crate::sql_functions::SqlAggregateInput::Numeric
        ) && !column_type_supports_numeric_aggregate(column.column_type)
        {
            return Err(SqlStructureReason::AggregateTypeUnsupported);
        }
        if matches!(
            descriptor.input,
            crate::sql_functions::SqlAggregateInput::Orderable
        ) && !column_type_supports_sort(column.column_type)
        {
            return Err(SqlStructureReason::AggregateTypeUnsupported);
        }
    }
    if aggregate
        .arguments
        .iter()
        .any(|column| column.is_empty() || column.chars().any(char::is_control))
    {
        return Err(SqlStructureReason::AggregateUnsupported);
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

fn aggregate_result_type(
    context: &SqlStructureContext,
    aggregate: &SqlStructureAggregate,
) -> Result<SqlStructureColumnType, SqlStructureReason> {
    match aggregate.function {
        SqlStructureAggregateFunction::Count => Ok(SqlStructureColumnType::Integer),
        SqlStructureAggregateFunction::Avg | SqlStructureAggregateFunction::Mean => {
            Ok(SqlStructureColumnType::Float)
        }
        SqlStructureAggregateFunction::Sum
        | SqlStructureAggregateFunction::Min
        | SqlStructureAggregateFunction::Max => aggregate
            .column
            .as_deref()
            .and_then(|column| find_column(context, column))
            .map(|column| column.column_type)
            .ok_or(SqlStructureReason::AggregateTypeUnsupported),
    }
}

fn having_value_type(
    context: &SqlStructureContext,
    value: &SqlStructureHavingValue,
) -> Result<SqlStructureColumnType, SqlStructureReason> {
    match value {
        SqlStructureHavingValue::Integer(value) => value
            .parse::<i64>()
            .map(|_| SqlStructureColumnType::Integer)
            .map_err(|_| SqlStructureReason::HavingUnsupported),
        SqlStructureHavingValue::Numeric(value) if valid_numeric_spelling(value) => {
            Ok(SqlStructureColumnType::Decimal)
        }
        SqlStructureHavingValue::Numeric(_) => Err(SqlStructureReason::HavingUnsupported),
        SqlStructureHavingValue::String(_) => Ok(SqlStructureColumnType::String),
        SqlStructureHavingValue::Date(_) => Ok(SqlStructureColumnType::Date),
        SqlStructureHavingValue::Timestamp(_) => Ok(SqlStructureColumnType::Timestamp),
        SqlStructureHavingValue::Boolean(_) => Ok(SqlStructureColumnType::Boolean),
        SqlStructureHavingValue::Variable(name) => {
            if !valid_variable_name(name) {
                return Err(SqlStructureReason::InvalidVariableName);
            }
            context
                .variables
                .iter()
                .find(|variable| variable.name == *name)
                .map(|variable| variable.variable_type.column_type())
                .ok_or(SqlStructureReason::UnknownVariable)
        }
    }
}

fn validate_having_predicate(
    context: &SqlStructureContext,
    aggregate: &SqlStructureAggregate,
    operator: SqlStructureHavingOperator,
    value: &SqlStructureHavingValue,
) -> Result<(), SqlStructureReason> {
    if aggregate.alias.is_some() {
        return Err(SqlStructureReason::HavingUnsupported);
    }
    validate_aggregate(context, aggregate)?;
    let result_type = aggregate_result_type(context, aggregate)?;
    let value_type = having_value_type(context, value)?;
    let numeric = |ty| {
        matches!(
            ty,
            SqlStructureColumnType::Integer
                | SqlStructureColumnType::Float
                | SqlStructureColumnType::Decimal
        )
    };
    let compatible = if numeric(result_type) && numeric(value_type) {
        true
    } else {
        result_type == value_type
    };
    if !compatible {
        return Err(SqlStructureReason::VariableTypeMismatch);
    }
    if !column_type_supports_sort(result_type)
        || (result_type == SqlStructureColumnType::Boolean
            && !matches!(
                operator,
                SqlStructureHavingOperator::Eq | SqlStructureHavingOperator::NotEq
            ))
    {
        return Err(SqlStructureReason::AggregateTypeUnsupported);
    }
    Ok(())
}

fn aggregate_to_select_item(aggregate: SqlStructureAggregate) -> SelectItem {
    let function_name = match aggregate.function {
        SqlStructureAggregateFunction::Count => "COUNT",
        SqlStructureAggregateFunction::Sum => "SUM",
        SqlStructureAggregateFunction::Avg => "AVG",
        SqlStructureAggregateFunction::Mean => "MEAN",
        SqlStructureAggregateFunction::Min => "MIN",
        SqlStructureAggregateFunction::Max => "MAX",
    };
    let mut arguments = aggregate
        .column
        .into_iter()
        .map(|column| {
            FunctionArg::Unnamed(FunctionArgExpr::Expr(Expr::Identifier(Ident::with_quote(
                '"', column,
            ))))
        })
        .chain(aggregate.arguments.into_iter().map(|column| {
            FunctionArg::Unnamed(FunctionArgExpr::Expr(Expr::Identifier(Ident::with_quote(
                '"', column,
            ))))
        }))
        .collect::<Vec<_>>();
    if arguments.is_empty() {
        arguments.push(FunctionArg::Unnamed(FunctionArgExpr::Wildcard));
    }
    let function = Expr::Function(Function {
        name: ObjectName(vec![ObjectNamePart::Identifier(Ident::new(function_name))]),
        uses_odbc_syntax: false,
        parameters: FunctionArguments::None,
        args: FunctionArguments::List(FunctionArgumentList {
            duplicate_treatment: aggregate.distinct.then_some(DuplicateTreatment::Distinct),
            args: arguments,
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
    joins: &[SqlStructureJoin],
    select: &Select,
) -> Result<Vec<SqlStructureSort>, SqlStructureReason> {
    let mut validated = Vec::with_capacity(order_by.len());
    let mut seen = BTreeSet::new();
    for sort in order_by {
        let (source_column, projection_alias) = match sort.projection_alias.as_ref() {
            Some(alias) => {
                if sort.qualifier.is_some() {
                    return Err(SqlStructureReason::SortUnsupported);
                }
                let identifier = if alias.quoted {
                    Ident::with_quote('"', alias.name.clone())
                } else {
                    Ident::new(alias.name.clone())
                };
                let Some((source, actual_alias)) =
                    projection_alias_binding(select, &identifier, context)?
                else {
                    return Err(SqlStructureReason::SortUnsupported);
                };
                if actual_alias != *alias || !names_match(&source, &sort.column) {
                    return Err(SqlStructureReason::SortUnsupported);
                }
                (source, Some(alias.clone()))
            }
            None => {
                let identifier = Ident::with_quote('"', sort.column.clone());
                if projection_alias_binding(select, &identifier, context)?
                    .is_some_and(|(source, _)| !names_match(&source, &sort.column))
                {
                    return Err(SqlStructureReason::SortUnsupported);
                }
                (sort.column.clone(), None)
            }
        };
        let column =
            find_column(context, &source_column).ok_or(SqlStructureReason::UnknownColumn)?;
        if !column_type_supports_sort(column.column_type) {
            return Err(SqlStructureReason::SortUnsupported);
        }
        let qualifier = resolve_filter_qualifier(sort.qualifier.as_ref(), joins)?;
        let key = (
            qualifier.as_ref().map(alias_resolution_key),
            column.name.clone(),
        );
        if !seen.insert(key) {
            return Err(SqlStructureReason::DuplicateColumn);
        }
        validated.push(SqlStructureSort {
            column: column.name.clone(),
            qualifier,
            projection_alias,
            direction: sort.direction,
            nulls_first: sort.nulls_first,
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
    let has_join = !parsed.joins.is_empty();
    let columns_available = !context.columns.is_empty();
    let structured_projection = parsed.aggregates.is_empty() && parsed.group_by.is_empty();
    let literal_projection_enabled = context.relation.is_empty()
        && !parsed.literal_projection.is_empty()
        && structured_projection;
    let columns_enabled =
        (columns_available || literal_projection_enabled) && structured_projection && !has_join;
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
    let flat_filter_supported = parsed
        .filter_expression
        .as_ref()
        .is_none_or(|expression| flat_conjunctive_filters(expression).is_some());
    SqlStructureAnalysis {
        contract_version: SQL_STRUCTURE_CONTRACT_VERSION,
        status: SqlStructureStatus::Supported,
        reason: None,
        projection: parsed.projection,
        literal_projection: parsed.literal_projection,
        projection_aliases: parsed.projection_aliases,
        filters: parsed.filters,
        filter_expression: parsed.filter_expression,
        aggregates: parsed.aggregates,
        group_by: parsed.group_by,
        having_expression: parsed.having_expression,
        distinct: parsed.distinct,
        row_limit: parsed.row_limit,
        order_by: parsed.order_by,
        joins: parsed.joins,
        referenced_variables: parsed.referenced_variables,
        stable_pagination: parsed.stable_pagination,
        capabilities: SqlStructureCapabilities {
            join: capability(
                !context.relation.is_empty() && (parsed.can_add_join || has_join),
                (!parsed.can_add_join && !has_join).then_some(SqlStructureReason::JoinUnsupported),
            ),
            columns: capability(
                columns_enabled,
                if has_join {
                    Some(SqlStructureReason::JoinUnsupported)
                } else if !columns_available && !literal_projection_enabled {
                    Some(SqlStructureReason::InvalidContext)
                } else {
                    (!structured_projection)
                        .then_some(SqlStructureReason::ProjectionUnsupported)
                        .or_else(|| {
                            (!columns_available && !literal_projection_enabled)
                                .then_some(SqlStructureReason::InvalidContext)
                        })
                },
            ),
            filter: capability(
                columns_available && flat_filter_supported,
                (!columns_available)
                    .then_some(SqlStructureReason::InvalidContext)
                    .or_else(|| {
                        (!flat_filter_supported).then_some(SqlStructureReason::FilterUnsupported)
                    }),
            ),
            filter_expression: capability(
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
            aggregate: capability(
                columns_available && !has_join,
                has_join.then_some(SqlStructureReason::JoinUnsupported),
            ),
            group_by: capability(
                columns_available && !has_join,
                has_join.then_some(SqlStructureReason::JoinUnsupported),
            ),
            having: capability(
                columns_available && !has_join,
                if has_join {
                    Some(SqlStructureReason::JoinUnsupported)
                } else if !columns_available {
                    Some(SqlStructureReason::InvalidContext)
                } else {
                    None
                },
            ),
            distinct: capability(
                distinct_enabled && !has_join,
                has_join.then_some(SqlStructureReason::JoinUnsupported),
            ),
            row_limit: capability(true, None),
            stable_pagination: capability(parsed.stable_pagination, pagination_reason),
        },
    }
}

fn projection_sources_are_unique(projection: &[String]) -> bool {
    projection.iter().collect::<BTreeSet<_>>().len() == projection.len()
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
        literal_projection: Vec::new(),
        projection_aliases: Vec::new(),
        filters: Vec::new(),
        filter_expression: None,
        aggregates: Vec::new(),
        group_by: Vec::new(),
        having_expression: None,
        distinct: false,
        row_limit: None,
        order_by: Vec::new(),
        joins: Vec::new(),
        referenced_variables: Vec::new(),
        stable_pagination: false,
        capabilities: SqlStructureCapabilities {
            join: disabled.clone(),
            columns: disabled.clone(),
            filter: disabled.clone(),
            filter_expression: disabled.clone(),
            sort: disabled.clone(),
            aggregate: disabled.clone(),
            group_by: disabled.clone(),
            having: disabled.clone(),
            distinct: disabled.clone(),
            row_limit: disabled.clone(),
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
        document_node_edit: None,
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

    fn assert_applied_node_proposal(
        proposal: &SqlStructureProposal,
        expected_kind: SqlDocumentNodeKind,
    ) {
        assert_eq!(proposal.status, SqlStructureStatus::Supported);
        assert!(proposal.requires_confirmation);
        let proposed_sql = proposal
            .proposed_sql
            .as_deref()
            .expect("successful changed proposal has preview SQL");
        let transaction = proposal
            .document_node_edit
            .as_ref()
            .expect("successful changed proposal has a typed node edit");
        assert_eq!(transaction.edits.len(), 1);
        assert_eq!(transaction.expected_source, proposal.base_sql);
        assert_eq!(transaction.edits[0].expected_kind, expected_kind);
        let applied = apply_sql_document_node_edit_transaction(&proposal.base_sql, transaction)
            .expect("Rust-derived node transaction applies");
        assert_eq!(applied.document.source, proposed_sql);
    }

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
                SqlStructureColumn {
                    name: "opaque_other".to_owned(),
                    column_type: SqlStructureColumnType::Other,
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
    fn structure_proposals_return_one_applicable_smallest_typed_node_edit() {
        let context = context();

        let projection = propose_sql_structure_operation(
            "SELECT * FROM \"Task\"",
            &context,
            SqlStructureOperation::SetColumns {
                columns: vec!["field_101".to_owned()],
                aliases: vec![],
            },
        );
        assert_applied_node_proposal(&projection, SqlDocumentNodeKind::SelectList);

        let filter = propose_sql_structure_operation(
            "SELECT * FROM \"Task\" WHERE \"field_102\" >= $priority",
            &context,
            SqlStructureOperation::SetFilters {
                filters: vec![SqlStructureFilter {
                    column: "field_101".to_owned(),
                    qualifier: None,
                    operator: SqlStructureFilterOperator::Eq,
                    parameter: Some("status".to_owned()),
                    second_parameter: None,
                }],
            },
        );
        assert_applied_node_proposal(&filter, SqlDocumentNodeKind::WhereClause);

        let sort = propose_sql_structure_operation(
            "SELECT * FROM \"Task\" ORDER BY \"_ugoite_id\" ASC",
            &context,
            SqlStructureOperation::SetSort {
                order_by: vec![SqlStructureSort {
                    column: "field_102".to_owned(),
                    qualifier: None,
                    projection_alias: None,
                    direction: SqlStructureSortDirection::Desc,
                    nulls_first: None,
                }],
            },
        );
        assert_applied_node_proposal(&sort, SqlDocumentNodeKind::OrderByClause);

        let grouped = propose_sql_structure_operation(
            "SELECT SUM(\"field_102\") FROM \"Task\" GROUP BY \"field_101\", \"field_102\"",
            &context,
            SqlStructureOperation::SetGroupBy {
                columns: vec!["field_101".to_owned()],
            },
        );
        assert_applied_node_proposal(&grouped, SqlDocumentNodeKind::GroupByClause);

        let joined = propose_sql_structure_operation(
            "SELECT * FROM \"Task\" AS t INNER JOIN \"Task\" AS other ON t.field_101 = other.field_101",
            &context,
            SqlStructureOperation::SetJoin {
                join_kind: SqlStructureJoinKind::Inner,
                left_column: "_ugoite_id".to_owned(),
                right_column: "_ugoite_id".to_owned(),
            },
        );
        assert_applied_node_proposal(&joined, SqlDocumentNodeKind::Join);

        let relation_free = propose_sql_structure_operation(
            "-- draft\nSELECT 1 -- keep this comment\n",
            &context,
            SqlStructureOperation::SetRelation {
                relation: "Task".to_owned(),
            },
        );
        assert_applied_node_proposal(&relation_free, SqlDocumentNodeKind::Select);

        let existing_relation = propose_sql_structure_operation(
            "SELECT \"field_102\" FROM \"Old\"",
            &context,
            SqlStructureOperation::SetRelation {
                relation: "Task".to_owned(),
            },
        );
        assert_applied_node_proposal(&existing_relation, SqlDocumentNodeKind::Relation);
    }

    #[test]
    fn rejected_and_noop_structure_proposals_have_no_document_node_edit() {
        let context = context();
        let rejected = propose_sql_structure_operation(
            "SELECT * FROM \"Task\"",
            &context,
            SqlStructureOperation::SetColumns {
                columns: vec!["missing".to_owned()],
                aliases: vec![],
            },
        );
        assert_eq!(rejected.status, SqlStructureStatus::Unsupported);
        assert!(rejected.document_node_edit.is_none());

        let noop = propose_sql_structure_operation(
            "SELECT \"field_101\" FROM \"Task\"",
            &context,
            SqlStructureOperation::SetColumns {
                columns: vec!["field_101".to_owned()],
                aliases: vec![],
            },
        );
        assert_eq!(noop.status, SqlStructureStatus::Supported);
        assert!(!noop.requires_confirmation);
        assert!(noop.document_node_edit.is_none());
    }

    #[test]
    fn shared_sql_feature_matrix_matches_structure_protocol_and_aggregate_types() {
        let matrix: serde_json::Value =
            serde_json::from_str(include_str!("../../../shared/sql/ugoite-sql-rules.json"))
                .expect("shared SQL rules must remain valid JSON");
        let editor = &matrix["feature_matrix"]["legacy_subset_editor"];
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
        let join_limits = editor["constructs"]
            .as_array()
            .expect("editor constructs must be an array")
            .iter()
            .find(|construct| construct["id"] == "joins")
            .expect("JOIN coverage must be declared")["limits"]
            .as_array()
            .expect("JOIN limits must be an array")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<BTreeSet<_>>();
        assert!(join_limits.contains(
            &"one self-join of the selected Form with explicit distinct aliases and a closed INNER/LEFT/RIGHT/FULL join-kind choice"
        ));
        assert!(join_limits.contains(
            &"WHERE supports bounded AND/OR/NOT trees of qualified parameter comparisons, single-value IN/NOT IN, two-bound BETWEEN, LIKE/ILIKE, and IS NULL/IS NOT NULL on qualified columns; adding a JOIN qualifies every supported predicate leaf; ORDER BY supports simple qualified columns with ASC/DESC and NULLS FIRST/LAST; unqualified references fail closed"
        ));
        let projection_limits = editor["constructs"]
            .as_array()
            .expect("editor constructs must be an array")
            .iter()
            .find(|construct| construct["id"] == "projection")
            .expect("SELECT projection coverage must be declared")["limits"]
            .as_array()
            .expect("projection limits must be an array")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<BTreeSet<_>>();
        assert!(projection_limits.contains(
            &"simple Form-column projection aliases are inspected and edited independently by SQL-document-scoped projection slot identity"
        ));
        assert!(projection_limits
            .contains(&"alias edits that would change ORDER BY output-name binding fail closed"));
        assert!(projection_limits.contains(
            &"repeated source columns remain editable, reorderable, and independently aliasable by projection slot"
        ));
        let declared_operations = editor["operations"]
            .as_array()
            .expect("feature matrix operations must be an array")
            .iter()
            .filter(|operation| operation["protocol_family"] != "sql_document")
            .chain(
                editor["legacy_operations"]
                    .as_array()
                    .expect("feature matrix legacy operations must be an array")
                    .iter(),
            )
            .map(|operation| {
                operation["protocol"]
                    .as_str()
                    .expect("each operation must name its protocol kind")
                    .to_owned()
            })
            .collect::<BTreeSet<_>>();
        let mut declared_operations = declared_operations;
        declared_operations.insert(
            editor["literal_projection_operation"]["protocol"]
                .as_str()
                .expect("literal projection operation must name its protocol kind")
                .to_owned(),
        );
        let protocol_operations = [
            SqlStructureOperation::SetRelation {
                relation: "Task".to_owned(),
            },
            SqlStructureOperation::SetJoin {
                join_kind: SqlStructureJoinKind::Inner,
                left_column: "field_101".to_owned(),
                right_column: "field_101".to_owned(),
            },
            SqlStructureOperation::SetLiteralProjection { values: vec![] },
            SqlStructureOperation::SetProjectionSlots { slots: vec![] },
            // SetColumns remains a narrower low-level operation; the GUI uses
            // the slot-addressed projection operation listed in the matrix.
            SqlStructureOperation::SetFilters { filters: vec![] },
            SqlStructureOperation::SetFilterExpression { expression: None },
            SqlStructureOperation::SetSort { order_by: vec![] },
            SqlStructureOperation::SetAggregates { aggregates: vec![] },
            SqlStructureOperation::SetGroupBy { columns: vec![] },
            SqlStructureOperation::SetHaving { expression: None },
            SqlStructureOperation::SetDistinct { distinct: false },
            SqlStructureOperation::SetRowLimit {
                limit: Some("1".to_owned()),
                offset: None,
            },
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
        let mut expected_editor_aggregates = aggregate_function_names;
        for aggregate in crate::sql_functions::USER_SQL_AGGREGATES {
            expected_editor_aggregates
                .extend(aggregate.aliases.iter().map(|alias| (*alias).to_owned()));
        }
        assert_eq!(editor_aggregates, expected_editor_aggregates);
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
                    qualifier: None,
                    operator: SqlStructureFilterOperator::Eq,
                    parameter: Some("status".to_owned()),
                    second_parameter: None,
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
                    arguments: vec![],
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
    fn analyzes_and_edits_one_supported_self_join_without_rewriting_other_sql() {
        let sql = "-- query\nSELECT * FROM \"Task\" AS t INNER JOIN \"Task\" AS other ON t.field_101 = other.field_101 /* keep */ LIMIT 8";
        let analysis = analyze_sql_structure(sql, &context());
        assert_eq!(analysis.status, SqlStructureStatus::Supported);
        assert_eq!(analysis.joins.len(), 1);
        assert_eq!(analysis.joins[0].relation, "Task");
        assert_eq!(analysis.joins[0].left_alias.name, "t");
        assert!(!analysis.joins[0].left_alias.quoted);
        assert_eq!(analysis.joins[0].right_alias.name, "other");
        assert_eq!(analysis.joins[0].left_column, "field_101");
        assert_eq!(analysis.joins[0].right_column, "field_101");
        assert_eq!(analysis.joins[0].join_kind, SqlStructureJoinKind::Inner);
        assert!(analysis.capabilities.join.enabled);
        assert!(!analysis.capabilities.columns.enabled);
        assert_eq!(
            analysis.capabilities.columns.reason,
            Some(SqlStructureReason::JoinUnsupported)
        );
        assert!(analysis.capabilities.filter.enabled);
        assert!(analysis.capabilities.sort.enabled);
        assert!(!analysis.capabilities.stable_pagination.enabled);
        assert_eq!(
            analysis.capabilities.stable_pagination.reason,
            Some(SqlStructureReason::StableOrderRequired)
        );
        assert_eq!(
            analysis.capabilities.aggregate.reason,
            Some(SqlStructureReason::JoinUnsupported)
        );
        assert_eq!(
            analysis.capabilities.group_by.reason,
            Some(SqlStructureReason::JoinUnsupported)
        );
        assert_eq!(
            analysis.capabilities.distinct.reason,
            Some(SqlStructureReason::JoinUnsupported)
        );

        let proposal = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetJoin {
                join_kind: SqlStructureJoinKind::Inner,
                left_column: "_ugoite_id".to_owned(),
                right_column: "_ugoite_id".to_owned(),
            },
        );
        assert_eq!(proposal.status, SqlStructureStatus::Supported);
        let proposed = proposal.proposed_sql.expect("join edit proposal");
        assert_eq!(
            proposed,
            "-- query\nSELECT * FROM \"Task\" AS t INNER JOIN \"Task\" AS other ON t.\"_ugoite_id\" = other.\"_ugoite_id\" /* keep */ LIMIT 8"
        );
        assert_eq!(
            analyze_sql_structure(&proposed, &context()).status,
            SqlStructureStatus::Supported
        );
    }

    #[test]
    fn self_join_kind_edits_are_closed_and_preserve_operator_trivia() {
        let context = context();
        let source = "SELECT * FROM \"Task\" AS t INNER /* preserve operator comment */ JOIN \"Task\" AS other ON t.field_101 = other.field_101";
        for (join_kind, keyword) in [
            (SqlStructureJoinKind::Inner, "INNER"),
            (SqlStructureJoinKind::Left, "LEFT"),
            (SqlStructureJoinKind::Right, "RIGHT"),
            (SqlStructureJoinKind::Full, "FULL"),
        ] {
            let proposal = propose_sql_structure_operation(
                source,
                &context,
                SqlStructureOperation::SetJoin {
                    join_kind,
                    left_column: "field_101".to_owned(),
                    right_column: "field_101".to_owned(),
                },
            );
            assert_eq!(
                proposal.status,
                SqlStructureStatus::Supported,
                "{join_kind:?}"
            );
            let proposed = proposal.proposed_sql.expect("join kind edit");
            assert!(
                proposed.contains(&format!("{keyword} /* preserve operator comment */ JOIN")),
                "{proposed}"
            );
            let analysis = analyze_sql_structure(&proposed, &context);
            assert_eq!(analysis.status, SqlStructureStatus::Supported, "{proposed}");
            assert_eq!(analysis.joins[0].join_kind, join_kind);
        }

        let explicit_outer = "SELECT * FROM \"Task\" AS t LEFT /* preserve outer trivia */ OUTER JOIN \"Task\" AS other ON t.field_101 = other.field_101";
        let proposal = propose_sql_structure_operation(
            explicit_outer,
            &context,
            SqlStructureOperation::SetJoin {
                join_kind: SqlStructureJoinKind::Right,
                left_column: "field_101".to_owned(),
                right_column: "field_101".to_owned(),
            },
        );
        assert_eq!(proposal.status, SqlStructureStatus::Supported);
        let proposed = proposal.proposed_sql.expect("outer join kind edit");
        assert!(proposed.contains("RIGHT /* preserve outer trivia */  JOIN"));
        assert_eq!(
            analyze_sql_structure(&proposed, &context).joins[0].join_kind,
            SqlStructureJoinKind::Right
        );

        let bare_join =
            "SELECT * FROM \"Task\" AS t JOIN \"Task\" AS other ON t.field_101 = other.field_101";
        let proposal = propose_sql_structure_operation(
            bare_join,
            &context,
            SqlStructureOperation::SetJoin {
                join_kind: SqlStructureJoinKind::Full,
                left_column: "field_101".to_owned(),
                right_column: "field_101".to_owned(),
            },
        );
        assert_eq!(proposal.status, SqlStructureStatus::Supported);
        assert!(proposal
            .proposed_sql
            .expect("bare JOIN kind edit")
            .contains("FULL JOIN"));
    }

    #[test]
    fn direct_projection_slot_proposal_rejects_same_form_self_join() {
        let sql = "SELECT * FROM \"Task\" AS t INNER JOIN \"Task\" AS other ON t.field_101 = other.field_101";
        let proposal = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetProjectionSlots {
                slots: vec![SqlStructureProjectionSlotEdit {
                    id: None,
                    column: "field_101".to_owned(),
                    alias: None,
                }],
            },
        );

        assert_eq!(proposal.status, SqlStructureStatus::Unsupported);
        assert_eq!(proposal.reason, Some(SqlStructureReason::JoinUnsupported));
        assert_eq!(proposal.base_sql, sql);
        assert_eq!(proposal.proposed_sql, None);

        let legacy_proposal = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetColumns {
                columns: vec!["field_101".to_owned()],
                aliases: vec![],
            },
        );
        assert_eq!(legacy_proposal.status, SqlStructureStatus::Unsupported);
        assert_eq!(
            legacy_proposal.reason,
            Some(SqlStructureReason::JoinUnsupported)
        );
        assert_eq!(legacy_proposal.base_sql, sql);
        assert_eq!(legacy_proposal.proposed_sql, None);
    }

    #[test]
    fn adds_a_bounded_self_join_and_preserves_trailing_clauses() {
        let sql = "SELECT * FROM \"Task\" LIMIT 20";
        let analysis = analyze_sql_structure(sql, &context());
        assert_eq!(analysis.status, SqlStructureStatus::Supported);
        assert!(analysis.capabilities.join.enabled);

        let proposal = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetJoin {
                join_kind: SqlStructureJoinKind::Inner,
                left_column: "field_101".to_owned(),
                right_column: "field_101".to_owned(),
            },
        );
        assert_eq!(proposal.status, SqlStructureStatus::Supported);
        let proposed = proposal.proposed_sql.expect("join proposal");
        assert_eq!(
            proposed,
            "SELECT * FROM \"Task\" AS \"_ugoite_left\" INNER JOIN \"Task\" AS \"_ugoite_right\" ON \"_ugoite_left\".\"field_101\" = \"_ugoite_right\".\"field_101\" LIMIT 20"
        );
        let parsed = analyze_sql_structure(&proposed, &context());
        assert_eq!(parsed.status, SqlStructureStatus::Supported);
        assert_eq!(parsed.joins.len(), 1);
    }

    #[test]
    fn joined_qualified_filters_and_ordering_are_editable_and_preserved() {
        let sql = "SELECT * FROM \"Task\" AS t INNER JOIN \"Task\" AS other ON t.field_101 = other.field_101 WHERE t.field_101 = $status AND other.field_102 IS NOT NULL ORDER BY other.field_101 DESC, t._ugoite_id ASC LIMIT 8";
        let analysis = analyze_sql_structure(sql, &context());
        assert_eq!(analysis.status, SqlStructureStatus::Supported);
        assert_eq!(analysis.filters.len(), 2);
        assert_eq!(analysis.filters[0].column, "field_101");
        assert_eq!(analysis.filters[0].qualifier.as_ref().unwrap().name, "t");
        assert_eq!(analysis.filters[1].column, "field_102");
        assert_eq!(
            analysis.filters[1].qualifier.as_ref().unwrap().name,
            "other"
        );
        assert_eq!(analysis.order_by.len(), 2);
        assert_eq!(analysis.order_by[0].column, "field_101");
        assert_eq!(
            analysis.order_by[0].qualifier.as_ref().unwrap().name,
            "other"
        );
        assert_eq!(analysis.order_by[1].column, "_ugoite_id");
        assert_eq!(analysis.order_by[1].qualifier.as_ref().unwrap().name, "t");
        assert!(analysis.capabilities.filter.enabled);
        assert!(analysis.capabilities.sort.enabled);

        let where_and_order = sql.split_once(" WHERE ").unwrap().1;
        let proposal = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetJoin {
                join_kind: SqlStructureJoinKind::Inner,
                left_column: "_ugoite_id".to_owned(),
                right_column: "_ugoite_id".to_owned(),
            },
        );
        assert_eq!(proposal.status, SqlStructureStatus::Supported);
        let proposed = proposal.proposed_sql.expect("join edit proposal");
        assert!(proposed.ends_with(&format!("WHERE {where_and_order}")));
        assert_eq!(
            analyze_sql_structure(&proposed, &context()).status,
            SqlStructureStatus::Supported
        );
    }

    #[test]
    fn adding_join_qualifies_existing_single_relation_filters_and_ordering() {
        let sql = "SELECT * FROM \"Task\" WHERE \"field_101\" = $status ORDER BY \"_ugoite_id\" DESC LIMIT 8";
        let proposal = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetJoin {
                join_kind: SqlStructureJoinKind::Inner,
                left_column: "field_101".to_owned(),
                right_column: "field_101".to_owned(),
            },
        );
        assert_eq!(proposal.status, SqlStructureStatus::Supported);
        let proposed = proposal.proposed_sql.expect("join proposal");
        assert!(proposed.contains(
            "WHERE \"_ugoite_left\".\"field_101\" = $status ORDER BY \"_ugoite_left\".\"_ugoite_id\" DESC LIMIT 8"
        ));
        assert_eq!(
            analyze_sql_structure(&proposed, &context()).status,
            SqlStructureStatus::Supported
        );
    }

    #[test]
    fn adding_join_qualifies_every_leaf_in_nested_boolean_filters() {
        let sql = "SELECT * FROM \"Task\" WHERE (\"field_101\" = $status OR \"field_102\" >= $priority) AND NOT (\"field_101\" != $status) ORDER BY \"_ugoite_id\" ASC";
        let analysis = analyze_sql_structure(sql, &context());
        assert_eq!(analysis.status, SqlStructureStatus::Supported);
        assert!(analysis.capabilities.join.enabled);

        let proposal = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetJoin {
                join_kind: SqlStructureJoinKind::Inner,
                left_column: "field_101".to_owned(),
                right_column: "field_101".to_owned(),
            },
        );
        assert_eq!(proposal.status, SqlStructureStatus::Supported);
        let proposed = proposal.proposed_sql.expect("join proposal");
        let joined = analyze_sql_structure(&proposed, &context());
        assert_eq!(joined.status, SqlStructureStatus::Supported, "{proposed}");
        assert_eq!(joined.joins.len(), 1);
        let left_alias = joined.joins[0].left_alias.clone();
        assert_eq!(
            joined.filter_expression,
            Some(SqlStructureFilterExpression::And {
                expressions: vec![
                    SqlStructureFilterExpression::Or {
                        expressions: vec![
                            SqlStructureFilterExpression::Predicate {
                                column: "field_101".to_owned(),
                                qualifier: Some(left_alias.clone()),
                                operator: SqlStructureFilterOperator::Eq,
                                parameter: Some("status".to_owned()),
                                second_parameter: None,
                            },
                            SqlStructureFilterExpression::Predicate {
                                column: "field_102".to_owned(),
                                qualifier: Some(left_alias.clone()),
                                operator: SqlStructureFilterOperator::GtEq,
                                parameter: Some("priority".to_owned()),
                                second_parameter: None,
                            },
                        ],
                    },
                    SqlStructureFilterExpression::Not {
                        expression: Box::new(SqlStructureFilterExpression::Predicate {
                            column: "field_101".to_owned(),
                            qualifier: Some(left_alias),
                            operator: SqlStructureFilterOperator::NotEq,
                            parameter: Some("status".to_owned()),
                            second_parameter: None,
                        }),
                    },
                ],
            })
        );
        assert!(proposed.contains("ORDER BY \"_ugoite_left\".\"_ugoite_id\" ASC"));
    }

    #[test]
    fn adding_join_preserves_existing_relation_alias_in_qualified_clauses() {
        let sql = "SELECT * FROM \"Task\" AS task_rows WHERE field_101 = $status ORDER BY _ugoite_id DESC LIMIT 8";
        let proposal = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetJoin {
                join_kind: SqlStructureJoinKind::Inner,
                left_column: "field_101".to_owned(),
                right_column: "field_101".to_owned(),
            },
        );
        assert_eq!(proposal.status, SqlStructureStatus::Supported);
        let proposed = proposal.proposed_sql.expect("join proposal");
        assert!(proposed.contains(
            "WHERE task_rows.\"field_101\" = $status ORDER BY task_rows.\"_ugoite_id\" DESC LIMIT 8"
        ));
        assert_eq!(
            analyze_sql_structure(&proposed, &context()).status,
            SqlStructureStatus::Supported
        );
        assert!(proposed
            .contains("FROM \"Task\" AS task_rows INNER JOIN \"Task\" AS \"_ugoite_right\""));
    }

    #[test]
    fn resolves_unquoted_uppercase_join_identifiers_case_insensitively() {
        let sql = "SELECT * FROM \"Task\" AS T INNER JOIN \"Task\" AS OTHER ON T.FIELD_101 = OTHER.FIELD_101 WHERE OTHER.FIELD_101 = $status ORDER BY T._UGOITE_ID DESC LIMIT 8";
        let analysis = analyze_sql_structure(sql, &context());
        assert_eq!(analysis.status, SqlStructureStatus::Supported);
        assert_eq!(analysis.joins[0].left_alias.name, "T");
        assert_eq!(analysis.joins[0].right_alias.name, "OTHER");
        assert_eq!(analysis.joins[0].left_column, "field_101");
        assert_eq!(analysis.filters[0].column, "field_101");
        assert_eq!(
            analysis.filters[0].qualifier.as_ref().unwrap().name,
            "OTHER"
        );
        assert_eq!(analysis.order_by[0].column, "_ugoite_id");
        assert_eq!(analysis.order_by[0].qualifier.as_ref().unwrap().name, "T");
    }

    #[test]
    fn rejects_unqualified_filters_and_ordering_for_joined_relations() {
        for sql in [
            "SELECT * FROM \"Task\" AS t INNER JOIN \"Task\" AS other ON t.field_101 = other.field_101 WHERE field_101 = $status",
            "SELECT * FROM \"Task\" AS t INNER JOIN \"Task\" AS other ON t.field_101 = other.field_101 ORDER BY _ugoite_id",
        ] {
            let analysis = analyze_sql_structure(sql, &context());
            assert_eq!(analysis.status, SqlStructureStatus::Unsupported, "{sql}");
        }
    }

    #[test]
    fn joined_filter_and_sort_operations_keep_explicit_aliases() {
        let sql = "SELECT * FROM \"Task\" AS t INNER JOIN \"Task\" AS other ON t.field_101 = other.field_101";
        let join = analyze_sql_structure(sql, &context()).joins[0].clone();
        let filter_proposal = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetFilters {
                filters: vec![SqlStructureFilter {
                    column: "field_101".to_owned(),
                    qualifier: Some(join.right_alias.clone()),
                    operator: SqlStructureFilterOperator::Eq,
                    parameter: Some("status".to_owned()),
                    second_parameter: None,
                }],
            },
        );
        assert_eq!(filter_proposal.status, SqlStructureStatus::Supported);
        let filtered_sql = filter_proposal.proposed_sql.expect("filter proposal");
        assert!(filtered_sql.contains("WHERE other.\"field_101\" = $status"));
        let filtered = analyze_sql_structure(&filtered_sql, &context());
        assert_eq!(filtered.status, SqlStructureStatus::Supported);
        assert_eq!(
            filtered.filters[0].qualifier,
            Some(join.right_alias.clone())
        );

        let sort_proposal = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetSort {
                order_by: vec![SqlStructureSort {
                    column: "field_101".to_owned(),
                    qualifier: Some(join.left_alias.clone()),
                    projection_alias: None,
                    direction: SqlStructureSortDirection::Desc,
                    nulls_first: None,
                }],
            },
        );
        assert_eq!(sort_proposal.status, SqlStructureStatus::Supported);
        let sorted_sql = sort_proposal.proposed_sql.expect("sort proposal");
        assert!(sorted_sql.contains(
            "ORDER BY t.\"field_101\" DESC, t.\"_ugoite_id\" ASC, other.\"_ugoite_id\" ASC"
        ));
        let sorted = analyze_sql_structure(&sorted_sql, &context());
        assert_eq!(sorted.status, SqlStructureStatus::Supported);
        assert!(sorted.stable_pagination);
    }

    #[test]
    fn joined_filter_and_sort_edits_preserve_comments_outside_the_clause() {
        let sql = "SELECT /* projection */ * FROM \"Task\" AS t INNER JOIN \"Task\" AS other ON t.field_101 = other.field_101 /* join */ WHERE other.field_101 = $status ORDER BY t._ugoite_id";
        assert_eq!(
            analyze_sql_structure(sql, &context()).status,
            SqlStructureStatus::Supported
        );

        let filter = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetFilters {
                filters: vec![SqlStructureFilter {
                    column: "field_102".to_owned(),
                    qualifier: Some(SqlStructureAlias {
                        name: "other".to_owned(),
                        quoted: false,
                    }),
                    operator: SqlStructureFilterOperator::GtEq,
                    parameter: Some("priority".to_owned()),
                    second_parameter: None,
                }],
            },
        );
        assert_eq!(filter.status, SqlStructureStatus::Supported);
        let filter_sql = filter.proposed_sql.expect("filter edit");
        assert!(filter_sql.contains("/* projection */"));
        assert!(filter_sql.contains("/* join */"));
        assert!(filter_sql.contains("WHERE other.\"field_102\" >= $priority"));
        assert_eq!(filter.base_sql, sql);

        let sort = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetSort {
                order_by: vec![SqlStructureSort {
                    column: "field_102".to_owned(),
                    qualifier: Some(SqlStructureAlias {
                        name: "t".to_owned(),
                        quoted: false,
                    }),
                    projection_alias: None,
                    direction: SqlStructureSortDirection::Desc,
                    nulls_first: None,
                }],
            },
        );
        assert_eq!(sort.status, SqlStructureStatus::Supported);
        let sort_sql = sort.proposed_sql.expect("sort edit");
        assert!(sort_sql.contains("/* projection */"));
        assert!(sort_sql.contains("/* join */"));
        assert!(sort_sql.contains("ORDER BY t.\"field_102\" DESC"));
        assert_eq!(sort.base_sql, sql);
    }

    #[test]
    fn changes_both_relations_in_a_supported_self_join_together() {
        let sql = "SELECT * FROM \"Old\" AS t INNER JOIN \"Old\" AS other ON t.field_101 = other.field_101 LIMIT 7";
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
            Some(
                "SELECT * FROM \"Task\" AS t INNER JOIN \"Task\" AS other ON t.field_101 = other.field_101 LIMIT 7"
            )
        );
        assert_eq!(
            analyze_sql_structure(proposal.proposed_sql.as_deref().unwrap(), &context()).status,
            SqlStructureStatus::Supported
        );
    }

    #[test]
    fn join_editor_fails_closed_outside_the_bounded_inner_equality_shape() {
        for sql in [
            "SELECT * FROM \"Task\" AS t LEFT JOIN \"Task\" AS other ON t.field_101 = other.field_101",
            "SELECT * FROM \"Task\" AS t CROSS JOIN \"Task\" AS other",
            "SELECT * FROM \"Task\" AS t JOIN \"Task\" AS other USING (field_101)",
            "SELECT * FROM \"Task\" AS t JOIN \"Task\" AS other ON t.field_101 = other.field_101 AND t.field_102 = other.field_102",
            "SELECT * FROM \"Task\" AS t JOIN \"Task\" AS other ON t.field_101 = other.field_101 JOIN \"Task\" AS last ON other.field_101 = last.field_101",
        ] {
            let analysis = analyze_sql_structure(sql, &context());
            assert_eq!(analysis.status, SqlStructureStatus::Unsupported, "{sql}");
            assert_eq!(
                analysis.reason,
                Some(SqlStructureReason::JoinUnsupported),
                "{sql}"
            );
        }

        let mismatch = propose_sql_structure_operation(
            "SELECT * FROM \"Task\"",
            &context(),
            SqlStructureOperation::SetJoin {
                join_kind: SqlStructureJoinKind::Inner,
                left_column: "field_101".to_owned(),
                right_column: "field_102".to_owned(),
            },
        );
        assert_eq!(mismatch.status, SqlStructureStatus::Unsupported);
        assert_eq!(mismatch.reason, Some(SqlStructureReason::JoinUnsupported));
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
    fn sort_editor_reads_and_edits_explicit_null_placement() {
        let source = "SELECT * FROM \"Task\" ORDER BY \"field_101\" ASC NULLS FIRST";
        let analysis = analyze_sql_structure(source, &context());
        assert_eq!(analysis.status, SqlStructureStatus::Supported);
        assert_eq!(analysis.order_by[0].nulls_first, Some(true));

        let proposal = propose_sql_structure_operation(
            source,
            &context(),
            SqlStructureOperation::SetSort {
                order_by: vec![SqlStructureSort {
                    column: "field_101".to_owned(),
                    qualifier: None,
                    projection_alias: None,
                    direction: SqlStructureSortDirection::Desc,
                    nulls_first: Some(false),
                }],
            },
        );
        assert_eq!(proposal.status, SqlStructureStatus::Supported);
        let proposed = proposal.proposed_sql.expect("NULL placement edit");
        assert!(proposed.contains("ORDER BY \"field_101\" DESC NULLS LAST"));
        let updated = analyze_sql_structure(&proposed, &context());
        assert_eq!(updated.status, SqlStructureStatus::Supported);
        assert_eq!(updated.order_by[0].nulls_first, Some(false));
    }

    #[test]
    fn repeated_projection_sources_are_editable_by_distinct_projection_slots() {
        let result = analyze_sql_structure(
            "SELECT \"field_101\" AS first, \"field_101\" AS second FROM \"Task\" WHERE \"field_101\" = $status ORDER BY \"_ugoite_id\"",
            &context(),
        );

        assert_eq!(result.status, SqlStructureStatus::Supported);
        assert_eq!(result.projection, vec!["field_101", "field_101"]);
        assert!(result.capabilities.columns.enabled);
        assert_eq!(result.projection_aliases.len(), 2);
        assert_eq!(result.projection_aliases[0].column, "field_101");
        assert_eq!(
            result.projection_aliases[0].alias.as_ref().unwrap().name,
            "first"
        );
        assert_eq!(result.projection_aliases[1].column, "field_101");
        assert_eq!(
            result.projection_aliases[1].alias.as_ref().unwrap().name,
            "second"
        );
        assert!(result.capabilities.filter.enabled);
        assert!(result.capabilities.sort.enabled);
    }

    #[test]
    fn set_columns_proposal_fails_closed_for_repeated_projection_sources() {
        let sql = "SELECT \"field_101\" AS first, \"field_101\" AS second FROM \"Task\"";
        let proposal = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetColumns {
                columns: vec!["field_101".to_owned()],
                aliases: vec![],
            },
        );

        assert_eq!(proposal.status, SqlStructureStatus::Unsupported);
        assert_eq!(
            proposal.reason,
            Some(SqlStructureReason::ProjectionUnsupported)
        );
        assert_eq!(proposal.base_sql, sql);
        assert_eq!(proposal.proposed_sql, None);
    }

    #[test]
    fn projection_slot_edits_reorder_and_rename_repeated_sources_independently() {
        let sql = "SELECT \"field_101\" AS first, \"field_101\" AS second FROM \"Task\" WHERE \"field_102\" > $priority ORDER BY \"_ugoite_id\"";
        let analysis = analyze_sql_structure(sql, &context());
        let proposal = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetProjectionSlots {
                slots: vec![
                    SqlStructureProjectionSlotEdit {
                        id: Some("projection-1".to_owned()),
                        column: "field_101".to_owned(),
                        alias: Some(SqlStructureAlias {
                            name: "second_copy".to_owned(),
                            quoted: true,
                        }),
                    },
                    SqlStructureProjectionSlotEdit {
                        id: Some("projection-0".to_owned()),
                        column: "field_101".to_owned(),
                        alias: Some(SqlStructureAlias {
                            name: "first_copy".to_owned(),
                            quoted: false,
                        }),
                    },
                ],
            },
        );

        assert_eq!(analysis.projection_aliases[0].column, "field_101");
        assert_eq!(analysis.projection_aliases[1].column, "field_101");
        assert_eq!(proposal.status, SqlStructureStatus::Supported);
        let proposed_sql = proposal.proposed_sql.expect("slot proposal");
        assert!(proposed_sql.starts_with(
            "SELECT \"field_101\" AS \"second_copy\", \"field_101\" AS first_copy FROM \"Task\""
        ));
        assert!(proposed_sql.ends_with("WHERE \"field_102\" > $priority ORDER BY \"_ugoite_id\""));
        let updated = analyze_sql_structure(&proposed_sql, &context());
        assert!(updated.capabilities.columns.enabled);
        assert_eq!(updated.projection, vec!["field_101", "field_101"]);
        assert_eq!(
            updated.projection_aliases[0].alias.as_ref().unwrap().name,
            "second_copy"
        );
        assert_eq!(
            updated.projection_aliases[1].alias.as_ref().unwrap().name,
            "first_copy"
        );
    }

    #[test]
    fn projection_slot_edits_reject_alias_rename_that_changes_order_by_resolution() {
        let sql = "SELECT field_101 AS field_102 FROM \"Task\" ORDER BY field_102";
        let proposal = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetProjectionSlots {
                slots: vec![SqlStructureProjectionSlotEdit {
                    id: Some("projection-0".to_owned()),
                    column: "field_101".to_owned(),
                    alias: Some(SqlStructureAlias {
                        name: "renamed".to_owned(),
                        quoted: false,
                    }),
                }],
            },
        );

        assert_eq!(proposal.status, SqlStructureStatus::Unsupported);
        assert_eq!(proposal.reason, Some(SqlStructureReason::SortUnsupported));
        assert_eq!(proposal.base_sql, sql);
        assert_eq!(proposal.proposed_sql, None);

        let legacy_proposal = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetColumns {
                columns: vec!["field_101".to_owned()],
                aliases: vec![Some(SqlStructureAlias {
                    name: "renamed".to_owned(),
                    quoted: false,
                })],
            },
        );
        assert_eq!(legacy_proposal.status, SqlStructureStatus::Unsupported);
        assert_eq!(
            legacy_proposal.reason,
            Some(SqlStructureReason::SortUnsupported)
        );
        assert_eq!(legacy_proposal.base_sql, sql);
        assert_eq!(legacy_proposal.proposed_sql, None);
    }

    #[test]
    fn projection_slot_edits_can_add_a_second_occurrence_and_expand_wildcard() {
        let sql = "SELECT \"field_101\" AS source FROM \"Task\"";
        let proposal = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetProjectionSlots {
                slots: vec![
                    SqlStructureProjectionSlotEdit {
                        id: Some("projection-0".to_owned()),
                        column: "field_101".to_owned(),
                        alias: Some(SqlStructureAlias {
                            name: "source".to_owned(),
                            quoted: false,
                        }),
                    },
                    SqlStructureProjectionSlotEdit {
                        id: None,
                        column: "field_101".to_owned(),
                        alias: Some(SqlStructureAlias {
                            name: "copy".to_owned(),
                            quoted: false,
                        }),
                    },
                ],
            },
        );
        assert_eq!(proposal.status, SqlStructureStatus::Supported);
        let proposed_sql = proposal.proposed_sql.expect("duplicate slot proposal");
        let updated = analyze_sql_structure(&proposed_sql, &context());
        assert_eq!(updated.projection, vec!["field_101", "field_101"]);
        assert_eq!(
            updated.projection_aliases[0].alias.as_ref().unwrap().name,
            "source"
        );
        assert_eq!(
            updated.projection_aliases[1].alias.as_ref().unwrap().name,
            "copy"
        );

        let wildcard = "SELECT * FROM \"Task\"";
        let expanded = propose_sql_structure_operation(
            wildcard,
            &context(),
            SqlStructureOperation::SetProjectionSlots {
                slots: vec![
                    SqlStructureProjectionSlotEdit {
                        id: None,
                        column: "field_101".to_owned(),
                        alias: None,
                    },
                    SqlStructureProjectionSlotEdit {
                        id: None,
                        column: "field_102".to_owned(),
                        alias: None,
                    },
                ],
            },
        );
        assert_eq!(expanded.status, SqlStructureStatus::Supported);
        assert!(expanded
            .proposed_sql
            .expect("wildcard expansion")
            .starts_with("SELECT \"field_101\", \"field_102\" FROM \"Task\""));
    }

    #[test]
    fn projection_slot_edits_reject_unknown_or_reused_source_ids() {
        let sql = "SELECT \"field_101\" AS first, \"field_101\" AS second FROM \"Task\"";
        for ids in [
            vec![Some("projection-0"), Some("projection-0")],
            vec![Some("projection-7")],
        ] {
            let proposal = propose_sql_structure_operation(
                sql,
                &context(),
                SqlStructureOperation::SetProjectionSlots {
                    slots: ids
                        .into_iter()
                        .map(|id| SqlStructureProjectionSlotEdit {
                            id: id.map(str::to_owned),
                            column: "field_101".to_owned(),
                            alias: None,
                        })
                        .collect(),
                },
            );
            assert_eq!(proposal.status, SqlStructureStatus::Unsupported);
            assert_eq!(
                proposal.reason,
                Some(SqlStructureReason::ProjectionUnsupported)
            );
            assert_eq!(proposal.base_sql, sql);
            assert_eq!(proposal.proposed_sql, None);
        }
    }

    #[test]
    fn analyzes_and_edits_simple_form_column_projection_aliases_losslessly() {
        let sql = "-- keep header\nSELECT \"field_102\" AS \"Total Amount\", \"field_101\" status FROM \"Task\" /* relation */ WHERE \"field_102\" >= $priority ORDER BY \"_ugoite_id\"";
        let analysis = analyze_sql_structure(sql, &context());
        assert_eq!(analysis.status, SqlStructureStatus::Supported);
        assert_eq!(analysis.projection, vec!["field_102", "field_101"]);
        assert_eq!(
            analysis.projection_aliases,
            vec![
                SqlStructureProjectionAlias {
                    column: "field_102".to_owned(),
                    alias: Some(SqlStructureAlias {
                        name: "Total Amount".to_owned(),
                        quoted: true,
                    }),
                },
                SqlStructureProjectionAlias {
                    column: "field_101".to_owned(),
                    alias: Some(SqlStructureAlias {
                        name: "status".to_owned(),
                        quoted: false,
                    }),
                },
            ]
        );

        let proposal = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetColumns {
                columns: vec!["field_102".to_owned(), "field_101".to_owned()],
                aliases: vec![
                    Some(SqlStructureAlias {
                        name: "Gross Total".to_owned(),
                        quoted: true,
                    }),
                    None,
                ],
            },
        );
        assert_eq!(proposal.status, SqlStructureStatus::Supported);
        assert_eq!(
            proposal.proposed_sql.as_deref(),
            Some(
                "-- keep header\nSELECT \"field_102\" AS \"Gross Total\", \"field_101\" FROM \"Task\" /* relation */ WHERE \"field_102\" >= $priority ORDER BY \"_ugoite_id\""
            )
        );
        assert!(proposal.requires_confirmation);
        let updated = analyze_sql_structure(proposal.proposed_sql.as_deref().unwrap(), &context());
        assert_eq!(updated.status, SqlStructureStatus::Supported);
        assert_eq!(updated.projection, analysis.projection);
        assert_eq!(
            updated.projection_aliases[0].alias.as_ref().unwrap().name,
            "Gross Total"
        );
        assert!(updated.projection_aliases[0].alias.as_ref().unwrap().quoted);
        assert_eq!(updated.projection_aliases[1].alias, None);
    }

    #[test]
    fn adding_projection_alias_preserves_other_sql_and_quoted_semantics() {
        let sql = "SELECT \"field_102\" FROM \"Task\" WHERE \"field_101\" = $status ORDER BY \"_ugoite_id\"";
        let proposal = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetColumns {
                columns: vec!["field_102".to_owned()],
                aliases: vec![Some(SqlStructureAlias {
                    name: "GrossTotal".to_owned(),
                    quoted: false,
                })],
            },
        );
        assert_eq!(proposal.status, SqlStructureStatus::Supported);
        assert_eq!(
            proposal.proposed_sql.as_deref(),
            Some(
                "SELECT \"field_102\" AS GrossTotal FROM \"Task\" WHERE \"field_101\" = $status ORDER BY \"_ugoite_id\""
            )
        );
    }

    #[test]
    fn adding_projection_alias_when_replacing_wildcard_preserves_other_clauses() {
        let sql = "SELECT * FROM \"Task\" WHERE \"field_101\" = $status ORDER BY \"_ugoite_id\"";
        let proposal = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetColumns {
                columns: vec!["field_102".to_owned()],
                aliases: vec![Some(SqlStructureAlias {
                    name: "total".to_owned(),
                    quoted: false,
                })],
            },
        );
        assert_eq!(proposal.status, SqlStructureStatus::Supported);
        assert_eq!(
            proposal.proposed_sql.as_deref(),
            Some(
                "SELECT \"field_102\" AS total FROM \"Task\" WHERE \"field_101\" = $status ORDER BY \"_ugoite_id\""
            )
        );
    }

    #[test]
    fn projection_alias_edits_fail_closed_when_they_change_order_by_binding() {
        let sql = "SELECT \"field_102\" AS \"field_101\" FROM \"Task\" ORDER BY \"field_101\"";
        let analysis = analyze_sql_structure(sql, &context());
        assert_eq!(analysis.status, SqlStructureStatus::Supported);

        let renamed = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetColumns {
                columns: vec!["field_102".to_owned()],
                aliases: vec![Some(SqlStructureAlias {
                    name: "total".to_owned(),
                    quoted: false,
                })],
            },
        );
        assert_eq!(renamed.status, SqlStructureStatus::Unsupported);
        assert_eq!(renamed.reason, Some(SqlStructureReason::SortUnsupported));
        assert_eq!(renamed.base_sql, sql);
        assert_eq!(renamed.proposed_sql, None);

        let alias_added = propose_sql_structure_operation(
            "SELECT \"field_102\" FROM \"Task\" ORDER BY \"field_101\"",
            &context(),
            SqlStructureOperation::SetColumns {
                columns: vec!["field_102".to_owned()],
                aliases: vec![Some(SqlStructureAlias {
                    name: "field_101".to_owned(),
                    quoted: true,
                })],
            },
        );
        assert_eq!(alias_added.status, SqlStructureStatus::Unsupported);
        assert_eq!(
            alias_added.reason,
            Some(SqlStructureReason::SortUnsupported)
        );
        assert_eq!(alias_added.proposed_sql, None);
    }

    #[test]
    fn order_by_resolves_projection_alias_before_a_colliding_source_column() {
        for (sql, quoted) in [
            (
                "SELECT \"field_102\" AS field_101 FROM \"Task\" ORDER BY field_101",
                false,
            ),
            (
                "SELECT \"field_102\" AS \"field_101\" FROM \"Task\" ORDER BY \"field_101\"",
                true,
            ),
        ] {
            let analysis = analyze_sql_structure(sql, &context());
            assert_eq!(analysis.status, SqlStructureStatus::Supported, "{sql}");
            assert_eq!(analysis.order_by.len(), 1, "{sql}");
            assert_eq!(analysis.order_by[0].column, "field_102", "{sql}");
            assert_eq!(
                analysis.order_by[0].projection_alias,
                Some(SqlStructureAlias {
                    name: "field_101".to_owned(),
                    quoted,
                }),
                "{sql}"
            );

            let alias_sort = propose_sql_structure_operation(
                sql,
                &context(),
                SqlStructureOperation::SetSort {
                    order_by: vec![SqlStructureSort {
                        column: "field_102".to_owned(),
                        qualifier: None,
                        projection_alias: analysis.order_by[0].projection_alias.clone(),
                        direction: SqlStructureSortDirection::Desc,
                        nulls_first: None,
                    }],
                },
            );
            assert_eq!(alias_sort.status, SqlStructureStatus::Supported, "{sql}");
            let sorted_sql = alias_sort.proposed_sql.expect("alias sort proposal");
            let expected_alias = if quoted { "\"field_101\"" } else { "field_101" };
            assert!(
                sorted_sql.contains(&format!("ORDER BY {expected_alias} DESC")),
                "{sorted_sql}"
            );
            assert!(sorted_sql.contains("\"_ugoite_id\" ASC"), "{sorted_sql}");
            let updated = analyze_sql_structure(&sorted_sql, &context());
            assert_eq!(
                updated.status,
                SqlStructureStatus::Supported,
                "{sorted_sql}"
            );
            assert_eq!(updated.order_by[0].column, "field_102");
            assert_eq!(
                updated.order_by[0].projection_alias,
                analysis.order_by[0].projection_alias
            );

            let source_sort = propose_sql_structure_operation(
                sql,
                &context(),
                SqlStructureOperation::SetSort {
                    order_by: vec![SqlStructureSort {
                        column: "field_101".to_owned(),
                        qualifier: None,
                        projection_alias: None,
                        direction: SqlStructureSortDirection::Asc,
                        nulls_first: None,
                    }],
                },
            );
            assert_eq!(source_sort.status, SqlStructureStatus::Unsupported, "{sql}");
            assert_eq!(
                source_sort.reason,
                Some(SqlStructureReason::SortUnsupported)
            );
            assert!(source_sort.proposed_sql.is_none());
        }
    }

    #[test]
    fn projection_alias_edits_fail_closed_for_comments_and_invalid_alias_shapes() {
        let commented_gap = "SELECT \"field_102\" /* keep */ AS total FROM \"Task\"";
        let removal = propose_sql_structure_operation(
            commented_gap,
            &context(),
            SqlStructureOperation::SetColumns {
                columns: vec!["field_102".to_owned()],
                aliases: vec![None],
            },
        );
        assert_eq!(removal.status, SqlStructureStatus::Unsupported);
        assert_eq!(
            removal.reason,
            Some(SqlStructureReason::ProjectionUnsupported)
        );
        assert_eq!(removal.base_sql, commented_gap);
        assert!(removal.proposed_sql.is_none());

        let comment_after_column = "SELECT \"field_102\" /* keep */ FROM \"Task\"";
        let addition = propose_sql_structure_operation(
            comment_after_column,
            &context(),
            SqlStructureOperation::SetColumns {
                columns: vec!["field_102".to_owned()],
                aliases: vec![Some(SqlStructureAlias {
                    name: "total".to_owned(),
                    quoted: false,
                })],
            },
        );
        assert_eq!(addition.status, SqlStructureStatus::Unsupported);
        assert_eq!(
            addition.reason,
            Some(SqlStructureReason::ProjectionUnsupported)
        );

        let expression = analyze_sql_structure(
            "SELECT \"field_102\" + 1 AS total FROM \"Task\"",
            &context(),
        );
        assert_eq!(expression.status, SqlStructureStatus::Unsupported);
        assert_eq!(
            expression.reason,
            Some(SqlStructureReason::ProjectionUnsupported)
        );

        let invalid_unquoted = propose_sql_structure_operation(
            "SELECT \"field_102\" FROM \"Task\"",
            &context(),
            SqlStructureOperation::SetColumns {
                columns: vec!["field_102".to_owned()],
                aliases: vec![Some(SqlStructureAlias {
                    name: "Total Amount".to_owned(),
                    quoted: false,
                })],
            },
        );
        assert_eq!(invalid_unquoted.status, SqlStructureStatus::Unsupported);
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
        assert!(analysis.capabilities.columns.enabled);

        let repeated_literals = analyze_sql_structure("SELECT 1, 1", &relation_free_context);
        assert_eq!(repeated_literals.status, SqlStructureStatus::Supported);
        assert_eq!(repeated_literals.literal_projection.len(), 2);
        assert!(repeated_literals.capabilities.columns.enabled);
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
    fn edits_typed_relation_free_literal_projection_without_adding_a_relation() {
        let relation_free_context = SqlStructureContext {
            relation: String::new(),
            columns: Vec::new(),
            variables: Vec::new(),
            stable_key: Vec::new(),
        };
        let source = "-- keep\nSELECT 1";
        let analysis = analyze_sql_structure(source, &relation_free_context);

        assert_eq!(analysis.status, SqlStructureStatus::Supported);
        assert_eq!(analysis.projection, vec!["1"]);
        assert_eq!(
            analysis.literal_projection,
            vec![SqlStructureLiteral::Integer("1".to_owned())]
        );
        assert!(analysis.capabilities.columns.enabled);

        let negative = analyze_sql_structure("SELECT -1", &relation_free_context);
        assert_eq!(negative.status, SqlStructureStatus::Supported);
        assert_eq!(
            negative.literal_projection,
            vec![SqlStructureLiteral::Integer("-1".to_owned())]
        );

        let proposal = propose_sql_structure_operation(
            source,
            &relation_free_context,
            SqlStructureOperation::SetLiteralProjection {
                values: vec![
                    SqlStructureLiteral::Integer("42".to_owned()),
                    SqlStructureLiteral::String("O'Reilly".to_owned()),
                    SqlStructureLiteral::Boolean(false),
                    SqlStructureLiteral::Null,
                ],
            },
        );

        assert_eq!(proposal.status, SqlStructureStatus::Supported);
        assert_eq!(
            proposal.proposed_sql.as_deref(),
            Some("-- keep\nSELECT 42, 'O''Reilly', false, NULL")
        );
        let edited_sql = proposal.proposed_sql.expect("literal projection proposal");
        let edited = analyze_sql_structure(&edited_sql, &relation_free_context);
        assert_eq!(edited.status, SqlStructureStatus::Supported);
        assert_eq!(
            edited.literal_projection,
            vec![
                SqlStructureLiteral::Integer("42".to_owned()),
                SqlStructureLiteral::String("O'Reilly".to_owned()),
                SqlStructureLiteral::Boolean(false),
                SqlStructureLiteral::Null,
            ]
        );
        assert!(!edited_sql.to_ascii_uppercase().contains(" FROM "));

        let negative_proposal = propose_sql_structure_operation(
            "SELECT -1",
            &relation_free_context,
            SqlStructureOperation::SetLiteralProjection {
                values: vec![SqlStructureLiteral::Integer("-2".to_owned())],
            },
        );
        assert_eq!(
            negative_proposal.status,
            SqlStructureStatus::Supported,
            "{negative_proposal:?}"
        );
        assert_eq!(negative_proposal.proposed_sql.as_deref(), Some("SELECT -2"));
    }

    #[test]
    fn literal_projection_edit_rejects_invalid_values_and_relation_contexts() {
        let relation_free_context = SqlStructureContext {
            relation: String::new(),
            columns: Vec::new(),
            variables: Vec::new(),
            stable_key: Vec::new(),
        };
        for (sql, operation) in [
            (
                "SELECT 1",
                SqlStructureOperation::SetLiteralProjection {
                    values: vec![SqlStructureLiteral::Integer(
                        "9223372036854775808".to_owned(),
                    )],
                },
            ),
            (
                "SELECT 1 /* retained */, 2",
                SqlStructureOperation::SetLiteralProjection {
                    values: vec![SqlStructureLiteral::Integer("2".to_owned())],
                },
            ),
        ] {
            let proposal = propose_sql_structure_operation(sql, &relation_free_context, operation);
            assert_eq!(proposal.status, SqlStructureStatus::Unsupported, "{sql}");
            assert!(proposal.proposed_sql.is_none(), "{sql}");
        }

        let form_context = context();
        let proposal = propose_sql_structure_operation(
            "SELECT 1 FROM \"Task\"",
            &form_context,
            SqlStructureOperation::SetLiteralProjection {
                values: vec![SqlStructureLiteral::Integer("2".to_owned())],
            },
        );
        assert_eq!(proposal.status, SqlStructureStatus::Unsupported);
        assert!(proposal.proposed_sql.is_none());
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
                aliases: vec![],
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

                    qualifier: None,
                    operator: SqlStructureFilterOperator::GtEq,
                    parameter: Some("priority".to_owned()),
                    second_parameter: None,
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
                    qualifier: None,
                    projection_alias: None,
                    direction: SqlStructureSortDirection::Desc,
                    nulls_first: None,
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
    fn analyzes_and_edits_nested_boolean_filter_trees_with_typed_leaves() {
        let sql = "SELECT * FROM \"Task\" WHERE (\"field_101\" = $status OR \"field_102\" >= $priority) AND NOT (\"field_101\" != $status)";
        let analysis = analyze_sql_structure(sql, &context());
        assert_eq!(analysis.status, SqlStructureStatus::Supported);
        assert!(analysis.filters.is_empty());
        assert!(!analysis.capabilities.filter.enabled);
        assert!(analysis.capabilities.filter_expression.enabled);
        assert_eq!(analysis.referenced_variables, ["priority", "status"]);
        assert!(matches!(
            analysis.filter_expression,
            Some(SqlStructureFilterExpression::And { ref expressions })
                if expressions.len() == 2
                    && matches!(expressions[0], SqlStructureFilterExpression::Or { .. })
                    && matches!(expressions[1], SqlStructureFilterExpression::Not { .. })
        ));
        let lossy_flat_edit = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetFilters {
                filters: vec![SqlStructureFilter {
                    column: "field_101".to_owned(),

                    qualifier: None,
                    operator: SqlStructureFilterOperator::Eq,
                    parameter: Some("status".to_owned()),
                    second_parameter: None,
                }],
            },
        );
        assert_eq!(lossy_flat_edit.status, SqlStructureStatus::Unsupported);
        assert_eq!(
            lossy_flat_edit.reason,
            Some(SqlStructureReason::FilterUnsupported)
        );

        let expression = SqlStructureFilterExpression::And {
            expressions: vec![
                SqlStructureFilterExpression::Predicate {
                    column: "field_101".to_owned(),
                    qualifier: None,
                    operator: SqlStructureFilterOperator::Eq,
                    parameter: Some("status".to_owned()),
                    second_parameter: None,
                },
                SqlStructureFilterExpression::Or {
                    expressions: vec![
                        SqlStructureFilterExpression::Predicate {
                            column: "field_102".to_owned(),
                            qualifier: None,
                            operator: SqlStructureFilterOperator::GtEq,
                            parameter: Some("priority".to_owned()),
                            second_parameter: None,
                        },
                        SqlStructureFilterExpression::Not {
                            expression: Box::new(SqlStructureFilterExpression::Predicate {
                                column: "field_101".to_owned(),
                                qualifier: None,
                                operator: SqlStructureFilterOperator::NotEq,
                                parameter: Some("status".to_owned()),
                                second_parameter: None,
                            }),
                        },
                    ],
                },
            ],
        };
        let proposal = propose_sql_structure_operation(
            "SELECT * FROM \"Task\" ORDER BY \"field_102\" DESC",
            &context(),
            SqlStructureOperation::SetFilterExpression {
                expression: Some(expression.clone()),
            },
        );
        assert_eq!(proposal.status, SqlStructureStatus::Supported);
        assert!(proposal.requires_confirmation);
        let proposed_sql = proposal.proposed_sql.expect("tree edit should propose SQL");
        let edited = analyze_sql_structure(&proposed_sql, &context());
        assert_eq!(edited.status, SqlStructureStatus::Supported);
        assert_eq!(edited.filter_expression, Some(expression));
        assert!(proposed_sql.contains("ORDER BY \"field_102\" DESC"));
    }

    #[test]
    fn typed_filter_edits_support_membership_ranges_and_string_patterns() {
        let mut context = context();
        context.variables.push(SqlStructureVariable {
            name: "status_upper".to_owned(),
            variable_type: SqlStructureVariableType::String,
        });
        context.variables.push(SqlStructureVariable {
            name: "status_pattern".to_owned(),
            variable_type: SqlStructureVariableType::String,
        });

        let range_sql =
            "SELECT * FROM \"Task\" WHERE \"field_101\" BETWEEN $status AND $status_upper";
        let range = analyze_sql_structure(range_sql, &context);
        assert_eq!(range.status, SqlStructureStatus::Supported, "{range:?}");
        assert_eq!(range.referenced_variables, ["status", "status_upper"]);
        assert!(matches!(
            range.filter_expression,
            Some(SqlStructureFilterExpression::Predicate {
                operator: SqlStructureFilterOperator::Between,
                parameter: Some(ref lower),
                second_parameter: Some(ref upper),
                ..
            }) if lower == "status" && upper == "status_upper"
        ));

        let between = SqlStructureFilterExpression::Predicate {
            column: "field_101".to_owned(),
            qualifier: None,
            operator: SqlStructureFilterOperator::NotBetween,
            parameter: Some("status".to_owned()),
            second_parameter: Some("status_upper".to_owned()),
        };
        let proposal = propose_sql_structure_operation(
            "SELECT * FROM \"Task\"",
            &context,
            SqlStructureOperation::SetFilterExpression {
                expression: Some(between.clone()),
            },
        );
        assert_eq!(
            proposal.status,
            SqlStructureStatus::Supported,
            "{proposal:?}"
        );
        let proposed = proposal.proposed_sql.expect("typed range edit");
        assert!(proposed.contains("NOT BETWEEN $status AND $status_upper"));
        assert_eq!(
            analyze_sql_structure(&proposed, &context).filter_expression,
            Some(between)
        );

        for (operator, sql_operator) in [
            (SqlStructureFilterOperator::In, "IN"),
            (SqlStructureFilterOperator::Like, "LIKE"),
            (SqlStructureFilterOperator::Ilike, "ILIKE"),
        ] {
            let parameter = if operator == SqlStructureFilterOperator::In {
                "status"
            } else {
                "status_pattern"
            };
            let expression = SqlStructureFilterExpression::Predicate {
                column: "field_101".to_owned(),
                qualifier: None,
                operator,
                parameter: Some(parameter.to_owned()),
                second_parameter: None,
            };
            let proposal = propose_sql_structure_operation(
                "SELECT * FROM \"Task\"",
                &context,
                SqlStructureOperation::SetFilterExpression {
                    expression: Some(expression.clone()),
                },
            );
            assert_eq!(
                proposal.status,
                SqlStructureStatus::Supported,
                "{operator:?}"
            );
            let proposed = proposal.proposed_sql.expect("typed predicate edit");
            assert!(proposed.contains(sql_operator), "{proposed}");
            assert_eq!(
                analyze_sql_structure(&proposed, &context).filter_expression,
                Some(expression)
            );
        }

        let mut mismatched_context = context.clone();
        mismatched_context
            .variables
            .iter_mut()
            .find(|variable| variable.name == "status_upper")
            .unwrap()
            .variable_type = SqlStructureVariableType::Integer;
        let mismatch = analyze_sql_structure(range_sql, &mismatched_context);
        assert_eq!(mismatch.status, SqlStructureStatus::Unsupported);
        assert_eq!(
            mismatch.reason,
            Some(SqlStructureReason::VariableTypeMismatch)
        );
    }

    #[test]
    fn column_comparison_filters_are_typed_and_round_trip() {
        let context = context();
        let expression = SqlStructureFilterExpression::ColumnComparison {
            left: SqlStructureColumnReference {
                column: "field_101".to_owned(),
                qualifier: None,
            },
            operator: SqlStructureFilterOperator::Eq,
            right: SqlStructureColumnReference {
                column: "_ugoite_id".to_owned(),
                qualifier: None,
            },
        };
        let proposal = propose_sql_structure_operation(
            "SELECT * FROM \"Task\"",
            &context,
            SqlStructureOperation::SetFilterExpression {
                expression: Some(expression.clone()),
            },
        );
        assert_eq!(
            proposal.status,
            SqlStructureStatus::Supported,
            "{proposal:?}"
        );
        let proposed = proposal.proposed_sql.expect("column comparison proposal");
        assert!(proposed.contains("\"field_101\" = \"_ugoite_id\""));
        let applied = apply_sql_document_node_edit_transaction(
            &proposal.base_sql,
            proposal
                .document_node_edit
                .as_ref()
                .expect("column comparison has a typed edit transaction"),
        )
        .expect("column comparison edit applies atomically");
        assert_eq!(applied.document.to_sql(), proposed);
        let undone = crate::sql_document::apply_sql_document_edit_transaction(
            &applied.document.to_sql(),
            &applied.undo,
        )
        .expect("column comparison has a guarded undo");
        assert_eq!(undone.document.to_sql(), proposal.base_sql);
        let reparsed = analyze_sql_structure(&proposed, &context);
        assert_eq!(
            reparsed.status,
            SqlStructureStatus::Supported,
            "{reparsed:?}"
        );
        assert_eq!(reparsed.filter_expression, Some(expression));

        let mismatched = SqlStructureFilterExpression::ColumnComparison {
            left: SqlStructureColumnReference {
                column: "field_101".to_owned(),
                qualifier: None,
            },
            operator: SqlStructureFilterOperator::Eq,
            right: SqlStructureColumnReference {
                column: "field_102".to_owned(),
                qualifier: None,
            },
        };
        let rejected = propose_sql_structure_operation(
            "SELECT * FROM \"Task\"",
            &context,
            SqlStructureOperation::SetFilterExpression {
                expression: Some(mismatched),
            },
        );
        assert_eq!(rejected.status, SqlStructureStatus::Unsupported);
        assert_eq!(
            rejected.reason,
            Some(SqlStructureReason::VariableTypeMismatch)
        );
    }

    #[test]
    fn nested_boolean_filter_edits_preserve_join_qualifiers() {
        let sql = "SELECT * FROM \"Task\" AS t INNER JOIN \"Task\" AS other ON t.field_101 = other.field_101 WHERE (t.field_101 = $status OR other.field_102 >= $priority) AND NOT (other.field_101 != $status) ORDER BY other._ugoite_id ASC, t._ugoite_id ASC";
        let analysis = analyze_sql_structure(sql, &context());
        assert_eq!(analysis.status, SqlStructureStatus::Supported);
        assert_eq!(analysis.filters, Vec::<SqlStructureFilter>::new());

        let expected = SqlStructureFilterExpression::And {
            expressions: vec![
                SqlStructureFilterExpression::Or {
                    expressions: vec![
                        SqlStructureFilterExpression::Predicate {
                            column: "field_101".to_owned(),
                            qualifier: Some(SqlStructureAlias {
                                name: "t".to_owned(),
                                quoted: false,
                            }),
                            operator: SqlStructureFilterOperator::Eq,
                            parameter: Some("status".to_owned()),
                            second_parameter: None,
                        },
                        SqlStructureFilterExpression::Predicate {
                            column: "field_102".to_owned(),
                            qualifier: Some(SqlStructureAlias {
                                name: "other".to_owned(),
                                quoted: false,
                            }),
                            operator: SqlStructureFilterOperator::GtEq,
                            parameter: Some("priority".to_owned()),
                            second_parameter: None,
                        },
                    ],
                },
                SqlStructureFilterExpression::Not {
                    expression: Box::new(SqlStructureFilterExpression::Predicate {
                        column: "field_101".to_owned(),
                        qualifier: Some(SqlStructureAlias {
                            name: "other".to_owned(),
                            quoted: false,
                        }),
                        operator: SqlStructureFilterOperator::NotEq,
                        parameter: Some("status".to_owned()),
                        second_parameter: None,
                    }),
                },
            ],
        };
        assert_eq!(analysis.filter_expression, Some(expected.clone()));

        let proposal = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetFilterExpression {
                expression: Some(expected.clone()),
            },
        );
        assert_eq!(proposal.status, SqlStructureStatus::Supported);
        let proposed = proposal.proposed_sql.expect("joined filter rewrite");
        assert!(proposed.contains("t.\"field_101\" = $status"));
        assert!(proposed.contains("other.\"field_102\" >= $priority"));
        assert!(proposed.contains("NOT"));
        assert!(proposed.contains("other.\"field_101\" <> $status"));
        let edited = analyze_sql_structure(&proposed, &context());
        assert_eq!(edited.status, SqlStructureStatus::Supported);
        assert_eq!(edited.filter_expression, Some(expected));
    }

    #[test]
    fn filter_expression_edits_preserve_unrelated_sql_trivia() {
        let expression = SqlStructureFilterExpression::Not {
            expression: Box::new(SqlStructureFilterExpression::Predicate {
                column: "field_101".to_owned(),
                qualifier: None,
                operator: SqlStructureFilterOperator::Eq,
                parameter: Some("status".to_owned()),
                second_parameter: None,
            }),
        };
        let sql = "SELECT /*keep projection*/ * FROM \"Task\" /*keep from*/ WHERE \"field_101\" = $status /*keep where*/ ORDER BY \"field_102\" DESC -- keep tail";
        let proposal = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetFilterExpression {
                expression: Some(expression),
            },
        );

        assert_eq!(proposal.status, SqlStructureStatus::Supported);
        let proposed = proposal
            .proposed_sql
            .expect("filter edit should be proposed");
        for comment in [
            "/*keep projection*/",
            "/*keep from*/",
            "/*keep where*/",
            "-- keep tail",
        ] {
            assert!(proposed.contains(comment), "missing {comment}: {proposed}");
        }
        assert!(proposed.contains("NOT"), "unexpected rewrite: {proposed}");
        assert!(proposed.contains("ORDER BY \"field_102\" DESC"));
        let edited = analyze_sql_structure(&proposed, &context());
        assert_eq!(edited.status, SqlStructureStatus::Supported);
        assert!(matches!(
            edited.filter_expression,
            Some(SqlStructureFilterExpression::Not { expression })
                if matches!(expression.as_ref(), SqlStructureFilterExpression::Predicate {
                    column,
                    operator: SqlStructureFilterOperator::Eq,
                    parameter: Some(parameter),
                    ..
                } if column == "field_101" && parameter == "status")
        ));
    }

    #[test]
    fn filter_expression_edit_fails_closed_for_comments_inside_rewritten_expression() {
        let sql = "SELECT * FROM \"Task\" WHERE \"field_101\" /*keep predicate*/ = $status";
        let proposal = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetFilterExpression {
                expression: Some(SqlStructureFilterExpression::Not {
                    expression: Box::new(SqlStructureFilterExpression::Predicate {
                        column: "field_101".to_owned(),
                        qualifier: None,
                        operator: SqlStructureFilterOperator::Eq,
                        parameter: Some("status".to_owned()),
                        second_parameter: None,
                    }),
                }),
            },
        );

        assert_eq!(proposal.status, SqlStructureStatus::Unsupported);
        assert_eq!(proposal.proposed_sql, None);
        assert!(proposal.base_sql.contains("/*keep predicate*/"));
    }

    #[test]
    fn adding_and_removing_filters_preserves_trivia_outside_where() {
        let expression = SqlStructureFilterExpression::Predicate {
            column: "field_101".to_owned(),
            qualifier: None,
            operator: SqlStructureFilterOperator::Eq,
            parameter: Some("status".to_owned()),
            second_parameter: None,
        };
        let insert = propose_sql_structure_operation(
            "SELECT /*keep projection*/ * FROM \"Task\" /*keep from*/ ORDER BY \"field_102\" DESC -- keep tail",
            &context(),
            SqlStructureOperation::SetFilterExpression {
                expression: Some(expression.clone()),
            },
        );
        assert_eq!(insert.status, SqlStructureStatus::Supported);
        let inserted = insert.proposed_sql.expect("filter insertion");
        for comment in ["/*keep projection*/", "/*keep from*/", "-- keep tail"] {
            assert!(inserted.contains(comment), "missing {comment}: {inserted}");
        }
        assert_eq!(
            analyze_sql_structure(&inserted, &context()).status,
            SqlStructureStatus::Supported
        );

        let remove = propose_sql_structure_operation(
            "SELECT /*keep projection*/ * FROM \"Task\" /*keep from*/ WHERE \"field_101\" = $status ORDER BY \"field_102\" DESC -- keep tail",
            &context(),
            SqlStructureOperation::SetFilterExpression { expression: None },
        );
        assert_eq!(remove.status, SqlStructureStatus::Supported);
        let removed = remove.proposed_sql.expect("filter removal");
        for comment in ["/*keep projection*/", "/*keep from*/", "-- keep tail"] {
            assert!(removed.contains(comment), "missing {comment}: {removed}");
        }
        assert!(!removed.contains(" WHERE "));
        assert_eq!(
            analyze_sql_structure(&removed, &context()).status,
            SqlStructureStatus::Supported
        );
    }

    #[test]
    fn set_filter_expression_requires_field_but_allows_explicit_null() {
        let missing = serde_json::from_value::<SqlStructureOperation>(serde_json::json!({
            "kind": "set_filter_expression",
        }));
        assert!(missing.is_err(), "missing expression must not clear WHERE");

        let explicit_null = serde_json::from_value::<SqlStructureOperation>(serde_json::json!({
            "kind": "set_filter_expression",
            "expression": null,
        }))
        .expect("explicit null means clear the filter tree");
        assert_eq!(
            explicit_null,
            SqlStructureOperation::SetFilterExpression { expression: None }
        );
    }

    #[test]
    fn wide_flat_conjunctions_remain_editable_with_bounded_tree_depth() {
        let predicates = std::iter::repeat_n("\"field_101\" = $status", 65)
            .collect::<Vec<_>>()
            .join(" AND ");
        let sql = format!("SELECT * FROM \"Task\" WHERE {predicates}");
        let analysis = analyze_sql_structure(&sql, &context());

        assert_eq!(analysis.status, SqlStructureStatus::Supported);
        assert!(analysis.capabilities.filter.enabled);
        assert!(analysis.capabilities.filter_expression.enabled);
        assert!(matches!(
            analysis.filter_expression,
            Some(SqlStructureFilterExpression::And { ref expressions })
                if expressions.len() == 65
        ));
    }

    #[test]
    fn boolean_filter_tree_rejects_malformed_groups_and_typed_leaf_mismatches() {
        let single_child_group = propose_sql_structure_operation(
            "SELECT * FROM \"Task\"",
            &context(),
            SqlStructureOperation::SetFilterExpression {
                expression: Some(SqlStructureFilterExpression::Or {
                    expressions: vec![SqlStructureFilterExpression::Predicate {
                        column: "field_101".to_owned(),
                        qualifier: None,
                        operator: SqlStructureFilterOperator::Eq,
                        parameter: Some("status".to_owned()),
                        second_parameter: None,
                    }],
                }),
            },
        );
        assert_eq!(single_child_group.status, SqlStructureStatus::Unsupported);
        assert_eq!(
            single_child_group.reason,
            Some(SqlStructureReason::FilterUnsupported)
        );

        let mismatched_leaf = propose_sql_structure_operation(
            "SELECT * FROM \"Task\"",
            &context(),
            SqlStructureOperation::SetFilterExpression {
                expression: Some(SqlStructureFilterExpression::Predicate {
                    column: "field_102".to_owned(),
                    qualifier: None,
                    operator: SqlStructureFilterOperator::Eq,
                    parameter: Some("status".to_owned()),
                    second_parameter: None,
                }),
            },
        );
        assert_eq!(mismatched_leaf.status, SqlStructureStatus::Unsupported);
        assert_eq!(
            mismatched_leaf.reason,
            Some(SqlStructureReason::VariableTypeMismatch)
        );

        let mut nested = SqlStructureFilterExpression::Predicate {
            column: "field_101".to_owned(),
            qualifier: None,
            operator: SqlStructureFilterOperator::Eq,
            parameter: Some("status".to_owned()),
            second_parameter: None,
        };
        for _ in 0..=MAX_FILTER_EXPRESSION_DEPTH {
            nested = SqlStructureFilterExpression::Not {
                expression: Box::new(nested),
            };
        }
        let excessive_depth = propose_sql_structure_operation(
            "SELECT * FROM \"Task\"",
            &context(),
            SqlStructureOperation::SetFilterExpression {
                expression: Some(nested),
            },
        );
        assert_eq!(excessive_depth.status, SqlStructureStatus::Unsupported);
        assert_eq!(
            excessive_depth.reason,
            Some(SqlStructureReason::FilterUnsupported)
        );
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
                    aliases: vec![],
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
                aliases: vec![],
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
    fn row_limit_is_typed_editable_and_preserves_surrounding_sql() {
        let context = context();
        let source = "-- retained\nSELECT * FROM \"Task\" ORDER BY \"_ugoite_id\" LIMIT 0005 OFFSET 10; -- tail\n";
        let analysis = analyze_sql_structure(source, &context);
        assert_eq!(analysis.status, SqlStructureStatus::Supported);
        assert!(analysis.capabilities.row_limit.enabled);
        assert_eq!(
            analysis.row_limit,
            Some(SqlStructureRowLimit {
                limit: Some("0005".to_owned()),
                offset: Some("10".to_owned()),
            })
        );

        let proposal = propose_sql_structure_operation(
            source,
            &context,
            SqlStructureOperation::SetRowLimit {
                limit: Some("25".to_owned()),
                offset: Some("2".to_owned()),
            },
        );
        assert_applied_node_proposal(&proposal, SqlDocumentNodeKind::LimitClause);
        assert_eq!(
            proposal.proposed_sql.as_deref(),
            Some(
                "-- retained\nSELECT * FROM \"Task\" ORDER BY \"_ugoite_id\" LIMIT 25 OFFSET 2; -- tail\n"
            )
        );
    }

    #[test]
    fn row_limit_insert_clear_and_invalid_values_are_handled_explicitly() {
        let context = context();
        assert!(serde_json::from_str::<SqlStructureOperation>(
            r#"{"kind":"set_row_limit","limit":"5"}"#
        )
        .is_err());
        let insert = propose_sql_structure_operation(
            "SELECT * FROM \"Task\"; -- retained",
            &context,
            SqlStructureOperation::SetRowLimit {
                limit: None,
                offset: Some("9007199254740993".to_owned()),
            },
        );
        assert_applied_node_proposal(&insert, SqlDocumentNodeKind::Query);
        assert!(insert
            .proposed_sql
            .as_deref()
            .is_some_and(|sql| sql.contains("OFFSET 9007199254740993; -- retained")));

        let clear = propose_sql_structure_operation(
            "SELECT * FROM \"Task\" LIMIT 5 OFFSET 10",
            &context,
            SqlStructureOperation::SetRowLimit {
                limit: None,
                offset: None,
            },
        );
        assert_eq!(clear.status, SqlStructureStatus::Supported);
        assert_eq!(
            clear.proposed_sql.as_deref(),
            Some("SELECT * FROM \"Task\" ")
        );
        assert_applied_node_proposal(&clear, SqlDocumentNodeKind::LimitClause);

        let invalid = propose_sql_structure_operation(
            "SELECT * FROM \"Task\"",
            &context,
            SqlStructureOperation::SetRowLimit {
                limit: Some("1; DROP TABLE x".to_owned()),
                offset: None,
            },
        );
        assert_eq!(invalid.status, SqlStructureStatus::Unsupported);
        assert_eq!(
            invalid.reason,
            Some(SqlStructureReason::RowLimitUnsupported)
        );
        assert!(invalid.proposed_sql.is_none());

        let overflow = propose_sql_structure_operation(
            "SELECT * FROM \"Task\"",
            &context,
            SqlStructureOperation::SetRowLimit {
                limit: Some("9223372036854775808".to_owned()),
                offset: None,
            },
        );
        assert_eq!(overflow.status, SqlStructureStatus::Unsupported);
        assert_eq!(
            overflow.reason,
            Some(SqlStructureReason::RowLimitUnsupported)
        );
    }

    #[test]
    fn row_limit_remains_editable_for_cte_and_set_operation_documents() {
        let context = context();
        for (source, expected) in [
            (
                "WITH q AS (SELECT * FROM \"Task\") SELECT * FROM q",
                "WITH q AS (SELECT * FROM \"Task\") SELECT * FROM q LIMIT 7",
            ),
            (
                "SELECT * FROM \"Task\" UNION ALL SELECT * FROM \"Task\"",
                "SELECT * FROM \"Task\" UNION ALL SELECT * FROM \"Task\" LIMIT 7",
            ),
            (
                "SELECT CASE WHEN \"field_102\" > 0 THEN 1 ELSE 0 END AS positive FROM \"Task\"",
                "SELECT CASE WHEN \"field_102\" > 0 THEN 1 ELSE 0 END AS positive FROM \"Task\" LIMIT 7",
            ),
        ] {
            let analysis = analyze_sql_structure(source, &context);
            assert_eq!(analysis.status, SqlStructureStatus::Unsupported, "{source}");
            assert!(analysis.capabilities.row_limit.enabled, "{source}");
            assert!(!analysis.capabilities.columns.enabled, "{source}");
            let proposal = propose_sql_structure_operation(
                source,
                &context,
                SqlStructureOperation::SetRowLimit {
                    limit: Some("7".to_owned()),
                    offset: None,
                },
            );
            assert_eq!(proposal.proposed_sql.as_deref(), Some(expected), "{source}");
            assert_applied_node_proposal(&proposal, SqlDocumentNodeKind::Query);
        }
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
        ] {
            let result = analyze_sql_structure(sql, &context());
            assert_eq!(result.status, SqlStructureStatus::Unsupported, "{sql}");
            assert_eq!(result.reason, Some(reason), "{sql}");
            assert!(!result.capabilities.columns.enabled, "{sql}");
            assert!(!result.capabilities.filter.enabled, "{sql}");
            assert!(!result.capabilities.sort.enabled, "{sql}");
        }

        let like = analyze_sql_structure(
            "SELECT * FROM \"Task\" WHERE \"field_101\" LIKE $status",
            &context(),
        );
        assert_eq!(like.status, SqlStructureStatus::Supported);
        assert!(like.capabilities.filter_expression.enabled);
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
                arguments: vec![],
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
        assert!(analysis.aggregates[1].arguments.is_empty());
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
    fn analyzes_typed_having_count_sum_and_boolean_trees() {
        let sql = "SELECT \"field_101\", COUNT(*) FROM \"Task\" WHERE \"field_101\" = $status GROUP BY \"field_101\" HAVING COUNT(*) >= 2 AND (SUM(\"field_102\") > $priority OR NOT (SUM(\"field_102\") = 0.25))";
        let analysis = analyze_sql_structure(sql, &context());

        assert_eq!(
            analysis.status,
            SqlStructureStatus::Supported,
            "{analysis:?}"
        );
        assert!(analysis.capabilities.having.enabled);
        assert_eq!(
            analysis.having_expression,
            Some(SqlStructureHavingExpression::And {
                expressions: vec![
                    SqlStructureHavingExpression::Predicate {
                        aggregate: SqlStructureAggregate {
                            function: SqlStructureAggregateFunction::Count,
                            column: None,
                            arguments: vec![],
                            distinct: false,
                            alias: None,
                        },
                        operator: SqlStructureHavingOperator::GtEq,
                        value: SqlStructureHavingValue::Integer("2".to_owned()),
                    },
                    SqlStructureHavingExpression::Or {
                        expressions: vec![
                            SqlStructureHavingExpression::Predicate {
                                aggregate: SqlStructureAggregate {
                                    function: SqlStructureAggregateFunction::Sum,
                                    column: Some("field_102".to_owned()),
                                    arguments: vec![],
                                    distinct: false,
                                    alias: None,
                                },
                                operator: SqlStructureHavingOperator::Gt,
                                value: SqlStructureHavingValue::Variable("priority".to_owned()),
                            },
                            SqlStructureHavingExpression::Not {
                                expression: Box::new(SqlStructureHavingExpression::Predicate {
                                    aggregate: SqlStructureAggregate {
                                        function: SqlStructureAggregateFunction::Sum,
                                        column: Some("field_102".to_owned()),
                                        arguments: vec![],
                                        distinct: false,
                                        alias: None,
                                    },
                                    operator: SqlStructureHavingOperator::Eq,
                                    value: SqlStructureHavingValue::Numeric("0.25".to_owned()),
                                }),
                            },
                        ],
                    },
                ],
            })
        );
        assert_eq!(analysis.referenced_variables, vec!["priority", "status"]);
    }

    #[test]
    fn typed_date_and_timestamp_literals_round_trip_in_having() {
        let context = SqlStructureContext {
            relation: "Task".to_owned(),
            columns: vec![
                SqlStructureColumn {
                    name: "day".to_owned(),
                    column_type: SqlStructureColumnType::Date,
                },
                SqlStructureColumn {
                    name: "created_at".to_owned(),
                    column_type: SqlStructureColumnType::Timestamp,
                },
            ],
            variables: vec![],
            stable_key: vec![],
        };
        let sql = "SELECT MIN(\"day\") FROM \"Task\" HAVING MIN(\"day\") >= DATE '2026-10-21'";
        let analysis = analyze_sql_structure(sql, &context);
        assert_eq!(
            analysis.status,
            SqlStructureStatus::Supported,
            "{analysis:?}"
        );
        assert!(matches!(
            analysis.having_expression,
            Some(SqlStructureHavingExpression::Predicate {
                value: SqlStructureHavingValue::Date(ref value),
                ..
            }) if value == "2026-10-21"
        ));
        let proposal = propose_sql_structure_operation(
            sql,
            &context,
            SqlStructureOperation::SetHaving {
                expression: Some(SqlStructureHavingExpression::Predicate {
                    aggregate: SqlStructureAggregate {
                        function: SqlStructureAggregateFunction::Max,
                        column: Some("created_at".to_owned()),
                        arguments: vec![],
                        distinct: false,
                        alias: None,
                    },
                    operator: SqlStructureHavingOperator::Gt,
                    value: SqlStructureHavingValue::Timestamp("2026-10-21 12:30:00".to_owned()),
                }),
            },
        );
        assert_eq!(
            proposal.status,
            SqlStructureStatus::Supported,
            "{proposal:?}"
        );
        assert!(proposal.proposed_sql.as_deref().is_some_and(|sql| {
            sql.contains("MAX(\"created_at\") > TIMESTAMP '2026-10-21 12:30:00'")
        }));
    }

    #[test]
    fn having_edit_preserves_unrelated_source_comments_and_emits_typed_node_edit() {
        let expression = SqlStructureHavingExpression::And {
            expressions: vec![
                SqlStructureHavingExpression::Predicate {
                    aggregate: SqlStructureAggregate {
                        function: SqlStructureAggregateFunction::Count,
                        column: None,
                        arguments: vec![],
                        distinct: false,
                        alias: None,
                    },
                    operator: SqlStructureHavingOperator::GtEq,
                    value: SqlStructureHavingValue::Integer("2".to_owned()),
                },
                SqlStructureHavingExpression::Not {
                    expression: Box::new(SqlStructureHavingExpression::Predicate {
                        aggregate: SqlStructureAggregate {
                            function: SqlStructureAggregateFunction::Sum,
                            column: Some("field_102".to_owned()),
                            arguments: vec![],
                            distinct: false,
                            alias: None,
                        },
                        operator: SqlStructureHavingOperator::Gt,
                        value: SqlStructureHavingValue::Variable("priority".to_owned()),
                    }),
                },
            ],
        };
        let sql = "-- leading\nSELECT /* projection */ \"field_101\", COUNT(*) FROM \"Task\" /* relation */ WHERE \"field_101\" = $status /* where */ GROUP BY \"field_101\" /* after group */ ORDER BY \"field_101\" ASC -- trailing\n";
        assert_eq!(
            analyze_sql_structure(sql, &context()).status,
            SqlStructureStatus::Supported
        );
        let proposal = propose_sql_structure_operation(
            sql,
            &context(),
            SqlStructureOperation::SetHaving {
                expression: Some(expression.clone()),
            },
        );

        assert_eq!(
            proposal.status,
            SqlStructureStatus::Supported,
            "{proposal:?}"
        );
        // A missing HAVING clause is inserted immediately before ORDER BY,
        // which is outside the sqlparser SELECT span, so the smallest
        // source-preserving existing node is the surrounding query node.
        assert_applied_node_proposal(&proposal, SqlDocumentNodeKind::Query);
        let proposed = proposal.proposed_sql.expect("HAVING edit preview");
        for comment in [
            "-- leading",
            "/* projection */",
            "/* relation */",
            "/* where */",
            "/* after group */",
            "-- trailing",
        ] {
            assert!(proposed.contains(comment), "missing {comment}: {proposed}");
        }
        assert!(
            proposed.contains("HAVING (COUNT(*) >= 2 AND NOT (SUM(\"field_102\") > $priority))")
        );
        let analysis = analyze_sql_structure(&proposed, &context());
        assert_eq!(analysis.status, SqlStructureStatus::Supported, "{proposed}");
        assert_eq!(analysis.having_expression, Some(expression));

        let clear = propose_sql_structure_operation(
            &proposed,
            &context(),
            SqlStructureOperation::SetHaving { expression: None },
        );
        assert_eq!(clear.status, SqlStructureStatus::Supported, "{clear:?}");
        let cleared = clear.proposed_sql.expect("HAVING removal preview");
        assert!(!cleared.contains("HAVING"));
        for comment in [
            "-- leading",
            "/* projection */",
            "/* relation */",
            "/* where */",
            "/* after group */",
            "-- trailing",
        ] {
            assert!(cleared.contains(comment), "missing {comment}: {cleared}");
        }

        let existing_clause = propose_sql_structure_operation(
            "SELECT COUNT(*) FROM \"Task\" HAVING COUNT(*) > 1",
            &context(),
            SqlStructureOperation::SetHaving {
                expression: Some(SqlStructureHavingExpression::Predicate {
                    aggregate: SqlStructureAggregate {
                        function: SqlStructureAggregateFunction::Count,
                        column: None,
                        arguments: vec![],
                        distinct: false,
                        alias: None,
                    },
                    operator: SqlStructureHavingOperator::GtEq,
                    value: SqlStructureHavingValue::Integer("2".to_owned()),
                }),
            },
        );
        assert_eq!(existing_clause.status, SqlStructureStatus::Supported);
        assert_applied_node_proposal(&existing_clause, SqlDocumentNodeKind::HavingClause);
    }

    #[test]
    fn having_rejects_type_mismatches_unsupported_shapes_and_inner_comments() {
        assert!(
            serde_json::from_value::<SqlStructureOperation>(serde_json::json!({
                "kind": "set_having"
            }))
            .is_err()
        );
        assert_eq!(
            serde_json::from_value::<SqlStructureOperation>(serde_json::json!({
                "kind": "set_having",
                "expression": null
            }))
            .expect("explicit null clears HAVING"),
            SqlStructureOperation::SetHaving { expression: None }
        );

        for (sql, reason) in [
            (
                "SELECT COUNT(*) FROM \"Task\" HAVING COUNT(*) > 'two'",
                SqlStructureReason::VariableTypeMismatch,
            ),
            (
                "SELECT COUNT(*) FROM \"Task\" HAVING COUNT(*) + 1 > 2",
                SqlStructureReason::HavingUnsupported,
            ),
            (
                "SELECT COUNT(*) FROM \"Task\" HAVING COUNT(*)",
                SqlStructureReason::HavingUnsupported,
            ),
            (
                "SELECT COUNT(*) FROM \"Task\" HAVING COUNT(*) >",
                SqlStructureReason::ParseFailed,
            ),
        ] {
            let analysis = analyze_sql_structure(sql, &context());
            let expected_status = if reason == SqlStructureReason::ParseFailed {
                SqlStructureStatus::InvalidSql
            } else {
                SqlStructureStatus::Unsupported
            };
            assert_eq!(analysis.status, expected_status, "{sql}: {analysis:?}");
            assert_eq!(analysis.reason, Some(reason), "{sql}");
            assert!(!analysis.capabilities.having.enabled, "{sql}");
        }

        let commented = "SELECT COUNT(*) FROM \"Task\" HAVING COUNT(*) /* preserve */ > 1";
        let proposal = propose_sql_structure_operation(
            commented,
            &context(),
            SqlStructureOperation::SetHaving { expression: None },
        );
        assert_eq!(proposal.status, SqlStructureStatus::Unsupported);
        assert_eq!(proposal.reason, Some(SqlStructureReason::HavingUnsupported));
        assert!(proposal.proposed_sql.is_none());
        assert!(proposal.base_sql.contains("/* preserve */"));
    }

    #[test]
    fn aggregate_editor_tracks_registry_signatures_and_modifiers() {
        for sql in [
            "SELECT COUNT() FROM \"Task\"",
            "SELECT COUNT(*) FROM \"Task\"",
            "SELECT COUNT(DISTINCT *) FROM \"Task\"",
            "SELECT COUNT(\"field_101\", \"field_102\") FROM \"Task\"",
            "SELECT MEAN(\"field_102\") FROM \"Task\"",
        ] {
            let analysis = analyze_sql_structure(sql, &context());
            assert_eq!(analysis.status, SqlStructureStatus::Supported, "{sql}");
            assert!(analysis.capabilities.aggregate.enabled, "{sql}");
        }

        let count_empty = analyze_sql_structure("SELECT COUNT() FROM \"Task\"", &context());
        assert!(count_empty.aggregates[0].arguments.is_empty());
        let nullary_count_rewrite = propose_sql_structure_operation(
            "SELECT COUNT() FROM \"Task\"",
            &context(),
            SqlStructureOperation::SetAggregates {
                aggregates: count_empty.aggregates.clone(),
            },
        );
        assert_eq!(
            nullary_count_rewrite.status,
            SqlStructureStatus::Supported,
            "{nullary_count_rewrite:?}"
        );
        assert!(nullary_count_rewrite
            .proposed_sql
            .expect("nullary COUNT canonical rewrite")
            .contains("COUNT(*)"));
        let count_distinct_star =
            analyze_sql_structure("SELECT COUNT(DISTINCT *) FROM \"Task\"", &context());
        assert_eq!(count_distinct_star.aggregates[0].column, None);
        assert!(count_distinct_star.aggregates[0].arguments.is_empty());
        assert!(count_distinct_star.aggregates[0].distinct);
        let distinct_wildcard_rewrite = propose_sql_structure_operation(
            "SELECT * FROM \"Task\"",
            &context(),
            SqlStructureOperation::SetAggregates {
                aggregates: count_distinct_star.aggregates.clone(),
            },
        );
        assert_eq!(
            distinct_wildcard_rewrite.status,
            SqlStructureStatus::Supported
        );
        assert!(distinct_wildcard_rewrite
            .proposed_sql
            .expect("COUNT(DISTINCT *) rewrite")
            .contains("COUNT(DISTINCT *)"));
        let mean = analyze_sql_structure("SELECT MEAN(\"field_102\") FROM \"Task\"", &context());
        assert_eq!(
            mean.aggregates[0].function,
            SqlStructureAggregateFunction::Mean
        );
        let mean_rewrite = propose_sql_structure_operation(
            "SELECT MEAN(\"field_102\") FROM \"Task\"",
            &context(),
            SqlStructureOperation::SetAggregates {
                aggregates: mean.aggregates.clone(),
            },
        );
        assert!(mean_rewrite
            .proposed_sql
            .expect("MEAN rewrite")
            .contains("MEAN(\"field_102\")"));

        let multiple = propose_sql_structure_operation(
            "SELECT * FROM \"Task\"",
            &context(),
            SqlStructureOperation::SetAggregates {
                aggregates: vec![SqlStructureAggregate {
                    function: SqlStructureAggregateFunction::Count,
                    column: Some("field_101".to_owned()),
                    arguments: vec!["field_102".to_owned()],
                    distinct: false,
                    alias: None,
                }],
            },
        );
        assert_eq!(multiple.status, SqlStructureStatus::Supported);
        assert!(multiple
            .proposed_sql
            .unwrap()
            .contains("COUNT(\"field_101\", \"field_102\")"));
        let multiple_wire = serde_json::to_value(SqlStructureAggregate {
            function: SqlStructureAggregateFunction::Count,
            column: Some("field_101".to_owned()),
            arguments: vec!["field_102".to_owned()],
            distinct: false,
            alias: None,
        })
        .expect("variadic COUNT wire value");
        assert_eq!(multiple_wire["column"], "field_101");
        assert_eq!(multiple_wire["arguments"], serde_json::json!(["field_102"]));

        let legacy_wire = serde_json::json!({
            "function": "count",
            "column": "field_101",
            "distinct": false
        });
        let legacy: SqlStructureAggregate =
            serde_json::from_value(legacy_wire).expect("legacy aggregate wire value");
        assert_eq!(legacy.column.as_deref(), Some("field_101"));
        assert!(legacy.arguments.is_empty());
        let legacy_serialized =
            serde_json::to_value(&legacy).expect("legacy aggregate reserialization");
        assert!(legacy_serialized.get("arguments").is_none());

        let unsupported_distinct_arity = analyze_sql_structure(
            "SELECT COUNT(DISTINCT \"field_101\", \"field_102\") FROM \"Task\"",
            &context(),
        );
        assert_eq!(
            unsupported_distinct_arity.status,
            SqlStructureStatus::Unsupported
        );
        assert_eq!(
            unsupported_distinct_arity.reason,
            Some(SqlStructureReason::AggregateUnsupported)
        );

        let count_other =
            analyze_sql_structure("SELECT COUNT(\"opaque_other\") FROM \"Task\"", &context());
        assert_eq!(count_other.status, SqlStructureStatus::Supported);
        let distinct_count_other = analyze_sql_structure(
            "SELECT COUNT(DISTINCT \"opaque_other\") FROM \"Task\"",
            &context(),
        );
        assert_eq!(distinct_count_other.status, SqlStructureStatus::Unsupported);
        assert_eq!(
            distinct_count_other.reason,
            Some(SqlStructureReason::AggregateTypeUnsupported)
        );
    }

    #[test]
    fn every_registered_aggregate_name_and_alias_has_a_gui_parse_path() {
        for descriptor in crate::sql_functions::USER_SQL_AGGREGATES {
            let argument = match descriptor.input {
                crate::sql_functions::SqlAggregateInput::AnyScalar => "\"field_101\"",
                crate::sql_functions::SqlAggregateInput::Numeric => "\"field_102\"",
                crate::sql_functions::SqlAggregateInput::Orderable => "\"field_101\"",
            };
            for spelling in
                std::iter::once(descriptor.name).chain(descriptor.aliases.iter().copied())
            {
                let sql = format!("SELECT {spelling}({argument}) FROM \"Task\"");
                let analysis = analyze_sql_structure(&sql, &context());
                assert_eq!(analysis.status, SqlStructureStatus::Supported, "{sql}");
                assert_eq!(analysis.aggregates.len(), 1, "{sql}");

                if descriptor.distinct != SqlAggregateDistinct::Unsupported {
                    let distinct_sql =
                        format!("SELECT {spelling}(DISTINCT {argument}) FROM \"Task\"");
                    let distinct_analysis = analyze_sql_structure(&distinct_sql, &context());
                    assert_eq!(
                        distinct_analysis.status,
                        SqlStructureStatus::Supported,
                        "{distinct_sql}"
                    );
                }
            }
        }
    }

    #[test]
    fn proposes_aggregate_group_by_and_select_distinct_as_source_preserving_edits() {
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
                    arguments: vec![],
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
    fn group_by_insertion_stays_before_limit_and_retains_statement_terminator() {
        let proposal = propose_sql_structure_operation(
            "SELECT \"field_101\" FROM \"Task\" LIMIT 5;",
            &context(),
            SqlStructureOperation::SetGroupBy {
                columns: vec!["field_101".to_owned()],
            },
        );
        assert_eq!(proposal.status, SqlStructureStatus::Supported);
        let proposed = proposal.proposed_sql.expect("grouping proposal");
        assert!(proposed.contains("GROUP BY \"field_101\" LIMIT 5;"));
        assert_eq!(
            analyze_sql_structure(&proposed, &context()).status,
            SqlStructureStatus::Supported
        );
    }

    #[test]
    fn aggregates_replace_wildcard_projection_and_retain_existing_alias_slots() {
        let with_comment = propose_sql_structure_operation(
            "SELECT * FROM \"Task\" /* retained relation comment */",
            &context(),
            SqlStructureOperation::SetAggregates {
                aggregates: vec![SqlStructureAggregate {
                    function: SqlStructureAggregateFunction::Count,
                    column: None,
                    arguments: vec![],
                    distinct: false,
                    alias: None,
                }],
            },
        );
        let commented_sql = with_comment.proposed_sql.expect("comment-preserving COUNT");
        assert!(commented_sql.starts_with("SELECT COUNT(*) FROM \"Task\""));
        assert!(commented_sql.contains("/* retained relation comment */"));

        let count = propose_sql_structure_operation(
            "SELECT * FROM \"Task\"",
            &context(),
            SqlStructureOperation::SetAggregates {
                aggregates: vec![SqlStructureAggregate {
                    function: SqlStructureAggregateFunction::Count,
                    column: None,
                    arguments: vec![],
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
                    arguments: vec![],
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
                        arguments: vec![],
                        distinct: false,
                        alias: None,
                    },
                    SqlStructureAggregate {
                        function: SqlStructureAggregateFunction::Count,
                        column: Some("field_101".to_owned()),
                        arguments: vec![],
                        distinct: true,
                        alias: None,
                    },
                    SqlStructureAggregate {
                        function: SqlStructureAggregateFunction::Sum,
                        column: Some("field_102".to_owned()),
                        arguments: vec![],
                        distinct: false,
                        alias: None,
                    },
                    SqlStructureAggregate {
                        function: SqlStructureAggregateFunction::Avg,
                        column: Some("field_102".to_owned()),
                        arguments: vec![],
                        distinct: false,
                        alias: None,
                    },
                    SqlStructureAggregate {
                        function: SqlStructureAggregateFunction::Mean,
                        column: Some("field_102".to_owned()),
                        arguments: vec![],
                        distinct: false,
                        alias: None,
                    },
                    SqlStructureAggregate {
                        function: SqlStructureAggregateFunction::Min,
                        column: Some("field_102".to_owned()),
                        arguments: vec![],
                        distinct: false,
                        alias: None,
                    },
                    SqlStructureAggregate {
                        function: SqlStructureAggregateFunction::Max,
                        column: Some("field_102".to_owned()),
                        arguments: vec![],
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
            "MEAN(\"field_102\")",
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

        let supported_sql = "SELECT COUNT(DISTINCT *) FROM \"Task\"";
        let accepted = propose_sql_structure_operation(
            supported_sql,
            &context(),
            SqlStructureOperation::SetDistinct { distinct: true },
        );
        assert_eq!(accepted.status, SqlStructureStatus::Supported);
        assert_eq!(accepted.base_sql, supported_sql);
        assert!(accepted.proposed_sql.is_some());

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
                    arguments: vec![],
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
                aliases: vec![],
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
                    qualifier: None,
                    operator: SqlStructureFilterOperator::Eq,
                    parameter: Some("status".to_owned()),
                    second_parameter: None,
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
    fn rejects_postgres_custom_ordering_operators() {
        let result = analyze_sql_structure(
            "SELECT \"field_101\" FROM \"Task\" ORDER BY \"field_101\" USING <",
            &context(),
        );

        assert_eq!(result.status, SqlStructureStatus::Unsupported);
        assert_eq!(result.reason, Some(SqlStructureReason::SortUnsupported));
        assert!(!result.capabilities.sort.enabled);
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
                    qualifier: None,
                    projection_alias: None,
                    direction: SqlStructureSortDirection::Asc,
                    nulls_first: None,
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
                    qualifier: None,
                    projection_alias: None,
                    direction: SqlStructureSortDirection::Desc,
                    nulls_first: None,
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
