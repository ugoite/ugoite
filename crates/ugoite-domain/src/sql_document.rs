//! Lossless source document and parser view for the SQL Workspace.
//!
//! This module records SQL syntax and source locations. It does not authorize,
//! type-check, plan, or execute a query. The Rust SQL executor remains the
//! authority for the read-only SQL policy and the query planner remains the
//! authority for expression types and result schemas.

use core::ops::ControlFlow;
use serde::{Deserialize, Serialize};
use sqlparser::ast::{
    CastKind, CteAsMaterialized, Distinct, DuplicateTreatment, Expr, FunctionArg, FunctionArgExpr,
    FunctionArguments, GroupByExpr, Ident, LimitClause, ObjectName, ObjectNamePart, OrderByKind,
    OrderBySort, Query, Select, SelectItem, SetExpr, Spanned, Statement, TableFactor, Value,
    ValueWithSpan, Visit, Visitor,
};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::{Parser, ParserError};
use sqlparser::tokenizer::{Location, Span, Token, Tokenizer, Whitespace};
use std::convert::Infallible;

/// Version of the Rust SQL Document view returned to adapters.
pub const SQL_DOCUMENT_CONTRACT_VERSION: u32 = 19;

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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

pub(crate) fn has_comment_token_in_range(source: &str, range: SqlSourceRange) -> Option<bool> {
    if range.start >= range.end || source.get(range.start..range.end).is_none() {
        return None;
    }

    let document = parse_sql_document(source);
    if document.status != SqlDocumentStatus::Parsed {
        return None;
    }

    let mut next_offset = 0;
    for token in &document.tokens {
        if token.range.start != next_offset
            || token.range.start >= token.range.end
            || document.source_slice(token.range).is_none()
        {
            return None;
        }
        next_offset = token.range.end;
    }
    if next_offset != source.len() {
        return None;
    }

    Some(document.tokens.iter().any(|token| {
        token.kind == SqlDocumentTokenKind::Comment
            && token.range.start < range.end
            && range.start < token.range.end
    }))
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
    /// A FROM table factor with its kind and optional alias.
    TableFactor {
        /// Parser AST category of the table factor.
        factor_kind: String,
        /// Declared alias, if present.
        alias: Option<SqlDocumentIdentifier>,
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
#[serde(deny_unknown_fields)]
pub struct SqlDocumentNodeEdit {
    /// Node index returned by the SQL Document parser.
    pub node_id: u32,
    /// Syntax category the caller selected in the parsed document.
    pub expected_kind: SqlDocumentNodeKind,
    /// Replacement SQL for the node's exact source range.
    pub replacement: String,
}

/// A set of typed node replacements guarded by the exact document source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlDocumentNodeEditTransaction {
    /// Parser contract version that produced the selected node IDs.
    pub contract_version: u32,
    /// Exact source used to select the node IDs. A mismatch rejects all edits.
    pub expected_source: String,
    /// Disjoint parser-node replacements interpreted against the original tree.
    pub edits: Vec<SqlDocumentNodeEdit>,
}

/// One typed semantic edit addressed to a parser-classified expression node.
///
/// The node ID and expected kind are meaningful only for the exact source and
/// contract version guarded by [`SqlDocumentSemanticEditTransaction`]. The
/// replacement is generated by Rust from the typed fields instead of
/// accepting SQL text from an adapter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SqlDocumentSemanticEdit {
    /// Set or remove the output alias on one exact SELECT projection item.
    SetProjectionAlias {
        node_id: u32,
        expected_kind: SqlDocumentNodeKind,
        alias: Option<SqlDocumentIdentifier>,
    },
    /// Add a typed comparison against a column in one SELECT scope. This is
    /// the task-facing constructor used when the document has no predicate node.
    AddFilterComparison {
        node_id: u32,
        expected_kind: SqlDocumentNodeKind,
        column: SqlDocumentColumnReference,
        operator: SqlDocumentFilterOperator,
        literal: Option<SqlDocumentSemanticLiteral>,
    },
    /// Add a typed column sort key without requiring an existing expression.
    AddOrderByColumn {
        node_id: u32,
        expected_kind: SqlDocumentNodeKind,
        column: SqlDocumentColumnReference,
        direction: SqlDocumentOrderDirection,
        nulls_order: Option<SqlDocumentNullsOrder>,
    },
    /// Construct the first SELECT in an empty (possibly trivia-only) draft.
    /// Rust renders the optional relation as one identifier; nonempty invalid
    /// SQL is never replaced by this operation.
    InitializeSelect {
        /// None produces a relation-free SELECT with an editable literal.
        relation: Option<SqlDocumentIdentifier>,
    },
    /// Replace an identifier expression with one or more typed identifier
    /// parts. One part emits an identifier expression; multiple parts emit a
    /// compound identifier expression.
    ReplaceIdentifier {
        /// Node index returned by the SQL Document parser.
        node_id: u32,
        /// Identifier syntax category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Identifier components in source order.
        parts: Vec<SqlDocumentIdentifier>,
    },
    /// Replace a literal value using a typed value, never an arbitrary SQL
    /// fragment.
    ReplaceLiteral {
        /// Node index returned by the SQL Document parser.
        node_id: u32,
        /// Literal syntax category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Typed SQL literal value.
        literal: SqlDocumentSemanticLiteral,
    },
    /// Replace only the operator token of a binary expression.
    ReplaceBinaryOperator {
        /// Node index returned by the SQL Document parser.
        node_id: u32,
        /// Binary expression category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Closed operator set supported by this edit API.
        operator: SqlDocumentBinaryOperator,
    },
    /// Replace the sign token of a parser-classified arithmetic unary
    /// expression while preserving the operand and intervening source.
    ReplaceUnarySign {
        /// Node index returned by the SQL Document parser.
        node_id: u32,
        /// Unary expression category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Closed arithmetic sign set accepted by this edit API.
        sign: SqlDocumentUnarySign,
    },
    /// Replace the set operator and quantifier between the exact parsed
    /// operands while preserving intervening whitespace and comments.
    ReplaceSetOperation {
        /// Node index returned by the SQL Document parser.
        node_id: u32,
        /// Set-operation category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Closed set operator accepted by this edit API.
        operator: SqlDocumentSetOperator,
        /// Closed set quantifier accepted by this edit API.
        quantifier: SqlDocumentSetQuantifier,
    },
    /// Append a typed SELECT seed as a new set-operation operand.
    AddSetOperationOperand {
        /// Query node to extend.
        node_id: u32,
        /// Query category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Operator joining the new operand.
        operator: SqlDocumentSetOperator,
        /// Duplicate-handling mode for the new operation.
        quantifier: SqlDocumentSetQuantifier,
    },
    /// Remove one side of a set operation while retaining the other side.
    RemoveSetOperationOperand {
        /// Set-operation node to reduce to one operand.
        node_id: u32,
        /// Set-operation category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// The operand that remains after the edit.
        keep: SqlDocumentSetOperand,
    },
    /// Swap the exact left and right operand source ranges of a set operation.
    /// Operator text and trivia between the operands remain in place.
    SwapSetOperationOperands {
        /// Set-operation node to reorder.
        node_id: u32,
        /// Set-operation category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
    },
    /// Replace one set-operation input with an exact query, SELECT, or nested
    /// set expression already present in this document.
    ReplaceSetOperationOperand {
        /// Set-operation input node to replace.
        node_id: u32,
        /// Set-operation side selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Parser-backed query expression to copy into the selected side.
        expression_node_id: u32,
        /// Parser category of the copied expression.
        expression_expected_kind: SqlDocumentNodeKind,
    },
    /// Replace the target type of a parser-classified cast expression. The
    /// cast syntax and operand source are retained exactly.
    ReplaceCastTargetType {
        /// Node index returned by the SQL Document parser.
        node_id: u32,
        /// CAST expression category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Portable target type chosen from the closed SQL GUI type set.
        target_type: SqlDocumentCastType,
    },
    /// Set a CAST target with exact, typed SQL type parameters.
    SetParameterizedCastTarget {
        node_id: u32,
        expected_kind: SqlDocumentNodeKind,
        target: SqlDocumentCastTarget,
    },
    /// Wrap an existing typed expression with a Rust-rendered CAST node.
    WrapExpressionInCast {
        /// Expression node to wrap.
        node_id: u32,
        /// Expression category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Portable target type chosen from the closed SQL GUI type set.
        target_type: SqlDocumentCastType,
    },
    /// Wrap one typed expression in an aggregate from the shared SQL registry.
    WrapExpressionInAggregate {
        /// Expression node to wrap.
        node_id: u32,
        /// Expression category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Registered aggregate selected by the caller.
        function: SqlDocumentAggregateFunction,
        /// Apply DISTINCT to the single selected argument.
        distinct: bool,
    },
    /// Change the registered aggregate called by a simple aggregate function.
    SetAggregateFunction {
        /// Aggregate function call to update.
        node_id: u32,
        /// Function-expression category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Closed aggregate registry entry to use.
        function: SqlDocumentAggregateFunction,
    },
    /// Set or remove DISTINCT on a simple registered aggregate call.
    SetAggregateDistinct {
        /// Aggregate function call to update.
        node_id: u32,
        /// Function-expression category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Whether the aggregate should use DISTINCT arguments.
        distinct: bool,
    },
    /// Remove a simple one-argument aggregate wrapper and keep its argument.
    RemoveAggregate {
        /// Aggregate function call to unwrap.
        node_id: u32,
        /// Function-expression category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
    },
    /// Wrap an existing typed expression in a searched CASE expression.
    WrapExpressionInCase {
        /// Expression node to wrap.
        node_id: u32,
        /// Expression category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
    },
    /// Build a typed predicate around an existing expression, using editable
    /// NULL or string literal seeds for any additional operands.
    WrapExpressionInPredicate {
        /// Expression node to wrap.
        node_id: u32,
        /// Expression category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Closed predicate template selected by the caller.
        template: SqlDocumentPredicateTemplate,
    },
    /// Append an editable NULL value to an `IN` or `NOT IN` value list.
    AppendInListValue {
        /// IN predicate node index returned by the parser.
        node_id: u32,
        /// Predicate category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
    },
    /// Remove the final value from an IN list that still has another value.
    RemoveLastInListValue {
        /// IN predicate node index returned by the parser.
        node_id: u32,
        /// Predicate category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
    },
    /// Replace a parsed table factor with a typed named or derived relation,
    /// or with an exact source-backed table factor from this document.
    ReplaceTableFactor {
        /// Table-factor node to replace.
        node_id: u32,
        /// Table-factor category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Typed relation factor that will occupy the selected slot.
        factor: SqlDocumentRelationFactor,
    },
    /// Append a typed JOIN to an existing FROM relation tree. Repeating this
    /// operation supports multiple Form relations without serializing the
    /// existing tree.
    AddJoin {
        /// Relation-tree node to extend.
        node_id: u32,
        /// Relation-tree category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Join operator and optional typed constraint.
        join: SqlDocumentJoin,
    },
    /// Add a typed relation to one SELECT scope, creating FROM when absent.
    /// Existing FROM lists receive a new comma-separated relation tree.
    AddFromRelation {
        /// SELECT scope receiving the relation.
        node_id: u32,
        /// SELECT category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Typed relation factor to add.
        factor: SqlDocumentRelationFactor,
    },
    /// Replace one parsed JOIN subtree with a typed operator, relation factor,
    /// and constraint. Comments inside the replaced subtree fail closed.
    ReplaceJoin {
        /// Join node to replace.
        node_id: u32,
        /// Join category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Replacement join subtree.
        join: SqlDocumentJoin,
    },
    /// Rename one CTE declaration and every unambiguous single-part relation
    /// reference to it in the same SQL document.
    RenameCommonTableExpression {
        /// Node index returned by the SQL Document parser.
        node_id: u32,
        /// CTE declaration category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Replacement CTE name emitted as a typed identifier.
        name: SqlDocumentIdentifier,
    },
    /// Add an unused `name AS (SELECT 1)` entry to this query's WITH clause,
    /// creating the clause when needed. The new body is a typed editable seed.
    AddCommonTableExpression {
        /// Query node whose WITH list receives the new CTE.
        node_id: u32,
        /// Query category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// New CTE name emitted as a typed identifier.
        name: SqlDocumentIdentifier,
    },
    /// Remove an unreferenced CTE from its WITH list.
    RemoveCommonTableExpression {
        /// CTE declaration category selected by the caller.
        node_id: u32,
        /// Parser category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
    },
    /// Replace a CTE body with an exact query already parsed in this document.
    ReplaceCommonTableExpressionQuery {
        /// CTE declaration whose query body is replaced.
        node_id: u32,
        /// CTE declaration category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Parser-backed query to copy as the CTE body.
        query_node_id: u32,
    },
    /// Move a CTE to a new ordinal within its own WITH clause.
    MoveCommonTableExpression {
        /// CTE declaration to move.
        node_id: u32,
        /// CTE declaration category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Zero-based destination ordinal within the same WITH clause.
        to_index: usize,
    },
    /// Toggle `NOT` on a parser-classified predicate while retaining the
    /// predicate operands and their source text.
    ReplacePredicateNegation {
        /// Node index returned by the SQL Document parser.
        node_id: u32,
        /// Predicate category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Whether the predicate is negated after the edit.
        negated: bool,
    },
    /// Append a searched `WHEN TRUE THEN NULL` branch to a parsed CASE node.
    /// The inserted branch is an editable draft whose leaves use the same
    /// typed expression operations as the rest of the tree.
    AddCaseBranch {
        /// Node index returned by the SQL Document parser.
        node_id: u32,
        /// CASE expression category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
    },
    /// Remove the final WHEN branch from a parsed CASE expression.
    RemoveCaseBranch {
        /// Node index returned by the SQL Document parser.
        node_id: u32,
        /// CASE expression category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
    },
    /// Add or remove the ELSE result of a parsed CASE expression.
    SetCaseElse {
        /// Node index returned by the SQL Document parser.
        node_id: u32,
        /// CASE expression category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Whether the expression should have an ELSE branch.
        has_else: bool,
    },
    /// Choose an expression as a simple CASE operand, or clear it to switch
    /// back to searched CASE. The selected expression must come from the same
    /// query scope and must not be inside the CASE being changed.
    SetCaseOperand {
        node_id: u32,
        expected_kind: SqlDocumentNodeKind,
        expression_node_id: Option<u32>,
        expression_expected_kind: Option<SqlDocumentNodeKind>,
    },
    /// Add a `WHERE TRUE` seed to a SELECT that does not have a WHERE clause.
    /// The boolean expression is a typed tree node and can be replaced or
    /// composed with other same-query expressions in follow-up edits.
    AddWhereClause {
        /// SELECT node whose query receives the WHERE clause.
        node_id: u32,
        /// SELECT category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
    },
    /// Replace a WHERE or HAVING predicate with a same-query typed expression.
    SetFilterPredicate {
        /// WHERE or HAVING clause to update.
        node_id: u32,
        /// WHERE or HAVING clause category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Parser-backed predicate expression to copy into the clause.
        expression_node_id: u32,
        /// Parser category of the copied expression.
        expression_expected_kind: SqlDocumentNodeKind,
    },
    /// Remove a WHERE or HAVING clause while retaining adjacent query clauses.
    RemoveFilterClause {
        /// WHERE or HAVING clause to remove.
        node_id: u32,
        /// WHERE or HAVING clause category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
    },
    /// Compose two typed expressions from the same query scope into a binary
    /// expression without accepting source SQL from the adapter.
    ComposeBinaryExpression {
        /// Expression or projection item node index returned by the parser.
        node_id: u32,
        /// SELECT item category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Left operand node selected from the same query.
        left_expression_node_id: u32,
        /// Parser category of the left expression.
        left_expression_expected_kind: SqlDocumentNodeKind,
        /// Closed binary operator selected by the caller.
        operator: SqlDocumentBinaryOperator,
        /// Right operand node selected from the same query.
        right_expression_node_id: u32,
        /// Parser category of the right expression.
        right_expression_expected_kind: SqlDocumentNodeKind,
    },
    /// Replace any expression node with another typed expression from the
    /// same query scope while retaining all source outside the target span.
    ReplaceExpressionFromNode {
        /// Expression node to replace.
        node_id: u32,
        /// Parser category selected by the caller for the target expression.
        expected_kind: SqlDocumentNodeKind,
        /// Existing typed expression to copy into the target span.
        expression_node_id: u32,
        /// Parser category selected by the caller for the source expression.
        expression_expected_kind: SqlDocumentNodeKind,
    },
    /// Replace the expression inside one parsed SELECT projection item by
    /// copying another typed expression node from the same document.
    SetProjectionExpression {
        /// Projection item node index returned by the parser.
        node_id: u32,
        /// SELECT item category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Parsed expression node to copy into this projection item.
        expression_node_id: u32,
        /// Parser category of the copied expression node.
        expression_expected_kind: SqlDocumentNodeKind,
    },
    /// Append an expression selected from the same document as a new SELECT
    /// projection item.
    AddProjectionExpression {
        /// SELECT-list node index returned by the parser.
        node_id: u32,
        /// SELECT-list category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Parsed expression node to copy into the new projection item.
        expression_node_id: u32,
        /// Parser category of the copied expression node.
        expression_expected_kind: SqlDocumentNodeKind,
    },
    /// Append a column reference assembled from typed identifiers.
    AddProjectionColumn {
        /// SELECT-list node to extend.
        node_id: u32,
        /// SELECT-list category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Typed column reference, optionally qualified by a relation alias.
        column: SqlDocumentColumnReference,
    },
    /// Expand one parsed wildcard into a supplied, typed list of columns.
    ExpandProjectionWildcard {
        /// Wildcard item to expand.
        node_id: u32,
        /// SELECT-item category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Column references from the selected relation schema.
        columns: Vec<SqlDocumentColumnReference>,
    },
    /// Append a relation-free scalar subquery projection using a typed seed.
    /// The new `SELECT 1` remains an ordinary parsed query tree for follow-up
    /// edits; no SQL fragment is accepted from the adapter.
    AddScalarSubqueryProjection {
        /// SELECT-list node to extend.
        node_id: u32,
        /// SELECT-list category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
    },
    /// Wrap an existing expression in an `IN (SELECT 1)` or `NOT IN` seed.
    /// The nested SELECT is a typed parser-visible draft for further edits.
    WrapExpressionInSubqueryPredicate {
        /// Expression node to wrap as the predicate's left operand.
        node_id: u32,
        /// Expression category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Closed subquery predicate form.
        template: SqlDocumentSubqueryPredicateTemplate,
    },
    /// Replace an expression with a typed `EXISTS (SELECT 1)` seed.
    ReplaceExpressionWithExistsSubquery {
        /// Expression node to replace.
        node_id: u32,
        /// Expression category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
    },
    /// Add an existing typed expression to a SELECT's GROUP BY clause.
    /// When the clause is absent, Rust inserts it before HAVING/ORDER/LIMIT.
    AddGroupByExpression {
        /// SELECT node whose grouping expressions are extended.
        node_id: u32,
        /// SELECT category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Existing expression node selected from the same query scope.
        expression_node_id: u32,
        /// Parser category of the selected expression.
        expression_expected_kind: SqlDocumentNodeKind,
    },
    /// Remove a grouping key, removing GROUP BY if it was the final key.
    RemoveGroupByItem {
        /// Grouping key to remove.
        node_id: u32,
        /// Group-by item category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
    },
    /// Move a grouping key to a new position in the same GROUP BY clause.
    MoveGroupByItem {
        /// Grouping key to move.
        node_id: u32,
        /// Group-by item category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Zero-based destination ordinal within the GROUP BY list.
        to_index: usize,
    },
    /// Add an existing expression from the same query scope to ORDER BY.
    /// The new key is appended as an ascending key while preserving all other
    /// query clauses and source trivia.
    AddOrderByExpression {
        /// Query node whose ORDER BY keys are extended.
        node_id: u32,
        /// Query category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Existing expression node selected from the same query scope.
        expression_node_id: u32,
        /// Parser category of the selected expression.
        expression_expected_kind: SqlDocumentNodeKind,
        /// Direction for the new ordering key.
        direction: SqlDocumentOrderDirection,
        /// Optional explicit NULL placement.
        nulls_order: Option<SqlDocumentNullsOrder>,
    },
    /// Replace the expression of one exact ORDER BY item with another
    /// parser-backed expression from the same query scope.
    SetOrderByExpression {
        /// ORDER BY item to update.
        node_id: u32,
        /// ORDER BY item category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Parser-backed expression to copy into the item.
        expression_node_id: u32,
        /// Parser category of the copied expression.
        expression_expected_kind: SqlDocumentNodeKind,
    },
    /// Set direction and NULL ordering for one ORDER BY item.
    SetOrderByOptions {
        /// ORDER BY item to update.
        node_id: u32,
        /// ORDER BY item category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Requested ordering direction.
        direction: SqlDocumentOrderDirection,
        /// Optional explicit NULL placement.
        nulls_order: Option<SqlDocumentNullsOrder>,
    },
    /// Remove one ORDER BY item, removing the clause when it was the only key.
    RemoveOrderByItem {
        /// ORDER BY item to remove.
        node_id: u32,
        /// ORDER BY item category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
    },
    /// Move one ORDER BY item to a new ordinal within its clause.
    MoveOrderByItem {
        /// ORDER BY item to move.
        node_id: u32,
        /// ORDER BY item category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Zero-based destination ordinal within the same ORDER BY clause.
        to_index: usize,
    },
    /// Toggle plain SELECT DISTINCT while retaining its projection and clauses.
    SetSelectDistinct {
        /// SELECT node to update.
        node_id: u32,
        /// SELECT category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Whether duplicate rows should be removed.
        distinct: bool,
    },
    /// Set or replace a literal LIMIT value using an unsigned integer.
    SetLimitValue {
        /// Query node to update.
        node_id: u32,
        /// Query category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Non-negative row limit.
        value: u64,
    },
    /// Remove LIMIT while retaining an existing OFFSET.
    RemoveLimit {
        /// Query node to update.
        node_id: u32,
        /// Query category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
    },
    /// Set or replace a literal OFFSET value using an unsigned integer.
    SetOffsetValue {
        /// Query node to update.
        node_id: u32,
        /// Query category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Non-negative row offset.
        value: u64,
    },
    /// Remove OFFSET while retaining an existing LIMIT.
    RemoveOffset {
        /// Query node to update.
        node_id: u32,
        /// Query category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
    },
    /// Add an editable `HAVING TRUE` seed to a SELECT without a HAVING clause.
    AddHavingClause {
        /// SELECT node to update.
        node_id: u32,
        /// SELECT category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
    },
    /// Remove an entire parsed HAVING clause.
    RemoveHavingClause {
        /// HAVING clause to remove.
        node_id: u32,
        /// HAVING-clause category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
    },
    /// Remove one parsed projection item while retaining comments and trivia
    /// outside its source range. The final item cannot be removed.
    RemoveProjectionItem {
        /// SELECT item node index returned by the parser.
        node_id: u32,
        /// SELECT item category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
    },
    /// Move one parsed projection item to a new ordinal in its SELECT list.
    /// Reordering is rejected when comments occur within the list because
    /// their attachment to individual items would otherwise be ambiguous.
    MoveProjectionItem {
        /// SELECT item node index returned by the parser.
        node_id: u32,
        /// SELECT item category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Zero-based destination ordinal in the same SELECT list.
        to_index: usize,
    },
    /// Replace one aggregate argument, or append a copied expression at the
    /// end when `argument_index` equals the current count. The expression is
    /// selected by another typed node from this same document.
    SetFunctionArgument {
        /// Aggregate function node index returned by the parser.
        node_id: u32,
        /// Function expression category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
        /// Zero-based argument to replace, or the current count to append.
        argument_index: usize,
        /// Parsed expression node to copy into the function argument.
        expression_node_id: u32,
        /// Parser category of the copied expression node.
        expression_expected_kind: SqlDocumentNodeKind,
    },
    /// Remove the final argument from a COUNT aggregate.
    RemoveLastFunctionArgument {
        /// Aggregate function node index returned by the parser.
        node_id: u32,
        /// Function expression category selected by the caller.
        expected_kind: SqlDocumentNodeKind,
    },
}

/// Arithmetic unary signs the typed expression editor can select.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentUnarySign {
    Positive,
    Negative,
    Not,
}

impl SqlDocumentUnarySign {
    fn sql_spelling(self) -> &'static str {
        match self {
            Self::Positive => "+",
            Self::Negative => "-",
            Self::Not => "NOT",
        }
    }
}

/// Set operators the typed SQL tree editor can select.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentSetOperator {
    Union,
    Intersect,
    Except,
}

impl SqlDocumentSetOperator {
    fn sql_spelling(self) -> &'static str {
        match self {
            Self::Union => "UNION",
            Self::Intersect => "INTERSECT",
            Self::Except => "EXCEPT",
        }
    }
}

/// Set quantifiers the typed SQL tree editor can select.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentSetQuantifier {
    Default,
    All,
    Distinct,
    ByName,
    AllByName,
    DistinctByName,
}

/// Direction used when adding an expression to ORDER BY.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentOrderDirection {
    Asc,
    Desc,
}

impl SqlDocumentOrderDirection {
    fn sql_spelling(self) -> &'static str {
        match self {
            Self::Asc => "ASC",
            Self::Desc => "DESC",
        }
    }
}

/// Explicit placement of NULL values for one ORDER BY expression.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentNullsOrder {
    First,
    Last,
}

impl SqlDocumentNullsOrder {
    fn sql_suffix(self) -> &'static str {
        match self {
            Self::First => " NULLS FIRST",
            Self::Last => " NULLS LAST",
        }
    }
}

/// Closed operators accepted by the task-facing Filter editor.
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

impl SqlDocumentFilterOperator {
    fn sql_spelling(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::NotEq => "<>",
            Self::Lt => "<",
            Self::Lte => "<=",
            Self::Gt => ">",
            Self::Gte => ">=",
            Self::Like => "LIKE",
            Self::NotLike => "NOT LIKE",
            Self::IsNull => "IS NULL",
            Self::IsNotNull => "IS NOT NULL",
        }
    }
    fn needs_value(self) -> bool {
        !matches!(self, Self::IsNull | Self::IsNotNull)
    }
}

impl SqlDocumentSetQuantifier {
    fn sql_spelling(self) -> &'static str {
        match self {
            Self::Default => "",
            Self::All => " ALL",
            Self::Distinct => " DISTINCT",
            Self::ByName => " BY NAME",
            Self::AllByName => " ALL BY NAME",
            Self::DistinctByName => " DISTINCT BY NAME",
        }
    }
}

/// Operand side retained by a typed set-operation removal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentSetOperand {
    Left,
    Right,
}

/// A table factor that can be authored without passing arbitrary SQL text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SqlDocumentRelationFactor {
    /// A qualified relation name, typically supplied by the Form picker.
    Table {
        /// Relation identifier components in source order.
        name: Vec<SqlDocumentIdentifier>,
        /// Optional SQL alias.
        alias: Option<SqlDocumentIdentifier>,
    },
    /// A derived relation copied from a parsed query in this document.
    Derived {
        /// Query node whose exact source is wrapped as a derived relation.
        query_node_id: u32,
        /// Required derived-relation alias.
        alias: SqlDocumentIdentifier,
    },
    /// An exact parser-backed table factor copied from this document.
    ExistingTableFactor {
        /// Source table-factor node to copy.
        node_id: u32,
    },
}

/// JOIN operator spellings supported by typed relation edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentJoinOperator {
    Inner,
    Left,
    LeftOuter,
    Right,
    RightOuter,
    Full,
    FullOuter,
    Cross,
    Natural,
    NaturalInner,
    NaturalLeft,
    NaturalLeftOuter,
    NaturalRight,
    NaturalRightOuter,
    NaturalFull,
    NaturalFullOuter,
}

impl SqlDocumentJoinOperator {
    fn sql_spelling(self) -> &'static str {
        match self {
            Self::Inner => "INNER JOIN",
            Self::Left => "LEFT JOIN",
            Self::LeftOuter => "LEFT OUTER JOIN",
            Self::Right => "RIGHT JOIN",
            Self::RightOuter => "RIGHT OUTER JOIN",
            Self::Full => "FULL JOIN",
            Self::FullOuter => "FULL OUTER JOIN",
            Self::Cross => "CROSS JOIN",
            Self::Natural => "NATURAL JOIN",
            Self::NaturalInner => "NATURAL INNER JOIN",
            Self::NaturalLeft => "NATURAL LEFT JOIN",
            Self::NaturalLeftOuter => "NATURAL LEFT OUTER JOIN",
            Self::NaturalRight => "NATURAL RIGHT JOIN",
            Self::NaturalRightOuter => "NATURAL RIGHT OUTER JOIN",
            Self::NaturalFull => "NATURAL FULL JOIN",
            Self::NaturalFullOuter => "NATURAL FULL OUTER JOIN",
        }
    }

    fn forbids_explicit_constraint(self) -> bool {
        matches!(
            self,
            Self::Cross
                | Self::Natural
                | Self::NaturalInner
                | Self::NaturalLeft
                | Self::NaturalLeftOuter
                | Self::NaturalRight
                | Self::NaturalRightOuter
                | Self::NaturalFull
                | Self::NaturalFullOuter
        )
    }
}

/// Typed constraint for a relation JOIN.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SqlDocumentJoinConstraint {
    /// No explicit constraint (valid for CROSS/NATURAL joins).
    None,
    /// Copy a parser-backed expression into an ON clause.
    On {
        /// Expression node selected from this document.
        expression_node_id: u32,
        /// Expression category selected by the caller.
        expression_expected_kind: SqlDocumentNodeKind,
    },
    /// Render a USING clause from typed identifier columns.
    Using {
        /// One or more columns shared by the joined relations.
        columns: Vec<SqlDocumentIdentifier>,
    },
}

/// Typed right side of a JOIN operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlDocumentJoin {
    /// Selected SQL JOIN operator.
    pub operator: SqlDocumentJoinOperator,
    /// Relation factor on the right side.
    pub relation: SqlDocumentRelationFactor,
    /// Optional ON or USING constraint.
    pub constraint: SqlDocumentJoinConstraint,
}

/// Portable cast targets exposed by the SQL expression editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentCastType {
    Varchar,
    Integer,
    Boolean,
    BigInt,
    Double,
    Decimal,
    Date,
    Timestamp,
    Json,
}

/// A portable CAST type and its optional numeric parameters. Precision and
/// length are unsigned SQL integers; scale is signed because admitted SQL
/// dialects may use negative decimal scales.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlDocumentCastTarget {
    pub kind: SqlDocumentCastType,
    pub length: Option<String>,
    pub precision: Option<String>,
    pub scale: Option<String>,
}

impl SqlDocumentCastTarget {
    fn sql_spelling(&self) -> Result<String, SqlDocumentEditError> {
        let base = self.kind.sql_spelling();
        let parameters = match self.kind {
            SqlDocumentCastType::Decimal => {
                match (self.precision.as_deref(), self.scale.as_deref()) {
                    (None, None) => None,
                    (Some(precision), scale) if valid_sql_integer(precision, false) => {
                        Some(match scale {
                            Some(scale) if valid_sql_integer(scale, true) => {
                                format!("{precision}, {scale}")
                            }
                            Some(_) => {
                                return Err(SqlDocumentEditError::InvalidSemanticReplacement)
                            }
                            None => precision.to_owned(),
                        })
                    }
                    (Some(_), _) | (None, Some(_)) => {
                        return Err(SqlDocumentEditError::InvalidSemanticReplacement)
                    }
                }
            }
            SqlDocumentCastType::Varchar => {
                if self.scale.is_some() || self.precision.is_some() {
                    return Err(SqlDocumentEditError::InvalidSemanticReplacement);
                }
                self.length
                    .as_deref()
                    .filter(|length| valid_sql_integer(length, false))
                    .map(str::to_owned)
            }
            SqlDocumentCastType::Timestamp => {
                if self.scale.is_some() || self.length.is_some() {
                    return Err(SqlDocumentEditError::InvalidSemanticReplacement);
                }
                self.precision
                    .as_deref()
                    .filter(|precision| valid_sql_integer(precision, false))
                    .map(str::to_owned)
            }
            _ => {
                if self.length.is_some() || self.precision.is_some() || self.scale.is_some() {
                    return Err(SqlDocumentEditError::InvalidSemanticReplacement);
                }
                None
            }
        };
        if self.kind == SqlDocumentCastType::Decimal && self.length.is_some() {
            return Err(SqlDocumentEditError::InvalidSemanticReplacement);
        }
        if (self.kind == SqlDocumentCastType::Varchar
            && self.length.is_some()
            && parameters.is_none())
            || (self.kind == SqlDocumentCastType::Timestamp
                && self.precision.is_some()
                && parameters.is_none())
        {
            return Err(SqlDocumentEditError::InvalidSemanticReplacement);
        }
        Ok(parameters.map_or_else(
            || base.to_owned(),
            |parameters| format!("{base}({parameters})"),
        ))
    }
}

fn valid_sql_integer(value: &str, signed: bool) -> bool {
    let digits = value.strip_prefix('-').filter(|_| signed).unwrap_or(value);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

/// Closed aggregate set rendered by the syntax-tree editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentAggregateFunction {
    Count,
    Sum,
    Avg,
    Min,
    Max,
}

impl SqlDocumentAggregateFunction {
    fn sql_spelling(self) -> &'static str {
        match self {
            Self::Count => "COUNT",
            Self::Sum => "SUM",
            Self::Avg => "AVG",
            Self::Min => "MIN",
            Self::Max => "MAX",
        }
    }
}

impl SqlDocumentCastType {
    fn sql_spelling(self) -> &'static str {
        match self {
            Self::Varchar => "VARCHAR",
            Self::Integer => "INTEGER",
            Self::Boolean => "BOOLEAN",
            Self::BigInt => "BIGINT",
            Self::Double => "DOUBLE",
            Self::Decimal => "DECIMAL",
            Self::Date => "DATE",
            Self::Timestamp => "TIMESTAMP",
            Self::Json => "JSON",
        }
    }
}

/// A SQL literal value accepted by the typed semantic edit API.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SqlDocumentSemanticLiteral {
    /// Numeric literal spelling. Rust validates the full spelling as one
    /// numeric token before parsing it as a value node.
    Number { value: String },
    /// String contents, escaped and quoted by Rust.
    String { value: String },
    /// SQL boolean literal.
    Boolean { value: bool },
    /// SQL NULL literal.
    Null,
    /// A named `$parameter` reference without the `$` prefix.
    NamedParameter { name: String },
}

/// Typed templates for constructing supported SQL predicates from an existing
/// expression node. Placeholder operands are ordinary literal nodes that can
/// be replaced or bound through subsequent semantic edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentPredicateTemplate {
    /// `expression IN (NULL)`.
    In,
    /// `expression NOT IN (NULL)`.
    NotIn,
    /// `expression BETWEEN NULL AND NULL`.
    Between,
    /// `expression NOT BETWEEN NULL AND NULL`.
    NotBetween,
    /// `expression LIKE ''`.
    Like,
    /// `expression NOT LIKE ''`.
    NotLike,
    /// `expression IS NULL`.
    IsNull,
    /// `expression IS NOT NULL`.
    IsNotNull,
}

/// Typed subquery predicate templates accepted by the SQL tree editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentSubqueryPredicateTemplate {
    /// `expression IN (SELECT 1)`.
    In,
    /// `expression NOT IN (SELECT 1)`.
    NotIn,
}

/// A closed set of binary operators accepted by semantic edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentBinaryOperator {
    /// Boolean conjunction (`AND`).
    And,
    /// Boolean disjunction (`OR`).
    Or,
    /// Bitwise conjunction (`&`).
    BitwiseAnd,
    /// Bitwise disjunction (`|`).
    BitwiseOr,
    /// Left shift (`<<`).
    ShiftLeft,
    /// Right shift (`>>`).
    ShiftRight,
    /// Equality (`=`).
    Eq,
    /// Inequality (`<>`).
    NotEq,
    /// Less than (`<`).
    Lt,
    /// Less than or equal (`<=`).
    Lte,
    /// Greater than (`>`).
    Gt,
    /// Greater than or equal (`>=`).
    Gte,
    /// Addition (`+`).
    Add,
    /// Subtraction (`-`).
    Subtract,
    /// Multiplication (`*`).
    Multiply,
    /// Division (`/`).
    Divide,
    /// Modulo (`%`).
    Modulo,
    /// String concatenation (`||`).
    Concat,
}

/// A set of typed expression edits guarded by the exact document source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlDocumentSemanticEditTransaction {
    /// Parser contract version that produced the selected node IDs.
    pub contract_version: u32,
    /// Exact source used to select the node IDs. A mismatch rejects all edits.
    pub expected_source: String,
    /// Typed edits interpreted against the original parsed tree.
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
    /// Projection position changes cannot preserve a positional grouping or sort reference.
    ProjectionPositionConflict,
    /// Removing a projection alias would rebind a same-query identifier reference.
    ProjectionAliasConflict,
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
            Self::ProjectionPositionConflict => {
                "projection edit would change a positional sort or grouping reference"
            }
            Self::ProjectionAliasConflict => {
                "projection alias edit would change a same-query binding or create an ambiguous output name"
            }
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
pub fn apply_sql_document_node_edit_transaction(
    current_source: &str,
    transaction: &SqlDocumentNodeEditTransaction,
) -> Result<SqlDocumentEditResult, SqlDocumentEditError> {
    if current_source != transaction.expected_source {
        return Err(SqlDocumentEditError::StaleSource);
    }
    if transaction.edits.len() > MAX_SQL_DOCUMENT_EDITS {
        return Err(SqlDocumentEditError::TooManyEdits);
    }
    if transaction.contract_version != SQL_DOCUMENT_CONTRACT_VERSION {
        return Err(SqlDocumentEditError::ContractVersionMismatch);
    }

    // Reparse the guarded source here instead of trusting an adapter-supplied
    // serialized `SqlDocument` tree. Node IDs are resolved only against the
    // Rust parser's current view of the exact source.
    let document = parse_sql_document(current_source);
    if document.status != SqlDocumentStatus::Parsed {
        if document.status == SqlDocumentStatus::TooLarge {
            return Err(SqlDocumentEditError::SourceTooLarge);
        }
        if document.status == SqlDocumentStatus::TooComplex {
            return Err(SqlDocumentEditError::SourceTooComplex);
        }
        return Err(SqlDocumentEditError::DocumentNotParsed);
    }

    // Resolve and validate the node identities before touching the supplied
    // replacement strings. Keep only their ranges here so an oversized batch
    // cannot force a second copy of each replacement before it is rejected.
    let mut resolved_ranges = Vec::with_capacity(transaction.edits.len());
    for node_edit in &transaction.edits {
        if resolved_ranges
            .iter()
            .any(|(node_id, _): &(u32, SqlSourceRange)| *node_id == node_edit.node_id)
        {
            return Err(SqlDocumentEditError::DuplicateNodeTarget);
        }
        let node = document
            .nodes
            .get(node_edit.node_id as usize)
            .filter(|node| node.id == node_edit.node_id)
            .ok_or(SqlDocumentEditError::NodeNotFound)?;
        if node.kind != node_edit.expected_kind {
            return Err(SqlDocumentEditError::NodeKindMismatch);
        }
        let range = node
            .range
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        document
            .source_slice(range)
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        resolved_ranges.push((node.id, range));
    }

    // Match the byte-edit transaction's range/overlap validation before
    // checking the aggregate output size. This keeps malformed or overlapping
    // node selections from being reported as a size error instead.
    let mut ordered_ranges: Vec<_> = resolved_ranges
        .iter()
        .zip(&transaction.edits)
        .map(|((_, range), edit)| (*range, edit.replacement.len()))
        .collect();
    ordered_ranges.sort_by_key(|(range, _)| (range.start, range.end));

    let mut previous: Option<SqlSourceRange> = None;
    let mut removed_bytes = 0usize;
    let mut added_bytes = 0usize;
    for (range, replacement_bytes) in ordered_ranges {
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
            if range.start < previous.end
                || (range.start == previous.start
                    && (range.start == range.end || previous.start == previous.end))
            {
                return Err(SqlDocumentEditError::OverlappingEdits);
            }
        }
        current_source
            .get(range.start..range.end)
            .ok_or(SqlDocumentEditError::NotUtf8Boundary)?;
        removed_bytes = removed_bytes
            .checked_add(range.end - range.start)
            .ok_or(SqlDocumentEditError::OutputTooLarge)?;
        added_bytes = added_bytes
            .checked_add(replacement_bytes)
            .ok_or(SqlDocumentEditError::OutputTooLarge)?;
        previous = Some(range);
    }

    current_source
        .len()
        .checked_sub(removed_bytes)
        .and_then(|size| size.checked_add(added_bytes))
        .filter(|size| *size <= MAX_SQL_DOCUMENT_BYTES)
        .ok_or(SqlDocumentEditError::OutputTooLarge)?;

    let edits = resolved_ranges
        .into_iter()
        .zip(&transaction.edits)
        .map(|((_, range), node_edit)| {
            let expected_text = document
                .source_slice(range)
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
            Ok(SqlDocumentEdit {
                range,
                expected_text: expected_text.to_owned(),
                replacement: node_edit.replacement.clone(),
            })
        })
        .collect::<Result<Vec<_>, SqlDocumentEditError>>()?;
    apply_sql_document_edit_transaction(
        current_source,
        &SqlDocumentEditTransaction {
            expected_source: transaction.expected_source.clone(),
            edits,
        },
    )
}

/// Apply typed semantic expression edits to parser-classified nodes.
///
/// Replacements are rendered by Rust and resolved only against the exact
/// source guarded by the transaction. Identifier and literal edits replace a
/// single expression leaf. Binary operator edits replace only the unique
/// operator token between its direct operands, preserving surrounding source
/// trivia. The returned inverse transaction remains guarded by the resulting
/// source. This API does not authorize, plan, or execute SQL.
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
    if transaction
        .edits
        .iter()
        .any(|edit| matches!(edit, SqlDocumentSemanticEdit::InitializeSelect { .. }))
    {
        let [SqlDocumentSemanticEdit::InitializeSelect { relation }] = transaction.edits.as_slice()
        else {
            return Err(SqlDocumentEditError::InvalidSemanticReplacement);
        };
        if document.status != SqlDocumentStatus::Empty {
            return Err(SqlDocumentEditError::InvalidSemanticReplacement);
        }
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
        match document.status {
            SqlDocumentStatus::TooLarge => return Err(SqlDocumentEditError::SourceTooLarge),
            SqlDocumentStatus::TooComplex => {
                return Err(SqlDocumentEditError::SourceTooComplex);
            }
            _ => return Err(SqlDocumentEditError::DocumentNotParsed),
        }
    }

    let mut targeted_nodes = Vec::with_capacity(transaction.edits.len());
    let mut edits = Vec::with_capacity(transaction.edits.len());
    for semantic_edit in &transaction.edits {
        let (node_id, expected_kind) = semantic_edit
            .target()
            .ok_or(SqlDocumentEditError::InvalidSemanticReplacement)?;
        if targeted_nodes.contains(&node_id) {
            return Err(SqlDocumentEditError::DuplicateNodeTarget);
        }
        targeted_nodes.push(node_id);

        let node = document
            .nodes
            .get(node_id as usize)
            .filter(|node| node.id == node_id)
            .ok_or(SqlDocumentEditError::NodeNotFound)?;

        if let SqlDocumentSemanticEdit::SetProjectionAlias { alias, .. } = semantic_edit {
            if node.kind != expected_kind || node.kind != SqlDocumentNodeKind::SelectItem {
                return Err(SqlDocumentEditError::NodeKindMismatch);
            }
            if alias.is_some() {
                ensure_unique_projection_alias(
                    document.nodes.as_slice(),
                    node,
                    alias.as_ref(),
                    &transaction.edits,
                )?;
            }
            let edit = set_projection_alias(&document, node, alias.as_ref())?;
            edits.push(edit);
            continue;
        }
        if node.kind != expected_kind {
            return Err(SqlDocumentEditError::NodeKindMismatch);
        }

        if let SqlDocumentSemanticEdit::AddSetOperationOperand {
            operator,
            quantifier,
            ..
        } = semantic_edit
        {
            edits.push(add_set_operation_operand(
                &document,
                node,
                *operator,
                *quantifier,
            )?);
            continue;
        }
        if let SqlDocumentSemanticEdit::RemoveSetOperationOperand { keep, .. } = semantic_edit {
            edits.push(remove_set_operation_operand(&document, node, *keep)?);
            continue;
        }
        if matches!(
            semantic_edit,
            SqlDocumentSemanticEdit::SwapSetOperationOperands { .. }
        ) {
            edits.extend(swap_set_operation_operands(&document, node)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::ReplaceSetOperationOperand {
            expression_node_id,
            expression_expected_kind,
            ..
        } = semantic_edit
        {
            let expression = document
                .nodes
                .get(*expression_node_id as usize)
                .filter(|candidate| candidate.id == *expression_node_id)
                .ok_or(SqlDocumentEditError::NodeNotFound)?;
            if expression.kind != *expression_expected_kind {
                return Err(SqlDocumentEditError::NodeKindMismatch);
            }
            edits.push(replace_set_operation_operand(&document, node, expression)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::RenameCommonTableExpression { name, .. } = semantic_edit {
            edits.extend(rename_cte_and_references(&document, node, name)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::AddCommonTableExpression { name, .. } = semantic_edit {
            edits.push(add_common_table_expression(&document, node, name)?);
            continue;
        }
        if matches!(
            semantic_edit,
            SqlDocumentSemanticEdit::RemoveCommonTableExpression { .. }
        ) {
            edits.push(remove_common_table_expression(&document, node)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::ReplaceCommonTableExpressionQuery {
            query_node_id, ..
        } = semantic_edit
        {
            let query = document
                .nodes
                .get(*query_node_id as usize)
                .filter(|candidate| candidate.id == *query_node_id)
                .ok_or(SqlDocumentEditError::NodeNotFound)?;
            if query.kind != SqlDocumentNodeKind::Query {
                return Err(SqlDocumentEditError::NodeKindMismatch);
            }
            edits.push(replace_common_table_expression_query(
                &document, node, query,
            )?);
            continue;
        }
        if let SqlDocumentSemanticEdit::MoveCommonTableExpression { to_index, .. } = semantic_edit {
            edits.push(move_common_table_expression(&document, node, *to_index)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::ReplacePredicateNegation { negated, .. } = semantic_edit {
            edits.push(replace_predicate_negation(&document, node, *negated)?);
            continue;
        }
        if matches!(semantic_edit, SqlDocumentSemanticEdit::AddCaseBranch { .. }) {
            edits.push(change_case_branch(&document, node, true)?);
            continue;
        }
        if matches!(
            semantic_edit,
            SqlDocumentSemanticEdit::RemoveCaseBranch { .. }
        ) {
            edits.push(change_case_branch(&document, node, false)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::SetCaseElse { has_else, .. } = semantic_edit {
            edits.push(set_case_else(&document, node, *has_else)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::SetCaseOperand {
            expression_node_id,
            expression_expected_kind,
            ..
        } = semantic_edit
        {
            let expression = match (expression_node_id, expression_expected_kind) {
                (Some(id), Some(kind)) => {
                    let expression = document
                        .nodes
                        .get(*id as usize)
                        .filter(|candidate| candidate.id == *id)
                        .ok_or(SqlDocumentEditError::NodeNotFound)?;
                    if expression.kind != *kind {
                        return Err(SqlDocumentEditError::NodeKindMismatch);
                    }
                    Some(expression)
                }
                (None, None) => None,
                _ => return Err(SqlDocumentEditError::InvalidSemanticReplacement),
            };
            edits.push(set_case_operand(&document, node, expression)?);
            continue;
        }
        if matches!(
            semantic_edit,
            SqlDocumentSemanticEdit::AddWhereClause { .. }
        ) {
            edits.push(add_where_clause(&document, node)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::AddFilterComparison {
            column,
            operator,
            literal,
            ..
        } = semantic_edit
        {
            edits.push(add_filter_comparison(
                &document,
                node,
                column,
                *operator,
                literal.as_ref(),
            )?);
            continue;
        }
        if matches!(
            semantic_edit,
            SqlDocumentSemanticEdit::AddHavingClause { .. }
        ) {
            edits.push(add_having_clause(&document, node)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::SetFilterPredicate {
            expression_node_id,
            expression_expected_kind,
            ..
        } = semantic_edit
        {
            let expression = document
                .nodes
                .get(*expression_node_id as usize)
                .filter(|candidate| candidate.id == *expression_node_id)
                .ok_or(SqlDocumentEditError::NodeNotFound)?;
            if expression.kind != *expression_expected_kind {
                return Err(SqlDocumentEditError::NodeKindMismatch);
            }
            edits.push(set_filter_predicate(&document, node, expression)?);
            continue;
        }
        if matches!(
            semantic_edit,
            SqlDocumentSemanticEdit::RemoveFilterClause { .. }
        ) {
            edits.push(remove_filter_clause(&document, node)?);
            continue;
        }
        if matches!(
            semantic_edit,
            SqlDocumentSemanticEdit::RemoveHavingClause { .. }
        ) {
            edits.push(remove_clause_node(
                &document,
                node,
                SqlDocumentNodeKind::HavingClause,
            )?);
            continue;
        }
        if let SqlDocumentSemanticEdit::SetSelectDistinct { distinct, .. } = semantic_edit {
            edits.push(set_select_distinct(&document, node, *distinct)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::SetLimitValue { value, .. } = semantic_edit {
            edits.push(set_query_clause_value(
                &document,
                node,
                QueryClauseValue::Limit(*value),
            )?);
            continue;
        }
        if matches!(semantic_edit, SqlDocumentSemanticEdit::RemoveLimit { .. }) {
            edits.push(remove_query_clause_value(
                &document,
                node,
                QueryClauseValueKind::Limit,
            )?);
            continue;
        }
        if let SqlDocumentSemanticEdit::SetOffsetValue { value, .. } = semantic_edit {
            edits.push(set_query_clause_value(
                &document,
                node,
                QueryClauseValue::Offset(*value),
            )?);
            continue;
        }
        if matches!(semantic_edit, SqlDocumentSemanticEdit::RemoveOffset { .. }) {
            edits.push(remove_query_clause_value(
                &document,
                node,
                QueryClauseValueKind::Offset,
            )?);
            continue;
        }
        if matches!(
            semantic_edit,
            SqlDocumentSemanticEdit::AddScalarSubqueryProjection { .. }
        ) {
            edits.push(add_scalar_subquery_projection(&document, node)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::WrapExpressionInSubqueryPredicate { template, .. } =
            semantic_edit
        {
            edits.push(wrap_expression_in_subquery_predicate(
                &document, node, *template,
            )?);
            continue;
        }
        if matches!(
            semantic_edit,
            SqlDocumentSemanticEdit::ReplaceExpressionWithExistsSubquery { .. }
        ) {
            edits.push(replace_expression_with_exists_subquery(&document, node)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::ReplaceTableFactor { factor, .. } = semantic_edit {
            edits.push(replace_table_factor(&document, node, factor)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::AddJoin { join, .. } = semantic_edit {
            edits.push(add_join(&document, node, join)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::AddFromRelation { factor, .. } = semantic_edit {
            edits.push(add_from_relation(&document, node, factor)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::ReplaceJoin { join, .. } = semantic_edit {
            edits.push(replace_join(&document, node, join)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::ComposeBinaryExpression {
            left_expression_node_id,
            left_expression_expected_kind,
            operator,
            right_expression_node_id,
            right_expression_expected_kind,
            ..
        } = semantic_edit
        {
            let left = document
                .nodes
                .get(*left_expression_node_id as usize)
                .filter(|candidate| candidate.id == *left_expression_node_id)
                .ok_or(SqlDocumentEditError::NodeNotFound)?;
            let right = document
                .nodes
                .get(*right_expression_node_id as usize)
                .filter(|candidate| candidate.id == *right_expression_node_id)
                .ok_or(SqlDocumentEditError::NodeNotFound)?;
            if left.kind != *left_expression_expected_kind
                || right.kind != *right_expression_expected_kind
            {
                return Err(SqlDocumentEditError::NodeKindMismatch);
            }
            edits.push(compose_binary_expression(
                &document, node, left, *operator, right,
            )?);
            continue;
        }
        if let SqlDocumentSemanticEdit::WrapExpressionInCast { target_type, .. } = semantic_edit {
            edits.push(wrap_expression_in_cast(&document, node, *target_type)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::WrapExpressionInAggregate {
            function, distinct, ..
        } = semantic_edit
        {
            edits.push(wrap_expression_in_aggregate(
                &document, node, *function, *distinct,
            )?);
            continue;
        }
        if let SqlDocumentSemanticEdit::SetAggregateFunction { function, .. } = semantic_edit {
            edits.push(set_aggregate_function(&document, node, *function)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::SetAggregateDistinct { distinct, .. } = semantic_edit {
            edits.push(set_aggregate_distinct(&document, node, *distinct)?);
            continue;
        }
        if matches!(
            semantic_edit,
            SqlDocumentSemanticEdit::RemoveAggregate { .. }
        ) {
            edits.push(remove_aggregate(&document, node)?);
            continue;
        }
        if matches!(
            semantic_edit,
            SqlDocumentSemanticEdit::WrapExpressionInCase { .. }
        ) {
            edits.push(wrap_expression_in_case(&document, node)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::WrapExpressionInPredicate { template, .. } = semantic_edit {
            edits.push(wrap_expression_in_predicate(&document, node, *template)?);
            continue;
        }
        if matches!(
            semantic_edit,
            SqlDocumentSemanticEdit::AppendInListValue { .. }
        ) {
            edits.push(edit_in_list_value(&document, node, InListEdit::Append)?);
            continue;
        }
        if matches!(
            semantic_edit,
            SqlDocumentSemanticEdit::RemoveLastInListValue { .. }
        ) {
            edits.push(edit_in_list_value(&document, node, InListEdit::RemoveLast)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::ReplaceExpressionFromNode {
            expression_node_id,
            expression_expected_kind,
            ..
        } = semantic_edit
        {
            let expression = document
                .nodes
                .get(*expression_node_id as usize)
                .filter(|candidate| candidate.id == *expression_node_id)
                .ok_or(SqlDocumentEditError::NodeNotFound)?;
            if expression.kind != *expression_expected_kind {
                return Err(SqlDocumentEditError::NodeKindMismatch);
            }
            edits.push(replace_expression_from_node(&document, node, expression)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::SetProjectionExpression {
            expression_node_id,
            expression_expected_kind,
            ..
        } = semantic_edit
        {
            let expression = document
                .nodes
                .get(*expression_node_id as usize)
                .filter(|candidate| candidate.id == *expression_node_id)
                .ok_or(SqlDocumentEditError::NodeNotFound)?;
            if expression.kind != *expression_expected_kind {
                return Err(SqlDocumentEditError::NodeKindMismatch);
            }
            edits.push(set_projection_expression(&document, node, expression)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::AddProjectionExpression {
            expression_node_id,
            expression_expected_kind,
            ..
        } = semantic_edit
        {
            let expression = document
                .nodes
                .get(*expression_node_id as usize)
                .filter(|candidate| candidate.id == *expression_node_id)
                .ok_or(SqlDocumentEditError::NodeNotFound)?;
            if expression.kind != *expression_expected_kind {
                return Err(SqlDocumentEditError::NodeKindMismatch);
            }
            edits.push(add_projection_expression(&document, node, expression)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::AddProjectionColumn { column, .. } = semantic_edit {
            edits.push(add_projection_column(&document, node, column)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::ExpandProjectionWildcard { columns, .. } = semantic_edit {
            edits.push(expand_projection_wildcard(&document, node, columns)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::AddGroupByExpression {
            expression_node_id,
            expression_expected_kind,
            ..
        } = semantic_edit
        {
            let expression = document
                .nodes
                .get(*expression_node_id as usize)
                .filter(|candidate| candidate.id == *expression_node_id)
                .ok_or(SqlDocumentEditError::NodeNotFound)?;
            if expression.kind != *expression_expected_kind {
                return Err(SqlDocumentEditError::NodeKindMismatch);
            }
            edits.push(add_group_by_expression(&document, node, expression)?);
            continue;
        }
        if matches!(
            semantic_edit,
            SqlDocumentSemanticEdit::RemoveGroupByItem { .. }
        ) {
            edits.push(remove_group_by_item(&document, node)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::MoveGroupByItem { to_index, .. } = semantic_edit {
            edits.push(move_group_by_item(&document, node, *to_index)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::AddOrderByExpression {
            expression_node_id,
            expression_expected_kind,
            direction,
            nulls_order,
            ..
        } = semantic_edit
        {
            let expression = document
                .nodes
                .get(*expression_node_id as usize)
                .filter(|candidate| candidate.id == *expression_node_id)
                .ok_or(SqlDocumentEditError::NodeNotFound)?;
            if expression.kind != *expression_expected_kind {
                return Err(SqlDocumentEditError::NodeKindMismatch);
            }
            edits.push(add_order_by_expression(
                &document,
                node,
                expression,
                *direction,
                *nulls_order,
            )?);
            continue;
        }
        if let SqlDocumentSemanticEdit::AddOrderByColumn {
            column,
            direction,
            nulls_order,
            ..
        } = semantic_edit
        {
            edits.push(add_order_by_column(
                &document,
                node,
                column,
                *direction,
                *nulls_order,
            )?);
            continue;
        }
        if let SqlDocumentSemanticEdit::SetOrderByExpression {
            expression_node_id,
            expression_expected_kind,
            ..
        } = semantic_edit
        {
            let expression = document
                .nodes
                .get(*expression_node_id as usize)
                .filter(|candidate| candidate.id == *expression_node_id)
                .ok_or(SqlDocumentEditError::NodeNotFound)?;
            if expression.kind != *expression_expected_kind {
                return Err(SqlDocumentEditError::NodeKindMismatch);
            }
            edits.push(set_order_by_expression(&document, node, expression)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::SetOrderByOptions {
            direction,
            nulls_order,
            ..
        } = semantic_edit
        {
            edits.push(set_order_by_options(
                &document,
                node,
                *direction,
                *nulls_order,
            )?);
            continue;
        }
        if matches!(
            semantic_edit,
            SqlDocumentSemanticEdit::RemoveOrderByItem { .. }
        ) {
            edits.push(remove_order_by_item(&document, node)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::MoveOrderByItem { to_index, .. } = semantic_edit {
            edits.push(move_order_by_item(&document, node, *to_index)?);
            continue;
        }
        if matches!(
            semantic_edit,
            SqlDocumentSemanticEdit::RemoveProjectionItem { .. }
        ) {
            ensure_projection_alias_unreferenced(&document, node)?;
            edits.extend(projection_position_edits(&document, node, None)?);
            edits.push(remove_projection_item(&document, node)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::MoveProjectionItem { to_index, .. } = semantic_edit {
            edits.extend(projection_position_edits(&document, node, Some(*to_index))?);
            edits.push(move_projection_item(&document, node, *to_index)?);
            continue;
        }
        if let SqlDocumentSemanticEdit::SetFunctionArgument {
            argument_index,
            expression_node_id,
            expression_expected_kind,
            ..
        } = semantic_edit
        {
            let expression = document
                .nodes
                .get(*expression_node_id as usize)
                .filter(|candidate| candidate.id == *expression_node_id)
                .ok_or(SqlDocumentEditError::NodeNotFound)?;
            if expression.kind != *expression_expected_kind {
                return Err(SqlDocumentEditError::NodeKindMismatch);
            }
            edits.push(set_function_argument(
                &document,
                node,
                *argument_index,
                expression,
            )?);
            continue;
        }
        if matches!(
            semantic_edit,
            SqlDocumentSemanticEdit::RemoveLastFunctionArgument { .. }
        ) {
            edits.push(remove_last_function_argument(&document, node)?);
            continue;
        }

        let (range, replacement) = match semantic_edit {
            SqlDocumentSemanticEdit::SetProjectionAlias { .. } => unreachable!("handled above"),
            SqlDocumentSemanticEdit::InitializeSelect { .. } => {
                return Err(SqlDocumentEditError::InvalidSemanticReplacement);
            }
            SqlDocumentSemanticEdit::AddFilterComparison { .. }
            | SqlDocumentSemanticEdit::AddOrderByColumn { .. } => {
                return Err(SqlDocumentEditError::InvalidSemanticReplacement);
            }
            SqlDocumentSemanticEdit::ReplaceIdentifier { parts, .. } => {
                let current_parts = match (&node.kind, node.semantic.as_ref()) {
                    (
                        SqlDocumentNodeKind::IdentifierExpression,
                        Some(SqlDocumentNodeData::Identifier { value, quote_style }),
                    ) => vec![SqlDocumentIdentifier {
                        value: value.clone(),
                        quote_style: *quote_style,
                    }],
                    (
                        SqlDocumentNodeKind::CompoundIdentifierExpression,
                        Some(SqlDocumentNodeData::CompoundIdentifier { parts }),
                    ) => parts.clone(),
                    _ if is_expression_node_kind(node.kind) => Vec::new(),
                    _ => return Err(SqlDocumentEditError::InvalidSemanticReplacement),
                };
                let range = node
                    .range
                    .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
                if range_has_comment(&document, range) {
                    return Err(SqlDocumentEditError::NodeRangeUnavailable);
                }
                let current_text = document
                    .source_slice(range)
                    .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
                if !current_parts.is_empty() {
                    validate_identifier_fragment(current_text, &current_parts)
                        .map_err(|_| SqlDocumentEditError::NodeRangeUnavailable)?;
                }

                let replacement = render_identifier_parts(parts)?;
                validate_identifier_fragment(&replacement, parts)
                    .map_err(|_| SqlDocumentEditError::InvalidSemanticReplacement)?;
                (range, replacement)
            }
            SqlDocumentSemanticEdit::ReplaceLiteral { literal, .. } => {
                if !is_expression_node_kind(node.kind) {
                    return Err(SqlDocumentEditError::InvalidSemanticReplacement);
                }
                let range = node
                    .range
                    .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
                if range_has_comment(&document, range) {
                    return Err(SqlDocumentEditError::NodeRangeUnavailable);
                }
                let current_text = document
                    .source_slice(range)
                    .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
                if let Some(SqlDocumentNodeData::Literal { literal_kind, .. }) =
                    node.semantic.as_ref()
                {
                    validate_literal_fragment(current_text, *literal_kind)
                        .map_err(|_| SqlDocumentEditError::NodeRangeUnavailable)?;
                }

                let (replacement, expected_kind) = render_semantic_literal(literal)?;
                validate_literal_fragment(&replacement, expected_kind)
                    .map_err(|_| SqlDocumentEditError::InvalidSemanticReplacement)?;
                (range, replacement)
            }
            SqlDocumentSemanticEdit::ReplaceBinaryOperator { operator, .. } => {
                if node.kind != SqlDocumentNodeKind::BinaryExpression
                    || !matches!(
                        node.semantic,
                        Some(SqlDocumentNodeData::BinaryOperator { .. })
                    )
                {
                    return Err(SqlDocumentEditError::InvalidSemanticReplacement);
                }
                (
                    binary_operator_token_range(&document, node)?,
                    operator.sql_spelling().to_owned(),
                )
            }
            SqlDocumentSemanticEdit::ReplaceUnarySign { sign, .. } => {
                let SqlDocumentNodeData::UnaryOperator { operator } = node
                    .semantic
                    .as_ref()
                    .ok_or(SqlDocumentEditError::InvalidSemanticReplacement)?
                else {
                    return Err(SqlDocumentEditError::InvalidSemanticReplacement);
                };
                if !matches!(operator.as_str(), "+" | "-" | "NOT") {
                    return Err(SqlDocumentEditError::InvalidSemanticReplacement);
                }
                (
                    unary_sign_token_range(&document, node)?,
                    sign.sql_spelling().to_owned(),
                )
            }
            SqlDocumentSemanticEdit::ReplaceSetOperation {
                operator,
                quantifier,
                ..
            } => replace_set_operation_fragment(&document, node, *operator, *quantifier)?,
            SqlDocumentSemanticEdit::ReplaceCastTargetType { target_type, .. } => {
                let Some(SqlDocumentNodeData::Cast {
                    cast_kind,
                    target_type: Some(current_target_type),
                }) = node.semantic.as_ref()
                else {
                    return Err(SqlDocumentEditError::InvalidSemanticReplacement);
                };
                let Some(range) =
                    cast_target_type_source_range(&document, node, cast_kind, current_target_type)
                else {
                    return Err(if node.range.is_some() {
                        SqlDocumentEditError::InvalidSemanticReplacement
                    } else {
                        SqlDocumentEditError::NodeRangeUnavailable
                    });
                };
                (range, target_type.sql_spelling().to_owned())
            }
            SqlDocumentSemanticEdit::SetParameterizedCastTarget { target, .. } => {
                let Some(SqlDocumentNodeData::Cast {
                    cast_kind,
                    target_type: Some(current_target_type),
                }) = node.semantic.as_ref()
                else {
                    return Err(SqlDocumentEditError::InvalidSemanticReplacement);
                };
                let Some(range) =
                    cast_target_type_source_range(&document, node, cast_kind, current_target_type)
                else {
                    return Err(if node.range.is_some() {
                        SqlDocumentEditError::InvalidSemanticReplacement
                    } else {
                        SqlDocumentEditError::NodeRangeUnavailable
                    });
                };
                (range, target.sql_spelling()?)
            }
            SqlDocumentSemanticEdit::WrapExpressionInCast { .. } => {
                unreachable!("CAST wrapper edits are expanded before single-node edits")
            }
            SqlDocumentSemanticEdit::WrapExpressionInAggregate { .. } => {
                unreachable!("aggregate wrapper edits are expanded before single-node edits")
            }
            SqlDocumentSemanticEdit::WrapExpressionInCase { .. } => {
                unreachable!("CASE wrapper edits are expanded before single-node edits")
            }
            SqlDocumentSemanticEdit::RenameCommonTableExpression { .. } => {
                unreachable!("CTE edits are expanded before single-node edits")
            }
            SqlDocumentSemanticEdit::AddCommonTableExpression { .. }
            | SqlDocumentSemanticEdit::RemoveCommonTableExpression { .. }
            | SqlDocumentSemanticEdit::ReplaceCommonTableExpressionQuery { .. }
            | SqlDocumentSemanticEdit::MoveCommonTableExpression { .. } => {
                unreachable!("CTE edits are expanded before single-node edits")
            }
            SqlDocumentSemanticEdit::AddSetOperationOperand { .. }
            | SqlDocumentSemanticEdit::RemoveSetOperationOperand { .. }
            | SqlDocumentSemanticEdit::SwapSetOperationOperands { .. }
            | SqlDocumentSemanticEdit::ReplaceSetOperationOperand { .. } => {
                unreachable!("set-operation structure edits are expanded before single-node edits")
            }
            SqlDocumentSemanticEdit::ReplacePredicateNegation { .. } => {
                unreachable!("predicate negation edits are resolved before node edits")
            }
            SqlDocumentSemanticEdit::AddCaseBranch { .. }
            | SqlDocumentSemanticEdit::RemoveCaseBranch { .. }
            | SqlDocumentSemanticEdit::SetCaseElse { .. }
            | SqlDocumentSemanticEdit::SetCaseOperand { .. }
            | SqlDocumentSemanticEdit::AddWhereClause { .. }
            | SqlDocumentSemanticEdit::SetFilterPredicate { .. }
            | SqlDocumentSemanticEdit::RemoveFilterClause { .. }
            | SqlDocumentSemanticEdit::AddHavingClause { .. }
            | SqlDocumentSemanticEdit::RemoveHavingClause { .. }
            | SqlDocumentSemanticEdit::SetSelectDistinct { .. }
            | SqlDocumentSemanticEdit::SetLimitValue { .. }
            | SqlDocumentSemanticEdit::RemoveLimit { .. }
            | SqlDocumentSemanticEdit::SetOffsetValue { .. }
            | SqlDocumentSemanticEdit::RemoveOffset { .. }
            | SqlDocumentSemanticEdit::AddScalarSubqueryProjection { .. }
            | SqlDocumentSemanticEdit::ReplaceTableFactor { .. }
            | SqlDocumentSemanticEdit::AddJoin { .. }
            | SqlDocumentSemanticEdit::AddFromRelation { .. }
            | SqlDocumentSemanticEdit::ReplaceJoin { .. }
            | SqlDocumentSemanticEdit::WrapExpressionInPredicate { .. }
            | SqlDocumentSemanticEdit::WrapExpressionInSubqueryPredicate { .. }
            | SqlDocumentSemanticEdit::ReplaceExpressionWithExistsSubquery { .. }
            | SqlDocumentSemanticEdit::AppendInListValue { .. }
            | SqlDocumentSemanticEdit::RemoveLastInListValue { .. }
            | SqlDocumentSemanticEdit::ComposeBinaryExpression { .. }
            | SqlDocumentSemanticEdit::ReplaceExpressionFromNode { .. }
            | SqlDocumentSemanticEdit::SetProjectionExpression { .. }
            | SqlDocumentSemanticEdit::AddProjectionExpression { .. }
            | SqlDocumentSemanticEdit::AddProjectionColumn { .. }
            | SqlDocumentSemanticEdit::ExpandProjectionWildcard { .. }
            | SqlDocumentSemanticEdit::AddGroupByExpression { .. }
            | SqlDocumentSemanticEdit::RemoveGroupByItem { .. }
            | SqlDocumentSemanticEdit::MoveGroupByItem { .. }
            | SqlDocumentSemanticEdit::AddOrderByExpression { .. }
            | SqlDocumentSemanticEdit::SetOrderByExpression { .. }
            | SqlDocumentSemanticEdit::SetOrderByOptions { .. }
            | SqlDocumentSemanticEdit::RemoveOrderByItem { .. }
            | SqlDocumentSemanticEdit::MoveOrderByItem { .. }
            | SqlDocumentSemanticEdit::RemoveProjectionItem { .. }
            | SqlDocumentSemanticEdit::MoveProjectionItem { .. }
            | SqlDocumentSemanticEdit::SetFunctionArgument { .. }
            | SqlDocumentSemanticEdit::RemoveLastFunctionArgument { .. }
            | SqlDocumentSemanticEdit::SetAggregateFunction { .. }
            | SqlDocumentSemanticEdit::SetAggregateDistinct { .. }
            | SqlDocumentSemanticEdit::RemoveAggregate { .. } => {
                unreachable!("structural expression edits are resolved before node edits")
            }
        };

        let expected_text = document
            .source_slice(range)
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        edits.push(SqlDocumentEdit {
            range,
            expected_text: expected_text.to_owned(),
            replacement,
        });
    }

    if edits.len() > MAX_SQL_DOCUMENT_EDITS {
        return Err(SqlDocumentEditError::TooManyEdits);
    }

    let result = apply_sql_document_edit_transaction(
        current_source,
        &SqlDocumentEditTransaction {
            expected_source: transaction.expected_source.clone(),
            edits,
        },
    )?;
    if result.document.status != SqlDocumentStatus::Parsed {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    Ok(result)
}

impl SqlDocumentSemanticEdit {
    fn target(&self) -> Option<(u32, SqlDocumentNodeKind)> {
        match self {
            Self::InitializeSelect { .. } => None,
            Self::AddFilterComparison {
                node_id,
                expected_kind,
                ..
            }
            | Self::AddOrderByColumn {
                node_id,
                expected_kind,
                ..
            }
            | Self::SetProjectionAlias {
                node_id,
                expected_kind,
                ..
            }
            | Self::ReplaceIdentifier {
                node_id,
                expected_kind,
                ..
            }
            | Self::ReplaceLiteral {
                node_id,
                expected_kind,
                ..
            }
            | Self::ReplaceBinaryOperator {
                node_id,
                expected_kind,
                ..
            }
            | Self::ReplaceUnarySign {
                node_id,
                expected_kind,
                ..
            }
            | Self::ReplaceSetOperation {
                node_id,
                expected_kind,
                ..
            }
            | Self::AddSetOperationOperand {
                node_id,
                expected_kind,
                ..
            }
            | Self::RemoveSetOperationOperand {
                node_id,
                expected_kind,
                ..
            }
            | Self::SwapSetOperationOperands {
                node_id,
                expected_kind,
            }
            | Self::ReplaceSetOperationOperand {
                node_id,
                expected_kind,
                ..
            }
            | Self::ReplaceCastTargetType {
                node_id,
                expected_kind,
                ..
            }
            | Self::SetParameterizedCastTarget {
                node_id,
                expected_kind,
                ..
            }
            | Self::WrapExpressionInCast {
                node_id,
                expected_kind,
                ..
            }
            | Self::WrapExpressionInAggregate {
                node_id,
                expected_kind,
                ..
            }
            | Self::SetAggregateFunction {
                node_id,
                expected_kind,
                ..
            }
            | Self::SetAggregateDistinct {
                node_id,
                expected_kind,
                ..
            }
            | Self::RemoveAggregate {
                node_id,
                expected_kind,
            }
            | Self::WrapExpressionInCase {
                node_id,
                expected_kind,
            }
            | Self::WrapExpressionInPredicate {
                node_id,
                expected_kind,
                ..
            }
            | Self::AppendInListValue {
                node_id,
                expected_kind,
            }
            | Self::RemoveLastInListValue {
                node_id,
                expected_kind,
            }
            | Self::ReplaceTableFactor {
                node_id,
                expected_kind,
                ..
            }
            | Self::AddJoin {
                node_id,
                expected_kind,
                ..
            }
            | Self::AddFromRelation {
                node_id,
                expected_kind,
                ..
            }
            | Self::ReplaceJoin {
                node_id,
                expected_kind,
                ..
            }
            | Self::RenameCommonTableExpression {
                node_id,
                expected_kind,
                ..
            }
            | Self::AddCommonTableExpression {
                node_id,
                expected_kind,
                ..
            }
            | Self::RemoveCommonTableExpression {
                node_id,
                expected_kind,
            }
            | Self::ReplaceCommonTableExpressionQuery {
                node_id,
                expected_kind,
                ..
            }
            | Self::MoveCommonTableExpression {
                node_id,
                expected_kind,
                ..
            }
            | Self::ReplacePredicateNegation {
                node_id,
                expected_kind,
                ..
            }
            | Self::AddCaseBranch {
                node_id,
                expected_kind,
            }
            | Self::RemoveCaseBranch {
                node_id,
                expected_kind,
            }
            | Self::SetCaseElse {
                node_id,
                expected_kind,
                ..
            }
            | Self::SetCaseOperand {
                node_id,
                expected_kind,
                ..
            }
            | Self::AddWhereClause {
                node_id,
                expected_kind,
            }
            | Self::SetFilterPredicate {
                node_id,
                expected_kind,
                ..
            }
            | Self::RemoveFilterClause {
                node_id,
                expected_kind,
            }
            | Self::ComposeBinaryExpression {
                node_id,
                expected_kind,
                ..
            }
            | Self::ReplaceExpressionFromNode {
                node_id,
                expected_kind,
                ..
            }
            | Self::SetProjectionExpression {
                node_id,
                expected_kind,
                ..
            }
            | Self::AddProjectionExpression {
                node_id,
                expected_kind,
                ..
            }
            | Self::AddProjectionColumn {
                node_id,
                expected_kind,
                ..
            }
            | Self::ExpandProjectionWildcard {
                node_id,
                expected_kind,
                ..
            }
            | Self::AddScalarSubqueryProjection {
                node_id,
                expected_kind,
            }
            | Self::WrapExpressionInSubqueryPredicate {
                node_id,
                expected_kind,
                ..
            }
            | Self::ReplaceExpressionWithExistsSubquery {
                node_id,
                expected_kind,
            }
            | Self::AddGroupByExpression {
                node_id,
                expected_kind,
                ..
            }
            | Self::RemoveGroupByItem {
                node_id,
                expected_kind,
            }
            | Self::MoveGroupByItem {
                node_id,
                expected_kind,
                ..
            }
            | Self::AddOrderByExpression {
                node_id,
                expected_kind,
                ..
            }
            | Self::SetOrderByExpression {
                node_id,
                expected_kind,
                ..
            }
            | Self::SetOrderByOptions {
                node_id,
                expected_kind,
                ..
            }
            | Self::RemoveOrderByItem {
                node_id,
                expected_kind,
            }
            | Self::MoveOrderByItem {
                node_id,
                expected_kind,
                ..
            }
            | Self::SetSelectDistinct {
                node_id,
                expected_kind,
                ..
            }
            | Self::SetLimitValue {
                node_id,
                expected_kind,
                ..
            }
            | Self::RemoveLimit {
                node_id,
                expected_kind,
            }
            | Self::SetOffsetValue {
                node_id,
                expected_kind,
                ..
            }
            | Self::RemoveOffset {
                node_id,
                expected_kind,
            }
            | Self::AddHavingClause {
                node_id,
                expected_kind,
            }
            | Self::RemoveHavingClause {
                node_id,
                expected_kind,
            }
            | Self::RemoveProjectionItem {
                node_id,
                expected_kind,
            }
            | Self::MoveProjectionItem {
                node_id,
                expected_kind,
                ..
            }
            | Self::SetFunctionArgument {
                node_id,
                expected_kind,
                ..
            }
            | Self::RemoveLastFunctionArgument {
                node_id,
                expected_kind,
                ..
            } => Some((*node_id, *expected_kind)),
        }
    }
}

impl SqlDocumentBinaryOperator {
    fn sql_spelling(self) -> &'static str {
        match self {
            Self::And => "AND",
            Self::Or => "OR",
            Self::BitwiseAnd => "&",
            Self::BitwiseOr => "|",
            Self::ShiftLeft => "<<",
            Self::ShiftRight => ">>",
            Self::Eq => "=",
            Self::NotEq => "<>",
            Self::Lt => "<",
            Self::Lte => "<=",
            Self::Gt => ">",
            Self::Gte => ">=",
            Self::Add => "+",
            Self::Subtract => "-",
            Self::Multiply => "*",
            Self::Divide => "/",
            Self::Modulo => "%",
            Self::Concat => "||",
        }
    }
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

fn validate_identifier_fragment(
    fragment: &str,
    expected_parts: &[SqlDocumentIdentifier],
) -> Result<(), SqlDocumentEditError> {
    if expected_parts.is_empty() {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let (document, fragment_range) = parsed_expression_fragment(fragment)?;
    let matching_nodes = document
        .nodes
        .iter()
        .filter(|node| {
            node.range == Some(fragment_range)
                && match (&node.kind, node.semantic.as_ref()) {
                    (
                        SqlDocumentNodeKind::IdentifierExpression,
                        Some(SqlDocumentNodeData::Identifier { value, quote_style }),
                    ) => {
                        expected_parts.len() == 1
                            && expected_parts[0].value == *value
                            && expected_parts[0].quote_style == *quote_style
                    }
                    (
                        SqlDocumentNodeKind::CompoundIdentifierExpression,
                        Some(SqlDocumentNodeData::CompoundIdentifier { parts }),
                    ) => parts == expected_parts,
                    _ => false,
                }
        })
        .count();
    if matching_nodes == 1 {
        Ok(())
    } else {
        Err(SqlDocumentEditError::InvalidSemanticReplacement)
    }
}

fn render_semantic_literal(
    literal: &SqlDocumentSemanticLiteral,
) -> Result<(String, SqlDocumentLiteralKind), SqlDocumentEditError> {
    match literal {
        SqlDocumentSemanticLiteral::Number { value } => {
            if value.len() > MAX_SQL_DOCUMENT_BYTES || !valid_sql_number_literal(value) {
                return Err(SqlDocumentEditError::InvalidSemanticReplacement);
            }
            Ok((value.clone(), SqlDocumentLiteralKind::Number))
        }
        SqlDocumentSemanticLiteral::String { value } => {
            if value.len() > MAX_SQL_DOCUMENT_BYTES || value.contains('\0') {
                return Err(SqlDocumentEditError::InvalidSemanticReplacement);
            }
            let mut rendered = String::with_capacity(value.len().saturating_add(2));
            rendered.push('\'');
            for character in value.chars() {
                rendered.push(character);
                if character == '\'' {
                    rendered.push('\'');
                }
            }
            rendered.push('\'');
            Ok((rendered, SqlDocumentLiteralKind::String))
        }
        SqlDocumentSemanticLiteral::Boolean { value } => Ok((
            if *value { "TRUE" } else { "FALSE" }.to_owned(),
            SqlDocumentLiteralKind::Boolean,
        )),
        SqlDocumentSemanticLiteral::Null => Ok(("NULL".to_owned(), SqlDocumentLiteralKind::Null)),
        SqlDocumentSemanticLiteral::NamedParameter { name } => {
            if name.len() >= MAX_SQL_DOCUMENT_BYTES || !valid_sql_parameter_name(name) {
                return Err(SqlDocumentEditError::InvalidSemanticReplacement);
            }
            Ok((format!("${name}"), SqlDocumentLiteralKind::Placeholder))
        }
    }
}

fn valid_sql_number_literal(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || !bytes.is_ascii() {
        return false;
    }

    let mut index = 0;
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

fn validate_literal_fragment(
    fragment: &str,
    expected_kind: SqlDocumentLiteralKind,
) -> Result<(), SqlDocumentEditError> {
    let (document, fragment_range) = parsed_expression_fragment(fragment)?;
    let matching_nodes = document
        .nodes
        .iter()
        .filter(|node| {
            node.kind == SqlDocumentNodeKind::Value
                && node.range == Some(fragment_range)
                && matches!(
                    node.semantic,
                    Some(SqlDocumentNodeData::Literal { literal_kind, .. })
                        if literal_kind == expected_kind
                )
        })
        .count();
    if matching_nodes == 1 {
        Ok(())
    } else {
        Err(SqlDocumentEditError::InvalidSemanticReplacement)
    }
}

fn parsed_expression_fragment(
    fragment: &str,
) -> Result<(SqlDocument, SqlSourceRange), SqlDocumentEditError> {
    if fragment.is_empty() || fragment.len() > MAX_SQL_DOCUMENT_BYTES.saturating_sub(7) {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let source = format!("SELECT {fragment}");
    let document = parse_sql_document(&source);
    if document.status != SqlDocumentStatus::Parsed {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    Ok((
        document,
        SqlSourceRange {
            start: "SELECT ".len(),
            end: source.len(),
        },
    ))
}

fn binary_operator_token_range(
    document: &SqlDocument,
    node: &SqlDocumentNode,
) -> Result<SqlSourceRange, SqlDocumentEditError> {
    let expression_range = node
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let mut operands = document
        .nodes
        .iter()
        .filter(|candidate| {
            candidate.parent == Some(node.id) && is_expression_node_kind(candidate.kind)
        })
        .collect::<Vec<_>>();
    if operands.len() != 2 {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    operands.sort_by_key(|operand| operand.range.map(|range| range.start));
    let left = operands[0]
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let right = operands[1]
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if left.start < expression_range.start
        || right.end > expression_range.end
        || left.end > right.start
    {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }

    let between = document
        .tokens
        .iter()
        .filter(|token| {
            token.range.start >= left.end
                && token.range.end <= right.start
                && !matches!(
                    token.kind,
                    SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
                )
        })
        .collect::<Vec<_>>();
    if between.len() != 1 {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let operator = between[0];
    if !matches!(
        operator.kind,
        SqlDocumentTokenKind::Operator | SqlDocumentTokenKind::Word
    ) || operator.range.start < expression_range.start
        || operator.range.end > expression_range.end
    {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let actual_operator = document
        .source_slice(operator.range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let SqlDocumentNodeData::BinaryOperator {
        operator: expected_operator,
    } = node
        .semantic
        .as_ref()
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?
    else {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    };
    if !binary_operator_token_matches(expected_operator, actual_operator) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    Ok(operator.range)
}

fn binary_operator_token_matches(expected: &str, actual: &str) -> bool {
    expected.eq_ignore_ascii_case(actual) || matches!((expected, actual), ("<>", "!="))
}

fn unary_sign_token_range(
    document: &SqlDocument,
    node: &SqlDocumentNode,
) -> Result<SqlSourceRange, SqlDocumentEditError> {
    if node.kind != SqlDocumentNodeKind::UnaryExpression
        || !matches!(
            node.semantic,
            Some(SqlDocumentNodeData::UnaryOperator { ref operator }) if matches!(operator.as_str(), "+" | "-" | "NOT")
        )
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let mut operands = document
        .nodes
        .iter()
        .filter(|candidate| {
            candidate.parent == Some(node.id) && is_expression_node_kind(candidate.kind)
        })
        .filter_map(|candidate| candidate.range)
        .collect::<Vec<_>>();
    if operands.len() != 1 {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let operand = operands.remove(0);
    let operator_index = previous_nontrivia_token(
        &document.tokens,
        document
            .tokens
            .iter()
            .position(|token| token.range.start >= operand.start)
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?,
    )
    .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let operator_token = &document.tokens[operator_index];
    if operator_token.range.end > operand.start
        || !matches!(
            operator_token.kind,
            SqlDocumentTokenKind::Operator | SqlDocumentTokenKind::Word
        )
        || !matches!(
            token_text(&document.source, operator_token),
            Some("+" | "-" | "NOT")
        )
    {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let SqlDocumentNodeData::UnaryOperator { operator: expected } = node
        .semantic
        .as_ref()
        .ok_or(SqlDocumentEditError::InvalidSemanticReplacement)?
    else {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    };
    if !token_text(&document.source, operator_token)
        .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
    {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    Ok(operator_token.range)
}

fn replace_predicate_negation(
    document: &SqlDocument,
    node: &SqlDocumentNode,
    negated: bool,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    let Some(SqlDocumentNodeData::Predicate {
        operator,
        negated: current_negation,
    }) = node.semantic.as_ref()
    else {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    };
    if *current_negation == negated {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }

    let mut operands = document
        .nodes
        .iter()
        .filter(|candidate| {
            candidate.parent == Some(node.id) && is_expression_node_kind(candidate.kind)
        })
        .filter_map(|candidate| candidate.range)
        .collect::<Vec<_>>();
    operands.sort_by_key(|range| (range.start, range.end));
    let left_operand = operands
        .first()
        .copied()
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;

    let is_predicate = matches!(
        operator.as_str(),
        "is_null" | "is_true" | "is_false" | "is_unknown" | "is_distinct_from"
    );
    let operator_word = if is_predicate {
        "IS"
    } else {
        match operator.as_str() {
            "in" | "in_subquery" | "in_unnest" => "IN",
            "between" => "BETWEEN",
            "like" => "LIKE",
            "ilike" => "ILIKE",
            "similar_to" => "SIMILAR",
            "rlike" => "RLIKE",
            _ => return Err(SqlDocumentEditError::InvalidSemanticReplacement),
        }
    };
    let first_after_operand = document
        .tokens
        .iter()
        .enumerate()
        .find(|(_, token)| {
            token.range.start >= left_operand.end
                && !matches!(
                    token.kind,
                    SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
                )
        })
        .map(|(index, _)| index)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let first_text = token_text(&document.source, &document.tokens[first_after_operand])
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let (operator_token_index, negation_token) = if is_predicate {
        if !first_text.eq_ignore_ascii_case("IS") {
            return Err(SqlDocumentEditError::NodeRangeUnavailable);
        }
        let next = next_nontrivia_token(&document.tokens, first_after_operand);
        let next_is_not = next.is_some_and(|index| {
            token_text(&document.source, &document.tokens[index])
                .is_some_and(|text| text.eq_ignore_ascii_case("NOT"))
        });
        match (*current_negation, next_is_not) {
            (true, true) => (first_after_operand, next),
            (false, false) => (first_after_operand, None),
            _ => return Err(SqlDocumentEditError::NodeRangeUnavailable),
        }
    } else if *current_negation {
        if !first_text.eq_ignore_ascii_case("NOT") {
            return Err(SqlDocumentEditError::NodeRangeUnavailable);
        }
        let operator_index = next_nontrivia_token(&document.tokens, first_after_operand)
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        (operator_index, Some(first_after_operand))
    } else {
        (first_after_operand, None)
    };
    let operator_token = &document.tokens[operator_token_index];
    if operator_token.kind != SqlDocumentTokenKind::Word
        || !token_text(&document.source, operator_token)
            .is_some_and(|text| text.eq_ignore_ascii_case(operator_word))
    {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    if operator == "similar_to" {
        let to_index = next_nontrivia_token(&document.tokens, operator_token_index)
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        if !token_text(&document.source, &document.tokens[to_index])
            .is_some_and(|text| text.eq_ignore_ascii_case("TO"))
        {
            return Err(SqlDocumentEditError::NodeRangeUnavailable);
        }
    }

    let (range, replacement) = if negated {
        if negation_token.is_some() {
            return Err(SqlDocumentEditError::InvalidSemanticReplacement);
        }
        let insertion = if is_predicate {
            operator_token.range.end
        } else {
            operator_token.range.start
        };
        (
            SqlSourceRange {
                start: insertion,
                end: insertion,
            },
            if is_predicate {
                " NOT".to_owned()
            } else {
                "NOT ".to_owned()
            },
        )
    } else {
        let index = negation_token.ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        (document.tokens[index].range, String::new())
    };
    let expected_text = document
        .source_slice(range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    Ok(SqlDocumentEdit {
        range,
        expected_text: expected_text.to_owned(),
        replacement,
    })
}

fn wrap_expression_in_predicate(
    document: &SqlDocument,
    node: &SqlDocumentNode,
    template: SqlDocumentPredicateTemplate,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if !is_expression_node_kind(node.kind) {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let range = node
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if range_has_comment(document, range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let operand = document
        .source_slice(range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if operand.trim().is_empty() {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let replacement = match template {
        SqlDocumentPredicateTemplate::In => format!("({operand}) IN (NULL)"),
        SqlDocumentPredicateTemplate::NotIn => format!("({operand}) NOT IN (NULL)"),
        SqlDocumentPredicateTemplate::Between => format!("({operand}) BETWEEN NULL AND NULL"),
        SqlDocumentPredicateTemplate::NotBetween => {
            format!("({operand}) NOT BETWEEN NULL AND NULL")
        }
        SqlDocumentPredicateTemplate::Like => format!("({operand}) LIKE ''"),
        SqlDocumentPredicateTemplate::NotLike => format!("({operand}) NOT LIKE ''"),
        SqlDocumentPredicateTemplate::IsNull => format!("({operand}) IS NULL"),
        SqlDocumentPredicateTemplate::IsNotNull => format!("({operand}) IS NOT NULL"),
    };
    let expected_text = document
        .source_slice(range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?
        .to_owned();
    Ok(SqlDocumentEdit {
        range,
        expected_text,
        replacement,
    })
}

fn wrap_expression_in_subquery_predicate(
    document: &SqlDocument,
    node: &SqlDocumentNode,
    template: SqlDocumentSubqueryPredicateTemplate,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if !is_expression_node_kind(node.kind)
        || matches!(node.kind, SqlDocumentNodeKind::OtherExpression)
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let range = node
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if range_has_comment(document, range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let operand = document
        .source_slice(range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if operand.trim().is_empty() {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let predicate = match template {
        SqlDocumentSubqueryPredicateTemplate::In => "IN",
        SqlDocumentSubqueryPredicateTemplate::NotIn => "NOT IN",
    };
    Ok(SqlDocumentEdit {
        range,
        expected_text: operand.to_owned(),
        replacement: format!("({operand}) {predicate} (SELECT 1)"),
    })
}

fn replace_expression_with_exists_subquery(
    document: &SqlDocument,
    node: &SqlDocumentNode,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if !is_expression_node_kind(node.kind)
        || matches!(node.kind, SqlDocumentNodeKind::OtherExpression)
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let range = node
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if range_has_comment(document, range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let expected_text = document
        .source_slice(range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if expected_text.trim().is_empty() {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    Ok(SqlDocumentEdit {
        range,
        expected_text: expected_text.to_owned(),
        replacement: "EXISTS (SELECT 1)".to_owned(),
    })
}

#[derive(Clone, Copy)]
enum InListEdit {
    Append,
    RemoveLast,
}

fn edit_in_list_value(
    document: &SqlDocument,
    node: &SqlDocumentNode,
    operation: InListEdit,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    let Some(SqlDocumentNodeData::Predicate { operator, .. }) = node.semantic.as_ref() else {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    };
    if operator != "in" || node.kind != SqlDocumentNodeKind::PredicateExpression {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let node_range = node
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let depths = token_depths(&document.source, &document.tokens);
    let mut candidates = Vec::new();
    for (index, token) in document.tokens.iter().enumerate() {
        if token.range.start < node_range.start || token.range.end > node_range.end {
            continue;
        }
        if !token_is_keyword_at(&document.source, &document.tokens, index, "IN") {
            continue;
        }
        let open_index = next_nontrivia_token(&document.tokens, index)
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        if token_text(&document.source, &document.tokens[open_index]) != Some("(") {
            continue;
        }
        let close_index = matching_close_paren(&document.source, &document.tokens, open_index)
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        if document.tokens[close_index].range.start <= node_range.end {
            candidates.push((depths[index], open_index, close_index));
        }
    }
    let (_, open_index, close_index) = candidates
        .into_iter()
        .min_by_key(|(depth, _, _)| *depth)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let list_range = SqlSourceRange {
        start: document.tokens[open_index].range.start,
        end: document.tokens[close_index].range.end,
    };
    if has_comment_within_range(&document.tokens, list_range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let open_depth = depths[open_index];
    let commas = (open_index + 1..close_index)
        .filter(|index| {
            depths[*index] == open_depth + 1
                && token_text(&document.source, &document.tokens[*index]) == Some(",")
        })
        .collect::<Vec<_>>();
    match operation {
        InListEdit::Append => {
            let has_values = document.tokens[open_index + 1..close_index]
                .iter()
                .any(|token| {
                    !matches!(
                        token.kind,
                        SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
                    )
                });
            let insertion = document.tokens[close_index].range.start;
            Ok(SqlDocumentEdit {
                range: SqlSourceRange {
                    start: insertion,
                    end: insertion,
                },
                expected_text: String::new(),
                replacement: if has_values {
                    ", NULL".to_owned()
                } else {
                    "NULL".to_owned()
                },
            })
        }
        InListEdit::RemoveLast => {
            let comma_index = commas
                .last()
                .copied()
                .ok_or(SqlDocumentEditError::InvalidSemanticReplacement)?;
            let last_value_index = previous_nontrivia_token(&document.tokens, close_index)
                .filter(|index| *index > comma_index)
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
            let range = SqlSourceRange {
                start: document.tokens[comma_index].range.start,
                end: document.tokens[last_value_index].range.end,
            };
            let expected_text = document
                .source_slice(range)
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?
                .to_owned();
            Ok(SqlDocumentEdit {
                range,
                expected_text,
                replacement: String::new(),
            })
        }
    }
}

fn add_where_clause(
    document: &SqlDocument,
    select: &SqlDocumentNode,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if select.kind != SqlDocumentNodeKind::Select
        || document.nodes.iter().any(|node| {
            node.parent == Some(select.id) && node.kind == SqlDocumentNodeKind::WhereClause
        })
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let select_range = select
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let query_id = select.parent.filter(|parent_id| {
        document
            .nodes
            .get(*parent_id as usize)
            .is_some_and(|node| node.id == *parent_id && node.kind == SqlDocumentNodeKind::Query)
    });
    let later_clause_start = document
        .nodes
        .iter()
        .filter(|node| {
            let is_later_clause = matches!(
                node.kind,
                SqlDocumentNodeKind::GroupByClause
                    | SqlDocumentNodeKind::HavingClause
                    | SqlDocumentNodeKind::OrderByClause
                    | SqlDocumentNodeKind::LimitClause
                    | SqlDocumentNodeKind::OffsetClause
            );
            is_later_clause
                && (node.parent == Some(select.id) || node.parent == query_id)
                && node
                    .range
                    .is_some_and(|range| range.start >= select_range.start)
        })
        .filter_map(|node| node.range.map(|range| range.start))
        .min();
    let offset = later_clause_start.unwrap_or(select_range.end);
    if offset < select_range.start || offset > document.source.len() {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let replacement = if later_clause_start.is_some() {
        "WHERE TRUE ".to_owned()
    } else {
        " WHERE TRUE".to_owned()
    };
    source_insertion(document, offset, replacement)
}

fn add_filter_comparison(
    document: &SqlDocument,
    select: &SqlDocumentNode,
    column: &SqlDocumentColumnReference,
    operator: SqlDocumentFilterOperator,
    literal: Option<&SqlDocumentSemanticLiteral>,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if select.kind != SqlDocumentNodeKind::Select || operator.needs_value() != literal.is_some() {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let left = render_column_reference(column)?;
    let right = literal
        .map(render_semantic_literal)
        .transpose()?
        .map(|(sql, _)| sql);
    let condition = right.map_or_else(
        || format!("{left} {}", operator.sql_spelling()),
        |value| format!("{left} {} {value}", operator.sql_spelling()),
    );
    let where_clause = document.nodes.iter().find(|candidate| {
        candidate.parent == Some(select.id) && candidate.kind == SqlDocumentNodeKind::WhereClause
    });
    if let Some(clause) = where_clause {
        let range = clause
            .range
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        if document.tokens.iter().any(|token| {
            matches!(token.kind, SqlDocumentTokenKind::Comment)
                && token.range.start >= range.start
                && token.range.end <= range.end
        }) {
            return Err(SqlDocumentEditError::InvalidSemanticReplacement);
        }
        let where_token = document
            .tokens
            .iter()
            .find(|token| {
                token.range.start >= range.start
                    && token.range.end <= range.end
                    && token_text(&document.source, token)
                        .is_some_and(|text| text.eq_ignore_ascii_case("WHERE"))
            })
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        let predicate = document
            .source
            .get(where_token.range.end..range.end)
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?
            .trim();
        if predicate.is_empty() {
            return Err(SqlDocumentEditError::InvalidSemanticReplacement);
        }
        let expected_text = document
            .source_slice(range)
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        return Ok(SqlDocumentEdit {
            range,
            expected_text: expected_text.to_owned(),
            replacement: format!("WHERE ({predicate}) AND ({condition})"),
        });
    }
    let mut edit = add_where_clause(document, select)?;
    edit.replacement = edit
        .replacement
        .replace("WHERE TRUE", &format!("WHERE {condition}"));
    Ok(edit)
}

fn add_order_by_column(
    document: &SqlDocument,
    query: &SqlDocumentNode,
    column: &SqlDocumentColumnReference,
    direction: SqlDocumentOrderDirection,
    nulls_order: Option<SqlDocumentNullsOrder>,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if query.kind != SqlDocumentNodeKind::Query {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let expression = render_column_reference(column)?;
    let clause = document.nodes.iter().find(|candidate| {
        candidate.parent == Some(query.id) && candidate.kind == SqlDocumentNodeKind::OrderByClause
    });
    if let Some(clause) = clause {
        return source_insertion(
            document,
            clause
                .range
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?
                .end,
            format!(
                ", {expression} {}{}",
                direction.sql_spelling(),
                nulls_order.map_or("", SqlDocumentNullsOrder::sql_suffix)
            ),
        );
    }
    let query_range = query
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let next_clause = document
        .nodes
        .iter()
        .filter(|candidate| {
            candidate.parent == Some(query.id)
                && matches!(
                    candidate.kind,
                    SqlDocumentNodeKind::LimitClause | SqlDocumentNodeKind::OffsetClause
                )
        })
        .filter_map(|candidate| candidate.range)
        .min_by_key(|range| range.start);
    let insert_at = next_clause.map_or(query_range.end, |range| range.start);
    let preceding_is_space = document
        .source
        .get(..insert_at)
        .and_then(|prefix| prefix.chars().next_back())
        .is_some_and(char::is_whitespace);
    let prefix = if preceding_is_space { "" } else { " " };
    let suffix = if next_clause.is_some() { " " } else { "" };
    source_insertion(
        document,
        insert_at,
        format!(
            "{prefix}ORDER BY {expression} {}{}{suffix}",
            direction.sql_spelling(),
            nulls_order.map_or("", SqlDocumentNullsOrder::sql_suffix)
        ),
    )
}

fn add_having_clause(
    document: &SqlDocument,
    select: &SqlDocumentNode,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if select.kind != SqlDocumentNodeKind::Select
        || document.nodes.iter().any(|node| {
            node.parent == Some(select.id) && node.kind == SqlDocumentNodeKind::HavingClause
        })
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let select_range = select
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let query_id =
        query_ancestor_id(document, select.id).ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let next_clause = document
        .nodes
        .iter()
        .filter(|node| node.parent == Some(query_id))
        .filter(|node| {
            matches!(
                node.kind,
                SqlDocumentNodeKind::OrderByClause
                    | SqlDocumentNodeKind::LimitClause
                    | SqlDocumentNodeKind::OffsetClause
            )
        })
        .filter_map(|node| node.range)
        .filter(|range| range.start >= select_range.end)
        .min_by_key(|range| range.start);
    let insert_at = next_clause.map_or(select_range.end, |range| range.start);
    if insert_at < select_range.start || insert_at > document.source.len() {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let preceding_is_space = document
        .source
        .get(..insert_at)
        .and_then(|prefix| prefix.chars().next_back())
        .is_some_and(char::is_whitespace);
    let prefix = if preceding_is_space { "" } else { " " };
    let suffix = if next_clause.is_some() { " " } else { "" };
    source_insertion(document, insert_at, format!("{prefix}HAVING TRUE{suffix}"))
}

fn remove_clause_node(
    document: &SqlDocument,
    clause: &SqlDocumentNode,
    expected_kind: SqlDocumentNodeKind,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if clause.kind != expected_kind
        || !matches!(
            expected_kind,
            SqlDocumentNodeKind::WhereClause | SqlDocumentNodeKind::HavingClause
        )
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let range = clause
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if range_has_comment(document, range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    source_removal(document, range)
}

fn set_filter_predicate(
    document: &SqlDocument,
    clause: &SqlDocumentNode,
    expression: &SqlDocumentNode,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if !matches!(
        clause.kind,
        SqlDocumentNodeKind::WhereClause | SqlDocumentNodeKind::HavingClause
    ) || !is_expression_node_kind(expression.kind)
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let select_id = clause
        .parent
        .filter(|parent| {
            document
                .nodes
                .get(*parent as usize)
                .is_some_and(|candidate| candidate.kind == SqlDocumentNodeKind::Select)
        })
        .ok_or(SqlDocumentEditError::InvalidSemanticReplacement)?;
    if query_ancestor_id(document, expression.id) != query_ancestor_id(document, select_id) {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let predicate = document
        .nodes
        .iter()
        .filter(|candidate| candidate.parent == Some(clause.id))
        .filter(|candidate| is_expression_node_kind(candidate.kind))
        .min_by_key(|candidate| {
            candidate
                .range
                .map(|range| range.end.saturating_sub(range.start))
                .unwrap_or(usize::MAX)
        })
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let target_range = predicate
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let expression_range = expression
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if range_has_comment(document, target_range)
        || range_has_comment(document, expression_range)
        || (expression_range.start <= target_range.start
            && target_range.end <= expression_range.end)
    {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let expected_text = document
        .source_slice(target_range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let replacement = document
        .source_slice(expression_range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if expected_text == replacement || replacement.trim().is_empty() {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    Ok(SqlDocumentEdit {
        range: target_range,
        expected_text: expected_text.to_owned(),
        replacement: replacement.to_owned(),
    })
}

fn remove_filter_clause(
    document: &SqlDocument,
    clause: &SqlDocumentNode,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    remove_clause_node(document, clause, clause.kind)
}

fn set_select_distinct(
    document: &SqlDocument,
    select: &SqlDocumentNode,
    distinct: bool,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if select.kind != SqlDocumentNodeKind::Select {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let select_range = select
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let select_keyword = document
        .tokens
        .iter()
        .enumerate()
        .find(|(index, token)| {
            select_range.start <= token.range.start
                && token.range.end <= select_range.end
                && token_is_keyword_at(&document.source, &document.tokens, *index, "SELECT")
        })
        .map(|(_, token)| token)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let next_significant = document.tokens.iter().enumerate().find(|(_, token)| {
        token.range.start >= select_keyword.range.end
            && token.range.end <= select_range.end
            && !matches!(
                token.kind,
                SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
            )
    });
    let quantifier = next_significant.and_then(|(index, token)| {
        token_is_keyword_at(&document.source, &document.tokens, index, "DISTINCT")
            .then_some((token, true))
            .or_else(|| {
                token_is_keyword_at(&document.source, &document.tokens, index, "ALL")
                    .then_some((token, false))
            })
    });
    let currently_distinct = quantifier.is_some_and(|(_, is_distinct)| is_distinct);
    if currently_distinct == distinct {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    if let Some((token, is_distinct)) = quantifier {
        if is_distinct {
            let next_after_quantifier =
                document.tokens.iter().enumerate().find(|(_, candidate)| {
                    candidate.range.start >= token.range.end
                        && candidate.range.end <= select_range.end
                        && !matches!(
                            candidate.kind,
                            SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
                        )
                });
            if next_after_quantifier.is_some_and(|(index, _)| {
                token_is_keyword_at(&document.source, &document.tokens, index, "ON")
            }) {
                return Err(SqlDocumentEditError::InvalidSemanticReplacement);
            }
        }
        let replacement = if distinct { "DISTINCT" } else { "" };
        let expected_text = document
            .source_slice(token.range)
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        return Ok(SqlDocumentEdit {
            range: token.range,
            expected_text: expected_text.to_owned(),
            replacement: replacement.to_owned(),
        });
    }

    let next_character = document
        .source
        .get(select_keyword.range.end..)
        .and_then(|suffix| suffix.chars().next());
    let suffix_space = next_character.is_some_and(|character| !character.is_whitespace());
    source_insertion(
        document,
        select_keyword.range.end,
        if suffix_space {
            " DISTINCT ".to_owned()
        } else {
            " DISTINCT".to_owned()
        },
    )
}

#[derive(Clone, Copy)]
enum QueryClauseValue {
    Limit(u64),
    Offset(u64),
}

#[derive(Clone, Copy)]
enum QueryClauseValueKind {
    Limit,
    Offset,
}

fn query_clause_nodes<'a>(
    document: &'a SqlDocument,
    query: &SqlDocumentNode,
) -> Result<(Option<&'a SqlDocumentNode>, Option<&'a SqlDocumentNode>), SqlDocumentEditError> {
    if query.kind != SqlDocumentNodeKind::Query {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let limit = document.nodes.iter().find(|node| {
        node.parent == Some(query.id) && node.kind == SqlDocumentNodeKind::LimitClause
    });
    let offset = limit.and_then(|limit| {
        document.nodes.iter().find(|node| {
            node.parent == Some(limit.id) && node.kind == SqlDocumentNodeKind::OffsetClause
        })
    });
    Ok((limit, offset))
}

fn keyword_token_in_range<'a>(
    document: &'a SqlDocument,
    range: SqlSourceRange,
    word: &str,
) -> Option<&'a SqlDocumentToken> {
    document
        .tokens
        .iter()
        .enumerate()
        .find(|(index, token)| {
            range.start <= token.range.start
                && token.range.end <= range.end
                && token_is_keyword_at(&document.source, &document.tokens, *index, word)
        })
        .map(|(_, token)| token)
}

fn next_significant_token(
    document: &SqlDocument,
    start: usize,
    end: usize,
) -> Option<&SqlDocumentToken> {
    document.tokens.iter().find(|token| {
        token.range.start >= start
            && token.range.end <= end
            && !matches!(
                token.kind,
                SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
            )
    })
}

fn set_query_clause_value(
    document: &SqlDocument,
    query: &SqlDocumentNode,
    value: QueryClauseValue,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    let query_range = query
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let (limit_clause, offset_clause) = query_clause_nodes(document, query)?;
    match value {
        QueryClauseValue::Limit(value) => {
            if let Some(limit_clause) = limit_clause {
                let clause_range = limit_clause
                    .range
                    .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
                if let Some(keyword) = keyword_token_in_range(document, clause_range, "LIMIT") {
                    let value_token = next_significant_token(
                        document,
                        keyword.range.end,
                        offset_clause
                            .and_then(|offset| offset.range)
                            .map_or(clause_range.end, |offset| offset.start),
                    )
                    .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
                    let token_text = document
                        .source_slice(value_token.range)
                        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
                    if token_text.eq_ignore_ascii_case("ALL") {
                        let expected_text = token_text.to_owned();
                        return Ok(SqlDocumentEdit {
                            range: value_token.range,
                            expected_text,
                            replacement: value.to_string(),
                        });
                    }
                    let token_range = SqlSourceRange {
                        start: keyword.range.end,
                        end: value_token.range.end,
                    };
                    if range_has_comment(document, token_range)
                        || !token_text.bytes().all(|byte| byte.is_ascii_digit())
                    {
                        return Err(SqlDocumentEditError::NodeRangeUnavailable);
                    }
                    let expected_text = document
                        .source_slice(value_token.range)
                        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
                    return Ok(SqlDocumentEdit {
                        range: value_token.range,
                        expected_text: expected_text.to_owned(),
                        replacement: value.to_string(),
                    });
                }
                let offset_start = offset_clause
                    .and_then(|offset| offset.range)
                    .map(|range| range.start)
                    .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
                return source_insertion(document, offset_start, format!("LIMIT {value} "));
            }
            source_insertion(document, query_range.end, format!(" LIMIT {value}"))
        }
        QueryClauseValue::Offset(value) => {
            if let Some(offset_clause) = offset_clause {
                let offset_range = offset_clause
                    .range
                    .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
                let keyword = keyword_token_in_range(document, offset_range, "OFFSET")
                    .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
                let value_token =
                    next_significant_token(document, keyword.range.end, offset_range.end)
                        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
                let token_range = SqlSourceRange {
                    start: keyword.range.end,
                    end: value_token.range.end,
                };
                let text = document
                    .source_slice(value_token.range)
                    .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
                if range_has_comment(document, token_range)
                    || !text.bytes().all(|byte| byte.is_ascii_digit())
                {
                    return Err(SqlDocumentEditError::NodeRangeUnavailable);
                }
                return Ok(SqlDocumentEdit {
                    range: value_token.range,
                    expected_text: text.to_owned(),
                    replacement: value.to_string(),
                });
            }
            if let Some(limit_clause) = limit_clause {
                let range = limit_clause
                    .range
                    .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
                let insert_at =
                    insertion_after_intervening_comments(document, range.end, query_range.end)?;
                return source_insertion(document, insert_at, format!(" OFFSET {value}"));
            }
            source_insertion(document, query_range.end, format!(" OFFSET {value}"))
        }
    }
}

fn remove_query_clause_value(
    document: &SqlDocument,
    query: &SqlDocumentNode,
    kind: QueryClauseValueKind,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    let (limit_clause, offset_clause) = query_clause_nodes(document, query)?;
    match kind {
        QueryClauseValueKind::Limit => {
            let limit_clause =
                limit_clause.ok_or(SqlDocumentEditError::InvalidSemanticReplacement)?;
            let clause_range = limit_clause
                .range
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
            let keyword = keyword_token_in_range(document, clause_range, "LIMIT");
            let Some(keyword) = keyword else {
                if offset_clause.is_some() {
                    return Err(SqlDocumentEditError::InvalidSemanticReplacement);
                }
                if range_has_comment(document, clause_range) {
                    return Err(SqlDocumentEditError::NodeRangeUnavailable);
                }
                return source_removal(document, clause_range);
            };
            let value_token = next_significant_token(document, keyword.range.end, clause_range.end)
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
            let value_range = SqlSourceRange {
                start: keyword.range.start,
                end: value_token.range.end,
            };
            if range_has_comment(document, value_range) {
                return Err(SqlDocumentEditError::NodeRangeUnavailable);
            }
            source_removal(document, value_range)
        }
        QueryClauseValueKind::Offset => {
            let offset_clause =
                offset_clause.ok_or(SqlDocumentEditError::InvalidSemanticReplacement)?;
            let offset_range = offset_clause
                .range
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
            if range_has_comment(document, offset_range) {
                return Err(SqlDocumentEditError::NodeRangeUnavailable);
            }
            if let Some(limit_clause) = limit_clause {
                let limit_range = limit_clause
                    .range
                    .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
                if keyword_token_in_range(document, limit_range, "LIMIT").is_some() {
                    return source_removal(document, offset_range);
                }
            }
            let limit_range = limit_clause
                .and_then(|limit| limit.range)
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
            if range_has_comment(document, limit_range) {
                return Err(SqlDocumentEditError::NodeRangeUnavailable);
            }
            source_removal(document, limit_range)
        }
    }
}

#[derive(Default)]
struct CaseControlTokens {
    case: Option<usize>,
    when: Vec<usize>,
    else_token: Option<usize>,
    end: Option<usize>,
}

fn case_control_tokens(
    document: &SqlDocument,
    node: &SqlDocumentNode,
) -> Result<CaseControlTokens, SqlDocumentEditError> {
    let range = node
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let start = document
        .tokens
        .iter()
        .enumerate()
        .find(|(index, token)| {
            token.range.start >= range.start
                && token.range.end <= range.end
                && token_is_keyword_at(&document.source, &document.tokens, *index, "CASE")
        })
        .map(|(index, _)| index)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let mut case_depth = 0usize;
    let mut delimiter_depth = 0usize;
    let mut markers = CaseControlTokens {
        case: Some(start),
        ..CaseControlTokens::default()
    };

    for index in start..document.tokens.len() {
        let token = &document.tokens[index];
        if token.range.start >= range.end {
            break;
        }
        let text = token_text(&document.source, token)
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        let punctuation = token.kind == SqlDocumentTokenKind::Punctuation;
        if punctuation && matches!(text, ")" | "]" | "}") {
            delimiter_depth = delimiter_depth
                .checked_sub(1)
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        }
        if delimiter_depth == 0 {
            if token_is_keyword_at(&document.source, &document.tokens, index, "CASE") {
                case_depth = case_depth.saturating_add(1);
            } else if case_depth == 1
                && token_is_keyword_at(&document.source, &document.tokens, index, "WHEN")
            {
                markers.when.push(index);
            } else if case_depth == 1
                && token_is_keyword_at(&document.source, &document.tokens, index, "ELSE")
            {
                markers.else_token = Some(index);
            } else if token_is_keyword_at(&document.source, &document.tokens, index, "END") {
                case_depth = case_depth
                    .checked_sub(1)
                    .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
                if case_depth == 0 {
                    markers.end = Some(index);
                    break;
                }
            }
        }
        if punctuation && matches!(text, "(" | "[" | "{") {
            delimiter_depth = delimiter_depth.saturating_add(1);
        }
    }
    markers
        .end
        .map(|_| markers)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)
}

fn direct_expression_children<'a>(
    document: &'a SqlDocument,
    node: &SqlDocumentNode,
) -> Vec<&'a SqlDocumentNode> {
    document
        .nodes
        .iter()
        .filter(|candidate| {
            candidate.parent == Some(node.id) && is_expression_node_kind(candidate.kind)
        })
        .collect()
}

fn case_metadata(node: &SqlDocumentNode) -> Result<(bool, usize, bool), SqlDocumentEditError> {
    match node.semantic.as_ref() {
        Some(SqlDocumentNodeData::Case {
            has_operand,
            when_count,
            has_else,
        }) if node.kind == SqlDocumentNodeKind::CaseExpression => {
            Ok((*has_operand, *when_count, *has_else))
        }
        _ => Err(SqlDocumentEditError::InvalidSemanticReplacement),
    }
}

fn set_case_operand(
    document: &SqlDocument,
    node: &SqlDocumentNode,
    expression: Option<&SqlDocumentNode>,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    let (has_operand, when_count, has_else) = case_metadata(node)?;
    if when_count == 0 || when_count > 32 || (!has_operand && expression.is_none()) {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let children = direct_expression_children(document, node);
    let expected_children = usize::from(has_operand) + when_count * 2 + usize::from(has_else);
    if children.len() != expected_children {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    if let Some(expression) = expression {
        let query_id = query_ancestor_id(document, node.id)
            .ok_or(SqlDocumentEditError::InvalidSemanticReplacement)?;
        if query_ancestor_id(document, expression.id) != Some(query_id) {
            return Err(SqlDocumentEditError::InvalidSemanticReplacement);
        }
        let mut ancestor = Some(expression.id);
        while let Some(id) = ancestor {
            if id == node.id {
                return Err(SqlDocumentEditError::InvalidSemanticReplacement);
            }
            ancestor = document
                .nodes
                .get(id as usize)
                .and_then(|candidate| candidate.parent);
        }
        let range = expression
            .range
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        let source = document
            .source_slice(range)
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        if source.trim().is_empty() {
            return Err(SqlDocumentEditError::InvalidSemanticReplacement);
        }
        if has_operand {
            let current = children
                .first()
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
            let range = current
                .range
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
            if range_has_comment(document, range) {
                return Err(SqlDocumentEditError::NodeRangeUnavailable);
            }
            let expected_text = document
                .source_slice(range)
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
            return Ok(SqlDocumentEdit {
                range,
                expected_text: expected_text.to_owned(),
                replacement: source.to_owned(),
            });
        } else {
            let control = case_control_tokens(document, node)?;
            let case_token = control
                .case
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
            return source_insertion(
                document,
                document.tokens[case_token].range.end,
                format!(" {source}"),
            );
        }
    }
    let operand = children
        .first()
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let range = operand
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if range_has_comment(document, range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    source_removal(document, range)
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

fn source_removal(
    document: &SqlDocument,
    range: SqlSourceRange,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    let expected_text = document
        .source_slice(range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?
        .to_owned();
    Ok(SqlDocumentEdit {
        range,
        expected_text,
        replacement: String::new(),
    })
}

fn preceding_horizontal_space_start(source: &str, mut offset: usize) -> usize {
    while offset > 0 && matches!(source.as_bytes()[offset - 1], b' ' | b'\t') {
        offset -= 1;
    }
    offset
}

fn change_case_branch(
    document: &SqlDocument,
    node: &SqlDocumentNode,
    add: bool,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    let (has_operand, when_count, has_else) = case_metadata(node)?;
    let control = case_control_tokens(document, node)?;
    let children = direct_expression_children(document, node);
    let offset = usize::from(has_operand);
    let expected_children = offset + when_count.saturating_mul(2) + usize::from(has_else);
    if when_count == 0
        || control.when.len() != when_count
        || children.len() != expected_children
        || when_count > 32
    {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    if add {
        if when_count >= 32 {
            return Err(SqlDocumentEditError::InvalidSemanticReplacement);
        }
        let last_result = children
            .get(offset + when_count.saturating_mul(2) - 1)
            .and_then(|child| child.range)
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        let case_range = node
            .range
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        if last_result.end <= case_range.start || last_result.end >= case_range.end {
            return Err(SqlDocumentEditError::NodeRangeUnavailable);
        }
        let branch_value = if has_operand { "NULL" } else { "TRUE" };
        return source_insertion(
            document,
            last_result.end,
            format!(" WHEN {branch_value} THEN NULL"),
        );
    }

    if when_count <= 1 {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let last_result = children
        .get(offset + when_count.saturating_mul(2) - 1)
        .and_then(|child| child.range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let when_token = &document.tokens[*control.when.last().unwrap()];
    let range = SqlSourceRange {
        start: preceding_horizontal_space_start(&document.source, when_token.range.start),
        end: last_result.end,
    };
    if range.start >= range.end {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    source_removal(document, range)
}

fn set_case_else(
    document: &SqlDocument,
    node: &SqlDocumentNode,
    has_else: bool,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    let (has_operand, when_count, current_has_else) = case_metadata(node)?;
    if has_else == current_has_else || when_count == 0 || when_count > 32 {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let control = case_control_tokens(document, node)?;
    let children = direct_expression_children(document, node);
    let offset = usize::from(has_operand);
    let expected_children = offset + when_count.saturating_mul(2) + usize::from(current_has_else);
    if control.when.len() != when_count || children.len() != expected_children {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    if has_else {
        let last_result = children
            .get(offset + when_count.saturating_mul(2) - 1)
            .and_then(|child| child.range)
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        return source_insertion(document, last_result.end, " ELSE NULL".to_owned());
    }

    let else_token = control
        .else_token
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let else_result = children
        .get(offset + when_count.saturating_mul(2))
        .and_then(|child| child.range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    source_removal(
        document,
        SqlSourceRange {
            start: preceding_horizontal_space_start(
                &document.source,
                document.tokens[else_token].range.start,
            ),
            end: else_result.end,
        },
    )
}

fn set_projection_expression(
    document: &SqlDocument,
    item: &SqlDocumentNode,
    expression: &SqlDocumentNode,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if item.kind != SqlDocumentNodeKind::SelectItem || !is_expression_node_kind(expression.kind) {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let item_range = item
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let target_range = document
        .nodes
        .iter()
        .filter(|candidate| candidate.parent == Some(item.id))
        .filter(|candidate| is_expression_node_kind(candidate.kind))
        .filter_map(|candidate| candidate.range)
        .filter(|range| item_range.start <= range.start && range.end <= item_range.end)
        .min_by_key(|range| range.end.saturating_sub(range.start))
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let expression_range = expression
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if expression_range.start <= item_range.start && item_range.end <= expression_range.end {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    if range_has_comment(document, target_range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let replacement = document
        .source_slice(expression_range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let expected_text = document
        .source_slice(target_range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if replacement.trim().is_empty() || replacement == expected_text {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    Ok(SqlDocumentEdit {
        range: target_range,
        expected_text: expected_text.to_owned(),
        replacement: replacement.to_owned(),
    })
}

fn compose_binary_expression(
    document: &SqlDocument,
    target: &SqlDocumentNode,
    left: &SqlDocumentNode,
    operator: SqlDocumentBinaryOperator,
    right: &SqlDocumentNode,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if (target.kind != SqlDocumentNodeKind::SelectItem && !is_expression_node_kind(target.kind))
        || !is_expression_node_kind(left.kind)
        || !is_expression_node_kind(right.kind)
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let query_id = query_ancestor_id(document, target.id)
        .ok_or(SqlDocumentEditError::InvalidSemanticReplacement)?;
    if query_ancestor_id(document, left.id) != Some(query_id)
        || query_ancestor_id(document, right.id) != Some(query_id)
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let target_range = target
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let target_range = if target.kind == SqlDocumentNodeKind::SelectItem {
        document
            .nodes
            .iter()
            .filter(|candidate| candidate.parent == Some(target.id))
            .filter(|candidate| is_expression_node_kind(candidate.kind))
            .filter_map(|candidate| candidate.range)
            .filter(|range| target_range.start <= range.start && range.end <= target_range.end)
            .min_by_key(|range| range.end.saturating_sub(range.start))
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?
    } else {
        target_range
    };
    if range_has_comment(document, target_range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let left_range = left
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let right_range = right
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let overlaps_target = |range: SqlSourceRange| {
        let overlaps = range.start < target_range.end && target_range.start < range.end;
        let is_exact_target = range.start == target_range.start && range.end == target_range.end;
        overlaps && !is_exact_target
    };
    if overlaps_target(left_range) || overlaps_target(right_range) {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let left_text = document
        .source_slice(left_range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let right_text = document
        .source_slice(right_range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if left_text.trim().is_empty() || right_text.trim().is_empty() {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let replacement = format!(
        "({left_text}\n) {} ({right_text}\n)",
        operator.sql_spelling()
    );
    let expected_text = document
        .source_slice(target_range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if replacement == expected_text {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    Ok(SqlDocumentEdit {
        range: target_range,
        expected_text: expected_text.to_owned(),
        replacement,
    })
}

fn wrap_expression_in_cast(
    document: &SqlDocument,
    expression: &SqlDocumentNode,
    target_type: SqlDocumentCastType,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if !is_expression_node_kind(expression.kind)
        || matches!(
            expression.kind,
            SqlDocumentNodeKind::OtherExpression | SqlDocumentNodeKind::SubqueryExpression
        )
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let range = expression
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if range_has_comment(document, range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let operand = document
        .source_slice(range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if operand.trim().is_empty() {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let replacement = format!("CAST({operand} AS {})", target_type.sql_spelling());
    if replacement == operand {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    Ok(SqlDocumentEdit {
        range,
        expected_text: operand.to_owned(),
        replacement,
    })
}

fn wrap_expression_in_aggregate(
    document: &SqlDocument,
    expression: &SqlDocumentNode,
    function: SqlDocumentAggregateFunction,
    distinct: bool,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if !is_expression_node_kind(expression.kind) {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let descriptor = crate::sql_functions::user_sql_aggregate(function.sql_spelling())
        .ok_or(SqlDocumentEditError::InvalidSemanticReplacement)?;
    if !descriptor.arity.accepts(1)
        || (distinct
            && descriptor.distinct == crate::sql_functions::SqlAggregateDistinct::Unsupported)
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let range = expression
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if range_has_comment(document, range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let argument = document
        .source_slice(range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if argument.trim().is_empty() {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let distinct_prefix = if distinct { "DISTINCT " } else { "" };
    let replacement = format!("{}({distinct_prefix}{argument})", function.sql_spelling());
    Ok(SqlDocumentEdit {
        range,
        expected_text: argument.to_owned(),
        replacement,
    })
}

fn aggregate_call_data(
    function: &SqlDocumentNode,
) -> Result<(String, usize, bool), SqlDocumentEditError> {
    if function.kind != SqlDocumentNodeKind::FunctionExpression {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let Some(SqlDocumentNodeData::Function {
        name,
        argument_count: Some(argument_count),
        distinct,
        has_filter: false,
        has_over: false,
        has_argument_clauses: false,
        ..
    }) = function.semantic.as_ref()
    else {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    };
    if name.len() != 1 || crate::sql_functions::user_sql_aggregate(&name[0].value).is_none() {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    Ok((name[0].value.clone(), *argument_count, *distinct))
}

fn set_aggregate_function(
    document: &SqlDocument,
    function_node: &SqlDocumentNode,
    function: SqlDocumentAggregateFunction,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    let (current_name, argument_count, distinct) = aggregate_call_data(function_node)?;
    crate::sql_functions::user_sql_aggregate(&current_name)
        .ok_or(SqlDocumentEditError::InvalidSemanticReplacement)?;
    let next_descriptor = crate::sql_functions::user_sql_aggregate(function.sql_spelling())
        .ok_or(SqlDocumentEditError::InvalidSemanticReplacement)?;
    if argument_count != 1
        || !next_descriptor.arity.accepts(argument_count)
        || (distinct
            && next_descriptor.distinct == crate::sql_functions::SqlAggregateDistinct::Unsupported)
        || current_name.eq_ignore_ascii_case(function.sql_spelling())
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let ranges = function_argument_ranges(document, function_node)?;
    let name_index = previous_nontrivia_token(&document.tokens, ranges.open_paren)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let name_token = &document.tokens[name_index];
    if !matches!(
        name_token.kind,
        SqlDocumentTokenKind::Word | SqlDocumentTokenKind::QuotedIdentifier
    ) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let name_range = name_token.range;
    let expected_text = document
        .source_slice(name_range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let unquoted = expected_text
        .strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
        .map(|text| text.replace("\"\"", "\""))
        .unwrap_or_else(|| expected_text.to_owned());
    if !unquoted.eq_ignore_ascii_case(&current_name) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    Ok(SqlDocumentEdit {
        range: name_range,
        expected_text: expected_text.to_owned(),
        replacement: function.sql_spelling().to_owned(),
    })
}

fn set_aggregate_distinct(
    document: &SqlDocument,
    function_node: &SqlDocumentNode,
    distinct: bool,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    let (current_name, argument_count, current_distinct) = aggregate_call_data(function_node)?;
    let descriptor = crate::sql_functions::user_sql_aggregate(&current_name)
        .ok_or(SqlDocumentEditError::InvalidSemanticReplacement)?;
    if argument_count != 1
        || distinct == current_distinct
        || (distinct
            && descriptor.distinct == crate::sql_functions::SqlAggregateDistinct::Unsupported)
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let ranges = function_argument_ranges(document, function_node)?;
    let argument = *ranges
        .arguments
        .first()
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if distinct {
        let first_token = skip_trivia_tokens(&document.tokens, ranges.open_paren + 1);
        if first_token >= ranges.close_paren {
            return Err(SqlDocumentEditError::NodeRangeUnavailable);
        }
        if token_is_keyword_at(&document.source, &document.tokens, first_token, "ALL") {
            return Err(SqlDocumentEditError::InvalidSemanticReplacement);
        }
        return source_insertion(document, argument.start, "DISTINCT ".to_owned());
    }

    let distinct_index = ranges
        .distinct_token
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let distinct_range = document.tokens[distinct_index].range;
    let range = SqlSourceRange {
        start: distinct_range.start,
        end: argument.start,
    };
    if range_has_comment(document, range)
        || document
            .source_slice(range)
            .is_none_or(|text| !text.to_ascii_uppercase().starts_with("DISTINCT"))
    {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    source_removal(document, range)
}

fn remove_aggregate(
    document: &SqlDocument,
    function_node: &SqlDocumentNode,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    let (_, argument_count, _) = aggregate_call_data(function_node)?;
    if argument_count != 1 {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let ranges = function_argument_ranges(document, function_node)?;
    let argument_range = *ranges
        .arguments
        .first()
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let mut function_range = function_node
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let closing_paren = document
        .tokens
        .get(ranges.close_paren)
        .filter(|token| token_text(&document.source, token) == Some(")"))
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if closing_paren.range.start < function_range.end {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    function_range.end = closing_paren.range.end;
    if range_has_comment(document, function_range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let expected_text = document
        .source_slice(function_range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let replacement = document
        .source_slice(argument_range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if replacement.trim().is_empty() {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    Ok(SqlDocumentEdit {
        range: function_range,
        expected_text: expected_text.to_owned(),
        replacement: replacement.to_owned(),
    })
}

fn wrap_expression_in_case(
    document: &SqlDocument,
    expression: &SqlDocumentNode,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if !is_expression_node_kind(expression.kind) {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let range = expression
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if range_has_comment(document, range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let value = document
        .source_slice(range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if value.trim().is_empty() {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    Ok(SqlDocumentEdit {
        range,
        expected_text: value.to_owned(),
        replacement: format!("CASE WHEN TRUE THEN {value} ELSE NULL END"),
    })
}

fn replace_expression_from_node(
    document: &SqlDocument,
    target: &SqlDocumentNode,
    expression: &SqlDocumentNode,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if !is_expression_node_kind(target.kind)
        || !is_expression_node_kind(expression.kind)
        || target.id == expression.id
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let query_id = query_ancestor_id(document, target.id)
        .ok_or(SqlDocumentEditError::InvalidSemanticReplacement)?;
    if query_ancestor_id(document, expression.id) != Some(query_id) {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let target_range = target
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let source_range = expression
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if range_has_comment(document, target_range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let expected_text = document
        .source_slice(target_range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let replacement = document
        .source_slice(source_range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if replacement.trim().is_empty() || replacement == expected_text {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    Ok(SqlDocumentEdit {
        range: target_range,
        expected_text: expected_text.to_owned(),
        replacement: replacement.to_owned(),
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
    document: &SqlDocument,
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
        SqlDocumentRelationFactor::Derived {
            query_node_id,
            alias,
        } => {
            let query = document
                .nodes
                .get(*query_node_id as usize)
                .filter(|candidate| {
                    candidate.id == *query_node_id && candidate.kind == SqlDocumentNodeKind::Query
                })
                .ok_or(SqlDocumentEditError::InvalidSemanticReplacement)?;
            let range = query
                .range
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
            let query_source = document
                .source_slice(range)
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
            if query_source.trim().is_empty() {
                return Err(SqlDocumentEditError::InvalidSemanticReplacement);
            }
            let alias = render_identifier_parts(std::slice::from_ref(alias))?;
            Ok(format!("({query_source}) AS {alias}"))
        }
        SqlDocumentRelationFactor::ExistingTableFactor { node_id } => {
            let table_factor = document
                .nodes
                .get(*node_id as usize)
                .filter(|candidate| {
                    candidate.id == *node_id
                        && candidate.kind == SqlDocumentNodeKind::TableFactor
                        && matches!(
                            candidate.semantic.as_ref(),
                            Some(SqlDocumentNodeData::TableFactor { .. })
                        )
                })
                .ok_or(SqlDocumentEditError::InvalidSemanticReplacement)?;
            let range = table_factor
                .range
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
            let source = document
                .source_slice(range)
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
            if source.trim().is_empty() {
                return Err(SqlDocumentEditError::InvalidSemanticReplacement);
            }
            if range_has_comment(document, range) {
                return Err(SqlDocumentEditError::NodeRangeUnavailable);
            }
            Ok(source.to_owned())
        }
    }
}

fn join_sql(
    document: &SqlDocument,
    target: &SqlDocumentNode,
    join: &SqlDocumentJoin,
) -> Result<String, SqlDocumentEditError> {
    if join.operator.forbids_explicit_constraint()
        && !matches!(&join.constraint, SqlDocumentJoinConstraint::None)
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let relation = relation_factor_sql(document, &join.relation)?;
    let mut sql = format!("{} {relation}", join.operator.sql_spelling());
    match &join.constraint {
        SqlDocumentJoinConstraint::None => {}
        SqlDocumentJoinConstraint::On {
            expression_node_id,
            expression_expected_kind,
        } => {
            if join.operator.forbids_explicit_constraint() {
                return Err(SqlDocumentEditError::InvalidSemanticReplacement);
            }
            let expression = document
                .nodes
                .get(*expression_node_id as usize)
                .filter(|candidate| candidate.id == *expression_node_id)
                .ok_or(SqlDocumentEditError::NodeNotFound)?;
            if expression.kind != *expression_expected_kind
                || !is_expression_node_kind(expression.kind)
                || expression.kind == SqlDocumentNodeKind::OtherExpression
            {
                return Err(SqlDocumentEditError::InvalidSemanticReplacement);
            }
            let query_id = query_ancestor_id(document, target.id)
                .ok_or(SqlDocumentEditError::InvalidSemanticReplacement)?;
            if query_ancestor_id(document, expression.id) != Some(query_id) {
                return Err(SqlDocumentEditError::InvalidSemanticReplacement);
            }
            let range = expression
                .range
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
            let source = document
                .source_slice(range)
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
            if source.trim().is_empty() {
                return Err(SqlDocumentEditError::InvalidSemanticReplacement);
            }
            sql.push_str(" ON ");
            sql.push_str(source);
        }
        SqlDocumentJoinConstraint::Using { columns } => {
            if join.operator.forbids_explicit_constraint()
                || columns.is_empty()
                || columns.len() > 64
            {
                return Err(SqlDocumentEditError::InvalidSemanticReplacement);
            }
            sql.push_str(" USING (");
            for (index, column) in columns.iter().enumerate() {
                if index > 0 {
                    sql.push_str(", ");
                }
                sql.push_str(&render_identifier_parts(std::slice::from_ref(column))?);
            }
            sql.push(')');
        }
    }
    Ok(sql)
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

fn add_join(
    document: &SqlDocument,
    relation_tree: &SqlDocumentNode,
    join: &SqlDocumentJoin,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if relation_tree.kind != SqlDocumentNodeKind::RelationTree
        || document
            .nodes
            .iter()
            .filter(|node| node.parent == Some(relation_tree.id))
            .filter(|node| node.kind == SqlDocumentNodeKind::Join)
            .count()
            >= 64
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let range = relation_tree
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    source_insertion(
        document,
        range.end,
        format!(" {}", join_sql(document, relation_tree, join)?),
    )
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

fn replace_join(
    document: &SqlDocument,
    join_node: &SqlDocumentNode,
    join: &SqlDocumentJoin,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if join_node.kind != SqlDocumentNodeKind::Join {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let range = join_node
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if range_has_comment(document, range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let expected_text = document
        .source_slice(range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let replacement = join_sql(document, join_node, join)?;
    if replacement == expected_text {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    Ok(SqlDocumentEdit {
        range,
        expected_text: expected_text.to_owned(),
        replacement,
    })
}

fn add_projection_expression(
    document: &SqlDocument,
    list: &SqlDocumentNode,
    expression: &SqlDocumentNode,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if list.kind != SqlDocumentNodeKind::SelectList || !is_expression_node_kind(expression.kind) {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let list_range = list
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let items = document
        .nodes
        .iter()
        .filter(|candidate| {
            candidate.parent == Some(list.id) && candidate.kind == SqlDocumentNodeKind::SelectItem
        })
        .filter_map(|candidate| candidate.range)
        .filter(|range| list_range.start <= range.start && range.end <= list_range.end)
        .collect::<Vec<_>>();
    if items.is_empty() || items.len() >= 256 {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let expression_range = expression
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if expression_range.start <= list_range.start && list_range.end <= expression_range.end {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let expression_text = document
        .source_slice(expression_range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if expression_text.trim().is_empty() {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let insertion = items
        .iter()
        .map(|range| range.end)
        .max()
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    source_insertion(document, insertion, format!(", {expression_text}"))
}

fn render_column_reference(
    column: &SqlDocumentColumnReference,
) -> Result<String, SqlDocumentEditError> {
    let mut parts = Vec::with_capacity(2);
    if let Some(qualifier) = &column.qualifier {
        parts.push(qualifier.clone());
    }
    parts.push(column.column.clone());
    render_identifier_parts(&parts)
}

fn add_projection_column(
    document: &SqlDocument,
    list: &SqlDocumentNode,
    column: &SqlDocumentColumnReference,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if list.kind != SqlDocumentNodeKind::SelectList {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let list_range = list
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let items = document
        .nodes
        .iter()
        .filter(|candidate| {
            candidate.parent == Some(list.id) && candidate.kind == SqlDocumentNodeKind::SelectItem
        })
        .filter_map(|candidate| candidate.range)
        .filter(|range| list_range.start <= range.start && range.end <= list_range.end)
        .collect::<Vec<_>>();
    if items.is_empty() || items.len() >= 256 {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let insertion = items
        .iter()
        .map(|range| range.end)
        .max()
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    source_insertion(
        document,
        insertion,
        format!(", {}", render_column_reference(column)?),
    )
}

fn expand_projection_wildcard(
    document: &SqlDocument,
    item: &SqlDocumentNode,
    columns: &[SqlDocumentColumnReference],
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if item.kind != SqlDocumentNodeKind::SelectItem || columns.is_empty() || columns.len() > 256 {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    if !matches!(
        item.semantic.as_ref(),
        Some(SqlDocumentNodeData::Wildcard {
            qualifier_is_expression: false,
            has_options: false,
            ..
        })
    ) {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let item_range = item
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let tokens = document
        .tokens
        .iter()
        .filter(|token| item_range.start <= token.range.start && token.range.end <= item_range.end)
        .filter(|token| {
            !matches!(
                token.kind,
                SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
            )
        })
        .collect::<Vec<_>>();
    let wildcard = tokens
        .last()
        .is_some_and(|token| document.source_slice(token.range) == Some("*"));
    if !wildcard || tokens.is_empty() {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let qualifier_tokens = &tokens[..tokens.len() - 1];
    if qualifier_tokens.len() % 2 != 0
        || qualifier_tokens.iter().enumerate().any(|(index, token)| {
            if index % 2 == 0 {
                !matches!(
                    token.kind,
                    SqlDocumentTokenKind::Word | SqlDocumentTokenKind::QuotedIdentifier
                )
            } else {
                token.kind != SqlDocumentTokenKind::Punctuation
                    || document.source_slice(token.range) != Some(".")
            }
        })
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let expression_range = SqlSourceRange {
        start: tokens
            .first()
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?
            .range
            .start,
        end: tokens
            .last()
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?
            .range
            .end,
    };
    if range_has_comment(document, expression_range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let columns = columns
        .iter()
        .map(render_column_reference)
        .collect::<Result<Vec<_>, _>>()?;
    let replacement = columns.join(", ");
    let expected_text = document
        .source_slice(expression_range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    Ok(SqlDocumentEdit {
        range: expression_range,
        expected_text: expected_text.to_owned(),
        replacement,
    })
}

fn add_scalar_subquery_projection(
    document: &SqlDocument,
    list: &SqlDocumentNode,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if list.kind != SqlDocumentNodeKind::SelectList {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let list_range = list
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let items = document
        .nodes
        .iter()
        .filter(|candidate| {
            candidate.parent == Some(list.id) && candidate.kind == SqlDocumentNodeKind::SelectItem
        })
        .filter_map(|candidate| candidate.range)
        .filter(|range| list_range.start <= range.start && range.end <= list_range.end)
        .collect::<Vec<_>>();
    if items.is_empty() || items.len() >= 256 {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let last_item_end = items
        .iter()
        .map(|range| range.end)
        .max()
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let alias = unique_scalar_subquery_alias(document, list_range);
    let select = document
        .nodes
        .get(
            list.parent
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)? as usize,
        )
        .filter(|candidate| candidate.kind == SqlDocumentNodeKind::Select)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let select_range = select
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let next_clause_start = document
        .nodes
        .iter()
        .filter(|candidate| candidate.parent == Some(select.id) && candidate.id != list.id)
        .filter_map(|candidate| candidate.range)
        .filter(|range| range.start >= list_range.end && range.start < select_range.end)
        .map(|range| range.start)
        .min()
        .unwrap_or(select_range.end);
    let mut insertion = last_item_end;
    for token in document.tokens.iter().filter(|token| {
        token.kind == SqlDocumentTokenKind::Comment
            && token.range.start >= last_item_end
            && token.range.end <= next_clause_start
    }) {
        insertion = token.range.end;
        if document
            .source_slice(token.range)
            .is_some_and(|text| text.starts_with("--"))
        {
            if let Some(newline) = document.source[insertion..next_clause_start].find('\n') {
                insertion += newline + 1;
            }
        }
    }
    let suffix = if insertion < document.source.len()
        && document
            .source
            .get(insertion..)
            .and_then(|remaining| remaining.chars().next())
            .is_some_and(|character| !character.is_whitespace())
    {
        " "
    } else {
        ""
    };
    source_insertion(
        document,
        insertion,
        format!(", (SELECT 1) AS {alias}{suffix}"),
    )
}

fn unique_scalar_subquery_alias(document: &SqlDocument, list_range: SqlSourceRange) -> String {
    let mut suffix = 0usize;
    loop {
        let alias = if suffix == 0 {
            "__ugoite_scalar_subquery".to_owned()
        } else {
            format!("__ugoite_scalar_subquery_{suffix}")
        };
        let already_used = document.tokens.iter().any(|token| {
            matches!(
                token.kind,
                SqlDocumentTokenKind::Word | SqlDocumentTokenKind::QuotedIdentifier
            ) && list_range.start <= token.range.start
                && token.range.end <= list_range.end
                && document
                    .source_slice(token.range)
                    .map(|text| {
                        let value = text
                            .strip_prefix('"')
                            .and_then(|text| text.strip_suffix('"'))
                            .map(|text| text.replace("\"\"", "\""))
                            .unwrap_or_else(|| text.to_owned());
                        value.eq_ignore_ascii_case(&alias)
                    })
                    .unwrap_or(false)
        });
        if !already_used {
            return alias;
        }
        suffix += 1;
    }
}

fn add_group_by_expression(
    document: &SqlDocument,
    select: &SqlDocumentNode,
    expression: &SqlDocumentNode,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if select.kind != SqlDocumentNodeKind::Select || !is_expression_node_kind(expression.kind) {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let query_id = query_ancestor_id(document, select.id)
        .ok_or(SqlDocumentEditError::InvalidSemanticReplacement)?;
    if query_ancestor_id(document, expression.id) != Some(query_id) {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let expression_range = expression
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let expression_text = document
        .source_slice(expression_range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if expression_text.trim().is_empty() {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }

    let existing_group = document.nodes.iter().find(|candidate| {
        candidate.parent == Some(select.id) && candidate.kind == SqlDocumentNodeKind::GroupByClause
    });
    if let Some(group) = existing_group {
        let items = group_by_items_for(document, group)?;
        let expressions = items
            .iter()
            .filter_map(|item| group_by_item_expression(document, item).ok())
            .filter_map(|expression| expression.range)
            .collect::<Vec<_>>();
        let last = items
            .last()
            .and_then(|item| item.range)
            .ok_or(SqlDocumentEditError::InvalidSemanticReplacement)?;
        if expressions.iter().any(|range| {
            document
                .source_slice(*range)
                .is_some_and(|source| source == expression_text)
        }) {
            return Err(SqlDocumentEditError::InvalidSemanticReplacement);
        }
        return source_insertion(document, last.end, format!(", {expression_text}"));
    }

    let select_range = select
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let select_list = document
        .nodes
        .iter()
        .find(|candidate| {
            candidate.parent == Some(select.id) && candidate.kind == SqlDocumentNodeKind::SelectList
        })
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let previous_end = document
        .nodes
        .iter()
        .filter(|candidate| candidate.parent == Some(select.id))
        .filter(|candidate| {
            matches!(
                candidate.kind,
                SqlDocumentNodeKind::SelectList
                    | SqlDocumentNodeKind::FromClause
                    | SqlDocumentNodeKind::WhereClause
            )
        })
        .filter_map(|candidate| candidate.range.map(|range| range.end))
        .max()
        .or_else(|| select_list.range.map(|range| range.end))
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let next_clause = document
        .nodes
        .iter()
        .filter(|candidate| {
            (candidate.parent == Some(select.id)
                && candidate.kind == SqlDocumentNodeKind::HavingClause)
                || (candidate.parent == Some(query_id)
                    && matches!(
                        candidate.kind,
                        SqlDocumentNodeKind::OrderByClause
                            | SqlDocumentNodeKind::LimitClause
                            | SqlDocumentNodeKind::OffsetClause
                    ))
        })
        .filter_map(|candidate| candidate.range)
        .filter(|range| range.start >= previous_end)
        .min_by_key(|range| range.start);
    let insert_at = next_clause.map_or(select_range.end, |range| range.start);
    if insert_at < previous_end || insert_at > select_range.end && next_clause.is_none() {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let preceding_is_space = document
        .source
        .get(..insert_at)
        .and_then(|prefix| prefix.chars().next_back())
        .is_some_and(char::is_whitespace);
    let prefix = if preceding_is_space { "" } else { " " };
    let suffix = if next_clause.is_some() { " " } else { "" };
    source_insertion(
        document,
        insert_at,
        format!("{prefix}GROUP BY {expression_text}{suffix}"),
    )
}

fn group_by_items_for<'a>(
    document: &'a SqlDocument,
    clause: &SqlDocumentNode,
) -> Result<Vec<&'a SqlDocumentNode>, SqlDocumentEditError> {
    if clause.kind != SqlDocumentNodeKind::GroupByClause {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let mut items = document
        .nodes
        .iter()
        .filter(|candidate| {
            candidate.parent == Some(clause.id)
                && candidate.kind == SqlDocumentNodeKind::GroupByItem
        })
        .collect::<Vec<_>>();
    items.sort_by_key(|candidate| {
        candidate
            .range
            .map(|range| (range.start, range.end))
            .unwrap_or((usize::MAX, usize::MAX))
    });
    if items.is_empty() {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    Ok(items)
}

fn group_by_item_expression<'a>(
    document: &'a SqlDocument,
    item: &SqlDocumentNode,
) -> Result<&'a SqlDocumentNode, SqlDocumentEditError> {
    if item.kind != SqlDocumentNodeKind::GroupByItem {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    document
        .nodes
        .iter()
        .filter(|candidate| candidate.parent == Some(item.id))
        .filter(|candidate| is_expression_node_kind(candidate.kind))
        .min_by_key(|candidate| {
            candidate
                .range
                .map(|range| range.end.saturating_sub(range.start))
                .unwrap_or(usize::MAX)
        })
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)
}

fn remove_group_by_item(
    document: &SqlDocument,
    item: &SqlDocumentNode,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    let clause = item
        .parent
        .and_then(|parent| document.nodes.get(parent as usize))
        .filter(|parent| parent.kind == SqlDocumentNodeKind::GroupByClause)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let items = group_by_items_for(document, clause)?;
    let index = items
        .iter()
        .position(|candidate| candidate.id == item.id)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let range = if items.len() == 1 {
        clause
            .range
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?
    } else if index == 0 {
        let comma = projection_separator(document, item, items[1])?;
        SqlSourceRange {
            start: list_item_prefix_start(document, clause, item)?,
            end: comma.range.end,
        }
    } else {
        let comma = projection_separator(document, items[index - 1], item)?;
        SqlSourceRange {
            start: comma.range.start,
            end: item
                .range
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?
                .end,
        }
    };
    if range_has_comment(document, range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    source_removal(document, range)
}

fn move_group_by_item(
    document: &SqlDocument,
    item: &SqlDocumentNode,
    to_index: usize,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    let clause = item
        .parent
        .and_then(|parent| document.nodes.get(parent as usize))
        .filter(|parent| parent.kind == SqlDocumentNodeKind::GroupByClause)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let items = group_by_items_for(document, clause)?;
    if to_index >= items.len() || items.len() <= 1 {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let from_index = items
        .iter()
        .position(|candidate| candidate.id == item.id)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if from_index == to_index {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    if range_has_comment(
        document,
        clause
            .range
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?,
    ) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let first = items
        .first()
        .and_then(|candidate| candidate.range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let last = items
        .last()
        .and_then(|candidate| candidate.range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let range = SqlSourceRange {
        start: first.start,
        end: last.end,
    };
    let mut reordered = items
        .iter()
        .map(|candidate| {
            let range = candidate
                .range
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
            document
                .source_slice(range)
                .map(str::to_owned)
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let moved = reordered.remove(from_index);
    reordered.insert(to_index, moved);
    let expected_text = document
        .source_slice(range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?
        .to_owned();
    Ok(SqlDocumentEdit {
        range,
        expected_text,
        replacement: reordered.join(", "),
    })
}

fn add_order_by_expression(
    document: &SqlDocument,
    query: &SqlDocumentNode,
    expression: &SqlDocumentNode,
    direction: SqlDocumentOrderDirection,
    nulls_order: Option<SqlDocumentNullsOrder>,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if query.kind != SqlDocumentNodeKind::Query || !is_expression_node_kind(expression.kind) {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    if query_ancestor_id(document, expression.id) != Some(query.id) {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let expression_range = expression
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let expression_text = document
        .source_slice(expression_range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if expression_text.trim().is_empty() {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }

    let order_clause = document.nodes.iter().find(|candidate| {
        candidate.parent == Some(query.id) && candidate.kind == SqlDocumentNodeKind::OrderByClause
    });
    if let Some(order_clause) = order_clause {
        let items = order_by_items_for(document, order_clause)?;
        if items.is_empty() {
            return Err(SqlDocumentEditError::InvalidSemanticReplacement);
        }
        if items.iter().any(|item| {
            order_by_item_expression(document, item)
                .ok()
                .and_then(|expression| expression.range)
                .and_then(|range| document.source_slice(range))
                == Some(expression_text)
        }) {
            return Err(SqlDocumentEditError::InvalidSemanticReplacement);
        }
        let order_range = order_clause
            .range
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        return source_insertion(
            document,
            order_range.end,
            format!(
                ", {expression_text} {}{}",
                direction.sql_spelling(),
                nulls_order.map_or("", SqlDocumentNullsOrder::sql_suffix)
            ),
        );
    }

    let query_range = query
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let next_clause = document
        .nodes
        .iter()
        .filter(|candidate| {
            candidate.parent == Some(query.id)
                && matches!(
                    candidate.kind,
                    SqlDocumentNodeKind::LimitClause | SqlDocumentNodeKind::OffsetClause
                )
        })
        .filter_map(|candidate| candidate.range)
        .min_by_key(|range| range.start);
    let insert_at = next_clause.map_or(query_range.end, |range| range.start);
    if insert_at < query_range.start || insert_at > query_range.end {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let preceding_is_space = document
        .source
        .get(..insert_at)
        .and_then(|prefix| prefix.chars().next_back())
        .is_some_and(char::is_whitespace);
    let prefix = if preceding_is_space { "" } else { " " };
    let suffix = if next_clause.is_some() { " " } else { "" };
    source_insertion(
        document,
        insert_at,
        format!(
            "{prefix}ORDER BY {expression_text} {}{}{suffix}",
            direction.sql_spelling(),
            nulls_order.map_or("", SqlDocumentNullsOrder::sql_suffix)
        ),
    )
}

fn order_by_items_for<'a>(
    document: &'a SqlDocument,
    clause: &SqlDocumentNode,
) -> Result<Vec<&'a SqlDocumentNode>, SqlDocumentEditError> {
    if clause.kind != SqlDocumentNodeKind::OrderByClause {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let mut items = document
        .nodes
        .iter()
        .filter(|candidate| {
            candidate.parent == Some(clause.id)
                && candidate.kind == SqlDocumentNodeKind::OrderByItem
        })
        .collect::<Vec<_>>();
    items.sort_by_key(|candidate| {
        candidate
            .range
            .map(|range| (range.start, range.end))
            .unwrap_or((usize::MAX, usize::MAX))
    });
    if items.is_empty() {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    Ok(items)
}

fn order_by_item_expression<'a>(
    document: &'a SqlDocument,
    item: &SqlDocumentNode,
) -> Result<&'a SqlDocumentNode, SqlDocumentEditError> {
    if item.kind != SqlDocumentNodeKind::OrderByItem {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    document
        .nodes
        .iter()
        .filter(|candidate| candidate.parent == Some(item.id))
        .filter(|candidate| is_expression_node_kind(candidate.kind))
        .min_by_key(|candidate| {
            candidate
                .range
                .map(|range| range.end.saturating_sub(range.start))
                .unwrap_or(usize::MAX)
        })
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)
}

fn set_order_by_expression(
    document: &SqlDocument,
    item: &SqlDocumentNode,
    expression: &SqlDocumentNode,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if item.kind != SqlDocumentNodeKind::OrderByItem || !is_expression_node_kind(expression.kind) {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let clause = item
        .parent
        .and_then(|parent| document.nodes.get(parent as usize))
        .filter(|parent| parent.kind == SqlDocumentNodeKind::OrderByClause)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let query = clause
        .parent
        .and_then(|parent| document.nodes.get(parent as usize))
        .filter(|parent| parent.kind == SqlDocumentNodeKind::Query)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if query_ancestor_id(document, expression.id) != Some(query.id) {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let target = order_by_item_expression(document, item)?;
    let target_range = target
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let expression_range = expression
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if range_has_comment(document, target_range)
        || expression_range.start <= target_range.start && target_range.end <= expression_range.end
    {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let replacement = document
        .source_slice(expression_range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let expected_text = document
        .source_slice(target_range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if replacement.trim().is_empty() || replacement == expected_text {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    Ok(SqlDocumentEdit {
        range: target_range,
        expected_text: expected_text.to_owned(),
        replacement: replacement.to_owned(),
    })
}

fn set_order_by_options(
    document: &SqlDocument,
    item: &SqlDocumentNode,
    direction: SqlDocumentOrderDirection,
    nulls_order: Option<SqlDocumentNullsOrder>,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    let expression = order_by_item_expression(document, item)?;
    let expression_range = expression
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let item_range = item
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if expression_range.end > item_range.end {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let suffix_range = SqlSourceRange {
        start: expression_range.end,
        end: item_range.end,
    };
    if range_has_comment(document, suffix_range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let suffix_tokens = document
        .tokens
        .iter()
        .filter(|token| {
            suffix_range.start <= token.range.start && token.range.end <= suffix_range.end
        })
        .filter(|token| {
            !matches!(
                token.kind,
                SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
            )
        })
        .collect::<Vec<_>>();
    let suffix_words = suffix_tokens
        .iter()
        .map(|token| {
            document
                .source_slice(token.range)
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let valid = match suffix_words.as_slice() {
        [] | ["ASC"] | ["DESC"] => true,
        [direction, "NULLS", placement] => {
            matches!(*direction, "ASC" | "DESC") && matches!(*placement, "FIRST" | "LAST")
        }
        ["NULLS", placement] => matches!(*placement, "FIRST" | "LAST"),
        _ => false,
    };
    if !valid {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let replacement = format!(
        " {}{}",
        direction.sql_spelling(),
        nulls_order.map_or("", SqlDocumentNullsOrder::sql_suffix)
    );
    let expected_text = document
        .source_slice(suffix_range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    Ok(SqlDocumentEdit {
        range: suffix_range,
        expected_text: expected_text.to_owned(),
        replacement,
    })
}

fn remove_order_by_item(
    document: &SqlDocument,
    item: &SqlDocumentNode,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    let clause = item
        .parent
        .and_then(|parent| document.nodes.get(parent as usize))
        .filter(|parent| parent.kind == SqlDocumentNodeKind::OrderByClause)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let items = order_by_items_for(document, clause)?;
    let index = items
        .iter()
        .position(|candidate| candidate.id == item.id)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let range = if items.len() == 1 {
        clause
            .range
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?
    } else if index == 0 {
        let comma = projection_separator(document, item, items[1])?;
        SqlSourceRange {
            start: list_item_prefix_start(document, clause, item)?,
            end: comma.range.end,
        }
    } else {
        let comma = projection_separator(document, items[index - 1], item)?;
        SqlSourceRange {
            start: comma.range.start,
            end: item
                .range
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?
                .end,
        }
    };
    if range_has_comment(document, range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    source_removal(document, range)
}

fn move_order_by_item(
    document: &SqlDocument,
    item: &SqlDocumentNode,
    to_index: usize,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    let clause = item
        .parent
        .and_then(|parent| document.nodes.get(parent as usize))
        .filter(|parent| parent.kind == SqlDocumentNodeKind::OrderByClause)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let items = order_by_items_for(document, clause)?;
    if to_index >= items.len() || items.len() <= 1 {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let from_index = items
        .iter()
        .position(|candidate| candidate.id == item.id)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if from_index == to_index {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let clause_range = clause
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if range_has_comment(document, clause_range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let first = items
        .first()
        .and_then(|candidate| candidate.range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let last = items
        .last()
        .and_then(|candidate| candidate.range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let range = SqlSourceRange {
        start: first.start,
        end: last.end,
    };
    let mut reordered = items
        .iter()
        .map(|candidate| {
            let range = candidate
                .range
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
            document
                .source_slice(range)
                .map(str::to_owned)
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let moved = reordered.remove(from_index);
    reordered.insert(to_index, moved);
    let expected_text = document
        .source_slice(range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?
        .to_owned();
    Ok(SqlDocumentEdit {
        range,
        expected_text,
        replacement: reordered.join(", "),
    })
}

fn projection_items_for<'a>(
    document: &'a SqlDocument,
    item: &SqlDocumentNode,
) -> Result<Vec<&'a SqlDocumentNode>, SqlDocumentEditError> {
    if item.kind != SqlDocumentNodeKind::SelectItem {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let list_id = item
        .parent
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let list = document
        .nodes
        .get(list_id as usize)
        .filter(|candidate| {
            candidate.id == list_id && candidate.kind == SqlDocumentNodeKind::SelectList
        })
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let list_range = list
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let mut items = document
        .nodes
        .iter()
        .filter(|candidate| {
            candidate.parent == Some(list_id) && candidate.kind == SqlDocumentNodeKind::SelectItem
        })
        .filter(|candidate| {
            candidate
                .range
                .is_some_and(|range| list_range.start <= range.start && range.end <= list_range.end)
        })
        .collect::<Vec<_>>();
    items.sort_by_key(|candidate| {
        candidate
            .range
            .map(|range| (range.start, range.end))
            .unwrap_or((usize::MAX, usize::MAX))
    });
    if items.is_empty() || !items.iter().any(|candidate| candidate.id == item.id) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    Ok(items)
}

fn projection_separator(
    document: &SqlDocument,
    left: &SqlDocumentNode,
    right: &SqlDocumentNode,
) -> Result<SqlDocumentToken, SqlDocumentEditError> {
    let left_range = left
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let right_range = right
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let commas = document
        .tokens
        .iter()
        .filter(|token| {
            token.kind == SqlDocumentTokenKind::Punctuation
                && token.range.start >= left_range.end
                && token.range.end <= right_range.start
                && document.source_slice(token.range) == Some(",")
        })
        .cloned()
        .collect::<Vec<_>>();
    if commas.len() != 1 {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    Ok(commas[0].clone())
}

fn list_item_prefix_start(
    document: &SqlDocument,
    clause: &SqlDocumentNode,
    item: &SqlDocumentNode,
) -> Result<usize, SqlDocumentEditError> {
    let clause_range = clause
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let item_range = item
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let previous = document
        .tokens
        .iter()
        .rfind(|token| {
            token.range.start >= clause_range.start
                && token.range.end <= item_range.start
                && !matches!(
                    token.kind,
                    SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
                )
        })
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if !token_text(&document.source, previous).is_some_and(|text| text.eq_ignore_ascii_case("BY")) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    Ok(previous.range.end)
}

/// Preserve positional GROUP BY / ORDER BY bindings when output fields move.
/// Removing the referenced field, or changing positions whose wildcard/set
/// scope cannot be established without execution schema, must fail closed.
fn projection_position_edits(
    document: &SqlDocument,
    item: &SqlDocumentNode,
    destination: Option<usize>,
) -> Result<Vec<SqlDocumentEdit>, SqlDocumentEditError> {
    let items = projection_items_for(document, item)?;
    let from = items
        .iter()
        .position(|candidate| candidate.id == item.id)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let list = document
        .nodes
        .get(
            item.parent
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)? as usize,
        )
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let select_id = list
        .parent
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let query_id =
        query_ancestor_id(document, select_id).ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let mut order = (0..items.len()).collect::<Vec<_>>();
    order.remove(from);
    if let Some(destination) = destination {
        if destination >= items.len() {
            return Err(SqlDocumentEditError::InvalidSemanticReplacement);
        }
        order.insert(destination, from);
    }
    let mut edits = Vec::new();
    for clause_item in &document.nodes {
        let parent = clause_item
            .parent
            .and_then(|id| document.nodes.get(id as usize));
        let in_group = clause_item.kind == SqlDocumentNodeKind::GroupByItem
            && parent.and_then(|node| node.parent) == Some(select_id);
        let in_order = clause_item.kind == SqlDocumentNodeKind::OrderByItem
            && parent.and_then(|node| node.parent) == Some(query_id);
        if !in_group && !in_order {
            continue;
        }
        let mut expression = document
            .nodes
            .iter()
            .find(|node| node.parent == Some(clause_item.id));
        while !in_order
            && expression.is_some_and(|node| node.kind == SqlDocumentNodeKind::NestedExpression)
        {
            expression = expression.and_then(|node| {
                document
                    .nodes
                    .iter()
                    .find(|child| child.parent == Some(node.id))
            });
        }
        let Some(expression) = expression else {
            continue;
        };
        let Some(SqlDocumentNodeData::Literal {
            literal_kind: SqlDocumentLiteralKind::Number,
            raw: Some(raw),
        }) = &expression.semantic
        else {
            continue;
        };
        if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let ordinal = raw
            .parse::<usize>()
            .map_err(|_| SqlDocumentEditError::InvalidSemanticReplacement)?;
        if ordinal == 0 {
            return Err(SqlDocumentEditError::InvalidSemanticReplacement);
        }
        if items
            .iter()
            .any(|node| matches!(node.semantic, Some(SqlDocumentNodeData::Wildcard { .. })))
            || (in_order
                && document
                    .nodes
                    .get(select_id as usize)
                    .and_then(|node| node.parent)
                    != Some(query_id))
        {
            return Err(SqlDocumentEditError::ProjectionPositionConflict);
        }
        let position = order
            .iter()
            .position(|index| *index == ordinal - 1)
            .ok_or(SqlDocumentEditError::ProjectionPositionConflict)?
            + 1;
        if position != ordinal {
            let range = expression
                .range
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
            let expected_text = document
                .source_slice(range)
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
            edits.push(SqlDocumentEdit {
                range,
                expected_text: expected_text.to_owned(),
                replacement: position.to_string(),
            });
        }
    }
    Ok(edits)
}

fn ensure_projection_alias_unreferenced(
    document: &SqlDocument,
    item: &SqlDocumentNode,
) -> Result<(), SqlDocumentEditError> {
    let range = item
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let text = document
        .source_slice(range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let statements = parse_generic_sql_statements(&format!("SELECT {text}"))
        .map_err(|_| SqlDocumentEditError::NodeRangeUnavailable)?;
    let Some(Statement::Query(query)) = statements.first() else {
        return Ok(());
    };
    let SetExpr::Select(select) = query.body.as_ref() else {
        return Ok(());
    };
    let Some(SelectItem::ExprWithAlias { alias, .. }) = select.projection.first() else {
        return Ok(());
    };
    ensure_projection_name_unreferenced(
        document,
        item,
        &SqlDocumentIdentifier {
            value: alias.value.clone(),
            quote_style: alias.quote_style,
        },
    )
}

fn ensure_projection_name_unreferenced(
    document: &SqlDocument,
    item: &SqlDocumentNode,
    alias: &SqlDocumentIdentifier,
) -> Result<(), SqlDocumentEditError> {
    let range = item
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let item_query = query_ancestor_id(document, item.id);
    let referenced = document.nodes.iter().any(|candidate| {
        if candidate.id == item.id
            || candidate.kind != SqlDocumentNodeKind::IdentifierExpression
            || query_ancestor_id(document, candidate.id) != item_query
            || candidate.range.is_some_and(|candidate_range| {
                range.start <= candidate_range.start && candidate_range.end <= range.end
            })
        {
            return false;
        }
        matches!(
            &candidate.semantic,
            Some(SqlDocumentNodeData::Identifier { value, quote_style })
                if sql_identifiers_match(
                    &SqlDocumentIdentifier { value: value.clone(), quote_style: *quote_style },
                    alias
                )
        )
    });
    if referenced {
        Err(SqlDocumentEditError::ProjectionAliasConflict)
    } else {
        Ok(())
    }
}

fn set_projection_alias(
    document: &SqlDocument,
    item: &SqlDocumentNode,
    alias: Option<&SqlDocumentIdentifier>,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if item.kind != SqlDocumentNodeKind::SelectItem {
        return Err(SqlDocumentEditError::NodeKindMismatch);
    }
    let item_range = item
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let old_alias = match item.semantic.as_ref() {
        Some(SqlDocumentNodeData::ProjectionAlias { alias }) => Some(alias),
        _ => None,
    };
    if old_alias == alias {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    if old_alias.is_some() {
        ensure_projection_alias_unreferenced(document, item)?;
    }
    if let Some(new_alias) = alias {
        ensure_projection_name_unreferenced(document, item, new_alias)?;
        if document.nodes.iter().any(|candidate| {
            candidate.id != item.id
                && candidate.kind == SqlDocumentNodeKind::SelectItem
                && candidate.parent == item.parent
                && matches!(&candidate.semantic, Some(SqlDocumentNodeData::ProjectionAlias { alias: existing }) if sql_identifiers_match(existing, new_alias))
        }) {
            return Err(SqlDocumentEditError::ProjectionAliasConflict);
        }
    }
    let render =
        |alias: &SqlDocumentIdentifier| render_identifier_parts(std::slice::from_ref(alias));
    let token = |identifier: &SqlDocumentIdentifier| -> Option<SqlSourceRange> {
        let expected = render(identifier).ok()?;
        document.tokens.iter().rev().find_map(|token| {
            if token.range.start < item_range.start
                || token.range.end > item_range.end
                || !matches!(
                    token.kind,
                    SqlDocumentTokenKind::Word | SqlDocumentTokenKind::QuotedIdentifier
                )
            {
                return None;
            }
            (document.source_slice(token.range) == Some(expected.as_str())).then_some(token.range)
        })
    };
    if let Some(old) = old_alias {
        let alias_range = token(old).ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        if let Some(new) = alias {
            return Ok(SqlDocumentEdit {
                range: alias_range,
                expected_text: document
                    .source_slice(alias_range)
                    .unwrap_or_default()
                    .to_string(),
                replacement: render(new)?,
            });
        }
        let expression_end = document
            .nodes
            .iter()
            .filter(|candidate| {
                candidate.parent == Some(item.id) && is_expression_node_kind(candidate.kind)
            })
            .filter_map(|candidate| candidate.range.map(|range| range.end))
            .max()
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        let as_token = document.tokens.iter().rev().find(|candidate| {
            candidate.range.start >= expression_end
                && candidate.range.end <= alias_range.start
                && document
                    .source_slice(candidate.range)
                    .is_some_and(|text| text.eq_ignore_ascii_case("AS"))
        });
        let start = as_token.map_or(expression_end, |token| {
            let has_comment_before_as = document.tokens.iter().any(|candidate| {
                candidate.kind == SqlDocumentTokenKind::Comment
                    && candidate.range.start >= expression_end
                    && candidate.range.end <= token.range.start
            });
            if has_comment_before_as {
                token.range.start
            } else {
                expression_end
            }
        });
        let range = SqlSourceRange {
            start,
            end: alias_range.end,
        };
        if document.tokens.iter().any(|candidate| {
            candidate.kind == SqlDocumentTokenKind::Comment
                && candidate.range.start >= range.start
                && candidate.range.end <= range.end
        }) {
            return Err(SqlDocumentEditError::NodeRangeUnavailable);
        }
        return Ok(SqlDocumentEdit {
            range,
            expected_text: document.source_slice(range).unwrap_or_default().to_string(),
            replacement: String::new(),
        });
    }
    let Some(new) = alias else {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    };
    let expression_end = document
        .nodes
        .iter()
        .filter(|node| node.parent == Some(item.id) && is_expression_node_kind(node.kind))
        .filter_map(|node| node.range.map(|range| range.end))
        .max()
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let insertion = SqlSourceRange {
        start: expression_end,
        end: expression_end,
    };
    Ok(SqlDocumentEdit {
        range: insertion,
        expected_text: String::new(),
        replacement: format!(" AS {}", render(new)?),
    })
}

fn ensure_unique_projection_alias(
    nodes: &[SqlDocumentNode],
    item: &SqlDocumentNode,
    alias: Option<&SqlDocumentIdentifier>,
    transaction_edits: &[SqlDocumentSemanticEdit],
) -> Result<(), SqlDocumentEditError> {
    let Some(alias) = alias else {
        return Ok(());
    };
    for sibling in nodes.iter().filter(|node| {
        node.kind == SqlDocumentNodeKind::SelectItem
            && node.parent == item.parent
            && node.id != item.id
    }) {
        let proposed = transaction_edits.iter().find_map(|edit| match edit {
            SqlDocumentSemanticEdit::SetProjectionAlias { node_id, alias, .. }
                if *node_id == sibling.id =>
            {
                Some(alias.as_ref())
            }
            _ => None,
        });
        let existing = match &sibling.semantic {
            Some(SqlDocumentNodeData::ProjectionAlias { alias }) => Some(alias),
            _ => None,
        };
        let duplicate_alias = if let Some(proposed) = proposed {
            proposed.is_some_and(|other| sql_identifiers_match(other, alias))
        } else if let Some(existing) = existing {
            sql_identifiers_match(existing, alias)
        } else {
            unaliased_projection_output_name_matches(nodes, sibling, alias)
        };
        if duplicate_alias {
            return Err(SqlDocumentEditError::ProjectionAliasConflict);
        }
    }
    Ok(())
}

fn unaliased_projection_output_name_matches(
    nodes: &[SqlDocumentNode],
    item: &SqlDocumentNode,
    alias: &SqlDocumentIdentifier,
) -> bool {
    nodes
        .iter()
        .find(|node| {
            node.parent == Some(item.id)
                && matches!(
                    node.kind,
                    SqlDocumentNodeKind::IdentifierExpression
                        | SqlDocumentNodeKind::CompoundIdentifierExpression
                )
        })
        .and_then(|node| match &node.semantic {
            Some(SqlDocumentNodeData::Identifier { value, quote_style }) => {
                Some(SqlDocumentIdentifier {
                    value: value.clone(),
                    quote_style: *quote_style,
                })
            }
            Some(SqlDocumentNodeData::CompoundIdentifier { parts }) => parts.last().cloned(),
            _ => None,
        })
        .is_some_and(|output_name| sql_identifiers_match(&output_name, alias))
}

fn remove_projection_item(
    document: &SqlDocument,
    item: &SqlDocumentNode,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    let items = projection_items_for(document, item)?;
    if items.len() <= 1 {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let index = items
        .iter()
        .position(|candidate| candidate.id == item.id)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let item_range = item
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if range_has_comment(document, item_range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let range = if index == 0 {
        let comma = projection_separator(document, item, items[1])?;
        SqlSourceRange {
            start: item_range.start,
            end: comma.range.end,
        }
    } else {
        let comma = projection_separator(document, items[index - 1], item)?;
        SqlSourceRange {
            start: comma.range.start,
            end: item_range.end,
        }
    };
    if range_has_comment(document, range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    source_removal(document, range)
}

fn move_projection_item(
    document: &SqlDocument,
    item: &SqlDocumentNode,
    to_index: usize,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    let items = projection_items_for(document, item)?;
    if to_index >= items.len() || items.len() <= 1 {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let from_index = items
        .iter()
        .position(|candidate| candidate.id == item.id)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if from_index == to_index {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let list = document
        .nodes
        .get(
            item.parent
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)? as usize,
        )
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let list_range = list
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if range_has_comment(document, list_range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let first = items
        .first()
        .and_then(|candidate| candidate.range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let last = items
        .last()
        .and_then(|candidate| candidate.range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let range = SqlSourceRange {
        start: first.start,
        end: last.end,
    };
    let mut reordered = items
        .iter()
        .map(|candidate| {
            let range = candidate
                .range
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
            document
                .source_slice(range)
                .map(str::to_owned)
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let moved = reordered.remove(from_index);
    reordered.insert(to_index, moved);
    let expected_text = document
        .source_slice(range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?
        .to_owned();
    Ok(SqlDocumentEdit {
        range,
        expected_text,
        replacement: reordered.join(", "),
    })
}

struct FunctionArgumentRanges {
    arguments: Vec<SqlSourceRange>,
    commas: Vec<usize>,
    distinct_token: Option<usize>,
    open_paren: usize,
    close_paren: usize,
}

fn function_argument_ranges(
    document: &SqlDocument,
    node: &SqlDocumentNode,
) -> Result<FunctionArgumentRanges, SqlDocumentEditError> {
    let Some(SqlDocumentNodeData::Function {
        name,
        argument_count: Some(argument_count),
        distinct,
        has_filter: false,
        has_over: false,
        has_argument_clauses: false,
        ..
    }) = node.semantic.as_ref()
    else {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    };
    if node.kind != SqlDocumentNodeKind::FunctionExpression
        || name
            .last()
            .is_none_or(|part| crate::sql_functions::user_sql_aggregate(&part.value).is_none())
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let range = node
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let open_paren = document
        .tokens
        .iter()
        .enumerate()
        .find(|(_, token)| {
            token.range.start >= range.start
                && token.kind == SqlDocumentTokenKind::Punctuation
                && token_text(&document.source, token) == Some("(")
        })
        .map(|(index, _)| index)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let close_paren = matching_close_paren(&document.source, &document.tokens, open_paren)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;

    let mut arguments = Vec::new();
    let mut commas = Vec::new();
    let mut current_start = None;
    let mut current_end = None;
    let mut distinct_token = None;
    let mut delimiter_depth = 0usize;
    let mut first_sql_token = true;

    for index in open_paren + 1..close_paren {
        let token = &document.tokens[index];
        let text = token_text(&document.source, token)
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        let punctuation = token.kind == SqlDocumentTokenKind::Punctuation;
        if punctuation && matches!(text, ")" | "]" | "}") {
            delimiter_depth = delimiter_depth
                .checked_sub(1)
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        }
        if delimiter_depth == 0 {
            if punctuation && text == "," {
                if let (Some(start), Some(end)) = (current_start.take(), current_end.take()) {
                    arguments.push(SqlSourceRange { start, end });
                    commas.push(index);
                } else {
                    return Err(SqlDocumentEditError::NodeRangeUnavailable);
                }
                continue;
            }
            if token.kind == SqlDocumentTokenKind::Word
                && ["ORDER", "LIMIT", "IGNORE", "RESPECT", "ON"]
                    .iter()
                    .any(|keyword| {
                        token_is_keyword_at(&document.source, &document.tokens, index, keyword)
                    })
            {
                return Err(SqlDocumentEditError::NodeRangeUnavailable);
            }
            if first_sql_token
                && token.kind != SqlDocumentTokenKind::Trivia
                && token.kind != SqlDocumentTokenKind::Comment
            {
                first_sql_token = false;
                if token_is_keyword_at(&document.source, &document.tokens, index, "DISTINCT") {
                    distinct_token = Some(index);
                    continue;
                }
                if token_is_keyword_at(&document.source, &document.tokens, index, "ALL") {
                    continue;
                }
            }
        }
        if !matches!(
            token.kind,
            SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
        ) {
            current_start.get_or_insert(token.range.start);
            current_end = Some(token.range.end);
        }
        if punctuation && matches!(text, "(" | "[" | "{") {
            delimiter_depth = delimiter_depth.saturating_add(1);
        }
    }
    if let (Some(start), Some(end)) = (current_start, current_end) {
        arguments.push(SqlSourceRange { start, end });
    }
    if arguments.len() != *argument_count
        || commas.len() != arguments.len().saturating_sub(1)
        || *distinct != distinct_token.is_some()
    {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    Ok(FunctionArgumentRanges {
        arguments,
        commas,
        distinct_token,
        open_paren,
        close_paren,
    })
}

fn range_has_comment(document: &SqlDocument, range: SqlSourceRange) -> bool {
    document.tokens.iter().any(|token| {
        token.kind == SqlDocumentTokenKind::Comment
            && token.range.start < range.end
            && range.start < token.range.end
    })
}

fn set_function_argument(
    document: &SqlDocument,
    function: &SqlDocumentNode,
    argument_index: usize,
    expression: &SqlDocumentNode,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if !is_expression_node_kind(expression.kind)
        || matches!(
            expression.kind,
            SqlDocumentNodeKind::OtherExpression | SqlDocumentNodeKind::SubqueryExpression
        )
        || expression.id == function.id
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let arguments = function_argument_ranges(document, function)?;
    let expression_range = expression
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let function_range = function
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if expression_range.start <= function_range.start && function_range.end <= expression_range.end
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let expression_text = document
        .source_slice(expression_range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if expression_text.trim().is_empty() {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    if let Some(target_range) = arguments.arguments.get(argument_index).copied() {
        if range_has_comment(document, target_range) {
            return Err(SqlDocumentEditError::NodeRangeUnavailable);
        }
        if document.source_slice(target_range) == Some(expression_text) {
            return Err(SqlDocumentEditError::InvalidSemanticReplacement);
        }
        return Ok(SqlDocumentEdit {
            range: target_range,
            expected_text: document
                .source_slice(target_range)
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?
                .to_owned(),
            replacement: expression_text.to_owned(),
        });
    }

    let Some(SqlDocumentNodeData::Function {
        can_add_argument, ..
    }) = function.semantic.as_ref()
    else {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    };
    if argument_index != arguments.arguments.len()
        || !can_add_argument
        || arguments.arguments.len() >= 32
        || arguments
            .arguments
            .last()
            .and_then(|range| document.source_slice(*range))
            .is_some_and(|text| text.trim() == "*" || text.trim().ends_with(".*"))
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let insertion = arguments.arguments.last().map_or(
        document.tokens[arguments.close_paren].range.start,
        |range| range.end,
    );
    let replacement = if arguments.arguments.is_empty() {
        expression_text.to_owned()
    } else {
        format!(", {expression_text}")
    };
    source_insertion(document, insertion, replacement)
}

fn remove_last_function_argument(
    document: &SqlDocument,
    function: &SqlDocumentNode,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    let Some(SqlDocumentNodeData::Function {
        can_remove_argument,
        ..
    }) = function.semantic.as_ref()
    else {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    };
    if !can_remove_argument {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let arguments = function_argument_ranges(document, function)?;
    let last = arguments
        .arguments
        .last()
        .copied()
        .ok_or(SqlDocumentEditError::InvalidSemanticReplacement)?;
    let range = if let Some(comma_index) = arguments.commas.last().copied() {
        SqlSourceRange {
            start: document.tokens[comma_index].range.start,
            end: last.end,
        }
    } else if let Some(distinct_index) = arguments.distinct_token {
        SqlSourceRange {
            start: document.tokens[distinct_index].range.start,
            end: last.end,
        }
    } else {
        last
    };
    if range_has_comment(document, range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    source_removal(document, range)
}

fn replace_set_operation_fragment(
    document: &SqlDocument,
    node: &SqlDocumentNode,
    operator: SqlDocumentSetOperator,
    quantifier: SqlDocumentSetQuantifier,
) -> Result<(SqlSourceRange, String), SqlDocumentEditError> {
    let SqlDocumentNodeData::SetOperation {
        operator: current_operator,
        quantifier: current_quantifier,
    } = node
        .semantic
        .as_ref()
        .ok_or(SqlDocumentEditError::InvalidSemanticReplacement)?
    else {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    };
    let operands = document
        .nodes
        .iter()
        .filter(|candidate| {
            candidate.parent == Some(node.id)
                && matches!(
                    candidate.kind,
                    SqlDocumentNodeKind::SetOperationLeft | SqlDocumentNodeKind::SetOperationRight
                )
        })
        .collect::<Vec<_>>();
    if operands.len() != 2 {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let mut operands = operands;
    operands.sort_by_key(|operand| operand.range.map(|range| range.start));
    let left = operands[0]
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let right = operands[1]
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if left.end > right.start {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let between = document
        .tokens
        .iter()
        .filter(|token| {
            token.range.start >= left.end
                && token.range.end <= right.start
                && !matches!(
                    token.kind,
                    SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
                )
        })
        .collect::<Vec<_>>();
    let current_operator_token = between
        .iter()
        .find(|token| token.kind == SqlDocumentTokenKind::Word)
        .copied()
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let current_operator_text = document
        .source_slice(current_operator_token.range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if !current_operator.eq_ignore_ascii_case(current_operator_text)
        || !matches!(
            current_operator_text.to_ascii_uppercase().as_str(),
            "UNION" | "INTERSECT" | "EXCEPT" | "MINUS"
        )
    {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }

    let remaining_words = between
        .iter()
        .skip_while(|token| **token != current_operator_token)
        .skip(1)
        .filter(|token| token.kind == SqlDocumentTokenKind::Word)
        .copied()
        .collect::<Vec<_>>();
    let current_quantifier_words = current_quantifier.split_whitespace().collect::<Vec<_>>();
    let quantifier_tokens = remaining_words
        .iter()
        .take(current_quantifier_words.len())
        .copied()
        .collect::<Vec<_>>();
    if quantifier_tokens.len() != current_quantifier_words.len()
        || !quantifier_tokens
            .iter()
            .zip(&current_quantifier_words)
            .all(|(token, word)| {
                document
                    .source_slice(token.range)
                    .is_some_and(|text| text.eq_ignore_ascii_case(word))
            })
    {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let existing_quantifier_range =
        quantifier_tokens
            .first()
            .zip(quantifier_tokens.last())
            .map(|(first, last)| SqlSourceRange {
                start: first.range.start,
                end: last.range.end,
            });

    let operator_spelling = match operator {
        SqlDocumentSetOperator::Union => "UNION",
        SqlDocumentSetOperator::Intersect => "INTERSECT",
        SqlDocumentSetOperator::Except => "EXCEPT",
    };
    let quantifier_spelling = quantifier.sql_spelling().trim_start();
    if let Some(existing_quantifier) = existing_quantifier_range {
        let range = SqlSourceRange {
            start: current_operator_token.range.start,
            end: existing_quantifier.end,
        };
        let between_operator_and_quantifier = document
            .source_slice(SqlSourceRange {
                start: current_operator_token.range.end,
                end: existing_quantifier.start,
            })
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        let replacement =
            format!("{operator_spelling}{between_operator_and_quantifier}{quantifier_spelling}");
        Ok((range, replacement))
    } else if !quantifier_spelling.is_empty() {
        Ok((
            current_operator_token.range,
            format!("{operator_spelling} {quantifier_spelling}"),
        ))
    } else {
        Ok((current_operator_token.range, operator_spelling.to_owned()))
    }
}

fn add_set_operation_operand(
    document: &SqlDocument,
    query_node: &SqlDocumentNode,
    operator: SqlDocumentSetOperator,
    quantifier: SqlDocumentSetQuantifier,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if query_node.kind != SqlDocumentNodeKind::Query {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let range = query_node
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let query_source = document
        .source_slice(range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if query_source.trim().is_empty() {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    Ok(SqlDocumentEdit {
        range,
        expected_text: query_source.to_owned(),
        replacement: format!(
            "({query_source}\n) {}{} SELECT 1",
            operator.sql_spelling(),
            quantifier.sql_spelling()
        ),
    })
}

fn remove_set_operation_operand(
    document: &SqlDocument,
    operation_node: &SqlDocumentNode,
    keep: SqlDocumentSetOperand,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if operation_node.kind != SqlDocumentNodeKind::SetOperation
        || !matches!(
            operation_node.semantic,
            Some(SqlDocumentNodeData::SetOperation { .. })
        )
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let operation_range = operation_node
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let keep_kind = match keep {
        SqlDocumentSetOperand::Left => SqlDocumentNodeKind::SetOperationLeft,
        SqlDocumentSetOperand::Right => SqlDocumentNodeKind::SetOperationRight,
    };
    let keep_range = document
        .nodes
        .iter()
        .find(|candidate| {
            candidate.parent == Some(operation_node.id) && candidate.kind == keep_kind
        })
        .and_then(|candidate| candidate.range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if keep_range.start < operation_range.start || keep_range.end > operation_range.end {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let mut replacement_range = operation_range;
    if replacement_range.start > 0
        && document.source.as_bytes().get(replacement_range.start - 1) == Some(&b'(')
    {
        replacement_range.start -= 1;
    }
    if document.source.as_bytes().get(replacement_range.end) == Some(&b')') {
        replacement_range.end += 1;
    }
    if document.tokens.iter().any(|token| {
        token.kind == SqlDocumentTokenKind::Comment
            && replacement_range.start <= token.range.start
            && token.range.end <= replacement_range.end
            && !(keep_range.start <= token.range.start && token.range.end <= keep_range.end)
    }) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let kept_source = document
        .source_slice(keep_range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let nested_set_operand = operation_node
        .parent
        .and_then(|parent_id| document.nodes.get(parent_id as usize))
        .is_some_and(|parent| {
            matches!(
                parent.kind,
                SqlDocumentNodeKind::SetOperationLeft | SqlDocumentNodeKind::SetOperationRight
            ) && parent
                .parent
                .and_then(|grandparent_id| document.nodes.get(grandparent_id as usize))
                .is_some_and(|grandparent| grandparent.kind == SqlDocumentNodeKind::SetOperation)
        });
    let replacement = if nested_set_operand {
        format!("({kept_source})")
    } else {
        kept_source.to_owned()
    };
    Ok(SqlDocumentEdit {
        range: replacement_range,
        expected_text: document
            .source_slice(replacement_range)
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?
            .to_owned(),
        replacement,
    })
}

fn swap_set_operation_operands(
    document: &SqlDocument,
    operation_node: &SqlDocumentNode,
) -> Result<Vec<SqlDocumentEdit>, SqlDocumentEditError> {
    if operation_node.kind != SqlDocumentNodeKind::SetOperation
        || !matches!(
            operation_node.semantic,
            Some(SqlDocumentNodeData::SetOperation { .. })
        )
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let operand_range = |kind| {
        document
            .nodes
            .iter()
            .find(|candidate| candidate.parent == Some(operation_node.id) && candidate.kind == kind)
            .and_then(|candidate| candidate.range)
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)
    };
    let left = operand_range(SqlDocumentNodeKind::SetOperationLeft)?;
    let right = operand_range(SqlDocumentNodeKind::SetOperationRight)?;
    if left.start >= left.end || left.end > right.start || right.start >= right.end {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let left_text = document
        .source_slice(left)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let right_text = document
        .source_slice(right)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if left_text == right_text {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    Ok(vec![
        SqlDocumentEdit {
            range: left,
            expected_text: left_text.to_owned(),
            replacement: right_text.to_owned(),
        },
        SqlDocumentEdit {
            range: right,
            expected_text: right_text.to_owned(),
            replacement: left_text.to_owned(),
        },
    ])
}

fn replace_set_operation_operand(
    document: &SqlDocument,
    target: &SqlDocumentNode,
    expression: &SqlDocumentNode,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if !matches!(
        target.kind,
        SqlDocumentNodeKind::SetOperationLeft | SqlDocumentNodeKind::SetOperationRight
    ) || !matches!(
        expression.kind,
        SqlDocumentNodeKind::Query
            | SqlDocumentNodeKind::Select
            | SqlDocumentNodeKind::SetOperation
    ) {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let parent = target
        .parent
        .and_then(|parent_id| document.nodes.get(parent_id as usize))
        .filter(|parent| parent.kind == SqlDocumentNodeKind::SetOperation)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let target_range = target
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let expression_range = expression
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let overlaps =
        target_range.start < expression_range.end && expression_range.start < target_range.end;
    if overlaps {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    if range_has_comment(document, target_range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let replacement_source = document
        .source_slice(expression_range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if replacement_source.trim().is_empty() || expression.id == parent.id {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let expected_text = document
        .source_slice(target_range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let replacement = format!("({replacement_source})");
    if replacement == expected_text {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    Ok(SqlDocumentEdit {
        range: target_range,
        expected_text: expected_text.to_owned(),
        replacement,
    })
}

fn rename_cte_and_references(
    document: &SqlDocument,
    node: &SqlDocumentNode,
    new_name: &SqlDocumentIdentifier,
) -> Result<Vec<SqlDocumentEdit>, SqlDocumentEditError> {
    let Some(SqlDocumentNodeData::CommonTableExpression { name: old_name, .. }) =
        node.semantic.as_ref()
    else {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    };
    if new_name.value.trim().is_empty() || sql_identifiers_match(old_name, new_name) {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let new_name_sql = render_identifier_parts(std::slice::from_ref(new_name))?;

    let with_clause_id = node
        .parent
        .filter(|parent_id| {
            document
                .nodes
                .get(*parent_id as usize)
                .is_some_and(|parent| {
                    parent.id == *parent_id && parent.kind == SqlDocumentNodeKind::WithClause
                })
        })
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let query_id = document
        .nodes
        .get(with_clause_id as usize)
        .and_then(|with_clause| with_clause.parent)
        .filter(|parent_id| {
            document
                .nodes
                .get(*parent_id as usize)
                .is_some_and(|parent| {
                    parent.id == *parent_id && parent.kind == SqlDocumentNodeKind::Query
                })
        })
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let target_query = document
        .nodes
        .get(query_id as usize)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let target_query_range = target_query
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let siblings = document
        .nodes
        .iter()
        .filter(|candidate| {
            candidate.parent == Some(with_clause_id)
                && candidate.kind == SqlDocumentNodeKind::CommonTableExpression
        })
        .collect::<Vec<_>>();
    let target_index = siblings
        .iter()
        .position(|candidate| candidate.id == node.id)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    for candidate in siblings
        .iter()
        .copied()
        .filter(|candidate| candidate.id != node.id)
    {
        let Some(SqlDocumentNodeData::CommonTableExpression { name, .. }) =
            candidate.semantic.as_ref()
        else {
            return Err(SqlDocumentEditError::NodeRangeUnavailable);
        };
        if sql_identifiers_match(name, new_name) {
            return Err(SqlDocumentEditError::InvalidSemanticReplacement);
        }
    }

    let recursive = matches!(
        document
            .nodes
            .get(with_clause_id as usize)
            .and_then(|with_clause| with_clause.semantic.as_ref()),
        Some(SqlDocumentNodeData::WithClause {
            recursive: true,
            ..
        })
    );

    let relation_nodes = document
        .nodes
        .iter()
        .filter_map(|candidate| match candidate.semantic.as_ref() {
            Some(SqlDocumentNodeData::Relation { name }) if name.len() == 1 => {
                Some((candidate, &name[0]))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if relation_nodes.iter().any(|(relation, relation_name)| {
        relation.range.is_some_and(|range| {
            target_query_range.start <= range.start && range.end <= target_query_range.end
        }) && sql_identifiers_match(relation_name, new_name)
            && !sql_identifiers_match(relation_name, old_name)
    }) {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }

    let cte_range = node
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let declaration_token = document
        .tokens
        .iter()
        .find(|token| {
            token.range.start >= cte_range.start
                && token.range.end <= cte_range.end
                && !matches!(
                    token.kind,
                    SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
                )
        })
        .filter(|token| {
            matches!(
                token.kind,
                SqlDocumentTokenKind::Word | SqlDocumentTokenKind::QuotedIdentifier
            )
        })
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let declaration_text = document
        .source_slice(declaration_token.range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    validate_identifier_fragment(declaration_text, std::slice::from_ref(old_name))
        .map_err(|_| SqlDocumentEditError::NodeRangeUnavailable)?;

    let mut edits = vec![SqlDocumentEdit {
        range: declaration_token.range,
        expected_text: declaration_text.to_owned(),
        replacement: new_name_sql.clone(),
    }];
    for (relation, relation_name) in
        relation_nodes
            .into_iter()
            .filter(|(relation, relation_name)| {
                sql_identifiers_match(relation_name, old_name)
                    && cte_reference_resolves_to_target(
                        document,
                        relation,
                        node.id,
                        with_clause_id,
                        target_index,
                        recursive,
                    )
            })
    {
        let range = relation
            .range
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        let current_text = document
            .source_slice(range)
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        validate_identifier_fragment(current_text, std::slice::from_ref(relation_name))
            .map_err(|_| SqlDocumentEditError::NodeRangeUnavailable)?;
        edits.push(SqlDocumentEdit {
            range,
            expected_text: current_text.to_owned(),
            replacement: new_name_sql.clone(),
        });
    }
    Ok(edits)
}

/// Returns whether a one-part relation name resolves to `target_cte_id` from
/// its nearest lexical WITH scope. Nested CTEs with the same name shadow the
/// target and are intentionally left untouched.
fn cte_reference_resolves_to_target(
    document: &SqlDocument,
    relation: &SqlDocumentNode,
    target_cte_id: u32,
    target_with_clause_id: u32,
    target_index: usize,
    target_recursive: bool,
) -> bool {
    let mut ancestor_queries = Vec::new();
    let mut current = relation.id;
    loop {
        let Some(node) = document.nodes.get(current as usize) else {
            return false;
        };
        if node.id != current {
            return false;
        }
        if node.kind == SqlDocumentNodeKind::Query {
            ancestor_queries.push(node.id);
        }
        let Some(parent) = node.parent else {
            break;
        };
        current = parent;
    }

    for query_id in ancestor_queries {
        let Some(with_clause) = document.nodes.iter().find(|candidate| {
            candidate.parent == Some(query_id) && candidate.kind == SqlDocumentNodeKind::WithClause
        }) else {
            continue;
        };
        let local_ctes = document
            .nodes
            .iter()
            .filter(|candidate| {
                candidate.parent == Some(with_clause.id)
                    && candidate.kind == SqlDocumentNodeKind::CommonTableExpression
            })
            .collect::<Vec<_>>();
        let Some(local_match) = local_ctes.iter().find(|candidate| {
            matches!(
                candidate.semantic.as_ref(),
                Some(SqlDocumentNodeData::CommonTableExpression { name, .. })
                    if relation.semantic.as_ref().is_some_and(|relation_data| {
                        matches!(relation_data, SqlDocumentNodeData::Relation { name: relation_name }
                            if relation_name.len() == 1 && sql_identifiers_match(&relation_name[0], name))
                    })
            )
        }) else {
            continue;
        };
        if local_match.id != target_cte_id || with_clause.id != target_with_clause_id {
            // A closer same-name CTE shadows the target. A same-name CTE in
            // another lexical scope is also a distinct binding.
            return false;
        }

        let target_body_is_self = local_ctes[target_index]
            .range
            .zip(relation.range)
            .is_some_and(|(cte_range, relation_range)| {
                cte_range.start <= relation_range.start && relation_range.end <= cte_range.end
            });
        let containing_cte_index = local_ctes.iter().position(|cte| {
            cte.range
                .zip(relation.range)
                .is_some_and(|(cte_range, relation_range)| {
                    cte_range.start <= relation_range.start && relation_range.end <= cte_range.end
                })
        });
        return match containing_cte_index {
            Some(index) if index == target_index => target_recursive && target_body_is_self,
            Some(index) => target_recursive || target_index < index,
            None => true,
        };
    }
    false
}

fn add_common_table_expression(
    document: &SqlDocument,
    query_node: &SqlDocumentNode,
    name: &SqlDocumentIdentifier,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if query_node.kind != SqlDocumentNodeKind::Query || name.value.trim().is_empty() {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let with_clause = document.nodes.iter().find(|candidate| {
        candidate.parent == Some(query_node.id) && candidate.kind == SqlDocumentNodeKind::WithClause
    });
    if with_clause.is_some_and(|with_clause| {
        document
            .nodes
            .iter()
            .filter(|candidate| {
                candidate.parent == Some(with_clause.id)
                    && candidate.kind == SqlDocumentNodeKind::CommonTableExpression
            })
            .any(|candidate| match candidate.semantic.as_ref() {
                Some(SqlDocumentNodeData::CommonTableExpression { name: current, .. }) => {
                    sql_identifiers_match(current, name)
                }
                _ => true,
            })
    }) {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let name_sql = render_identifier_parts(std::slice::from_ref(name))?;
    let query_range = query_node
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let (range, replacement) = if let Some(with_clause) = with_clause {
        let Some(SqlDocumentNodeData::WithClause { cte_count, .. }) = with_clause.semantic.as_ref()
        else {
            return Err(SqlDocumentEditError::NodeRangeUnavailable);
        };
        if *cte_count >= 64 {
            return Err(SqlDocumentEditError::InvalidSemanticReplacement);
        }
        let with_range = with_clause
            .range
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        (
            SqlSourceRange {
                start: with_range.end,
                end: with_range.end,
            },
            format!(", {name_sql} AS (SELECT 1)"),
        )
    } else {
        (
            SqlSourceRange {
                start: query_range.start,
                end: query_range.start,
            },
            format!("WITH {name_sql} AS (SELECT 1) "),
        )
    };
    Ok(SqlDocumentEdit {
        range,
        expected_text: String::new(),
        replacement,
    })
}

fn remove_common_table_expression(
    document: &SqlDocument,
    cte_node: &SqlDocumentNode,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if cte_node.kind != SqlDocumentNodeKind::CommonTableExpression {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let Some(SqlDocumentNodeData::CommonTableExpression { name, .. }) = cte_node.semantic.as_ref()
    else {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    };
    let with_clause = document
        .nodes
        .iter()
        .find(|candidate| {
            candidate.id == cte_node.parent.unwrap_or(u32::MAX)
                && candidate.kind == SqlDocumentNodeKind::WithClause
        })
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let siblings = document
        .nodes
        .iter()
        .filter(|candidate| {
            candidate.parent == Some(with_clause.id)
                && candidate.kind == SqlDocumentNodeKind::CommonTableExpression
        })
        .collect::<Vec<_>>();
    let target_index = siblings
        .iter()
        .position(|candidate| candidate.id == cte_node.id)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let recursive = matches!(
        with_clause.semantic.as_ref(),
        Some(SqlDocumentNodeData::WithClause {
            recursive: true,
            ..
        })
    );
    if document.nodes.iter().any(|candidate| {
        matches!(
            candidate.semantic.as_ref(),
            Some(SqlDocumentNodeData::Relation { name: parts })
                if parts.len() == 1
                    && sql_identifiers_match(&parts[0], name)
                    && cte_reference_resolves_to_target(
                        document,
                        candidate,
                        cte_node.id,
                        with_clause.id,
                        target_index,
                        recursive,
                    )
        )
    }) {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let with_range = with_clause
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let mut siblings = document
        .nodes
        .iter()
        .filter(|candidate| {
            candidate.parent == Some(with_clause.id)
                && candidate.kind == SqlDocumentNodeKind::CommonTableExpression
        })
        .collect::<Vec<_>>();
    siblings.sort_by_key(|candidate| candidate.range.map(|range| range.start));
    let index = siblings
        .iter()
        .position(|candidate| candidate.id == cte_node.id)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let selected_range = cte_node
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let range = if siblings.len() == 1 {
        let mut end = with_range.end;
        while let Some(character) = document
            .source
            .get(end..)
            .and_then(|remaining| remaining.chars().next())
        {
            if !character.is_whitespace() {
                break;
            }
            end += character.len_utf8();
        }
        SqlSourceRange {
            start: with_range.start,
            end,
        }
    } else if index > 0 {
        let previous_range = siblings[index - 1]
            .range
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        SqlSourceRange {
            start: previous_range.end,
            end: selected_range.end,
        }
    } else {
        let next_range = siblings[index + 1]
            .range
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        SqlSourceRange {
            start: selected_range.start,
            end: next_range.start,
        }
    };
    if document.tokens.iter().any(|token| {
        token.kind == SqlDocumentTokenKind::Comment
            && range.start <= token.range.start
            && token.range.end <= range.end
    }) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let expected_text = document
        .source_slice(range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?
        .to_owned();
    Ok(SqlDocumentEdit {
        range,
        expected_text,
        replacement: String::new(),
    })
}

fn replace_common_table_expression_query(
    document: &SqlDocument,
    cte_node: &SqlDocumentNode,
    query_node: &SqlDocumentNode,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if cte_node.kind != SqlDocumentNodeKind::CommonTableExpression
        || query_node.kind != SqlDocumentNodeKind::Query
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let target = document
        .nodes
        .iter()
        .find(|candidate| {
            candidate.parent == Some(cte_node.id) && candidate.kind == SqlDocumentNodeKind::Query
        })
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let target_range = target
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let source_range = query_node
        .range
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if target.id == query_node.id
        || (target_range.start < source_range.end && source_range.start < target_range.end)
    {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    if range_has_comment(document, target_range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let replacement = document
        .source_slice(source_range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let expected_text = document
        .source_slice(target_range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if replacement.trim().is_empty() || replacement == expected_text {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    Ok(SqlDocumentEdit {
        range: target_range,
        expected_text: expected_text.to_owned(),
        replacement: replacement.to_owned(),
    })
}

fn move_common_table_expression(
    document: &SqlDocument,
    cte_node: &SqlDocumentNode,
    to_index: usize,
) -> Result<SqlDocumentEdit, SqlDocumentEditError> {
    if cte_node.kind != SqlDocumentNodeKind::CommonTableExpression {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let with_clause = cte_node
        .parent
        .and_then(|parent_id| document.nodes.get(parent_id as usize))
        .filter(|node| node.kind == SqlDocumentNodeKind::WithClause)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let mut ctes = document
        .nodes
        .iter()
        .filter(|node| {
            node.parent == Some(with_clause.id)
                && node.kind == SqlDocumentNodeKind::CommonTableExpression
        })
        .collect::<Vec<_>>();
    ctes.sort_by_key(|node| node.range.map(|range| range.start));
    let from_index = ctes
        .iter()
        .position(|node| node.id == cte_node.id)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    if to_index >= ctes.len() || to_index == from_index || ctes.len() < 2 {
        return Err(SqlDocumentEditError::InvalidSemanticReplacement);
    }
    let first = ctes
        .first()
        .and_then(|node| node.range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let last = ctes
        .last()
        .and_then(|node| node.range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
    let range = SqlSourceRange {
        start: first.start,
        end: last.end,
    };
    if range_has_comment(document, range) {
        return Err(SqlDocumentEditError::NodeRangeUnavailable);
    }
    let mut sources = ctes
        .iter()
        .map(|node| {
            let range = node
                .range
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
            document
                .source_slice(range)
                .map(str::to_owned)
                .ok_or(SqlDocumentEditError::NodeRangeUnavailable)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let moved = sources.remove(from_index);
    sources.insert(to_index, moved);
    let expected_text = document
        .source_slice(range)
        .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?
        .to_owned();
    Ok(SqlDocumentEdit {
        range,
        expected_text,
        replacement: sources.join(", "),
    })
}

fn sql_identifiers_match(left: &SqlDocumentIdentifier, right: &SqlDocumentIdentifier) -> bool {
    match (left.quote_style, right.quote_style) {
        (None, None) => left.value.eq_ignore_ascii_case(&right.value),
        _ => left.value == right.value,
    }
}

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

fn valid_sql_parameter_name(name: &str) -> bool {
    let mut characters = name.chars();
    matches!(characters.next(), Some(first) if first == '_' || first.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
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
pub fn apply_sql_document_edit_transaction(
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
pub fn parse_sql_document(source: &str) -> SqlDocument {
    let mut document = SqlDocument {
        contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
        source: source.to_owned(),
        status: SqlDocumentStatus::InvalidSql,
        diagnostics: Vec::new(),
        tokens: Vec::new(),
        nodes: Vec::new(),
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

    document
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
            if let Some(predicate_range) = self.span_range(selection) {
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
            .and_then(|expression| self.span_range(expression).map(|range| range.end))
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
                            end: group_range.end,
                        };
                        let clause_id = self.add_node(
                            SqlDocumentNodeKind::GroupByClause,
                            Some(range),
                            select_id,
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
                        previous_clause_end = group_range.end;
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
                        self.add_node(SqlDocumentNodeKind::GroupByClause, Some(range), select_id);
                        previous_clause_end = all_range.end;
                    }
                }
            }
        }

        if let Some(having) = &select.having {
            if let Some(predicate_range) = self.span_range(having) {
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

fn next_nontrivia_token(tokens: &[SqlDocumentToken], after: usize) -> Option<usize> {
    (after + 1..tokens.len()).find(|index| {
        !matches!(
            tokens[*index].kind,
            SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
        )
    })
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
fn cast_target_type_source_range(
    document: &SqlDocument,
    node: &SqlDocumentNode,
    cast_kind: &str,
    target_type: &str,
) -> Option<SqlSourceRange> {
    let wrapper_range = node.range?;
    let expected_type = normalized_sql_token_text(target_type)?;
    if expected_type.is_empty() {
        return None;
    }
    let source = &document.source;
    let tokens = &document.tokens;

    let candidate_matches = |range: SqlSourceRange| {
        source
            .get(range.start..range.end)
            .and_then(normalized_sql_token_text)
            .as_ref()
            == Some(&expected_type)
    };

    let range = match cast_kind {
        "cast" | "try_cast" | "safe_cast" => {
            let keyword = tokens.iter().position(|token| {
                token.range.start == wrapper_range.start
                    && token.kind == SqlDocumentTokenKind::Word
                    && token_text(source, token).is_some_and(|text| {
                        text.eq_ignore_ascii_case(match cast_kind {
                            "cast" => "CAST",
                            "try_cast" => "TRY_CAST",
                            _ => "SAFE_CAST",
                        })
                    })
            })?;
            let open_index = skip_trivia_tokens(tokens, keyword + 1);
            if token_text(source, tokens.get(open_index)?)? != "(" {
                return None;
            }
            let close_index = matching_close_paren(source, tokens, open_index)?;
            let as_index = top_level_cast_as_token(source, tokens, open_index, close_index)?;
            argument_source_range(tokens, as_index + 1, close_index)?
        }
        "double_colon" => {
            let mut matches = Vec::new();
            for (index, token) in tokens.iter().enumerate() {
                if token.range.start < wrapper_range.start
                    || token.range.end > wrapper_range.end
                    || token_text(source, token)? != "::"
                {
                    continue;
                }
                let range = argument_source_range(
                    tokens,
                    index + 1,
                    tokens
                        .iter()
                        .position(|candidate| candidate.range.start >= wrapper_range.end)
                        .unwrap_or(tokens.len()),
                )?;
                if range.start >= wrapper_range.start
                    && range.end <= wrapper_range.end
                    && candidate_matches(range)
                {
                    matches.push(range);
                }
            }
            if matches.len() != 1 {
                return None;
            }
            matches[0]
        }
        "convert" | "try_convert" => {
            let keyword = tokens.iter().position(|token| {
                token.range.start == wrapper_range.start
                    && token.kind == SqlDocumentTokenKind::Word
                    && token_text(source, token).is_some_and(|text| {
                        text.eq_ignore_ascii_case(if cast_kind == "try_convert" {
                            "TRY_CONVERT"
                        } else {
                            "CONVERT"
                        })
                    })
            })?;
            let open_index = skip_trivia_tokens(tokens, keyword + 1);
            if token_text(source, tokens.get(open_index)?)? != "(" {
                return None;
            }
            let close_index = matching_close_paren(source, tokens, open_index)?;
            let commas = top_level_comma_tokens(source, tokens, open_index, close_index)?;
            if commas.len() != 1 {
                return None;
            }
            let comma_index = commas[0];
            let candidates = [
                argument_source_range(tokens, open_index + 1, comma_index)?,
                argument_source_range(tokens, comma_index + 1, close_index)?,
            ];
            let matching = candidates
                .into_iter()
                .filter(|range| candidate_matches(*range))
                .collect::<Vec<_>>();
            if matching.len() != 1 {
                return None;
            }
            matching[0]
        }
        _ => return None,
    };

    (range.start >= wrapper_range.start
        && range.end <= wrapper_range.end
        && !range_has_comment(document, range)
        && candidate_matches(range))
    .then_some(range)
}

/// Resolve a postfix `<expr>::TYPE` wrapper using the recursively resolved
/// source span of its operand and the parser-derived target type. If the exact
/// target token sequence cannot be located, return no range instead of the
/// incomplete span sqlparser assigns to this expression variant.
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
    SqlDocumentNodeData::TableFactor {
        factor_kind: factor_kind.to_owned(),
        alias: alias.map(|alias| identifier_data(&alias.name)),
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

    match expression {
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

    fn semantic_node_id(document: &SqlDocument, kind: SqlDocumentNodeKind, fragment: &str) -> u32 {
        let nodes = document
            .nodes
            .iter()
            .filter(|node| {
                node.kind == kind
                    && node.range.and_then(|range| document.source_slice(range)) == Some(fragment)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            nodes.len(),
            1,
            "expected one {kind:?} node for {fragment:?}"
        );
        nodes[0].id
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
    fn typed_node_edit_preserves_aliases_literals_comments_and_other_clauses() {
        let source = "-- report\nSELECT /* left */ \"Amount\" AS \"Total\", 'paid' AS label FROM \"G\" WHERE \"Amount\" > 1 ORDER BY \"Total\" DESC /* tail */";
        let document = parse_sql_document(source);
        assert_eq!(document.status, SqlDocumentStatus::Parsed);
        assert_eq!(document.to_sql().as_bytes(), source.as_bytes());

        let predicate = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::BinaryExpression
                    && node.range.and_then(|range| document.source_slice(range))
                        == Some("\"Amount\" > 1")
            })
            .expect("the WHERE comparison has a typed expression node");
        let changed = apply_sql_document_node_edit_transaction(
            source,
            &SqlDocumentNodeEditTransaction {
                contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                expected_source: source.to_owned(),
                edits: vec![SqlDocumentNodeEdit {
                    node_id: predicate.id,
                    expected_kind: SqlDocumentNodeKind::BinaryExpression,
                    replacement: "\"Amount\" >= 2".to_owned(),
                }],
            },
        )
        .expect("the selected expression can be replaced atomically");

        let expected = "-- report\nSELECT /* left */ \"Amount\" AS \"Total\", 'paid' AS label FROM \"G\" WHERE \"Amount\" >= 2 ORDER BY \"Total\" DESC /* tail */";
        assert_eq!(changed.document.status, SqlDocumentStatus::Parsed);
        assert_eq!(changed.document.to_sql(), expected);
        assert!(changed.document.to_sql().contains("AS \"Total\""));
        assert!(changed.document.to_sql().contains("'paid' AS label"));
        assert!(changed.document.to_sql().contains("/* left */"));
        assert!(changed.document.to_sql().ends_with("/* tail */"));
        assert!(changed.document.to_sql().contains("FROM \"G\""));
        assert!(changed
            .document
            .to_sql()
            .contains("ORDER BY \"Total\" DESC"));

        let undone = apply_sql_document_edit_transaction(&changed.document.source, &changed.undo)
            .expect("the typed edit returns a guarded inverse transaction");
        assert_eq!(undone.document.to_sql().as_bytes(), source.as_bytes());
    }

    #[test]
    fn typed_node_edits_reject_stale_or_mistyped_targets() {
        let source = "SELECT amount FROM \"G\" WHERE amount > 1";
        let document = parse_sql_document(source);
        let predicate = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::BinaryExpression)
            .expect("the comparison has a typed expression node");
        let node_edit = SqlDocumentNodeEdit {
            node_id: predicate.id,
            expected_kind: SqlDocumentNodeKind::IdentifierExpression,
            replacement: "amount >= 2".to_owned(),
        };

        assert_eq!(
            apply_sql_document_node_edit_transaction(
                source,
                &SqlDocumentNodeEditTransaction {
                    contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                    expected_source: "SELECT changed FROM \"G\" WHERE amount > 1".to_owned(),
                    edits: vec![node_edit.clone()],
                },
            ),
            Err(SqlDocumentEditError::StaleSource)
        );
        assert_eq!(
            apply_sql_document_node_edit_transaction(
                source,
                &SqlDocumentNodeEditTransaction {
                    contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                    expected_source: source.to_owned(),
                    edits: vec![node_edit],
                },
            ),
            Err(SqlDocumentEditError::NodeKindMismatch)
        );

        assert_eq!(
            apply_sql_document_node_edit_transaction(
                source,
                &SqlDocumentNodeEditTransaction {
                    contract_version: SQL_DOCUMENT_CONTRACT_VERSION + 1,
                    expected_source: source.to_owned(),
                    edits: vec![],
                },
            ),
            Err(SqlDocumentEditError::ContractVersionMismatch)
        );
        assert_eq!(
            apply_sql_document_node_edit_transaction(
                "SELECT (",
                &SqlDocumentNodeEditTransaction {
                    contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                    expected_source: "SELECT (".to_owned(),
                    edits: vec![],
                },
            ),
            Err(SqlDocumentEditError::DocumentNotParsed)
        );

        let select_item = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .expect("the SELECT projection has a typed node");
        let duplicate_target = SqlDocumentNodeEdit {
            node_id: select_item.id,
            expected_kind: SqlDocumentNodeKind::SelectItem,
            replacement: "other".to_owned(),
        };
        assert_eq!(
            apply_sql_document_node_edit_transaction(
                source,
                &SqlDocumentNodeEditTransaction {
                    contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                    expected_source: source.to_owned(),
                    edits: vec![duplicate_target.clone(), duplicate_target],
                },
            ),
            Err(SqlDocumentEditError::DuplicateNodeTarget)
        );
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
        for source in [
            "SELECT 1",
            "SELECT (",
            "DELETE FROM x",
            "SELECT 1; SELECT 2",
        ] {
            assert_eq!(
                apply_sql_document_semantic_edit_transaction(
                    source,
                    &semantic_transaction(source, vec![initialize.clone()])
                ),
                Err(SqlDocumentEditError::InvalidSemanticReplacement)
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
    fn semantic_edits_render_typed_leaves_and_preserve_untouched_source() {
        let source = "-- heading\nSELECT display_value AS \"Display\", 1 AS n, 'old' AS text, TRUE AS enabled, $old AS parameter FROM \"G\" WHERE amount /* left */ + /* operator */ 2 > 3";
        let document = parse_sql_document(source);
        assert_eq!(document.status, SqlDocumentStatus::Parsed);

        let identifier_id = semantic_node_id(
            &document,
            SqlDocumentNodeKind::IdentifierExpression,
            "display_value",
        );
        let number_id = semantic_node_id(&document, SqlDocumentNodeKind::Value, "1");
        let string_id = semantic_node_id(&document, SqlDocumentNodeKind::Value, "'old'");
        let boolean_id = semantic_node_id(&document, SqlDocumentNodeKind::Value, "TRUE");
        let parameter_id = semantic_node_id(&document, SqlDocumentNodeKind::Value, "$old");
        let binary_node_id = |operator: &str| {
            let nodes = document
                .nodes
                .iter()
                .filter(|node| {
                    node.kind == SqlDocumentNodeKind::BinaryExpression
                        && matches!(
                            &node.semantic,
                            Some(SqlDocumentNodeData::BinaryOperator { operator: value })
                                if value == operator
                        )
                })
                .collect::<Vec<_>>();
            assert_eq!(nodes.len(), 1, "expected one binary {operator:?} node");
            nodes[0].id
        };
        let plus_id = binary_node_id("+");
        let greater_id = binary_node_id(">");

        let edits = vec![
            SqlDocumentSemanticEdit::ReplaceIdentifier {
                node_id: identifier_id,
                expected_kind: SqlDocumentNodeKind::IdentifierExpression,
                parts: vec![
                    SqlDocumentIdentifier {
                        value: "sales".to_owned(),
                        quote_style: Some('"'),
                    },
                    SqlDocumentIdentifier {
                        value: "display".to_owned(),
                        quote_style: Some('"'),
                    },
                ],
            },
            SqlDocumentSemanticEdit::ReplaceLiteral {
                node_id: number_id,
                expected_kind: SqlDocumentNodeKind::Value,
                literal: SqlDocumentSemanticLiteral::Number {
                    value: "9007199254740993".to_owned(),
                },
            },
            SqlDocumentSemanticEdit::ReplaceLiteral {
                node_id: string_id,
                expected_kind: SqlDocumentNodeKind::Value,
                literal: SqlDocumentSemanticLiteral::String {
                    value: "paid' now".to_owned(),
                },
            },
            SqlDocumentSemanticEdit::ReplaceLiteral {
                node_id: boolean_id,
                expected_kind: SqlDocumentNodeKind::Value,
                literal: SqlDocumentSemanticLiteral::Boolean { value: false },
            },
            SqlDocumentSemanticEdit::ReplaceLiteral {
                node_id: parameter_id,
                expected_kind: SqlDocumentNodeKind::Value,
                literal: SqlDocumentSemanticLiteral::NamedParameter {
                    name: "new_day".to_owned(),
                },
            },
            SqlDocumentSemanticEdit::ReplaceBinaryOperator {
                node_id: plus_id,
                expected_kind: SqlDocumentNodeKind::BinaryExpression,
                operator: SqlDocumentBinaryOperator::Concat,
            },
            SqlDocumentSemanticEdit::ReplaceBinaryOperator {
                node_id: greater_id,
                expected_kind: SqlDocumentNodeKind::BinaryExpression,
                operator: SqlDocumentBinaryOperator::Gte,
            },
        ];
        let serialized = serde_json::to_value(&edits[0]).unwrap();
        assert_eq!(serialized["kind"], "replace_identifier");
        assert_eq!(serialized["expected_kind"], "identifier_expression");
        assert_eq!(serialized["parts"][0]["quote_style"], "\"");
        let serialized_literal = serde_json::to_value(&edits[1]).unwrap();
        assert_eq!(serialized_literal["literal"]["kind"], "number");
        assert_eq!(serialized_literal["literal"]["value"], "9007199254740993");
        let serialized_operator = serde_json::to_value(&edits[5]).unwrap();
        assert_eq!(serialized_operator["kind"], "replace_binary_operator");
        assert_eq!(serialized_operator["operator"], "concat");

        let changed = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(source, edits),
        )
        .expect("typed semantic edits should be rendered and applied atomically");
        let expected = "-- heading\nSELECT \"sales\".\"display\" AS \"Display\", 9007199254740993 AS n, 'paid'' now' AS text, FALSE AS enabled, $new_day AS parameter FROM \"G\" WHERE amount /* left */ || /* operator */ 2 >= 3";
        assert_eq!(changed.document.status, SqlDocumentStatus::Parsed);
        assert_eq!(changed.document.to_sql(), expected);

        let undone = apply_sql_document_edit_transaction(&changed.document.source, &changed.undo)
            .expect("typed edit undo should restore the guarded source");
        assert_eq!(undone.document.to_sql().as_bytes(), source.as_bytes());
    }

    #[test]
    fn semantic_edits_reject_arbitrary_fragments_and_invalid_identifiers() {
        let source = "SELECT 1, display_value FROM \"G\"";
        let document = parse_sql_document(source);
        let number_id = semantic_node_id(&document, SqlDocumentNodeKind::Value, "1");
        let identifier_id = semantic_node_id(
            &document,
            SqlDocumentNodeKind::IdentifierExpression,
            "display_value",
        );

        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                source,
                &semantic_transaction(
                    source,
                    vec![SqlDocumentSemanticEdit::ReplaceLiteral {
                        node_id: number_id,
                        expected_kind: SqlDocumentNodeKind::Value,
                        literal: SqlDocumentSemanticLiteral::Number {
                            value: "1; DELETE FROM G".to_owned(),
                        },
                    }],
                ),
            ),
            Err(SqlDocumentEditError::InvalidSemanticReplacement)
        );
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                source,
                &semantic_transaction(
                    source,
                    vec![SqlDocumentSemanticEdit::ReplaceIdentifier {
                        node_id: identifier_id,
                        expected_kind: SqlDocumentNodeKind::IdentifierExpression,
                        parts: vec![SqlDocumentIdentifier {
                            value: "unsafe'name".to_owned(),
                            quote_style: Some('\''),
                        }],
                    }],
                ),
            ),
            Err(SqlDocumentEditError::InvalidSemanticReplacement)
        );
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                source,
                &semantic_transaction(
                    source,
                    vec![SqlDocumentSemanticEdit::ReplaceLiteral {
                        node_id: number_id,
                        expected_kind: SqlDocumentNodeKind::Value,
                        literal: SqlDocumentSemanticLiteral::NamedParameter {
                            name: "x; DROP".to_owned(),
                        },
                    }],
                ),
            ),
            Err(SqlDocumentEditError::InvalidSemanticReplacement)
        );
    }

    #[test]
    fn semantic_set_operation_edit_preserves_comments_and_supports_undo() {
        let source = "SELECT 1 /* left */ UNION /* operator */ ALL /* right */ SELECT 2";
        let document = parse_sql_document(source);
        assert_eq!(document.status, SqlDocumentStatus::Parsed);
        let node = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SetOperation)
            .expect("set operation is represented in the document");

        let changed = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::ReplaceSetOperation {
                    node_id: node.id,
                    expected_kind: SqlDocumentNodeKind::SetOperation,
                    operator: SqlDocumentSetOperator::Except,
                    quantifier: SqlDocumentSetQuantifier::Distinct,
                }],
            ),
        )
        .expect("a typed set operation edit should be applied");
        assert_eq!(
            changed.document.to_sql(),
            "SELECT 1 /* left */ EXCEPT /* operator */ DISTINCT /* right */ SELECT 2"
        );
        assert_eq!(changed.document.status, SqlDocumentStatus::Parsed);

        let undone = apply_sql_document_edit_transaction(&changed.document.source, &changed.undo)
            .expect("set operation edit undo restores the exact source");
        assert_eq!(undone.document.to_sql().as_bytes(), source.as_bytes());

        let defaulted = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::ReplaceSetOperation {
                    node_id: node.id,
                    expected_kind: SqlDocumentNodeKind::SetOperation,
                    operator: SqlDocumentSetOperator::Intersect,
                    quantifier: SqlDocumentSetQuantifier::Default,
                }],
            ),
        )
        .expect("the default quantifier is editable");
        assert_eq!(
            defaulted.document.to_sql(),
            "SELECT 1 /* left */ INTERSECT /* operator */  /* right */ SELECT 2"
        );

        let no_quantifier = "SELECT 1 UNION /* separator */ SELECT 2";
        let no_quantifier_document = parse_sql_document(no_quantifier);
        let no_quantifier_node = no_quantifier_document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SetOperation)
            .expect("set operation without a quantifier is represented");
        let with_all = apply_sql_document_semantic_edit_transaction(
            no_quantifier,
            &semantic_transaction(
                no_quantifier,
                vec![SqlDocumentSemanticEdit::ReplaceSetOperation {
                    node_id: no_quantifier_node.id,
                    expected_kind: SqlDocumentNodeKind::SetOperation,
                    operator: SqlDocumentSetOperator::Union,
                    quantifier: SqlDocumentSetQuantifier::All,
                }],
            ),
        )
        .expect("an explicit ALL quantifier can be inserted");
        assert_eq!(
            with_all.document.to_sql(),
            "SELECT 1 UNION ALL /* separator */ SELECT 2"
        );
    }

    #[test]
    fn semantic_set_operation_operand_can_be_replaced_from_a_typed_node_and_undone() {
        let source = "SELECT 1 AS v UNION ALL SELECT 2 AS v";
        let document = parse_sql_document(source);
        let right_operand = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SetOperationRight)
            .expect("the right set operand has a typed node");
        let left_select = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::Select
                    && node.range.and_then(|range| document.source_slice(range))
                        == Some("SELECT 1 AS v")
            })
            .expect("the left SELECT is a parser-backed source operand");
        let replaced = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::ReplaceSetOperationOperand {
                    node_id: right_operand.id,
                    expected_kind: SqlDocumentNodeKind::SetOperationRight,
                    expression_node_id: left_select.id,
                    expression_expected_kind: SqlDocumentNodeKind::Select,
                }],
            ),
        )
        .expect("a set operand can be replaced with a typed SELECT node");
        assert_eq!(replaced.document.status, SqlDocumentStatus::Parsed);
        assert_eq!(
            replaced.document.to_sql(),
            "SELECT 1 AS v UNION ALL (SELECT 1 AS v)"
        );
        let undone = apply_sql_document_edit_transaction(&replaced.document.source, &replaced.undo)
            .expect("the operand replacement has an exact inverse");
        assert_eq!(undone.document.to_sql().as_bytes(), source.as_bytes());

        let commented_source = "SELECT 1 AS v UNION ALL SELECT (/* keep */ 2) AS v";
        let commented = parse_sql_document(commented_source);
        let right_operand = commented
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SetOperationRight)
            .expect("the commented right operand is represented");
        let left_select = commented
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::Select
                    && node.range.and_then(|range| commented.source_slice(range))
                        == Some("SELECT 1 AS v")
            })
            .expect("the left source operand is represented");
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                commented_source,
                &semantic_transaction(
                    commented_source,
                    vec![SqlDocumentSemanticEdit::ReplaceSetOperationOperand {
                        node_id: right_operand.id,
                        expected_kind: SqlDocumentNodeKind::SetOperationRight,
                        expression_node_id: left_select.id,
                        expression_expected_kind: SqlDocumentNodeKind::Select,
                    }],
                ),
            ),
            Err(SqlDocumentEditError::NodeRangeUnavailable),
            "replacement must fail closed if it would discard comments in the operand"
        );
    }

    #[test]
    fn semantic_set_operation_by_name_quantifiers_are_representable_and_undoable() {
        for (quantifier, expected) in [
            (SqlDocumentSetQuantifier::ByName, "BY NAME"),
            (SqlDocumentSetQuantifier::AllByName, "ALL BY NAME"),
            (SqlDocumentSetQuantifier::DistinctByName, "DISTINCT BY NAME"),
        ] {
            let source = "SELECT 1 AS a UNION SELECT 2 AS a";
            let document = parse_sql_document(source);
            let operation = document
                .nodes
                .iter()
                .find(|node| node.kind == SqlDocumentNodeKind::SetOperation)
                .expect("the parser exposes the set operation");
            let changed = apply_sql_document_semantic_edit_transaction(
                source,
                &semantic_transaction(
                    source,
                    vec![SqlDocumentSemanticEdit::ReplaceSetOperation {
                        node_id: operation.id,
                        expected_kind: SqlDocumentNodeKind::SetOperation,
                        operator: SqlDocumentSetOperator::Union,
                        quantifier,
                    }],
                ),
            )
            .expect("the typed selector can represent parser-supported BY NAME quantifiers");
            assert_eq!(changed.document.status, SqlDocumentStatus::Parsed);
            assert_eq!(
                changed.document.to_sql(),
                format!("SELECT 1 AS a UNION {expected} SELECT 2 AS a")
            );
            let undone =
                apply_sql_document_edit_transaction(&changed.document.source, &changed.undo)
                    .expect("BY NAME quantifier edits have a guarded inverse");
            assert_eq!(undone.document.to_sql().as_bytes(), source.as_bytes());
        }
    }

    #[test]
    fn semantic_set_operation_operands_can_be_swapped_and_undone() {
        let source = "SELECT 1 /* left */ UNION ALL SELECT 2 /* right */";
        let document = parse_sql_document(source);
        assert_eq!(document.status, SqlDocumentStatus::Parsed);
        let operation = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SetOperation)
            .expect("set operation is represented in the syntax tree");

        let swapped = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::SwapSetOperationOperands {
                    node_id: operation.id,
                    expected_kind: SqlDocumentNodeKind::SetOperation,
                }],
            ),
        )
        .expect("a typed set-operation reorder should be applied");
        assert_eq!(swapped.document.status, SqlDocumentStatus::Parsed);
        assert_eq!(
            swapped.document.to_sql(),
            "SELECT 2 /* left */ UNION ALL SELECT 1 /* right */"
        );
        assert_eq!(swapped.document.to_sql().matches("/* left */").count(), 1);
        assert_eq!(swapped.document.to_sql().matches("/* right */").count(), 1);

        let undone = apply_sql_document_edit_transaction(&swapped.document.source, &swapped.undo)
            .expect("operand reorder returns guarded undo");
        assert_eq!(undone.document.to_sql().as_bytes(), source.as_bytes());
    }

    #[test]
    fn semantic_set_operation_add_seed_and_operand_removal_are_typed_and_undoable() {
        let source = "SELECT 1";
        let document = parse_sql_document(source);
        let query = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::Query
                    && node
                        .range
                        .and_then(|range| document.source_slice(range))
                        .is_some_and(|text| text.trim() == source)
            })
            .expect("the query has a typed node");
        let added = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::AddSetOperationOperand {
                    node_id: query.id,
                    expected_kind: SqlDocumentNodeKind::Query,
                    operator: SqlDocumentSetOperator::Union,
                    quantifier: SqlDocumentSetQuantifier::All,
                }],
            ),
        )
        .expect("a typed set-operation seed can be appended");
        assert_eq!(added.document.status, SqlDocumentStatus::Parsed);
        assert_eq!(added.document.to_sql(), "(SELECT 1\n) UNION ALL SELECT 1");
        let undone = apply_sql_document_edit_transaction(&added.document.source, &added.undo)
            .expect("appending a set-operation seed returns guarded undo");
        assert_eq!(undone.document.to_sql().as_bytes(), source.as_bytes());

        let added_operation = added
            .document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SetOperation)
            .expect("the appended operation has a typed node");
        let removed_added_operand = apply_sql_document_semantic_edit_transaction(
            &added.document.source,
            &semantic_transaction(
                &added.document.source,
                vec![SqlDocumentSemanticEdit::RemoveSetOperationOperand {
                    node_id: added_operation.id,
                    expected_kind: SqlDocumentNodeKind::SetOperation,
                    keep: SqlDocumentSetOperand::Left,
                }],
            ),
        )
        .unwrap_or_else(|error| {
            panic!(
                "the appended right operand is removable: {error}; nodes={:#?}",
                added.document.nodes
            )
        });
        assert_eq!(removed_added_operand.document.to_sql(), source);

        let operation_source = "SELECT 1 UNION SELECT 2";
        for (keep, expected) in [
            (SqlDocumentSetOperand::Left, "SELECT 1"),
            (SqlDocumentSetOperand::Right, "SELECT 2"),
        ] {
            let operation_document = parse_sql_document(operation_source);
            let operation = operation_document
                .nodes
                .iter()
                .find(|node| node.kind == SqlDocumentNodeKind::SetOperation)
                .expect("the set operation has a typed node");
            let removed = apply_sql_document_semantic_edit_transaction(
                operation_source,
                &semantic_transaction(
                    operation_source,
                    vec![SqlDocumentSemanticEdit::RemoveSetOperationOperand {
                        node_id: operation.id,
                        expected_kind: SqlDocumentNodeKind::SetOperation,
                        keep,
                    }],
                ),
            )
            .expect("either operand can be retained explicitly");
            assert_eq!(removed.document.status, SqlDocumentStatus::Parsed);
            assert_eq!(removed.document.to_sql(), expected);
            let undone =
                apply_sql_document_edit_transaction(&removed.document.source, &removed.undo)
                    .expect("operand removal returns guarded undo");
            assert_eq!(
                undone.document.to_sql().as_bytes(),
                operation_source.as_bytes()
            );
        }
    }

    #[test]
    fn semantic_set_operation_operand_removal_fails_closed_when_comments_would_be_dropped() {
        let source = "SELECT 1 /* retained */ UNION SELECT 2";
        let document = parse_sql_document(source);
        let operation = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SetOperation)
            .expect("the set operation has a typed node");
        let comment_in_left_gap = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::RemoveSetOperationOperand {
                    node_id: operation.id,
                    expected_kind: SqlDocumentNodeKind::SetOperation,
                    keep: SqlDocumentSetOperand::Left,
                }],
            ),
        );
        assert_eq!(
            comment_in_left_gap,
            Err(SqlDocumentEditError::NodeRangeUnavailable)
        );

        let ambiguous_source = "SELECT 1 UNION /* keep operator comment */ SELECT 2";
        let ambiguous_document = parse_sql_document(ambiguous_source);
        let ambiguous_operation = ambiguous_document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SetOperation)
            .expect("the set operation with a comment has a typed node");
        let rejected = apply_sql_document_semantic_edit_transaction(
            ambiguous_source,
            &semantic_transaction(
                ambiguous_source,
                vec![SqlDocumentSemanticEdit::RemoveSetOperationOperand {
                    node_id: ambiguous_operation.id,
                    expected_kind: SqlDocumentNodeKind::SetOperation,
                    keep: SqlDocumentSetOperand::Right,
                }],
            ),
        );
        assert_eq!(rejected, Err(SqlDocumentEditError::NodeRangeUnavailable));
    }

    #[test]
    fn semantic_binary_operator_edit_rejects_ambiguous_token_ranges() {
        let source = "SELECT a + b";
        let mut document = parse_sql_document(source);
        let binary = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::BinaryExpression)
            .cloned()
            .expect("the addition expression has a binary node");
        let right_operand = document
            .nodes
            .iter_mut()
            .find(|node| {
                node.parent == Some(binary.id)
                    && node.kind == SqlDocumentNodeKind::IdentifierExpression
            })
            .expect("the right side has an identifier node");
        right_operand.range = Some(SqlSourceRange {
            start: source.len(),
            end: source.len(),
        });

        assert_eq!(
            binary_operator_token_range(&document, &binary),
            Err(SqlDocumentEditError::NodeRangeUnavailable)
        );
    }

    #[test]
    fn semantic_unary_sign_edit_preserves_operand_comments_and_supports_undo() {
        let source = "SELECT - /* keep */ amount";
        let document = parse_sql_document(source);
        assert_eq!(document.status, SqlDocumentStatus::Parsed);
        let unary = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::UnaryExpression
                    && matches!(
                        node.semantic,
                        Some(SqlDocumentNodeData::UnaryOperator { ref operator })
                            if operator == "-"
                    )
            })
            .expect("the arithmetic unary sign has a typed expression node");

        let changed = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::ReplaceUnarySign {
                    node_id: unary.id,
                    expected_kind: SqlDocumentNodeKind::UnaryExpression,
                    sign: SqlDocumentUnarySign::Positive,
                }],
            ),
        )
        .expect("the arithmetic sign is changed through a typed operation");
        assert_eq!(changed.document.status, SqlDocumentStatus::Parsed);
        assert_eq!(changed.document.to_sql(), "SELECT + /* keep */ amount");
        let undone = apply_sql_document_edit_transaction(&changed.document.source, &changed.undo)
            .expect("the unary sign edit returns a guarded undo transaction");
        assert_eq!(undone.document.to_sql().as_bytes(), source.as_bytes());
    }

    #[test]
    fn semantic_unary_sign_edit_constructs_boolean_not_and_supports_undo() {
        let source = "SELECT - TRUE";
        let document = parse_sql_document(source);
        let unary = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::UnaryExpression
                    && matches!(
                        node.semantic,
                        Some(SqlDocumentNodeData::UnaryOperator { ref operator })
                            if operator == "-"
                    )
            })
            .expect("the parser exposes the unary expression");

        let changed = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::ReplaceUnarySign {
                    node_id: unary.id,
                    expected_kind: SqlDocumentNodeKind::UnaryExpression,
                    sign: SqlDocumentUnarySign::Not,
                }],
            ),
        )
        .expect("the typed operation constructs boolean NOT");
        assert_eq!(changed.document.to_sql(), "SELECT NOT TRUE");
        let undone = apply_sql_document_edit_transaction(&changed.document.source, &changed.undo)
            .expect("the unary operation has a guarded inverse");
        assert_eq!(undone.document.to_sql(), source);
    }

    #[test]
    fn semantic_bitwise_and_shift_operators_are_constructible_and_undoable() {
        let source = "SELECT 5 + 1";
        for (operator, expected) in [
            (SqlDocumentBinaryOperator::BitwiseAnd, "SELECT 5 & 1"),
            (SqlDocumentBinaryOperator::BitwiseOr, "SELECT 5 | 1"),
            (SqlDocumentBinaryOperator::ShiftLeft, "SELECT 5 << 1"),
            (SqlDocumentBinaryOperator::ShiftRight, "SELECT 5 >> 1"),
        ] {
            let document = parse_sql_document(source);
            let binary = document
                .nodes
                .iter()
                .find(|node| node.kind == SqlDocumentNodeKind::BinaryExpression)
                .expect("the arithmetic expression has a typed binary node");
            let changed = apply_sql_document_semantic_edit_transaction(
                source,
                &semantic_transaction(
                    source,
                    vec![SqlDocumentSemanticEdit::ReplaceBinaryOperator {
                        node_id: binary.id,
                        expected_kind: SqlDocumentNodeKind::BinaryExpression,
                        operator,
                    }],
                ),
            )
            .expect("the runtime-admitted operator is emitted through a typed operation");
            assert_eq!(changed.document.status, SqlDocumentStatus::Parsed);
            assert_eq!(changed.document.to_sql(), expected);
            let undone =
                apply_sql_document_edit_transaction(&changed.document.source, &changed.undo)
                    .expect("the operator edit has a guarded inverse");
            assert_eq!(undone.document.to_sql(), source);
        }
    }

    #[test]
    fn typed_node_edits_preflight_aggregate_output_size() {
        let source = "SELECT a, b FROM \"G\"";
        let document = parse_sql_document(source);
        assert_eq!(document.status, SqlDocumentStatus::Parsed);

        let select_items = document
            .nodes
            .iter()
            .filter(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .collect::<Vec<_>>();
        assert_eq!(select_items.len(), 2);

        // Each replacement is within the document bound, but the transaction
        // would exceed it after both replacements are applied.
        let replacement = "x".repeat(MAX_SQL_DOCUMENT_BYTES / 2);
        let error = apply_sql_document_node_edit_transaction(
            source,
            &SqlDocumentNodeEditTransaction {
                contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                expected_source: source.to_owned(),
                edits: select_items
                    .into_iter()
                    .map(|node| SqlDocumentNodeEdit {
                        node_id: node.id,
                        expected_kind: SqlDocumentNodeKind::SelectItem,
                        replacement: replacement.clone(),
                    })
                    .collect(),
            },
        )
        .expect_err("aggregate replacement size exceeds the document bound");

        assert_eq!(error, SqlDocumentEditError::OutputTooLarge);
        assert_eq!(
            error.to_string(),
            "SQL document edit output exceeds the size limit"
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
        let source = format!("SELECT {}", "$x,".repeat(20_000));
        let new_name = "renamed_".repeat(20_000);

        assert_eq!(
            rename_sql_document_parameter(&source, "x", &new_name),
            Err(SqlDocumentEditError::OutputTooLarge)
        );
    }

    #[test]
    fn parameter_rename_rewrites_many_tokens_in_one_bounded_pass() {
        let source = format!("SELECT {}", "$x,".repeat(20_000));
        let document = rename_sql_document_parameter(&source, "x", "long_name")
            .expect("replacement remains within the source limit");

        assert_eq!(
            document.source,
            format!("SELECT {}", "$long_name,".repeat(20_000))
        );
        assert_eq!(
            document.source.len(),
            format!("SELECT {}", "$long_name,".repeat(20_000)).len()
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
            &|data| matches!(data, SqlDocumentNodeData::TableFactor { factor_kind, alias }
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
    fn cast_node_edit_is_unavailable_when_it_would_discard_internal_comments() {
        let source =
            "SELECT CAST(amount /* keep this */ AS BIGINT), amount /* keep this too */ :: TEXT";
        let document = parse_sql_document(source);
        assert_eq!(document.status, SqlDocumentStatus::Parsed);
        assert_eq!(document.to_sql(), source);

        let cast_nodes = document
            .nodes
            .iter()
            .filter(|node| node.kind == SqlDocumentNodeKind::CastExpression)
            .collect::<Vec<_>>();
        assert_eq!(cast_nodes.len(), 2);
        assert!(cast_nodes.iter().all(|node| node.range.is_none()));

        let result = apply_sql_document_node_edit_transaction(
            source,
            &SqlDocumentNodeEditTransaction {
                contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                expected_source: source.to_owned(),
                edits: vec![SqlDocumentNodeEdit {
                    node_id: cast_nodes[0].id,
                    expected_kind: SqlDocumentNodeKind::CastExpression,
                    replacement: "CAST(amount AS VARCHAR)".to_owned(),
                }],
            },
        );
        assert_eq!(result, Err(SqlDocumentEditError::NodeRangeUnavailable));
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
    fn cast_node_edit_replaces_the_complete_wrapper_and_keeps_surrounding_source() {
        let source = "SELECT /* prefix */ CAST(amount AS BIGINT) AS value /* suffix */ FROM \"G\"";
        let document = parse_sql_document(source);
        assert_eq!(document.status, SqlDocumentStatus::Parsed);
        let cast = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::CastExpression)
            .expect("CAST has a typed expression node");
        assert_eq!(
            cast.range.and_then(|range| document.source_slice(range)),
            Some("CAST(amount AS BIGINT)")
        );

        let changed = apply_sql_document_node_edit_transaction(
            source,
            &SqlDocumentNodeEditTransaction {
                contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                expected_source: source.to_owned(),
                edits: vec![SqlDocumentNodeEdit {
                    node_id: cast.id,
                    expected_kind: SqlDocumentNodeKind::CastExpression,
                    replacement: "CAST(amount AS VARCHAR)".to_owned(),
                }],
            },
        )
        .expect("the full CAST wrapper is a safe source replacement range");
        assert_eq!(
            changed.document.to_sql(),
            "SELECT /* prefix */ CAST(amount AS VARCHAR) AS value /* suffix */ FROM \"G\""
        );
        let undone =
            apply_sql_document_edit_transaction(&changed.document.source, &changed.undo).unwrap();
        assert_eq!(undone.document.to_sql().as_bytes(), source.as_bytes());
    }

    #[test]
    fn semantic_cast_target_edit_preserves_operand_and_supports_undo() {
        let source = "SELECT /* outside */ CAST(amount + 1 AS BIGINT) AS value FROM \"G\"";
        let document = parse_sql_document(source);
        assert_eq!(document.status, SqlDocumentStatus::Parsed);
        let cast = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::CastExpression)
            .expect("CAST has a typed expression node");

        let changed = apply_sql_document_semantic_edit_transaction(
            source,
            &SqlDocumentSemanticEditTransaction {
                contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                expected_source: source.to_owned(),
                edits: vec![SqlDocumentSemanticEdit::ReplaceCastTargetType {
                    node_id: cast.id,
                    expected_kind: SqlDocumentNodeKind::CastExpression,
                    target_type: SqlDocumentCastType::Double,
                }],
            },
        )
        .expect("portable CAST target is emitted by the Rust document editor");
        assert_eq!(
            changed.document.to_sql(),
            "SELECT /* outside */ CAST(amount + 1 AS DOUBLE) AS value FROM \"G\""
        );
        assert_eq!(
            changed.document.status,
            SqlDocumentStatus::Parsed,
            "the typed edit reparses through the same Rust document parser"
        );

        let undone = apply_sql_document_edit_transaction(&changed.document.source, &changed.undo)
            .expect("the semantic edit returns a guarded undo transaction");
        assert_eq!(undone.document.to_sql().as_bytes(), source.as_bytes());
    }

    #[test]
    fn semantic_parameterized_cast_edits_keep_exact_numeric_parameters_and_undo() {
        for (source, target, expected) in [
            (
                "SELECT CAST(1 AS DECIMAL)",
                SqlDocumentCastTarget {
                    kind: SqlDocumentCastType::Decimal,
                    precision: Some("40".into()),
                    scale: Some("-4".into()),
                    length: None,
                },
                "SELECT CAST(1 AS DECIMAL(40, -4))",
            ),
            (
                "SELECT CAST(1 AS VARCHAR)",
                SqlDocumentCastTarget {
                    kind: SqlDocumentCastType::Varchar,
                    precision: None,
                    scale: None,
                    length: Some("2".into()),
                },
                "SELECT CAST(1 AS VARCHAR(2))",
            ),
            (
                "SELECT CAST(1 AS TIMESTAMP)",
                SqlDocumentCastTarget {
                    kind: SqlDocumentCastType::Timestamp,
                    precision: Some("3".into()),
                    scale: None,
                    length: None,
                },
                "SELECT CAST(1 AS TIMESTAMP(3))",
            ),
            (
                "SELECT CAST(1 AS BIGINT)",
                SqlDocumentCastTarget {
                    kind: SqlDocumentCastType::Integer,
                    precision: None,
                    scale: None,
                    length: None,
                },
                "SELECT CAST(1 AS INTEGER)",
            ),
        ] {
            let document = parse_sql_document(source);
            let cast = document
                .nodes
                .iter()
                .find(|node| node.kind == SqlDocumentNodeKind::CastExpression)
                .unwrap();
            let changed = apply_sql_document_semantic_edit_transaction(
                source,
                &semantic_transaction(
                    source,
                    vec![SqlDocumentSemanticEdit::SetParameterizedCastTarget {
                        node_id: cast.id,
                        expected_kind: SqlDocumentNodeKind::CastExpression,
                        target,
                    }],
                ),
            )
            .expect("the target type and numeric parameters are typed values");
            assert_eq!(changed.document.source, expected);
            assert_eq!(changed.document.status, SqlDocumentStatus::Parsed);
            let undone =
                apply_sql_document_edit_transaction(&changed.document.source, &changed.undo)
                    .unwrap();
            assert_eq!(undone.document.source, source);
        }
    }

    #[test]
    fn semantic_cast_target_edit_preserves_every_supported_cast_form_and_undoes() {
        for (source, expected_source) in [
            (
                "SELECT TRY_CAST(amount AS BIGINT)",
                "SELECT TRY_CAST(amount AS DOUBLE)",
            ),
            (
                "SELECT SAFE_CAST(amount AS BIGINT)",
                "SELECT SAFE_CAST(amount AS DOUBLE)",
            ),
            ("SELECT amount::BIGINT", "SELECT amount::DOUBLE"),
            (
                "SELECT CONVERT(amount, BIGINT)",
                "SELECT CONVERT(amount, DOUBLE)",
            ),
            (
                "SELECT TRY_CONVERT(amount, BIGINT)",
                "SELECT TRY_CONVERT(amount, DOUBLE)",
            ),
        ] {
            let document = parse_sql_document(source);
            assert_eq!(document.status, SqlDocumentStatus::Parsed);
            let cast = document
                .nodes
                .iter()
                .find(|node| node.kind == SqlDocumentNodeKind::CastExpression)
                .expect("cast syntax is represented in the SQL document");
            let edited = apply_sql_document_semantic_edit_transaction(
                source,
                &SqlDocumentSemanticEditTransaction {
                    contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                    expected_source: source.to_owned(),
                    edits: vec![SqlDocumentSemanticEdit::ReplaceCastTargetType {
                        node_id: cast.id,
                        expected_kind: SqlDocumentNodeKind::CastExpression,
                        target_type: SqlDocumentCastType::Double,
                    }],
                },
            )
            .expect("the target type can be changed without rewriting cast syntax");
            assert_eq!(
                edited.document.to_sql(),
                expected_source,
                "cast target selection mismatch for {source}; semantic={:?}",
                cast.semantic
            );
            let undone = apply_sql_document_edit_transaction(&edited.document.source, &edited.undo)
                .expect("the type edit provides guarded undo");
            assert_eq!(undone.document.to_sql().as_bytes(), source.as_bytes());
        }
    }

    #[test]
    fn semantic_cast_target_edits_keep_nested_nodes_and_utf8_source_ranges_distinct() {
        let source = "-- 日本語\nSELECT CAST(TRY_CAST(amount AS BIGINT) AS BIGINT)";
        let document = parse_sql_document(source);
        assert_eq!(document.status, SqlDocumentStatus::Parsed);
        let cast_nodes = document
            .nodes
            .iter()
            .filter(|node| node.kind == SqlDocumentNodeKind::CastExpression)
            .collect::<Vec<_>>();
        assert_eq!(cast_nodes.len(), 2);

        for cast in cast_nodes {
            let original_cast = cast
                .range
                .and_then(|range| document.source_slice(range))
                .expect("both nested cast forms have exact source ranges");
            let expected_source = match original_cast {
                "CAST(TRY_CAST(amount AS BIGINT) AS BIGINT)" => {
                    "-- 日本語\nSELECT CAST(TRY_CAST(amount AS BIGINT) AS DOUBLE)"
                }
                "TRY_CAST(amount AS BIGINT)" => {
                    "-- 日本語\nSELECT CAST(TRY_CAST(amount AS DOUBLE) AS BIGINT)"
                }
                other => panic!("unexpected nested cast range: {other}"),
            };
            let changed = apply_sql_document_semantic_edit_transaction(
                source,
                &SqlDocumentSemanticEditTransaction {
                    contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                    expected_source: source.to_owned(),
                    edits: vec![SqlDocumentSemanticEdit::ReplaceCastTargetType {
                        node_id: cast.id,
                        expected_kind: SqlDocumentNodeKind::CastExpression,
                        target_type: SqlDocumentCastType::Double,
                    }],
                },
            )
            .expect("one nested cast target can be edited independently");
            assert_eq!(changed.document.to_sql(), expected_source);
            let undone =
                apply_sql_document_edit_transaction(&changed.document.source, &changed.undo)
                    .expect("the nested cast edit is undoable");
            assert_eq!(undone.document.to_sql().as_bytes(), source.as_bytes());
        }
    }

    #[test]
    fn semantic_cast_target_edit_fails_closed_when_comments_hide_the_wrapper() {
        for source in [
            "SELECT CAST(amount /* keep */ AS BIGINT)",
            "SELECT amount /* keep this too */ :: TEXT",
            "SELECT CAST(amount AS DECIMAL /* keep */ (10, 2))",
        ] {
            let document = parse_sql_document(source);
            assert_eq!(document.status, SqlDocumentStatus::Parsed);
            let cast = document
                .nodes
                .iter()
                .find(|node| node.kind == SqlDocumentNodeKind::CastExpression)
                .expect("cast syntax is represented in the SQL document");
            assert_eq!(cast.range, None);
            let edited = apply_sql_document_semantic_edit_transaction(
                source,
                &SqlDocumentSemanticEditTransaction {
                    contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                    expected_source: source.to_owned(),
                    edits: vec![SqlDocumentSemanticEdit::ReplaceCastTargetType {
                        node_id: cast.id,
                        expected_kind: SqlDocumentNodeKind::CastExpression,
                        target_type: SqlDocumentCastType::Varchar,
                    }],
                },
            );
            assert_eq!(edited, Err(SqlDocumentEditError::NodeRangeUnavailable));
        }

        let ambiguous_source = "SELECT CONVERT(BIGINT, BIGINT)";
        let document = parse_sql_document(ambiguous_source);
        let cast = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::CastExpression)
            .expect("CONVERT has a typed cast node");
        let edited = apply_sql_document_semantic_edit_transaction(
            ambiguous_source,
            &SqlDocumentSemanticEditTransaction {
                contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                expected_source: ambiguous_source.to_owned(),
                edits: vec![SqlDocumentSemanticEdit::ReplaceCastTargetType {
                    node_id: cast.id,
                    expected_kind: SqlDocumentNodeKind::CastExpression,
                    target_type: SqlDocumentCastType::Double,
                }],
            },
        );
        assert_eq!(
            edited,
            Err(SqlDocumentEditError::InvalidSemanticReplacement)
        );
    }

    #[test]
    fn semantic_cte_rename_updates_declaration_and_references_atomically() {
        let source = "-- keep heading\nWITH c AS (SELECT 1 AS n), d AS (SELECT n FROM c) SELECT n FROM c -- keep tail";
        let document = parse_sql_document(source);
        assert_eq!(document.status, SqlDocumentStatus::Parsed);
        let cte = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::CommonTableExpression
                    && matches!(node.semantic, Some(SqlDocumentNodeData::CommonTableExpression { ref name, .. }) if name.value == "c")
            })
            .expect("the named CTE has a typed document node");

        let changed = apply_sql_document_semantic_edit_transaction(
            source,
            &SqlDocumentSemanticEditTransaction {
                contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                expected_source: source.to_owned(),
                edits: vec![SqlDocumentSemanticEdit::RenameCommonTableExpression {
                    node_id: cte.id,
                    expected_kind: SqlDocumentNodeKind::CommonTableExpression,
                    name: SqlDocumentIdentifier {
                        value: "rows".to_owned(),
                        quote_style: Some('"'),
                    },
                }],
            },
        )
        .expect("the CTE declaration and its references are one atomic edit");
        assert_eq!(
            changed.document.to_sql(),
            "-- keep heading\nWITH \"rows\" AS (SELECT 1 AS n), d AS (SELECT n FROM \"rows\") SELECT n FROM \"rows\" -- keep tail"
        );
        let undone = apply_sql_document_edit_transaction(&changed.document.source, &changed.undo)
            .expect("the CTE rename returns a guarded undo transaction");
        assert_eq!(undone.document.to_sql().as_bytes(), source.as_bytes());
    }

    #[test]
    fn semantic_cte_rename_respects_nested_shadowing_and_other_query_scopes() {
        let source = "WITH c AS (SELECT 1 AS n), d AS (WITH c AS (SELECT 2 AS n) SELECT n FROM c), e AS (SELECT n FROM c) SELECT n FROM c";
        let document = parse_sql_document(source);
        assert_eq!(document.status, SqlDocumentStatus::Parsed);
        let target = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::CommonTableExpression
                    && matches!(node.semantic, Some(SqlDocumentNodeData::CommonTableExpression { ref name, .. }) if name.value == "c")
                    && node.parent.is_some_and(|with_id| {
                        document.nodes.get(with_id as usize).is_some_and(|with| {
                            with.kind == SqlDocumentNodeKind::WithClause
                                && with.parent.is_some_and(|query_id| {
                                    document.nodes.get(query_id as usize).is_some_and(|query| {
                                        query.range.and_then(|range| document.source_slice(range))
                                            .is_some_and(|sql| sql.contains("e AS"))
                                    })
                                })
                        })
                    })
            })
            .expect("outer CTE declaration is present");

        let changed = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::RenameCommonTableExpression {
                    node_id: target.id,
                    expected_kind: SqlDocumentNodeKind::CommonTableExpression,
                    name: SqlDocumentIdentifier {
                        value: "rows".to_owned(),
                        quote_style: None,
                    },
                }],
            ),
        )
        .expect("only references bound to the selected CTE are renamed");
        assert_eq!(
            changed.document.to_sql(),
            "WITH rows AS (SELECT 1 AS n), d AS (WITH c AS (SELECT 2 AS n) SELECT n FROM c), e AS (SELECT n FROM rows) SELECT n FROM rows"
        );
        assert_eq!(changed.document.status, SqlDocumentStatus::Parsed);
    }

    #[test]
    fn semantic_cte_rename_rejects_ambiguous_name_collisions() {
        let source = "WITH c AS (SELECT 1), rows AS (SELECT 2) SELECT * FROM c";
        let document = parse_sql_document(source);
        let cte = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::CommonTableExpression
                    && matches!(node.semantic, Some(SqlDocumentNodeData::CommonTableExpression { ref name, .. }) if name.value == "c")
            })
            .expect("the target CTE is present");
        let edited = apply_sql_document_semantic_edit_transaction(
            source,
            &SqlDocumentSemanticEditTransaction {
                contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                expected_source: source.to_owned(),
                edits: vec![SqlDocumentSemanticEdit::RenameCommonTableExpression {
                    node_id: cte.id,
                    expected_kind: SqlDocumentNodeKind::CommonTableExpression,
                    name: SqlDocumentIdentifier {
                        value: "rows".to_owned(),
                        quote_style: None,
                    },
                }],
            },
        );
        assert_eq!(
            edited,
            Err(SqlDocumentEditError::InvalidSemanticReplacement)
        );
    }

    #[test]
    fn semantic_cte_add_creates_or_extends_with_and_supports_undo() {
        for (source, expected) in [
            ("SELECT 1", "WITH rows AS (SELECT 1) SELECT 1"),
            (
                "WITH existing AS (SELECT 2) SELECT 1",
                "WITH existing AS (SELECT 2), rows AS (SELECT 1) SELECT 1",
            ),
        ] {
            let document = parse_sql_document(source);
            assert_eq!(document.status, SqlDocumentStatus::Parsed, "{source}");
            let query = document
                .nodes
                .iter()
                .find(|node| {
                    node.kind == SqlDocumentNodeKind::Query
                        && node
                            .range
                            .and_then(|range| document.source_slice(range))
                            .is_some_and(|text| text.trim() == source)
                })
                .expect("the target query has a typed document node");
            let changed = apply_sql_document_semantic_edit_transaction(
                source,
                &semantic_transaction(
                    source,
                    vec![SqlDocumentSemanticEdit::AddCommonTableExpression {
                        node_id: query.id,
                        expected_kind: SqlDocumentNodeKind::Query,
                        name: SqlDocumentIdentifier {
                            value: "rows".to_owned(),
                            quote_style: None,
                        },
                    }],
                ),
            )
            .unwrap_or_else(|error| panic!("{source}: {error}"));
            assert_eq!(changed.document.status, SqlDocumentStatus::Parsed);
            assert_eq!(changed.document.to_sql(), expected);
            let undone =
                apply_sql_document_edit_transaction(&changed.document.source, &changed.undo)
                    .expect("adding a CTE returns a guarded undo transaction");
            assert_eq!(undone.document.to_sql().as_bytes(), source.as_bytes());
        }
    }

    #[test]
    fn semantic_cte_add_and_remove_reject_ambiguous_edits() {
        let source = "WITH c AS (SELECT 1) SELECT 2";
        let document = parse_sql_document(source);
        let query = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Query)
            .expect("the target query has a typed document node");
        let duplicate = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::AddCommonTableExpression {
                    node_id: query.id,
                    expected_kind: SqlDocumentNodeKind::Query,
                    name: SqlDocumentIdentifier {
                        value: "c".to_owned(),
                        quote_style: None,
                    },
                }],
            ),
        );
        assert_eq!(
            duplicate,
            Err(SqlDocumentEditError::InvalidSemanticReplacement)
        );

        let referenced_source = "WITH c AS (SELECT 1) SELECT * FROM c";
        let referenced_document = parse_sql_document(referenced_source);
        let referenced_cte = referenced_document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::CommonTableExpression)
            .expect("the referenced CTE has a typed declaration");
        let referenced_remove = apply_sql_document_semantic_edit_transaction(
            referenced_source,
            &semantic_transaction(
                referenced_source,
                vec![SqlDocumentSemanticEdit::RemoveCommonTableExpression {
                    node_id: referenced_cte.id,
                    expected_kind: SqlDocumentNodeKind::CommonTableExpression,
                }],
            ),
        );
        assert_eq!(
            referenced_remove,
            Err(SqlDocumentEditError::InvalidSemanticReplacement)
        );

        let comment_source = "WITH c AS (SELECT 1 /* keep */) SELECT 2";
        let comment_document = parse_sql_document(comment_source);
        let comment_cte = comment_document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::CommonTableExpression)
            .expect("the CTE containing a comment has a typed declaration");
        let comment_remove = apply_sql_document_semantic_edit_transaction(
            comment_source,
            &semantic_transaction(
                comment_source,
                vec![SqlDocumentSemanticEdit::RemoveCommonTableExpression {
                    node_id: comment_cte.id,
                    expected_kind: SqlDocumentNodeKind::CommonTableExpression,
                }],
            ),
        );
        assert_eq!(
            comment_remove,
            Err(SqlDocumentEditError::NodeRangeUnavailable)
        );
    }

    #[test]
    fn semantic_cte_add_and_remove_respect_nested_with_scopes() {
        let add_source = "WITH c AS (SELECT 1) SELECT * FROM (SELECT 2) AS nested";
        let add_document = parse_sql_document(add_source);
        let nested_query = add_document
            .nodes
            .iter()
            .filter(|node| node.kind == SqlDocumentNodeKind::Query)
            .filter(|node| {
                node.range
                    .and_then(|range| add_document.source_slice(range))
                    .is_some_and(|text| text.contains("SELECT 2"))
            })
            .min_by_key(|node| {
                node.range
                    .map(|range| range.end.saturating_sub(range.start))
                    .unwrap_or(usize::MAX)
            })
            .expect("the nested query has its own typed scope");
        let added = apply_sql_document_semantic_edit_transaction(
            add_source,
            &semantic_transaction(
                add_source,
                vec![SqlDocumentSemanticEdit::AddCommonTableExpression {
                    node_id: nested_query.id,
                    expected_kind: SqlDocumentNodeKind::Query,
                    name: SqlDocumentIdentifier {
                        value: "c".to_owned(),
                        quote_style: None,
                    },
                }],
            ),
        )
        .expect("same names are valid in separate WITH scopes");
        assert_eq!(
            added.document.to_sql(),
            "WITH c AS (SELECT 1) SELECT * FROM (WITH c AS (SELECT 1) SELECT 2) AS nested"
        );

        let remove_source =
            "WITH c AS (SELECT 1), d AS (WITH c AS (SELECT 2) SELECT * FROM c) SELECT 3";
        let remove_document = parse_sql_document(remove_source);
        let outer_cte = remove_document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::CommonTableExpression
                    && matches!(node.semantic, Some(SqlDocumentNodeData::CommonTableExpression { ref name, .. }) if name.value == "c")
                    && node.parent.is_some_and(|with_id| {
                        remove_document.nodes.get(with_id as usize).is_some_and(|with| {
                            with.kind == SqlDocumentNodeKind::WithClause
                                && with.parent.is_some_and(|query_id| {
                                    remove_document.nodes.get(query_id as usize).is_some_and(|query| {
                                        query.range.and_then(|range| remove_document.source_slice(range))
                                            .is_some_and(|sql| sql.contains("d AS"))
                                    })
                                })
                        })
                    })
            })
            .expect("the outer CTE is present");
        let removed = apply_sql_document_semantic_edit_transaction(
            remove_source,
            &semantic_transaction(
                remove_source,
                vec![SqlDocumentSemanticEdit::RemoveCommonTableExpression {
                    node_id: outer_cte.id,
                    expected_kind: SqlDocumentNodeKind::CommonTableExpression,
                }],
            ),
        )
        .expect("an inner shadow reference does not keep the outer CTE alive");
        assert_eq!(
            removed.document.to_sql(),
            "WITH d AS (WITH c AS (SELECT 2) SELECT * FROM c) SELECT 3"
        );
        assert_eq!(removed.document.status, SqlDocumentStatus::Parsed);
    }

    #[test]
    fn semantic_cte_body_replace_and_order_are_typed_and_undoable() {
        let source = "WITH c AS (SELECT 1 AS n), d AS (SELECT 2 AS n) SELECT n FROM c";
        let document = parse_sql_document(source);
        let cte_c = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::CommonTableExpression
                    && matches!(node.semantic, Some(SqlDocumentNodeData::CommonTableExpression { ref name, .. }) if name.value == "c")
            })
            .expect("the target CTE is represented");
        let query_d = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::Query
                    && node.parent.is_some_and(|parent| {
                        document.nodes.get(parent as usize).is_some_and(|cte| {
                            cte.kind == SqlDocumentNodeKind::CommonTableExpression
                                && matches!(cte.semantic, Some(SqlDocumentNodeData::CommonTableExpression { ref name, .. }) if name.value == "d")
                        })
                    })
            })
            .expect("the source CTE body is a parser-backed query");
        let replaced = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::ReplaceCommonTableExpressionQuery {
                    node_id: cte_c.id,
                    expected_kind: SqlDocumentNodeKind::CommonTableExpression,
                    query_node_id: query_d.id,
                }],
            ),
        )
        .expect("a CTE query body can be replaced from a typed query node");
        assert_eq!(replaced.document.status, SqlDocumentStatus::Parsed);
        assert_eq!(
            replaced.document.to_sql(),
            "WITH c AS (SELECT 2 AS n), d AS (SELECT 2 AS n) SELECT n FROM c"
        );
        let undone = apply_sql_document_edit_transaction(&replaced.document.source, &replaced.undo)
            .expect("the CTE body replacement has an exact inverse");
        assert_eq!(undone.document.to_sql().as_bytes(), source.as_bytes());

        let cte_d = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::CommonTableExpression
                    && matches!(node.semantic, Some(SqlDocumentNodeData::CommonTableExpression { ref name, .. }) if name.value == "d")
            })
            .expect("the second CTE is represented");
        let moved = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::MoveCommonTableExpression {
                    node_id: cte_d.id,
                    expected_kind: SqlDocumentNodeKind::CommonTableExpression,
                    to_index: 0,
                }],
            ),
        )
        .expect("a CTE can move within the same WITH list");
        assert_eq!(moved.document.status, SqlDocumentStatus::Parsed);
        assert_eq!(
            moved.document.to_sql(),
            "WITH d AS (SELECT 2 AS n), c AS (SELECT 1 AS n) SELECT n FROM c"
        );
        let undone = apply_sql_document_edit_transaction(&moved.document.source, &moved.undo)
            .expect("the CTE reorder has a guarded inverse");
        assert_eq!(undone.document.to_sql().as_bytes(), source.as_bytes());

        let comment_source = "WITH c AS (SELECT 1), /* keep */ d AS (SELECT 2) SELECT 1";
        let comment_document = parse_sql_document(comment_source);
        let comment_cte = comment_document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::CommonTableExpression
                    && matches!(node.semantic, Some(SqlDocumentNodeData::CommonTableExpression { ref name, .. }) if name.value == "d")
            })
            .expect("the CTE next to the comment is represented");
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                comment_source,
                &semantic_transaction(
                    comment_source,
                    vec![SqlDocumentSemanticEdit::MoveCommonTableExpression {
                        node_id: comment_cte.id,
                        expected_kind: SqlDocumentNodeKind::CommonTableExpression,
                        to_index: 0,
                    }],
                ),
            ),
            Err(SqlDocumentEditError::NodeRangeUnavailable),
            "CTE order changes fail closed when trivia attachment would be ambiguous"
        );
    }

    #[test]
    fn semantic_cte_remove_preserves_other_ctes_and_supports_undo() {
        for (selected_name, expected) in [
            ("c", "WITH d AS (SELECT 2) SELECT 3"),
            ("d", "WITH c AS (SELECT 1) SELECT 3"),
        ] {
            let source = "WITH c AS (SELECT 1), d AS (SELECT 2) SELECT 3";
            let document = parse_sql_document(source);
            let cte = document
                .nodes
                .iter()
                .find(|node| {
                    node.kind == SqlDocumentNodeKind::CommonTableExpression
                        && matches!(
                            node.semantic,
                            Some(SqlDocumentNodeData::CommonTableExpression { ref name, .. })
                                if name.value == selected_name
                        )
                })
                .expect("the requested CTE has a typed declaration");
            let removed = apply_sql_document_semantic_edit_transaction(
                source,
                &semantic_transaction(
                    source,
                    vec![SqlDocumentSemanticEdit::RemoveCommonTableExpression {
                        node_id: cte.id,
                        expected_kind: SqlDocumentNodeKind::CommonTableExpression,
                    }],
                ),
            )
            .unwrap_or_else(|error| panic!("remove {selected_name}: {error}"));
            assert_eq!(removed.document.status, SqlDocumentStatus::Parsed);
            assert_eq!(removed.document.to_sql(), expected);
            let undone =
                apply_sql_document_edit_transaction(&removed.document.source, &removed.undo)
                    .expect("removing a CTE returns a guarded undo transaction");
            assert_eq!(undone.document.to_sql().as_bytes(), source.as_bytes());
        }
    }

    #[test]
    fn semantic_predicate_negation_preserves_comments_and_supports_undo() {
        for (source, expected_operator, negated, expected) in [
            (
                "SELECT x FROM \"G\" WHERE x IN (1, 2)",
                "in",
                true,
                "SELECT x FROM \"G\" WHERE x NOT IN (1, 2)",
            ),
            (
                "SELECT x FROM \"G\" WHERE x NOT BETWEEN 0 /* lower */ AND 10",
                "between",
                false,
                "SELECT x FROM \"G\" WHERE x  BETWEEN 0 /* lower */ AND 10",
            ),
            (
                "SELECT x FROM \"G\" WHERE x IS /* predicate */ NULL",
                "is_null",
                true,
                "SELECT x FROM \"G\" WHERE x IS NOT /* predicate */ NULL",
            ),
            (
                "SELECT x FROM \"G\" WHERE x IS NOT DISTINCT FROM y",
                "is_distinct_from",
                false,
                "SELECT x FROM \"G\" WHERE x IS  DISTINCT FROM y",
            ),
        ] {
            let document = parse_sql_document(source);
            assert_eq!(document.status, SqlDocumentStatus::Parsed, "{source}");
            let predicate = document
                .nodes
                .iter()
                .find(|node| {
                    node.kind == SqlDocumentNodeKind::PredicateExpression
                        && matches!(
                            node.semantic,
                            Some(SqlDocumentNodeData::Predicate {
                                operator: ref predicate_operator,
                                negated: current_negated,
                            }) if predicate_operator == expected_operator && current_negated != negated
                        )
                })
                .expect("the requested predicate is represented in the SQL document");
            let changed = apply_sql_document_semantic_edit_transaction(
                source,
                &SqlDocumentSemanticEditTransaction {
                    contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                    expected_source: source.to_owned(),
                    edits: vec![SqlDocumentSemanticEdit::ReplacePredicateNegation {
                        node_id: predicate.id,
                        expected_kind: SqlDocumentNodeKind::PredicateExpression,
                        negated,
                    }],
                },
            )
            .unwrap_or_else(|error| panic!("{source}: {error}"));
            assert_eq!(changed.document.status, SqlDocumentStatus::Parsed);
            assert_eq!(changed.document.to_sql(), expected);
            let undone =
                apply_sql_document_edit_transaction(&changed.document.source, &changed.undo)
                    .expect("predicate negation returns a guarded undo transaction");
            assert_eq!(undone.document.to_sql().as_bytes(), source.as_bytes());
        }
    }

    #[test]
    fn semantic_case_structure_edits_add_remove_branches_and_else_losslessly() {
        let source =
            "SELECT CASE WHEN a > 0 THEN 1 WHEN a < 0 THEN 2 /* keep */ ELSE 3 END AS value";
        let document = parse_sql_document(source);
        let case_id = semantic_node_id(
            &document,
            SqlDocumentNodeKind::CaseExpression,
            "CASE WHEN a > 0 THEN 1 WHEN a < 0 THEN 2 /* keep */ ELSE 3 END",
        );
        let added = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::AddCaseBranch {
                    node_id: case_id,
                    expected_kind: SqlDocumentNodeKind::CaseExpression,
                }],
            ),
        )
        .expect("a typed branch can be added");
        assert_eq!(
            added.document.source,
            "SELECT CASE WHEN a > 0 THEN 1 WHEN a < 0 THEN 2 WHEN TRUE THEN NULL /* keep */ ELSE 3 END AS value"
        );
        let undone = apply_sql_document_edit_transaction(&added.document.source, &added.undo)
            .expect("the generated transaction has an exact inverse");
        assert_eq!(undone.document.source, source);

        let added_case_id = semantic_node_id(
            &added.document,
            SqlDocumentNodeKind::CaseExpression,
            "CASE WHEN a > 0 THEN 1 WHEN a < 0 THEN 2 WHEN TRUE THEN NULL /* keep */ ELSE 3 END",
        );
        let removed = apply_sql_document_semantic_edit_transaction(
            &added.document.source,
            &semantic_transaction(
                &added.document.source,
                vec![SqlDocumentSemanticEdit::RemoveCaseBranch {
                    node_id: added_case_id,
                    expected_kind: SqlDocumentNodeKind::CaseExpression,
                }],
            ),
        )
        .expect("the final branch can be removed");
        assert_eq!(removed.document.source, source);

        let simple_source = "SELECT CASE a WHEN 1 THEN 'one' END";
        let simple_document = parse_sql_document(simple_source);
        let simple_case_id = semantic_node_id(
            &simple_document,
            SqlDocumentNodeKind::CaseExpression,
            "CASE a WHEN 1 THEN 'one' END",
        );
        let simple_added = apply_sql_document_semantic_edit_transaction(
            simple_source,
            &semantic_transaction(
                simple_source,
                vec![SqlDocumentSemanticEdit::AddCaseBranch {
                    node_id: simple_case_id,
                    expected_kind: SqlDocumentNodeKind::CaseExpression,
                }],
            ),
        )
        .expect("a simple CASE receives a typed NULL match value");
        assert_eq!(
            simple_added.document.source,
            "SELECT CASE a WHEN 1 THEN 'one' WHEN NULL THEN NULL END"
        );
    }

    #[test]
    fn semantic_case_operand_can_be_added_replaced_and_cleared_losslessly() {
        let searched = "SELECT amount, other, CASE WHEN amount = 1 THEN 2 ELSE 3 END";
        let document = parse_sql_document(searched);
        let case_node = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::CaseExpression)
            .unwrap();
        let operand = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::IdentifierExpression
                    && node.range.and_then(|range| document.source_slice(range)) == Some("amount")
            })
            .unwrap();
        let simple = apply_sql_document_semantic_edit_transaction(
            searched,
            &semantic_transaction(
                searched,
                vec![SqlDocumentSemanticEdit::SetCaseOperand {
                    node_id: case_node.id,
                    expected_kind: SqlDocumentNodeKind::CaseExpression,
                    expression_node_id: Some(operand.id),
                    expression_expected_kind: Some(operand.kind),
                }],
            ),
        )
        .unwrap();
        assert_eq!(
            simple.document.source,
            "SELECT amount, other, CASE amount WHEN amount = 1 THEN 2 ELSE 3 END"
        );
        assert_eq!(simple.document.status, SqlDocumentStatus::Parsed);

        let simple_case = simple
            .document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::CaseExpression)
            .unwrap();
        let selected_operand = simple
            .document
            .nodes
            .iter()
            .find(|node| {
                node.parent == Some(simple_case.id)
                    && node.kind == SqlDocumentNodeKind::IdentifierExpression
                    && node
                        .range
                        .and_then(|range| simple.document.source_slice(range))
                        == Some("amount")
            })
            .unwrap();
        let other = simple
            .document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::IdentifierExpression
                    && node
                        .range
                        .and_then(|range| simple.document.source_slice(range))
                        == Some("other")
                    && node.id != selected_operand.id
            })
            .unwrap();
        let edited = apply_sql_document_semantic_edit_transaction(
            &simple.document.source,
            &semantic_transaction(
                &simple.document.source,
                vec![SqlDocumentSemanticEdit::SetCaseOperand {
                    node_id: simple_case.id,
                    expected_kind: SqlDocumentNodeKind::CaseExpression,
                    expression_node_id: Some(other.id),
                    expression_expected_kind: Some(other.kind),
                }],
            ),
        )
        .unwrap();
        assert_eq!(
            edited.document.source,
            "SELECT amount, other, CASE other WHEN amount = 1 THEN 2 ELSE 3 END"
        );

        let cleared = apply_sql_document_semantic_edit_transaction(
            &edited.document.source,
            &semantic_transaction(
                &edited.document.source,
                vec![SqlDocumentSemanticEdit::SetCaseOperand {
                    node_id: simple_case.id,
                    expected_kind: SqlDocumentNodeKind::CaseExpression,
                    expression_node_id: None,
                    expression_expected_kind: None,
                }],
            ),
        )
        .unwrap();
        assert_eq!(
            cleared.document.source,
            "SELECT amount, other, CASE  WHEN amount = 1 THEN 2 ELSE 3 END"
        );
        assert_eq!(cleared.document.status, SqlDocumentStatus::Parsed);
        assert_eq!(
            apply_sql_document_edit_transaction(&cleared.document.source, &cleared.undo)
                .unwrap()
                .document
                .source,
            edited.document.source
        );
    }

    #[test]
    fn semantic_case_else_edits_are_typed_and_reversible() {
        let source = "SELECT CASE WHEN a > 0 THEN 1 END";
        let document = parse_sql_document(source);
        let case_id = semantic_node_id(
            &document,
            SqlDocumentNodeKind::CaseExpression,
            "CASE WHEN a > 0 THEN 1 END",
        );
        let with_else = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::SetCaseElse {
                    node_id: case_id,
                    expected_kind: SqlDocumentNodeKind::CaseExpression,
                    has_else: true,
                }],
            ),
        )
        .expect("an ELSE branch can be added");
        assert_eq!(
            with_else.document.source,
            "SELECT CASE WHEN a > 0 THEN 1 ELSE NULL END"
        );

        let with_else_id = semantic_node_id(
            &with_else.document,
            SqlDocumentNodeKind::CaseExpression,
            "CASE WHEN a > 0 THEN 1 ELSE NULL END",
        );
        let without_else = apply_sql_document_semantic_edit_transaction(
            &with_else.document.source,
            &semantic_transaction(
                &with_else.document.source,
                vec![SqlDocumentSemanticEdit::SetCaseElse {
                    node_id: with_else_id,
                    expected_kind: SqlDocumentNodeKind::CaseExpression,
                    has_else: false,
                }],
            ),
        )
        .expect("an ELSE branch can be removed");
        assert_eq!(without_else.document.source, source);
    }

    #[test]
    fn semantic_function_argument_edits_copy_typed_nodes_and_support_undo() {
        let source = "SELECT COUNT(field_100), label FROM \"G\"";
        let document = parse_sql_document(source);
        let function_id = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::FunctionExpression
                    && matches!(&node.semantic, Some(SqlDocumentNodeData::Function {
                        name,
                        ..
                    }) if name.last().is_some_and(|part| part.value.eq_ignore_ascii_case("COUNT")))
            })
            .expect("the SQL tree exposes COUNT")
            .id;
        let label_start = source.rfind("label").expect("projection label is present");
        let label = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::IdentifierExpression
                    && node.range.is_some_and(|range| range.start == label_start)
            })
            .expect("the projection label is a typed expression node");
        let replaced = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::SetFunctionArgument {
                    node_id: function_id,
                    expected_kind: SqlDocumentNodeKind::FunctionExpression,
                    argument_index: 0,
                    expression_node_id: label.id,
                    expression_expected_kind: SqlDocumentNodeKind::IdentifierExpression,
                }],
            ),
        )
        .expect("a function argument can copy a typed identifier expression");
        assert_eq!(
            replaced.document.source,
            "SELECT COUNT(label), label FROM \"G\""
        );

        let replaced_function_id = replaced
            .document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::FunctionExpression
                    && matches!(&node.semantic, Some(SqlDocumentNodeData::Function {
                        name,
                        ..
                    }) if name.last().is_some_and(|part| part.value.eq_ignore_ascii_case("COUNT")))
            })
            .expect("the edited SQL tree exposes COUNT")
            .id;
        let current_argument = replaced
            .document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::IdentifierExpression
                    && node.parent == Some(replaced_function_id)
                    && node
                        .range
                        .and_then(|range| replaced.document.source_slice(range))
                        == Some("label")
            })
            .expect("the first typed function argument remains selectable");
        let appended = apply_sql_document_semantic_edit_transaction(
            &replaced.document.source,
            &semantic_transaction(
                &replaced.document.source,
                vec![SqlDocumentSemanticEdit::SetFunctionArgument {
                    node_id: replaced_function_id,
                    expected_kind: SqlDocumentNodeKind::FunctionExpression,
                    argument_index: 1,
                    expression_node_id: current_argument.id,
                    expression_expected_kind: SqlDocumentNodeKind::IdentifierExpression,
                }],
            ),
        )
        .expect("COUNT accepts a copied expression at the end of its argument list");
        assert_eq!(
            appended.document.source,
            "SELECT COUNT(label, label), label FROM \"G\""
        );
        let appended_function_id = appended
            .document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::FunctionExpression
                    && matches!(&node.semantic, Some(SqlDocumentNodeData::Function {
                        name,
                        ..
                    }) if name.last().is_some_and(|part| part.value.eq_ignore_ascii_case("COUNT")))
            })
            .expect("the appended SQL tree exposes COUNT")
            .id;
        let removed = apply_sql_document_semantic_edit_transaction(
            &appended.document.source,
            &semantic_transaction(
                &appended.document.source,
                vec![SqlDocumentSemanticEdit::RemoveLastFunctionArgument {
                    node_id: appended_function_id,
                    expected_kind: SqlDocumentNodeKind::FunctionExpression,
                }],
            ),
        )
        .expect("the final COUNT argument can be removed");
        assert_eq!(removed.document.source, replaced.document.source);
        let restored =
            apply_sql_document_edit_transaction(&appended.document.source, &appended.undo)
                .expect("the generated argument edit has an exact inverse");
        assert_eq!(restored.document.source, replaced.document.source);
    }

    #[test]
    fn semantic_function_arguments_follow_registry_arity_and_preserve_comments() {
        let source = "SELECT COUNT(field_100 /* keep */), label FROM \"G\"";
        let document = parse_sql_document(source);
        let function = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::FunctionExpression
                    && matches!(
                        node.semantic,
                        Some(SqlDocumentNodeData::Function {
                            can_add_argument: true,
                            can_remove_argument: true,
                            ..
                        })
                    )
            })
            .expect("COUNT exposes registry-approved argument edits");
        let label_start = source.rfind("label").expect("projection label is present");
        let label = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::IdentifierExpression
                    && node.range.is_some_and(|range| range.start == label_start)
            })
            .expect("the projection label is a typed expression node");
        let appended = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::SetFunctionArgument {
                    node_id: function.id,
                    expected_kind: SqlDocumentNodeKind::FunctionExpression,
                    argument_index: 1,
                    expression_node_id: label.id,
                    expression_expected_kind: SqlDocumentNodeKind::IdentifierExpression,
                }],
            ),
        )
        .expect("a copied argument is inserted before trailing comments");
        assert_eq!(
            appended.document.source,
            "SELECT COUNT(field_100, label /* keep */), label FROM \"G\""
        );

        let wildcard_source = "SELECT COUNT(*), field_100 FROM \"G\"";
        let wildcard_document = parse_sql_document(wildcard_source);
        let wildcard_function = wildcard_document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::FunctionExpression)
            .expect("COUNT(*) is a parsed function expression");
        assert!(matches!(
            wildcard_function.semantic,
            Some(SqlDocumentNodeData::Function {
                can_replace_argument: true,
                can_add_argument: false,
                can_remove_argument: true,
                ..
            })
        ));
        let wildcard_label_start = wildcard_source
            .rfind("field_100")
            .expect("the output column is present");
        let wildcard_label = wildcard_document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::IdentifierExpression
                    && node
                        .range
                        .is_some_and(|range| range.start == wildcard_label_start)
            })
            .expect("the output column can replace the wildcard argument");
        let replaced_wildcard = apply_sql_document_semantic_edit_transaction(
            wildcard_source,
            &semantic_transaction(
                wildcard_source,
                vec![SqlDocumentSemanticEdit::SetFunctionArgument {
                    node_id: wildcard_function.id,
                    expected_kind: SqlDocumentNodeKind::FunctionExpression,
                    argument_index: 0,
                    expression_node_id: wildcard_label.id,
                    expression_expected_kind: SqlDocumentNodeKind::IdentifierExpression,
                }],
            ),
        )
        .expect("the wildcard can be replaced by a typed expression");
        assert_eq!(
            replaced_wildcard.document.source,
            "SELECT COUNT(field_100), field_100 FROM \"G\""
        );

        let sum_source = "SELECT SUM(field_100) FROM \"G\"";
        let sum_document = parse_sql_document(sum_source);
        let sum = sum_document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::FunctionExpression)
            .expect("SUM is a parsed function expression");
        assert!(matches!(
            sum.semantic,
            Some(SqlDocumentNodeData::Function {
                can_replace_argument: true,
                can_add_argument: false,
                can_remove_argument: false,
                ..
            })
        ));
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                sum_source,
                &semantic_transaction(
                    sum_source,
                    vec![SqlDocumentSemanticEdit::RemoveLastFunctionArgument {
                        node_id: sum.id,
                        expected_kind: SqlDocumentNodeKind::FunctionExpression,
                    }],
                ),
            ),
            Err(SqlDocumentEditError::InvalidSemanticReplacement)
        );
    }

    #[test]
    fn semantic_projection_expression_edits_copy_typed_nodes_and_support_undo() {
        let source = "SELECT field_100, CASE WHEN field_100 = 'alpha' THEN field_100 ELSE 'other' END AS label FROM \"G\"";
        let document = parse_sql_document(source);
        let mut items = document
            .nodes
            .iter()
            .filter(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .collect::<Vec<_>>();
        items.sort_by_key(|node| node.range.map(|range| range.start));
        let target = items[0];
        let source_item = items[1];
        let expression = document
            .nodes
            .iter()
            .find(|node| {
                node.parent == Some(source_item.id)
                    && node.kind == SqlDocumentNodeKind::CaseExpression
            })
            .expect("the second projection exposes its typed CASE expression");
        let edited = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::SetProjectionExpression {
                    node_id: target.id,
                    expected_kind: SqlDocumentNodeKind::SelectItem,
                    expression_node_id: expression.id,
                    expression_expected_kind: SqlDocumentNodeKind::CaseExpression,
                }],
            ),
        )
        .expect("a projection expression can copy a typed CASE node");
        assert_eq!(
            edited.document.source,
            "SELECT CASE WHEN field_100 = 'alpha' THEN field_100 ELSE 'other' END, CASE WHEN field_100 = 'alpha' THEN field_100 ELSE 'other' END AS label FROM \"G\""
        );
        let restored = apply_sql_document_edit_transaction(&edited.document.source, &edited.undo)
            .expect("the projection edit has an exact inverse");
        assert_eq!(restored.document.source, source);

        let list = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SelectList)
            .expect("the parser exposes the SELECT list");
        let appended = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::AddProjectionExpression {
                    node_id: list.id,
                    expected_kind: SqlDocumentNodeKind::SelectList,
                    expression_node_id: expression.id,
                    expression_expected_kind: SqlDocumentNodeKind::CaseExpression,
                }],
            ),
        )
        .expect("a typed expression can be appended as a projection slot");
        assert_eq!(
            appended.document.source,
            "SELECT field_100, CASE WHEN field_100 = 'alpha' THEN field_100 ELSE 'other' END AS label, CASE WHEN field_100 = 'alpha' THEN field_100 ELSE 'other' END FROM \"G\""
        );
        let restored =
            apply_sql_document_edit_transaction(&appended.document.source, &appended.undo)
                .expect("the appended projection has an exact inverse");
        assert_eq!(restored.document.source, source);

        let source = "SELECT * FROM \"G\" WHERE a = 1 OR b = 2";
        let document = parse_sql_document(source);
        let select = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Select)
            .unwrap();
        let grouped = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::AddFilterComparison {
                    node_id: select.id,
                    expected_kind: select.kind,
                    column: SqlDocumentColumnReference {
                        qualifier: None,
                        column: SqlDocumentIdentifier {
                            value: "count".into(),
                            quote_style: None,
                        },
                    },
                    operator: SqlDocumentFilterOperator::Gt,
                    literal: Some(SqlDocumentSemanticLiteral::Number { value: "3".into() }),
                }],
            ),
        )
        .unwrap();
        assert_eq!(
            grouped.document.source,
            "SELECT * FROM \"G\" WHERE (a = 1 OR b = 2) AND (count > 3)"
        );
        let restored =
            apply_sql_document_edit_transaction(&grouped.document.source, &grouped.undo).unwrap();
        assert_eq!(restored.document.source, source);
    }

    #[test]
    fn semantic_nested_subqueries_and_from_relations_are_typed_and_undoable() {
        let source = "SELECT 1";
        let document = parse_sql_document(source);
        let list = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SelectList)
            .expect("the outer projection list is represented");
        let scalar = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::AddScalarSubqueryProjection {
                    node_id: list.id,
                    expected_kind: SqlDocumentNodeKind::SelectList,
                }],
            ),
        )
        .expect("the scalar subquery is inserted from a typed seed");
        let nested_select = scalar
            .document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::Select
                    && node
                        .range
                        .and_then(|range| scalar.document.source_slice(range))
                        == Some("SELECT 1")
                    && node.parent.is_some_and(|parent| {
                        scalar
                            .document
                            .nodes
                            .get(parent as usize)
                            .is_some_and(|query| {
                                query.kind == SqlDocumentNodeKind::Query
                                    && query.parent.is_some_and(|grandparent| {
                                        scalar.document.nodes.get(grandparent as usize).is_some_and(
                                            |projection| {
                                                projection.kind
                                                    == SqlDocumentNodeKind::SubqueryExpression
                                            },
                                        )
                                    })
                            })
                    })
            })
            .expect("the scalar subquery has a separately editable SELECT scope");
        let from = apply_sql_document_semantic_edit_transaction(
            &scalar.document.source,
            &semantic_transaction(
                &scalar.document.source,
                vec![SqlDocumentSemanticEdit::AddFromRelation {
                    node_id: nested_select.id,
                    expected_kind: SqlDocumentNodeKind::Select,
                    factor: SqlDocumentRelationFactor::Table {
                        name: vec![SqlDocumentIdentifier {
                            value: "tasks".to_owned(),
                            quote_style: Some('"'),
                        }],
                        alias: None,
                    },
                }],
            ),
        )
        .expect("FROM can be added to the nested query without raw SQL");
        assert_eq!(from.document.status, SqlDocumentStatus::Parsed);
        assert_eq!(
            from.document.to_sql(),
            "SELECT 1, (SELECT 1 FROM \"tasks\") AS __ugoite_scalar_subquery"
        );
        let undone = apply_sql_document_edit_transaction(&from.document.source, &from.undo)
            .expect("the nested FROM addition has a guarded inverse");
        assert_eq!(undone.document.to_sql(), scalar.document.source);

        for (template, expected) in [
            (
                SqlDocumentSubqueryPredicateTemplate::In,
                "SELECT (1) IN (SELECT 1)",
            ),
            (
                SqlDocumentSubqueryPredicateTemplate::NotIn,
                "SELECT (1) NOT IN (SELECT 1)",
            ),
        ] {
            let source = "SELECT 1";
            let document = parse_sql_document(source);
            let operand = document
                .nodes
                .iter()
                .find(|node| {
                    is_expression_node_kind(node.kind)
                        && node.range.and_then(|range| document.source_slice(range)) == Some("1")
                        && node.parent.is_some_and(|parent| {
                            document
                                .nodes
                                .get(parent as usize)
                                .is_some_and(|item| item.kind == SqlDocumentNodeKind::SelectItem)
                        })
                })
                .expect("the selected numeric expression is parser-backed");
            let edited = apply_sql_document_semantic_edit_transaction(
                source,
                &semantic_transaction(
                    source,
                    vec![SqlDocumentSemanticEdit::WrapExpressionInSubqueryPredicate {
                        node_id: operand.id,
                        expected_kind: operand.kind,
                        template,
                    }],
                ),
            )
            .expect("the subquery predicate is emitted by a closed typed template");
            assert_eq!(edited.document.status, SqlDocumentStatus::Parsed);
            assert_eq!(edited.document.to_sql(), expected);
            let undone = apply_sql_document_edit_transaction(&edited.document.source, &edited.undo)
                .expect("subquery predicate construction has an exact inverse");
            assert_eq!(undone.document.to_sql(), source);
        }

        let source = "SELECT 1 WHERE TRUE";
        let document = parse_sql_document(source);
        let predicate = document
            .nodes
            .iter()
            .find(|node| {
                is_expression_node_kind(node.kind)
                    && node.range.and_then(|range| document.source_slice(range)) == Some("TRUE")
            })
            .expect("the WHERE seed has a typed boolean leaf");
        let exists = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![
                    SqlDocumentSemanticEdit::ReplaceExpressionWithExistsSubquery {
                        node_id: predicate.id,
                        expected_kind: predicate.kind,
                    },
                ],
            ),
        )
        .expect("EXISTS can be constructed as a typed nested-query seed");
        assert_eq!(exists.document.status, SqlDocumentStatus::Parsed);
        assert_eq!(exists.document.to_sql(), "SELECT 1 WHERE EXISTS (SELECT 1)");
        let undone = apply_sql_document_edit_transaction(&exists.document.source, &exists.undo)
            .expect("EXISTS construction has a guarded inverse");
        assert_eq!(undone.document.to_sql(), source);
    }

    #[test]
    fn semantic_scalar_subquery_projection_is_typed_parseable_and_undoable() {
        let source = "SELECT 1 /* keep */ FROM \"G\" ORDER BY 1";
        let document = parse_sql_document(source);
        let list = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SelectList)
            .expect("the parser exposes the SELECT list");
        let edited = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::AddScalarSubqueryProjection {
                    node_id: list.id,
                    expected_kind: SqlDocumentNodeKind::SelectList,
                }],
            ),
        )
        .expect("a scalar subquery seed can be appended");
        assert_eq!(
            edited.document.source,
            "SELECT 1 /* keep */, (SELECT 1) AS __ugoite_scalar_subquery FROM \"G\" ORDER BY 1"
        );
        assert!(edited
            .document
            .nodes
            .iter()
            .any(|node| { node.kind == SqlDocumentNodeKind::SubqueryExpression }));
        assert!(edited.document.nodes.iter().any(|node| {
            node.kind == SqlDocumentNodeKind::Select
                && node.parent.is_some_and(|parent| {
                    edited.document.nodes[parent as usize].kind == SqlDocumentNodeKind::Query
                })
        }));
        let restored = apply_sql_document_edit_transaction(&edited.document.source, &edited.undo)
            .expect("the subquery insertion has an exact inverse");
        assert_eq!(restored.document.source, source);

        let line_comment_source = "SELECT 1 -- keep\nFROM \"G\"";
        let line_comment_document = parse_sql_document(line_comment_source);
        let line_comment_list = line_comment_document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SelectList)
            .expect("the parser exposes the SELECT list before a line comment");
        let line_comment_edit = apply_sql_document_semantic_edit_transaction(
            line_comment_source,
            &semantic_transaction(
                line_comment_source,
                vec![SqlDocumentSemanticEdit::AddScalarSubqueryProjection {
                    node_id: line_comment_list.id,
                    expected_kind: SqlDocumentNodeKind::SelectList,
                }],
            ),
        )
        .expect("line-comment trivia remains attached to the previous projection");
        assert_eq!(
            line_comment_edit.document.source,
            "SELECT 1 -- keep\n, (SELECT 1) AS __ugoite_scalar_subquery FROM \"G\""
        );
    }

    #[test]
    fn semantic_group_by_expression_adds_before_later_clauses_and_supports_undo() {
        let source = "SELECT field_100, field_200 FROM \"G\" ORDER BY field_100 DESC NULLS LAST";
        let document = parse_sql_document(source);
        let select = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Select)
            .expect("the parser exposes the SELECT body");
        let expression = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::IdentifierExpression
                    && node.parent.is_some_and(|parent| {
                        document.nodes[parent as usize].kind == SqlDocumentNodeKind::SelectItem
                    })
            })
            .expect("the projection exposes its typed identifier");
        let edited = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::AddGroupByExpression {
                    node_id: select.id,
                    expected_kind: SqlDocumentNodeKind::Select,
                    expression_node_id: expression.id,
                    expression_expected_kind: expression.kind,
                }],
            ),
        )
        .expect("a typed expression can be added to GROUP BY");
        assert_eq!(
            edited.document.source,
            "SELECT field_100, field_200 FROM \"G\" GROUP BY field_100 ORDER BY field_100 DESC NULLS LAST"
        );
        let restored = apply_sql_document_edit_transaction(&edited.document.source, &edited.undo)
            .expect("the GROUP BY insertion has an exact inverse");
        assert_eq!(restored.document.source, source);
    }

    #[test]
    fn semantic_projection_columns_expand_wildcards_and_support_undo() {
        let source = "SELECT /* lead */ t.* /* tail */ FROM \"G\" AS t";
        let document = parse_sql_document(source);
        let item = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .expect("the wildcard has a typed projection item");
        assert_eq!(
            item.semantic,
            Some(SqlDocumentNodeData::Wildcard {
                qualifier: vec![SqlDocumentIdentifier {
                    value: "t".to_owned(),
                    quote_style: None,
                }],
                qualifier_is_expression: false,
                has_options: false,
            })
        );
        let expanded = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::ExpandProjectionWildcard {
                    node_id: item.id,
                    expected_kind: SqlDocumentNodeKind::SelectItem,
                    columns: vec!["field_100", "field_200"]
                        .into_iter()
                        .map(|column| SqlDocumentColumnReference {
                            qualifier: Some(SqlDocumentIdentifier {
                                value: "t".to_owned(),
                                quote_style: None,
                            }),
                            column: SqlDocumentIdentifier {
                                value: column.to_owned(),
                                quote_style: None,
                            },
                        })
                        .collect(),
                }],
            ),
        )
        .expect("schema-backed columns expand the wildcard");
        assert_eq!(
            expanded.document.source,
            "SELECT /* lead */ t.field_100, t.field_200 /* tail */ FROM \"G\" AS t"
        );
        assert_eq!(expanded.document.status, SqlDocumentStatus::Parsed);
        let restored =
            apply_sql_document_edit_transaction(&expanded.document.source, &expanded.undo)
                .expect("wildcard expansion has an exact inverse");
        assert_eq!(restored.document.source, source);

        let source = "SELECT a FROM t";
        let document = parse_sql_document(source);
        let list = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SelectList)
            .expect("the projection list is represented");
        let added = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::AddProjectionColumn {
                    node_id: list.id,
                    expected_kind: SqlDocumentNodeKind::SelectList,
                    column: SqlDocumentColumnReference {
                        qualifier: Some(SqlDocumentIdentifier {
                            value: "t".to_owned(),
                            quote_style: None,
                        }),
                        column: SqlDocumentIdentifier {
                            value: "b".to_owned(),
                            quote_style: None,
                        },
                    },
                }],
            ),
        )
        .expect("a typed Form column can be projected without SQL fragments");
        assert_eq!(added.document.source, "SELECT a, t.b FROM t");
        let restored = apply_sql_document_edit_transaction(&added.document.source, &added.undo)
            .expect("column insertion has an exact inverse");
        assert_eq!(restored.document.source, source);

        let commented = "SELECT t /* keep */ .* FROM t";
        let commented_document = parse_sql_document(commented);
        let item = commented_document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .expect("the wildcard item exists");
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                commented,
                &semantic_transaction(
                    commented,
                    vec![SqlDocumentSemanticEdit::ExpandProjectionWildcard {
                        node_id: item.id,
                        expected_kind: SqlDocumentNodeKind::SelectItem,
                        columns: vec![SqlDocumentColumnReference {
                            qualifier: None,
                            column: SqlDocumentIdentifier {
                                value: "a".to_owned(),
                                quote_style: None,
                            },
                        }],
                    }],
                )
            ),
            Err(SqlDocumentEditError::NodeRangeUnavailable)
        );
    }

    #[test]
    fn semantic_order_by_items_can_be_edited_removed_moved_and_undone() {
        let source = "SELECT a, b FROM t ORDER BY a DESC NULLS LAST, b ASC";
        let document = parse_sql_document(source);
        let mut items = document
            .nodes
            .iter()
            .filter(|node| node.kind == SqlDocumentNodeKind::OrderByItem)
            .collect::<Vec<_>>();
        items.sort_by_key(|node| node.range.map(|range| range.start));
        assert_eq!(items.len(), 2);

        let options = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::SetOrderByOptions {
                    node_id: items[0].id,
                    expected_kind: SqlDocumentNodeKind::OrderByItem,
                    direction: SqlDocumentOrderDirection::Asc,
                    nulls_order: None,
                }],
            ),
        )
        .expect("sort direction and null placement are editable independently");
        assert_eq!(
            options.document.source,
            "SELECT a, b FROM t ORDER BY a ASC, b ASC"
        );
        let restored = apply_sql_document_edit_transaction(&options.document.source, &options.undo)
            .expect("sort options have a guarded inverse");
        assert_eq!(restored.document.source, source);

        let moved = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::MoveOrderByItem {
                    node_id: items[1].id,
                    expected_kind: SqlDocumentNodeKind::OrderByItem,
                    to_index: 0,
                }],
            ),
        )
        .expect("sort keys can be reordered as source-backed items");
        assert_eq!(
            moved.document.source,
            "SELECT a, b FROM t ORDER BY b ASC, a DESC NULLS LAST"
        );
        let restored = apply_sql_document_edit_transaction(&moved.document.source, &moved.undo)
            .expect("sort reordering has a guarded inverse");
        assert_eq!(restored.document.source, source);

        let removed = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::RemoveOrderByItem {
                    node_id: items[0].id,
                    expected_kind: SqlDocumentNodeKind::OrderByItem,
                }],
            ),
        )
        .expect("one sort key can be removed while retaining the other");
        assert_eq!(removed.document.source, "SELECT a, b FROM t ORDER BY b ASC");
        let restored = apply_sql_document_edit_transaction(&removed.document.source, &removed.undo)
            .expect("sort-key removal has a guarded inverse");
        assert_eq!(restored.document.source, source);

        let commented = "SELECT a, b FROM t ORDER BY a /* keep */, b";
        let document = parse_sql_document(commented);
        let item = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::OrderByItem)
            .expect("the commented sort key is represented");
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                commented,
                &semantic_transaction(
                    commented,
                    vec![SqlDocumentSemanticEdit::MoveOrderByItem {
                        node_id: item.id,
                        expected_kind: SqlDocumentNodeKind::OrderByItem,
                        to_index: 1,
                    }],
                )
            ),
            Err(SqlDocumentEditError::NodeRangeUnavailable)
        );
    }

    #[test]
    fn semantic_distinct_having_limit_and_offset_edits_are_typed_and_undoable() {
        let source = "SELECT a FROM t";
        let document = parse_sql_document(source);
        let select = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Select)
            .expect("the SELECT node exists");
        let distinct = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::SetSelectDistinct {
                    node_id: select.id,
                    expected_kind: SqlDocumentNodeKind::Select,
                    distinct: true,
                }],
            ),
        )
        .expect("plain DISTINCT can be added");
        assert_eq!(distinct.document.source, "SELECT DISTINCT a FROM t");
        let without_distinct = apply_sql_document_semantic_edit_transaction(
            &distinct.document.source,
            &semantic_transaction(
                &distinct.document.source,
                vec![SqlDocumentSemanticEdit::SetSelectDistinct {
                    node_id: distinct
                        .document
                        .nodes
                        .iter()
                        .find(|node| node.kind == SqlDocumentNodeKind::Select)
                        .expect("the edited SELECT remains typed")
                        .id,
                    expected_kind: SqlDocumentNodeKind::Select,
                    distinct: false,
                }],
            ),
        )
        .expect("plain DISTINCT can be removed");
        assert_eq!(without_distinct.document.source, "SELECT  a FROM t");

        let source = "SELECT a, b FROM t GROUP BY a, b ORDER BY a LIMIT 10";
        let document = parse_sql_document(source);
        let select = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Select)
            .expect("the SELECT body exists");
        let having = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::AddHavingClause {
                    node_id: select.id,
                    expected_kind: SqlDocumentNodeKind::Select,
                }],
            ),
        )
        .expect("HAVING receives a typed boolean seed");
        assert_eq!(
            having.document.source,
            "SELECT a, b FROM t GROUP BY a, b HAVING TRUE ORDER BY a LIMIT 10"
        );
        let restored = apply_sql_document_edit_transaction(&having.document.source, &having.undo)
            .expect("HAVING insertion has an exact inverse");
        assert_eq!(restored.document.source, source);
        let having_clause = having
            .document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::HavingClause)
            .expect("the new HAVING clause is represented");
        let removed_having = apply_sql_document_semantic_edit_transaction(
            &having.document.source,
            &semantic_transaction(
                &having.document.source,
                vec![SqlDocumentSemanticEdit::RemoveHavingClause {
                    node_id: having_clause.id,
                    expected_kind: SqlDocumentNodeKind::HavingClause,
                }],
            ),
        )
        .expect("the whole HAVING clause can be removed");
        assert!(!removed_having.document.source.contains("HAVING"));
        let restored = apply_sql_document_edit_transaction(
            &removed_having.document.source,
            &removed_having.undo,
        )
        .expect("HAVING removal has a guarded inverse");
        assert_eq!(restored.document.source, having.document.source);

        let source = "SELECT a FROM t ORDER BY a";
        let document = parse_sql_document(source);
        let query = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Query)
            .expect("the query exists");
        let with_limit = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::SetLimitValue {
                    node_id: query.id,
                    expected_kind: SqlDocumentNodeKind::Query,
                    value: 10,
                }],
            ),
        )
        .expect("a typed LIMIT value can be added");
        assert_eq!(
            with_limit.document.source,
            "SELECT a FROM t ORDER BY a LIMIT 10"
        );
        let query = with_limit
            .document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Query)
            .expect("the changed query remains typed");
        let with_offset = apply_sql_document_semantic_edit_transaction(
            &with_limit.document.source,
            &semantic_transaction(
                &with_limit.document.source,
                vec![SqlDocumentSemanticEdit::SetOffsetValue {
                    node_id: query.id,
                    expected_kind: SqlDocumentNodeKind::Query,
                    value: 3,
                }],
            ),
        )
        .expect("a typed OFFSET value can be added");
        assert_eq!(
            with_offset.document.source,
            "SELECT a FROM t ORDER BY a LIMIT 10 OFFSET 3"
        );
        let query = with_offset
            .document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Query)
            .expect("the query remains typed");
        let without_limit = apply_sql_document_semantic_edit_transaction(
            &with_offset.document.source,
            &semantic_transaction(
                &with_offset.document.source,
                vec![SqlDocumentSemanticEdit::RemoveLimit {
                    node_id: query.id,
                    expected_kind: SqlDocumentNodeKind::Query,
                }],
            ),
        )
        .expect("LIMIT can be removed while OFFSET remains");
        assert!(without_limit.document.source.contains("OFFSET 3"));
        assert!(!without_limit.document.source.contains("LIMIT"));
        let query = without_limit
            .document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Query)
            .expect("the query without LIMIT remains typed");
        let without_offset = apply_sql_document_semantic_edit_transaction(
            &without_limit.document.source,
            &semantic_transaction(
                &without_limit.document.source,
                vec![SqlDocumentSemanticEdit::RemoveOffset {
                    node_id: query.id,
                    expected_kind: SqlDocumentNodeKind::Query,
                }],
            ),
        )
        .expect("OFFSET-only can be removed");
        assert!(!without_offset.document.source.contains("OFFSET"));

        let commented_limit = "SELECT a FROM t LIMIT /* keep */ 10";
        let document = parse_sql_document(commented_limit);
        let query = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Query)
            .expect("the query with commented LIMIT exists");
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                commented_limit,
                &semantic_transaction(
                    commented_limit,
                    vec![SqlDocumentSemanticEdit::SetLimitValue {
                        node_id: query.id,
                        expected_kind: SqlDocumentNodeKind::Query,
                        value: 5,
                    }],
                )
            ),
            Err(SqlDocumentEditError::NodeRangeUnavailable)
        );
    }

    #[test]
    fn semantic_filter_group_and_aggregate_ops_are_typed_and_undoable() {
        let source = "SELECT DISTINCT amount > 0, amount FROM t WHERE amount = 1 GROUP BY amount, amount + 1 HAVING SUM(amount) > 0 ORDER BY amount DESC NULLS LAST LIMIT 10 OFFSET 2";
        let document = parse_sql_document(source);
        assert_eq!(document.status, SqlDocumentStatus::Parsed);

        let select = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Select)
            .expect("SELECT is represented");
        assert_eq!(
            select.semantic,
            Some(SqlDocumentNodeData::Select {
                distinct: true,
                distinct_on: false,
            })
        );
        let order = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::OrderByItem)
            .expect("ORDER BY item is represented");
        assert_eq!(
            order.semantic,
            Some(SqlDocumentNodeData::OrderByItem {
                direction: Some(SqlDocumentOrderDirection::Desc),
                nulls_order: Some(SqlDocumentNullsOrder::Last),
            })
        );
        let limit = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::LimitClause)
            .expect("LIMIT is represented");
        assert_eq!(
            limit.semantic,
            Some(SqlDocumentNodeData::QueryClauseValue { value: Some(10) })
        );
        let offset = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::OffsetClause)
            .expect("OFFSET is represented");
        assert_eq!(
            offset.semantic,
            Some(SqlDocumentNodeData::QueryClauseValue { value: Some(2) })
        );

        let where_clause = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::WhereClause)
            .expect("WHERE is represented");
        let replacement = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::BinaryExpression
                    && document.source_slice(node.range.expect("expression range"))
                        == Some("amount > 0")
            })
            .expect("the projection predicate is a reusable typed expression");
        let filter = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::SetFilterPredicate {
                    node_id: where_clause.id,
                    expected_kind: SqlDocumentNodeKind::WhereClause,
                    expression_node_id: replacement.id,
                    expression_expected_kind: replacement.kind,
                }],
            ),
        )
        .expect("a filter can be replaced from a same-query typed expression");
        assert!(filter.document.source.contains("WHERE amount > 0"));
        let restored = apply_sql_document_edit_transaction(&filter.document.source, &filter.undo)
            .expect("filter replacement has a guarded inverse");
        assert_eq!(restored.document.source, source);
        let removed_filter = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::RemoveFilterClause {
                    node_id: where_clause.id,
                    expected_kind: SqlDocumentNodeKind::WhereClause,
                }],
            ),
        )
        .expect("WHERE can be removed independently of other clauses");
        assert!(!removed_filter.document.source.contains("WHERE"));
        assert!(removed_filter.document.source.contains("GROUP BY"));
        let restored = apply_sql_document_edit_transaction(
            &removed_filter.document.source,
            &removed_filter.undo,
        )
        .expect("filter removal has a guarded inverse");
        assert_eq!(restored.document.source, source);

        let group = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::GroupByClause)
            .expect("GROUP BY is represented");
        let mut group_items = document
            .nodes
            .iter()
            .filter(|node| {
                node.parent == Some(group.id) && node.kind == SqlDocumentNodeKind::GroupByItem
            })
            .collect::<Vec<_>>();
        group_items.sort_by_key(|item| item.range.map(|range| range.start));
        assert_eq!(group_items.len(), 2);
        let moved_group = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::MoveGroupByItem {
                    node_id: group_items[1].id,
                    expected_kind: SqlDocumentNodeKind::GroupByItem,
                    to_index: 0,
                }],
            ),
        )
        .expect("GROUP BY keys can be reordered as source-backed nodes");
        assert!(moved_group
            .document
            .source
            .contains("GROUP BY amount + 1, amount"));
        let restored =
            apply_sql_document_edit_transaction(&moved_group.document.source, &moved_group.undo)
                .expect("group-key reordering has an exact inverse");
        assert_eq!(restored.document.source, source);

        let removed_group = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::RemoveGroupByItem {
                    node_id: group_items[1].id,
                    expected_kind: SqlDocumentNodeKind::GroupByItem,
                }],
            ),
        )
        .expect("a GROUP BY key can be removed");
        assert!(removed_group
            .document
            .source
            .contains("GROUP BY amount HAVING"));
        let restored = apply_sql_document_edit_transaction(
            &removed_group.document.source,
            &removed_group.undo,
        )
        .expect("group-key removal has an exact inverse");
        assert_eq!(restored.document.source, source);

        let aggregate = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::FunctionExpression
                    && matches!(
                        node.semantic,
                        Some(SqlDocumentNodeData::Function { ref name, .. })
                            if name.last().is_some_and(|part| part.value.eq_ignore_ascii_case("sum"))
                    )
            })
            .expect("the registered SUM expression is represented");
        let changed = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::SetAggregateFunction {
                    node_id: aggregate.id,
                    expected_kind: SqlDocumentNodeKind::FunctionExpression,
                    function: SqlDocumentAggregateFunction::Avg,
                }],
            ),
        )
        .expect("an aggregate can change to another registered function");
        assert!(changed.document.source.contains("HAVING AVG(amount) > 0"));
        let restored = apply_sql_document_edit_transaction(&changed.document.source, &changed.undo)
            .expect("aggregate function change has an exact inverse");
        assert_eq!(restored.document.source, source);

        let changed_document = parse_sql_document(&changed.document.source);
        let avg = changed_document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::FunctionExpression
                    && matches!(
                        node.semantic,
                        Some(SqlDocumentNodeData::Function { ref name, .. })
                            if name.last().is_some_and(|part| part.value.eq_ignore_ascii_case("avg"))
                    )
            })
            .expect("the changed aggregate remains represented");
        let distinct = apply_sql_document_semantic_edit_transaction(
            &changed.document.source,
            &semantic_transaction(
                &changed.document.source,
                vec![SqlDocumentSemanticEdit::SetAggregateDistinct {
                    node_id: avg.id,
                    expected_kind: SqlDocumentNodeKind::FunctionExpression,
                    distinct: true,
                }],
            ),
        )
        .expect("aggregate DISTINCT is toggleable");
        assert!(distinct.document.source.contains("AVG(DISTINCT amount)"));
        let distinct_document = parse_sql_document(&distinct.document.source);
        let avg = distinct_document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::FunctionExpression
                    && matches!(
                        node.semantic,
                        Some(SqlDocumentNodeData::Function { ref name, .. })
                            if name.last().is_some_and(|part| part.value.eq_ignore_ascii_case("avg"))
                    )
            })
            .expect("the DISTINCT aggregate remains represented");
        let removed_aggregate = apply_sql_document_semantic_edit_transaction(
            &distinct.document.source,
            &semantic_transaction(
                &distinct.document.source,
                vec![SqlDocumentSemanticEdit::RemoveAggregate {
                    node_id: avg.id,
                    expected_kind: SqlDocumentNodeKind::FunctionExpression,
                }],
            ),
        )
        .expect("a simple aggregate wrapper can be removed");
        assert!(removed_aggregate
            .document
            .source
            .contains("HAVING amount > 0"));
        let restored = apply_sql_document_edit_transaction(
            &removed_aggregate.document.source,
            &removed_aggregate.undo,
        )
        .expect("aggregate removal has a guarded inverse");
        assert_eq!(restored.document.source, distinct.document.source);
    }

    #[test]
    fn semantic_filter_group_and_aggregate_edits_fail_closed_on_comments() {
        let filter_source = "SELECT amount > 0 FROM t WHERE amount /* keep */ = 1";
        let filter_document = parse_sql_document(filter_source);
        let where_clause = filter_document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::WhereClause)
            .expect("WHERE is represented");
        let replacement = filter_document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::BinaryExpression
                    && filter_document.source_slice(node.range.unwrap()) == Some("amount > 0")
            })
            .expect("a same-scope predicate is available");
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                filter_source,
                &semantic_transaction(
                    filter_source,
                    vec![SqlDocumentSemanticEdit::SetFilterPredicate {
                        node_id: where_clause.id,
                        expected_kind: where_clause.kind,
                        expression_node_id: replacement.id,
                        expression_expected_kind: replacement.kind,
                    }],
                ),
            ),
            Err(SqlDocumentEditError::NodeRangeUnavailable),
            "a replacement must not erase an embedded SQL comment"
        );

        let aggregate_source = "SELECT SUM(/* keep */ amount) FROM t";
        let aggregate_document = parse_sql_document(aggregate_source);
        let aggregate = aggregate_document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::FunctionExpression)
            .expect("the aggregate function is represented");
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                aggregate_source,
                &semantic_transaction(
                    aggregate_source,
                    vec![SqlDocumentSemanticEdit::RemoveAggregate {
                        node_id: aggregate.id,
                        expected_kind: aggregate.kind,
                    }],
                ),
            ),
            Err(SqlDocumentEditError::NodeRangeUnavailable),
            "unwrapping must not discard comments inside the function call"
        );
    }

    #[test]
    fn semantic_order_by_expression_appends_and_inserts_before_limit_with_undo() {
        let source = "SELECT field_100 FROM \"G\" LIMIT 10";
        let document = parse_sql_document(source);
        let query = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Query)
            .expect("the parser exposes the query");
        let expression = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::IdentifierExpression
                    && node.parent.is_some_and(|parent| {
                        document.nodes[parent as usize].kind == SqlDocumentNodeKind::SelectItem
                    })
            })
            .expect("the projection exposes its typed identifier");
        let edited = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::AddOrderByExpression {
                    node_id: query.id,
                    expected_kind: SqlDocumentNodeKind::Query,
                    expression_node_id: expression.id,
                    expression_expected_kind: expression.kind,
                    direction: SqlDocumentOrderDirection::Desc,
                    nulls_order: Some(SqlDocumentNullsOrder::Last),
                }],
            ),
        )
        .expect("a typed expression can be added before LIMIT");
        assert_eq!(
            edited.document.source,
            "SELECT field_100 FROM \"G\" ORDER BY field_100 DESC NULLS LAST LIMIT 10"
        );
        let restored = apply_sql_document_edit_transaction(&edited.document.source, &edited.undo)
            .expect("the ORDER BY insertion has an exact inverse");
        assert_eq!(restored.document.source, source);

        let source = "SELECT field_100, field_200 FROM \"G\" ORDER BY field_100 DESC NULLS LAST";
        let document = parse_sql_document(source);
        let query = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Query)
            .expect("the parser exposes the query");
        let mut identifiers = document
            .nodes
            .iter()
            .filter(|node| node.kind == SqlDocumentNodeKind::IdentifierExpression)
            .collect::<Vec<_>>();
        identifiers.sort_by_key(|node| node.range.map(|range| range.start));
        let order_expression = identifiers
            .iter()
            .copied()
            .find(|node| {
                node.parent.is_some_and(|parent| {
                    document.nodes[parent as usize].kind == SqlDocumentNodeKind::OrderByItem
                })
            })
            .expect("the existing key is parsed");
        let projection_expression = identifiers
            .iter()
            .copied()
            .find(|node| {
                node.range.and_then(|range| document.source_slice(range)) == Some("field_200")
                    && node.parent.is_some_and(|parent| {
                        document.nodes[parent as usize].kind == SqlDocumentNodeKind::SelectItem
                    })
            })
            .expect("the projection key is parsed");
        let edited = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::AddOrderByExpression {
                    node_id: query.id,
                    expected_kind: SqlDocumentNodeKind::Query,
                    expression_node_id: order_expression.id,
                    expression_expected_kind: order_expression.kind,
                    direction: SqlDocumentOrderDirection::Asc,
                    nulls_order: None,
                }],
            ),
        );
        assert_eq!(
            edited,
            Err(SqlDocumentEditError::InvalidSemanticReplacement)
        );
        let edited = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::AddOrderByExpression {
                    node_id: query.id,
                    expected_kind: SqlDocumentNodeKind::Query,
                    expression_node_id: projection_expression.id,
                    expression_expected_kind: projection_expression.kind,
                    direction: SqlDocumentOrderDirection::Desc,
                    nulls_order: Some(SqlDocumentNullsOrder::First),
                }],
            ),
        )
        .expect("a second key can be appended");
        assert_eq!(
            edited.document.source,
            "SELECT field_100, field_200 FROM \"G\" ORDER BY field_100 DESC NULLS LAST, field_200 DESC NULLS FIRST"
        );
    }

    #[test]
    fn semantic_expression_leaf_edits_create_literals_and_identifiers_with_undo() {
        let source = "SELECT field_100 + 1 FROM \"G\"";
        let document = parse_sql_document(source);
        let target = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::BinaryExpression)
            .expect("the arithmetic expression is parsed");
        let literal = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::ReplaceLiteral {
                    node_id: target.id,
                    expected_kind: target.kind,
                    literal: SqlDocumentSemanticLiteral::Number {
                        value: "7".to_owned(),
                    },
                }],
            ),
        )
        .expect("an expression can be replaced by a typed literal leaf");
        assert_eq!(literal.document.source, "SELECT 7 FROM \"G\"");
        let restored = apply_sql_document_edit_transaction(&literal.document.source, &literal.undo)
            .expect("the literal replacement has an exact inverse");
        assert_eq!(restored.document.source, source);

        let source = "SELECT 1 FROM \"G\"";
        let document = parse_sql_document(source);
        let target = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Value)
            .expect("the value expression is parsed");
        let identifier = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::ReplaceIdentifier {
                    node_id: target.id,
                    expected_kind: target.kind,
                    parts: vec![SqlDocumentIdentifier {
                        value: "field_100".to_owned(),
                        quote_style: None,
                    }],
                }],
            ),
        )
        .expect("an expression can be replaced by a typed identifier leaf");
        assert_eq!(identifier.document.source, "SELECT field_100 FROM \"G\"");
        let restored =
            apply_sql_document_edit_transaction(&identifier.document.source, &identifier.undo)
                .expect("the identifier replacement has an exact inverse");
        assert_eq!(restored.document.source, source);
    }

    #[test]
    fn semantic_expression_cast_wrapper_creates_typed_cast_with_undo() {
        let source = "SELECT field_100 + 1 FROM \"G\"";
        let document = parse_sql_document(source);
        let expression = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::BinaryExpression)
            .expect("the arithmetic expression is parsed");
        let edited = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::WrapExpressionInCast {
                    node_id: expression.id,
                    expected_kind: expression.kind,
                    target_type: SqlDocumentCastType::Double,
                }],
            ),
        )
        .expect("an expression can be wrapped with a typed CAST");
        assert_eq!(
            edited.document.source,
            "SELECT CAST(field_100 + 1 AS DOUBLE) FROM \"G\""
        );
        assert!(edited.document.nodes.iter().any(|node| {
            node.kind == SqlDocumentNodeKind::CastExpression
                && node.semantic.as_ref().is_some_and(|semantic| {
                    matches!(
                        semantic,
                        SqlDocumentNodeData::Cast {
                            cast_kind,
                            target_type: Some(target_type),
                        } if cast_kind == "cast" && target_type == "DOUBLE"
                    )
                })
        }));
        let restored = apply_sql_document_edit_transaction(&edited.document.source, &edited.undo)
            .expect("the CAST wrapper has an exact inverse");
        assert_eq!(restored.document.source, source);

        let source = "SELECT field_100 /* retained */ + 1 FROM \"G\"";
        let document = parse_sql_document(source);
        let expression = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::BinaryExpression)
            .expect("the commented expression is parsed");
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                source,
                &semantic_transaction(
                    source,
                    vec![SqlDocumentSemanticEdit::WrapExpressionInCast {
                        node_id: expression.id,
                        expected_kind: expression.kind,
                        target_type: SqlDocumentCastType::Varchar,
                    }],
                ),
            ),
            Err(SqlDocumentEditError::NodeRangeUnavailable)
        );
    }

    #[test]
    fn semantic_expression_aggregate_wrapper_uses_registered_function_and_undo() {
        let source = "SELECT amount + 1 FROM \"G\"";
        let document = parse_sql_document(source);
        let expression = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::BinaryExpression)
            .expect("the arithmetic expression is parsed");
        let edited = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::WrapExpressionInAggregate {
                    node_id: expression.id,
                    expected_kind: expression.kind,
                    function: SqlDocumentAggregateFunction::Sum,
                    distinct: true,
                }],
            ),
        )
        .expect("a registered aggregate can wrap a typed expression");
        assert_eq!(
            edited.document.source,
            "SELECT SUM(DISTINCT amount + 1) FROM \"G\""
        );
        assert!(edited.document.nodes.iter().any(|node| {
            node.kind == SqlDocumentNodeKind::FunctionExpression
                && matches!(
                    node.semantic.as_ref(),
                    Some(SqlDocumentNodeData::Function {
                        name,
                        distinct: true,
                        ..
                    }) if name.last().is_some_and(|part| part.value.eq_ignore_ascii_case("sum"))
                )
        }));
        let restored = apply_sql_document_edit_transaction(&edited.document.source, &edited.undo)
            .expect("the aggregate wrapper has an exact inverse");
        assert_eq!(restored.document.source, source);

        let source = "SELECT amount FROM \"G\"";
        let document = parse_sql_document(source);
        let expression = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::IdentifierExpression)
            .expect("the identifier is parsed");
        let edited = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::WrapExpressionInAggregate {
                    node_id: expression.id,
                    expected_kind: expression.kind,
                    function: SqlDocumentAggregateFunction::Count,
                    distinct: true,
                }],
            ),
        )
        .expect("COUNT DISTINCT uses the same registry-backed wrapper");
        assert_eq!(
            edited.document.source,
            "SELECT COUNT(DISTINCT amount) FROM \"G\""
        );
    }

    #[test]
    fn semantic_predicate_templates_create_editable_typed_operands_and_undo() {
        let source = "SELECT amount + 1 FROM \"G\"";
        let document = parse_sql_document(source);
        let expression = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::BinaryExpression)
            .expect("the arithmetic expression is parsed");
        let edited = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::WrapExpressionInPredicate {
                    node_id: expression.id,
                    expected_kind: expression.kind,
                    template: SqlDocumentPredicateTemplate::Between,
                }],
            ),
        )
        .expect("the BETWEEN template is a typed structural edit");
        assert_eq!(
            edited.document.source,
            "SELECT (amount + 1) BETWEEN NULL AND NULL FROM \"G\""
        );
        assert_eq!(
            edited
                .document
                .nodes
                .iter()
                .filter(|node| {
                    node.kind == SqlDocumentNodeKind::LiteralExpression
                        && node
                            .range
                            .and_then(|range| edited.document.source_slice(range))
                            == Some("NULL")
                })
                .count(),
            2,
            "both BETWEEN bounds remain independently editable literal nodes"
        );
        let restored = apply_sql_document_edit_transaction(&edited.document.source, &edited.undo)
            .expect("the predicate template has an exact inverse");
        assert_eq!(restored.document.source, source);
    }

    #[test]
    fn semantic_predicate_template_rejects_comment_bearing_operands() {
        let source = "SELECT amount + /* retained */ 1 FROM \"G\"";
        let document = parse_sql_document(source);
        let expression = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::BinaryExpression)
            .expect("the binary expression is parsed");
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                source,
                &semantic_transaction(
                    source,
                    vec![SqlDocumentSemanticEdit::WrapExpressionInPredicate {
                        node_id: expression.id,
                        expected_kind: expression.kind,
                        template: SqlDocumentPredicateTemplate::IsNull,
                    }],
                ),
            )
            .unwrap_err(),
            SqlDocumentEditError::NodeRangeUnavailable
        );
    }

    #[test]
    fn semantic_in_list_values_append_remove_and_undo_preserve_surrounding_source() {
        let source =
            "SELECT amount FROM \"G\" WHERE amount IN (1, (2 + 3), 4) /* keep */ ORDER BY amount";
        let document = parse_sql_document(source);
        let predicate = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::PredicateExpression
                    && matches!(
                        node.semantic,
                        Some(SqlDocumentNodeData::Predicate { ref operator, .. })
                            if operator == "in"
                    )
            })
            .expect("the IN predicate is parsed");
        let appended = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::AppendInListValue {
                    node_id: predicate.id,
                    expected_kind: predicate.kind,
                }],
            ),
        )
        .expect("IN values can be appended as typed NULL leaves");
        assert_eq!(
            appended.document.source,
            "SELECT amount FROM \"G\" WHERE amount IN (1, (2 + 3), 4, NULL) /* keep */ ORDER BY amount"
        );

        let appended_predicate = appended
            .document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::PredicateExpression
                    && matches!(
                        node.semantic,
                        Some(SqlDocumentNodeData::Predicate { ref operator, .. })
                            if operator == "in"
                    )
            })
            .expect("the updated IN predicate is parsed");
        let removed = apply_sql_document_semantic_edit_transaction(
            &appended.document.source,
            &semantic_transaction(
                &appended.document.source,
                vec![SqlDocumentSemanticEdit::RemoveLastInListValue {
                    node_id: appended_predicate.id,
                    expected_kind: appended_predicate.kind,
                }],
            ),
        )
        .expect("the final IN value can be removed while another remains");
        assert_eq!(removed.document.source, source);
        let restored = apply_sql_document_edit_transaction(&removed.document.source, &removed.undo)
            .expect("the IN list removal has an exact inverse");
        assert_eq!(restored.document.source, appended.document.source);
    }

    #[test]
    fn semantic_in_list_edits_reject_single_value_removal_and_comments_inside_list() {
        for source in [
            "SELECT amount FROM \"G\" WHERE amount IN (1)",
            "SELECT amount FROM \"G\" WHERE amount IN (1, /* keep */ 2)",
        ] {
            let document = parse_sql_document(source);
            let predicate = document
                .nodes
                .iter()
                .find(|node| {
                    node.kind == SqlDocumentNodeKind::PredicateExpression
                        && matches!(
                            node.semantic,
                            Some(SqlDocumentNodeData::Predicate { ref operator, .. })
                                if operator == "in"
                        )
                })
                .expect("the IN predicate is parsed");
            assert_eq!(
                apply_sql_document_semantic_edit_transaction(
                    source,
                    &semantic_transaction(
                        source,
                        vec![SqlDocumentSemanticEdit::RemoveLastInListValue {
                            node_id: predicate.id,
                            expected_kind: predicate.kind,
                        }],
                    ),
                )
                .unwrap_err(),
                if source.contains("/*") {
                    SqlDocumentEditError::NodeRangeUnavailable
                } else {
                    SqlDocumentEditError::InvalidSemanticReplacement
                }
            );
        }
    }

    #[test]
    fn semantic_where_clause_seed_is_added_before_later_clauses_and_is_undoable() {
        let source = "SELECT 1 FROM \"G\" GROUP BY 1 ORDER BY 1 LIMIT 2";
        let document = parse_sql_document(source);
        let select = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Select)
            .expect("the SELECT is represented in the syntax tree");
        let edited = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::AddWhereClause {
                    node_id: select.id,
                    expected_kind: select.kind,
                }],
            ),
        )
        .expect("a missing WHERE clause receives an editable boolean seed");
        assert_eq!(
            edited.document.source,
            "SELECT 1 FROM \"G\" WHERE TRUE GROUP BY 1 ORDER BY 1 LIMIT 2"
        );
        assert!(edited.document.nodes.iter().any(|node| {
            node.kind == SqlDocumentNodeKind::WhereClause
                && node
                    .range
                    .and_then(|range| edited.document.source_slice(range))
                    == Some("WHERE TRUE")
        }));
        let restored = apply_sql_document_edit_transaction(&edited.document.source, &edited.undo)
            .expect("the WHERE seed has an exact inverse");
        assert_eq!(restored.document.source, source);
    }

    #[test]
    fn task_filter_and_sort_constructors_create_typed_columns_and_undo() {
        let source = "SELECT * FROM \"G\" LIMIT 20";
        let document = parse_sql_document(source);
        let select = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Select)
            .unwrap();
        let query = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Query)
            .unwrap();
        let column = SqlDocumentColumnReference {
            qualifier: None,
            column: SqlDocumentIdentifier {
                value: "count".into(),
                quote_style: None,
            },
        };
        let filtered = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::AddFilterComparison {
                    node_id: select.id,
                    expected_kind: SqlDocumentNodeKind::Select,
                    column: column.clone(),
                    operator: SqlDocumentFilterOperator::Gte,
                    literal: Some(SqlDocumentSemanticLiteral::Number {
                        value: "9007199254740993".into(),
                    }),
                }],
            ),
        )
        .expect("the task editor adds a typed comparison without copying an AST node");
        assert_eq!(
            filtered.document.source,
            "SELECT * FROM \"G\" WHERE count >= 9007199254740993 LIMIT 20"
        );
        let restored =
            apply_sql_document_edit_transaction(&filtered.document.source, &filtered.undo).unwrap();
        assert_eq!(restored.document.source, source);

        let sorted = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::AddOrderByColumn {
                    node_id: query.id,
                    expected_kind: SqlDocumentNodeKind::Query,
                    column,
                    direction: SqlDocumentOrderDirection::Desc,
                    nulls_order: Some(SqlDocumentNullsOrder::Last),
                }],
            ),
        )
        .expect("the task editor inserts a typed sort key before LIMIT");
        assert_eq!(
            sorted.document.source,
            "SELECT * FROM \"G\" ORDER BY count DESC NULLS LAST LIMIT 20"
        );
        let restored =
            apply_sql_document_edit_transaction(&sorted.document.source, &sorted.undo).unwrap();
        assert_eq!(restored.document.source, source);
    }

    #[test]
    fn semantic_where_clause_seed_rejects_a_second_where_clause() {
        let source = "SELECT 1 WHERE TRUE";
        let document = parse_sql_document(source);
        let select = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Select)
            .expect("the SELECT is represented in the syntax tree");
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                source,
                &semantic_transaction(
                    source,
                    vec![SqlDocumentSemanticEdit::AddWhereClause {
                        node_id: select.id,
                        expected_kind: select.kind,
                    }],
                ),
            )
            .unwrap_err(),
            SqlDocumentEditError::InvalidSemanticReplacement
        );
    }

    #[test]
    fn semantic_expression_case_wrapper_creates_editable_searched_case_with_undo() {
        let source = "SELECT amount + 1 FROM \"G\"";
        let document = parse_sql_document(source);
        let expression = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::BinaryExpression)
            .expect("the arithmetic expression is parsed");
        let edited = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::WrapExpressionInCase {
                    node_id: expression.id,
                    expected_kind: expression.kind,
                }],
            ),
        )
        .expect("a typed expression can seed a searched CASE");
        assert_eq!(
            edited.document.source,
            "SELECT CASE WHEN TRUE THEN amount + 1 ELSE NULL END FROM \"G\""
        );
        assert!(edited.document.nodes.iter().any(|node| {
            node.kind == SqlDocumentNodeKind::CaseExpression
                && matches!(
                    node.semantic.as_ref(),
                    Some(SqlDocumentNodeData::Case {
                        has_operand: false,
                        when_count: 1,
                        has_else: true,
                    })
                )
        }));
        let restored = apply_sql_document_edit_transaction(&edited.document.source, &edited.undo)
            .expect("the CASE wrapper has an exact inverse");
        assert_eq!(restored.document.source, source);
    }

    #[test]
    fn projection_alias_edits_are_typed_guarded_and_preserve_bindings() {
        for clause in [
            "ORDER BY total",
            "GROUP BY total",
            "GROUP BY amount HAVING total > 1",
        ] {
            let source = format!("SELECT amount AS total FROM \"G\" {clause}");
            let document = parse_sql_document(&source);
            let item = document
                .nodes
                .iter()
                .find(|node| node.kind == SqlDocumentNodeKind::SelectItem)
                .unwrap();
            assert!(matches!(
                item.semantic,
                Some(SqlDocumentNodeData::ProjectionAlias { .. })
            ));
            let conflict = SqlDocumentSemanticEdit::SetProjectionAlias {
                node_id: item.id,
                expected_kind: SqlDocumentNodeKind::SelectItem,
                alias: Some(SqlDocumentIdentifier {
                    value: "renamed".into(),
                    quote_style: Some('"'),
                }),
            };
            assert_eq!(
                apply_sql_document_semantic_edit_transaction(
                    &source,
                    &semantic_transaction(&source, vec![conflict])
                ),
                Err(SqlDocumentEditError::ProjectionAliasConflict),
                "{clause}"
            );
        }

        let capture_source = "SELECT amount FROM \"G\" ORDER BY total";
        let capture_document = parse_sql_document(capture_source);
        let item = capture_document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .unwrap();
        let capture = SqlDocumentSemanticEdit::SetProjectionAlias {
            node_id: item.id,
            expected_kind: SqlDocumentNodeKind::SelectItem,
            alias: Some(SqlDocumentIdentifier {
                value: "total".into(),
                quote_style: Some('"'),
            }),
        };
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                capture_source,
                &semantic_transaction(capture_source, vec![capture])
            ),
            Err(SqlDocumentEditError::ProjectionAliasConflict)
        );

        let duplicate_source = "SELECT amount, other FROM \"G\"";
        let duplicate_document = parse_sql_document(duplicate_source);
        let items = duplicate_document
            .nodes
            .iter()
            .filter(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .collect::<Vec<_>>();
        let duplicate = |node_id| SqlDocumentSemanticEdit::SetProjectionAlias {
            node_id,
            expected_kind: SqlDocumentNodeKind::SelectItem,
            alias: Some(SqlDocumentIdentifier {
                value: "total".into(),
                quote_style: Some('"'),
            }),
        };
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                duplicate_source,
                &semantic_transaction(
                    duplicate_source,
                    vec![duplicate(items[0].id), duplicate(items[1].id)]
                )
            ),
            Err(SqlDocumentEditError::ProjectionAliasConflict)
        );
        let implicit_name_conflict = SqlDocumentSemanticEdit::SetProjectionAlias {
            node_id: items[0].id,
            expected_kind: SqlDocumentNodeKind::SelectItem,
            alias: Some(SqlDocumentIdentifier {
                value: "other".into(),
                quote_style: Some('"'),
            }),
        };
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                duplicate_source,
                &semantic_transaction(duplicate_source, vec![implicit_name_conflict])
            ),
            Err(SqlDocumentEditError::ProjectionAliasConflict)
        );

        let qualified_source = "SELECT q.amount, q.other FROM \"G\" q";
        let qualified_document = parse_sql_document(qualified_source);
        let qualified_first = qualified_document
            .nodes
            .iter()
            .filter(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .min_by_key(|node| node.range.unwrap().start)
            .unwrap();
        let qualified_conflict = SqlDocumentSemanticEdit::SetProjectionAlias {
            node_id: qualified_first.id,
            expected_kind: SqlDocumentNodeKind::SelectItem,
            alias: Some(SqlDocumentIdentifier {
                value: "other".into(),
                quote_style: Some('\"'),
            }),
        };
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                qualified_source,
                &semantic_transaction(qualified_source, vec![qualified_conflict])
            ),
            Err(SqlDocumentEditError::ProjectionAliasConflict)
        );

        let nested_source =
            "SELECT amount AS total FROM \"G\" WHERE EXISTS (SELECT total FROM \"G\")";
        let nested_document = parse_sql_document(nested_source);
        let outer = nested_document
            .nodes
            .iter()
            .filter(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .min_by_key(|node| node.range.unwrap().start)
            .unwrap();
        let nested_edit = SqlDocumentSemanticEdit::SetProjectionAlias {
            node_id: outer.id,
            expected_kind: SqlDocumentNodeKind::SelectItem,
            alias: Some(SqlDocumentIdentifier {
                value: "renamed".into(),
                quote_style: Some('"'),
            }),
        };
        let renamed = apply_sql_document_semantic_edit_transaction(
            nested_source,
            &semantic_transaction(nested_source, vec![nested_edit]),
        )
        .expect("nested query identifiers belong to their own scope");
        assert!(renamed.document.source.contains("SELECT total FROM \"G\")"));

        let source = "SELECT amount /* keep */ FROM \"G\" -- tail";
        let document = parse_sql_document(source);
        let item = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .unwrap();
        let edit = SqlDocumentSemanticEdit::SetProjectionAlias {
            node_id: item.id,
            expected_kind: SqlDocumentNodeKind::SelectItem,
            alias: Some(SqlDocumentIdentifier {
                value: "display total".into(),
                quote_style: Some('"'),
            }),
        };
        let result = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(source, vec![edit]),
        )
        .unwrap();
        assert_eq!(
            result.document.source,
            "SELECT amount AS \"display total\" /* keep */ FROM \"G\" -- tail"
        );
        let edited = parse_sql_document(&result.document.source);
        let item = edited
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .unwrap();
        let rename = SqlDocumentSemanticEdit::SetProjectionAlias {
            node_id: item.id,
            expected_kind: SqlDocumentNodeKind::SelectItem,
            alias: Some(SqlDocumentIdentifier {
                value: "gross".into(),
                quote_style: None,
            }),
        };
        let renamed = apply_sql_document_semantic_edit_transaction(
            &result.document.source,
            &semantic_transaction(&result.document.source, vec![rename]),
        )
        .unwrap();
        assert_eq!(
            renamed.document.source,
            "SELECT amount AS gross /* keep */ FROM \"G\" -- tail"
        );
        let edited = parse_sql_document(&renamed.document.source);
        let item = edited
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .unwrap();
        let remove = SqlDocumentSemanticEdit::SetProjectionAlias {
            node_id: item.id,
            expected_kind: SqlDocumentNodeKind::SelectItem,
            alias: None,
        };
        let removed = apply_sql_document_semantic_edit_transaction(
            &renamed.document.source,
            &semantic_transaction(&renamed.document.source, vec![remove]),
        )
        .unwrap();
        assert_eq!(removed.document.source, source);
        let undone =
            apply_sql_document_edit_transaction(&result.document.source, &result.undo).unwrap();
        assert_eq!(undone.document.source, source);
    }

    #[test]
    fn semantic_binary_expression_uses_same_query_nodes_and_supports_undo() {
        let source = "SELECT field_100, 'x' FROM \"G\"";
        let document = parse_sql_document(source);
        let mut items = document
            .nodes
            .iter()
            .filter(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .collect::<Vec<_>>();
        items.sort_by_key(|node| node.range.map(|range| range.start));
        let left = document
            .nodes
            .iter()
            .find(|node| {
                node.parent == Some(items[0].id)
                    && node.kind == SqlDocumentNodeKind::IdentifierExpression
            })
            .expect("the first projection exposes a typed identifier");
        let right = document
            .nodes
            .iter()
            .find(|node| {
                matches!(
                    &node.semantic,
                    Some(SqlDocumentNodeData::Literal { raw: Some(raw), .. }) if raw == "'x'"
                )
            })
            .expect("the second projection exposes its typed string literal");
        let edited = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::ComposeBinaryExpression {
                    node_id: items[0].id,
                    expected_kind: SqlDocumentNodeKind::SelectItem,
                    left_expression_node_id: left.id,
                    left_expression_expected_kind: left.kind,
                    operator: SqlDocumentBinaryOperator::Concat,
                    right_expression_node_id: right.id,
                    right_expression_expected_kind: right.kind,
                }],
            ),
        )
        .expect("typed expressions from one query can build a projection expression");
        assert_eq!(
            edited.document.source,
            "SELECT (field_100\n) || ('x'\n), 'x' FROM \"G\""
        );
        let restored = apply_sql_document_edit_transaction(&edited.document.source, &edited.undo)
            .expect("the binary expression edit has an exact inverse");
        assert_eq!(restored.document.source, source);

        let nested_source = "SELECT 1, (SELECT 2)";
        let nested_document = parse_sql_document(nested_source);
        let nested_items = nested_document
            .nodes
            .iter()
            .filter(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .collect::<Vec<_>>();
        let outer_item = nested_items
            .iter()
            .min_by_key(|node| node.range.map(|range| range.start))
            .expect("outer projection exists");
        let outer_value = nested_document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::Value
                    && nested_document.source_slice(node.range.unwrap()) == Some("1")
            })
            .expect("outer literal exists");
        let nested_value = nested_document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::Value
                    && nested_document.source_slice(node.range.unwrap()) == Some("2")
            })
            .expect("nested literal exists");
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                nested_source,
                &semantic_transaction(
                    nested_source,
                    vec![SqlDocumentSemanticEdit::ComposeBinaryExpression {
                        node_id: outer_item.id,
                        expected_kind: SqlDocumentNodeKind::SelectItem,
                        left_expression_node_id: outer_value.id,
                        left_expression_expected_kind: outer_value.kind,
                        operator: SqlDocumentBinaryOperator::Add,
                        right_expression_node_id: nested_value.id,
                        right_expression_expected_kind: nested_value.kind,
                    }],
                ),
            ),
            Err(SqlDocumentEditError::InvalidSemanticReplacement),
            "expression nodes from different query scopes must not be combined"
        );
    }

    #[test]
    fn semantic_binary_expression_edits_any_expression_in_the_same_query_scope() {
        let source = "SELECT 1 WHERE 1 = 2";
        let document = parse_sql_document(source);
        let literals = document
            .nodes
            .iter()
            .filter(|node| node.kind == SqlDocumentNodeKind::Value)
            .collect::<Vec<_>>();
        let projection_value = literals
            .iter()
            .find(|node| document.source_slice(node.range.unwrap()) == Some("1"))
            .expect("the projection literal is typed");
        let filter_left = literals
            .iter()
            .filter(|node| document.source_slice(node.range.unwrap()) == Some("1"))
            .find(|node| node.range.unwrap().start > source.find("WHERE").unwrap())
            .expect("the WHERE literal is typed");
        let filter_right = literals
            .iter()
            .find(|node| document.source_slice(node.range.unwrap()) == Some("2"))
            .expect("the right WHERE literal is typed");
        let edited = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::ComposeBinaryExpression {
                    node_id: filter_left.id,
                    expected_kind: filter_left.kind,
                    left_expression_node_id: projection_value.id,
                    left_expression_expected_kind: projection_value.kind,
                    operator: SqlDocumentBinaryOperator::Add,
                    right_expression_node_id: filter_right.id,
                    right_expression_expected_kind: filter_right.kind,
                }],
            ),
        )
        .expect("typed expressions can compose outside SELECT projections");

        assert_eq!(edited.document.source, "SELECT 1 WHERE (1\n) + (2\n) = 2");
        let restored = apply_sql_document_edit_transaction(&edited.document.source, &edited.undo)
            .expect("the generic binary edit has an exact inverse");
        assert_eq!(restored.document.source, source);
    }

    #[test]
    fn semantic_expression_nodes_can_copy_same_query_nodes_and_support_undo() {
        let source = "SELECT field_100 + 1, field_100 FROM \"G\"";
        let document = parse_sql_document(source);
        let target = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::BinaryExpression
                    && node.range.and_then(|range| document.source_slice(range))
                        == Some("field_100 + 1")
            })
            .expect("the first projection exposes a binary expression");
        let source_expression = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::IdentifierExpression
                    && node.range.is_some_and(|range| {
                        range.start > target.range.expect("target has a source range").end
                    })
            })
            .expect("the second projection exposes a same-query identifier");
        let edited = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::ReplaceExpressionFromNode {
                    node_id: target.id,
                    expected_kind: SqlDocumentNodeKind::BinaryExpression,
                    expression_node_id: source_expression.id,
                    expression_expected_kind: SqlDocumentNodeKind::IdentifierExpression,
                }],
            ),
        )
        .expect("an expression can be replaced by another typed expression in its query");
        assert_eq!(
            edited.document.source,
            "SELECT field_100, field_100 FROM \"G\""
        );
        let restored = apply_sql_document_edit_transaction(&edited.document.source, &edited.undo)
            .expect("the expression replacement has an exact inverse");
        assert_eq!(restored.document.source, source);
    }

    #[test]
    fn projection_reordering_preserves_positional_sort_and_group_bindings() {
        let source = "-- 保持\nSELECT amount, day, category FROM \"G\" GROUP BY 1, 2, 3 ORDER BY 2 DESC, (1) ASC LIMIT 10";
        let document = parse_sql_document(source);
        let id = semantic_node_id(&document, SqlDocumentNodeKind::SelectItem, "amount");
        let edited = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::MoveProjectionItem {
                    node_id: id,
                    expected_kind: SqlDocumentNodeKind::SelectItem,
                    to_index: 2,
                }],
            ),
        )
        .expect("moving a field preserves all positional references");
        assert_eq!(edited.document.source, "-- 保持\nSELECT day, category, amount FROM \"G\" GROUP BY 3, 1, 2 ORDER BY 1 DESC, (1) ASC LIMIT 10");
        let restored =
            apply_sql_document_edit_transaction(&edited.document.source, &edited.undo).unwrap();
        assert_eq!(restored.document.source, source);
    }

    #[test]
    fn projection_removal_preserves_shifted_positions_and_rejects_removed_references() {
        let source = "SELECT amount, day, category FROM \"G\" GROUP BY 2, 3 ORDER BY 3 DESC";
        let document = parse_sql_document(source);
        let id = semantic_node_id(&document, SqlDocumentNodeKind::SelectItem, "amount");
        let edited = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::RemoveProjectionItem {
                    node_id: id,
                    expected_kind: SqlDocumentNodeKind::SelectItem,
                }],
            ),
        )
        .unwrap();
        assert!(edited
            .document
            .source
            .ends_with("GROUP BY 1, 2 ORDER BY 2 DESC"));
        assert_eq!(
            apply_sql_document_edit_transaction(&edited.document.source, &edited.undo)
                .unwrap()
                .document
                .source,
            source
        );
        let referenced = semantic_node_id(&document, SqlDocumentNodeKind::SelectItem, "day");
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                source,
                &semantic_transaction(
                    source,
                    vec![SqlDocumentSemanticEdit::RemoveProjectionItem {
                        node_id: referenced,
                        expected_kind: SqlDocumentNodeKind::SelectItem
                    }]
                )
            ),
            Err(SqlDocumentEditError::ProjectionPositionConflict)
        );

        let alias_sql =
            "SELECT 'fixed' AS field_100, field_100 AS actual FROM tasks ORDER BY field_100";
        let alias_document = parse_sql_document(alias_sql);
        let alias_item = alias_document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .unwrap();
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                alias_sql,
                &semantic_transaction(
                    alias_sql,
                    vec![SqlDocumentSemanticEdit::RemoveProjectionItem {
                        node_id: alias_item.id,
                        expected_kind: SqlDocumentNodeKind::SelectItem,
                    }],
                ),
            ),
            Err(SqlDocumentEditError::ProjectionAliasConflict)
        );
    }

    #[test]
    fn projection_position_edits_do_not_touch_nested_query_positions_or_guess_wildcard_widths() {
        let source = "SELECT amount, (SELECT 2 ORDER BY 1) AS nested FROM \"G\" ORDER BY 1";
        let document = parse_sql_document(source);
        let id = semantic_node_id(&document, SqlDocumentNodeKind::SelectItem, "amount");
        let edited = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::MoveProjectionItem {
                    node_id: id,
                    expected_kind: SqlDocumentNodeKind::SelectItem,
                    to_index: 1,
                }],
            ),
        )
        .unwrap();
        assert_eq!(
            edited.document.source,
            "SELECT (SELECT 2 ORDER BY 1) AS nested, amount FROM \"G\" ORDER BY 2"
        );
        for source in [
            "SELECT *, amount FROM \"G\" ORDER BY 1",
            "SELECT amount, day FROM \"G\" UNION SELECT other_amount, day FROM \"H\" ORDER BY 1",
        ] {
            let document = parse_sql_document(source);
            let id = semantic_node_id(&document, SqlDocumentNodeKind::SelectItem, "amount");
            assert_eq!(
                apply_sql_document_semantic_edit_transaction(
                    source,
                    &semantic_transaction(
                        source,
                        vec![SqlDocumentSemanticEdit::MoveProjectionItem {
                            node_id: id,
                            expected_kind: SqlDocumentNodeKind::SelectItem,
                            to_index: 1
                        }]
                    )
                ),
                Err(SqlDocumentEditError::ProjectionPositionConflict)
            );
        }
    }

    #[test]
    fn semantic_projection_items_can_be_removed_moved_and_undone() {
        let source = "SELECT a, b AS second, c FROM \"G\"";
        let document = parse_sql_document(source);
        let mut items = document
            .nodes
            .iter()
            .filter(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .collect::<Vec<_>>();
        items.sort_by_key(|node| node.range.map(|range| range.start));

        let moved = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::MoveProjectionItem {
                    node_id: items[0].id,
                    expected_kind: SqlDocumentNodeKind::SelectItem,
                    to_index: 2,
                }],
            ),
        )
        .expect("a projection item can move to another ordinal");
        assert_eq!(moved.document.source, "SELECT b AS second, c, a FROM \"G\"");
        let restored = apply_sql_document_edit_transaction(&moved.document.source, &moved.undo)
            .expect("the reordered projection has an exact inverse");
        assert_eq!(restored.document.source, source);

        let current_items = moved
            .document
            .nodes
            .iter()
            .filter(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .collect::<Vec<_>>();
        let removed = apply_sql_document_semantic_edit_transaction(
            &moved.document.source,
            &semantic_transaction(
                &moved.document.source,
                vec![SqlDocumentSemanticEdit::RemoveProjectionItem {
                    node_id: current_items[1].id,
                    expected_kind: SqlDocumentNodeKind::SelectItem,
                }],
            ),
        )
        .expect("a selected projection item can be removed");
        assert_eq!(removed.document.source, "SELECT b AS second, a FROM \"G\"");
        let restored = apply_sql_document_edit_transaction(&removed.document.source, &removed.undo)
            .expect("the removed projection has an exact inverse");
        assert_eq!(restored.document.source, moved.document.source);
    }

    #[test]
    fn semantic_projection_item_edits_preserve_or_reject_ambiguous_comments() {
        let source = "SELECT a /* keep */, b, c FROM \"G\"";
        let document = parse_sql_document(source);
        let mut items = document
            .nodes
            .iter()
            .filter(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .collect::<Vec<_>>();
        items.sort_by_key(|node| node.range.map(|range| range.start));
        let removed = apply_sql_document_semantic_edit_transaction(
            source,
            &semantic_transaction(
                source,
                vec![SqlDocumentSemanticEdit::RemoveProjectionItem {
                    node_id: items[1].id,
                    expected_kind: SqlDocumentNodeKind::SelectItem,
                }],
            ),
        )
        .expect("removal outside the comment preserves the comment");
        assert_eq!(removed.document.source, "SELECT a /* keep */, c FROM \"G\"");

        let move_with_comment = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::MoveProjectionItem {
                node_id: items[2].id,
                expected_kind: SqlDocumentNodeKind::SelectItem,
                to_index: 0,
            }],
        };
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(source, &move_with_comment),
            Err(SqlDocumentEditError::NodeRangeUnavailable)
        );

        let single_source = "SELECT a FROM \"G\"";
        let single = parse_sql_document(single_source);
        let only_item = single
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .expect("the single projection is modeled");
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                single_source,
                &semantic_transaction(
                    single_source,
                    vec![SqlDocumentSemanticEdit::RemoveProjectionItem {
                        node_id: only_item.id,
                        expected_kind: SqlDocumentNodeKind::SelectItem,
                    }],
                ),
            ),
            Err(SqlDocumentEditError::InvalidSemanticReplacement)
        );
    }

    #[test]
    fn semantic_predicate_negation_rejects_unknown_predicate_forms() {
        let source = "SELECT x FROM \"G\" WHERE x = 1";
        let document = parse_sql_document(source);
        let binary = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::BinaryExpression)
            .expect("the parser exposes the equality expression");
        let edited = apply_sql_document_semantic_edit_transaction(
            source,
            &SqlDocumentSemanticEditTransaction {
                contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                expected_source: source.to_owned(),
                edits: vec![SqlDocumentSemanticEdit::ReplacePredicateNegation {
                    node_id: binary.id,
                    expected_kind: SqlDocumentNodeKind::PredicateExpression,
                    negated: true,
                }],
            },
        );
        assert_eq!(edited, Err(SqlDocumentEditError::NodeKindMismatch));
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
    fn semantic_relation_edits_author_multiple_join_kinds_and_undo_losslessly() {
        let source = "-- report\nSELECT g.id FROM \"G\" AS g WHERE g.id = 1 /* keep */";
        let document = parse_sql_document(source);
        let relation_tree = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::RelationTree)
            .expect("the Form relation has a relation tree");
        let on_expression = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::BinaryExpression
                    && node.range.and_then(|range| document.source_slice(range)) == Some("g.id = 1")
            })
            .expect("the same-query predicate can seed an ON clause");

        let first = apply_sql_document_semantic_edit_transaction(
            source,
            &SqlDocumentSemanticEditTransaction {
                contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                expected_source: source.to_owned(),
                edits: vec![SqlDocumentSemanticEdit::AddJoin {
                    node_id: relation_tree.id,
                    expected_kind: SqlDocumentNodeKind::RelationTree,
                    join: SqlDocumentJoin {
                        operator: SqlDocumentJoinOperator::Left,
                        relation: SqlDocumentRelationFactor::Table {
                            name: vec![SqlDocumentIdentifier {
                                value: "H".to_owned(),
                                quote_style: Some('"'),
                            }],
                            alias: Some(SqlDocumentIdentifier {
                                value: "h".to_owned(),
                                quote_style: None,
                            }),
                        },
                        constraint: SqlDocumentJoinConstraint::On {
                            expression_node_id: on_expression.id,
                            expression_expected_kind: SqlDocumentNodeKind::BinaryExpression,
                        },
                    },
                }],
            },
        )
        .expect("a typed second Form relation can be joined");
        assert_eq!(
            first.document.source,
            "-- report\nSELECT g.id FROM \"G\" AS g LEFT JOIN \"H\" AS h ON g.id = 1 WHERE g.id = 1 /* keep */"
        );
        assert_eq!(first.document.status, SqlDocumentStatus::Parsed);
        assert!(first.document.source.starts_with("-- report\n"));
        assert!(first.document.source.ends_with("/* keep */"));
        assert_eq!(
            apply_sql_document_edit_transaction(&first.document.source, &first.undo)
                .expect("join addition has a guarded inverse")
                .document
                .source,
            source
        );

        let mut current_source = first.document.source;
        for join in [
            SqlDocumentJoin {
                operator: SqlDocumentJoinOperator::FullOuter,
                relation: SqlDocumentRelationFactor::Table {
                    name: vec![SqlDocumentIdentifier {
                        value: "I".to_owned(),
                        quote_style: Some('"'),
                    }],
                    alias: None,
                },
                constraint: SqlDocumentJoinConstraint::Using {
                    columns: vec![SqlDocumentIdentifier {
                        value: "id".to_owned(),
                        quote_style: Some('"'),
                    }],
                },
            },
            SqlDocumentJoin {
                operator: SqlDocumentJoinOperator::NaturalLeft,
                relation: SqlDocumentRelationFactor::Table {
                    name: vec![SqlDocumentIdentifier {
                        value: "J".to_owned(),
                        quote_style: Some('"'),
                    }],
                    alias: None,
                },
                constraint: SqlDocumentJoinConstraint::None,
            },
            SqlDocumentJoin {
                operator: SqlDocumentJoinOperator::Cross,
                relation: SqlDocumentRelationFactor::Table {
                    name: vec![SqlDocumentIdentifier {
                        value: "K".to_owned(),
                        quote_style: Some('"'),
                    }],
                    alias: None,
                },
                constraint: SqlDocumentJoinConstraint::None,
            },
        ] {
            let document = parse_sql_document(&current_source);
            let relation_tree = document
                .nodes
                .iter()
                .find(|node| node.kind == SqlDocumentNodeKind::RelationTree)
                .expect("the relation tree remains selectable");
            let edited = apply_sql_document_semantic_edit_transaction(
                &current_source,
                &SqlDocumentSemanticEditTransaction {
                    contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                    expected_source: current_source.clone(),
                    edits: vec![SqlDocumentSemanticEdit::AddJoin {
                        node_id: relation_tree.id,
                        expected_kind: SqlDocumentNodeKind::RelationTree,
                        join: join.clone(),
                    }],
                },
            )
            .unwrap_or_else(|error| {
                panic!("typed JOIN variant {join:?} failed for {current_source}: {error:?}")
            });
            assert_eq!(edited.document.status, SqlDocumentStatus::Parsed);
            current_source = edited.document.source;
        }
        assert!(current_source.contains("FULL OUTER JOIN \"I\" USING (\"id\")"));
        assert!(current_source.contains("NATURAL LEFT JOIN \"J\""));
        assert!(current_source.contains("CROSS JOIN \"K\""));
    }

    #[test]
    fn semantic_relation_edits_replace_table_and_join_subtrees_safely() {
        let source = "SELECT (SELECT 1) FROM \"G\" CROSS JOIN \"H\"";
        let document = parse_sql_document(source);
        let table_factor = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::TableFactor
                    && node.range.and_then(|range| document.source_slice(range)) == Some("\"G\"")
            })
            .expect("the selected Form factor exists");
        let derived_query = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::Query
                    && node.range.and_then(|range| document.source_slice(range)) == Some("SELECT 1")
            })
            .expect("the parsed nested query can seed a derived relation");
        let replaced = apply_sql_document_semantic_edit_transaction(
            source,
            &SqlDocumentSemanticEditTransaction {
                contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                expected_source: source.to_owned(),
                edits: vec![SqlDocumentSemanticEdit::ReplaceTableFactor {
                    node_id: table_factor.id,
                    expected_kind: SqlDocumentNodeKind::TableFactor,
                    factor: SqlDocumentRelationFactor::Derived {
                        query_node_id: derived_query.id,
                        alias: SqlDocumentIdentifier {
                            value: "d".to_owned(),
                            quote_style: None,
                        },
                    },
                }],
            },
        )
        .expect("a parsed query can be inserted as a derived factor");
        assert_eq!(
            replaced.document.source,
            "SELECT (SELECT 1) FROM (SELECT 1) AS d CROSS JOIN \"H\""
        );
        assert_eq!(replaced.document.status, SqlDocumentStatus::Parsed);
        assert_eq!(
            apply_sql_document_edit_transaction(&replaced.document.source, &replaced.undo)
                .expect("the factor replacement has a guarded inverse")
                .document
                .source,
            source
        );

        let join_node = replaced
            .document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Join)
            .expect("the remaining CROSS JOIN is parsed as a join node");
        let join_replaced = apply_sql_document_semantic_edit_transaction(
            &replaced.document.source,
            &SqlDocumentSemanticEditTransaction {
                contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                expected_source: replaced.document.source.clone(),
                edits: vec![SqlDocumentSemanticEdit::ReplaceJoin {
                    node_id: join_node.id,
                    expected_kind: SqlDocumentNodeKind::Join,
                    join: SqlDocumentJoin {
                        operator: SqlDocumentJoinOperator::Natural,
                        relation: SqlDocumentRelationFactor::Table {
                            name: vec![SqlDocumentIdentifier {
                                value: "I".to_owned(),
                                quote_style: Some('"'),
                            }],
                            alias: None,
                        },
                        constraint: SqlDocumentJoinConstraint::None,
                    },
                }],
            },
        )
        .expect("a JOIN subtree can be replaced by a NATURAL relation");
        assert_eq!(
            join_replaced.document.source,
            "SELECT (SELECT 1) FROM (SELECT 1) AS d NATURAL JOIN \"I\""
        );
        assert_eq!(join_replaced.document.status, SqlDocumentStatus::Parsed);
    }

    #[test]
    fn semantic_relation_edits_fail_closed_on_comments_inside_replaced_subtrees() {
        let factor_source = "SELECT 1 FROM (SELECT 1 /* keep */) AS d";
        let factor_document = parse_sql_document(factor_source);
        let factor = factor_document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::TableFactor
                    && node
                        .range
                        .and_then(|range| factor_document.source_slice(range))
                        .is_some_and(|source| source.starts_with('('))
            })
            .expect("derived factor exists");
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                factor_source,
                &SqlDocumentSemanticEditTransaction {
                    contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                    expected_source: factor_source.to_owned(),
                    edits: vec![SqlDocumentSemanticEdit::ReplaceTableFactor {
                        node_id: factor.id,
                        expected_kind: SqlDocumentNodeKind::TableFactor,
                        factor: SqlDocumentRelationFactor::Table {
                            name: vec![SqlDocumentIdentifier {
                                value: "G".to_owned(),
                                quote_style: Some('"'),
                            }],
                            alias: None,
                        },
                    }],
                }
            ),
            Err(SqlDocumentEditError::NodeRangeUnavailable)
        );
        let relation_tree = factor_document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::RelationTree)
            .expect("derived factor relation tree exists");
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                factor_source,
                &SqlDocumentSemanticEditTransaction {
                    contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                    expected_source: factor_source.to_owned(),
                    edits: vec![SqlDocumentSemanticEdit::AddJoin {
                        node_id: relation_tree.id,
                        expected_kind: SqlDocumentNodeKind::RelationTree,
                        join: SqlDocumentJoin {
                            operator: SqlDocumentJoinOperator::Cross,
                            relation: SqlDocumentRelationFactor::ExistingTableFactor {
                                node_id: factor.id,
                            },
                            constraint: SqlDocumentJoinConstraint::None,
                        },
                    }],
                }
            ),
            Err(SqlDocumentEditError::NodeRangeUnavailable)
        );

        let join_source = "SELECT 1 FROM \"G\" CROSS /* keep */ JOIN \"H\"";
        let join_document = parse_sql_document(join_source);
        let join = join_document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Join)
            .expect("join exists");
        assert_eq!(
            apply_sql_document_semantic_edit_transaction(
                join_source,
                &SqlDocumentSemanticEditTransaction {
                    contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                    expected_source: join_source.to_owned(),
                    edits: vec![SqlDocumentSemanticEdit::ReplaceJoin {
                        node_id: join.id,
                        expected_kind: SqlDocumentNodeKind::Join,
                        join: SqlDocumentJoin {
                            operator: SqlDocumentJoinOperator::Cross,
                            relation: SqlDocumentRelationFactor::Table {
                                name: vec![SqlDocumentIdentifier {
                                    value: "I".to_owned(),
                                    quote_style: Some('"'),
                                }],
                                alias: None,
                            },
                            constraint: SqlDocumentJoinConstraint::None,
                        },
                    }],
                }
            ),
            Err(SqlDocumentEditError::NodeRangeUnavailable)
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

    fn canonical_query(source: &str) -> String {
        let statements = Parser::parse_sql_with_comments(&GenericDialect {}, source)
            .expect("query should parse")
            .0;
        match statements.as_slice() {
            [Statement::Query(query)] => query.to_string(),
            _ => panic!("expected one query"),
        }
    }
}
