//! Lossless source document and parser view for the SQL Workspace.
//!
//! This module records SQL syntax and source locations. It does not authorize,
//! type-check, plan, or execute a query. The Rust SQL executor remains the
//! authority for the read-only SQL policy and the query planner remains the
//! authority for expression types and result schemas.

use core::ops::ControlFlow;
use dissimilar::Chunk;
use serde::{Deserialize, Serialize};
use sqlparser::ast::{
    Distinct, Expr, GroupByExpr, Join, LimitClause, OffsetRows, Query, Select, SelectItem, SetExpr,
    SetOperator, SetQuantifier, Spanned, Statement, TableFactor, TableWithJoins, ValueWithSpan,
    Visit, Visitor,
};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::{Parser, ParserError};
use sqlparser::tokenizer::{Location, Span, Token, Tokenizer, Whitespace};
use std::convert::Infallible;

/// Version of the Rust SQL Document view returned to adapters.
pub const SQL_DOCUMENT_CONTRACT_VERSION: u32 = 1;

/// Bound on source SQL accepted by the editor parser.
pub const MAX_SQL_DOCUMENT_BYTES: usize = 256 * 1024;

/// Maximum number of disjoint edits accepted by one atomic document update.
pub const MAX_SQL_DOCUMENT_EDITS: usize = 256;
/// Bound on syntax tokens processed into an editable SQL tree.
pub const MAX_SQL_DOCUMENT_COMPLEXITY_TOKENS: usize = 65_536;

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
    /// A `WITH` clause and its common table expression definitions.
    WithClause,
    /// One common table expression definition.
    CommonTableExpression,
    /// A set-operation tree such as `UNION ALL`.
    SetOperation,
    /// One left or right query branch of a set operation.
    SetOperationBranch,
    /// The set operator and its optional quantifier.
    SetOperationOperator,
    /// A `SELECT` node.
    Select,
    /// One item in a `SELECT` projection, including expressions and wildcards.
    SelectItem,
    /// A named or derived relation in a `FROM` tree.
    Relation,
    /// A table factor such as a named table, join, or derived relation.
    TableFactor,
    /// The `FROM` clause and its relation/join tree.
    FromClause,
    /// One join and its joined relation and constraint.
    Join,
    /// The `WHERE` clause.
    WhereClause,
    /// The `GROUP BY` clause.
    GroupByClause,
    /// The `HAVING` clause.
    HavingClause,
    /// A `SELECT DISTINCT` quantifier, including an optional `ON` expression list.
    DistinctClause,
    /// The `ORDER BY` clause.
    OrderByClause,
    /// A row limit clause, including its `LIMIT` and/or `OFFSET` keywords.
    RowLimitClause,
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

/// Result of applying an edit transaction, including a guarded inverse transaction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlDocumentEditResult {
    /// Reparsed document, including diagnostics for incomplete or invalid drafts.
    pub document: SqlDocument,
    /// Inverse edits guarded by the exact resulting source.
    pub undo: SqlDocumentEditTransaction,
}

const MAX_SQL_SOURCE_DIFF_BYTES: usize = 4096;

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

/// Apply a Rust-generated SQL replacement as bounded, source-preserving edits.
///
/// Small proposals use a text diff to retain unchanged source fragments between
/// edited regions. Larger proposals use a linear-time common-edge edit. The
/// final result always comes from the normal guarded edit transaction and is
/// reparsed by the SQL Document parser; this function does not admit or execute
/// SQL.
pub fn apply_sql_document_replacement_transaction(
    current_source: &str,
    replacement_source: &str,
) -> Result<SqlDocumentEditResult, SqlDocumentEditError> {
    if current_source.len() > MAX_SQL_DOCUMENT_BYTES
        || replacement_source.len() > MAX_SQL_DOCUMENT_BYTES
    {
        return Err(SqlDocumentEditError::SourceTooLarge);
    }

    let edits = if current_source == replacement_source {
        Vec::new()
    } else if current_source.len() <= MAX_SQL_SOURCE_DIFF_BYTES
        && replacement_source.len() <= MAX_SQL_SOURCE_DIFF_BYTES
    {
        source_diff_edits(current_source, replacement_source)
            .filter(|edits| edits.len() <= MAX_SQL_DOCUMENT_EDITS)
            .unwrap_or_else(|| vec![common_edge_replacement(current_source, replacement_source)])
    } else {
        vec![common_edge_replacement(current_source, replacement_source)]
    };

    let transaction = SqlDocumentEditTransaction {
        expected_source: current_source.to_owned(),
        edits,
    };
    let result = apply_sql_document_edit_transaction(current_source, &transaction)?;
    if result.document.source != replacement_source {
        return Err(SqlDocumentEditError::PreimageMismatch);
    }
    Ok(result)
}

fn source_diff_edits(
    current_source: &str,
    replacement_source: &str,
) -> Option<Vec<SqlDocumentEdit>> {
    let mut edits = Vec::new();
    let mut source_offset = 0usize;
    let mut edit_start = None;
    let mut expected_text = String::new();
    let mut replacement = String::new();

    let flush = |edits: &mut Vec<SqlDocumentEdit>,
                 edit_start: &mut Option<usize>,
                 source_offset: usize,
                 expected_text: &mut String,
                 replacement: &mut String| {
        if let Some(start) = edit_start.take() {
            if expected_text != replacement {
                edits.push(SqlDocumentEdit {
                    range: SqlSourceRange {
                        start,
                        end: source_offset,
                    },
                    expected_text: core::mem::take(expected_text),
                    replacement: core::mem::take(replacement),
                });
            } else {
                expected_text.clear();
                replacement.clear();
            }
        }
    };

    for chunk in dissimilar::diff(current_source, replacement_source) {
        match chunk {
            Chunk::Equal(text) => {
                flush(
                    &mut edits,
                    &mut edit_start,
                    source_offset,
                    &mut expected_text,
                    &mut replacement,
                );
                source_offset = source_offset.checked_add(text.len())?;
            }
            Chunk::Delete(text) => {
                edit_start.get_or_insert(source_offset);
                source_offset = source_offset.checked_add(text.len())?;
                expected_text.push_str(text);
            }
            Chunk::Insert(text) => {
                edit_start.get_or_insert(source_offset);
                replacement.push_str(text);
            }
        }
    }
    flush(
        &mut edits,
        &mut edit_start,
        source_offset,
        &mut expected_text,
        &mut replacement,
    );
    (source_offset == current_source.len()).then_some(edits)
}

