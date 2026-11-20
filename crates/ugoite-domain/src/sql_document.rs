//! Lossless source document and parser view for the SQL Workspace.
//!
//! This module records SQL syntax and source locations. It does not authorize,
//! type-check, plan, or execute a query. The Rust SQL executor remains the
//! authority for the read-only SQL policy and the query planner remains the
//! authority for expression types and result schemas.

use core::ops::ControlFlow;
use serde::{Deserialize, Serialize};
use sqlparser::ast::{
    AccessExpr, CastKind, CteAsMaterialized, Distinct, DuplicateTreatment, Expr, FunctionArg,
    FunctionArgExpr, FunctionArguments, GroupByExpr, GroupByWithModifier, Ident, LimitClause,
    ObjectName, ObjectNamePart, OrderByKind, OrderBySort, Query, Select, SelectItem, SetExpr,
    Spanned, Statement, Subscript, TableFactor, Value, ValueWithSpan, Visit, Visitor,
};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::{Parser, ParserError};
use sqlparser::tokenizer::{Location, Span, Token, Tokenizer, Whitespace};
use std::collections::HashSet;
use std::convert::Infallible;

/// Version of the Rust SQL Document view returned to adapters.
pub const SQL_DOCUMENT_CONTRACT_VERSION: u32 = 27;

/// Bound on source SQL accepted by the editor parser.
pub const MAX_SQL_DOCUMENT_BYTES: usize = 256 * 1024;

/// Maximum number of disjoint edits accepted by one atomic document update.
pub const MAX_SQL_DOCUMENT_EDITS: usize = 256;
/// Bound on syntax tokens processed into an editable SQL tree.
pub const MAX_SQL_DOCUMENT_COMPLEXITY_TOKENS: usize = 65_536;

/// Keep the source parser aligned with the bounded DataFusion SQL parser used
/// by the executor, including the 32-level boolean tree editor contract.
pub(crate) const SQL_DOCUMENT_PARSER_RECURSION_LIMIT: usize = 256;

pub(crate) fn parse_generic_sql_statements(source: &str) -> Result<Vec<Statement>, ParserError> {
    let dialect = GenericDialect {};
    Parser::new(&dialect)
        .with_recursion_limit(SQL_DOCUMENT_PARSER_RECURSION_LIMIT)
        .try_with_sql(source)?
        .parse_statements()
}

/// One half-open byte range in the original UTF-8 SQL source.
///
/// Ranges always point into [`SqlDocument::source`]. The parser view uses byte
/// offsets so callers can slice without re-encoding or normalizing the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlSourceRange {
    /// Inclusive UTF-8 byte offset.
    pub start: usize,
    /// Exclusive UTF-8 byte offset.
    pub end: usize,
}

impl SqlSourceRange {
    fn from_span(source: &str, source_map: &SqlSourceMap, span: Span) -> Option<Self> {
        let start = source_map.byte_offset(span.start)?;
        let end = source_map.byte_offset(span.end)?;
        (start <= end && source.is_char_boundary(start) && source.is_char_boundary(end))
            .then_some(Self { start, end })
    }
}

pub(crate) fn source_range_from_span(source: &str, span: Span) -> Option<SqlSourceRange> {
    SqlSourceRange::from_span(source, &SqlSourceMap::new(source), span)
}

/// Maps parser line/character locations to UTF-8 byte offsets in linear time.
struct SqlSourceMap {
    line_boundary_indices: Vec<usize>,
    byte_offsets: Vec<usize>,
}

impl SqlSourceMap {
    fn new(source: &str) -> Self {
        let mut line_boundary_indices = vec![0];
        let mut byte_offsets = vec![0];

        for (offset, character) in source.char_indices() {
            let next_offset = offset + character.len_utf8();
            if character == '\n' {
                line_boundary_indices.push(byte_offsets.len());
                byte_offsets.push(next_offset);
            } else {
                byte_offsets.push(next_offset);
            }
        }

        Self {
            line_boundary_indices,
            byte_offsets,
        }
    }

    fn byte_offset(&self, location: Location) -> Option<usize> {
        let line_index = usize::try_from(location.line.checked_sub(1)?).ok()?;
        let column_index = usize::try_from(location.column.checked_sub(1)?).ok()?;
        let boundary_index = self
            .line_boundary_indices
            .get(line_index)?
            .checked_add(column_index)?;
        self.byte_offsets.get(boundary_index).copied()
    }
}

/// Lexical class of one source token.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentTokenKind {
    /// SQL whitespace that is retained exactly in the source document.
    Trivia,
    /// A line or block comment.
    Comment,
    /// An unquoted word, including keywords.
    Word,
    /// A quoted identifier.
    QuotedIdentifier,
    /// A string, numeric, or other literal token.
    Literal,
    /// A parameter placeholder token.
    Placeholder,
    /// Punctuation such as commas, parentheses, or semicolons.
    Punctuation,
    /// An operator token.
    Operator,
}

/// A token range into the exact source SQL.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlDocumentToken {
    /// Token category used by syntax-aware editor controls.
    pub kind: SqlDocumentTokenKind,
    /// Exact range in [`SqlDocument::source`].
    pub range: SqlSourceRange,
}

/// Compact `[source_start, source_end, normalized_length]` byte edit.
pub type SqlTemplateReplacement = [usize; 3];

/// SQL template text and the ordered parameter names derived from its tokens.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlTemplateNormalization {
    /// SQL source with `{{name}}` markers outside protected tokens rewritten
    /// to `$name`; all other source bytes are retained.
    pub sql: String,
    /// Unique parameter names in first-occurrence source order.
    pub parameters: Vec<String>,
    /// Compact source and replacement-length byte coordinates for each rewrite.
    pub replacements: Vec<SqlTemplateReplacement>,
    /// First malformed marker, if present. Valid markers before and after it
    /// remain normalized so draft editors can retain parameter discovery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<SqlTemplateNormalizationError>,
}

/// Failure to normalize a user-facing SQL parameter marker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case", deny_unknown_fields)]
pub enum SqlTemplateNormalizationError {
    /// An opening `{{` marker has no matching `}}` in SQL code.
    MissingClosingBraces { range: SqlSourceRange },
    /// The marker contents do not form a valid SQL parameter name.
    InvalidParameterName { range: SqlSourceRange },
}

impl SqlTemplateNormalizationError {
    /// Stable protocol code for this failure.
    pub fn code(&self) -> &'static str {
        match self {
            Self::MissingClosingBraces { .. } => "missing_closing_braces",
            Self::InvalidParameterName { .. } => "invalid_parameter_name",
        }
    }

    /// Exact diagnostic range in the submitted UTF-8 SQL source.
    pub fn range(&self) -> SqlSourceRange {
        match self {
            Self::MissingClosingBraces { range } | Self::InvalidParameterName { range } => *range,
        }
    }
}

impl std::fmt::Display for SqlTemplateNormalizationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingClosingBraces { .. } => {
                formatter.write_str("SQL template variable is missing closing braces")
            }
            Self::InvalidParameterName { .. } => {
                formatter.write_str("SQL template variable name must match [A-Za-z_][A-Za-z0-9_]*")
            }
        }
    }
}

impl std::error::Error for SqlTemplateNormalizationError {}

/// High-level node category in the SQL parser view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentNodeKind {
    /// A top-level query statement.
    QueryStatement,
    /// A top-level non-query statement, retained for a useful rejection.
    OtherStatement,
    /// A query, including nested queries and set-expression roots.
    Query,
    /// A `WITH` clause containing one or more common table expressions.
    WithClause,
    /// One named common table expression in a `WITH` clause.
    CommonTableExpression,
    /// A set operation such as `UNION`, `INTERSECT`, or `EXCEPT`.
    SetOperation,
    /// The left input of a set operation.
    SetOperationLeft,
    /// The right input of a set operation.
    SetOperationRight,
    /// A `SELECT` node.
    Select,
    /// The projection list following `SELECT`.
    SelectList,
    /// One item in a `SELECT` projection, including expressions and wildcards.
    SelectItem,
    /// A `FROM` clause containing relation trees.
    FromClause,
    /// One relation tree in a `FROM` clause, including its joins.
    RelationTree,
    /// One join operation and its right relation and condition.
    Join,
    /// A `WHERE` clause and its predicate.
    WhereClause,
    /// A `GROUP BY` clause and its grouping expressions.
    GroupByClause,
    /// One source-backed grouping key inside a `GROUP BY` clause.
    GroupByItem,
    /// A `HAVING` clause and its predicate.
    HavingClause,
    /// An `ORDER BY` clause and its ordered expressions.
    OrderByClause,
    /// One ordered expression within an `ORDER BY` clause.
    OrderByItem,
    /// A row-limit clause, including any attached offset.
    LimitClause,
    /// An offset expression within a limit clause.
    OffsetClause,
    /// A named or derived relation in a `FROM` tree.
    Relation,
    /// A table factor such as a named table, join, or derived relation.
    TableFactor,
    /// A simple column or name reference.
    IdentifierExpression,
    /// A compound column or name reference.
    CompoundIdentifierExpression,
    /// A literal value expression.
    LiteralExpression,
    /// A binary operation expression.
    BinaryExpression,
    /// A unary operation expression.
    UnaryExpression,
    /// A function call expression.
    FunctionExpression,
    /// A `CASE` expression.
    CaseExpression,
    /// A cast expression.
    CastExpression,
    /// A nested or scalar subquery expression.
    SubqueryExpression,
    /// A boolean or comparison predicate expression.
    PredicateExpression,
    /// Parenthesized expression.
    NestedExpression,
    /// A syntactically valid expression not yet given a more specific view.
    OtherExpression,
    /// A literal token represented as an AST value node.
    Value,
}

/// A parser-derived identifier component. The original spelling, including
/// quote style and trivia, remains available through the node's source range.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlDocumentIdentifier {
    /// Identifier contents without surrounding quote characters.
    pub value: String,
    /// Quote character used in the parsed identifier, if any.
    pub quote_style: Option<char>,
}

/// A SQL column reference constructed from typed identifier parts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlDocumentColumnReference {
    /// Optional relation or table alias qualifier.
    pub qualifier: Option<SqlDocumentIdentifier>,
    /// Selected column name.
    pub column: SqlDocumentIdentifier,
}

/// One bounded collection access suffix constructed by the SQL Document editor.
/// The index is one-based SQL syntax and is kept as decimal text so the
/// browser never rounds it through JavaScript numbers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SqlDocumentValueAccess {
    /// Select a named Struct field using a SQL string-literal subscript.
    StructField { name: String },
    /// Select one array element using a positive integer literal.
    ArrayIndex { one_based_index: String },
    /// Select a fully bounded array slice using positive, inclusive SQL bounds.
    ArraySlice {
        one_based_start: String,
        one_based_end: String,
    },
}

/// Broad lexical family for an expression literal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentLiteralKind {
    /// Numeric literal, retaining its exact spelling in the document source.
    Number,
    /// String or byte-string literal.
    String,
    /// Boolean literal.
    Boolean,
    /// SQL NULL literal.
    Null,
    /// Bound SQL parameter placeholder.
    Placeholder,
    /// A parser literal form not classified above.
    Other,
}

/// Typed semantic attributes attached to selected syntax nodes.
///
/// This is intentionally a view over sqlparser's parsed AST, not another SQL
/// grammar or an execution contract. `source` and token ranges remain the
/// authority for exact spelling and trivia; Core remains authoritative for
/// admission, planning, and types.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SqlDocumentNodeData {
    /// Output alias declared by one SELECT projection item.
    ProjectionAlias { alias: SqlDocumentIdentifier },
    /// SELECT-level duplicate handling.
    Select {
        /// Whether plain DISTINCT is active.
        distinct: bool,
        /// Whether DISTINCT ON is present and therefore needs a richer editor.
        distinct_on: bool,
    },
    /// Parsed GROUP BY mode and exact per-set members for editable grouping
    /// structures. Empty member lists represent the valid empty set `()`.
    GroupBy {
        mode: SqlDocumentGroupByMode,
        groups: Vec<Vec<u32>>,
        /// Whether the current source grouping shape fits the tree controls.
        /// Composite ROLLUP/CUBE elements remain source-backed and read-only.
        editable: bool,
    },
    /// Direction and NULL placement parsed from one ORDER BY item.
    OrderByItem {
        /// Explicit direction, or `None` when SQL leaves it implicit.
        direction: Option<SqlDocumentOrderDirection>,
        /// Explicit NULL ordering, or `None` when SQL leaves it implicit.
        nulls_order: Option<SqlDocumentNullsOrder>,
    },
    /// Literal value parsed from a LIMIT or OFFSET clause when representable.
    QueryClauseValue {
        /// Unsigned integer value, absent for non-literal or out-of-range SQL.
        value: Option<u64>,
    },
    /// Parsed wildcard projection form and any AST modifiers.
    Wildcard {
        /// Qualified identifier parts, empty for an unqualified `*`.
        qualifier: Vec<SqlDocumentIdentifier>,
        /// Whether the qualifier is an expression rather than an object name.
        qualifier_is_expression: bool,
        /// Whether the wildcard has EXCEPT/REPLACE/alias or dialect options.
        has_options: bool,
    },
    /// One unqualified identifier expression.
    Identifier {
        /// Identifier contents without quotes.
        value: String,
        /// Quote character used, if any.
        quote_style: Option<char>,
    },
    /// A multi-part identifier expression, such as `schema.table.column`.
    CompoundIdentifier {
        /// Identifier parts in source order.
        parts: Vec<SqlDocumentIdentifier>,
    },
    /// A literal expression.
    Literal {
        /// Broad parser-derived literal category.
        literal_kind: SqlDocumentLiteralKind,
        /// Exact literal text, when the parser range is available.
        raw: Option<String>,
    },
    /// A typed SQL string literal, such as `DATE '2026-01-01'`.
    TypedLiteral {
        /// Parsed SQL type name.
        data_type: String,
        /// Exact literal expression text when its parser range is available.
        raw: Option<String>,
    },
    /// Parser-classified ARRAY literal expression. Exact range coverage may
    /// still be supplied by its containing projection item.
    ArrayLiteral,
    /// A binary operator expression.
    BinaryOperator {
        /// sqlparser's canonical operator spelling.
        operator: String,
    },
    /// A unary operator expression.
    UnaryOperator {
        /// sqlparser's canonical operator spelling.
        operator: String,
    },
    /// A function call and its simple argument/decoration shape.
    Function {
        /// Qualified function name parts.
        name: Vec<SqlDocumentIdentifier>,
        /// Number of regular arguments, when the AST has a normal argument list.
        argument_count: Option<usize>,
        /// Whether the regular argument list uses DISTINCT.
        distinct: bool,
        /// Whether a FILTER clause is present.
        has_filter: bool,
        /// Whether an OVER clause is present.
        has_over: bool,
        /// Whether the argument list contains modifiers such as ORDER BY or LIMIT.
        has_argument_clauses: bool,
        /// Whether the shared SQL aggregate registry permits replacing an
        /// existing argument on this exact call shape.
        can_replace_argument: bool,
        /// Whether the shared SQL aggregate registry permits adding an argument
        /// to this exact call shape.
        can_add_argument: bool,
        /// Whether the shared SQL aggregate registry permits removing the final
        /// argument from this exact call shape.
        can_remove_argument: bool,
    },
    /// A CASE expression's branch structure.
    Case {
        /// Whether this is a simple CASE with an operand.
        has_operand: bool,
        /// Number of WHEN branches.
        when_count: usize,
        /// Whether an ELSE branch is present.
        has_else: bool,
    },
    /// A CAST or CONVERT expression.
    Cast {
        /// Cast spelling/kind from the parser AST.
        cast_kind: String,
        /// Target SQL type rendered from the parsed AST.
        target_type: Option<String>,
    },
    /// Predicate operator and whether it is negated.
    Predicate {
        /// Predicate spelling, such as `IN`, `BETWEEN`, or `LIKE`.
        operator: String,
        /// Whether the predicate has an explicit NOT modifier.
        negated: bool,
    },
    /// A set operation between two query inputs.
    SetOperation {
        /// Set operation, such as `UNION` or `INTERSECT`.
        operator: String,
        /// Quantifier, such as `ALL`, `DISTINCT`, or empty.
        quantifier: String,
    },
    /// A WITH clause.
    WithClause {
        /// Whether the clause is recursive.
        recursive: bool,
        /// Number of declared CTEs.
        cte_count: usize,
    },
    /// One common table expression declaration.
    CommonTableExpression {
        /// Declared CTE name.
        name: SqlDocumentIdentifier,
        /// Optional declared output columns.
        columns: Vec<SqlDocumentIdentifier>,
        /// Materialization hint, if explicitly present.
        materialized: Option<bool>,
    },
    /// A table/relation name node.
    Relation {
        /// Qualified relation name parts.
        name: Vec<SqlDocumentIdentifier>,
    },
    /// A fully bounded literal slice at the end of an array access chain.
    /// The parser ranges identify only the literal bounds so an edit retains
    /// brackets, colons, and comments around them exactly.
    ArraySlice {
        /// Exact source text of the current lower bound.
        one_based_start: String,
        /// Exact source text of the current upper bound.
        one_based_end: String,
        /// Parser-derived source range of the lower bound literal.
        start_range: SqlSourceRange,
        /// Parser-derived source range of the upper bound literal.
        end_range: SqlSourceRange,
    },
    /// A FROM table factor with its kind and optional alias.
    TableFactor {
        /// Parser AST category of the table factor.
        factor_kind: String,
        /// Declared alias, if present.
        alias: Option<SqlDocumentIdentifier>,
        /// Declared output column aliases.
        column_aliases: Vec<SqlDocumentIdentifier>,
        /// Whether any declared alias column includes a type annotation.
        has_column_alias_types: bool,
    },
    /// An expression AST variant without dedicated semantic fields.
    ///
    /// The expression remains source-backed and is still distinguished by its
    /// [`SqlDocumentNodeKind`]. This reason tells adapters that no richer
    /// structured edit contract is defined for the variant yet.
    UnmodeledExpression {
        /// Typed syntax category assigned by the parser visitor.
        node_kind: SqlDocumentNodeKind,
        /// Why this node has no dedicated semantic attributes.
        reason: String,
    },
}

/// One syntax node and its parent in the parsed query tree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlDocumentNode {
    /// Index of this node in the document's node list.
    pub id: u32,
    /// Parent node index, if this node is nested.
    pub parent: Option<u32>,
    /// Syntax category for the node.
    pub kind: SqlDocumentNodeKind,
    /// Source range in the original SQL. Parser spans are used when usable;
    /// projection-item boundaries are aligned with the tokenized source.
    pub range: Option<SqlSourceRange>,
    /// Parser-derived semantic attributes for syntax-aware controls.
    pub semantic: Option<SqlDocumentNodeData>,
}

/// Parse state of one SQL draft.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentStatus {
    /// A single query statement parsed into a syntax tree.
    Parsed,
    /// The source contains no SQL statement.
    Empty,
    /// The source could not be tokenized or parsed.
    InvalidSql,
    /// The source contains more than one statement.
    MultipleStatements,
    /// The source is valid SQL but is not a query statement.
    SelectRequired,
    /// The source exceeds the bounded editor input size.
    TooLarge,
    /// The source contains too many syntax tokens for bounded tree construction.
    TooComplex,
}

/// Stable error category for a SQL document diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentDiagnosticCode {
    /// Source exceeded the bounded editor input size.
    SqlTooLarge,
    /// SQL contains more syntax tokens than the editor parser accepts.
    SqlTooComplex,
    /// The parser or tokenizer rejected the SQL syntax.
    ParseFailed,
    /// The SQL source is empty.
    EmptyDocument,
    /// More than one statement was supplied.
    SingleStatementRequired,
    /// The single statement is not a `SELECT` query.
    SelectRequired,
}

/// Local, source-associated diagnostic emitted while parsing a SQL draft.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlDocumentDiagnostic {
    /// Stable diagnostic category.
    pub code: SqlDocumentDiagnosticCode,
    /// Human-readable parser or policy detail.
    pub message: String,
    /// Parser-provided source range when one is available.
    pub range: Option<SqlSourceRange>,
}

/// Original SQL text and its lossless lexical and syntax view.
///
/// Parsing does not imply that the executor will admit the query. A parsed
/// document still requires the normal Rust SQL planning and authorization
/// path before it can be saved or executed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlDocument {
    /// Version of the serialized document view contract.
    pub contract_version: u32,
    /// Authoritative SQL text. Parsing and read-only serialization preserve it
    /// byte-for-byte.
    pub source: String,
    /// Syntactic state of the draft.
    pub status: SqlDocumentStatus,
    /// Parser and statement-shape diagnostics.
    pub diagnostics: Vec<SqlDocumentDiagnostic>,
    /// Tokens, including whitespace and comments, in source order.
    pub tokens: Vec<SqlDocumentToken>,
    /// Query syntax nodes with source ranges and parent links.
    pub nodes: Vec<SqlDocumentNode>,
    /// Rust-derived condition editor projection for each SELECT scope.
    pub conditions: Vec<SqlDocumentConditionScope>,
}

/// Read-only condition projection derived from the parser tree; SQL remains authoritative.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlDocumentConditionScope {
    pub query_node_id: u32,
    /// The SELECT WHERE scope or aggregate FILTER scope represented here.
    pub kind: SqlDocumentConditionScopeKind,
    /// Parser node identifying the editable scope (SELECT or aggregate).
    pub scope_node_id: u32,
    pub where_clause_node_id: Option<u32>,
    pub expression: Option<SqlDocumentConditionNode>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentConditionScopeKind {
    Where,
    AggregateFilter,
}

/// A supported condition or an exact-source advanced predicate leaf.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SqlDocumentConditionNode {
    Group {
        node_id: u32,
        operator: SqlDocumentConditionGroupOperator,
        children: Vec<SqlDocumentConditionNode>,
    },
    Comparison {
        node_id: u32,
        column_node_id: u32,
        column: SqlDocumentColumnReference,
        operator: SqlDocumentFilterOperator,
        value: Option<SqlDocumentSemanticLiteral>,
    },
    Advanced {
        node_id: u32,
        sql: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentConditionGroupOperator {
    And,
    Or,
}

/// Typed condition supplied to SQL Document semantic edit operations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SqlDocumentFilterCondition {
    Group {
        operator: SqlDocumentConditionGroupOperator,
        children: Vec<SqlDocumentFilterCondition>,
    },
    Comparison {
        column: SqlDocumentColumnReference,
        operator: SqlDocumentFilterOperator,
        value: Option<SqlDocumentSemanticLiteral>,
    },
    CopyNode {
        node_id: u32,
        expected_kind: SqlDocumentNodeKind,
    },
}

impl SqlDocument {
    /// Return the exact original SQL source without formatting or rewriting.
    pub fn to_sql(&self) -> &str {
        &self.source
    }

    /// Return the source bytes covered by a range from this document.
    pub fn source_slice(&self, range: SqlSourceRange) -> Option<&str> {
        self.source.get(range.start..range.end)
    }
}

/// One guarded replacement against the original SQL source of a transaction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlDocumentEdit {
    /// Half-open UTF-8 byte range to replace in the original source.
    pub range: SqlSourceRange,
    /// Exact source text expected at `range`.
    pub expected_text: String,
    /// Replacement text. It may leave the SQL draft syntactically incomplete.
    pub replacement: String,
}

/// A set of simultaneous edits guarded by the complete source they were made against.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlDocumentEditTransaction {
    /// Exact original SQL source. A mismatch rejects the whole transaction as stale.
    pub expected_source: String,
    /// Disjoint replacements interpreted against `expected_source`, not sequentially.
    pub edits: Vec<SqlDocumentEdit>,
}

/// One replacement addressed to a parser-classified syntax node.
///
/// The node ID is meaningful only for the exact source guarded by the parent
/// transaction. Requiring the expected node kind prevents an adapter from
/// applying an old selection to a different syntax category after reparsing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SqlDocumentSemanticEdit {
    /// Construct the first SELECT in an empty (possibly trivia-only) draft.
    InitializeSelect {
        relation: Option<SqlDocumentIdentifier>,
    },
    /// Replace a selected table factor with the Form chosen by the picker.
    ReplaceTableFactor {
        node_id: u32,
        expected_kind: SqlDocumentNodeKind,
        factor: SqlDocumentRelationFactor,
    },
    /// Add the selected Form to a SELECT scope that has no existing relation.
    AddFromRelation {
        node_id: u32,
        expected_kind: SqlDocumentNodeKind,
        factor: SqlDocumentRelationFactor,
    },
}

/// Direction and null ordering represented in parser output for ORDER BY nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentOrderDirection {
    Asc,
    Desc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentNullsOrder {
    First,
    Last,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentGroupByMode {
    Expressions,
    All,
    Rollup,
    Cube,
    GroupingSets,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentFilterOperator {
    Eq,
    NotEq,
    Lt,
    Lte,
    Gt,
    Gte,
    Like,
    NotLike,
    IsNull,
    IsNotNull,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentTypedLiteralType {
    Date,
    Timestamp,
    Interval,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SqlDocumentSemanticLiteral {
    Number {
        value: String,
    },
    String {
        value: String,
    },
    Boolean {
        value: bool,
    },
    Null,
    NamedParameter {
        name: String,
    },
    TypedString {
        data_type: SqlDocumentTypedLiteralType,
        value: String,
    },
    HexString {
        value: String,
    },
}

/// A parser-safe relation supplied by the Saved SQL Form picker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SqlDocumentRelationFactor {
    Table {
        name: Vec<SqlDocumentIdentifier>,
        alias: Option<SqlDocumentIdentifier>,
    },
}

/// A Form-picker edit guarded by the exact document source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlDocumentSemanticEditTransaction {
    pub contract_version: u32,
    pub expected_source: String,
    pub edits: Vec<SqlDocumentSemanticEdit>,
}

/// Result of applying an edit transaction, including a guarded inverse transaction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlDocumentEditResult {
    /// Reparsed document, including diagnostics for incomplete or invalid drafts.
    pub document: SqlDocument,
    /// Inverse edits guarded by the exact resulting source.
    pub undo: SqlDocumentEditTransaction,
}

/// Failure to apply an SQL Document edit transaction atomically.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentEditError {
    /// The old parameter name is not a valid named SQL parameter.
    InvalidOldParameterName,
    /// The new parameter name is not a valid named SQL parameter.
    InvalidNewParameterName,
    /// The current source differs from the transaction's expected base.
    StaleSource,
    /// The current or expected source exceeds the bounded document size.
    SourceTooLarge,
    /// The source exceeds the bounded syntax complexity used for token edits.
    SourceTooComplex,
    /// The transaction exceeds the edit-count limit.
    TooManyEdits,
    /// A typed node edit requires a successfully parsed single query document.
    DocumentNotParsed,
    /// The caller's node IDs were produced by a different document contract.
    ContractVersionMismatch,
    /// A typed node edit refers to a node index absent from its source document.
    NodeNotFound,
    /// A typed node edit's expected category differs from the parsed node.
    NodeKindMismatch,
    /// A selected syntax node has no source range suitable for replacement.
    NodeRangeUnavailable,
    /// A typed semantic replacement is invalid or does not form the expected
    /// single SQL expression value.
    InvalidSemanticReplacement,
    /// A node ID appears more than once in one typed edit transaction.
    DuplicateNodeTarget,
    /// An edit has a reversed range.
    InvalidRange,
    /// An edit range extends past the source boundary.
    RangeOutOfBounds,
    /// An edit range splits a UTF-8 code point.
    NotUtf8Boundary,
    /// Two edits overlap or have an ambiguous shared insertion point.
    OverlappingEdits,
    /// The source at an edit range differs from its expected preimage.
    PreimageMismatch,
    /// The resulting source would exceed the bounded document size.
    OutputTooLarge,
}

