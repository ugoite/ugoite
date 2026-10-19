//! Lossless source document and parser view for the SQL Workspace.
//!
//! This module records SQL syntax and source locations. It does not authorize,
//! type-check, plan, or execute a query. The Rust SQL executor remains the
//! authority for the read-only SQL policy and the query planner remains the
//! authority for expression types and result schemas.

use core::ops::ControlFlow;
use serde::{Deserialize, Serialize};
use sqlparser::ast::{
    Distinct, Expr, Query, Select, SelectItem, Spanned, Statement, TableFactor, ValueWithSpan,
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
    /// A `SELECT` node.
    Select,
    /// One item in a `SELECT` projection, including expressions and wildcards.
    SelectItem,
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
        nodes: Vec::new(),
        parents: Vec::new(),
    };
    for statement in statements {
        let _ = statement.visit(&mut collector);
    }
    collector.attach_projection_children();
    collector.nodes
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
