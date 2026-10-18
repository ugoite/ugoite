//! Lossless source document and parser view for the SQL Workspace.
//!
//! This module records SQL syntax and source locations. It does not authorize,
//! type-check, plan, or execute a query. The Rust SQL executor remains the
//! authority for the read-only SQL policy and the query planner remains the
//! authority for expression types and result schemas.

use core::ops::ControlFlow;
use serde::{Deserialize, Serialize};
use sqlparser::ast::{
    Expr, Query, Select, Spanned, Statement, TableFactor, ValueWithSpan, Visit, Visitor,
};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::{Parser, ParserError};
use sqlparser::tokenizer::{Location, Span, Token, Tokenizer, Whitespace};
use std::convert::Infallible;

/// Version of the Rust SQL Document view returned to adapters.
pub const SQL_DOCUMENT_CONTRACT_VERSION: u32 = 1;

/// Bound on source SQL accepted by the editor parser.
pub const MAX_SQL_DOCUMENT_BYTES: usize = 256 * 1024;

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
    /// Parser-provided source range when the parser exposes a usable span.
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
}

/// Stable error category for a SQL document diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlDocumentDiagnosticCode {
    /// Source exceeded the bounded editor input size.
    SqlTooLarge,
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
        Ok(tokens) => document.tokens = tokens,
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
                document.nodes = collect_nodes(&statements, source, &source_map);
            }
            [_] => {
                document.status = SqlDocumentStatus::SelectRequired;
                document.nodes = collect_nodes(&statements, source, &source_map);
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
                document.nodes = collect_nodes(&statements, source, &source_map);
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
) -> Vec<SqlDocumentNode> {
    let mut collector = NodeCollector {
        source,
        source_map,
        nodes: Vec::new(),
        parents: Vec::new(),
    };
    for statement in statements {
        let _ = statement.visit(&mut collector);
    }
    collector.nodes
}

struct NodeCollector<'a> {
    source: &'a str,
    source_map: &'a SqlSourceMap,
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
