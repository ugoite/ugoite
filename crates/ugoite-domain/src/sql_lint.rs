//! Portable SQL syntax linting that is independent of a Space or query planner.
//!
//! Linting only checks lexical/syntax rules. Relation resolution, column types,
//! current authorization, and execution remain the responsibility of the SQL
//! planner and query executor.

use crate::sql_document::{
    normalize_sql_template, parse_generic_sql_statements, source_range_from_span, SqlSourceRange,
    SqlTemplateNormalizationError, MAX_SQL_DOCUMENT_BYTES,
};
use serde::{Deserialize, Serialize};
use sqlparser::ast::{Query, Select, SelectItem, SetExpr, Spanned, Statement, Visit, Visitor};
use std::convert::Infallible;
use std::ops::ControlFlow;

/// Version of the portable SQL lint diagnostic contract.
pub const SQL_LINT_CONTRACT_VERSION: u32 = 1;

/// Static SQL policy selected by a caller.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlLintProfile {
    /// One read-only SELECT with no Composition-specific projection rule.
    #[default]
    Standard,
    /// Composition's explicit-projection policy.
    Composition,
}

/// Severity for one stable SQL lint diagnostic.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlLintSeverity {
    Error,
}

/// Stable code for a portable SQL lint diagnostic.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlLintDiagnosticCode {
    SqlTooLarge,
    InvalidTemplate,
    InvalidSyntax,
    EmptySql,
    SingleStatementRequired,
    SelectRequired,
    WildcardProjection,
}

/// One source-associated diagnostic returned by the Rust SQL linter.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SqlLintDiagnostic {
    pub code: SqlLintDiagnosticCode,
    pub severity: SqlLintSeverity,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<SqlSourceRange>,
}

/// Portable output for one SQL lint request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SqlLintResponse {
    pub contract_version: u32,
    pub profile: SqlLintProfile,
    pub diagnostics: Vec<SqlLintDiagnostic>,
}

impl SqlLintResponse {
    pub fn is_valid(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

/// Check SQL syntax and the selected static policy without reading Space data.
pub fn lint_sql(source: &str, profile: SqlLintProfile) -> SqlLintResponse {
    let mut response = SqlLintResponse {
        contract_version: SQL_LINT_CONTRACT_VERSION,
        profile,
        diagnostics: Vec::new(),
    };

    if source.len() > MAX_SQL_DOCUMENT_BYTES {
        response.diagnostics.push(diagnostic(
            SqlLintDiagnosticCode::SqlTooLarge,
            "SQL exceeds the supported source length",
            Some(SqlSourceRange {
                start: 0,
                end: source.len(),
            }),
        ));
        return response;
    }

    let normalized = normalize_sql_template(source);
    if let Some(error) = normalized.error {
        let (message, range) = match error {
            SqlTemplateNormalizationError::MissingClosingBraces { range } => {
                ("SQL template variable is missing closing braces", range)
            }
            SqlTemplateNormalizationError::InvalidParameterName { range } => {
                ("SQL template variable name is invalid", range)
            }
        };
        response.diagnostics.push(diagnostic(
            SqlLintDiagnosticCode::InvalidTemplate,
            message,
            Some(range),
        ));
        return response;
    }

    if normalized.sql.trim().is_empty() {
        response.diagnostics.push(diagnostic(
            SqlLintDiagnosticCode::EmptySql,
            "SQL must not be empty",
            Some(SqlSourceRange {
                start: 0,
                end: source.len(),
            }),
        ));
        return response;
    }

    let statements = match parse_generic_sql_statements(&normalized.sql) {
        Ok(statements) if !statements.is_empty() => statements,
        Ok(_) => {
            response.diagnostics.push(diagnostic(
                SqlLintDiagnosticCode::EmptySql,
                "SQL must contain a statement",
                Some(SqlSourceRange {
                    start: 0,
                    end: source.len(),
                }),
            ));
            return response;
        }
        Err(_) => {
            response.diagnostics.push(diagnostic(
                SqlLintDiagnosticCode::InvalidSyntax,
                "SQL syntax is invalid",
                None,
            ));
            return response;
        }
    };

    if statements.len() != 1 {
        response.diagnostics.push(diagnostic(
            SqlLintDiagnosticCode::SingleStatementRequired,
            "SQL must contain one SELECT statement",
            None,
        ));
        return response;
    }
    if !matches!(statements.first(), Some(Statement::Query(_))) {
        response.diagnostics.push(diagnostic(
            SqlLintDiagnosticCode::SelectRequired,
            "SQL must be a SELECT query",
            None,
        ));
        return response;
    }
    if let Some(Statement::Query(query)) = statements.first() {
        if !query_contains_only_select_bodies(query) {
            response.diagnostics.push(diagnostic(
                SqlLintDiagnosticCode::SelectRequired,
                "SQL query bodies must use SELECT",
                None,
            ));
            return response;
        }
    }

    if profile == SqlLintProfile::Composition {
        let mut visitor = ProjectionWildcardVisitor {
            source: &normalized.sql,
            replacements: &normalized.replacements,
            diagnostics: Vec::new(),
        };
        for statement in &statements {
            let _ = statement.visit(&mut visitor);
        }
        response.diagnostics = visitor.diagnostics;
    }

    response
}

fn query_contains_only_select_bodies(query: &Query) -> bool {
    fn set_expr_is_select_only(expression: &SetExpr) -> bool {
        match expression {
            SetExpr::Select(_) => true,
            SetExpr::Query(query) => set_expr_is_select_only(query.body.as_ref()),
            SetExpr::SetOperation { left, right, .. } => {
                set_expr_is_select_only(left) && set_expr_is_select_only(right)
            }
            _ => false,
        }
    }

    struct SelectBodyValidator {
        valid: bool,
    }

    impl Visitor for SelectBodyValidator {
        type Break = ();

        fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<Self::Break> {
            if set_expr_is_select_only(query.body.as_ref()) {
                ControlFlow::Continue(())
            } else {
                self.valid = false;
                ControlFlow::Break(())
            }
        }
    }

    let mut validator = SelectBodyValidator { valid: true };
    let _ = query.visit(&mut validator);
    validator.valid
}

fn diagnostic(
    code: SqlLintDiagnosticCode,
    message: &str,
    range: Option<SqlSourceRange>,
) -> SqlLintDiagnostic {
    SqlLintDiagnostic {
        code,
        severity: SqlLintSeverity::Error,
        message: message.to_owned(),
        range,
    }
}

struct ProjectionWildcardVisitor<'a> {
    source: &'a str,
    replacements: &'a [[usize; 3]],
    diagnostics: Vec<SqlLintDiagnostic>,
}

impl Visitor for ProjectionWildcardVisitor<'_> {
    type Break = Infallible;