fn common_edge_replacement(current_source: &str, replacement_source: &str) -> SqlDocumentEdit {
    let prefix_bytes = current_source
        .chars()
        .zip(replacement_source.chars())
        .take_while(|(current, replacement)| current == replacement)
        .map(|(character, _)| character.len_utf8())
        .sum::<usize>();
    let max_suffix_bytes = current_source
        .len()
        .saturating_sub(prefix_bytes)
        .min(replacement_source.len().saturating_sub(prefix_bytes));
    let mut suffix_bytes = 0usize;
    for (current, replacement) in current_source
        .chars()
        .rev()
        .zip(replacement_source.chars().rev())
    {
        let character_bytes = current.len_utf8();
        if current != replacement || suffix_bytes + character_bytes > max_suffix_bytes {
            break;
        }
        suffix_bytes += character_bytes;
    }
    let current_end = current_source.len() - suffix_bytes;
    let replacement_end = replacement_source.len() - suffix_bytes;
    SqlDocumentEdit {
        range: SqlSourceRange {
            start: prefix_bytes,
            end: current_end,
        },
        expected_text: current_source[prefix_bytes..current_end].to_owned(),
        replacement: replacement_source[prefix_bytes..replacement_end].to_owned(),
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

    let mut resolved = Vec::with_capacity(transaction.edits.len());
    for node_edit in &transaction.edits {
        if resolved
            .iter()
            .any(|(node_id, _): &(u32, SqlDocumentEdit)| *node_id == node_edit.node_id)
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
        let expected_text = document
            .source_slice(range)
            .ok_or(SqlDocumentEditError::NodeRangeUnavailable)?;
        resolved.push((
            node.id,
            SqlDocumentEdit {
                range,
                expected_text: expected_text.to_owned(),
                replacement: node_edit.replacement.clone(),
            },
        ));
    }

    let edits = resolved.into_iter().map(|(_, edit)| edit).collect();
    apply_sql_document_edit_transaction(
        current_source,
        &SqlDocumentEditTransaction {
            expected_source: transaction.expected_source.clone(),
            edits,
        },
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

    let parsed = Parser::parse_sql_with_comments(&dialect, source);
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
        Ok((statements, _comments)) => match statements.as_slice() {
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
        token_depths_before: token_depths_before(source, tokens),
        nodes: Vec::new(),
        parents: Vec::new(),
    };
    for statement in statements {
        let _ = statement.visit(&mut collector);
    }
    collector.attach_projection_children();
    collector.rebuild_parent_links()
}

fn token_depths_before(source: &str, tokens: &[SqlDocumentToken]) -> Vec<usize> {
    let mut depths = Vec::with_capacity(tokens.len() + 1);
    let mut depth = 0_usize;
    depths.push(depth);
    for token in tokens {
        if token.kind == SqlDocumentTokenKind::Punctuation {
            match source.get(token.range.start..token.range.end) {
                Some("(" | "[" | "{") => depth = depth.saturating_add(1),
                Some(")" | "]" | "}") => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        depths.push(depth);
    }
    depths
}

struct NodeCollector<'a> {
    source: &'a str,
    source_map: &'a SqlSourceMap,
    tokens: &'a [SqlDocumentToken],
    /// Parenthesis/bracket/brace depth before each token, plus a final entry.
    /// Clause discovery uses this prefix table instead of rescanning all tokens.
    token_depths_before: Vec<usize>,
    nodes: Vec<SqlDocumentNode>,
    parents: Vec<u32>,
}

struct ActiveRangeGroup {
    range: SqlSourceRange,
    last_by_order: std::collections::BTreeMap<u8, u32>,
}

impl NodeCollector<'_> {
    fn push<T: Spanned>(&mut self, kind: SqlDocumentNodeKind, node: &T) {
        let id = self.nodes.len() as u32;
        self.nodes.push(SqlDocumentNode {
            id,
            parent: self.parents.last().copied(),
            kind,
            range: SqlSourceRange::from_span(self.source, self.source_map, node.span()),
        });
        self.parents.push(id);
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
            });
        }
    }

    fn push_range(&mut self, kind: SqlDocumentNodeKind, range: Option<SqlSourceRange>) {
        let id = self.nodes.len() as u32;
        self.nodes.push(SqlDocumentNode {
            id,
            parent: None,
            kind,
            range,
        });
    }

    fn source_range<T: Spanned>(&self, node: &T) -> Option<SqlSourceRange> {
        SqlSourceRange::from_span(self.source, self.source_map, node.span())
    }

    fn token_index_range(&self, range: SqlSourceRange) -> std::ops::Range<usize> {
        let start = self
            .tokens
            .partition_point(|token| token.range.start < range.start);
        let end = self
            .tokens
            .partition_point(|token| token.range.start < range.end);
        start..end
    }

    fn nesting_depth_before(&self, offset: usize) -> usize {
        let consumed = self
            .tokens
            .partition_point(|token| token.range.end <= offset);
        self.token_depths_before[consumed]
    }

    fn top_level_words(&self, scope: SqlSourceRange) -> Vec<(usize, String)> {
        let baseline = self.nesting_depth_before(scope.start);
        let mut words = Vec::new();
        for index in self.token_index_range(scope) {
            let token = &self.tokens[index];
            if token.range.end > scope.end {
                continue;
            }
            if token.kind == SqlDocumentTokenKind::Word
                && self.token_depths_before[index] == baseline
            {
                if let Some(text) = self.source.get(token.range.start..token.range.end) {
                    words.push((index, text.to_ascii_uppercase()));
                }
            }
        }
        words
    }

    fn keyword_sequence_before(
        &self,
        scope: SqlSourceRange,
        before: usize,
        keywords: &[&str],
    ) -> Option<SqlSourceRange> {
        let words = self.top_level_words(scope);
        let sequence = keywords
            .iter()
            .map(|word| word.to_ascii_uppercase())
            .collect::<Vec<_>>();
        let found = words.windows(sequence.len()).rev().find(|window| {
            let Some((last_index, _)) = window.last() else {
                return false;
            };
            let Some(last) = self.tokens.get(*last_index) else {
                return false;
            };
            last.range.end <= before
                && window
                    .iter()
                    .zip(&sequence)
                    .all(|((_, actual), expected)| actual == expected)
        })?;
        let first = self.tokens.get(found.first()?.0)?;
        let last = self.tokens.get(found.last()?.0)?;
        Some(SqlSourceRange {
            start: first.range.start,
            end: last.range.end,
        })
    }

    fn keyword_sequence_after(
        &self,
        scope: SqlSourceRange,
        after: usize,
        keywords: &[&str],
    ) -> Option<SqlSourceRange> {
        let words = self.top_level_words(scope);
        let sequence = keywords
            .iter()
            .map(|word| word.to_ascii_uppercase())
            .collect::<Vec<_>>();
        let found = words.windows(sequence.len()).find(|window| {
            let Some((first_index, _)) = window.first() else {
                return false;
            };
            let Some(first) = self.tokens.get(*first_index) else {
                return false;
            };
            first.range.start >= after
                && window
                    .iter()
                    .zip(&sequence)
                    .all(|((_, actual), expected)| actual == expected)
        })?;
        let first = self.tokens.get(found.first()?.0)?;
        let last = self.tokens.get(found.last()?.0)?;
        Some(SqlSourceRange {
            start: first.range.start,
            end: last.range.end,
        })
    }

    fn clause_range_from_start(
        &self,
        scope: SqlSourceRange,
        start: Option<SqlSourceRange>,
        boundaries: &[Option<SqlSourceRange>],
    ) -> Option<SqlSourceRange> {
        let start = start?;
        let end_bound = boundaries
            .iter()
            .flatten()
            .filter(|candidate| candidate.start > start.start)
            .map(|candidate| candidate.start)
            .min()
            .unwrap_or(scope.end);
        let end = self
            .token_index_range(SqlSourceRange {
                start: start.start,
                end: end_bound,
            })
            .map(|index| &self.tokens[index])
            .filter(|token| token.range.end <= end_bound)
            .filter(|token| {
                !matches!(
                    token.kind,
                    SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
                )
            })
            .map(|token| token.range.end)
            .max()?;
        (start.start < end).then_some(SqlSourceRange {
            start: start.start,
            end,
        })
    }

    fn query_scope_range(&self, query: &Query) -> Option<SqlSourceRange> {
        self.source_range(query)
    }

    fn query_lexical_scope(&self, query_range: SqlSourceRange) -> SqlSourceRange {
        let baseline = self.nesting_depth_before(query_range.start);
        let tail = self.token_index_range(SqlSourceRange {
            start: query_range.end,
            end: self.source.len(),
        });
        for index in tail {
            let token = &self.tokens[index];
            if token.kind != SqlDocumentTokenKind::Punctuation {
                continue;
            }
            match self.source.get(token.range.start..token.range.end) {
                Some(";") if self.token_depths_before[index] == baseline => {
                    return SqlSourceRange {
                        start: query_range.start,
                        end: token.range.start,
                    };
                }
                Some(")" | "]" | "}") if self.token_depths_before[index] == baseline => {
                    return SqlSourceRange {
                        start: query_range.start,
                        end: token.range.start,
                    };
                }
                _ => {}
            }
        }
        SqlSourceRange {
            start: query_range.start,
            end: self.source.len(),
        }
    }

    fn add_query_structure_nodes(&mut self, query: &Query) {
        let Some(query_range) = self.query_scope_range(query) else {
            return;
        };

        if let Some(with) = &query.with {
            let with_range = self.source_range(with);
            self.push_range(SqlDocumentNodeKind::WithClause, with_range);
            for cte in &with.cte_tables {
                self.push_range(
                    SqlDocumentNodeKind::CommonTableExpression,
                    self.source_range(cte),
                );
            }
        }

        self.add_set_expression_nodes(&query.body);

        let order_by_start = query.order_by.as_ref().and_then(|order_by| {
            let range = self.source_range(order_by)?;
            self.keyword_sequence_before(query_range, range.start, &["ORDER", "BY"])
        });
        let row_limit_range = self.row_limit_range(query, query_range);
        if query.order_by.is_some() {
            self.push_range(
                SqlDocumentNodeKind::OrderByClause,
                self.clause_range_from_start(query_range, order_by_start, &[row_limit_range]),
            );
        }
        if query.limit_clause.is_some() || query.fetch.is_some() {
            self.push_range(SqlDocumentNodeKind::RowLimitClause, row_limit_range);
        }
    }

    fn add_set_expression_nodes(&mut self, expression: &SetExpr) {
        let SetExpr::SetOperation {
            left,
            op,
            set_quantifier,
            right,
        } = expression
        else {
            return;
        };
        let operation_range = self.source_range(expression);
        self.push_range(SqlDocumentNodeKind::SetOperation, operation_range);
        self.push_range(
            SqlDocumentNodeKind::SetOperationBranch,
            self.source_range(left.as_ref()),
        );
        self.push_range(
            SqlDocumentNodeKind::SetOperationBranch,
            self.source_range(right.as_ref()),
        );
        self.push_range(
            SqlDocumentNodeKind::SetOperationOperator,
            self.set_operation_operator_range(left, *op, *set_quantifier, right),
        );
        self.add_set_expression_nodes(left);
        self.add_set_expression_nodes(right);
    }

    fn set_operation_operator_range(
        &self,
        left: &SetExpr,
        operator: SetOperator,
        quantifier: SetQuantifier,
        right: &SetExpr,
    ) -> Option<SqlSourceRange> {
        let left_range = self.source_range(left)?;
        let right_range = self.source_range(right)?;
        if left_range.end > right_range.start {
            return None;
        }
        let gap = SqlSourceRange {
            start: left_range.end,
            end: right_range.start,
        };
        let operator_word = operator.to_string().to_ascii_uppercase();
        let operator_index = self
            .top_level_words(gap)
            .into_iter()
            .find_map(|(index, word)| (word == operator_word).then_some(index))?;
        let operator_token = self.tokens.get(operator_index)?;
        let mut end = operator_token.range.end;
        if quantifier != SetQuantifier::None {
            let expected = quantifier.to_string().to_ascii_uppercase();
            let mut actual = String::new();
            for (index, word) in self.top_level_words(gap) {
                if index <= operator_index {
                    continue;
                }
                if !actual.is_empty() {
                    actual.push(' ');
                }
                actual.push_str(&word);
                end = self.tokens.get(index)?.range.end;
                if actual == expected {
                    break;
                }
                if !expected.starts_with(&actual) {
                    return None;
                }
            }
            if actual != expected {
                return None;
            }
        }
        Some(SqlSourceRange {
            start: operator_token.range.start,
            end,
        })
    }

    fn add_select_structure_nodes(&mut self, select: &Select) {
        let Some(select_range) = self.source_range(select) else {
            return;
        };
        let from_start = select.from.first().and_then(|relation| {
            let range = self.source_range(relation)?;
            self.keyword_sequence_before(select_range, range.start, &["FROM"])
        });
        let where_start = select.selection.as_ref().and_then(|expr| {
            let range = self.source_range(expr)?;
            self.keyword_sequence_before(select_range, range.start, &["WHERE"])
        });
        let group_by_start = match &select.group_by {
            GroupByExpr::Expressions(expressions, _) if !expressions.is_empty() => {
                self.source_range(&select.group_by).and_then(|range| {
                    self.keyword_sequence_before(select_range, range.start, &["GROUP", "BY"])
                })
            }
            GroupByExpr::All(_) => self.source_range(&select.group_by).and_then(|range| {
                self.keyword_sequence_before(select_range, range.start, &["GROUP", "BY"])
            }),
            _ => None,
        };
        let having_start = select.having.as_ref().and_then(|expr| {
            let range = self.source_range(expr)?;
            self.keyword_sequence_before(select_range, range.start, &["HAVING"])
        });
        let prewhere_start = select.prewhere.as_ref().and_then(|expr| {
            let range = self.source_range(expr)?;
            self.keyword_sequence_before(select_range, range.start, &["PREWHERE"])
        });
        let qualify_start = select.qualify.as_ref().and_then(|expr| {
            let range = self.source_range(expr)?;
            self.keyword_sequence_before(select_range, range.start, &["QUALIFY"])
        });
        if matches!(select.distinct, Some(Distinct::Distinct | Distinct::On(_))) {
            if let Some(first_projection) =
                projection_item_ranges(self.source, self.source_map, self.tokens, select)
                    .and_then(|ranges| ranges.first().copied())
            {
                let distinct_start = self.keyword_sequence_before(
                    select_range,
                    first_projection.start,
                    &["DISTINCT"],
                );
                self.push_range(
                    SqlDocumentNodeKind::DistinctClause,
                    self.clause_range_from_start(
                        select_range,
                        distinct_start,
                        &[Some(first_projection)],
                    ),
                );
            }
        }
        let select_boundaries = [
            where_start,
            group_by_start,
            having_start,
            prewhere_start,
            qualify_start,
        ];
        if !select.from.is_empty() {
            if let Some(range) =
                self.clause_range_from_start(select_range, from_start, &select_boundaries)
            {
                self.push_range(SqlDocumentNodeKind::FromClause, Some(range));
                for relation in &select.from {
                    self.add_table_join_nodes(relation, range);
                }
            } else {
                // The parser's relation span is a safe fallback if a dialect
                // accepts an unusual FROM form without a conventional keyword.
                self.push_range(
                    SqlDocumentNodeKind::FromClause,
                    select
                        .from
                        .first()
                        .and_then(|relation| self.source_range(relation)),
                );
            }
        }
        if select.selection.is_some() {
            self.push_range(
                SqlDocumentNodeKind::WhereClause,
                self.clause_range_from_start(
                    select_range,
                    where_start,
                    &[group_by_start, having_start, qualify_start],
                ),
            );
        }
        let has_group_by = match &select.group_by {
            GroupByExpr::Expressions(expressions, _) => !expressions.is_empty(),
            GroupByExpr::All(_) => true,
        };
        if has_group_by {
            self.push_range(
                SqlDocumentNodeKind::GroupByClause,
                self.clause_range_from_start(
                    select_range,
                    group_by_start,
                    &[having_start, qualify_start],
                ),
            );
        }
        if select.having.is_some() {
            self.push_range(
                SqlDocumentNodeKind::HavingClause,
                self.clause_range_from_start(select_range, having_start, &[qualify_start]),
            );
        }
    }

    fn add_table_join_nodes(&mut self, table: &TableWithJoins, from_range: SqlSourceRange) {
        let mut previous_end = self
            .source_range(&table.relation)
            .map_or(from_range.start, |range| range.end);
        for join in &table.joins {
            if let Some(range) = self.join_range(join, previous_end, from_range) {
                self.push_range(SqlDocumentNodeKind::Join, Some(range));
            }
            previous_end = self
                .source_range(join)
                .map_or(previous_end, |range| range.end);
            self.add_nested_table_join_nodes(&join.relation, from_range);
        }
        self.add_nested_table_join_nodes(&table.relation, from_range);
    }

    fn add_nested_table_join_nodes(&mut self, factor: &TableFactor, from_range: SqlSourceRange) {
        if let TableFactor::NestedJoin {
            table_with_joins, ..
        } = factor
        {
            self.add_table_join_nodes(table_with_joins, from_range);
        }
    }

    fn join_range(
        &self,
        join: &Join,
        lower_bound: usize,
        from_range: SqlSourceRange,
    ) -> Option<SqlSourceRange> {
        let joined_range = self.source_range(&join.relation)?;
        let end = self.source_range(join)?.end.max(joined_range.end);
        let scope = SqlSourceRange {
            start: lower_bound.max(from_range.start),
            end: joined_range.start,
        };
        let words = self.top_level_words(scope);
        let join_index = words
            .iter()
            .rev()
            .find_map(|(index, word)| (word == "JOIN").then_some(*index))?;
        let join_token = self.tokens.get(join_index)?;
        let mut start = join_token.range.start;
        for (index, _) in words
            .iter()
            .rev()
            .filter(|(index, _)| *index < join_index)
            .take_while(|(_, word)| {
                matches!(
                    word.as_str(),
                    "INNER"
                        | "LEFT"
                        | "RIGHT"
                        | "FULL"
                        | "CROSS"
                        | "NATURAL"
                        | "OUTER"
                        | "SEMI"
                        | "ANTI"
                        | "ASOF"
                )
            })
        {
            start = self.tokens.get(*index)?.range.start;
        }
        Some(SqlSourceRange { start, end })
    }

    fn row_limit_range(
        &self,
        query: &Query,
        query_range: SqlSourceRange,
    ) -> Option<SqlSourceRange> {
        let mut parts = Vec::new();
        if let Some(limit) = &query.limit_clause {
            if let Some(range) = self.explicit_limit_range(query_range, limit) {
                parts.push(range);
            }
        }
        if let Some(fetch) = &query.fetch {
            if let Some(range) = self.fetch_range(query, fetch, query_range) {
                parts.push(range);
            }
        }
        let start = parts.iter().map(|range| range.start).min()?;
        let end = parts.iter().map(|range| range.end).max()?;
        Some(SqlSourceRange { start, end })
    }

    fn explicit_limit_range(
        &self,
        query_range: SqlSourceRange,
        limit: &LimitClause,
    ) -> Option<SqlSourceRange> {
        let (limit_anchor, offset_anchor, offset_value_range, offset_rows) = match limit {
            LimitClause::LimitOffset { limit, offset, .. } => (
                limit.as_ref().and_then(|value| self.source_range(value)),
                offset
                    .as_ref()
                    .and_then(|offset| self.source_range(&offset.value)),
                offset
                    .as_ref()
                    .and_then(|offset| self.source_range(&offset.value)),
                offset.as_ref().map(|offset| offset.rows),
            ),
            // MySQL's `LIMIT offset, limit` stores the first expression in
            // the AST's `offset` field, but the source keyword still precedes it.
            LimitClause::OffsetCommaLimit { offset, .. } => {
                (self.source_range(offset), None, None, None)
            }
        };
        let limit_start = limit_anchor
            .and_then(|range| self.keyword_sequence_before(query_range, range.start, &["LIMIT"]));
        let offset_start = offset_anchor
            .and_then(|range| self.keyword_sequence_before(query_range, range.start, &["OFFSET"]));
        let start = [limit_start, offset_start]
            .into_iter()
            .flatten()
            .min_by_key(|range| range.start)?;
        let end = self
            .tokens
            .iter()
            .filter(|token| token.range.start >= start.start && token.range.end <= query_range.end)
            .filter(|token| {
                !matches!(
                    token.kind,
                    SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
                )
            })
            .map(|token| token.range.end)
            .max()?;
        let end = if let (Some(value_range), Some(rows)) = (offset_value_range, offset_rows) {
            if rows == OffsetRows::None {
                end
            } else {
                let expected = match rows {
                    OffsetRows::Row => "ROW",
                    OffsetRows::Rows => "ROWS",
                    OffsetRows::None => unreachable!(),
                };
                self.next_significant_token(
                    value_range.end,
                    self.query_lexical_scope(query_range).end,
                )
                .filter(|token| {
                    self.source.get(token.range.start..token.range.end) == Some(expected)
                })
                .map_or(end, |token| end.max(token.range.end))
            }
        } else {
            end
        };
        (start.start < end).then_some(SqlSourceRange {
            start: start.start,
            end,
        })
    }

    fn fetch_range(
        &self,
        query: &Query,
        fetch: &sqlparser::ast::Fetch,
        query_range: SqlSourceRange,
    ) -> Option<SqlSourceRange> {
        let scope = self.query_lexical_scope(query_range);
        let start = if let Some(quantity) = &fetch.quantity {
            let range = self.source_range(quantity)?;
            self.keyword_sequence_before(scope, range.start, &["FETCH"])?
        } else {
            let after = query
                .order_by
                .as_ref()
                .and_then(|order_by| self.source_range(order_by))
                .or_else(|| self.source_range(query.body.as_ref()))?
                .end;
            self.keyword_sequence_after(scope, after, &["FETCH"])?
        };
        let terminal = if fetch.with_ties { "TIES" } else { "ONLY" };
        let end = self
            .top_level_words(scope)
            .into_iter()
            .rev()
            .find_map(|(index, word)| {
                (word == terminal)
                    .then(|| self.tokens.get(index).map(|token| token.range.end))
                    .flatten()
            })?;
        (start.start < end).then_some(SqlSourceRange {
            start: start.start,
            end,
        })
    }

    fn next_significant_token(&self, offset: usize, end_bound: usize) -> Option<&SqlDocumentToken> {
        self.token_index_range(SqlSourceRange {
            start: offset,
            end: end_bound,
        })
        .map(|index| &self.tokens[index])
        .filter(|token| token.range.end <= end_bound)
        .find(|token| {
            !matches!(
                token.kind,
                SqlDocumentTokenKind::Trivia | SqlDocumentTokenKind::Comment
            )
        })
    }

    fn rebuild_parent_links(mut self) -> Vec<SqlDocumentNode> {
        let original_parents = self
            .nodes
            .iter()
            .map(|node| (node.id, node.parent))
            .collect::<std::collections::BTreeMap<_, _>>();
        self.nodes.sort_by(|left, right| {
            let left_range = left.range.unwrap_or(SqlSourceRange {
                start: usize::MAX,
                end: usize::MAX,
            });
            let right_range = right.range.unwrap_or(SqlSourceRange {
                start: usize::MAX,
                end: usize::MAX,
            });
            left_range
                .start
                .cmp(&right_range.start)
                .then_with(|| right_range.end.cmp(&left_range.end))
                .then_with(|| {
                    node_kind_parent_order(left.kind).cmp(&node_kind_parent_order(right.kind))
                })
                .then_with(|| left.id.cmp(&right.id))
        });

        let old_to_new = self
            .nodes
            .iter()
            .enumerate()
            .map(|(new, node)| (node.id, new as u32))
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut range_stack: Vec<ActiveRangeGroup> = Vec::new();
        for index in 0..self.nodes.len() {
            let Some(child_range) = self.nodes[index].range else {
                let old_parent = original_parents
                    .get(&self.nodes[index].id)
                    .copied()
                    .flatten();
                self.nodes[index].parent =
                    old_parent.and_then(|parent| old_to_new.get(&parent).copied());
                continue;
            };
            while range_stack.last().is_some_and(|group| {
                group.range.start > child_range.start || child_range.end > group.range.end
            }) {
                range_stack.pop();
            }

            let old_parent = original_parents
                .get(&self.nodes[index].id)
                .copied()
                .flatten()
                .and_then(|parent| old_to_new.get(&parent).copied());
            let child_order = node_kind_parent_order(self.nodes[index].kind);
            let original_same_range_parent = old_parent.filter(|candidate| {
                (*candidate as usize) < index
                    && self.nodes[*candidate as usize].range == Some(child_range)
            });
            let parent = if let Some(group) = range_stack.last() {
                if group.range == child_range {
                    original_same_range_parent.or_else(|| {
                        group
                            .last_by_order
                            .range(..child_order)
                            .next_back()
                            .map(|(_, parent)| *parent)
                    })
                } else {
                    group
                        .last_by_order
                        .last_key_value()
                        .map(|(_, parent)| *parent)
                }
            } else {
                None
            };
            self.nodes[index].parent = parent;

            if range_stack
                .last()
                .is_some_and(|group| group.range == child_range)
            {
                if let Some(group) = range_stack.last_mut() {
                    group.last_by_order.insert(child_order, index as u32);
                }
            } else {
                let mut last_by_order = std::collections::BTreeMap::new();
                last_by_order.insert(child_order, index as u32);
                range_stack.push(ActiveRangeGroup {
                    range: child_range,
                    last_by_order,
                });
            }
        }
        for (index, node) in self.nodes.iter_mut().enumerate() {
            node.id = index as u32;
        }
        self.nodes
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

fn node_kind_parent_order(kind: SqlDocumentNodeKind) -> u8 {
    match kind {
        SqlDocumentNodeKind::QueryStatement => 0,
        SqlDocumentNodeKind::SubqueryExpression | SqlDocumentNodeKind::SetOperationBranch => 1,
        SqlDocumentNodeKind::Query => 2,
        SqlDocumentNodeKind::WithClause => 3,
        SqlDocumentNodeKind::CommonTableExpression => 4,
        SqlDocumentNodeKind::SetOperation => 4,
        SqlDocumentNodeKind::SetOperationOperator => 5,
        SqlDocumentNodeKind::Select => 6,
        SqlDocumentNodeKind::SelectItem => 7,
        SqlDocumentNodeKind::FromClause => 8,
        SqlDocumentNodeKind::Join => 9,
        SqlDocumentNodeKind::TableFactor => 10,
        SqlDocumentNodeKind::Relation => 11,
        SqlDocumentNodeKind::WhereClause
        | SqlDocumentNodeKind::GroupByClause
        | SqlDocumentNodeKind::HavingClause
        | SqlDocumentNodeKind::DistinctClause
        | SqlDocumentNodeKind::OrderByClause
        | SqlDocumentNodeKind::RowLimitClause => 12,
        SqlDocumentNodeKind::IdentifierExpression
        | SqlDocumentNodeKind::CompoundIdentifierExpression
        | SqlDocumentNodeKind::LiteralExpression
        | SqlDocumentNodeKind::BinaryExpression
        | SqlDocumentNodeKind::UnaryExpression
        | SqlDocumentNodeKind::FunctionExpression
        | SqlDocumentNodeKind::CaseExpression
        | SqlDocumentNodeKind::CastExpression
        | SqlDocumentNodeKind::PredicateExpression
        | SqlDocumentNodeKind::NestedExpression
        | SqlDocumentNodeKind::OtherExpression => 20,
        SqlDocumentNodeKind::Value => 21,
        SqlDocumentNodeKind::OtherStatement => 0,
    }
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
        self.add_query_structure_nodes(query);
        ControlFlow::Continue(())
    }

    fn post_visit_query(&mut self, _query: &Query) -> ControlFlow<Self::Break> {
        self.pop();
        ControlFlow::Continue(())
    }

    fn pre_visit_select(&mut self, select: &Select) -> ControlFlow<Self::Break> {
        self.push(SqlDocumentNodeKind::Select, select);
        self.push_select_items(select);
        self.add_select_structure_nodes(select);
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
        self.push(SqlDocumentNodeKind::Relation, relation);
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
        self.push(SqlDocumentNodeKind::TableFactor, table_factor);
        ControlFlow::Continue(())
    }

    fn post_visit_table_factor(&mut self, _table_factor: &TableFactor) -> ControlFlow<Self::Break> {
        self.pop();
        ControlFlow::Continue(())
    }

    fn pre_visit_expr(&mut self, expression: &Expr) -> ControlFlow<Self::Break> {
        self.push(expression_kind(expression), expression);
        ControlFlow::Continue(())
    }

    fn post_visit_expr(&mut self, _expression: &Expr) -> ControlFlow<Self::Break> {
        self.pop();
        ControlFlow::Continue(())
    }

    fn pre_visit_value(&mut self, value: &ValueWithSpan) -> ControlFlow<Self::Break> {
        self.push(SqlDocumentNodeKind::Value, value);
        ControlFlow::Continue(())
    }

    fn post_visit_value(&mut self, _value: &ValueWithSpan) -> ControlFlow<Self::Break> {
        self.pop();
        ControlFlow::Continue(())
    }
}

