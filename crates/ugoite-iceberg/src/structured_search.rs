//! Trusted compilation of typed structured Search into authorized queries.
//!
//! Pipeline: Form resolve -> authorization scope resolve -> field type
//! resolve -> operator/type validation -> canonical value normalization ->
//! authorized query plan -> storage adapter execution.
//!
//! Callers supply only logical `form`/`field` identity. SQL relation/column
//! selection, identifier quoting, literal escaping, and LIKE escaping happen
//! exclusively in this adapter. SQL is an internal implementation detail.

use std::collections::{BTreeMap, HashMap};

use anyhow::{anyhow, Context, Result};
use opendal::Operator;
use serde_json::{Map, Value};
use ugoite_core::query::EntryScope;
use ugoite_core::structured_search::{
    NormalizedConditionValue, SearchOperator, StructuredSearch, ValidatedStructuredSearch,
};
use ugoite_domain::form::{sql_column_name, sql_relation_name, FormDefinition};

/// Adapter-owned SQL identifier quoting. Never supplied by callers.
pub(crate) fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Adapter-owned LIKE pattern escaping for `contains`.
pub(crate) fn escape_like_pattern(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

async fn load_form_definitions(op: &Operator, ws_path: &str) -> Result<Vec<FormDefinition>> {
    let workspace = crate::iceberg_store::native_workspace(op, ws_path).await?;
    workspace
        .list_forms_bounded(100_000, 256 * 1024 * 1024)
        .await
}

fn resolve_form<'a>(forms: &'a [FormDefinition], form_name: &str) -> Result<&'a FormDefinition> {
    forms
        .iter()
        .find(|form| form.name == form_name)
        .ok_or_else(|| anyhow!("structured search form '{form_name}' was not found"))
}

/// Compiled SQL plan with bound parameters. Relation/columns are trusted
/// adapter resolution; callers never supply them.
#[derive(Debug, Clone)]
pub(crate) struct CompiledStructuredSearch {
    pub(crate) sql: String,
    pub(crate) values: Map<String, Value>,
    pub(crate) types: BTreeMap<String, String>,
    pub(crate) limit: usize,
}

/// Compile validated criteria into SQL with bound parameters.
/// Identifier quoting and LIKE escaping are adapter-owned.
pub(crate) fn compile_validated_search(
    validated: &ValidatedStructuredSearch,
    form: &FormDefinition,
) -> Result<CompiledStructuredSearch> {
    let relation = sql_relation_name(form.id);
    let mut column_by_field: HashMap<&str, String> = HashMap::new();
    for field in &form.fields {
        column_by_field.insert(field.name.as_str(), sql_column_name(field.id));
    }

    let mut conditions: Vec<String> = Vec::new();
    let mut values = Map::new();
    let mut types = BTreeMap::new();
    let mut index = 0usize;
    let mut bind = |value: Value, kind: &str| -> String {
        let name = format!("search_{index}");
        index += 1;
        values.insert(name.clone(), value);
        types.insert(name.clone(), kind.to_owned());
        format!("${name}")
    };

    if let Some(from) = validated.updated_from {
        let text = from.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let placeholder = bind(Value::String(text), "timestamp");
        conditions.push(format!(
            "{} >= {placeholder}",
            quote_identifier("_ugoite_updated_at")
        ));
    }
    if let Some(to) = validated.updated_to {
        let text = to.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let placeholder = bind(Value::String(text), "timestamp");
        conditions.push(format!(
            "{} < {placeholder}",
            quote_identifier("_ugoite_updated_at")
        ));
    }

    for condition in &validated.conditions {
        let column = column_by_field
            .get(condition.field.as_str())
            .with_context(|| {
                format!(
                    "structured search field '{}' was not found",
                    condition.field
                )
            })?;
        let quoted = quote_identifier(column);
        let operator_sql = match condition.operator {
            SearchOperator::Equals => "=",
            SearchOperator::Contains => "ILIKE",
            SearchOperator::Lt => "<",
            SearchOperator::Lte => "<=",
            SearchOperator::Gt => ">",
            SearchOperator::Gte => ">=",
        };
        match &condition.value {
            NormalizedConditionValue::String(raw) => {
                if condition.operator == SearchOperator::Contains {
                    let pattern = format!("%{}%", escape_like_pattern(raw));
                    let placeholder = bind(Value::String(pattern), "string");
                    conditions.push(format!("{quoted} {operator_sql} {placeholder} ESCAPE '\\'"));
                } else {
                    let placeholder = bind(Value::String(raw.clone()), "string");
                    conditions.push(format!("{quoted} {operator_sql} {placeholder}"));
                }
            }
            NormalizedConditionValue::Boolean(flag) => {
                let placeholder = bind(Value::Bool(*flag), "boolean");
                conditions.push(format!("{quoted} {operator_sql} {placeholder}"));
            }
            NormalizedConditionValue::Integer(number) => {
                let placeholder = bind(Value::Number((*number).into()), "integer");
                conditions.push(format!("{quoted} {operator_sql} {placeholder}"));
            }
            NormalizedConditionValue::Numeric(number) => {
                let number_value = serde_json::Number::from_f64(*number)
                    .ok_or_else(|| anyhow!("structured search numeric value is not finite"))?;
                let placeholder = bind(Value::Number(number_value), "float");
                conditions.push(format!("{quoted} {operator_sql} {placeholder}"));
            }
            NormalizedConditionValue::Date(text) => {
                let placeholder = bind(Value::String(text.clone()), "date");
                conditions.push(format!("{quoted} {operator_sql} {placeholder}"));
            }
            NormalizedConditionValue::Timestamp(text) => {
                let placeholder = bind(Value::String(text.clone()), "timestamp");
                conditions.push(format!("{quoted} {operator_sql} {placeholder}"));
            }
        }
    }

    let limit = validated
        .limit
        .map(|value| value as usize)
        .unwrap_or(crate::MAX_NORMAL_READ_ROWS.min(1000));
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };
    let sql = format!(
        "SELECT * FROM {}{where_clause} ORDER BY {} DESC, {} ASC LIMIT {limit}",
        quote_identifier(&relation),
        quote_identifier("_ugoite_updated_at"),
        quote_identifier("_ugoite_id"),
    );
    Ok(CompiledStructuredSearch {
        sql,
        values,
        types,
        limit,
    })
}

