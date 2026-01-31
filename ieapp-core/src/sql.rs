use anyhow::{anyhow, Result};
use serde_json::Value;
use sqlparser::ast::{
    BinaryOperator, Expr, Ident, ObjectName, OrderByExpr, SelectItem, SetExpr, Statement,
    TableFactor, Value as SqlValue,
};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;
use std::cmp::Ordering;

#[derive(Debug, Clone)]
pub struct SqlQuery {
    pub table: String,
    pub table_alias: Option<String>,
    pub selection: Option<Expr>,
    pub order_by: Vec<OrderByExpr>,
    pub limit: Option<usize>,
}

pub fn parse_sql(query: &str) -> Result<SqlQuery> {
    let dialect = GenericDialect {};
    let statements =
        Parser::parse_sql(&dialect, query).map_err(|e| anyhow!("SQL parse error: {e}"))?;
    if statements.len() != 1 {
        return Err(anyhow!("Only a single SQL statement is supported"));
    }

    let Statement::Query(boxed) = &statements[0] else {
        return Err(anyhow!("Only SELECT queries are supported"));
    };

    let SetExpr::Select(select) = boxed.body.as_ref() else {
        return Err(anyhow!("Only SELECT queries are supported"));
    };

    if select.projection.is_empty()
        || !select.projection.iter().all(|item| {
            matches!(
                item,
                SelectItem::Wildcard(_) | SelectItem::QualifiedWildcard(_, _)
            )
        })
    {
        return Err(anyhow!("Only SELECT * is supported in IEapp SQL"));
    }

    if select.from.len() != 1 {
        return Err(anyhow!("Exactly one FROM target is required"));
    }

    let relation = &select.from[0].relation;
    let (table, table_alias) = match relation {
        TableFactor::Table { name, alias, .. } => (object_name_to_string(name), alias.clone()),
        _ => return Err(anyhow!("Unsupported FROM clause (expected a table name)")),
    };

    let selection = select.selection.clone();
    let order_by = boxed.order_by.clone();
    let limit = match &boxed.limit {
        Some(Expr::Value(SqlValue::Number(num, _))) => num.parse::<usize>().ok(),
        Some(Expr::UnaryOp { op, expr }) => match (&**expr, op.to_string().as_str()) {
            (Expr::Value(SqlValue::Number(num, _)), "-") => num.parse::<usize>().ok(),
            _ => None,
        },
        Some(_) => None,
        None => None,
    };

    Ok(SqlQuery {
        table,
        table_alias: table_alias.map(|alias| alias.name.value),
        selection,
        order_by,
        limit,
    })
}

pub fn filter_notes_by_sql(notes: Vec<Value>, query: &SqlQuery) -> Result<Vec<Value>> {
    let mut filtered: Vec<Value> = notes
        .into_iter()
        .filter(|note| match_class_filter(note, &query.table))
        .filter(|note| {
            if let Some(expr) = &query.selection {
                matches_expr(note, expr, &query.table, query.table_alias.as_deref())
                    .unwrap_or(false)
            } else {
                true
            }
        })
        .collect();

    if !query.order_by.is_empty() {
        filtered.sort_by(|a, b| {
            compare_notes(
                a,
                b,
                &query.order_by,
                &query.table,
                query.table_alias.as_deref(),
            )
            .unwrap_or(Ordering::Equal)
        });
    }

    if let Some(limit) = query.limit {
        filtered.truncate(limit);
    }

    Ok(filtered)
}

fn object_name_to_string(name: &ObjectName) -> String {
    name.0
        .last()
        .map(|ident| ident.value.clone())
        .unwrap_or_else(|| "notes".to_string())
}

fn match_class_filter(note: &Value, table: &str) -> bool {
    if table.eq_ignore_ascii_case("notes") {
        return true;
    }
    note.get("class")
        .and_then(|v| v.as_str())
        .map(|class| class.eq_ignore_ascii_case(table))
        .unwrap_or(false)
}

