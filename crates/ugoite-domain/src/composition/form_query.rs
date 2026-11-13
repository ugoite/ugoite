//! Rust-owned SQL seeds for the Form entry point in Composition's binding picker.
//! These definitions still require authorized server planning and identity binding.
use serde::{Deserialize, Serialize};
use sqlparser::ast::Ident;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompositionFormQueryError {
    InvalidIdentifier,
    NoProjectableFields,
}

impl CompositionFormQueryError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidIdentifier => "invalid_identifier",
            Self::NoProjectableFields => "form_no_projectable_fields",
        }
    }

    pub const fn message(self) -> &'static str {
        match self {
            Self::InvalidIdentifier => "invalid SQL identifier",
            Self::NoProjectableFields => "Form has no projectable data fields",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionFormQuery {
    pub relation: String,
    #[serde(default)]
    pub columns: Vec<CompositionFormQueryColumn>,
    pub aggregate: Option<CompositionFormAggregate>,
}

/// Rust-owned SQL and the exact SELECT aliases exposed by its projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionFormQueryOutput {
    pub sql: String,
    pub projection_aliases: Vec<String>,
}

/// One authorized, projectable Form field supplied to the SQL seed builder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionFormQueryColumn {
    pub column: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
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

pub fn composition_form_query_sql(
    query: CompositionFormQuery,
) -> Result<String, CompositionFormQueryError> {
    Ok(composition_form_query_seed(query)?.sql)
}

/// Build a Form SQL seed and return its canonical projection aliases.
pub fn composition_form_query_seed(
    query: CompositionFormQuery,
) -> Result<CompositionFormQueryOutput, CompositionFormQueryError> {
    let quote = |value: &str| {
        if value.trim().is_empty() || value.contains('\0') {
            return Err(CompositionFormQueryError::InvalidIdentifier);
        }
        Ok(Ident::with_quote('"', value).to_string())
    };
    let relation = quote(&query.relation)?;
    let is_aggregate = query.aggregate.is_some();
    let mut projection_aliases = Vec::new();
    let projection = match query.aggregate {
        None => {
            if query.columns.is_empty() {
                return Err(CompositionFormQueryError::NoProjectableFields);
            }
            let mut aliases = BTreeSet::new();
            let columns = query
                .columns
                .iter()
                .map(|field| {
                    let column = quote(&field.column)?;
                    let base_alias = field
                        .label
                        .as_deref()
                        .map(str::trim)
                        .filter(|label| !label.is_empty())
                        .unwrap_or(&field.column);
                    let mut alias = base_alias.to_owned();
                    let mut suffix = 2usize;
                    while !aliases.insert(alias.clone()) {
                        alias = format!("{base_alias} ({suffix})");
                        suffix += 1;
                    }
                    projection_aliases.push(alias.clone());
                    let alias = quote(&alias)?;
                    Ok::<String, CompositionFormQueryError>(format!("{column} AS {alias}"))
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            columns
        }
        Some(CompositionFormAggregate::Count {}) => {
            projection_aliases.push("value".to_owned());
            "COUNT(*) AS value".to_owned()
        }
        Some(CompositionFormAggregate::Sum { column }) => {
            projection_aliases.push("value".to_owned());
            format!("SUM({}) AS value", quote(&column)?)
        }
        Some(CompositionFormAggregate::Avg { column }) => {
            projection_aliases.push("value".to_owned());
            format!("AVG({}) AS value", quote(&column)?)
        }
        Some(CompositionFormAggregate::Min { column }) => {
            projection_aliases.push("value".to_owned());
            format!("MIN({}) AS value", quote(&column)?)
        }
        Some(CompositionFormAggregate::Max { column }) => {
            projection_aliases.push("value".to_owned());
            format!("MAX({}) AS value", quote(&column)?)
        }
    };
    let order_by = if is_aggregate {
        String::new()
    } else {
        " ORDER BY \"_ugoite_updated_at\" DESC, \"_ugoite_id\"".to_owned()
    };
    Ok(CompositionFormQueryOutput {
        sql: format!("SELECT {projection} FROM {relation}{order_by}"),
        projection_aliases,
    })
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
                columns: vec![CompositionFormQueryColumn {
                    column: "title".into(),
                    label: Some("Title".into()),
                }],
                aggregate,
            })
            .unwrap();
            assert_eq!(parse_generic_sql_statements(&sql).unwrap().len(), 1);
            assert!(sql.contains("FROM \"form\"\"; DROP TABLE x;--\""));
        }
        assert_eq!(
            composition_form_query_sql(CompositionFormQuery {
                relation: "form_identity".into(),
                columns: vec![CompositionFormQueryColumn {
                    column: "field_title".into(),
                    label: Some("Title".into()),
                }],
                aggregate: None
            })
            .unwrap(),
            "SELECT \"field_title\" AS \"Title\" FROM \"form_identity\" ORDER BY \"_ugoite_updated_at\" DESC, \"_ugoite_id\""
        );
    }

    #[test]
    fn form_seed_projects_fields_with_unique_aliases() {
        let seed = composition_form_query_seed(CompositionFormQuery {
            relation: "Expenses".into(),
            columns: vec![
                CompositionFormQueryColumn {
                    column: "spent_on".into(),
                    label: Some("Date".into()),
                },
                CompositionFormQueryColumn {
                    column: "category".into(),
                    label: Some("Date".into()),
                },
                CompositionFormQueryColumn {
                    column: "amount".into(),
                    label: Some("Date (2)".into()),
                },
            ],
            aggregate: None,
        })
        .unwrap();
        let sql = seed.sql;

        assert_eq!(
            sql,
            "SELECT \"spent_on\" AS \"Date\", \"category\" AS \"Date (2)\", \"amount\" AS \"Date (2) (2)\" FROM \"Expenses\" ORDER BY \"_ugoite_updated_at\" DESC, \"_ugoite_id\""
        );
        assert!(!sql.contains("_ugoite_created_at\" AS"));
        assert!(!sql.contains("LIMIT"));
        assert_eq!(parse_generic_sql_statements(&sql).unwrap().len(), 1);
        assert_eq!(
            seed.projection_aliases,
            ["Date", "Date (2)", "Date (2) (2)"]
        );
    }

    #[test]
    fn form_seed_rejects_empty_projectable_fields() {
        assert_eq!(
            composition_form_query_sql(CompositionFormQuery {
                relation: "Form".into(),
                columns: Vec::new(),
                aggregate: None,
            }),
            Err(CompositionFormQueryError::NoProjectableFields)
        );
    }

    #[test]
    fn form_seeds_reject_invalid_identifiers_and_non_typed_aggregates() {
        assert!(composition_form_query_sql(CompositionFormQuery {
            relation: "".into(),
            columns: Vec::new(),
            aggregate: None
        })
        .is_err());
        assert!(composition_form_query_sql(CompositionFormQuery {
            relation: "form".into(),
            columns: Vec::new(),
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
