//! Rust-owned SQL seeds for the Form entry point in Composition's binding picker.
//! These definitions still require authorized server planning and identity binding.
use serde::{Deserialize, Serialize};
use sqlparser::ast::Ident;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionFormQuery {
    pub relation: String,
    pub aggregate: Option<CompositionFormAggregate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompositionFormAggregate {
    Count {},
    Sum { column: String },
    Avg { column: String },
    Min { column: String },
    Max { column: String },
}

pub fn composition_form_query_sql(query: CompositionFormQuery) -> Result<String, &'static str> {
    let quote = |value: &str| {
        if value.trim().is_empty() || value.contains('\0') {
            return Err("invalid SQL identifier");
        }
        Ok(Ident::with_quote('"', value).to_string())
    };
    let relation = quote(&query.relation)?;
    let projection = match query.aggregate {
        None => "*".to_owned(),
        Some(CompositionFormAggregate::Count {}) => "COUNT(*) AS value".to_owned(),
        Some(CompositionFormAggregate::Sum { column }) => {
            format!("SUM({}) AS value", quote(&column)?)
        }
        Some(CompositionFormAggregate::Avg { column }) => {
            format!("AVG({}) AS value", quote(&column)?)
        }
        Some(CompositionFormAggregate::Min { column }) => {
            format!("MIN({}) AS value", quote(&column)?)
        }
        Some(CompositionFormAggregate::Max { column }) => {
            format!("MAX({}) AS value", quote(&column)?)
        }
    };
    Ok(format!("SELECT {projection} FROM {relation}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql_document::parse_generic_sql_statements;
    #[test]
    fn form_seeds_quote_identity_and_aggregate_columns_as_single_identifiers() {
        for aggregate in [
            None,
            Some(CompositionFormAggregate::Count {}),
            Some(CompositionFormAggregate::Sum {
                column: "a\"b".into(),
            }),
            Some(CompositionFormAggregate::Avg {
                column: "price".into(),
            }),
            Some(CompositionFormAggregate::Min {
                column: "price".into(),
            }),
            Some(CompositionFormAggregate::Max {
                column: "price".into(),
            }),
        ] {
            let sql = composition_form_query_sql(CompositionFormQuery {
                relation: "form\"; DROP TABLE x;--".into(),
                aggregate,
            })
            .unwrap();
            assert_eq!(parse_generic_sql_statements(&sql).unwrap().len(), 1);
            assert!(sql.ends_with("FROM \"form\"\"; DROP TABLE x;--\""));
        }
        assert_eq!(
            composition_form_query_sql(CompositionFormQuery {
                relation: "form_identity".into(),
                aggregate: None
            })
            .unwrap(),
            "SELECT * FROM \"form_identity\""
        );
    }
    #[test]
    fn form_seeds_reject_invalid_identifiers_and_non_typed_aggregates() {
        assert!(composition_form_query_sql(CompositionFormQuery {
            relation: "".into(),
            aggregate: None
        })
        .is_err());
        assert!(composition_form_query_sql(CompositionFormQuery {
            relation: "form".into(),
            aggregate: Some(CompositionFormAggregate::Sum {
                column: "\0".into()
            })
        })
        .is_err());
        assert!(serde_json::from_value::<CompositionFormQuery>(
            serde_json::json!({"relation":"form", "aggregate":{"kind":"count", "column":"secret"}})
        )
        .is_err());
    }
}