fn form_authorized(form: &FormDefinition, relation_scopes: &BTreeMap<String, EntryScope>) -> bool {
    let relation = sql_relation_name(form.id);
    for key in [
        form.name.to_ascii_lowercase(),
        relation.to_ascii_lowercase(),
    ] {
        match relation_scopes.get(&key) {
            None => continue,
            // Empty allow-list means no readable Entries: filter before execution.
            Some(EntryScope::Only(ids)) if ids.is_empty() => continue,
            Some(_) => return true,
        }
    }
    false
}

/// Core direct execution: AllCurrent scopes. Permission filtering is still
/// applied by the authorized DataFusion view before execution.
pub async fn search_structured(
    op: &Operator,
    ws_path: &str,
    criteria: &StructuredSearch,
) -> Result<Vec<Value>> {
    let scopes = crate::index::all_current_form_scopes(op, ws_path).await?;
    search_structured_with_scopes(op, ws_path, criteria, &scopes).await
}

/// Authorized execution: caller-supplied scopes are applied before query
/// execution. Unauthorized Forms return empty sets, never errors that leak
/// existence.
pub async fn search_structured_with_scopes(
    op: &Operator,
    ws_path: &str,
    criteria: &StructuredSearch,
    relation_scopes: &BTreeMap<String, EntryScope>,
) -> Result<Vec<Value>> {
    // Admission first: syntax validation before any Storage-heavy work beyond
    // Form registry load. Full validation happens after Form resolve.
    ugoite_core::structured_search::validate_structured_search_syntax(criteria)
        .map_err(|error| anyhow!("{}", error.message()))?;
    let forms = load_form_definitions(op, ws_path).await?;
    let form = resolve_form(&forms, &criteria.form)?;
    if !form_authorized(form, relation_scopes) {
        return Ok(Vec::new());
    }
    let validated = ugoite_core::structured_search::resolve_structured_search(criteria, form)
        .map_err(|error| anyhow!("{}: {}", error.code_str(), error.message()))?;
    let compiled = compile_validated_search(&validated, form)?;
    let parameters = crate::index::datafusion_parameters(&compiled.values, &compiled.types)?;
    let (rows, _count) = crate::index::query_structured_search_page_with_parameters(
        op,
        ws_path,
        &compiled.sql,
        relation_scopes,
        parameters,
        0,
        compiled.limit,
    )
    .await?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_quoting_prevents_injection() {
        assert_eq!(quote_identifier("field_1"), "\"field_1\"");
        assert_eq!(quote_identifier("we\"ird"), "\"we\"\"ird\"");
    }

    #[test]
    fn like_escaping_covers_special_chars() {
        assert_eq!(escape_like_pattern("%_\\'\""), "\\%\\_\\\\'\"");
        assert_eq!(escape_like_pattern("100%_done"), "100\\%\\_done");
    }
}