fn expression_kind(expression: &Expr) -> SqlDocumentNodeKind {
    match expression {
        Expr::Identifier(_) => SqlDocumentNodeKind::IdentifierExpression,
        Expr::CompoundIdentifier(_) => SqlDocumentNodeKind::CompoundIdentifierExpression,
        Expr::Value(_) => SqlDocumentNodeKind::LiteralExpression,
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

    #[test]
    fn replacement_transaction_preserves_unchanged_fragments_and_undoes_exactly() {
        let source = "SELECT a /* keep this comment */, b FROM \"G\" WHERE id = 1 -- keep tail";
        let replacement =
            "SELECT total_a /* keep this comment */, total_b FROM \"G\" WHERE id = 2 -- keep tail";

        let result = apply_sql_document_replacement_transaction(source, replacement).unwrap();
        assert_eq!(result.document.source, replacement);
        assert_eq!(result.document.status, SqlDocumentStatus::Parsed);
        assert!(result.undo.edits.len() > 1);
        assert!(result.undo.edits.iter().all(|edit| {
            !edit.expected_text.contains("keep this comment")
                && !edit.expected_text.contains("keep tail")
                && !edit.replacement.contains("keep this comment")
                && !edit.replacement.contains("keep tail")
        }));

        let restored =
            apply_sql_document_edit_transaction(&result.document.source, &result.undo).unwrap();
        assert_eq!(restored.document.source, source);
    }

    #[test]
    fn replacement_transaction_uses_bounded_utf8_safe_edit_for_large_sources() {
        let source = format!("SELECT old_{} FROM \"G\"", "a".repeat(5000));
        let replacement = format!("SELECT new_{} FROM \"G\"", "a".repeat(5000));

        let result = apply_sql_document_replacement_transaction(&source, &replacement).unwrap();
        assert_eq!(result.document.source, replacement);
        assert_eq!(result.undo.edits.len(), 1);
        assert_eq!(result.undo.edits[0].expected_text, "new");
        assert_eq!(result.undo.edits[0].replacement, "old");

        let unicode_source = "SELECT '東京'";
        let unicode_replacement = "SELECT '大阪'";
        let unicode_result =
            apply_sql_document_replacement_transaction(unicode_source, unicode_replacement)
                .unwrap();
        assert_eq!(unicode_result.document.source, unicode_replacement);
        let restored = apply_sql_document_edit_transaction(
            &unicode_result.document.source,
            &unicode_result.undo,
        )
        .unwrap();
        assert_eq!(restored.document.source, unicode_source);
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
        let outer_items = document
            .nodes
            .iter()
            .filter(|node| {
                node.kind == SqlDocumentNodeKind::SelectItem && node.parent == Some(outer_select.id)
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
        let item = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::SelectItem && node.parent == Some(select.id)
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
    fn classifies_nested_cte_set_operation_and_clause_paths_with_source_slices() {
        let source = "WITH first_cte AS (SELECT g.id, g.amount FROM \"G\" AS g LEFT JOIN \"H\" AS h ON g.id = h.id WHERE g.amount > 0), second_cte AS (SELECT id, amount FROM first_cte GROUP BY id, amount HAVING SUM(amount) > 1) SELECT id, amount FROM (SELECT id, amount FROM second_cte WHERE id > 1 UNION ALL SELECT id, amount FROM second_cte WHERE id < 9) AS derived WHERE amount > 2 GROUP BY id, amount HAVING SUM(amount) > 3 ORDER BY id DESC LIMIT 10 OFFSET 2 /* trailing comment */";
        let document = parse_sql_document(source);

        assert_eq!(document.status, SqlDocumentStatus::Parsed);
        let exact = |kind, expected: &str| {
            document
                .nodes
                .iter()
                .find(|node| {
                    node.kind == kind
                        && node.range.and_then(|range| document.source_slice(range))
                            == Some(expected)
                })
                .unwrap_or_else(|| {
                    panic!(
                        "missing {kind:?} node for {expected:?}; actual nodes: {:?}",
                        document
                            .nodes
                            .iter()
                            .map(|node| (
                                node.kind,
                                node.range
                                    .and_then(|range| document.source_slice(range))
                                    .unwrap_or("<no range>")
                            ))
                            .collect::<Vec<_>>()
                    )
                })
        };

        let with_clause = exact(
            SqlDocumentNodeKind::WithClause,
            "WITH first_cte AS (SELECT g.id, g.amount FROM \"G\" AS g LEFT JOIN \"H\" AS h ON g.id = h.id WHERE g.amount > 0), second_cte AS (SELECT id, amount FROM first_cte GROUP BY id, amount HAVING SUM(amount) > 1)",
        );
        let first_cte = exact(
            SqlDocumentNodeKind::CommonTableExpression,
            "first_cte AS (SELECT g.id, g.amount FROM \"G\" AS g LEFT JOIN \"H\" AS h ON g.id = h.id WHERE g.amount > 0)",
        );
        let second_cte = exact(
            SqlDocumentNodeKind::CommonTableExpression,
            "second_cte AS (SELECT id, amount FROM first_cte GROUP BY id, amount HAVING SUM(amount) > 1)",
        );
        assert_eq!(first_cte.parent, Some(with_clause.id));
        assert_eq!(second_cte.parent, Some(with_clause.id));

        let join = exact(
            SqlDocumentNodeKind::Join,
            "LEFT JOIN \"H\" AS h ON g.id = h.id",
        );
        let first_from = exact(
            SqlDocumentNodeKind::FromClause,
            "FROM \"G\" AS g LEFT JOIN \"H\" AS h ON g.id = h.id",
        );
        assert_eq!(join.parent, Some(first_from.id));
        exact(SqlDocumentNodeKind::WhereClause, "WHERE g.amount > 0");
        exact(SqlDocumentNodeKind::GroupByClause, "GROUP BY id, amount");
        exact(SqlDocumentNodeKind::HavingClause, "HAVING SUM(amount) > 1");

        let set_operation = exact(
            SqlDocumentNodeKind::SetOperation,
            "SELECT id, amount FROM second_cte WHERE id > 1 UNION ALL SELECT id, amount FROM second_cte WHERE id < 9",
        );
        let operator = exact(SqlDocumentNodeKind::SetOperationOperator, "UNION ALL");
        assert_eq!(operator.parent, Some(set_operation.id));
        let left_branch = exact(
            SqlDocumentNodeKind::SetOperationBranch,
            "SELECT id, amount FROM second_cte WHERE id > 1",
        );
        let right_branch = exact(
            SqlDocumentNodeKind::SetOperationBranch,
            "SELECT id, amount FROM second_cte WHERE id < 9",
        );
        assert_eq!(left_branch.parent, Some(set_operation.id));
        assert_eq!(right_branch.parent, Some(set_operation.id));
        exact(SqlDocumentNodeKind::WhereClause, "WHERE id > 1");
        exact(SqlDocumentNodeKind::WhereClause, "WHERE id < 9");

        exact(
            SqlDocumentNodeKind::FromClause,
            "FROM (SELECT id, amount FROM second_cte WHERE id > 1 UNION ALL SELECT id, amount FROM second_cte WHERE id < 9) AS derived",
        );
        exact(SqlDocumentNodeKind::WhereClause, "WHERE amount > 2");
        exact(SqlDocumentNodeKind::GroupByClause, "GROUP BY id, amount");
        exact(SqlDocumentNodeKind::HavingClause, "HAVING SUM(amount) > 3");
        exact(SqlDocumentNodeKind::OrderByClause, "ORDER BY id DESC");
        exact(SqlDocumentNodeKind::RowLimitClause, "LIMIT 10 OFFSET 2");
        assert!(document.source.ends_with("/* trailing comment */"));
    }

    #[test]
    fn row_limit_keyword_lookup_ignores_limit_and_offset_aliases_and_trailing_comments() {
        let source =
            "SELECT 1 AS limit, 2 AS offset FROM t ORDER BY limit LIMIT 3 OFFSET 4 /* trailing */";
        let document = parse_sql_document(source);

        assert_eq!(document.status, SqlDocumentStatus::Parsed);
        let order = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::OrderByClause)
            .and_then(|node| node.range.map(|range| document.source_slice(range)))
            .flatten();
        assert_eq!(order, Some("ORDER BY limit"));
        let row_limit = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::RowLimitClause)
            .and_then(|node| node.range.map(|range| document.source_slice(range)))
            .flatten();
        assert_eq!(row_limit, Some("LIMIT 3 OFFSET 4"));

        let no_limit = parse_sql_document("SELECT 1 AS limit, 2 AS offset FROM t /* trailing */");
        assert_eq!(no_limit.status, SqlDocumentStatus::Parsed);
        assert!(!no_limit
            .nodes
            .iter()
            .any(|node| node.kind == SqlDocumentNodeKind::RowLimitClause));

        for (source, expected) in [
            ("SELECT 1 FROM t LIMIT 2, 10 /* trailing */", "LIMIT 2, 10"),
            (
                "SELECT 1 FROM t OFFSET 2 LIMIT 10 /* trailing */",
                "OFFSET 2 LIMIT 10",
            ),
        ] {
            let document = parse_sql_document(source);
            assert_eq!(document.status, SqlDocumentStatus::Parsed, "{source}");
            let row_limit = document
                .nodes
                .iter()
                .find(|node| node.kind == SqlDocumentNodeKind::RowLimitClause)
                .and_then(|node| node.range.map(|range| document.source_slice(range)))
                .flatten();
            assert_eq!(row_limit, Some(expected), "{source}");
        }
    }

    #[test]
    fn classifies_distinct_and_fetch_row_limit_ranges() {
        let distinct = parse_sql_document("SELECT DISTINCT a, b FROM t /* trailing */");
        assert_eq!(distinct.status, SqlDocumentStatus::Parsed);
        let distinct_range = distinct
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::DistinctClause)
            .and_then(|node| node.range.map(|range| distinct.source_slice(range)))
            .flatten();
        assert_eq!(distinct_range, Some("DISTINCT"));

        let fetch = parse_sql_document(
            "SELECT a FROM t ORDER BY a FETCH FIRST 10 ROWS ONLY /* trailing */",
        );
        assert_eq!(fetch.status, SqlDocumentStatus::Parsed);
        let fetch_range = fetch
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::RowLimitClause)
            .and_then(|node| node.range.map(|range| fetch.source_slice(range)))
            .flatten();
        assert_eq!(fetch_range, Some("FETCH FIRST 10 ROWS ONLY"));
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