fn matches_expr(note: &Value, expr: &Expr, table: &str, table_alias: Option<&str>) -> Result<bool> {
    match expr {
        Expr::BinaryOp { left, op, right } => match op {
            BinaryOperator::And => Ok(matches_expr(note, left, table, table_alias)?
                && matches_expr(note, right, table, table_alias)?),
            BinaryOperator::Or => Ok(matches_expr(note, left, table, table_alias)?
                || matches_expr(note, right, table, table_alias)?),
            BinaryOperator::Eq
            | BinaryOperator::NotEq
            | BinaryOperator::Gt
            | BinaryOperator::GtEq
            | BinaryOperator::Lt
            | BinaryOperator::LtEq => {
                let left_value = resolve_operand(note, left, table, table_alias)?;
                let right_value = resolve_operand(note, right, table, table_alias)?;
                compare_values(&left_value, &right_value, op)
            }
            _ => Err(anyhow!("Unsupported SQL operator: {op}")),
        },
        Expr::Nested(inner) => matches_expr(note, inner, table, table_alias),
        Expr::UnaryOp { op, expr } if op.to_string().to_lowercase() == "not" => {
            Ok(!matches_expr(note, expr, table, table_alias)?)
        }
        Expr::IsNull(expr) => {
            let value = resolve_operand(note, expr, table, table_alias)?;
            Ok(value.is_null())
        }
        Expr::IsNotNull(expr) => {
            let value = resolve_operand(note, expr, table, table_alias)?;
            Ok(!value.is_null())
        }
        Expr::InList {
            expr,
            list,
            negated,
        } => {
            let value = resolve_operand(note, expr, table, table_alias)?;
            let mut matches = false;
            for item in list {
                let expected = resolve_operand(note, item, table, table_alias)?;
                if compare_values(&value, &expected, &BinaryOperator::Eq)? {
                    matches = true;
                    break;
                }
            }
            Ok(if *negated { !matches } else { matches })
        }
        Expr::Like {
            negated,
            expr,
            pattern,
            ..
        } => {
            let value = resolve_operand(note, expr, table, table_alias)?;
            let pattern_value = resolve_operand(note, pattern, table, table_alias)?;
            let candidate = value.as_str().unwrap_or_default();
            let pattern_str = pattern_value.as_str().unwrap_or_default();
            let matches = like_match(candidate, pattern_str);
            Ok(if *negated { !matches } else { matches })
        }
        Expr::ILike {
            negated,
            expr,
            pattern,
            ..
        } => {
            let value = resolve_operand(note, expr, table, table_alias)?;
            let pattern_value = resolve_operand(note, pattern, table, table_alias)?;
            let candidate = value.as_str().unwrap_or_default().to_lowercase();
            let pattern_str = pattern_value.as_str().unwrap_or_default().to_lowercase();
            let matches = like_match(&candidate, &pattern_str);
            Ok(if *negated { !matches } else { matches })
        }
        Expr::Between {
            expr,
            low,
            high,
            negated,
        } => {
            let value = resolve_operand(note, expr, table, table_alias)?;
            let low_value = resolve_operand(note, low, table, table_alias)?;
            let high_value = resolve_operand(note, high, table, table_alias)?;
            let lower_ok = compare_values(&value, &low_value, &BinaryOperator::GtEq)?;
            let upper_ok = compare_values(&value, &high_value, &BinaryOperator::LtEq)?;
            let matches = lower_ok && upper_ok;
            Ok(if *negated { !matches } else { matches })
        }
        _ => Err(anyhow!("Unsupported SQL expression: {expr:?}")),
    }
}

fn resolve_operand(
    note: &Value,
    expr: &Expr,
    table: &str,
    table_alias: Option<&str>,
) -> Result<Value> {
    match expr {
        Expr::Identifier(ident) => Ok(resolve_identifier(
            note,
            std::slice::from_ref(ident),
            table,
            table_alias,
        )),
        Expr::CompoundIdentifier(idents) => {
            Ok(resolve_identifier(note, idents, table, table_alias))
        }
        Expr::Value(value) => Ok(sql_value_to_json(value)),
        Expr::UnaryOp { op, expr } if op.to_string() == "-" => {
            let value = resolve_operand(note, expr, table, table_alias)?;
            if let Some(n) = value.as_f64() {
                Ok(Value::Number(serde_json::Number::from_f64(-n).unwrap()))
            } else {
                Ok(Value::Null)
            }
        }
        _ => Err(anyhow!("Unsupported SQL operand: {expr:?}")),
    }
}