impl core::fmt::Display for SqlDocumentEditError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let message = match self {
            Self::InvalidOldParameterName => {
                "old SQL parameter name must match [A-Za-z_][A-Za-z0-9_]*"
            }
            Self::InvalidNewParameterName => {
                "new SQL parameter name must match [A-Za-z_][A-Za-z0-9_]*"
            }
            Self::StaleSource => "SQL document edit is based on stale source",
            Self::SourceTooLarge => "SQL source exceeds the document size limit",
            Self::SourceTooComplex => "SQL source exceeds the document complexity limit",
            Self::TooManyEdits => "SQL document edit transaction exceeds the edit limit",
            Self::DocumentNotParsed => {
                "typed SQL document edits require one successfully parsed query"
            }
            Self::ContractVersionMismatch => {
                "SQL document edit contract version does not match the parser"
            }
            Self::NodeNotFound => "SQL document edit node does not exist in this source",
            Self::NodeKindMismatch => "SQL document edit node kind does not match",
            Self::NodeRangeUnavailable => "SQL document edit node has no replaceable source range",
            Self::InvalidSemanticReplacement => "SQL document semantic edit replacement is invalid",
            Self::DuplicateNodeTarget => {
                "SQL document edit transaction targets one node more than once"
            }
            Self::InvalidRange => "SQL document edit range is reversed",
            Self::RangeOutOfBounds => "SQL document edit range is outside the source",
            Self::NotUtf8Boundary => "SQL document edit range splits a UTF-8 code point",
            Self::OverlappingEdits => "SQL document edits overlap or share an insertion point",
            Self::PreimageMismatch => "SQL document edit source text does not match",
            Self::OutputTooLarge => "SQL document edit output exceeds the size limit",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for SqlDocumentEditError {}

/// Apply simultaneous replacements to parser-classified nodes.
///
/// The exact source must match the transaction guard, and the supplied parser
/// contract version must be current. Node IDs and ranges are resolved from a
/// fresh Rust parse instead of an adapter-provided tree. The operation then
/// delegates to the same bounded byte-range transaction used by text edits,
/// so preimages, UTF-8 boundaries, overlap, output size, and guarded undo have
/// one authority. A parsed result is still subject to the normal Core planner,
/// authorization, and executor admission.
pub fn apply_sql_document_semantic_edit_transaction(
    current_source: &str,
    transaction: &SqlDocumentSemanticEditTransaction,
) -> Result<SqlDocumentEditResult, SqlDocumentEditError> {
    if current_source.len() > MAX_SQL_DOCUMENT_BYTES
        || transaction.expected_source.len() > MAX_SQL_DOCUMENT_BYTES
    {
        return Err(SqlDocumentEditError::SourceTooLarge);
    }
    if current_source != transaction.expected_source {
        return Err(SqlDocumentEditError::StaleSource);
    }
    if transaction.edits.len() > MAX_SQL_DOCUMENT_EDITS {
        return Err(SqlDocumentEditError::TooManyEdits);
    }
    if transaction.contract_version != SQL_DOCUMENT_CONTRACT_VERSION {
        return Err(SqlDocumentEditError::ContractVersionMismatch);
    }

    let document = parse_sql_document(current_source);
    if document.status == SqlDocumentStatus::Empty {
        let [SqlDocumentSemanticEdit::InitializeSelect { relation }] = transaction.edits.as_slice()
        else {
            return Err(SqlDocumentEditError::InvalidSemanticReplacement);
        };
        let seed = match relation {
            Some(relation) => format!(
                "SELECT * FROM {}",
                render_identifier_parts(std::slice::from_ref(relation))?
            ),
            None => "SELECT 1".to_owned(),
        };
        let prefix = if current_source.is_empty() || current_source.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        return apply_sql_document_edit_transaction(
            current_source,
            &SqlDocumentEditTransaction {
                expected_source: current_source.to_owned(),
                edits: vec![SqlDocumentEdit {
                    range: SqlSourceRange {
                        start: current_source.len(),
                        end: current_source.len(),
                    },
                    expected_text: String::new(),
                    replacement: format!("{prefix}{seed}"),
                }],
            },
        );
    }
    if document.status != SqlDocumentStatus::Parsed {
        return Err(match document.status {
            SqlDocumentStatus::TooLarge => SqlDocumentEditError::SourceTooLarge,
            SqlDocumentStatus::TooComplex => SqlDocumentEditError::SourceTooComplex,
            _ => SqlDocumentEditError::DocumentNotParsed,
        });
    }

    let mut targets = Vec::with_capacity(transaction.edits.len());
    let mut edits = Vec::with_capacity(transaction.edits.len());
    for semantic_edit in &transaction.edits {
        let (node_id, expected_kind) = match semantic_edit {
            SqlDocumentSemanticEdit::InitializeSelect { .. } => {
                return Err(SqlDocumentEditError::InvalidSemanticReplacement);
            }
            SqlDocumentSemanticEdit::ReplaceTableFactor {
                node_id,
                expected_kind,
                ..
            }
            | SqlDocumentSemanticEdit::AddFromRelation {
                node_id,
                expected_kind,
                ..
            } => (*node_id, *expected_kind),
        };
        if targets.contains(&node_id) {
            return Err(SqlDocumentEditError::DuplicateNodeTarget);
        }
        targets.push(node_id);
        let node = document
            .nodes
            .get(node_id as usize)
            .filter(|node| node.id == node_id)
            .ok_or(SqlDocumentEditError::NodeNotFound)?;
        if node.kind != expected_kind {
            return Err(SqlDocumentEditError::NodeKindMismatch);
        }
        match semantic_edit {
            SqlDocumentSemanticEdit::ReplaceTableFactor { factor, .. } => {
                edits.push(replace_table_factor(&document, node, factor)?);
            }
            SqlDocumentSemanticEdit::AddFromRelation { factor, .. } => {
                edits.push(add_from_relation(&document, node, factor)?);
            }
            SqlDocumentSemanticEdit::InitializeSelect { .. } => unreachable!(),
        }
    }

    apply_sql_document_edit_transaction(
        current_source,
        &SqlDocumentEditTransaction {
            expected_source: transaction.expected_source.clone(),
            edits,
        },
    )
}

fn render_identifier_parts(
    parts: &[SqlDocumentIdentifier],
) -> Result<String, SqlDocumentEditError> {
    if parts.is_empty() {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }

    let mut rendered = String::new();
    for (index, identifier) in parts.iter().enumerate() {
        if index > 0 {
            rendered.push('.');
        }
        if identifier.value.is_empty() || identifier.value.contains('\0') {
            return Err(SqlDocumentEditError::InvalidSemanticReplacement);
        }
        if identifier.value.len() > MAX_SQL_DOCUMENT_BYTES {
            return Err(SqlDocumentEditError::OutputTooLarge);
        }
        match identifier.quote_style {
            None => {
                if !valid_sql_unquoted_identifier(&identifier.value) {
                    return Err(SqlDocumentEditError::InvalidSemanticReplacement);
                }
                rendered.push_str(&identifier.value);
            }
            Some(quote @ ('"' | '`')) => {
                rendered.push(quote);
                for character in identifier.value.chars() {
                    rendered.push(character);
                    if character == quote {
                        rendered.push(character);
                    }
                }
                rendered.push(quote);
            }
            Some('[') => {
                rendered.push('[');
                for character in identifier.value.chars() {
                    rendered.push(character);
                    if character == ']' {
                        rendered.push(']');
                    }
                }
                rendered.push(']');
            }
            Some(_) => return Err(SqlDocumentEditError::InvalidSemanticReplacement),
        }
        if rendered.len() > MAX_SQL_DOCUMENT_BYTES {
            return Err(SqlDocumentEditError::OutputTooLarge);
        }
    }
    Ok(rendered)
}

fn valid_sql_unquoted_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    matches!(characters.next(), Some(first) if first == '_' || first.is_alphabetic())
        && characters
            .all(|character| character == '_' || character == '$' || character.is_alphanumeric())
}

fn source_insertion(
    document: &SqlDocument,
    offset: usize,
    replacement: String,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if !document.source.is_char_boundary(offset) {
        return Err(SqlDocumentEditError::NotUtf8Boundary);
    }
    Ok(SqlDocumentEdit {
        range: SqlSourceRange {
            start: offset,
            end: offset,
        },
        expected_text: String::new(),
        replacement,
    })
}

fn query_ancestor_id(document: &SqlDocument, mut node_id: u32) -> Option<u32> {
    loop {
        let node = document.nodes.get(node_id as usize)?;
        if node.id != node_id {
            return None;
        }
        if node.kind == SqlDocumentNodeKind::Query {
            return Some(node_id);
        }
        node_id = node.parent?;
    }
}

fn relation_factor_sql(
    _document: &SqlDocument,
    factor: &SqlDocumentRelationFactor,
) -> Result<String, SqlDocumentEditError> {
    match factor {
        SqlDocumentRelationFactor::Table { name, alias } => {
            let mut sql = render_identifier_parts(name)?;
            if let Some(alias) = alias {
                sql.push_str(" AS ");
                sql.push_str(&render_identifier_parts(std::slice::from_ref(alias))?);
            }
            Ok(sql)
        }
    }
}