    fn pre_visit_select(&mut self, select: &Select) -> ControlFlow<Self::Break> {
        for item in &select.projection {
            if !matches!(
                item,
                SelectItem::Wildcard(_) | SelectItem::QualifiedWildcard(_, _)
            ) {
                continue;
            }
            let Some(normalized_range) = source_range_from_span(self.source, item.span()) else {
                continue;
            };
            let range = SqlSourceRange {
                start: map_normalized_offset(
                    normalized_range.start,
                    self.replacements,
                    OffsetBias::Start,
                ),
                end: map_normalized_offset(
                    normalized_range.end,
                    self.replacements,
                    OffsetBias::End,
                ),
            };
            self.diagnostics.push(diagnostic(
                SqlLintDiagnosticCode::WildcardProjection,
                "Composition SQL must list projected columns explicitly",
                Some(range),
            ));
        }
        ControlFlow::Continue(())
    }
}

#[derive(Clone, Copy)]
enum OffsetBias {
    Start,
    End,
}

/// Translate a byte boundary in normalized SQL back to the user's source.
fn map_normalized_offset(
    normalized_offset: usize,
    replacements: &[[usize; 3]],
    bias: OffsetBias,
) -> usize {
    let mut source_cursor = 0usize;
    let mut normalized_cursor = 0usize;
    for [source_start, source_end, replacement_len] in replacements {
        let unchanged_len = source_start.saturating_sub(source_cursor);
        let unchanged_end = normalized_cursor.saturating_add(unchanged_len);
        if normalized_offset <= unchanged_end {
            return source_cursor
                .saturating_add(normalized_offset.saturating_sub(normalized_cursor));
        }

        let replacement_start = unchanged_end;
        let replacement_end = replacement_start.saturating_add(*replacement_len);
        if normalized_offset < replacement_end {
            return match bias {
                OffsetBias::Start => *source_start,
                OffsetBias::End => *source_end,
            };
        }
        if normalized_offset == replacement_end {
            return *source_end;
        }

        source_cursor = *source_end;
        normalized_cursor = replacement_end;
    }

    source_cursor.saturating_add(normalized_offset.saturating_sub(normalized_cursor))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(sql: &str, profile: SqlLintProfile) -> Vec<SqlLintDiagnosticCode> {
        lint_sql(sql, profile)
            .diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.code)
            .collect()
    }

    #[test]
    fn composition_profile_rejects_only_projection_wildcards() {
        for sql in [
            "SELECT * FROM t",
            "SELECT t.* FROM t",
            "WITH source AS (SELECT * FROM t) SELECT id FROM source",
            "SELECT (SELECT * FROM t) AS nested_value",
        ] {
            assert_eq!(
                codes(sql, SqlLintProfile::Composition),
                [SqlLintDiagnosticCode::WildcardProjection],
                "{sql}"
            );
        }

        for sql in [
            "SELECT COUNT(*) FROM t",
            "SELECT price * quantity FROM t",
            "SELECT '*' FROM t",
            "SELECT id /* * */ FROM t",
        ] {
            assert!(
                lint_sql(sql, SqlLintProfile::Composition).is_valid(),
                "{sql}"
            );
        }

        assert!(lint_sql("SELECT * FROM t", SqlLintProfile::Standard).is_valid());
    }

    #[test]
    fn composition_profile_keeps_source_ranges_across_template_normalization() {
        let sql = "SELECT {{month}}, 店.* FROM records";
        let diagnostic = lint_sql(sql, SqlLintProfile::Composition)
            .diagnostics
            .remove(0);
        let range = diagnostic.range.expect("wildcard range");
        assert_eq!(&sql[range.start..range.end], "店.*");
    }

    #[test]
    fn composition_profile_requires_one_select_and_checks_template_markers() {
        assert_eq!(
            codes("SELECT 1; SELECT 2", SqlLintProfile::Composition),
            [SqlLintDiagnosticCode::SingleStatementRequired]
        );
        assert_eq!(
            codes("DELETE FROM t", SqlLintProfile::Composition),
            [SqlLintDiagnosticCode::SelectRequired]
        );
        assert_eq!(
            codes("DELETE FROM t", SqlLintProfile::Standard),
            [SqlLintDiagnosticCode::SelectRequired]
        );
        assert_eq!(
            codes("SELECT 1; SELECT 2", SqlLintProfile::Standard),
            [SqlLintDiagnosticCode::SingleStatementRequired]
        );
        assert_eq!(
            codes("SELECT {{bad-name}}", SqlLintProfile::Composition),
            [SqlLintDiagnosticCode::InvalidTemplate]
        );
        assert!(lint_sql("SELECT '{{ignored}}'", SqlLintProfile::Composition).is_valid());
        assert!(lint_sql("SELECT 1 -- {{ignored}}", SqlLintProfile::Composition).is_valid());
    }

    #[test]
    fn static_lint_rejects_non_select_query_bodies() {
        for (sql, expected_code) in [
            ("VALUES (1)", SqlLintDiagnosticCode::SelectRequired),
            ("TABLE entries", SqlLintDiagnosticCode::InvalidSyntax),
            (
                "SELECT 1 UNION VALUES (2)",
                SqlLintDiagnosticCode::SelectRequired,
            ),
            (
                "WITH source AS (VALUES (1)) SELECT column1 FROM source",
                SqlLintDiagnosticCode::SelectRequired,
            ),
            (
                "SELECT * FROM (VALUES (1)) AS source(column1)",
                SqlLintDiagnosticCode::SelectRequired,
            ),
        ] {
            assert_eq!(
                codes(sql, SqlLintProfile::Standard),
                [expected_code],
                "{sql}"
            );
            assert_eq!(
                codes(sql, SqlLintProfile::Composition),
                [expected_code],
                "{sql}"
            );
        }
    }

    #[test]
    fn response_has_a_stable_portable_shape() {
        let response = lint_sql("SELECT * FROM t", SqlLintProfile::Composition);
        let value = serde_json::to_value(&response).expect("serialize response");
        assert_eq!(value["contract_version"], 1);
        assert_eq!(value["profile"], "composition");
        assert_eq!(value["diagnostics"][0]["code"], "wildcard_projection");
        assert_eq!(value["diagnostics"][0]["severity"], "error");
        assert_eq!(value["diagnostics"][0]["range"]["start"], 7);
        assert_eq!(value["diagnostics"][0]["range"]["end"], 8);
    }
}