fn resolve_identifier(
    note: &Value,
    idents: &[Ident],
    table: &str,
    table_alias: Option<&str>,
) -> Value {
    if idents.is_empty() {
        return Value::Null;
    }

    let mut parts: Vec<String> = idents.iter().map(|i| i.value.clone()).collect();
    if let Some(first) = parts.first() {
        if first.eq_ignore_ascii_case(table)
            || table_alias
                .map(|alias| alias.eq_ignore_ascii_case(first))
                .unwrap_or(false)
        {
            parts.remove(0);
        }
    }

    if parts.len() >= 2 && parts[0].eq_ignore_ascii_case("properties") {
        let key = parts[1..].join(".");
        if let Some(props) = note.get("properties").and_then(|v| v.as_object()) {
            if let Some(value) = props.get(&key) {
                return value.clone();
            }
            if let Some(value) = find_case_insensitive(props, &key) {
                return value.clone();
            }
        }
        return Value::Null;
    }

    if parts.len() == 1 {
        let key = &parts[0];
        if let Some(obj) = note.as_object() {
            if let Some(value) = obj.get(key) {
                return value.clone();
            }
            if let Some(value) = find_case_insensitive(obj, key) {
                return value.clone();
            }
        }
        if let Some(props) = note.get("properties").and_then(|v| v.as_object()) {
            if let Some(value) = props.get(key) {
                return value.clone();
            }
            if let Some(value) = find_case_insensitive(props, key) {
                return value.clone();
            }
        }
    }

    Value::Null
}

fn find_case_insensitive<'a>(
    map: &'a serde_json::Map<String, Value>,
    key: &str,
) -> Option<&'a Value> {
    map.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v)
}

fn sql_value_to_json(value: &SqlValue) -> Value {
    match value {
        SqlValue::Number(num, _) => num
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        SqlValue::SingleQuotedString(s) | SqlValue::DoubleQuotedString(s) => {
            Value::String(s.clone())
        }
        SqlValue::Boolean(b) => Value::Bool(*b),
        SqlValue::Null => Value::Null,
        _ => Value::Null,
    }
}

fn compare_values(left: &Value, right: &Value, op: &BinaryOperator) -> Result<bool> {
    match op {
        BinaryOperator::Eq => Ok(values_equal(left, right)),
        BinaryOperator::NotEq => Ok(!values_equal(left, right)),
        BinaryOperator::Gt => Ok(compare_order(left, right) == Some(Ordering::Greater)),
        BinaryOperator::GtEq => Ok(matches!(
            compare_order(left, right),
            Some(Ordering::Greater | Ordering::Equal)
        )),
        BinaryOperator::Lt => Ok(compare_order(left, right) == Some(Ordering::Less)),
        BinaryOperator::LtEq => Ok(matches!(
            compare_order(left, right),
            Some(Ordering::Less | Ordering::Equal)
        )),
        _ => Err(anyhow!("Unsupported SQL comparison operator")),
    }
}

fn values_equal(left: &Value, right: &Value) -> bool {
    if let (Some(left_list), Some(expected)) = (left.as_array(), right.as_str()) {
        return left_list
            .iter()
            .any(|item| item == &Value::String(expected.to_string()));
    }
    if let (Some(right_list), Some(expected)) = (right.as_array(), left.as_str()) {
        return right_list
            .iter()
            .any(|item| item == &Value::String(expected.to_string()));
    }
    left == right
}

fn compare_order(left: &Value, right: &Value) -> Option<Ordering> {
    if let (Some(left_num), Some(right_num)) = (left.as_f64(), right.as_f64()) {
        return left_num.partial_cmp(&right_num);
    }
    if let (Some(left_str), Some(right_str)) = (left.as_str(), right.as_str()) {
        return Some(left_str.cmp(right_str));
    }
    None
}

fn like_match(value: &str, pattern: &str) -> bool {
    if pattern == "%" {
        return true;
    }
    let escaped = regex::escape(pattern).replace('%', ".*");
    let re = format!("^{}$", escaped);
    regex::Regex::new(&re)
        .map(|regex| regex.is_match(value))
        .unwrap_or(false)
}

fn compare_notes(
    left: &Value,
    right: &Value,
    order_by: &[OrderByExpr],
    table: &str,
    table_alias: Option<&str>,
) -> Result<Ordering> {
    for order in order_by {
        let (Expr::Identifier(_) | Expr::CompoundIdentifier(_) | Expr::Value(_)) = &order.expr
        else {
            return Err(anyhow!("Unsupported ORDER BY expression"));
        };
        let left_value = resolve_operand(left, &order.expr, table, table_alias)?;
        let right_value = resolve_operand(right, &order.expr, table, table_alias)?;
        let ordering = compare_order(&left_value, &right_value).unwrap_or(Ordering::Equal);
        if ordering != Ordering::Equal {
            return Ok(if order.asc.unwrap_or(true) {
                ordering
            } else {
                ordering.reverse()
            });
        }
    }
    Ok(Ordering::Equal)
}