fn replace_table_factor(
    document: &SqlDocument,
    table_factor: &SqlDocumentNode,
    factor: &SqlDocumentRelationFactor,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if table_factor.kind != SqlDocumentNodeKind::TableFactor
        || !matches!(
            table_factor.semantic.as_ref(),
            Some(SqlDocumentNodeData::TableFactor { .. })
        )
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let range = table_factor
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if range_has_comment(document, range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let expected_text = document
        .source_slice(range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let replacement = relation_factor_sql(document, factor)?;
    if replacement == expected_text {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    Ok(SqlDocumentEdit {
        range,
        expected_text: expected_text.to_owned(),
        replacement,
    })
}

fn add_from_relation(
    document: &SqlDocument,
    select: &SqlDocumentNode,
    factor: &SqlDocumentRelationFactor,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if select.kind != SqlDocumentNodeKind::Select {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let factor_sql = relation_factor_sql(document, factor)?;
    let from_clause = document.nodes.iter().find(|node| {
        node.parent == Some(select.id) && node.kind == SqlDocumentNodeKind::FromClause
    });
    if let Some(from_clause) = from_clause {
        let mut relation_trees = document
            .nodes
            .iter()
            .filter(|node| {
                node.parent == Some(from_clause.id)
                    && node.kind == SqlDocumentNodeKind::RelationTree
            })
            .filter_map(|node| node.range)
            .collect::<Vec<_>>();
        relation_trees.sort_by_key(|range| range.start);
        let last = relation_trees
            .last()
            .copied()
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        if relation_trees.len() >= 64 {
            return Err(SqlDocumentEditError::InvalidSemanticReplacement);
        }
        let insertion = insertion_after_intervening_comments(
            document,
            last.end,
            next_select_clause_start(document, select, from_clause.range.map(|range| range.end))
                .unwrap_or(
                    select
                        .range
                        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?
                        .end,
                ),
        )?;
        let suffix_space = if insertion > last.end {
            " "
        } else {
            Default::default()
        };
        return source_insertion(document, insertion, format!(", {factor_sql}{suffix_space}"));
    }

    let select_range = select
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let select_list = document
        .nodes
        .iter()
        .find(|node| node.parent == Some(select.id) && node.kind == SqlDocumentNodeKind::SelectList)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let last_item = document
        .nodes
        .iter()
        .filter(|node| {
            node.parent == Some(select_list.id) && node.kind == SqlDocumentNodeKind::SelectItem
        })
        .filter_map(|node| node.range)
        .max_by_key(|range| range.end)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let clause_start = next_select_clause_start(document, select, None)
        .unwrap_or(select_range.end)
        .min(select_range.end.max(last_item.end));
    let insertion = insertion_after_intervening_comments(document, last_item.end, clause_start)?;
    let prefix_space = if document
        .source
        .get(..insertion)
        .and_then(|prefix| prefix.chars().next_back())
        .is_some_and(char::is_whitespace)
    {
        ""
    } else {
        " "
    };
    let suffix_space = if insertion < clause_start {
        " "
    } else {
        Default::default()
    };
    source_insertion(
        document,
        insertion,
        format!("{prefix_space}FROM {factor_sql}{suffix_space}"),
    )
}

fn next_select_clause_start(
    document: &SqlDocument,
    select: &SqlDocumentNode,
    after: Option<usize>,
) -> Option<usize> {
    let query_id = query_ancestor_id(document, select.id);
    document
        .nodes
        .iter()
        .filter(|node| {
            let select_clause = node.parent == Some(select.id)
                && matches!(
                    node.kind,
                    SqlDocumentNodeKind::WhereClause
                        | SqlDocumentNodeKind::GroupByClause
                        | SqlDocumentNodeKind::HavingClause
                );
            let query_clause = node.parent == query_id
                && matches!(
                    node.kind,
                    SqlDocumentNodeKind::OrderByClause
                        | SqlDocumentNodeKind::LimitClause
                        | SqlDocumentNodeKind::OffsetClause
                );
            select_clause || query_clause
        })
        .filter_map(|node| node.range.map(|range| range.start))
        .filter(|start| after.is_none_or(|after| *start >= after))
        .min()
}

fn insertion_after_intervening_comments(
    document: &SqlDocument,
    base: usize,
    limit: usize,
) -> Result<usize, SqlDocumentEditError> {
    if base > limit || limit > document.source.len() {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let mut insertion = base;
    for token in document.tokens.iter().filter(|token| {
        token.kind == SqlDocumentTokenKind::Comment
            && token.range.start >= base
            && token.range.end <= limit
    }) {
        insertion = token.range.end;
        if document
            .source_slice(token.range)
            .is_some_and(|text| text.starts_with("--"))
        {
            if let Some(newline) = document.source[insertion..limit].find('\n') {
                insertion += newline + 1;
            }
        }
    }
    Ok(insertion)
}

fn range_has_comment(document: &SqlDocument, range: SqlSourceRange) -> bool {
    document.tokens.iter().any(|token| {
        token.kind == SqlDocumentTokenKind::Comment
            && token.range.start < range.end
            && range.start < token.range.end
    })
}

fn apply_sql_document_edit_transaction(
    current_source: &str,
    transaction: &SqlDocumentEditTransaction,
) -> Result<SqlDocumentEditResult, SqlDocumentEditError> {
    if current_source.len() > MAX_SQL_DOCUMENT_BYTES
        || transaction.expected_source.len() > MAX_SQL_DOCUMENT_BYTES
    {
        return Err(SqlDocumentEditError::SourceTooLarge);
    }
    if current_source != transaction.expected_source {
        return Err(SqlDocumentEditError::StaleSource);
    }
    if transaction.edits.len() > MAX_SQL_DOCUMENT_EDITS {
        return Err(SqlDocumentEditError::TooManyEdits);
    }

    let mut edits: Vec<_> = transaction.edits.iter().collect();
    edits.sort_by_key(|edit| (edit.range.start, edit.range.end));

    let mut previous: Option<&SqlDocumentEdit> = None;
    let mut removed_bytes = 0usize;
    let mut added_bytes = 0usize;
    for edit in &edits {
        let range = edit.range;
        if range.start > range.end {
            return Err(SqlDocumentEditError::InvalidRange);
        }
        if range.end > current_source.len() {
            return Err(SqlDocumentEditError::RangeOutOfBounds);
        }
        if !current_source.is_char_boundary(range.start)
            || !current_source.is_char_boundary(range.end)
        {
            return Err(SqlDocumentEditError::NotUtf8Boundary);
        }
        if let Some(previous) = previous {
            if range.start < previous.range.end
                || (range.start == previous.range.start
                    && (range.start == range.end || previous.range.start == previous.range.end))
            {
                return Err(SqlDocumentEditError::OverlappingEdits);
            }
        }
        if current_source.get(range.start..range.end) != Some(edit.expected_text.as_str()) {
            return Err(SqlDocumentEditError::PreimageMismatch);
        }
        removed_bytes = removed_bytes
            .checked_add(range.end - range.start)
            .ok_or(SqlDocumentEditError::OutputTooLarge)?;
        added_bytes = added_bytes
            .checked_add(edit.replacement.len())
            .ok_or(SqlDocumentEditError::OutputTooLarge)?;
        previous = Some(edit);
    }

    let output_len = current_source
        .len()
        .checked_sub(removed_bytes)
        .and_then(|size| size.checked_add(added_bytes))
        .filter(|size| *size <= MAX_SQL_DOCUMENT_BYTES)
        .ok_or(SqlDocumentEditError::OutputTooLarge)?;

    let mut output = String::with_capacity(output_len);
    let mut inverse_edits: Vec<SqlDocumentEdit> = Vec::with_capacity(edits.len());
    let mut source_cursor = 0;
    for edit in edits {
        output.push_str(&current_source[source_cursor..edit.range.start]);
        let inverse_start = output.len();
        output.push_str(&edit.replacement);
        let inverse = SqlDocumentEdit {
            range: SqlSourceRange {
                start: inverse_start,
                end: output.len(),
            },
            expected_text: edit.replacement.clone(),
            replacement: edit.expected_text.clone(),
        };
        if let Some(previous) = inverse_edits.last_mut() {
            if previous.range.end == inverse.range.start {
                previous.range.end = inverse.range.end;
                previous.expected_text.push_str(&inverse.expected_text);
                previous.replacement.push_str(&inverse.replacement);
            } else {
                inverse_edits.push(inverse);
            }
        } else {
            inverse_edits.push(inverse);
        }
        source_cursor = edit.range.end;
    }
    output.push_str(&current_source[source_cursor..]);
    debug_assert_eq!(output.len(), output_len);

    let undo = SqlDocumentEditTransaction {
        expected_source: output.clone(),
        edits: inverse_edits,
    };
    Ok(SqlDocumentEditResult {
        document: parse_sql_document(&output),
        undo,
    })
}

/// Parse SQL text into its lossless lexical document and syntax view.
///
/// `GenericDialect` matches the default dialect used by the current DataFusion
/// SQL parser. This is a syntax parser only: runtime admission, function
/// policy, authorization, planning, and type checking remain Core/executor
/// decisions.
fn valid_sql_parameter_name(name: &str) -> bool {
    let mut characters = name.chars();
    matches!(characters.next(), Some(first) if first == '_' || first.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn is_sql_parameter_boundary(source: &str, start: usize) -> bool {
    let Some(previous_byte) = start
        .checked_sub(1)
        .and_then(|offset| source.as_bytes().get(offset))
    else {
        return true;
    };
    !previous_byte.is_ascii_alphanumeric() && *previous_byte != b'_' && *previous_byte != b'$'
}

fn sql_template_parameter_name(source: &str, start: usize, end: usize) -> Option<&str> {
    let contents = source.get(start..end)?;
    let mut characters = contents.char_indices();
    let (name_start, first) = loop {
        let (offset, character) = characters.next()?;
        if !character.is_whitespace() {
            break (offset, character);
        }
    };
    if first != '_' && !first.is_ascii_alphabetic() {
        return None;
    }

    let mut name_end = name_start + first.len_utf8();
    let mut trailing_whitespace = false;
    for (offset, character) in characters {
        if trailing_whitespace {
            if !character.is_whitespace() {
                return None;
            }
        } else if character == '_' || character.is_ascii_alphanumeric() {
            name_end = offset + character.len_utf8();
        } else if character.is_whitespace() {
            trailing_whitespace = true;
        } else {
            return None;
        }
    }
    Some(&contents[name_start..name_end])
}

/// Apply simultaneous, guarded byte-range replacements to an SQL source.
///
/// Every range and expected slice is checked against the same original source.
/// If any guard fails, no output is produced. The returned inverse transaction
/// has both a whole-source stale guard and per-range preimage guards, so it can
/// safely be used for undo only while the edited draft is still current.
///
/// This operation is a bounded source edit only. The result is reparsed for
/// syntax diagnostics, but it does not authorize, validate, plan, save, or run
/// SQL.

fn is_expression_node_kind(kind: SqlDocumentNodeKind) -> bool {
    matches!(
        kind,
        SqlDocumentNodeKind::IdentifierExpression
            | SqlDocumentNodeKind::CompoundIdentifierExpression
            | SqlDocumentNodeKind::Value
            | SqlDocumentNodeKind::LiteralExpression
            | SqlDocumentNodeKind::BinaryExpression
            | SqlDocumentNodeKind::UnaryExpression
            | SqlDocumentNodeKind::FunctionExpression
            | SqlDocumentNodeKind::CaseExpression
            | SqlDocumentNodeKind::CastExpression
            | SqlDocumentNodeKind::SubqueryExpression
            | SqlDocumentNodeKind::PredicateExpression
            | SqlDocumentNodeKind::NestedExpression
            | SqlDocumentNodeKind::OtherExpression
    )
}

/// Rename every exact named `$old_name` placeholder token in a SQL source.
///
/// Names are supplied without the `$` prefix. Replacements are limited to
/// tokenizer-classified placeholder tokens, so strings, comments, quoted
/// identifiers, and longer parameter names remain byte-for-byte unchanged.
/// The resulting source is reparsed into a document; syntax-invalid drafts
/// remain editable and retain their updated diagnostics.

fn valid_sql_number_literal(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || !bytes.is_ascii() {
        return false;
    }

    let mut index = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    if index == bytes.len() {
        return false;
    }
    let integer_start = index;
    while bytes.get(index).is_some_and(u8::is_ascii_digit) {
        index += 1;
    }
    let has_integer_digits = index > integer_start;

    let mut has_fraction_digits = false;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let fraction_start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        has_fraction_digits = index > fraction_start;
    }
    if !has_integer_digits && !has_fraction_digits {
        return false;
    }

    if matches!(bytes.get(index), Some(b'e' | b'E')) {
        index += 1;
        if matches!(bytes.get(index), Some(b'+' | b'-')) {
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

/// Rename every exact named `$old_name` placeholder token in a SQL source.
///
/// Names are supplied without the `$` prefix. Replacements are limited to
/// tokenizer-classified placeholder tokens, so strings, comments, quoted
/// identifiers, and longer parameter names remain byte-for-byte unchanged.
/// The resulting source is reparsed into a document; syntax-invalid drafts
/// remain editable and retain their updated diagnostics.
pub fn rename_sql_document_parameter(
    source: &str,
    old_name: &str,
    new_name: &str,
) -> Result<SqlDocument, SqlDocumentEditError> {
    if !valid_sql_parameter_name(old_name) {
        return Err(SqlDocumentEditError::InvalidOldParameterName);
    }
    if !valid_sql_parameter_name(new_name) {
        return Err(SqlDocumentEditError::InvalidNewParameterName);
    }

    let document = parse_sql_document(source);
    match document.status {
        SqlDocumentStatus::TooLarge => return Err(SqlDocumentEditError::SourceTooLarge),
        SqlDocumentStatus::TooComplex => return Err(SqlDocumentEditError::SourceTooComplex),
        _ => {}
    }
    let replacements = document
        .tokens
        .iter()
        .filter(|token| token.kind == SqlDocumentTokenKind::Placeholder)
        .filter_map(|token| {
            let text = document.source_slice(token.range)?;
            (text.strip_prefix('$') == Some(old_name)).then_some(token.range)
        })
        .collect::<Vec<_>>();

    if replacements.is_empty() {
        return Ok(document);
    }

    let old_token_bytes = old_name
        .len()
        .checked_add(1)
        .ok_or(SqlDocumentEditError::OutputTooLarge)?;
    let replacement_bytes = new_name
        .len()
        .checked_add(1)
        .ok_or(SqlDocumentEditError::OutputTooLarge)?;
    let output_len = source
        .len()
        .checked_sub(
            replacements
                .len()
                .checked_mul(old_token_bytes)
                .ok_or(SqlDocumentEditError::OutputTooLarge)?,
        )
        .and_then(|length| {
            replacements
                .len()
                .checked_mul(replacement_bytes)
                .and_then(|added| length.checked_add(added))
        })
        .filter(|length| *length <= MAX_SQL_DOCUMENT_BYTES)
        .ok_or(SqlDocumentEditError::OutputTooLarge)?;

    let replacement = format!("${new_name}");
    let mut rewritten_source = String::with_capacity(output_len);
    let mut source_cursor = 0;
    for range in replacements {
        rewritten_source.push_str(&source[source_cursor..range.start]);
        rewritten_source.push_str(&replacement);
        source_cursor = range.end;
    }
    rewritten_source.push_str(&source[source_cursor..]);
    debug_assert_eq!(rewritten_source.len(), output_len);

    Ok(parse_sql_document(&rewritten_source))
}

pub fn parse_sql_document(source: &str) -> SqlDocument {
    let mut document = SqlDocument {
        contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
        source: source.to_owned(),
        status: SqlDocumentStatus::InvalidSql,
        diagnostics: Vec::new(),
        tokens: Vec::new(),
        nodes: Vec::new(),
        conditions: Vec::new(),
    };

    if source.len() > MAX_SQL_DOCUMENT_BYTES {
        document.status = SqlDocumentStatus::TooLarge;
        document.diagnostics.push(SqlDocumentDiagnostic {
            code: SqlDocumentDiagnosticCode::SqlTooLarge,
            message: format!("SQL exceeds the {MAX_SQL_DOCUMENT_BYTES}-byte limit"),
            range: None,
        });
        return document;
    }

    let source_map = SqlSourceMap::new(source);
    let dialect = GenericDialect {};
    match tokenize_source(source, &dialect, &source_map) {
        Ok(tokens) => {
            let syntax_token_count = tokens
                .iter()
                .filter(|token| {
                    !matches!(
                        token.kind,
                        SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
                    )
                })
                .count();
            if syntax_token_count > MAX_SQL_DOCUMENT_COMPLEXITY_TOKENS {
                document.status = SqlDocumentStatus::TooComplex;
                document.diagnostics.push(SqlDocumentDiagnostic {
                    code: SqlDocumentDiagnosticCode::SqlTooComplex,
                    message: format!(
                        "SQL exceeds the {MAX_SQL_DOCUMENT_COMPLEXITY_TOKENS}-token SQL Document complexity limit"
                    ),
                    range: None,
                });
                return document;
            }
            document.tokens = tokens;
        }
        Err(error) => {
            document.diagnostics.push(SqlDocumentDiagnostic {
                code: SqlDocumentDiagnosticCode::ParseFailed,
                message: error,
                range: None,
            });
        }
    }

    let parsed = parse_generic_sql_statements(source);
    match parsed {
        Err(error) => {
            document.status = SqlDocumentStatus::InvalidSql;
            if document.diagnostics.is_empty() {
                document.diagnostics.push(SqlDocumentDiagnostic {
                    code: SqlDocumentDiagnosticCode::ParseFailed,
                    message: parser_error_message(error),
                    range: None,
                });
            }
        }
        Ok(statements) => match statements.as_slice() {
            [] => {
                document.status = SqlDocumentStatus::Empty;
                document.diagnostics.push(SqlDocumentDiagnostic {
                    code: SqlDocumentDiagnosticCode::EmptyDocument,
                    message: "SQL statement is required".to_owned(),
                    range: None,
                });
            }
            [Statement::Query(_)] => {
                document.status = SqlDocumentStatus::Parsed;
                document.nodes = collect_nodes(&statements, source, &source_map, &document.tokens);
            }
            [_] => {
                document.status = SqlDocumentStatus::SelectRequired;
                document.nodes = collect_nodes(&statements, source, &source_map, &document.tokens);
                document.diagnostics.push(SqlDocumentDiagnostic {
                    code: SqlDocumentDiagnosticCode::SelectRequired,
                    message: "a single read-only SELECT statement is required".to_owned(),
                    range: statements.first().and_then(|statement| {
                        SqlSourceRange::from_span(source, &source_map, statement.span())
                    }),
                });
            }
            _ => {
                document.status = SqlDocumentStatus::MultipleStatements;
                document.nodes = collect_nodes(&statements, source, &source_map, &document.tokens);
                document.diagnostics.push(SqlDocumentDiagnostic {
                    code: SqlDocumentDiagnosticCode::SingleStatementRequired,
                    message: "exactly one SQL statement is required".to_owned(),
                    range: None,
                });
            }
        },
    }

    document.conditions = derive_sql_document_conditions(&document);
    document
}

/// Normalize the user-facing `{{name}}` SQL parameter spelling using the
/// shared Rust SQL tokenizer and preserve original-source edit coordinates.
///
/// Only opening braces classified as SQL punctuation can begin a marker, so
/// strings (including dollar-quoted and triple-quoted strings), quoted
/// identifiers, and comments remain opaque. SQL syntax errors remain drafts:
/// if tokenization itself fails, the original source is returned unchanged
/// and later parser/executor diagnostics remain authoritative.
pub fn normalize_sql_template(source: &str) -> SqlTemplateNormalization {
    let dialect = GenericDialect {};
    let source_map = SqlSourceMap::new(source);
    let tokens = match tokenize_source(source, &dialect, &source_map) {
        Ok(tokens) => tokens,
        Err(_) => {
            return SqlTemplateNormalization {
                sql: source.to_owned(),
                parameters: Vec::new(),
                replacements: Vec::new(),
                error: None,
            };
        }
    };

    let mut parameters = Vec::new();
    let mut seen_parameters = HashSet::new();
    let mut edits = Vec::new();
    let mut error = None;
    let mut closing_braces = source.match_indices("}}").peekable();
    let mut index = 0;

    while index < tokens.len() {
        let token = &tokens[index];
        let Some(text) = source.get(token.range.start..token.range.end) else {
            index += 1;
            continue;
        };

        if token.kind == SqlDocumentTokenKind::Placeholder {
            if let Some(name) = text.strip_prefix('$').filter(|name| {
                valid_sql_parameter_name(name)
                    && is_sql_parameter_boundary(source, token.range.start)
            }) {
                if seen_parameters.insert(name.to_owned()) {
                    parameters.push(name.to_owned());
                }
            }
            index += 1;
            continue;
        }

        if token.kind != SqlDocumentTokenKind::Punctuation
            || text != "{"
            || !source[token.range.start..].starts_with("{{")
        {
            index += 1;
            continue;
        }

        let marker_contents_start = token.range.start + 2;
        while closing_braces
            .peek()
            .is_some_and(|(start, _)| *start < marker_contents_start)
        {
            closing_braces.next();
        }
        let Some((close_start, _)) = closing_braces.peek().copied() else {
            error.get_or_insert(SqlTemplateNormalizationError::MissingClosingBraces {
                range: SqlSourceRange {
                    start: token.range.start,
                    end: source.len(),
                },
            });
            index += 1;
            continue;
        };
        let name = sql_template_parameter_name(source, marker_contents_start, close_start);
        let range = SqlSourceRange {
            start: token.range.start,
            end: close_start + 2,
        };
        let Some(name) = name else {
            error.get_or_insert(SqlTemplateNormalizationError::InvalidParameterName { range });
            index += 1;
            continue;
        };

        let replacement = format!("${name}");
        edits.push((range, replacement.clone()));
        if seen_parameters.insert(name.to_owned()) {
            parameters.push(name.to_owned());
        }
        while index < tokens.len() && tokens[index].range.end <= range.end {
            index += 1;
        }
    }

    let mut normalized_sql = String::with_capacity(source.len());
    let mut replacements = Vec::with_capacity(edits.len());
    let mut source_cursor = 0;
    for (source_range, replacement) in edits {
        normalized_sql.push_str(&source[source_cursor..source_range.start]);
        normalized_sql.push_str(&replacement);
        replacements.push([source_range.start, source_range.end, replacement.len()]);
        source_cursor = source_range.end;
    }
    normalized_sql.push_str(&source[source_cursor..]);

    SqlTemplateNormalization {
        sql: normalized_sql,
        parameters,
        replacements,
        error,
    }
}

fn derive_sql_document_conditions(document: &SqlDocument) -> Vec<SqlDocumentConditionScope> {
    document
        .nodes
        .iter()
        .filter(|node| node.kind == SqlDocumentNodeKind::Select)
        .map(|select| {
            let clause = document.nodes.iter().find(|node| {
                node.parent == Some(select.id) && node.kind == SqlDocumentNodeKind::WhereClause
            });
            let expression = clause
                .and_then(|clause| {
                    document.nodes.iter().find(|node| {
                        node.parent == Some(clause.id) && is_expression_node_kind(node.kind)
                    })
                })
                .and_then(|root| condition_node_from_document(document, root));
            SqlDocumentConditionScope {
                query_node_id: select.id,
                kind: SqlDocumentConditionScopeKind::Where,
                scope_node_id: select.id,
                where_clause_node_id: clause.map(|clause| clause.id),
                expression,
            }
        })
        .chain(document.nodes.iter().filter_map(|function| {
            if function.kind != SqlDocumentNodeKind::FunctionExpression {
                return None;
            }
            let is_registered_aggregate = match &function.semantic {
                Some(SqlDocumentNodeData::Function {
                    name,
                    argument_count: Some(argument_count),
                    has_filter: true,
                    has_over: false,
                    has_argument_clauses: false,
                    ..
                }) if *argument_count > 0 && name.len() == 1 => name.last().is_some_and(|part| {
                    crate::sql_functions::user_sql_aggregate(&part.value).is_some()
                }),
                _ => false,
            };
            if !is_registered_aggregate {
                return None;
            }
            let clause = document.nodes.iter().find(|candidate| {
                candidate.parent == Some(function.id)
                    && candidate.kind == SqlDocumentNodeKind::WhereClause
            })?;
            let root = document.nodes.iter().find(|candidate| {
                candidate.parent == Some(clause.id) && is_expression_node_kind(candidate.kind)
            })?;
            let mut ancestor = function.id;
            let select_id = loop {
                let current = document.nodes.get(ancestor as usize)?;
                if current.id != ancestor {
                    return None;
                }
                if current.kind == SqlDocumentNodeKind::Select {
                    break ancestor;
                }
                ancestor = current.parent?;
            };
            Some(SqlDocumentConditionScope {
                query_node_id: select_id,
                kind: SqlDocumentConditionScopeKind::AggregateFilter,
                scope_node_id: function.id,
                where_clause_node_id: Some(clause.id),
                expression: condition_node_from_document(document, root),
            })
        }))
        .collect()
}

fn condition_node_from_document(
    document: &SqlDocument,
    node: &SqlDocumentNode,
) -> Option<SqlDocumentConditionNode> {
    condition_node_from_document_at_depth(document, node, 0)
}

fn condition_node_from_document_at_depth(
    document: &SqlDocument,
    node: &SqlDocumentNode,
    depth: usize,
) -> Option<SqlDocumentConditionNode> {
    let raw = || {
        node.range
            .and_then(|range| document.source_slice(range))
            .map(str::to_owned)
            .unwrap_or_default()
    };
    let children: Vec<_> = document
        .nodes
        .iter()
        .filter(|candidate| {
            candidate.parent == Some(node.id) && is_expression_node_kind(candidate.kind)
        })
        .collect();
    if depth >= 32 {
        return Some(SqlDocumentConditionNode::Advanced {
            node_id: node.id,
            sql: raw(),
        });
    }
    if node.kind == SqlDocumentNodeKind::NestedExpression {
        if let [child] = children.as_slice() {
            if let Some(condition) =
                condition_node_from_document_at_depth(document, child, depth + 1)
            {
                return Some(condition);
            }
        }
    }
    if let Some(SqlDocumentNodeData::Predicate { operator, negated }) = node.semantic.as_ref() {
        if operator == "is_null" {
            if let [column_node] = children.as_slice() {
                if let Some(column) = column_reference_from_node(column_node) {
                    return Some(SqlDocumentConditionNode::Comparison {
                        node_id: node.id,
                        column_node_id: column_node.id,
                        column,
                        operator: if *negated {
                            SqlDocumentFilterOperator::IsNotNull
                        } else {
                            SqlDocumentFilterOperator::IsNull
                        },
                        value: None,
                    });
                }
            }
        }
    }
    if let Some(SqlDocumentNodeData::BinaryOperator { operator }) = node.semantic.as_ref() {
        if operator.eq_ignore_ascii_case("AND") || operator.eq_ignore_ascii_case("OR") {
            let group_operator = if operator.eq_ignore_ascii_case("AND") {
                SqlDocumentConditionGroupOperator::And
            } else {
                SqlDocumentConditionGroupOperator::Or
            };
            let mut condition_children = Vec::new();
            for child in children.iter().copied() {
                let Some(condition) =
                    condition_node_from_document_at_depth(document, child, depth + 1)
                else {
                    continue;
                };
                match condition {
                    SqlDocumentConditionNode::Group {
                        operator: child_operator,
                        children,
                        ..
                    } if child.kind == SqlDocumentNodeKind::BinaryExpression
                        && child_operator == group_operator =>
                    {
                        condition_children.extend(children);
                    }
                    _ => condition_children.push(condition),
                }
            }
            if condition_children.len() >= 2 {
                return Some(SqlDocumentConditionNode::Group {
                    node_id: node.id,
                    operator: group_operator,
                    children: condition_children,
                });
            }
        }
        let filter_operator = match operator.as_str() {
            "=" => Some(SqlDocumentFilterOperator::Eq),
            "<>" | "!=" => Some(SqlDocumentFilterOperator::NotEq),
            "<" => Some(SqlDocumentFilterOperator::Lt),
            "<=" => Some(SqlDocumentFilterOperator::Lte),
            ">" => Some(SqlDocumentFilterOperator::Gt),
            ">=" => Some(SqlDocumentFilterOperator::Gte),
            "LIKE" => Some(SqlDocumentFilterOperator::Like),
            "NOT LIKE" => Some(SqlDocumentFilterOperator::NotLike),
            _ => None,
        };
        if let (Some(operator), [column_node, value_node]) = (filter_operator, children.as_slice())
        {
            if let (Some(column), Some(value)) = (
                column_reference_from_node(column_node),
                literal_from_node(document, value_node),
            ) {
                return Some(SqlDocumentConditionNode::Comparison {
                    node_id: node.id,
                    column_node_id: column_node.id,
                    column,
                    operator,
                    value: Some(value),
                });
            }
        }
    }
    Some(SqlDocumentConditionNode::Advanced {
        node_id: node.id,
        sql: raw(),
    })
}

fn column_reference_from_node(node: &SqlDocumentNode) -> Option<SqlDocumentColumnReference> {
    match node.semantic.as_ref()? {
        SqlDocumentNodeData::Identifier { value, quote_style } => {
            Some(SqlDocumentColumnReference {
                qualifier: None,
                column: SqlDocumentIdentifier {
                    value: value.clone(),
                    quote_style: *quote_style,
                },
            })
        }
        SqlDocumentNodeData::CompoundIdentifier { parts } if parts.len() == 2 => {
            Some(SqlDocumentColumnReference {
                qualifier: Some(parts[0].clone()),
                column: parts[1].clone(),
            })
        }
        _ => None,
    }
}

fn literal_from_node(
    document: &SqlDocument,
    node: &SqlDocumentNode,
) -> Option<SqlDocumentSemanticLiteral> {
    if let Some(SqlDocumentNodeData::UnaryOperator { operator }) = node.semantic.as_ref() {
        if !matches!(operator.as_str(), "+" | "-") {
            return None;
        }
        let operand = document.nodes.iter().find(|candidate| {
            candidate.parent == Some(node.id) && is_expression_node_kind(candidate.kind)
        })?;
        let SqlDocumentSemanticLiteral::Number { value } = literal_from_node(document, operand)?
        else {
            return None;
        };
        let value = format!("{operator}{value}");
        return valid_sql_number_literal(&value)
            .then_some(SqlDocumentSemanticLiteral::Number { value });
    }
    match node.semantic.as_ref()? {
        SqlDocumentNodeData::Literal {
            literal_kind: SqlDocumentLiteralKind::Number,
            raw: Some(value),
        } if valid_sql_number_literal(value) => Some(SqlDocumentSemanticLiteral::Number {
            value: value.clone(),
        }),
        SqlDocumentNodeData::Literal {
            literal_kind: SqlDocumentLiteralKind::String,
            raw: Some(value),
        } => Some(SqlDocumentSemanticLiteral::String {
            value: parse_sql_string_value(value)?,
        }),
        SqlDocumentNodeData::Literal {
            literal_kind: SqlDocumentLiteralKind::Boolean,
            raw: Some(value),
        } => Some(SqlDocumentSemanticLiteral::Boolean {
            value: value.eq_ignore_ascii_case("true"),
        }),
        SqlDocumentNodeData::Literal {
            literal_kind: SqlDocumentLiteralKind::Null,
            ..
        } => Some(SqlDocumentSemanticLiteral::Null),
        SqlDocumentNodeData::Literal {
            literal_kind: SqlDocumentLiteralKind::Placeholder,
            raw: Some(value),
        } if value.starts_with('$') && valid_sql_parameter_name(&value[1..]) => {
            Some(SqlDocumentSemanticLiteral::NamedParameter {
                name: value[1..].to_owned(),
            })
        }
        _ => None,
    }
}

fn parse_sql_string_value(source: &str) -> Option<String> {
    let trimmed = source.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('\'') && trimmed.ends_with('\'') {
        Some(trimmed[1..trimmed.len() - 1].replace("''", "'"))
    } else {
        None
    }
}

fn parser_error_message(error: ParserError) -> String {
    match error {
        ParserError::TokenizerError(message) | ParserError::ParserError(message) => message,
        ParserError::RecursionLimitExceeded => "SQL nesting limit exceeded".to_owned(),
    }
}

fn tokenize_source(
    source: &str,
    dialect: &dyn sqlparser::dialect::Dialect,
    source_map: &SqlSourceMap,
) -> Result<Vec<SqlDocumentToken>, String> {
    let mut tokenizer = Tokenizer::new(dialect, source).with_unescape(false);
    let tokens = tokenizer
        .tokenize_with_location()
        .map_err(|error| error.to_string())?;

    Ok(tokens
        .into_iter()
        .filter_map(|token| {
            if token.token == Token::EOF {
                return None;
            }
            let range = SqlSourceRange::from_span(source, source_map, token.span)?;
            Some(SqlDocumentToken {
                kind: classify_token(&token.token),
                range,
            })
        })
        .collect())
}

fn classify_token(token: &Token) -> SqlDocumentTokenKind {
    match token {
        Token::Whitespace(
            Whitespace::SingleLineComment { .. } | Whitespace::MultiLineComment(_),
        ) => SqlDocumentTokenKind::Comment,
        Token::Whitespace(_) => SqlDocumentTokenKind::Trivia,
        Token::Word(word) if word.quote_style.is_some() => SqlDocumentTokenKind::QuotedIdentifier,
        Token::Word(_) => SqlDocumentTokenKind::Word,
        Token::Placeholder(_) => SqlDocumentTokenKind::Placeholder,
        Token::Number(_, _)
        | Token::SingleQuotedString(_)
        | Token::DoubleQuotedString(_)
        | Token::TripleSingleQuotedString(_)
        | Token::TripleDoubleQuotedString(_)
        | Token::DollarQuotedString(_)
        | Token::SingleQuotedByteStringLiteral(_)
        | Token::DoubleQuotedByteStringLiteral(_)
        | Token::TripleSingleQuotedByteStringLiteral(_)
        | Token::TripleDoubleQuotedByteStringLiteral(_)
        | Token::SingleQuotedRawStringLiteral(_)
        | Token::DoubleQuotedRawStringLiteral(_)
        | Token::TripleSingleQuotedRawStringLiteral(_)
        | Token::TripleDoubleQuotedRawStringLiteral(_)
        | Token::NationalStringLiteral(_)
        | Token::QuoteDelimitedStringLiteral(_)
        | Token::NationalQuoteDelimitedStringLiteral(_)
        | Token::EscapedStringLiteral(_)
        | Token::UnicodeStringLiteral(_)
        | Token::HexStringLiteral(_) => SqlDocumentTokenKind::Literal,
        Token::Comma
        | Token::LParen
        | Token::RParen
        | Token::Period
        | Token::Colon
        | Token::SemiColon
        | Token::LBracket
        | Token::RBracket
        | Token::LBrace
        | Token::RBrace => SqlDocumentTokenKind::Punctuation,
        _ => SqlDocumentTokenKind::Operator,
    }
}

fn collect_nodes(
    statements: &[Statement],
    source: &str,
    source_map: &SqlSourceMap,
    tokens: &[SqlDocumentToken],
) -> Vec<SqlDocumentNode> {
    let mut collector = NodeCollector {
        source,
        source_map,
        tokens,
        nodes: Vec::new(),
        parents: Vec::new(),
    };
    for statement in statements {
        let _ = statement.visit(&mut collector);
    }
    collector.attach_projection_children();
    let mut nodes = collector.nodes;
    collect_query_structure_nodes(statements, source, source_map, tokens, &mut nodes);
    nodes
}

fn collect_query_structure_nodes(
    statements: &[Statement],
    source: &str,
    source_map: &SqlSourceMap,
    tokens: &[SqlDocumentToken],
    nodes: &mut Vec<SqlDocumentNode>,
) {
    let query_ids = nodes
        .iter()
        .filter(|node| node.kind == SqlDocumentNodeKind::Query)
        .map(|node| node.id)
        .collect();
    let select_ids = nodes
        .iter()
        .filter(|node| node.kind == SqlDocumentNodeKind::Select)
        .map(|node| node.id)
        .collect();
    let original_node_count = nodes.len();
    let mut collector = QueryStructureCollector {
        source,
        source_map,
        tokens,
        token_depths: token_depths(source, tokens),
        nodes,
        query_ids,
        select_ids,
        query_cursor: 0,
        select_cursor: 0,
        original_node_count,
    };
    for statement in statements {
        let _ = statement.visit(&mut collector);
    }
}

struct QueryStructureCollector<'a> {
    source: &'a str,
    source_map: &'a SqlSourceMap,
    tokens: &'a [SqlDocumentToken],
    token_depths: Vec<usize>,
    nodes: &'a mut Vec<SqlDocumentNode>,
    query_ids: Vec<u32>,
    select_ids: Vec<u32>,
    query_cursor: usize,
    select_cursor: usize,
    original_node_count: usize,
}

impl QueryStructureCollector<'_> {
    fn group_by_data(&self, group_by: &GroupByExpr) -> Option<SqlDocumentNodeData> {
        let expression_groups = |groups: &[Vec<Expr>]| -> Option<Vec<Vec<u32>>> {
            groups
                .iter()
                .map(|group| {
                    group
                        .iter()
                        .map(|expression| self.expression_node_id(expression))
                        .collect()
                })
                .collect()
        };
        let expression_ids = |expressions: &[Expr]| -> Option<Vec<u32>> {
            expressions
                .iter()
                .map(|expression| self.expression_node_id(expression))
                .collect()
        };
        let grouped_expression =
            |expression: &Expr| -> Option<(SqlDocumentGroupByMode, Vec<Vec<u32>>, bool)> {
                match expression {
                    Expr::GroupingSets(groups) => Some((
                        SqlDocumentGroupByMode::GroupingSets,
                        expression_groups(groups)?,
                        true,
                    )),
                    Expr::Rollup(groups) | Expr::Cube(groups) => {
                        let mode = if matches!(expression, Expr::Rollup(_)) {
                            SqlDocumentGroupByMode::Rollup
                        } else {
                            SqlDocumentGroupByMode::Cube
                        };
                        let groups = expression_groups(groups)?;
                        let editable = groups.iter().all(|group| group.len() == 1);
                        let groups = if editable {
                            vec![groups.into_iter().flatten().collect()]
                        } else {
                            groups
                        };
                        Some((mode, groups, editable))
                    }
                    _ => None,
                }
            };

        let (mode, groups, editable) = match group_by {
            GroupByExpr::All(_) => (SqlDocumentGroupByMode::All, Vec::new(), true),
            GroupByExpr::Expressions(expressions, modifiers) => {
                if let Some((mode, groups, editable)) =
                    modifiers.iter().find_map(|modifier| match modifier {
                        GroupByWithModifier::GroupingSets(expression) => {
                            grouped_expression(expression)
                        }
                        GroupByWithModifier::Rollup => Some((
                            SqlDocumentGroupByMode::Rollup,
                            vec![expression_ids(expressions)?],
                            true,
                        )),
                        GroupByWithModifier::Cube => Some((
                            SqlDocumentGroupByMode::Cube,
                            vec![expression_ids(expressions)?],
                            true,
                        )),
                        GroupByWithModifier::Totals => None,
                    })
                {
                    (mode, groups, editable)
                } else if expressions.len() == 1 {
                    grouped_expression(&expressions[0]).unwrap_or((
                        SqlDocumentGroupByMode::Expressions,
                        vec![expression_ids(expressions)?],
                        true,
                    ))
                } else {
                    (
                        SqlDocumentGroupByMode::Expressions,
                        vec![expression_ids(expressions)?],
                        true,
                    )
                }
            }
        };
        Some(SqlDocumentNodeData::GroupBy {
            mode,
            groups,
            editable,
        })
    }

    fn expression_node_id(&self, expression: &Expr) -> Option<u32> {
        let range = SqlSourceRange::from_span(self.source, self.source_map, expression.span())?;
        self.nodes[..self.original_node_count]
            .iter()
            .filter(|node| {
                node.kind == expression_kind(expression)
                    && node
                        .range
                        .is_some_and(|candidate| candidate.start == range.start)
            })
            .max_by_key(|node| node.range.map(|candidate| candidate.end))
            .map(|node| node.id)
    }

    fn expression_node_range(&self, parent: u32, expression: &Expr) -> Option<SqlSourceRange> {
        let span = SqlSourceRange::from_span(self.source, self.source_map, expression.span())?;
        self.nodes[..self.original_node_count]
            .iter()
            .filter(|node| {
                node.parent == Some(parent)
                    && node.kind == expression_kind(expression)
                    && node.range.is_some_and(|range| range.start == span.start)
            })
            .filter_map(|node| node.range)
            .max_by_key(|range| range.end)
            .or(Some(span))
    }

    fn add_node(
        &mut self,
        kind: SqlDocumentNodeKind,
        range: Option<SqlSourceRange>,
        parent: u32,
    ) -> u32 {
        self.add_node_with_data(kind, range, parent, None)
    }

    fn add_node_with_data(
        &mut self,
        kind: SqlDocumentNodeKind,
        range: Option<SqlSourceRange>,
        parent: u32,
        semantic: Option<SqlDocumentNodeData>,
    ) -> u32 {
        let id = self.nodes.len() as u32;
        self.nodes.push(SqlDocumentNode {
            id,
            parent: Some(parent),
            kind,
            range,
            semantic,
        });
        id
    }

    fn span_range<T: Spanned>(&self, node: &T) -> Option<SqlSourceRange> {
        SqlSourceRange::from_span(self.source, self.source_map, node.span())
    }

    fn direct_child(
        &self,
        parent: u32,
        kind: SqlDocumentNodeKind,
        range: Option<SqlSourceRange>,
    ) -> Option<u32> {
        let range = range?;
        self.nodes[..self.original_node_count]
            .iter()
            .find(|node| {
                node.parent == Some(parent) && node.kind == kind && node.range == Some(range)
            })
            .map(|node| node.id)
    }

    fn reparent_direct_children(
        &mut self,
        anchor: u32,
        parent: u32,
        range: SqlSourceRange,
        kinds: &[SqlDocumentNodeKind],
    ) {
        for node in &mut self.nodes[..self.original_node_count] {
            if node.parent == Some(anchor)
                && kinds.contains(&node.kind)
                && node
                    .range
                    .is_some_and(|child| range.start <= child.start && child.end <= range.end)
            {
                node.parent = Some(parent);
            }
        }
    }

    fn select_depth(&self, select: &Select) -> Option<usize> {
        let span = select.select_token.0.span;
        let range = SqlSourceRange::from_span(self.source, self.source_map, span)?;
        self.depth_at(range.start)
    }

    fn depth_at(&self, offset: usize) -> Option<usize> {
        self.tokens
            .iter()
            .position(|token| token.range.start <= offset && offset < token.range.end)
            .map(|index| self.token_depths[index])
    }

    fn keyword_phrase(
        &self,
        words: &[&str],
        start: usize,
        end: usize,
        depth: usize,
    ) -> Option<SqlSourceRange> {
        for (index, token) in self.tokens.iter().enumerate() {
            if token.range.start < start
                || token.range.start >= end
                || self.token_depths[index] != depth
            {
                continue;
            }
            if !token_is_keyword_at(self.source, self.tokens, index, words[0]) {
                continue;
            }
            let mut next_index = index + 1;
            let mut matched = true;
            for word in &words[1..] {
                next_index = skip_trivia_tokens(self.tokens, next_index);
                if self
                    .token_depths
                    .get(next_index)
                    .is_none_or(|next_depth| *next_depth != depth)
                    || !token_is_keyword_at(self.source, self.tokens, next_index, word)
                {
                    matched = false;
                    break;
                }
                next_index += 1;
            }
            if matched {
                return Some(token.range);
            }
        }
        None
    }

    fn last_keyword_before(&self, word: &str, end: usize, depth: usize) -> Option<SqlSourceRange> {
        self.tokens
            .iter()
            .enumerate()
            .filter(|(index, token)| {
                token.range.start < end
                    && self.token_depths[*index] == depth
                    && token_is_keyword_at(self.source, self.tokens, *index, word)
            })
            .map(|(_, token)| token.range)
            .next_back()
    }

    fn order_item_range(
        &self,
        expression_range: SqlSourceRange,
        order_clause_range: SqlSourceRange,
        depth: usize,
    ) -> SqlSourceRange {
        let end = self
            .tokens
            .iter()
            .enumerate()
            .find(|(index, token)| {
                token.range.start >= expression_range.end
                    && token.range.start < order_clause_range.end
                    && self.token_depths[*index] == depth
                    && token.kind == SqlDocumentTokenKind::Punctuation
                    && token_text(self.source, token) == Some(",")
            })
            .map(|(_, comma)| comma.range.start)
            .or_else(|| {
                self.tokens
                    .iter()
                    .enumerate()
                    .filter(|(index, token)| {
                        token.range.start >= expression_range.end
                            && token.range.end <= order_clause_range.end
                            && self.token_depths[*index] == depth
                            && !matches!(
                                token.kind,
                                SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
                            )
                    })
                    .map(|(_, token)| token.range.end)
                    .next_back()
            })
            .unwrap_or(expression_range.end);
        SqlSourceRange {
            start: expression_range.start,
            end: end.max(expression_range.end),
        }
    }

    fn collect_with_clause(&mut self, query: &Query, query_id: u32) {
        let Some(with) = &query.with else {
            return;
        };
        let with_id = self.add_node_with_data(
            SqlDocumentNodeKind::WithClause,
            self.span_range(with),
            query_id,
            Some(SqlDocumentNodeData::WithClause {
                recursive: with.recursive,
                cte_count: with.cte_tables.len(),
            }),
        );
        for cte in &with.cte_tables {
            let cte_id = self.add_node_with_data(
                SqlDocumentNodeKind::CommonTableExpression,
                self.span_range(cte),
                with_id,
                Some(SqlDocumentNodeData::CommonTableExpression {
                    name: identifier_data(&cte.alias.name),
                    columns: cte
                        .alias
                        .columns
                        .iter()
                        .map(|column| identifier_data(&column.name))
                        .collect(),
                    materialized: cte.materialized.map(|materialized| {
                        matches!(materialized, CteAsMaterialized::Materialized)
                    }),
                }),
            );
            if let Some(query_range) = self.span_range(cte.query.as_ref()) {
                if let Some(query_child) =
                    self.direct_child(query_id, SqlDocumentNodeKind::Query, Some(query_range))
                {
                    self.nodes[query_child as usize].parent = Some(cte_id);
                }
            }
        }
    }

    fn collect_set_expr(&mut self, expression: &SetExpr, parent: u32, query_id: u32) {
        match expression {
            SetExpr::SetOperation {
                left,
                op,
                set_quantifier,
                right,
            } => {
                let operation_id = self.add_node_with_data(
                    SqlDocumentNodeKind::SetOperation,
                    self.span_range(expression),
                    parent,
                    Some(SqlDocumentNodeData::SetOperation {
                        operator: op.to_string(),
                        quantifier: set_quantifier.to_string(),
                    }),
                );
                self.collect_set_operand(
                    left,
                    SqlDocumentNodeKind::SetOperationLeft,
                    operation_id,
                    query_id,
                );
                self.collect_set_operand(
                    right,
                    SqlDocumentNodeKind::SetOperationRight,
                    operation_id,
                    query_id,
                );
            }
            SetExpr::Select(select) => {
                if let Some(select_id) = self.direct_child(
                    query_id,
                    SqlDocumentNodeKind::Select,
                    self.span_range(select.as_ref()),
                ) {
                    self.nodes[select_id as usize].parent = Some(parent);
                }
            }
            SetExpr::Query(query) => {
                if let Some(query_id) = self.direct_child(
                    query_id,
                    SqlDocumentNodeKind::Query,
                    self.span_range(query.as_ref()),
                ) {
                    self.nodes[query_id as usize].parent = Some(parent);
                }
            }
            _ => {}
        }
    }

    fn collect_set_operand(
        &mut self,
        expression: &SetExpr,
        kind: SqlDocumentNodeKind,
        parent: u32,
        query_id: u32,
    ) {
        let operand_id = self.add_node(kind, self.span_range(expression), parent);
        self.collect_set_expr(expression, operand_id, query_id);
    }

    fn collect_query_clauses(&mut self, query: &Query, query_id: u32) {
        if let Some(order_by) = &query.order_by {
            let body_range = self.span_range(query.body.as_ref());
            let order_span = self.span_range(order_by);
            let scope_end = self.query_scope_end(query);
            if let (Some(body_range), Some(_order_span), Some(scope_end)) =
                (body_range, order_span, scope_end)
            {
                if let Some(keyword) = self.keyword_phrase(
                    &["ORDER", "BY"],
                    body_range.end,
                    scope_end,
                    self.depth_at(body_range.start).unwrap_or(0),
                ) {
                    let range = SqlSourceRange {
                        start: keyword.start,
                        end: self.order_clause_end(
                            keyword.end,
                            scope_end,
                            self.depth_at(keyword.start).unwrap_or(0),
                        ),
                    };
                    let order_id =
                        self.add_node(SqlDocumentNodeKind::OrderByClause, Some(range), query_id);
                    if let OrderByKind::Expressions(items) = &order_by.kind {
                        for item in items {
                            if let Some(expression_range) = self.span_range(&item.expr) {
                                let item_range = self.order_item_range(
                                    expression_range,
                                    range,
                                    self.depth_at(expression_range.start)
                                        .unwrap_or(self.depth_at(keyword.start).unwrap_or(0)),
                                );
                                let item_id = self.add_node_with_data(
                                    SqlDocumentNodeKind::OrderByItem,
                                    Some(item_range),
                                    order_id,
                                    Some(SqlDocumentNodeData::OrderByItem {
                                        direction: item.options.sort.as_ref().and_then(|sort| {
                                            match sort {
                                                OrderBySort::Asc => {
                                                    Some(SqlDocumentOrderDirection::Asc)
                                                }
                                                OrderBySort::Desc => {
                                                    Some(SqlDocumentOrderDirection::Desc)
                                                }
                                                OrderBySort::Using(_) => None,
                                            }
                                        }),
                                        nulls_order: item.options.nulls_first.map(|first| {
                                            if first {
                                                SqlDocumentNullsOrder::First
                                            } else {
                                                SqlDocumentNullsOrder::Last
                                            }
                                        }),
                                    }),
                                );
                                self.reparent_direct_children(
                                    query_id,
                                    item_id,
                                    item_range,
                                    &expression_node_kinds(),
                                );
                            }
                        }
                    }
                }
            }
        }

        if let Some(limit_clause) = &query.limit_clause {
            self.collect_limit_clause(query, query_id, limit_clause);
        }

        if let SetExpr::SetOperation { .. } = query.body.as_ref() {
            self.collect_set_expr(query.body.as_ref(), query_id, query_id);
        }
    }

    fn query_scope_end(&self, query: &Query) -> Option<usize> {
        let query_range = self.span_range(query)?;
        let start_index = self
            .tokens
            .iter()
            .position(|token| token.range.start >= query_range.start)?;
        let query_depth = self.token_depths.get(start_index).copied()?;
        let previous = skip_trivia_tokens_reverse(self.tokens, start_index);
        if let Some(open_index) = previous {
            if self
                .tokens
                .get(open_index)
                .and_then(|token| token_text(self.source, token))
                == Some("(")
            {
                if let Some(close_index) =
                    matching_close_paren(self.source, self.tokens, open_index)
                {
                    if self.tokens[close_index].range.start >= query_range.end {
                        return Some(self.tokens[close_index].range.start);
                    }
                }
            }
        }
        self.tokens
            .iter()
            .enumerate()
            .skip(start_index)
            .find(|(index, token)| {
                self.token_depths[*index] == query_depth
                    && token.kind == SqlDocumentTokenKind::Punctuation
                    && token_text(self.source, token) == Some(";")
            })
            .map(|(_, token)| token.range.start)
            .or(Some(self.source.len()))
    }

    fn order_clause_end(&self, start: usize, scope_end: usize, depth: usize) -> usize {
        let mut end = start;
        for (index, token) in self.tokens.iter().enumerate() {
            if token.range.start < start || token.range.start >= scope_end {
                continue;
            }
            if self.token_depths[index] == depth
                && ["LIMIT", "OFFSET", "FETCH", "FOR", "SETTINGS", "FORMAT"]
                    .iter()
                    .any(|word| token_is_keyword_at(self.source, self.tokens, index, word))
            {
                return end;
            }
            if !matches!(
                token.kind,
                SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
            ) {
                end = token.range.end;
            }
        }
        end
    }

    fn select_clause_end(&self, start: usize, depth: usize) -> usize {
        let mut end = start;
        for (index, token) in self.tokens.iter().enumerate() {
            if token.range.start < start {
                continue;
            }
            let token_depth = self.token_depths[index];
            let word =
                |candidate: &str| token_is_keyword_at(self.source, self.tokens, index, candidate);
            if token_depth < depth
                || (token_depth == depth
                    && (word("HAVING")
                        || word("QUALIFY")
                        || word("WINDOW")
                        || word("ORDER")
                            && self
                                .keyword_phrase(
                                    &["ORDER", "BY"],
                                    token.range.start,
                                    self.source.len(),
                                    depth,
                                )
                                .is_some()
                        || word("LIMIT")
                        || word("OFFSET")
                        || word("FETCH")
                        || word("FOR")
                        || word("UNION")
                        || word("INTERSECT")
                        || word("EXCEPT")
                        || token.kind == SqlDocumentTokenKind::Punctuation
                            && token_text(self.source, token) == Some(";")))
            {
                return end;
            }
            if !matches!(
                token.kind,
                SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
            ) {
                end = token.range.end;
            }
        }
        end
    }

    fn collect_limit_clause(&mut self, query: &Query, query_id: u32, limit_clause: &LimitClause) {
        let Some(payload_range) = self.span_range(limit_clause) else {
            return;
        };
        let Some(scope_end) = self.query_scope_end(query) else {
            return;
        };
        let query_depth = self.depth_at(payload_range.start).unwrap_or(0);
        let body_end = self
            .span_range(query.body.as_ref())
            .map_or(0, |range| range.end);
        let limit_keyword = self.keyword_phrase(&["LIMIT"], body_end, scope_end, query_depth);
        let offset_payload = match limit_clause {
            LimitClause::LimitOffset { offset, .. } => {
                offset.as_ref().and_then(|offset| self.span_range(offset))
            }
            LimitClause::OffsetCommaLimit { offset, .. } => self.span_range(offset),
        };
        let limit_expression = match limit_clause {
            LimitClause::LimitOffset { limit, .. } => limit.as_ref(),
            LimitClause::OffsetCommaLimit { limit, .. } => Some(limit),
        };
        let offset_keyword = offset_payload.and_then(|offset_range| {
            self.keyword_phrase(&["OFFSET"], body_end, offset_range.start, query_depth)
        });
        let (limit_value, offset_value) = match limit_clause {
            LimitClause::LimitOffset { limit, offset, .. } => (
                limit.as_ref().and_then(literal_u64),
                offset
                    .as_ref()
                    .and_then(|offset| literal_u64(&offset.value)),
            ),
            LimitClause::OffsetCommaLimit { offset, limit } => {
                (literal_u64(limit), literal_u64(offset))
            }
        };
        let clause_start = limit_keyword
            .map(|range| range.start)
            .or_else(|| offset_keyword.map(|range| range.start))
            .unwrap_or(payload_range.start);
        let limit_id = self.add_node_with_data(
            SqlDocumentNodeKind::LimitClause,
            Some(SqlSourceRange {
                start: clause_start,
                end: payload_range.end,
            }),
            query_id,
            Some(SqlDocumentNodeData::QueryClauseValue { value: limit_value }),
        );
        if let Some(limit_range) =
            limit_expression.and_then(|expression| self.span_range(expression))
        {
            self.reparent_direct_children(
                query_id,
                limit_id,
                limit_range,
                &expression_node_kinds(),
            );
        }
        if let Some(offset_range) = offset_payload {
            let offset_start = offset_keyword
                .map(|range| range.start)
                .unwrap_or(offset_range.start);
            let offset_id = self.add_node_with_data(
                SqlDocumentNodeKind::OffsetClause,
                Some(SqlSourceRange {
                    start: offset_start,
                    end: offset_range.end,
                }),
                limit_id,
                Some(SqlDocumentNodeData::QueryClauseValue {
                    value: offset_value,
                }),
            );
            self.reparent_direct_children(
                query_id,
                offset_id,
                offset_range,
                &expression_node_kinds(),
            );
        }
    }

    fn collect_select_clauses(&mut self, select: &Select, select_id: u32) {
        let Some(select_depth) = self.select_depth(select) else {
            return;
        };
        self.nodes[select_id as usize].semantic = Some(SqlDocumentNodeData::Select {
            distinct: matches!(select.distinct, Some(sqlparser::ast::Distinct::Distinct)),
            distinct_on: matches!(select.distinct, Some(sqlparser::ast::Distinct::On(_))),
        });
        let select_keyword =
            SqlSourceRange::from_span(self.source, self.source_map, select.select_token.0.span);
        let projection_ranges =
            projection_item_ranges(self.source, self.source_map, self.tokens, select);
        if let (Some(select_keyword), Some(projection_ranges)) =
            (select_keyword, projection_ranges.as_ref())
        {
            let list_range = SqlSourceRange {
                start: select_keyword.start,
                end: projection_ranges
                    .last()
                    .map_or(select_keyword.end, |range| range.end),
            };
            let list_id =
                self.add_node(SqlDocumentNodeKind::SelectList, Some(list_range), select_id);
            self.reparent_direct_children(
                select_id,
                list_id,
                list_range,
                &[SqlDocumentNodeKind::SelectItem],
            );
        }

        let from_range = self.collect_from_clause(select, select_id, select_depth);
        let projection_end = projection_ranges
            .as_ref()
            .and_then(|ranges| ranges.last().map(|range| range.end))
            .or_else(|| select_keyword.map(|range| range.end));
        let from_end = from_range.map(|range| range.end).or(projection_end);

        if let Some(selection) = &select.selection {
            if let Some(predicate_range) = self.expression_node_range(select_id, selection) {
                if let Some(keyword) = self.keyword_phrase(
                    &["WHERE"],
                    from_end.unwrap_or(0),
                    predicate_range.start,
                    select_depth,
                ) {
                    let range = SqlSourceRange {
                        start: keyword.start,
                        end: predicate_range.end,
                    };
                    let clause_id =
                        self.add_node(SqlDocumentNodeKind::WhereClause, Some(range), select_id);
                    self.reparent_direct_children(
                        select_id,
                        clause_id,
                        predicate_range,
                        &expression_node_kinds(),
                    );
                }
            }
        }

        let mut previous_clause_end = select
            .selection
            .as_ref()
            .and_then(|expression| {
                self.expression_node_range(select_id, expression)
                    .map(|range| range.end)
            })
            .or(from_end)
            .or(projection_end)
            .unwrap_or(0);
        match &select.group_by {
            GroupByExpr::Expressions(expressions, _) => {
                if let Some(group_range) = self.span_range(&select.group_by) {
                    if let Some(keyword) = self.keyword_phrase(
                        &["GROUP", "BY"],
                        previous_clause_end,
                        self.span_range(select)
                            .map_or(self.source.len(), |range| range.end),
                        select_depth,
                    ) {
                        let range = SqlSourceRange {
                            start: keyword.start,
                            end: self
                                .select_clause_end(keyword.end, select_depth)
                                .max(group_range.end),
                        };
                        let clause_id = self.add_node_with_data(
                            SqlDocumentNodeKind::GroupByClause,
                            Some(range),
                            select_id,
                            self.group_by_data(&select.group_by),
                        );
                        for expression in expressions {
                            if let Some(expression_range) = self.span_range(expression) {
                                let item_id = self.add_node(
                                    SqlDocumentNodeKind::GroupByItem,
                                    Some(expression_range),
                                    clause_id,
                                );
                                self.reparent_direct_children(
                                    select_id,
                                    item_id,
                                    expression_range,
                                    &expression_node_kinds(),
                                );
                            }
                        }
                        previous_clause_end = range.end;
                    }
                }
            }
            GroupByExpr::All(_) => {
                if let Some(keyword) = self.keyword_phrase(
                    &["GROUP", "BY"],
                    previous_clause_end,
                    self.source.len(),
                    select_depth,
                ) {
                    let by_range = self
                        .tokens
                        .iter()
                        .enumerate()
                        .find(|(index, token)| {
                            token.range.start >= keyword.end
                                && self.token_depths[*index] == select_depth
                                && !matches!(
                                    token.kind,
                                    SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
                                )
                        })
                        .and_then(|(index, token)| {
                            token_is_keyword_at(self.source, self.tokens, index, "BY")
                                .then_some(token.range)
                        });
                    let all_range = by_range.and_then(|by| {
                        self.tokens
                            .iter()
                            .enumerate()
                            .find(|(index, token)| {
                                token.range.start >= by.end
                                    && self.token_depths[*index] == select_depth
                                    && !matches!(
                                        token.kind,
                                        SqlDocumentTokenKind::Trivia
                                            | SqlDocumentTokenKind::Comment
                                    )
                            })
                            .and_then(|(index, token)| {
                                token_is_keyword_at(self.source, self.tokens, index, "ALL")
                                    .then_some(token.range)
                            })
                    });
                    if let Some(all_range) = all_range {
                        let range = SqlSourceRange {
                            start: keyword.start,
                            end: all_range.end,
                        };
                        self.add_node_with_data(
                            SqlDocumentNodeKind::GroupByClause,
                            Some(range),
                            select_id,
                            self.group_by_data(&select.group_by),
                        );
                        previous_clause_end = all_range.end;
                    }
                }
            }
        }

        if let Some(having) = &select.having {
            if let Some(predicate_range) = self.expression_node_range(select_id, having) {
                if let Some(keyword) = self.keyword_phrase(
                    &["HAVING"],
                    previous_clause_end,
                    predicate_range.start,
                    select_depth,
                ) {
                    let range = SqlSourceRange {
                        start: keyword.start,
                        end: predicate_range.end,
                    };
                    let clause_id =
                        self.add_node(SqlDocumentNodeKind::HavingClause, Some(range), select_id);
                    self.reparent_direct_children(
                        select_id,
                        clause_id,
                        predicate_range,
                        &expression_node_kinds(),
                    );
                }
            }
        }
    }

    fn collect_from_clause(
        &mut self,
        select: &Select,
        select_id: u32,
        depth: usize,
    ) -> Option<SqlSourceRange> {
        let first_relation_range = table_factor_source_range(
            self.source,
            self.source_map,
            self.tokens,
            &select.from.first()?.relation,
        )?;
        let from_keyword = self.last_keyword_before("FROM", first_relation_range.start, depth)?;
        let mut last_from_end = first_relation_range.end;
        let mut relation_trees = Vec::with_capacity(select.from.len());
        for table_with_joins in &select.from {
            let Some(base_range) = table_factor_source_range(
                self.source,
                self.source_map,
                self.tokens,
                &table_with_joins.relation,
            ) else {
                continue;
            };
            let mut previous_end = base_range.end;
            let mut joins = Vec::with_capacity(table_with_joins.joins.len());
            for join in &table_with_joins.joins {
                let Some(relation_range) = table_factor_source_range(
                    self.source,
                    self.source_map,
                    self.tokens,
                    &join.relation,
                ) else {
                    continue;
                };
                let Some(join_start) =
                    self.join_header_start(previous_end, relation_range.start, depth)
                else {
                    continue;
                };
                let join_end = self
                    .span_range(join)
                    .map(|range| SqlSourceRange {
                        start: join_start,
                        end: range.end.max(relation_range.end),
                    })
                    .and_then(|range| close_unbalanced_parentheses(self.source, self.tokens, range))
                    .map_or(relation_range.end, |range| {
                        range.end.max(relation_range.end)
                    });
                let join_range = SqlSourceRange {
                    start: join_start,
                    end: join_end,
                };
                previous_end = join_range.end;
                joins.push(join_range);
            }
            let tree_range = SqlSourceRange {
                start: base_range.start,
                end: previous_end,
            };
            last_from_end = last_from_end.max(tree_range.end);
            relation_trees.push((tree_range, joins));
        }
        let from_range = SqlSourceRange {
            start: from_keyword.start,
            end: last_from_end,
        };
        let from_id = self.add_node(SqlDocumentNodeKind::FromClause, Some(from_range), select_id);
        for (tree_range, joins) in relation_trees {
            let tree_id =
                self.add_node(SqlDocumentNodeKind::RelationTree, Some(tree_range), from_id);
            self.reparent_direct_children(
                select_id,
                tree_id,
                tree_range,
                &[SqlDocumentNodeKind::TableFactor],
            );
            for join_range in joins {
                let join_id = self.add_node(SqlDocumentNodeKind::Join, Some(join_range), tree_id);
                self.reparent_direct_children(
                    tree_id,
                    join_id,
                    join_range,
                    &[
                        SqlDocumentNodeKind::TableFactor,
                        SqlDocumentNodeKind::BinaryExpression,
                        SqlDocumentNodeKind::PredicateExpression,
                        SqlDocumentNodeKind::IdentifierExpression,
                        SqlDocumentNodeKind::CompoundIdentifierExpression,
                    ],
                );
                self.reparent_direct_children(
                    select_id,
                    join_id,
                    join_range,
                    &expression_node_kinds(),
                );
            }
        }
        Some(from_range)
    }

    fn join_header_start(
        &self,
        previous_end: usize,
        relation_start: usize,
        depth: usize,
    ) -> Option<usize> {
        let join_index = self
            .tokens
            .iter()
            .enumerate()
            .filter(|(index, token)| {
                token.range.start >= previous_end
                    && token.range.start < relation_start
                    && self.token_depths[*index] == depth
                    && (token_is_keyword_at(self.source, self.tokens, *index, "JOIN")
                        || token_is_keyword_at(self.source, self.tokens, *index, "APPLY"))
            })
            .map(|(index, _)| index)
            .next_back()?;
        let mut start_index = join_index;
        while let Some(previous_index) = previous_nontrivia_token(self.tokens, start_index) {
            let token = &self.tokens[previous_index];
            if self.token_depths[previous_index] != depth
                || !token_is_join_header_word(self.source, token)
            {
                break;
            }
            start_index = previous_index;
        }
        Some(self.tokens[start_index].range.start)
    }
}

impl Visitor for QueryStructureCollector<'_> {
    type Break = Infallible;

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<Self::Break> {
        if let Some(query_id) = self.query_ids.get(self.query_cursor).copied() {
            self.query_cursor += 1;
            self.collect_with_clause(query, query_id);
            self.collect_query_clauses(query, query_id);
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_select(&mut self, select: &Select) -> ControlFlow<Self::Break> {
        if let Some(select_id) = self.select_ids.get(self.select_cursor).copied() {
            self.select_cursor += 1;
            self.collect_select_clauses(select, select_id);
        }
        ControlFlow::Continue(())
    }
}

fn expression_node_kinds() -> [SqlDocumentNodeKind; 16] {
    [
        SqlDocumentNodeKind::IdentifierExpression,
        SqlDocumentNodeKind::CompoundIdentifierExpression,
        SqlDocumentNodeKind::LiteralExpression,
        SqlDocumentNodeKind::BinaryExpression,
        SqlDocumentNodeKind::UnaryExpression,
        SqlDocumentNodeKind::FunctionExpression,
        SqlDocumentNodeKind::CaseExpression,
        SqlDocumentNodeKind::CastExpression,
        SqlDocumentNodeKind::SubqueryExpression,
        SqlDocumentNodeKind::PredicateExpression,
        SqlDocumentNodeKind::NestedExpression,
        SqlDocumentNodeKind::OtherExpression,
        SqlDocumentNodeKind::Value,
        SqlDocumentNodeKind::SelectItem,
        SqlDocumentNodeKind::Relation,
        SqlDocumentNodeKind::TableFactor,
    ]
}

fn literal_u64(expression: &Expr) -> Option<u64> {
    let Expr::Value(ValueWithSpan {
        value: Value::Number(value, _),
        ..
    }) = expression
    else {
        return None;
    };
    value.parse().ok()
}

fn token_depths(source: &str, tokens: &[SqlDocumentToken]) -> Vec<usize> {
    let mut depth = 0usize;
    tokens
        .iter()
        .map(|token| {
            let text = token_text(source, token);
            if token.kind == SqlDocumentTokenKind::Punctuation && text == Some(")") {
                depth = depth.saturating_sub(1);
            }
            let token_depth = depth;
            if token.kind == SqlDocumentTokenKind::Punctuation && text == Some("(") {
                depth = depth.saturating_add(1);
            }
            token_depth
        })
        .collect()
}

fn token_text<'a>(source: &'a str, token: &SqlDocumentToken) -> Option<&'a str> {
    source.get(token.range.start..token.range.end)
}

/// sqlparser's span for parenthesized relation factors starts at the inner
/// query/relation, omitting the opening parenthesis. Expand those spans so
/// structural edits cover the complete factor, including comments and trivia.
fn table_factor_source_range(
    source: &str,
    source_map: &SqlSourceMap,
    tokens: &[SqlDocumentToken],
    table_factor: &TableFactor,
) -> Option<SqlSourceRange> {
    let mut range = SqlSourceRange::from_span(source, source_map, table_factor.span())?;
    let inner_span = match table_factor {
        TableFactor::Derived { subquery, .. } => subquery.span(),
        TableFactor::NestedJoin {
            table_with_joins, ..
        } => table_with_joins.span(),
        _ => return Some(range),
    };
    let inner_range = SqlSourceRange::from_span(source, source_map, inner_span)?;
    if range.start > inner_range.start || range.end < inner_range.end {
        return None;
    }

    let mut open_parens = Vec::new();
    for token in tokens
        .iter()
        .take_while(|token| token.range.end <= inner_range.start)
    {
        if token.kind != SqlDocumentTokenKind::Punctuation {
            continue;
        }
        match token_text(source, token)? {
            "(" => open_parens.push(token.range),
            ")" => {
                open_parens.pop()?;
            }
            _ => {}
        }
    }
    let opening = *open_parens.last()?;
    if tokens.iter().any(|token| {
        token.range.start >= opening.end
            && token.range.end <= inner_range.start
            && !matches!(
                token.kind,
                SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
            )
    }) {
        return None;
    }

    let mut depth = 1usize;
    let mut closing_found = false;
    for token in tokens
        .iter()
        .filter(|token| token.range.start >= opening.end)
    {
        if token.kind != SqlDocumentTokenKind::Punctuation {
            continue;
        }
        match token_text(source, token)? {
            "(" => depth = depth.checked_add(1)?,
            ")" => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    closing_found = token.range.start >= inner_range.end;
                    break;
                }
            }
            _ => {}
        }
    }
    if !closing_found {
        return None;
    }
    range.start = opening.start;
    Some(range)
}

/// Some sqlparser clause spans stop at the final identifier inside a wrapper,
/// such as `USING (id)`. Extend a syntax range through any still-open wrappers
/// so relation-tree edits are placed after the complete JOIN subtree.
fn close_unbalanced_parentheses(
    source: &str,
    tokens: &[SqlDocumentToken],
    mut range: SqlSourceRange,
) -> Option<SqlSourceRange> {
    let mut depth = 0usize;
    for token in tokens
        .iter()
        .filter(|token| range.start <= token.range.start && token.range.end <= range.end)
    {
        if token.kind != SqlDocumentTokenKind::Punctuation {
            continue;
        }
        match token_text(source, token)? {
            "(" => depth = depth.checked_add(1)?,
            ")" => depth = depth.checked_sub(1)?,
            _ => {}
        }
    }
    if depth == 0 {
        return Some(range);
    }

    for token in tokens.iter().filter(|token| token.range.start >= range.end) {
        if token.kind != SqlDocumentTokenKind::Punctuation {
            if !matches!(
                token.kind,
                SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
            ) {
                return None;
            }
            continue;
        }
        match token_text(source, token)? {
            "(" => depth = depth.checked_add(1)?,
            ")" => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    range.end = token.range.end;
                    return Some(range);
                }
            }
            _ => return None,
        }
    }
    None
}

fn previous_nontrivia_token(tokens: &[SqlDocumentToken], before: usize) -> Option<usize> {
    (0..before).rev().find(|index| {
        !matches!(
            tokens[*index].kind,
            SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
        )
    })
}

fn skip_trivia_tokens_reverse(tokens: &[SqlDocumentToken], before: usize) -> Option<usize> {
    previous_nontrivia_token(tokens, before)
}

fn matching_close_paren(
    source: &str,
    tokens: &[SqlDocumentToken],
    open_index: usize,
) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open_index) {
        if token.kind != SqlDocumentTokenKind::Punctuation {
            continue;
        }
        match token_text(source, token)? {
            "(" => depth = depth.checked_add(1)?,
            ")" => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn normalized_sql_token_text(source: &str) -> Option<Vec<String>> {
    let dialect = GenericDialect {};
    let tokens = Tokenizer::new(&dialect, source)
        .with_unescape(false)
        .tokenize_with_location()
        .ok()?;
    Some(
        tokens
            .into_iter()
            .filter_map(|token| match token.token {
                Token::EOF | Token::Whitespace(_) => None,
                Token::Word(word) if word.quote_style.is_none() => {
                    Some(format!("word:{}", word.value.to_ascii_lowercase()))
                }
                token => Some(token.to_string()),
            })
            .collect(),
    )
}

fn top_level_cast_as_token(
    source: &str,
    tokens: &[SqlDocumentToken],
    open_index: usize,
    close_index: usize,
) -> Option<usize> {
    let mut depth = 1usize;
    for index in open_index + 1..close_index {
        let token = tokens.get(index)?;
        let text = token_text(source, token)?;
        if depth == 1 && token_is_keyword_at(source, tokens, index, "AS") {
            return Some(index);
        }
        if token.kind == SqlDocumentTokenKind::Punctuation {
            match text {
                "(" => depth = depth.checked_add(1)?,
                ")" => depth = depth.checked_sub(1)?,
                _ => {}
            }
        }
    }
    None
}

/// Find the smallest cast wrapper that strictly contains the parser-derived
/// operand span and has the same target type. sqlparser's `Expr::Cast` span
/// currently covers only the operand; recursive child resolution lets nested
/// wrappers be matched without relying on visitor order.
fn find_cast_wrapper_range(
    source: &str,
    tokens: &[SqlDocumentToken],
    wrapper_keyword: &str,
    target_type: &str,
    operand_range: Option<SqlSourceRange>,
) -> Option<SqlSourceRange> {
    let expected_type = normalized_sql_token_text(target_type)?;
    let operand_range = operand_range?;
    let mut best_range = None;

    for keyword_index in 0..tokens.len() {
        let keyword = tokens.get(keyword_index)?;
        if keyword.kind != SqlDocumentTokenKind::Word
            || !token_text(source, keyword)?.eq_ignore_ascii_case(wrapper_keyword)
        {
            continue;
        }
        let open_index = skip_trivia_tokens(tokens, keyword_index + 1);
        if token_text(source, tokens.get(open_index)?)? != "(" {
            continue;
        }
        let close_index = matching_close_paren(source, tokens, open_index)?;
        let close = tokens.get(close_index)?;
        let range = SqlSourceRange {
            start: keyword.range.start,
            end: close.range.end,
        };
        if operand_range.start < range.start
            || operand_range.end > range.end
            || (operand_range.start == range.start && operand_range.end == range.end)
        {
            continue;
        }
        let Some(as_index) = top_level_cast_as_token(source, tokens, open_index, close_index)
        else {
            continue;
        };
        let type_start = skip_trivia_tokens(tokens, as_index + 1);
        let Some(type_start_token) = tokens.get(type_start) else {
            continue;
        };
        let Some(type_source) = source.get(type_start_token.range.start..close.range.start) else {
            continue;
        };
        if normalized_sql_token_text(type_source).as_ref() == Some(&expected_type) {
            let is_smaller = best_range
                .is_none_or(|best: SqlSourceRange| range.end - range.start < best.end - best.start);
            if is_smaller {
                best_range = Some(range);
            }
        }
    }
    best_range
}

fn top_level_comma_tokens(
    source: &str,
    tokens: &[SqlDocumentToken],
    open_index: usize,
    close_index: usize,
) -> Option<Vec<usize>> {
    let mut depth = 1usize;
    let mut commas = Vec::new();
    for index in open_index + 1..close_index {
        let token = tokens.get(index)?;
        if token.kind == SqlDocumentTokenKind::Punctuation {
            match token_text(source, token)? {
                "(" => depth = depth.checked_add(1)?,
                ")" => depth = depth.checked_sub(1)?,
                "," if depth == 1 => commas.push(index),
                _ => {}
            }
        }
    }
    Some(commas)
}

fn argument_source_range(
    tokens: &[SqlDocumentToken],
    start_index: usize,
    end_index: usize,
) -> Option<SqlSourceRange> {
    let first = tokens.get(skip_trivia_tokens(tokens, start_index))?;
    let last_index = (start_index..end_index).rev().find(|index| {
        tokens.get(*index).is_some_and(|token| {
            !matches!(
                token.kind,
                SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
            )
        })
    })?;
    let last = tokens.get(last_index)?;
    Some(SqlSourceRange {
        start: first.range.start,
        end: last.range.end,
    })
}

fn call_argument_matches_type(
    source: &str,
    tokens: &[SqlDocumentToken],
    start_index: usize,
    end_index: usize,
    target_type: &str,
) -> bool {
    let Some(range) = argument_source_range(tokens, start_index, end_index) else {
        return false;
    };
    source
        .get(range.start..range.end)
        .and_then(normalized_sql_token_text)
        .as_ref()
        == normalized_sql_token_text(target_type).as_ref()
}

/// Find the smallest typed CONVERT call whose value operand and target type
/// match the parser node. sqlparser's expression span omits the call wrapper,
/// so an unverified operand-only span must never be exposed as the cast range.
fn find_convert_wrapper_range(
    source: &str,
    tokens: &[SqlDocumentToken],
    wrapper_keyword: &str,
    target_type: &str,
    target_before_value: bool,
    operand_range: Option<SqlSourceRange>,
) -> Option<SqlSourceRange> {
    let operand_range = operand_range?;
    let mut best = None;
    for keyword_index in 0..tokens.len() {
        let keyword = tokens.get(keyword_index)?;
        if keyword.kind != SqlDocumentTokenKind::Word
            || !token_text(source, keyword)?.eq_ignore_ascii_case(wrapper_keyword)
        {
            continue;
        }
        let open_index = skip_trivia_tokens(tokens, keyword_index + 1);
        if token_text(source, tokens.get(open_index)?)? != "(" {
            continue;
        }
        let close_index = matching_close_paren(source, tokens, open_index)?;
        let commas = top_level_comma_tokens(source, tokens, open_index, close_index)?;
        if commas.len() != 1 {
            continue;
        }
        let comma_index = commas[0];
        let (value_start, value_end, type_start, type_end) = if target_before_value {
            (comma_index + 1, close_index, open_index + 1, comma_index)
        } else {
            (open_index + 1, comma_index, comma_index + 1, close_index)
        };
        if !call_argument_matches_type(source, tokens, type_start, type_end, target_type) {
            continue;
        }
        let Some(value_range) = argument_source_range(tokens, value_start, value_end) else {
            continue;
        };
        if operand_range.start < value_range.start || operand_range.end > value_range.end {
            continue;
        }
        let close = tokens.get(close_index)?;
        let range = SqlSourceRange {
            start: keyword.range.start,
            end: close.range.end,
        };
        let is_smaller = best.is_none_or(|best_range: SqlSourceRange| {
            range.end - range.start < best_range.end - best_range.start
        });
        if is_smaller {
            best = Some(range);
        }
    }
    best
}

/// Locate only the target type inside a parser-classified cast wrapper. The
/// edit replaces this range rather than regenerating the expression, which
/// retains whether the author wrote CAST, TRY_CAST, SAFE_CAST, `::`, CONVERT,
/// or TRY_CONVERT.
fn find_double_colon_range(
    source: &str,
    tokens: &[SqlDocumentToken],
    operand_range: Option<SqlSourceRange>,
    target_type: &str,
) -> Option<SqlSourceRange> {
    let operand_range = operand_range?;
    let operand_end_index = tokens
        .iter()
        .position(|token| token.range.end == operand_range.end)?;
    let operator_index = skip_trivia_tokens(tokens, operand_end_index + 1);
    if token_text(source, tokens.get(operator_index)?)? != "::" {
        return None;
    }
    let type_start = skip_trivia_tokens(tokens, operator_index + 1);
    let expected_type = normalized_sql_token_text(target_type)?;
    if expected_type.is_empty() {
        return None;
    }

    let mut actual_type = Vec::new();
    let mut last_type_index = None;
    for (index, token) in tokens.iter().enumerate().skip(type_start) {
        if matches!(
            token.kind,
            SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
        ) {
            continue;
        }
        actual_type.extend(normalized_sql_token_text(token_text(source, token)?)?);
        last_type_index = Some(index);
        if actual_type.len() >= expected_type.len() {
            break;
        }
    }
    if actual_type != expected_type {
        return None;
    }
    let end = tokens.get(last_type_index?)?.range.end;
    Some(SqlSourceRange {
        start: operand_range.start,
        end,
    })
}

fn has_comment_within_range(tokens: &[SqlDocumentToken], range: SqlSourceRange) -> bool {
    tokens.iter().any(|token| {
        token.kind == SqlDocumentTokenKind::Comment
            && token.range.start >= range.start
            && token.range.end <= range.end
    })
}

fn token_is_join_header_word(source: &str, token: &SqlDocumentToken) -> bool {
    token.kind == SqlDocumentTokenKind::Word
        && token_text(source, token).is_some_and(|text| {
            matches!(
                text.to_ascii_uppercase().as_str(),
                "JOIN"
                    | "INNER"
                    | "LEFT"
                    | "RIGHT"
                    | "FULL"
                    | "OUTER"
                    | "CROSS"
                    | "NATURAL"
                    | "APPLY"
                    | "GLOBAL"
                    | "ASOF"
                    | "SEMI"
                    | "ANTI"
                    | "STRAIGHT_JOIN"
            )
        })
}

struct NodeCollector<'a> {
    source: &'a str,
    source_map: &'a SqlSourceMap,
    tokens: &'a [SqlDocumentToken],
    nodes: Vec<SqlDocumentNode>,
    parents: Vec<u32>,
}

impl NodeCollector<'_> {
    fn push<T: Spanned>(&mut self, kind: SqlDocumentNodeKind, node: &T) {
        self.push_with_data(kind, node, None);
    }

    fn push_with_data<T: Spanned>(
        &mut self,
        kind: SqlDocumentNodeKind,
        node: &T,
        semantic: Option<SqlDocumentNodeData>,
    ) {
        self.push_with_range(
            kind,
            SqlSourceRange::from_span(self.source, self.source_map, node.span()),
            semantic,
        );
    }

    fn push_with_range(
        &mut self,
        kind: SqlDocumentNodeKind,
        range: Option<SqlSourceRange>,
        semantic: Option<SqlDocumentNodeData>,
    ) {
        let id = self.nodes.len() as u32;
        self.nodes.push(SqlDocumentNode {
            id,
            parent: self.parents.last().copied(),
            kind,
            range,
            semantic,
        });
        self.parents.push(id);
    }

    fn expression_range(&mut self, expression: &Expr) -> Option<SqlSourceRange> {
        let (range, is_cast_node) = match expression {
            Expr::Cast {
                kind: CastKind::DoubleColon,
                expr,
                data_type,
                ..
            } => (
                find_double_colon_range(
                    self.source,
                    self.tokens,
                    self.expression_range(expr),
                    &data_type.to_string(),
                ),
                true,
            ),
            Expr::Cast {
                kind,
                expr,
                data_type,
                ..
            } => {
                let wrapper_keyword = match kind {
                    CastKind::Cast => "CAST",
                    CastKind::TryCast => "TRY_CAST",
                    CastKind::SafeCast => "SAFE_CAST",
                    CastKind::DoubleColon => unreachable!("handled above"),
                };
                let range = find_cast_wrapper_range(
                    self.source,
                    self.tokens,
                    wrapper_keyword,
                    &data_type.to_string(),
                    self.expression_range(expr),
                );
                (range, true)
            }
            Expr::Convert {
                is_try,
                expr,
                data_type,
                charset,
                target_before_value,
                styles,
            } => {
                let range = if charset.is_none() && styles.is_empty() {
                    data_type.as_ref().and_then(|data_type| {
                        let wrapper_keyword = if *is_try { "TRY_CONVERT" } else { "CONVERT" };
                        find_convert_wrapper_range(
                            self.source,
                            self.tokens,
                            wrapper_keyword,
                            &data_type.to_string(),
                            *target_before_value,
                            self.expression_range(expr),
                        )
                    })
                } else {
                    None
                };
                (range, true)
            }
            _ => (
                SqlSourceRange::from_span(self.source, self.source_map, expression.span()),
                false,
            ),
        };

        let range = match (expression, range) {
            (
                Expr::IsNull(_)
                | Expr::IsNotNull(_)
                | Expr::IsTrue(_)
                | Expr::IsNotTrue(_)
                | Expr::IsFalse(_)
                | Expr::IsNotFalse(_)
                | Expr::IsUnknown(_)
                | Expr::IsNotUnknown(_),
                Some(range),
            ) => extend_is_postfix_range(self.source, self.tokens, range).or(Some(range)),
            (Expr::IsDistinctFrom(_, right) | Expr::IsNotDistinctFrom(_, right), Some(range)) => {
                Some(SqlSourceRange {
                    start: range.start,
                    end: self
                        .expression_range(right)
                        .map_or(range.end, |right_range| range.end.max(right_range.end)),
                })
            }
            (_, range) => range,
        };

        if is_cast_node && range.is_some_and(|range| has_comment_within_range(self.tokens, range)) {
            // Typed node edits replace an entire expression range. With the
            // current edit protocol there is no target-only edit operation, so
            // comments inside a cast wrapper make that range unsafe to replace.
            return None;
        }
        range
    }

    fn pop(&mut self) {
        self.parents.pop();
    }

    fn push_select_items(&mut self, select: &Select) {
        let Some(parent) = self.parents.last().copied() else {
            return;
        };

        let ranges = projection_item_ranges(self.source, self.source_map, self.tokens, select);
        for (index, item) in select.projection.iter().enumerate() {
            let id = self.nodes.len() as u32;
            self.nodes.push(SqlDocumentNode {
                id,
                parent: Some(parent),
                kind: SqlDocumentNodeKind::SelectItem,
                range: ranges
                    .as_ref()
                    .and_then(|ranges| ranges.get(index).copied())
                    .or_else(|| {
                        SqlSourceRange::from_span(self.source, self.source_map, item.span())
                    }),
                semantic: match item {
                    SelectItem::ExprWithAlias { alias, .. } => {
                        Some(SqlDocumentNodeData::ProjectionAlias {
                            alias: SqlDocumentIdentifier {
                                value: alias.value.clone(),
                                quote_style: alias.quote_style,
                            },
                        })
                    }
                    _ => wildcard_node_data(item),
                },
            });
        }
    }

    fn attach_projection_children(&mut self) {
        let projections = self
            .nodes
            .iter()
            .filter(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .filter_map(|node| {
                node.range
                    .and_then(|range| node.parent.map(|parent| (node.id, parent, range)))
            })
            .collect::<Vec<_>>();

        for node in &mut self.nodes {
            let Some(parent) = node.parent else {
                continue;
            };
            if node.kind == SqlDocumentNodeKind::SelectItem {
                continue;
            }
            let Some(range) = node.range else {
                continue;
            };

            if let Some((item_id, _, _)) = projections
                .iter()
                .filter(|(_, select_id, item_range)| {
                    *select_id == parent
                        && item_range.start <= range.start
                        && range.end <= item_range.end
                })
                .min_by_key(|(_, _, item_range)| item_range.end - item_range.start)
            {
                node.parent = Some(*item_id);
            }
        }
    }
}

fn wildcard_node_data(item: &SelectItem) -> Option<SqlDocumentNodeData> {
    let wildcard = match item {
        SelectItem::Wildcard(options) => Some((Vec::new(), false, options)),
        SelectItem::QualifiedWildcard(
            sqlparser::ast::SelectItemQualifiedWildcardKind::ObjectName(name),
            options,
        ) => Some((object_name_parts(name), false, options)),
        SelectItem::QualifiedWildcard(
            sqlparser::ast::SelectItemQualifiedWildcardKind::Expr(_),
            options,
        ) => Some((Vec::new(), true, options)),
        _ => None,
    }?;
    let (qualifier, qualifier_is_expression, options) = wildcard;
    Some(SqlDocumentNodeData::Wildcard {
        qualifier,
        qualifier_is_expression,
        has_options: options.opt_ilike.is_some()
            || options.opt_exclude.is_some()
            || options.opt_except.is_some()
            || options.opt_replace.is_some()
            || options.opt_rename.is_some()
            || options.opt_alias.is_some(),
    })
}

fn projection_item_ranges(
    source: &str,
    source_map: &SqlSourceMap,
    tokens: &[SqlDocumentToken],
    select: &Select,
) -> Option<Vec<SqlSourceRange>> {
    let select_range = SqlSourceRange::from_span(source, source_map, select.select_token.0.span)?;
    let mut token_index = tokens
        .iter()
        .position(|token| token.range.start >= select_range.end)?;
    token_index = skip_select_prefix(source, tokens, token_index, select);

    let mut ranges = Vec::with_capacity(select.projection.len());
    let mut item_start = None;
    let mut item_end = None;
    let mut depth = 0_usize;
    let mut wildcard_except_seen = false;

    for token in tokens.iter().skip(token_index) {
        let text = source.get(token.range.start..token.range.end)?;
        if depth == 0 {
            if token.kind == SqlDocumentTokenKind::Punctuation && text == "," {
                push_projection_range(&mut ranges, &mut item_start, &mut item_end);
                wildcard_except_seen = false;
                continue;
            }
            if token.kind == SqlDocumentTokenKind::Punctuation && matches!(text, ")" | ";") {
                break;
            }
            if token.kind == SqlDocumentTokenKind::Word && is_projection_clause_keyword(text) {
                if text.eq_ignore_ascii_case("EXCEPT")
                    && !wildcard_except_seen
                    && projection_item_has_wildcard_except(select, ranges.len())
                {
                    wildcard_except_seen = true;
                } else {
                    break;
                }
            }
        }

        if !matches!(
            token.kind,
            SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
        ) {
            item_start.get_or_insert(token.range.start);
            item_end = Some(token.range.end);
        }

        if token.kind == SqlDocumentTokenKind::Punctuation {
            match text {
                "(" | "[" | "{" => depth = depth.checked_add(1)?,
                ")" | "]" | "}" => depth = depth.checked_sub(1)?,
                _ => {}
            }
        }
    }
    push_projection_range(&mut ranges, &mut item_start, &mut item_end);

    (ranges.len() == select.projection.len()).then_some(ranges)
}

fn projection_item_has_wildcard_except(select: &Select, index: usize) -> bool {
    match select.projection.get(index) {
        Some(SelectItem::Wildcard(options) | SelectItem::QualifiedWildcard(_, options)) => {
            options.opt_except.is_some()
        }
        _ => false,
    }
}

fn push_projection_range(
    ranges: &mut Vec<SqlSourceRange>,
    start: &mut Option<usize>,
    end: &mut Option<usize>,
) {
    if let (Some(start), Some(end)) = (start.take(), end.take()) {
        ranges.push(SqlSourceRange { start, end });
    }
}

fn skip_select_prefix(
    source: &str,
    tokens: &[SqlDocumentToken],
    mut index: usize,
    select: &Select,
) -> usize {
    loop {
        index = skip_trivia_tokens(tokens, index);
        let Some(token) = tokens.get(index) else {
            return index;
        };
        let Some(text) = source.get(token.range.start..token.range.end) else {
            return index;
        };

        if select.top.is_some() && token_is_keyword(token, text, "TOP") {
            index = skip_trivia_tokens(tokens, index + 1);
            if tokens
                .get(index)
                .is_some_and(|token| source.get(token.range.start..token.range.end) == Some("("))
            {
                index = skip_balanced_tokens(source, tokens, index).unwrap_or(index);
            } else if tokens.get(index).is_some() {
                index += 1;
            }
            index = skip_trivia_tokens(tokens, index);
            if token_is_keyword_at(source, tokens, index, "PERCENT") {
                index += 1;
            }
            index = skip_trivia_tokens(tokens, index);
            if token_is_keyword_at(source, tokens, index, "WITH")
                && token_is_keyword_at(
                    source,
                    tokens,
                    skip_trivia_tokens(tokens, index + 1),
                    "TIES",
                )
            {
                index = skip_trivia_tokens(tokens, index + 1) + 1;
            }
            continue;
        }

        if select.select_modifiers.is_some() && is_select_modifier_keyword(text) {
            index += 1;
            continue;
        }

        if let Some(distinct) = &select.distinct {
            if matches!(distinct, Distinct::All | Distinct::Distinct)
                && token.kind == SqlDocumentTokenKind::Word
                && matches!(
                    text.to_ascii_uppercase().as_str(),
                    "ALL" | "DISTINCT" | "DISTINCTROW"
                )
            {
                index += 1;
                continue;
            }
            if matches!(distinct, Distinct::On(_)) && token_is_keyword(token, text, "DISTINCT") {
                index = skip_trivia_tokens(tokens, index + 1);
                if token_is_keyword_at(source, tokens, index, "ON") {
                    index = skip_trivia_tokens(tokens, index + 1);
                    if tokens.get(index).is_some_and(|token| {
                        source.get(token.range.start..token.range.end) == Some("(")
                    }) {
                        index = skip_balanced_tokens(source, tokens, index).unwrap_or(index);
                    }
                }
                continue;
            }
        }

        return index;
    }
}

fn skip_trivia_tokens(tokens: &[SqlDocumentToken], mut index: usize) -> usize {
    while tokens.get(index).is_some_and(|token| {
        matches!(
            token.kind,
            SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
        )
    }) {
        index += 1;
    }
    index
}

fn extend_is_postfix_range(
    source: &str,
    tokens: &[SqlDocumentToken],
    mut range: SqlSourceRange,
) -> Option<SqlSourceRange> {
    let mut index = tokens
        .iter()
        .position(|token| token.range.start >= range.end)?;
    index = skip_trivia_tokens(tokens, index);
    if !token_is_keyword_at(source, tokens, index, "IS") {
        return None;
    }
    index = skip_trivia_tokens(tokens, index + 1);
    if token_is_keyword_at(source, tokens, index, "NOT") {
        index = skip_trivia_tokens(tokens, index + 1);
    }
    if ["NULL", "TRUE", "FALSE", "UNKNOWN"]
        .iter()
        .any(|keyword| token_is_keyword_at(source, tokens, index, keyword))
    {
        range.end = tokens.get(index)?.range.end;
        Some(range)
    } else {
        None
    }
}

fn skip_balanced_tokens(source: &str, tokens: &[SqlDocumentToken], start: usize) -> Option<usize> {
    let mut depth = 0_usize;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        let text = source.get(token.range.start..token.range.end)?;
        if token.kind == SqlDocumentTokenKind::Punctuation {
            match text {
                "(" => depth = depth.checked_add(1)?,
                ")" => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        return Some(index + 1);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

#[derive(Clone, Copy)]
struct AggregateFilterClauseTokens {
    where_keyword: usize,
}

fn aggregate_filter_clause_tokens(
    source: &str,
    tokens: &[SqlDocumentToken],
    function_range: SqlSourceRange,
    filter_range: SqlSourceRange,
) -> Option<AggregateFilterClauseTokens> {
    let arguments_open = tokens.iter().position(|token| {
        token.range.start >= function_range.start
            && token.range.end <= function_range.end
            && token.kind == SqlDocumentTokenKind::Punctuation
            && token_text(source, token) == Some("(")
    })?;
    let arguments_close = matching_close_paren(source, tokens, arguments_open)?;
    let filter = skip_trivia_tokens(tokens, arguments_close + 1);
    if !token_is_keyword_at(source, tokens, filter, "FILTER") {
        return None;
    }
    let filter_open = skip_trivia_tokens(tokens, filter + 1);
    if token_text(source, tokens.get(filter_open)?)? != "(" {
        return None;
    }
    let where_keyword = skip_trivia_tokens(tokens, filter_open + 1);
    if !token_is_keyword_at(source, tokens, where_keyword, "WHERE") {
        return None;
    }
    let close_paren = matching_close_paren(source, tokens, filter_open)?;
    let condition_start = tokens.get(where_keyword)?.range.end;
    let clause_end = tokens.get(close_paren)?.range.start;
    (condition_start <= filter_range.start
        && filter_range.start <= filter_range.end
        && filter_range.end <= clause_end)
        .then_some(AggregateFilterClauseTokens { where_keyword })
}

fn token_is_keyword(token: &SqlDocumentToken, text: &str, keyword: &str) -> bool {
    token.kind == SqlDocumentTokenKind::Word && text.eq_ignore_ascii_case(keyword)
}

fn token_is_keyword_at(
    source: &str,
    tokens: &[SqlDocumentToken],
    index: usize,
    keyword: &str,
) -> bool {
    tokens.get(index).is_some_and(|token| {
        source
            .get(token.range.start..token.range.end)
            .is_some_and(|text| token_is_keyword(token, text, keyword))
    })
}

fn is_select_modifier_keyword(text: &str) -> bool {
    matches!(
        text.to_ascii_uppercase().as_str(),
        "HIGH_PRIORITY"
            | "STRAIGHT_JOIN"
            | "SQL_SMALL_RESULT"
            | "SQL_BIG_RESULT"
            | "SQL_BUFFER_RESULT"
            | "SQL_NO_CACHE"
            | "SQL_CALC_FOUND_ROWS"
    )
}

fn is_projection_clause_keyword(text: &str) -> bool {
    matches!(
        text.to_ascii_uppercase().as_str(),
        "FROM"
            | "WHERE"
            | "PREWHERE"
            | "GROUP"
            | "HAVING"
            | "WINDOW"
            | "QUALIFY"
            | "ORDER"
            | "LIMIT"
            | "OFFSET"
            | "FETCH"
            | "FOR"
            | "INTO"
            | "CONNECT"
            | "CLUSTER"
            | "DISTRIBUTE"
            | "SORT"
            | "SETTINGS"
            | "FORMAT"
            | "EMIT"
            | "UNION"
            | "EXCEPT"
            | "INTERSECT"
    )
}

impl Visitor for NodeCollector<'_> {
    type Break = Infallible;

    fn pre_visit_statement(&mut self, statement: &Statement) -> ControlFlow<Self::Break> {
        let kind = if matches!(statement, Statement::Query(_)) {
            SqlDocumentNodeKind::QueryStatement
        } else {
            SqlDocumentNodeKind::OtherStatement
        };
        self.push(kind, statement);
        ControlFlow::Continue(())
    }

    fn post_visit_statement(&mut self, _statement: &Statement) -> ControlFlow<Self::Break> {
        self.pop();
        ControlFlow::Continue(())
    }

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<Self::Break> {
        self.push(SqlDocumentNodeKind::Query, query);
        ControlFlow::Continue(())
    }

    fn post_visit_query(&mut self, _query: &Query) -> ControlFlow<Self::Break> {
        self.pop();
        ControlFlow::Continue(())
    }

    fn pre_visit_select(&mut self, select: &Select) -> ControlFlow<Self::Break> {
        self.push(SqlDocumentNodeKind::Select, select);
        self.push_select_items(select);
        ControlFlow::Continue(())
    }

    fn post_visit_select(&mut self, _select: &Select) -> ControlFlow<Self::Break> {
        self.pop();
        ControlFlow::Continue(())
    }

    fn pre_visit_relation(
        &mut self,
        relation: &sqlparser::ast::ObjectName,
    ) -> ControlFlow<Self::Break> {
        self.push_with_data(
            SqlDocumentNodeKind::Relation,
            relation,
            Some(SqlDocumentNodeData::Relation {
                name: object_name_parts(relation),
            }),
        );
        ControlFlow::Continue(())
    }

    fn post_visit_relation(
        &mut self,
        _relation: &sqlparser::ast::ObjectName,
    ) -> ControlFlow<Self::Break> {
        self.pop();
        ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(&mut self, table_factor: &TableFactor) -> ControlFlow<Self::Break> {
        self.push_with_range(
            SqlDocumentNodeKind::TableFactor,
            table_factor_source_range(self.source, self.source_map, self.tokens, table_factor),
            Some(table_factor_data(table_factor)),
        );
        ControlFlow::Continue(())
    }

    fn post_visit_table_factor(&mut self, _table_factor: &TableFactor) -> ControlFlow<Self::Break> {
        self.pop();
        ControlFlow::Continue(())
    }

    fn pre_visit_expr(&mut self, expression: &Expr) -> ControlFlow<Self::Break> {
        let range = self.expression_range(expression);
        let semantic = expression_data(expression, self.source, self.source_map);
        self.push_with_range(expression_kind(expression), range, semantic);
        ControlFlow::Continue(())
    }

    fn post_visit_expr(&mut self, _expression: &Expr) -> ControlFlow<Self::Break> {
        if let Expr::Function(function) = _expression {
            let is_admitted_aggregate = function
                .name
                .0
                .last()
                .and_then(|part| match part {
                    ObjectNamePart::Identifier(identifier) => {
                        crate::sql_functions::user_sql_aggregate(&identifier.value)
                    }
                    ObjectNamePart::Function(_) => None,
                })
                .is_some();
            if is_admitted_aggregate {
                if let (Some(filter), Some(function_id)) =
                    (function.filter.as_deref(), self.parents.last().copied())
                {
                    let function_range = self.nodes[function_id as usize].range;
                    let filter_range = self.expression_range(filter);
                    if let (Some(function_range), Some(filter_range)) =
                        (function_range, filter_range)
                    {
                        let filter_clause = aggregate_filter_clause_tokens(
                            self.source,
                            self.tokens,
                            function_range,
                            filter_range,
                        );
                        let filter_node_id = self
                            .nodes
                            .iter()
                            .find(|node| {
                                node.parent == Some(function_id)
                                    && node.kind == expression_kind(filter)
                                    && node
                                        .range
                                        .is_some_and(|range| range.start == filter_range.start)
                            })
                            .map(|node| node.id);
                        if let (Some(filter_clause), Some(filter_node_id)) =
                            (filter_clause, filter_node_id)
                        {
                            let clause_id = self.nodes.len() as u32;
                            self.nodes.push(SqlDocumentNode {
                                id: clause_id,
                                parent: Some(function_id),
                                kind: SqlDocumentNodeKind::WhereClause,
                                range: Some(SqlSourceRange {
                                    start: self.tokens[filter_clause.where_keyword].range.start,
                                    end: filter_range.end,
                                }),
                                semantic: None,
                            });
                            self.nodes[filter_node_id as usize].parent = Some(clause_id);
                        }
                    }
                }
            }
        }
        if let Some(node_id) = self.parents.last().copied() {
            let child_end = self
                .nodes
                .iter()
                .filter(|node| node.parent == Some(node_id))
                .filter_map(|node| node.range.map(|range| range.end))
                .max();
            if let (Some(child_end), Some(range)) = (
                child_end,
                self.nodes
                    .get_mut(node_id as usize)
                    .and_then(|node| node.range.as_mut()),
            ) {
                range.end = range.end.max(child_end);
            }
        }
        self.pop();
        ControlFlow::Continue(())
    }

    fn pre_visit_value(&mut self, value: &ValueWithSpan) -> ControlFlow<Self::Break> {
        self.push_with_data(
            SqlDocumentNodeKind::Value,
            value,
            Some(SqlDocumentNodeData::Literal {
                literal_kind: literal_kind(&value.value),
                raw: SqlSourceRange::from_span(self.source, self.source_map, value.span)
                    .and_then(|range| self.source.get(range.start..range.end))
                    .map(str::to_owned),
            }),
        );
        ControlFlow::Continue(())
    }

    fn post_visit_value(&mut self, _value: &ValueWithSpan) -> ControlFlow<Self::Break> {
        self.pop();
        ControlFlow::Continue(())
    }
}

fn identifier_data(identifier: &Ident) -> SqlDocumentIdentifier {
    SqlDocumentIdentifier {
        value: identifier.value.clone(),
        quote_style: identifier.quote_style,
    }
}

fn object_name_parts(name: &ObjectName) -> Vec<SqlDocumentIdentifier> {
    name.0
        .iter()
        .map(|part| match part {
            ObjectNamePart::Identifier(identifier) => identifier_data(identifier),
            ObjectNamePart::Function(function) => SqlDocumentIdentifier {
                value: function.to_string(),
                quote_style: None,
            },
        })
        .collect()
}

fn table_factor_data(table_factor: &TableFactor) -> SqlDocumentNodeData {
    let (factor_kind, alias) = match table_factor {
        TableFactor::Table { alias, .. } => ("table", alias.as_ref()),
        TableFactor::Derived { alias, .. } => ("derived", alias.as_ref()),
        TableFactor::TableFunction { alias, .. } => ("table_function", alias.as_ref()),
        TableFactor::Function { alias, .. } => ("function", alias.as_ref()),
        TableFactor::UNNEST { alias, .. } => ("unnest", alias.as_ref()),
        TableFactor::JsonTable { alias, .. } => ("json_table", alias.as_ref()),
        TableFactor::OpenJsonTable { alias, .. } => ("open_json_table", alias.as_ref()),
        TableFactor::NestedJoin { alias, .. } => ("nested_join", alias.as_ref()),
        TableFactor::Pivot { alias, .. } => ("pivot", alias.as_ref()),
        TableFactor::Unpivot { alias, .. } => ("unpivot", alias.as_ref()),
        TableFactor::UnpivotExpr { .. } => ("unpivot_expression", None),
        TableFactor::MatchRecognize { alias, .. } => ("match_recognize", alias.as_ref()),
        TableFactor::XmlTable { alias, .. } => ("xml_table", alias.as_ref()),
        TableFactor::SemanticView { alias, .. } => ("semantic_view", alias.as_ref()),
    };
    let alias_columns = alias.map(|alias| {
        (
            alias
                .columns
                .iter()
                .map(|column| identifier_data(&column.name))
                .collect::<Vec<_>>(),
            alias
                .columns
                .iter()
                .any(|column| column.data_type.is_some()),
        )
    });
    SqlDocumentNodeData::TableFactor {
        factor_kind: factor_kind.to_owned(),
        alias: alias.map(|alias| identifier_data(&alias.name)),
        column_aliases: alias_columns
            .as_ref()
            .map(|(columns, _)| columns.clone())
            .unwrap_or_default(),
        has_column_alias_types: alias_columns.is_some_and(|(_, has_types)| has_types),
    }
}

fn literal_kind(value: &Value) -> SqlDocumentLiteralKind {
    match value {
        Value::Number(..) => SqlDocumentLiteralKind::Number,
        Value::SingleQuotedString(..)
        | Value::DollarQuotedString(..)
        | Value::TripleSingleQuotedString(..)
        | Value::TripleDoubleQuotedString(..)
        | Value::EscapedStringLiteral(..)
        | Value::UnicodeStringLiteral(..)
        | Value::SingleQuotedByteStringLiteral(..)
        | Value::DoubleQuotedByteStringLiteral(..)
        | Value::TripleSingleQuotedByteStringLiteral(..)
        | Value::TripleDoubleQuotedByteStringLiteral(..)
        | Value::SingleQuotedRawStringLiteral(..)
        | Value::DoubleQuotedRawStringLiteral(..)
        | Value::TripleSingleQuotedRawStringLiteral(..)
        | Value::TripleDoubleQuotedRawStringLiteral(..)
        | Value::NationalStringLiteral(..)
        | Value::QuoteDelimitedStringLiteral(..)
        | Value::NationalQuoteDelimitedStringLiteral(..)
        | Value::HexStringLiteral(..)
        | Value::DoubleQuotedString(..) => SqlDocumentLiteralKind::String,
        Value::Boolean(..) => SqlDocumentLiteralKind::Boolean,
        Value::Null => SqlDocumentLiteralKind::Null,
        Value::Placeholder(..) => SqlDocumentLiteralKind::Placeholder,
    }
}

fn function_argument_is_wildcard(argument: &FunctionArg) -> bool {
    let expression = match argument {
        FunctionArg::Named { arg, .. } | FunctionArg::ExprNamed { arg, .. } => arg,
        FunctionArg::Unnamed(arg) => arg,
    };
    match expression {
        FunctionArgExpr::Wildcard
        | FunctionArgExpr::QualifiedWildcard(_)
        | FunctionArgExpr::WildcardWithOptions(_) => true,
        FunctionArgExpr::Expr(Expr::Wildcard(_) | Expr::QualifiedWildcard(_, _)) => true,
        FunctionArgExpr::Expr(_) => false,
    }
}

fn expression_data(
    expression: &Expr,
    source: &str,
    source_map: &SqlSourceMap,
) -> Option<SqlDocumentNodeData> {
    let raw = || {
        SqlSourceRange::from_span(source, source_map, expression.span())
            .and_then(|range| source.get(range.start..range.end))
            .map(str::to_owned)
    };
    let predicate = |operator: &str, negated| {
        Some(SqlDocumentNodeData::Predicate {
            operator: operator.to_owned(),
            negated,
        })
    };
    let slice_bound = |bound: &Expr| -> Option<(String, SqlSourceRange)> {
        match bound {
            Expr::Value(ValueWithSpan {
                value: Value::Number(..),
                ..
            }) => {}
            _ => return None,
        }
        let range = SqlSourceRange::from_span(source, source_map, bound.span())?;
        let raw = source.get(range.start..range.end)?;
        raw.parse::<i64>().ok()?;
        Some((raw.to_owned(), range))
    };

    match expression {
        Expr::CompoundFieldAccess { access_chain, .. } => {
            let Some(AccessExpr::Subscript(Subscript::Slice {
                lower_bound: Some(lower),
                upper_bound: Some(upper),
                stride: None,
            })) = access_chain.last()
            else {
                return Some(SqlDocumentNodeData::UnmodeledExpression {
                    node_kind: expression_kind(expression),
                    reason: "This access chain has no fully bounded terminal slice.".to_owned(),
                });
            };
            let Some((one_based_start, start_range)) = slice_bound(lower) else {
                return Some(SqlDocumentNodeData::UnmodeledExpression {
                    node_kind: expression_kind(expression),
                    reason: "The lower bound is not one signed 64-bit integer literal.".to_owned(),
                });
            };
            let Some((one_based_end, end_range)) = slice_bound(upper) else {
                return Some(SqlDocumentNodeData::UnmodeledExpression {
                    node_kind: expression_kind(expression),
                    reason: "The upper bound is not one signed 64-bit integer literal.".to_owned(),
                });
            };
            Some(SqlDocumentNodeData::ArraySlice {
                one_based_start,
                one_based_end,
                start_range,
                end_range,
            })
        }
        Expr::Array(_) => Some(SqlDocumentNodeData::ArrayLiteral),
        Expr::Identifier(identifier) => Some(SqlDocumentNodeData::Identifier {
            value: identifier.value.clone(),
            quote_style: identifier.quote_style,
        }),
        Expr::CompoundIdentifier(identifiers) => Some(SqlDocumentNodeData::CompoundIdentifier {
            parts: identifiers.iter().map(identifier_data).collect(),
        }),
        Expr::Value(value) => Some(SqlDocumentNodeData::Literal {
            literal_kind: literal_kind(&value.value),
            raw: raw(),
        }),
        Expr::TypedString(typed_string) => Some(SqlDocumentNodeData::TypedLiteral {
            data_type: typed_string.data_type.to_string(),
            raw: raw(),
        }),
        Expr::Interval(_) => Some(SqlDocumentNodeData::TypedLiteral {
            data_type: "INTERVAL".to_owned(),
            raw: raw(),
        }),
        Expr::BinaryOp { op, .. } => Some(SqlDocumentNodeData::BinaryOperator {
            operator: op.to_string(),
        }),
        Expr::UnaryOp { op, .. } => Some(SqlDocumentNodeData::UnaryOperator {
            operator: op.to_string(),
        }),
        Expr::Function(function) => {
            let (argument_count, distinct) = match &function.args {
                FunctionArguments::None => (Some(0), false),
                FunctionArguments::Subquery(_) => (None, false),
                FunctionArguments::List(arguments) => (
                    Some(arguments.args.len()),
                    arguments.duplicate_treatment == Some(DuplicateTreatment::Distinct),
                ),
            };
            let has_filter = function.filter.is_some();
            let has_over = function.over.is_some();
            let has_argument_clauses = matches!(
                &function.args,
                FunctionArguments::List(arguments) if !arguments.clauses.is_empty()
            );
            let has_wildcard_argument = matches!(
                &function.args,
                FunctionArguments::List(arguments)
                    if arguments.args.iter().any(function_argument_is_wildcard)
            );
            let aggregate = function
                .name
                .0
                .last()
                .and_then(|part| match part {
                    ObjectNamePart::Identifier(identifier) => {
                        crate::sql_functions::user_sql_aggregate(&identifier.value)
                    }
                    ObjectNamePart::Function(_) => None,
                });
            let can_replace_argument = matches!(
                &function.args,
                FunctionArguments::List(arguments)
                    if !has_filter
                        && !has_over
                        && arguments.clauses.is_empty()
                        && !arguments.args.is_empty()
                        && aggregate.is_some()
            );
            let can_add_argument = matches!(
                &function.args,
                FunctionArguments::List(arguments)
                    if !has_filter
                        && !has_over
                        && arguments.clauses.is_empty()
                        && !has_wildcard_argument
                        && !matches!(arguments.duplicate_treatment, Some(DuplicateTreatment::Distinct))
                        && aggregate.is_some_and(|descriptor| descriptor.arity.accepts(arguments.args.len() + 1))
            );
            let can_remove_argument = matches!(
                &function.args,
                FunctionArguments::List(arguments)
                    if !has_filter
                        && !has_over
                        && arguments.clauses.is_empty()
                        && !arguments.args.is_empty()
                        && aggregate.is_some_and(|descriptor| descriptor.arity.accepts(arguments.args.len() - 1))
            );
            Some(SqlDocumentNodeData::Function {
                name: object_name_parts(&function.name),
                argument_count,
                distinct,
                has_filter,
                has_over,
                has_argument_clauses,
                can_replace_argument,
                can_add_argument,
                can_remove_argument,
            })
        }
        Expr::Case {
            operand,
            conditions,
            else_result,
            ..
        } => Some(SqlDocumentNodeData::Case {
            has_operand: operand.is_some(),
            when_count: conditions.len(),
            has_else: else_result.is_some(),
        }),
        Expr::Cast {
            kind, data_type, ..
        } => Some(SqlDocumentNodeData::Cast {
            cast_kind: match kind {
                CastKind::Cast => "cast",
                CastKind::TryCast => "try_cast",
                CastKind::SafeCast => "safe_cast",
                CastKind::DoubleColon => "double_colon",
            }
            .to_owned(),
            target_type: Some(data_type.to_string()),
        }),
        Expr::Convert {
            is_try, data_type, ..
        } => Some(SqlDocumentNodeData::Cast {
            cast_kind: if *is_try { "try_convert" } else { "convert" }.to_owned(),
            target_type: data_type.as_ref().map(ToString::to_string),
        }),
        Expr::InList { negated, .. } => predicate("in", *negated),
        Expr::InSubquery { negated, .. } => predicate("in_subquery", *negated),
        Expr::InUnnest { negated, .. } => predicate("in_unnest", *negated),
        Expr::Between { negated, .. } => predicate("between", *negated),
        Expr::Like { negated, .. } => predicate("like", *negated),
        Expr::ILike { negated, .. } => predicate("ilike", *negated),
        Expr::SimilarTo { negated, .. } => predicate("similar_to", *negated),
        Expr::RLike { negated, .. } => predicate("rlike", *negated),
        Expr::IsFalse(_) => predicate("is_false", false),
        Expr::IsNotFalse(_) => predicate("is_false", true),
        Expr::IsTrue(_) => predicate("is_true", false),
        Expr::IsNotTrue(_) => predicate("is_true", true),
        Expr::IsNull(_) => predicate("is_null", false),
        Expr::IsNotNull(_) => predicate("is_null", true),
        Expr::IsUnknown(_) => predicate("is_unknown", false),
        Expr::IsNotUnknown(_) => predicate("is_unknown", true),
        Expr::IsDistinctFrom(_, _) => predicate("is_distinct_from", false),
        Expr::IsNotDistinctFrom(_, _) => predicate("is_distinct_from", true),
        _ => Some(SqlDocumentNodeData::UnmodeledExpression {
            node_kind: expression_kind(expression),
            reason: "No dedicated semantic attributes are defined for this parser expression variant; its exact source remains available from the document range and tokens.".to_owned(),
        }),
    }
}

fn expression_kind(expression: &Expr) -> SqlDocumentNodeKind {
    match expression {
        Expr::Identifier(_) => SqlDocumentNodeKind::IdentifierExpression,
        Expr::CompoundIdentifier(_) => SqlDocumentNodeKind::CompoundIdentifierExpression,
        Expr::Value(_) | Expr::TypedString(_) => SqlDocumentNodeKind::LiteralExpression,
        Expr::BinaryOp { .. } => SqlDocumentNodeKind::BinaryExpression,
        Expr::UnaryOp { .. } => SqlDocumentNodeKind::UnaryExpression,
        Expr::Function(_) => SqlDocumentNodeKind::FunctionExpression,
        Expr::Case { .. } => SqlDocumentNodeKind::CaseExpression,
        Expr::Cast { .. } | Expr::Convert { .. } => SqlDocumentNodeKind::CastExpression,
        Expr::Subquery(_) | Expr::Exists { .. } | Expr::InSubquery { .. } => {
            SqlDocumentNodeKind::SubqueryExpression
        }
        Expr::IsFalse(_)
        | Expr::IsNotFalse(_)
        | Expr::IsTrue(_)
        | Expr::IsNotTrue(_)
        | Expr::IsNull(_)
        | Expr::IsNotNull(_)
        | Expr::IsUnknown(_)
        | Expr::IsNotUnknown(_)
        | Expr::IsDistinctFrom(_, _)
        | Expr::IsNotDistinctFrom(_, _)
        | Expr::InList { .. }
        | Expr::InUnnest { .. }
        | Expr::Between { .. }
        | Expr::Like { .. }
        | Expr::ILike { .. }
        | Expr::SimilarTo { .. }
        | Expr::RLike { .. } => SqlDocumentNodeKind::PredicateExpression,
        Expr::Nested(_) => SqlDocumentNodeKind::NestedExpression,
        _ => SqlDocumentNodeKind::OtherExpression,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_template_normalization_uses_tokens_and_preserves_source_ranges() {
        let source = "SELECT café, $first, {{ second }}, $first, {{second}}";
        let normalized = normalize_sql_template(source);

        assert_eq!(
            normalized.sql,
            "SELECT café, $first, $second, $first, $second"
        );
        assert_eq!(normalized.parameters, ["first", "second"]);
        assert_eq!(normalized.replacements.len(), 2);
        let first = &normalized.replacements[0];
        assert_eq!(&source[first[0]..first[1]], "{{ second }}");
        assert_eq!(&normalized.sql[first[0]..first[0] + first[2]], "$second");
        let second = &normalized.replacements[1];
        let second_normalized_start = second[0] - (first[1] - first[0] - first[2]);
        assert_eq!(&source[second[0]..second[1]], "{{second}}");
        assert_eq!(
            &normalized.sql[second_normalized_start..second_normalized_start + second[2]],
            "$second"
        );
        assert_eq!(normalized.error, None);
    }

    #[test]
    fn sql_template_normalization_keeps_all_tokenized_literals_identifiers_and_comments_opaque() {
        let source = "SELECT '{{single}}', $$ {{dollar}} $$, '''{{triple_single}}''', \"{{identifier}}\", `{{backtick}}` -- {{line}}\n/* {{block}} */ FROM \"G\" WHERE id = {{actual}}";
        let normalized = normalize_sql_template(source);

        assert_eq!(
            normalized.sql,
            "SELECT '{{single}}', $$ {{dollar}} $$, '''{{triple_single}}''', \"{{identifier}}\", `{{backtick}}` -- {{line}}\n/* {{block}} */ FROM \"G\" WHERE id = $actual"
        );
        assert_eq!(normalized.parameters, ["actual"]);
    }

    #[test]
    fn sql_template_normalization_keeps_syntax_invalid_drafts_for_parser_diagnostics() {
        let source = "SELECT {{minimum}} +";
        let normalized = normalize_sql_template(source);
        assert_eq!(normalized.sql, "SELECT $minimum +");
        assert_eq!(normalized.parameters, ["minimum"]);
        assert_eq!(
            parse_sql_document(source).status,
            SqlDocumentStatus::InvalidSql
        );

        let unterminated_literal = "SELECT '{{minimum}}";
        let normalized = normalize_sql_template(unterminated_literal);
        assert_eq!(normalized.sql, unterminated_literal);
        assert!(normalized.parameters.is_empty());
    }

    #[test]
    fn sql_template_normalization_keeps_partial_references_and_marker_errors() {
        let partial =
            normalize_sql_template("SELECT $one, {{bad-name}}, {{two}}, {{also-bad}}, $tail");
        assert_eq!(
            partial.sql,
            "SELECT $one, {{bad-name}}, $two, {{also-bad}}, $tail"
        );
        assert_eq!(partial.parameters, ["one", "two", "tail"]);
        let invalid = partial
            .error
            .expect("the first malformed marker is reported");
        assert_eq!(invalid.code(), "invalid_parameter_name");
        assert_eq!(invalid.range(), SqlSourceRange { start: 13, end: 25 });
        assert_eq!(
            invalid.to_string(),
            "SQL template variable name must match [A-Za-z_][A-Za-z0-9_]*"
        );

        let missing = normalize_sql_template("SELECT {{minimum")
            .error
            .expect("an unclosed marker is reported");
        assert_eq!(missing.code(), "missing_closing_braces");
        assert_eq!(
            missing.to_string(),
            "SQL template variable is missing closing braces"
        );
        assert_eq!(missing.range(), SqlSourceRange { start: 7, end: 16 });
    }

    #[test]
    fn sql_template_normalization_preserves_native_behavior_above_document_limit() {
        let source = format!("SELECT {{{{value}}}}{}", " ".repeat(MAX_SQL_DOCUMENT_BYTES));
        let normalized = normalize_sql_template(&source);
        assert!(source.len() > MAX_SQL_DOCUMENT_BYTES);
        assert!(normalized.sql.starts_with("SELECT $value"));
        assert_eq!(normalized.parameters, ["value"]);
    }

    #[test]
    fn sql_template_normalization_handles_maximum_unclosed_and_terminal_close_drafts_linearly() {
        let prefix = "SELECT $one ";
        let source = format!(
            "{prefix}{}",
            "{{".repeat((MAX_SQL_DOCUMENT_BYTES - prefix.len()) / 2)
        );
        assert_eq!(source.len(), MAX_SQL_DOCUMENT_BYTES);

        let started = std::time::Instant::now();
        let normalized = normalize_sql_template(&source);
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        assert_eq!(normalized.sql, source);
        assert_eq!(normalized.parameters, ["one"]);
        assert!(normalized.replacements.is_empty());
        assert_eq!(
            normalized.error,
            Some(SqlTemplateNormalizationError::MissingClosingBraces {
                range: SqlSourceRange {
                    start: prefix.len(),
                    end: source.len(),
                },
            })
        );

        let opening_pairs = (MAX_SQL_DOCUMENT_BYTES - prefix.len() - 2) / 4;
        let trailing_whitespace = MAX_SQL_DOCUMENT_BYTES - prefix.len() - opening_pairs * 2 - 2;
        let mut source = format!("{prefix}{}", "{{".repeat(opening_pairs));
        source.push_str(&" ".repeat(trailing_whitespace));
        source.push_str("}}");
        assert_eq!(source.len(), MAX_SQL_DOCUMENT_BYTES);

        let started = std::time::Instant::now();
        let normalized = normalize_sql_template(&source);
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        assert_eq!(normalized.sql, source);
        assert_eq!(normalized.parameters, ["one"]);
        assert!(normalized.replacements.is_empty());
        assert_eq!(
            normalized.error,
            Some(SqlTemplateNormalizationError::InvalidParameterName {
                range: SqlSourceRange {
                    start: prefix.len(),
                    end: source.len(),
                },
            })
        );
    }

    #[test]
    fn shared_parser_parity_cases_are_parsed_by_the_sql_document() {
        let fixtures: serde_json::Value = serde_json::from_str(include_str!(
            "../../../shared/sql/sql-document-parser-parity.json"
        ))
        .expect("the shared SQL parser parity fixture is valid JSON");
        for case in fixtures["cases"].as_array().expect("fixture cases") {
            let id = case["id"].as_str().expect("case id");
            let sql = case["sql"].as_str().expect("case SQL");
            let document = parse_sql_document(sql);
            assert_eq!(
                document.status,
                SqlDocumentStatus::Parsed,
                "SQL Document parser did not accept parity case {id}: {:?}",
                document.diagnostics
            );
            assert_eq!(document.to_sql(), sql, "source changed for case {id}");
        }
    }
    use sqlparser::ast::Statement;

    fn edit(source: &str, expected_text: &str, replacement: &str) -> SqlDocumentEdit {
        let start = source
            .find(expected_text)
            .expect("test source should contain expected text");
        SqlDocumentEdit {
            range: SqlSourceRange {
                start,
                end: start + expected_text.len(),
            },
            expected_text: expected_text.to_owned(),
            replacement: replacement.to_owned(),
        }
    }

    fn transaction(source: &str, edits: Vec<SqlDocumentEdit>) -> SqlDocumentEditTransaction {
        SqlDocumentEditTransaction {
            expected_source: source.to_owned(),
            edits,
        }
    }

    fn semantic_transaction(
        source: &str,
        edits: Vec<SqlDocumentSemanticEdit>,
    ) -> SqlDocumentSemanticEditTransaction {
        SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits,
        }
    }

    #[test]
    fn preserves_source_tokens_comments_and_quoted_identifiers() {
        let source = "-- title\nSELECT \"MiX\", '$day' AS literal, $day /* tail */ FROM \"G\";";
        let document = parse_sql_document(source);

        assert_eq!(document.status, SqlDocumentStatus::Parsed);
        assert_eq!(document.to_sql(), source);
        let token_texts: Vec<_> = document
            .tokens
            .iter()
            .map(|token| document.source_slice(token.range).unwrap())
            .collect();
        assert!(token_texts.iter().any(|text| text.starts_with("-- title")));
        assert!(token_texts.contains(&"\"MiX\""));
        assert!(token_texts.contains(&"'$day'"));
        assert!(token_texts.contains(&"$day"));
        assert!(token_texts
            .iter()
            .any(|text| text.starts_with("/* tail */")));
        assert_eq!(
            document
                .tokens
                .iter()
                .find(|token| document.source_slice(token.range) == Some("$day"))
                .map(|token| token.kind),
            Some(SqlDocumentTokenKind::Placeholder)
        );
    }

    #[test]
    fn document_parser_accepts_the_filter_tree_depth_exposed_by_the_gui() {
        let predicate = "\"_ugoite_id\" = $entry_id";
        let mut expression = predicate.to_owned();
        for depth in 0..32 {
            let operator = if depth % 2 == 0 { "AND" } else { "OR" };
            expression = format!("({predicate} {operator} ({expression}))");
        }
        let source = format!("SELECT \"_ugoite_id\" FROM \"G\" WHERE {expression}");

        let document = parse_sql_document(&source);

        assert_eq!(document.status, SqlDocumentStatus::Parsed);
        assert_eq!(document.to_sql(), source);
    }

    #[test]
    fn semantic_initialize_select_preserves_empty_draft_trivia_and_has_guarded_undo() {
        for source in ["", "  ", "-- 日本語", "/* keep */\n"] {
            for relation in [
                None,
                Some(SqlDocumentIdentifier {
                    value: "Travel \"Costs\"".to_owned(),
                    quote_style: Some('"'),
                }),
            ] {
                let result = apply_sql_document_semantic_edit_transaction(
                    source,
                    &semantic_transaction(
                        source,
                        vec![SqlDocumentSemanticEdit::InitializeSelect {
                            relation: relation.clone(),
                        }],
                    ),
                )
                .unwrap();
                assert_eq!(result.document.status, SqlDocumentStatus::Parsed);
                assert!(result.document.source.starts_with(source));
                assert!(result.document.source.contains(if relation.is_some() {
                    r#"SELECT * FROM "Travel ""Costs""""#
                } else {
                    "SELECT 1"
                }));
                assert_eq!(
                    apply_sql_document_edit_transaction(&result.document.source, &result.undo)
                        .unwrap()
                        .document
                        .source,
                    source
                );
            }
        }
    }

    #[test]
    fn semantic_initialize_select_rejects_nonempty_invalid_stale_and_mixed_edits() {
        let initialize = SqlDocumentSemanticEdit::InitializeSelect { relation: None };
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                "SELECT 1",
                &semantic_transaction("SELECT 1", vec![initialize.clone()])
            ),
            Err(SqlDocumentEditError::InvalidSemanticReplacement)
        );
        for source in ["SELECT (", "DELETE FROM x", "SELECT 1; SELECT 2"] {
            assert_eq!(
                apply_sql_document_semantic_edit_transaction(
                    source,
                    &semantic_transaction(source, vec![initialize.clone()])
                ),
                Err(SqlDocumentEditError::DocumentNotParsed)
            );
        }
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                " ",
                &semantic_transaction("", vec![initialize.clone()])
            ),
            Err(SqlDocumentEditError::StaleSource)
        );
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                "",
                &semantic_transaction("", vec![initialize.clone(), initialize])
            ),
            Err(SqlDocumentEditError::InvalidSemanticReplacement)
        );
        let mut transaction = semantic_transaction(
            "",
            vec![SqlDocumentSemanticEdit::InitializeSelect { relation: None }],
        );
        transaction.contract_version += 1;
        assert_eq!(
            apply_sql_document_semantic_edit_transaction("", &transaction),
            Err(SqlDocumentEditError::ContractVersionMismatch)
        );
    }

    #[test]
    fn semantic_form_picker_replaces_and_adds_a_relation_losslessly() {
        let source =
            "SELECT e.amount /* keep */ FROM \"Expenses\" AS e WHERE e.amount > 5 ORDER BY e.amount";
        let document = parse_sql_document(source);
        let table_factor = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::TableFactor
                    && node.range.and_then(|range| document.source_slice(range))
                        == Some("\"Expenses\" AS e")
            })
            .expect("the selected Form relation has a source node");
        let invoices = SqlDocumentRelationFactor::Table {
            name: vec![SqlDocumentIdentifier {
                value: "Invoices".to_owned(),
                quote_style: Some('"'),
            }],
            alias: Some(SqlDocumentIdentifier {
                value: "e".to_owned(),
                quote_style: None,
            }),
        };
        let replaced = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::ReplaceTableFactor {
                    node_id: table_factor.id,
                    expected_kind: SqlDocumentNodeKind::TableFactor,
                    factor: invoices.clone(),
                }],
            ),
        )
        .unwrap();
        assert_eq!(
            replaced.document.source,
            "SELECT e.amount /* keep */ FROM \"Invoices\" AS e WHERE e.amount > 5 ORDER BY e.amount"
        );
        assert_eq!(
            apply_sql_document_edit_transaction(&replaced.document.source, &replaced.undo)
                .unwrap()
                .document
                .source,
            source
        );

        let source = "SELECT 42 AS answer";
        let document = parse_sql_document(source);
        let select = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Select)
            .expect("the relation-free query has a SELECT node");
        let added = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::AddFromRelation {
                    node_id: select.id,
                    expected_kind: SqlDocumentNodeKind::Select,
                    factor: invoices,
                }],
            ),
        )
        .unwrap();
        assert_eq!(
            added.document.source,
            "SELECT 42 AS answer FROM \"Invoices\" AS e"
        );
        assert_eq!(
            apply_sql_document_edit_transaction(&added.document.source, &added.undo)
                .unwrap()
                .document
                .source,
            source
        );
    }

    #[test]
    fn renames_repeated_parameter_tokens_and_preserves_lookalikes() {
        let source = "-- $day\nSELECT $day + $day AS total, '$day' AS literal, \"$day\" AS identifier, $day2 AS other, $$ $day $$ AS dollar_text FROM \"G\" /* $day */";
        let document = rename_sql_document_parameter(source, "day", "billing_day")
            .expect("valid parameter names");

        assert_eq!(document.status, SqlDocumentStatus::Parsed);
        assert_eq!(
            document.source,
            "-- $day\nSELECT $billing_day + $billing_day AS total, '$day' AS literal, \"$day\" AS identifier, $day2 AS other, $$ $day $$ AS dollar_text FROM \"G\" /* $day */"
        );
        let placeholders = document
            .tokens
            .iter()
            .filter(|token| token.kind == SqlDocumentTokenKind::Placeholder)
            .map(|token| document.source_slice(token.range).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(placeholders, ["$billing_day", "$billing_day", "$day2"]);
    }

    #[test]
    fn parameter_rename_without_a_match_preserves_source_and_diagnostics() {
        let source = "-- keep $other\nSELECT '$other' AS literal, $other FROM \"G\"";
        let document = rename_sql_document_parameter(source, "missing", "replacement")
            .expect("valid parameter names");

        assert_eq!(document.source, source);
        assert_eq!(document, parse_sql_document(source));
    }

    #[test]
    fn parameter_rename_bounds_output_before_allocating() {
        let source = "SELECT $x, $x";
        let new_name = "r".repeat(MAX_SQL_DOCUMENT_BYTES / 2 + 2);

        assert_eq!(
            rename_sql_document_parameter(&source, "x", &new_name),
            Err(SqlDocumentEditError::OutputTooLarge)
        );
    }

    #[test]
    fn parameter_rename_rewrites_many_tokens_in_one_bounded_pass() {
        let source = format!("SELECT {}", "$x,".repeat(128));
        let document = rename_sql_document_parameter(&source, "x", "long_name")
            .expect("replacement remains within the source limit");

        assert_eq!(
            document.source,
            format!("SELECT {}", "$long_name,".repeat(128))
        );
        assert_eq!(
            document.source.len(),
            format!("SELECT {}", "$long_name,".repeat(128)).len()
        );
    }

    #[test]
    fn parameter_rename_rejects_invalid_names() {
        assert_eq!(
            rename_sql_document_parameter("SELECT $old", "bad-name", "new_name"),
            Err(SqlDocumentEditError::InvalidOldParameterName)
        );
        assert_eq!(
            rename_sql_document_parameter("SELECT $old", "old", "$new"),
            Err(SqlDocumentEditError::InvalidNewParameterName)
        );
        let long_name = "a".repeat(129);
        let source = format!("SELECT ${long_name}");
        let renamed = rename_sql_document_parameter(&source, &long_name, "short")
            .expect("the shared SQL name grammar has no 128-byte limit");
        assert_eq!(renamed.source, "SELECT $short");
    }

    #[test]
    fn parameter_rename_rejects_sources_over_complexity_limit() {
        let source = format!(
            "SELECT $old {}",
            "(".repeat(MAX_SQL_DOCUMENT_COMPLEXITY_TOKENS + 1)
        );
        assert!(source.len() < MAX_SQL_DOCUMENT_BYTES);
        assert_eq!(
            rename_sql_document_parameter(&source, "old", "new"),
            Err(SqlDocumentEditError::SourceTooComplex)
        );
    }

    #[test]
    fn parameter_rename_rejects_sources_over_byte_limit() {
        let source = format!("SELECT $old {}", "x".repeat(MAX_SQL_DOCUMENT_BYTES));
        assert_eq!(
            rename_sql_document_parameter(&source, "old", "new"),
            Err(SqlDocumentEditError::SourceTooLarge)
        );
    }

    #[test]
    fn parameter_rename_reparses_invalid_drafts_and_keeps_diagnostics() {
        let source = "SELECT $old +";
        let before = parse_sql_document(source);
        assert_eq!(before.status, SqlDocumentStatus::InvalidSql);
        assert_eq!(
            before.diagnostics.first().map(|diagnostic| diagnostic.code),
            Some(SqlDocumentDiagnosticCode::ParseFailed)
        );

        let after =
            rename_sql_document_parameter(source, "old", "new").expect("valid parameter names");
        assert_eq!(after.source, "SELECT $new +");
        assert_eq!(after.status, SqlDocumentStatus::InvalidSql);
        assert_eq!(
            after.diagnostics.first().map(|diagnostic| diagnostic.code),
            Some(SqlDocumentDiagnosticCode::ParseFailed)
        );
    }

    #[test]
    fn builds_source_ranged_nodes_for_nested_and_expression_syntax() {
        let source = "WITH c AS (SELECT CAST(x AS INT) AS y FROM \"G\") SELECT CASE WHEN y > 0 THEN y ELSE 0 END AS result FROM c WHERE y IN (SELECT z FROM h)";
        let document = parse_sql_document(source);

        assert_eq!(document.status, SqlDocumentStatus::Parsed);
        assert!(document.nodes.iter().all(|node| {
            node.range
                .is_none_or(|range| document.source_slice(range).is_some())
        }));
        let kinds: Vec<_> = document.nodes.iter().map(|node| node.kind).collect();
        assert!(kinds.contains(&SqlDocumentNodeKind::Query));
        assert!(kinds.contains(&SqlDocumentNodeKind::Select));
        assert!(kinds.contains(&SqlDocumentNodeKind::CaseExpression));
        assert!(kinds.contains(&SqlDocumentNodeKind::CastExpression));
        assert!(kinds.contains(&SqlDocumentNodeKind::SubqueryExpression));
        assert!(document
            .nodes
            .iter()
            .any(|node| node.parent.is_some_and(|parent| parent < node.id)));
    }

    #[test]
    fn exposes_query_clauses_relations_ctes_and_set_operands() {
        let source = "WITH c AS (SELECT a FROM g WHERE a > 0) SELECT c.a FROM c JOIN (SELECT a FROM h) h ON c.a = h.a WHERE c.a > 1 GROUP BY c.a HAVING COUNT(*) > 1 UNION SELECT a FROM i ORDER BY a DESC LIMIT 10 OFFSET 2";
        let document = parse_sql_document(source);
        assert_eq!(document.status, SqlDocumentStatus::Parsed);
        assert_eq!(document.to_sql(), source);

        let slice = |kind: SqlDocumentNodeKind, needle: &str| {
            document
                .nodes
                .iter()
                .filter(|node| node.kind == kind)
                .filter_map(|node| node.range.and_then(|range| document.source_slice(range)))
                .find(|text| text.contains(needle))
                .unwrap_or_else(|| panic!("missing {kind:?} node containing {needle:?}"))
        };
        assert!(slice(SqlDocumentNodeKind::WithClause, "WITH c AS").starts_with("WITH c AS"));
        assert!(slice(SqlDocumentNodeKind::CommonTableExpression, "c AS (").contains("WHERE a > 0"));
        assert!(slice(SqlDocumentNodeKind::SetOperation, "UNION").contains("UNION"));
        assert!(slice(SqlDocumentNodeKind::SelectList, "SELECT c.a").starts_with("SELECT c.a"));
        assert!(slice(SqlDocumentNodeKind::FromClause, "FROM c JOIN").starts_with("FROM c JOIN"));
        assert!(slice(SqlDocumentNodeKind::RelationTree, "c JOIN").contains("ON c.a = h.a"));
        assert!(slice(SqlDocumentNodeKind::Join, "JOIN (").contains("ON c.a = h.a"));
        assert!(
            slice(SqlDocumentNodeKind::WhereClause, "WHERE c.a > 1").starts_with("WHERE c.a > 1")
        );
        assert!(
            slice(SqlDocumentNodeKind::GroupByClause, "GROUP BY c.a").starts_with("GROUP BY c.a")
        );
        assert!(
            slice(SqlDocumentNodeKind::HavingClause, "HAVING COUNT").starts_with("HAVING COUNT")
        );
        assert_eq!(
            slice(SqlDocumentNodeKind::OrderByClause, "ORDER BY"),
            "ORDER BY a DESC"
        );
        assert_eq!(
            slice(SqlDocumentNodeKind::LimitClause, "LIMIT"),
            "LIMIT 10 OFFSET 2"
        );
        assert_eq!(
            slice(SqlDocumentNodeKind::OffsetClause, "OFFSET"),
            "OFFSET 2"
        );

        let with = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::WithClause)
            .expect("WITH clause exists");
        let cte = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::CommonTableExpression
                    && node.parent == Some(with.id)
            })
            .expect("CTE belongs to WITH");
        assert!(document
            .nodes
            .iter()
            .any(|node| node.kind == SqlDocumentNodeKind::Query && node.parent == Some(cte.id)));

        let set_operation = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SetOperation)
            .expect("set operation exists");
        for operand_kind in [
            SqlDocumentNodeKind::SetOperationLeft,
            SqlDocumentNodeKind::SetOperationRight,
        ] {
            let operand = document
                .nodes
                .iter()
                .find(|node| node.kind == operand_kind && node.parent == Some(set_operation.id))
                .expect("set operation has both typed operands");
            assert!(document.nodes.iter().any(|node| {
                matches!(
                    node.kind,
                    SqlDocumentNodeKind::Select | SqlDocumentNodeKind::Query
                ) && node.parent == Some(operand.id)
            }));
        }

        let from = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::FromClause
                    && node
                        .range
                        .and_then(|range| document.source_slice(range))
                        .is_some_and(|text| text.starts_with("FROM c JOIN"))
            })
            .expect("outer FROM clause exists");
        let tree = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::RelationTree && node.parent == Some(from.id)
            })
            .expect("FROM owns a relation tree");
        assert!(document.nodes.iter().any(|node| {
            node.kind == SqlDocumentNodeKind::Join && node.parent == Some(tree.id)
        }));
    }

    #[test]
    fn attaches_parser_derived_semantic_attributes_without_rewriting_source() {
        let source = "WITH RECURSIVE named(v) AS (SELECT CAST(a + 2 AS BIGINT) FROM \"schema\".\"G\" AS g) SELECT CASE WHEN v >= 3 THEN v ELSE 0 END AS result FROM named WHERE v IN (1, 2) AND v BETWEEN 0 AND 9 AND label LIKE 'A%' UNION ALL SELECT COUNT(DISTINCT v) FROM other";
        let document = parse_sql_document(source);
        assert_eq!(document.status, SqlDocumentStatus::Parsed);
        assert_eq!(document.to_sql(), source);
        assert_eq!(document.contract_version, SQL_DOCUMENT_CONTRACT_VERSION);
        assert!(document
            .tokens
            .iter()
            .all(|token| document.source_slice(token.range).is_some()));

        let has_semantic = |node_kind, expected: &dyn Fn(&SqlDocumentNodeData) -> bool| {
            document
                .nodes
                .iter()
                .any(|node| node.kind == node_kind && node.semantic.as_ref().is_some_and(expected))
        };
        assert!(has_semantic(
            SqlDocumentNodeKind::WithClause,
            &|data| matches!(
                data,
                SqlDocumentNodeData::WithClause {
                    recursive: true,
                    cte_count: 1
                }
            )
        ));
        assert!(has_semantic(
            SqlDocumentNodeKind::CommonTableExpression,
            &|data| matches!(data, SqlDocumentNodeData::CommonTableExpression {
                name: SqlDocumentIdentifier { value, .. },
                columns,
                materialized: None
            } if value == "named" && columns.len() == 1 && columns[0].value == "v")
        ));
        assert!(has_semantic(
            SqlDocumentNodeKind::SetOperation,
            &|data| matches!(data, SqlDocumentNodeData::SetOperation { operator, quantifier }
                if operator == "UNION" && quantifier == "ALL")
        ));
        assert!(has_semantic(
            SqlDocumentNodeKind::CaseExpression,
            &|data| matches!(
                data,
                SqlDocumentNodeData::Case {
                    has_operand: false,
                    when_count: 1,
                    has_else: true
                }
            )
        ));
        assert!(has_semantic(
            SqlDocumentNodeKind::CastExpression,
            &|data| matches!(data, SqlDocumentNodeData::Cast { cast_kind, target_type }
                if cast_kind == "cast" && target_type.as_deref() == Some("BIGINT"))
        ));
        assert!(document.nodes.iter().any(|node| {
            node.kind == SqlDocumentNodeKind::CastExpression
                && node.range.and_then(|range| document.source_slice(range))
                    == Some("CAST(a + 2 AS BIGINT)")
        }));
        assert!(has_semantic(
            SqlDocumentNodeKind::BinaryExpression,
            &|data| matches!(data, SqlDocumentNodeData::BinaryOperator { operator }
                if operator == "+")
        ));
        assert!(has_semantic(
            SqlDocumentNodeKind::BinaryExpression,
            &|data| matches!(data, SqlDocumentNodeData::BinaryOperator { operator }
                if operator == ">=")
        ));
        for expected_operator in ["in", "between", "like"] {
            assert!(has_semantic(
                SqlDocumentNodeKind::PredicateExpression,
                &|data| matches!(data, SqlDocumentNodeData::Predicate { operator, negated: false }
                    if operator == expected_operator)
            ));
        }
        assert!(has_semantic(
            SqlDocumentNodeKind::FunctionExpression,
            &|data| matches!(data, SqlDocumentNodeData::Function {
                name,
                argument_count: Some(1),
                distinct: true,
                has_filter: false,
                has_over: false,
                has_argument_clauses: false,
                can_replace_argument: true,
                can_add_argument: false,
                can_remove_argument: true,
            } if name.last().is_some_and(|part| part.value.eq_ignore_ascii_case("COUNT")))
        ));
        assert!(has_semantic(
            SqlDocumentNodeKind::Relation,
            &|data| matches!(data, SqlDocumentNodeData::Relation { name }
                if name.len() == 2
                    && name[0].value == "schema"
                    && name[0].quote_style == Some('"')
                    && name[1].value == "G"
                    && name[1].quote_style == Some('"'))
        ));
        assert!(has_semantic(
            SqlDocumentNodeKind::TableFactor,
            &|data| matches!(data, SqlDocumentNodeData::TableFactor { factor_kind, alias, .. }
                if factor_kind == "table"
                    && alias.as_ref().is_some_and(|alias| alias.value == "g"))
        ));
        assert!(has_semantic(
            SqlDocumentNodeKind::LiteralExpression,
            &|data| matches!(data, SqlDocumentNodeData::Literal {
                literal_kind: SqlDocumentLiteralKind::String,
                raw: Some(raw)
            } if raw == "'A%'")
        ));
    }

    #[test]
    fn cast_node_ranges_cover_nested_cast_wrappers() {
        let source = "SELECT CAST(CAST(amount AS BIGINT) AS DECIMAL(10, 2))";
        let document = parse_sql_document(source);
        assert_eq!(document.status, SqlDocumentStatus::Parsed);

        let cast_ranges = document
            .nodes
            .iter()
            .filter(|node| node.kind == SqlDocumentNodeKind::CastExpression)
            .map(|node| {
                node.range
                    .and_then(|range| document.source_slice(range))
                    .expect("CAST wrapper range points into the original source")
            })
            .collect::<Vec<_>>();
        assert_eq!(
            cast_ranges,
            [
                "CAST(CAST(amount AS BIGINT) AS DECIMAL(10, 2))",
                "CAST(amount AS BIGINT)"
            ]
        );
        assert_eq!(document.to_sql(), source);
    }

    #[test]
    fn cast_node_ranges_cover_double_colon_and_convert_wrappers() {
        let source = "SELECT CAST(amount AS BIGINT)::DECIMAL(10, 2), amount::INT::TEXT, CONVERT(amount, BIGINT)";
        let document = parse_sql_document(source);
        assert_eq!(document.status, SqlDocumentStatus::Parsed);

        let cast_ranges = document
            .nodes
            .iter()
            .filter(|node| node.kind == SqlDocumentNodeKind::CastExpression)
            .map(|node| node.range.and_then(|range| document.source_slice(range)))
            .collect::<Vec<_>>();
        assert_eq!(
            cast_ranges,
            [
                Some("CAST(amount AS BIGINT)::DECIMAL(10, 2)"),
                Some("CAST(amount AS BIGINT)"),
                Some("amount::INT::TEXT"),
                Some("amount::INT"),
                Some("CONVERT(amount, BIGINT)"),
            ]
        );
        assert_eq!(document.to_sql(), source);
    }

    #[test]
    fn gives_every_expression_node_a_semantic_view_or_explicit_fallback() {
        let source = "SELECT g.name, COUNT(*) AS row_count, SUM(CAST(g.amount + 1 AS BIGINT)) AS adjusted, CASE WHEN g.name LIKE 'a%' THEN 1 ELSE 0 END AS category, EXTRACT(YEAR FROM DATE '2024-01-01') AS year FROM \"G\" AS g WHERE NOT ((g.amount IN (SELECT amount FROM \"G\")) OR g.amount BETWEEN 0 AND 10) AND g.amount IS NOT NULL GROUP BY g.name HAVING COUNT(*) > 0 ORDER BY SUM(g.amount) DESC LIMIT 10";
        let document = parse_sql_document(source);
        assert_eq!(document.status, SqlDocumentStatus::Parsed);

        let expression_kinds = [
            SqlDocumentNodeKind::IdentifierExpression,
            SqlDocumentNodeKind::CompoundIdentifierExpression,
            SqlDocumentNodeKind::LiteralExpression,
            SqlDocumentNodeKind::BinaryExpression,
            SqlDocumentNodeKind::UnaryExpression,
            SqlDocumentNodeKind::FunctionExpression,
            SqlDocumentNodeKind::CaseExpression,
            SqlDocumentNodeKind::CastExpression,
            SqlDocumentNodeKind::SubqueryExpression,
            SqlDocumentNodeKind::PredicateExpression,
            SqlDocumentNodeKind::NestedExpression,
            SqlDocumentNodeKind::OtherExpression,
            SqlDocumentNodeKind::Value,
        ];
        for kind in expression_kinds {
            let nodes = document
                .nodes
                .iter()
                .filter(|node| node.kind == kind)
                .collect::<Vec<_>>();
            assert!(!nodes.is_empty(), "fixture should exercise {kind:?}");
            for node in nodes {
                let semantic = node.semantic.as_ref().unwrap_or_else(|| {
                    panic!(
                        "{kind:?} has no semantic data; source range: {:?}",
                        node.range
                    )
                });
                if let SqlDocumentNodeData::UnmodeledExpression { node_kind, reason } = semantic {
                    assert_eq!(*node_kind, kind);
                    assert!(!reason.trim().is_empty());
                    assert!(
                        node.range
                            .and_then(|range| document.source_slice(range))
                            .is_some(),
                        "fallback expression must remain source-backed"
                    );
                }
            }
        }
        assert!(document.nodes.iter().any(|node| {
            node.kind == SqlDocumentNodeKind::LiteralExpression
                && matches!(
                    node.semantic.as_ref(),
                    Some(SqlDocumentNodeData::TypedLiteral { data_type, .. })
                        if data_type.eq_ignore_ascii_case("DATE")
                )
        }));
        assert_eq!(document.to_sql(), source);
    }

    #[test]
    fn composite_rollup_and_cube_metadata_preserves_groups_and_marks_them_read_only() {
        for (source, expected_mode, expected_group) in [
            (
                "SELECT a, b, c FROM tasks GROUP BY ROLLUP ((a, b), c)",
                SqlDocumentGroupByMode::Rollup,
                "GROUP BY ROLLUP ((a, b), c)",
            ),
            (
                "SELECT a, b, c FROM tasks GROUP BY CUBE ((a, b), c)",
                SqlDocumentGroupByMode::Cube,
                "GROUP BY CUBE ((a, b), c)",
            ),
        ] {
            let document = parse_sql_document(source);
            assert_eq!(document.status, SqlDocumentStatus::Parsed, "{source}");
            let group = document
                .nodes
                .iter()
                .find(|node| node.kind == SqlDocumentNodeKind::GroupByClause)
                .expect("the parser exposes GROUP BY");
            let Some(SqlDocumentNodeData::GroupBy {
                mode,
                groups,
                editable,
            }) = group.semantic.as_ref()
            else {
                panic!("GROUP BY exposes typed grouping boundaries");
            };
            assert_eq!(*mode, expected_mode);
            assert!(!editable);
            assert_eq!(groups.iter().map(Vec::len).collect::<Vec<_>>(), vec![2, 1]);
            assert_eq!(
                document.source_slice(group.range.unwrap()),
                Some(expected_group)
            );
        }
    }

    #[test]
    fn captures_spans_for_group_by_all_and_apply_joins() {
        let source = "SELECT g.a FROM g CROSS APPLY (SELECT h.a FROM h) h GROUP BY ALL";
        let document = parse_sql_document(source);
        assert_eq!(document.status, SqlDocumentStatus::Parsed);

        let slice = |kind: SqlDocumentNodeKind| {
            document
                .nodes
                .iter()
                .filter(|node| node.kind == kind)
                .filter_map(|node| node.range.and_then(|range| document.source_slice(range)))
                .collect::<Vec<_>>()
        };
        assert!(
            slice(SqlDocumentNodeKind::Join)
                .iter()
                .any(|text| text.starts_with("CROSS APPLY") && text.ends_with(") h")),
            "join ranges: {:?}",
            slice(SqlDocumentNodeKind::Join)
        );
        assert!(
            slice(SqlDocumentNodeKind::GroupByClause).contains(&"GROUP BY ALL"),
            "group ranges: {:?}",
            slice(SqlDocumentNodeKind::GroupByClause)
        );
    }

    #[test]
    fn exposes_offset_without_limit_from_its_parser_span() {
        let source = "SELECT a FROM g OFFSET 2";
        let document = parse_sql_document(source);
        assert_eq!(document.status, SqlDocumentStatus::Parsed);

        let offset = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::OffsetClause)
            .expect("offset clause exists");
        assert_eq!(
            offset.range.and_then(|range| document.source_slice(range)),
            Some("OFFSET 2")
        );
        let limit = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::LimitClause)
            .expect("row-limit clause contains offset-only syntax");
        assert_eq!(
            limit.range.and_then(|range| document.source_slice(range)),
            Some("OFFSET 2")
        );
    }

    #[test]
    fn exposes_projection_item_ranges_and_nested_select_parent_links() {
        let source = "SELECT /* before */ *, t.* /* wildcard tail */, CAST(amount AS DOUBLE) AS total /* alias tail */, (SELECT x AS inner_value FROM nested) AS nested_value FROM \"G\"";
        let document = parse_sql_document(source);

        assert_eq!(document.status, SqlDocumentStatus::Parsed);
        assert_eq!(document.to_sql(), source);

        let outer_select = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::Select
                    && node
                        .range
                        .and_then(|range| document.source_slice(range))
                        .is_some_and(|slice| slice.starts_with("SELECT /* before */"))
            })
            .expect("the outer SELECT has a node");
        let outer_list = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::SelectList && node.parent == Some(outer_select.id)
            })
            .expect("the outer SELECT has a projection list");
        let outer_items = document
            .nodes
            .iter()
            .filter(|node| {
                node.kind == SqlDocumentNodeKind::SelectItem && node.parent == Some(outer_list.id)
            })
            .collect::<Vec<_>>();
        let item_texts = outer_items
            .iter()
            .map(|node| {
                document
                    .source_slice(node.range.expect("projection item has a parser range"))
                    .expect("projection range is valid")
            })
            .collect::<Vec<_>>();
        assert_eq!(
            item_texts,
            [
                "*",
                "t.*",
                "CAST(amount AS DOUBLE) AS total",
                "(SELECT x AS inner_value FROM nested) AS nested_value",
            ]
        );

        let nested_item = outer_items[3];
        let nested_item_range = nested_item.range.expect("nested projection has a range");
        let inner_select = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::Select
                    && node
                        .range
                        .and_then(|range| document.source_slice(range))
                        .is_some_and(|slice| slice.starts_with("SELECT x AS inner_value"))
            })
            .expect("nested SELECT has a node");
        let inner_select_range = inner_select.range.expect("nested SELECT has a range");
        assert!(nested_item_range.start <= inner_select_range.start);
        assert!(inner_select_range.end <= nested_item_range.end);

        let inner_query = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::Query
                    && node
                        .range
                        .is_some_and(|range| range.start == inner_select_range.start)
            })
            .expect("nested SELECT belongs to a query node");
        assert_eq!(inner_select.parent, Some(inner_query.id));
        let query_parent = document
            .nodes
            .iter()
            .find(|node| node.id == inner_query.parent.expect("nested query has a parent"))
            .expect("nested query parent id resolves to a node");
        assert_eq!(query_parent.kind, SqlDocumentNodeKind::SubqueryExpression);
        assert_eq!(query_parent.parent, Some(nested_item.id));
        let mut ancestor = inner_query.parent;
        let mut ancestors = Vec::new();
        while let Some(id) = ancestor {
            let node = document
                .nodes
                .iter()
                .find(|node| node.id == id)
                .expect("parent id resolves to a node");
            ancestors.push(node);
            ancestor = node.parent;
        }
        assert!(ancestors
            .iter()
            .any(|node| node.id == nested_item.id && node.kind == SqlDocumentNodeKind::SelectItem));

        for comment in ["/* before */", "/* wildcard tail */", "/* alias tail */"] {
            assert!(document.tokens.iter().any(|token| {
                token.kind == SqlDocumentTokenKind::Comment
                    && document.source_slice(token.range) == Some(comment)
            }));
        }
        let mut next_offset = 0;
        for token in &document.tokens {
            assert_eq!(token.range.start, next_offset);
            next_offset = token.range.end;
        }
        assert_eq!(next_offset, source.len());
    }

    #[test]
    fn wildcard_except_options_are_included_in_projection_item_range() {
        let source = "SELECT * EXCEPT (a) FROM t";
        let document = parse_sql_document(source);

        assert_eq!(document.status, SqlDocumentStatus::Parsed);
        assert_eq!(document.to_sql(), source);
        let select = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Select)
            .expect("SELECT has a node");
        let select_list = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::SelectList && node.parent == Some(select.id)
            })
            .expect("SELECT has a projection list");
        let item = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::SelectItem && node.parent == Some(select_list.id)
            })
            .expect("wildcard projection has an item node");
        assert_eq!(
            document.source_slice(item.range.expect("projection item has a range")),
            Some("* EXCEPT (a)")
        );
    }

    #[test]
    fn no_from_select_and_set_operation_parse_as_queries() {
        for source in ["SELECT 1", "SELECT COUNT(*)", "SELECT 1 UNION SELECT 2"] {
            let document = parse_sql_document(source);
            assert_eq!(document.status, SqlDocumentStatus::Parsed, "{source}");
            assert_eq!(document.to_sql(), source);
        }
    }

    #[test]
    fn invalid_and_unsupported_drafts_keep_the_original_source() {
        let invalid = parse_sql_document("SELECT (");
        assert_eq!(invalid.status, SqlDocumentStatus::InvalidSql);
        assert_eq!(invalid.to_sql(), "SELECT (");
        assert_eq!(
            invalid
                .diagnostics
                .first()
                .map(|diagnostic| diagnostic.code),
            Some(SqlDocumentDiagnosticCode::ParseFailed)
        );

        let multiple = parse_sql_document("SELECT 1; SELECT 2");
        assert_eq!(multiple.status, SqlDocumentStatus::MultipleStatements);
        assert_eq!(multiple.to_sql(), "SELECT 1; SELECT 2");

        let write = parse_sql_document("DELETE FROM \"G\"");
        assert_eq!(write.status, SqlDocumentStatus::SelectRequired);
        assert_eq!(write.to_sql(), "DELETE FROM \"G\"");
    }

    #[test]
    fn unicode_token_ranges_are_utf8_byte_ranges() {
        let source = "-- 日\nSELECT '東京' AS \"日付\"";
        let document = parse_sql_document(source);
        assert_eq!(document.status, SqlDocumentStatus::Parsed);
        assert!(document
            .tokens
            .iter()
            .any(|token| { document.source_slice(token.range) == Some("'東京'") }));
        assert!(document
            .tokens
            .iter()
            .any(|token| { document.source_slice(token.range) == Some("\"日付\"") }));
    }

    #[test]
    fn syntax_round_trip_keeps_query_meaning_and_source_bytes() {
        let source = "SELECT /* projection */ CAST(amount AS DOUBLE) AS total FROM \"G\" WHERE day = $day ORDER BY amount DESC";
        let document = parse_sql_document(source);
        let reparsed = parse_sql_document(document.to_sql());

        assert_eq!(document.status, SqlDocumentStatus::Parsed);
        assert_eq!(reparsed.status, SqlDocumentStatus::Parsed);
        assert_eq!(document.to_sql().as_bytes(), source.as_bytes());
        assert_eq!(canonical_query(source), canonical_query(reparsed.to_sql()));
    }

    fn canonical_query(source: &str) -> String {
        let statements = Parser::parse_sql_with_comments(&GenericDialect {}, source)
            .expect("query should parse")
            .0;
        match statements.as_slice() {
            [Statement::Query(query)] => query.to_string(),
            _ => panic!("expected one query"),
        }
    }

    #[test]
    fn token_ranges_partition_valid_sql_sources_without_dropping_trivia() {
        let bodies = [
            "SELECT 1",
            "SELECT \"日付\", '$value' FROM \"G\" WHERE id = $id",
            "WITH c AS (SELECT x FROM \"G\") SELECT x FROM c",
        ];
        let prefixes = ["", "  ", "-- note\n"];
        let suffixes = ["", " ", "\n/* end */\n"];

        for body in bodies {
            for prefix in prefixes {
                for suffix in suffixes {
                    let source = format!("{prefix}{body};{suffix}");
                    let document = parse_sql_document(&source);
                    assert_eq!(document.status, SqlDocumentStatus::Parsed, "{source}");

                    let mut next_offset = 0;
                    for token in &document.tokens {
                        assert_eq!(token.range.start, next_offset, "{source}");
                        next_offset = token.range.end;
                    }
                    assert_eq!(next_offset, source.len(), "{source}");
                    assert_eq!(document.to_sql().as_bytes(), source.as_bytes());
                }
            }
        }
    }

    #[test]
    fn lossless_document_retains_source_tokens_and_syntax_ranges_without_execution_admission() {
        let source = "-- daily\nWITH c AS (SELECT CAST(amount AS DOUBLE) AS n FROM \"G\") SELECT CASE WHEN n > 0 THEN n ELSE 0 END AS total FROM c WHERE n = $amount /* keep */";
        let document = parse_sql_document(source);

        assert_eq!(document.status, SqlDocumentStatus::Parsed);
        assert_eq!(document.to_sql().as_bytes(), source.as_bytes());
        assert!(document.tokens.iter().all(|token| {
            document.source_slice(token.range).is_some()
                && token.range.start <= token.range.end
                && source.is_char_boundary(token.range.start)
                && source.is_char_boundary(token.range.end)
        }));
        for kind in [
            SqlDocumentTokenKind::Trivia,
            SqlDocumentTokenKind::Comment,
            SqlDocumentTokenKind::QuotedIdentifier,
            SqlDocumentTokenKind::Literal,
            SqlDocumentTokenKind::Placeholder,
        ] {
            assert!(
                document.tokens.iter().any(|token| token.kind == kind),
                "{kind:?}"
            );
        }
        assert!(document.nodes.iter().all(|node| {
            node.range.is_none_or(|range| {
                document.source_slice(range).is_some()
                    && source.is_char_boundary(range.start)
                    && source.is_char_boundary(range.end)
            })
        }));
        assert!(document
            .nodes
            .iter()
            .any(|node| node.kind == SqlDocumentNodeKind::CaseExpression));
        assert!(document
            .nodes
            .iter()
            .any(|node| node.kind == SqlDocumentNodeKind::CastExpression));
        // `Parsed` is syntax state only. No plan or executor admission is run
        // by this document parser.
    }

    #[test]
    fn oversized_input_fails_before_tokenization() {
        let source = "x".repeat(MAX_SQL_DOCUMENT_BYTES + 1);
        let document = parse_sql_document(&source);
        assert_eq!(document.status, SqlDocumentStatus::TooLarge);
        assert!(document.tokens.is_empty());
        assert_eq!(document.to_sql(), source);
    }

    #[test]
    fn applies_disjoint_edits_simultaneously_against_the_original_source() {
        let source = "SELECT alpha, beta FROM \"G\"";
        let edits = vec![
            edit(source, "beta", "gamma"),
            edit(source, "alpha", "delta"),
        ];
        let result = apply_sql_document_edit_transaction(source, &transaction(source, edits))
            .expect("disjoint edits should apply");

        assert_eq!(result.document.to_sql(), "SELECT delta, gamma FROM \"G\"");
        assert_eq!(result.document.status, SqlDocumentStatus::Parsed);
        assert_eq!(result.undo.expected_source, result.document.source);
    }

    #[test]
    fn adjacent_replacements_and_boundary_insertions_have_guarded_undo() {
        let source = "SELECT ab";
        let adjacent = transaction(source, vec![edit(source, "a", "A"), edit(source, "b", "B")]);
        let changed = apply_sql_document_edit_transaction(source, &adjacent)
            .expect("adjacent replacements should be unambiguous");
        assert_eq!(changed.document.to_sql(), "SELECT AB");
        assert_eq!(changed.undo.edits.len(), 1);
        let restored = apply_sql_document_edit_transaction(&changed.document.source, &changed.undo)
            .expect("adjacent inverse edits should be safely coalesced");
        assert_eq!(restored.document.to_sql(), source);

        let insertion_boundary = source.len();
        let boundary_edits = transaction(
            source,
            vec![
                SqlDocumentEdit {
                    range: SqlSourceRange {
                        start: insertion_boundary,
                        end: insertion_boundary,
                    },
                    expected_text: String::new(),
                    replacement: "!".to_owned(),
                },
                SqlDocumentEdit {
                    range: SqlSourceRange {
                        start: insertion_boundary - 1,
                        end: insertion_boundary,
                    },
                    expected_text: "b".to_owned(),
                    replacement: "B".to_owned(),
                },
            ],
        );
        let changed = apply_sql_document_edit_transaction(source, &boundary_edits)
            .expect("insertion at an adjacent range boundary should be ordered");
        assert_eq!(changed.document.to_sql(), "SELECT aB!");
        let restored = apply_sql_document_edit_transaction(&changed.document.source, &changed.undo)
            .expect("boundary insertion inverse should restore the original source");
        assert_eq!(restored.document.to_sql(), source);

        let ambiguous_insertions = transaction(
            source,
            vec![
                SqlDocumentEdit {
                    range: SqlSourceRange {
                        start: insertion_boundary,
                        end: insertion_boundary,
                    },
                    expected_text: String::new(),
                    replacement: "!".to_owned(),
                },
                SqlDocumentEdit {
                    range: SqlSourceRange {
                        start: insertion_boundary,
                        end: insertion_boundary,
                    },
                    expected_text: String::new(),
                    replacement: "?".to_owned(),
                },
            ],
        );
        assert_eq!(
            apply_sql_document_edit_transaction(source, &ambiguous_insertions),
            Err(SqlDocumentEditError::OverlappingEdits)
        );
    }

    #[test]
    fn rejects_stale_overlapping_and_mismatched_preimage_edits_atomically() {
        let source = "SELECT amount FROM \"G\"";
        let stale = transaction(source, vec![edit(source, "amount", "total")]);
        assert_eq!(
            apply_sql_document_edit_transaction("SELECT changed FROM \"G\"", &stale),
            Err(SqlDocumentEditError::StaleSource)
        );

        let first = source.find("amount").unwrap();
        let overlapping = transaction(
            source,
            vec![
                SqlDocumentEdit {
                    range: SqlSourceRange {
                        start: first,
                        end: first + 4,
                    },
                    expected_text: "amou".to_owned(),
                    replacement: "x".to_owned(),
                },
                SqlDocumentEdit {
                    range: SqlSourceRange {
                        start: first + 2,
                        end: first + 6,
                    },
                    expected_text: "ount".to_owned(),
                    replacement: "y".to_owned(),
                },
            ],
        );
        assert_eq!(
            apply_sql_document_edit_transaction(source, &overlapping),
            Err(SqlDocumentEditError::OverlappingEdits)
        );

        let mismatched_preimage = transaction(
            source,
            vec![SqlDocumentEdit {
                range: SqlSourceRange {
                    start: first,
                    end: first + "amount".len(),
                },
                expected_text: "total".to_owned(),
                replacement: "value".to_owned(),
            }],
        );
        assert_eq!(
            apply_sql_document_edit_transaction(source, &mismatched_preimage),
            Err(SqlDocumentEditError::PreimageMismatch)
        );
    }

    #[test]
    fn rejects_out_of_bounds_and_non_utf8_boundary_ranges() {
        let source = "SELECT '東京'";
        let out_of_bounds = transaction(
            source,
            vec![SqlDocumentEdit {
                range: SqlSourceRange {
                    start: source.len(),
                    end: source.len() + 1,
                },
                expected_text: "x".to_owned(),
                replacement: "y".to_owned(),
            }],
        );
        assert_eq!(
            apply_sql_document_edit_transaction(source, &out_of_bounds),
            Err(SqlDocumentEditError::RangeOutOfBounds)
        );

        let japanese_start = source.find("東京").unwrap();
        let split_code_point = transaction(
            source,
            vec![SqlDocumentEdit {
                range: SqlSourceRange {
                    start: japanese_start + 1,
                    end: japanese_start + 3,
                },
                expected_text: "京".to_owned(),
                replacement: "都".to_owned(),
            }],
        );
        assert_eq!(
            apply_sql_document_edit_transaction(source, &split_code_point),
            Err(SqlDocumentEditError::NotUtf8Boundary)
        );
    }

    #[test]
    fn keeps_invalid_draft_diagnostics_in_the_edit_result() {
        let source = "SELECT value FROM \"G\"";
        let result = apply_sql_document_edit_transaction(
            source,
            &transaction(source, vec![edit(source, "value", "(")]),
        )
        .expect("syntax diagnostics do not reject a source edit");

        assert_eq!(result.document.status, SqlDocumentStatus::InvalidSql);
        assert!(result
            .document
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == SqlDocumentDiagnosticCode::ParseFailed }));
        assert_eq!(result.document.to_sql(), "SELECT ( FROM \"G\"");
    }

    #[test]
    fn guarded_undo_restores_comments_and_every_untouched_source_byte() {
        let source =
            "-- keep this\nSELECT /* projection */ amount, day FROM \"G\" WHERE day = 1 /* tail */";
        let edits = vec![
            edit(source, "amount", "total"),
            edit(source, "day = 1", "day = 2"),
        ];
        let result = apply_sql_document_edit_transaction(source, &transaction(source, edits))
            .expect("disjoint source edits should apply");
        assert_eq!(
            result.document.to_sql(),
            "-- keep this\nSELECT /* projection */ total, day FROM \"G\" WHERE day = 2 /* tail */"
        );
        assert!(result.document.to_sql().contains("-- keep this\n"));
        assert!(result.document.to_sql().contains("/* projection */"));
        assert!(result.document.to_sql().ends_with("/* tail */"));

        let undone = apply_sql_document_edit_transaction(&result.document.source, &result.undo)
            .expect("guarded inverse should restore the original draft");
        assert_eq!(undone.document.to_sql().as_bytes(), source.as_bytes());
        assert_eq!(undone.document.status, SqlDocumentStatus::Parsed);

        let stale_undo = apply_sql_document_edit_transaction(
            "-- changed\nSELECT /* projection */ total, day FROM \"G\" WHERE day = 2 /* tail */",
            &result.undo,
        );
        assert_eq!(stale_undo, Err(SqlDocumentEditError::StaleSource));
    }

    #[test]
    fn caps_edit_count_and_result_size() {
        let source = "SELECT 1";
        let too_many = transaction(
            source,
            (0..=MAX_SQL_DOCUMENT_EDITS)
                .map(|_| SqlDocumentEdit {
                    range: SqlSourceRange { start: 0, end: 0 },
                    expected_text: String::new(),
                    replacement: String::new(),
                })
                .collect(),
        );
        assert_eq!(
            apply_sql_document_edit_transaction(source, &too_many),
            Err(SqlDocumentEditError::TooManyEdits)
        );

        let too_large = transaction(
            source,
            vec![SqlDocumentEdit {
                range: SqlSourceRange { start: 0, end: 0 },
                expected_text: String::new(),
                replacement: "x".repeat(MAX_SQL_DOCUMENT_BYTES),
            }],
        );
        assert_eq!(
            apply_sql_document_edit_transaction(source, &too_large),
            Err(SqlDocumentEditError::OutputTooLarge)
        );
    }

    #[test]
    fn excessive_syntax_tokens_fail_before_ast_construction() {
        let source = "(".repeat(MAX_SQL_DOCUMENT_COMPLEXITY_TOKENS + 1);
        let document = parse_sql_document(&source);

        assert_eq!(document.status, SqlDocumentStatus::TooComplex);
        assert!(document.tokens.is_empty());
        assert!(document.nodes.is_empty());
        assert_eq!(
            document.diagnostics[0].code,
            SqlDocumentDiagnosticCode::SqlTooComplex
        );
    }
}
