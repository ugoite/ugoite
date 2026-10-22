//! SQL parser settings shared by direct validation and DataFusion execution.

use datafusion::sql::parser::{DFParserBuilder, Statement};
use std::collections::VecDeque;

/// Match the maximum expression depth exposed by the typed SQL structure
/// editor while keeping parser recursion bounded for every SQL entry point.
pub(crate) const SQL_PARSER_RECURSION_LIMIT: usize = 256;

pub(crate) fn parse_sql(
    sql: &str,
) -> Result<VecDeque<Statement>, datafusion::common::DataFusionError> {
    DFParserBuilder::new(sql)
        .with_recursion_limit(SQL_PARSER_RECURSION_LIMIT)
        .build()?
        .parse_statements()
}

#[cfg(test)]
mod tests {
    use super::parse_sql;
    use ugoite_domain::sql_document::{parse_sql_document, SqlDocumentStatus};

    #[test]
    fn shared_parser_parity_cases_are_accepted_by_datafusion_and_sql_document() {
        let fixtures: serde_json::Value = serde_json::from_str(include_str!(
            "../../../shared/sql/sql-document-parser-parity.json"
        ))
        .expect("the shared SQL parser parity fixture is valid JSON");
        for case in fixtures["cases"].as_array().expect("fixture cases") {
            let id = case["id"].as_str().expect("case id");
            let sql = case["sql"].as_str().expect("case SQL");
            assert!(
                parse_sql(sql).is_ok(),
                "DataFusion rejected parser parity case {id}"
            );
            let document = parse_sql_document(sql);
            assert_eq!(
                document.status,
                SqlDocumentStatus::Parsed,
                "SQL Document parser rejected parity case {id}: {:?}",
                document.diagnostics
            );
            assert_eq!(document.to_sql(), sql, "source changed for case {id}");
        }
    }

    #[test]
    fn parser_accepts_the_filter_tree_depth_exposed_by_the_gui() {
        let predicate = "\"_ugoite_id\" = $entry_id";
        let mut expression = predicate.to_owned();
        for depth in 0..32 {
            let operator = if depth % 2 == 0 { "AND" } else { "OR" };
            expression = format!("({predicate} {operator} ({expression}))");
        }
        let sql = format!("SELECT \"_ugoite_id\" FROM \"G\" WHERE {expression}");

        assert!(parse_sql(&sql).is_ok());
    }
}
