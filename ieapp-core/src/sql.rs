use anyhow::{anyhow, Result};
use serde_json::Value;
use sqlparser::ast::{
    BinaryOperator, Expr, Ident, Join, JoinConstraint, JoinOperator, ObjectName, OrderByExpr,
    SelectItem, SetExpr, Statement, TableFactor, Value as SqlValue,
};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone)]
pub struct SqlQuery {
    pub from: SqlTableRef,
    pub joins: Vec<SqlJoin>,
    pub selection: Option<Expr>,
    pub order_by: Vec<OrderByExpr>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct SqlTableRef {
    pub name: String,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlJoinType {
    Inner,
    Left,
    Cross,
}

#[derive(Debug, Clone)]
pub struct SqlJoin {
    pub join_type: SqlJoinType,
    pub table: SqlTableRef,
    pub constraint: JoinConstraint,
}

const SQL_ERROR_PREFIX: &str = "IEAPP_SQL_ERROR";
const MAX_QUERY_LIMIT: usize = 1000;
const LIKE_REGEX_CACHE_LIMIT: usize = 256;

fn sql_error(message: impl std::fmt::Display) -> anyhow::Error {
    anyhow!("{SQL_ERROR_PREFIX}: {message}")
}

pub fn parse_sql(query: &str) -> Result<SqlQuery> {
    let dialect = GenericDialect {};
    let statements = Parser::parse_sql(&dialect, query)
        .map_err(|e| sql_error(format!("SQL parse error: {e}")))?;
    if statements.len() != 1 {
        return Err(sql_error("Only a single SQL statement is supported"));
    }

    let Statement::Query(boxed) = &statements[0] else {
        return Err(sql_error("Only SELECT queries are supported"));
    };

    let SetExpr::Select(select) = boxed.body.as_ref() else {
        return Err(sql_error("Only SELECT queries are supported"));
    };

    if select.projection.is_empty()
        || !select.projection.iter().all(|item| {
            matches!(
                item,
                SelectItem::Wildcard(_) | SelectItem::QualifiedWildcard(_, _)
            )
        })
    {
        return Err(sql_error("Only SELECT * is supported in IEapp SQL"));
    }

    if select.from.len() != 1 {
        return Err(sql_error("Exactly one FROM target is required"));
    }

    let from = &select.from[0];
    let from_table = parse_table_ref(&from.relation)?;
    let joins = from
        .joins
        .iter()
        .map(parse_join)
        .collect::<Result<Vec<_>>>()?;

    let selection = select.selection.clone();
    let order_by = boxed.order_by.clone();
    let limit = match &boxed.limit {
        Some(expr) => Some(parse_limit(expr)?),
        None => None,
    };

    Ok(SqlQuery {
        from: from_table,
        joins,
        selection,
        order_by,
        limit,
    })
}

pub fn filter_notes_by_sql(
    tables: &HashMap<String, Vec<Value>>,
    query: &SqlQuery,
) -> Result<Vec<Value>> {
    let base_rows = table_rows(tables, &query.from.name)?;

    let mut contexts: Vec<RowContext> = base_rows
        .iter()
        .map(|row| RowContext::new(&query.from, row.clone()))
        .collect();

    for join in &query.joins {
        let join_rows = table_rows(tables, &join.table.name)?;
        let mut joined = Vec::new();
        for context in contexts.into_iter() {
            let mut matched = false;
            for row in join_rows {
                let mut next = context.clone();
                next.add_table(&join.table, row.clone());

                let matches = match &join.constraint {
                    JoinConstraint::On(expr) => matches_expr(&next, expr)?,
                    JoinConstraint::None => true,
                    _ => return Err(sql_error("Only JOIN ... ON and CROSS JOIN are supported")),
                };

                if matches {
                    matched = true;
                    joined.push(next);
                }
            }

            if !matched && join.join_type == SqlJoinType::Left {
                let mut next = context.clone();
                next.add_table(&join.table, Value::Null);
                joined.push(next);
            }
        }
        contexts = joined;
    }

    let mut filtered: Vec<RowContext> = Vec::new();
    for context in contexts.into_iter() {
        if let Some(expr) = &query.selection {
            if !matches_expr(&context, expr)? {
                continue;
            }
        }
        filtered.push(context);
    }

    if !query.order_by.is_empty() {
        let mut sort_error: Option<anyhow::Error> = None;
        filtered.sort_by(|a, b| {
            if sort_error.is_some() {
                return Ordering::Equal;
            }
            match compare_rows(a, b, &query.order_by) {
                Ok(ordering) => ordering,
                Err(err) => {
                    sort_error = Some(err);
                    Ordering::Equal
                }
            }
        });
        if let Some(err) = sort_error {
            return Err(err);
        }
    }

    let effective_limit = query.limit.unwrap_or(MAX_QUERY_LIMIT);
    filtered.truncate(effective_limit);

    Ok(filtered
        .into_iter()
        .map(|context| context.to_value())
        .collect())
}

fn parse_limit(expr: &Expr) -> Result<usize> {
    let raw = match expr {
        Expr::Value(SqlValue::Number(num, _)) => num
            .parse::<i64>()
            .map_err(|_| sql_error("LIMIT must be an integer literal"))?,
        Expr::UnaryOp { op, expr } if op.to_string() == "-" => {
            let inner = match &**expr {
                Expr::Value(SqlValue::Number(num, _)) => num
                    .parse::<i64>()
                    .map_err(|_| sql_error("LIMIT must be an integer literal"))?,
                _ => return Err(sql_error("LIMIT must be an integer literal")),
            };
            -inner
        }
        _ => return Err(sql_error("LIMIT must be an integer literal")),
    };

    if raw < 0 {
        return Err(sql_error("LIMIT must be greater than or equal to 0"));
    }
    let limit = usize::try_from(raw).map_err(|_| sql_error("LIMIT exceeds platform size"))?;
    if limit > MAX_QUERY_LIMIT {
        return Err(sql_error(format!(
            "LIMIT exceeds maximum of {MAX_QUERY_LIMIT}"
        )));
    }
    Ok(limit)
}

fn parse_table_ref(relation: &TableFactor) -> Result<SqlTableRef> {
    match relation {
        TableFactor::Table { name, alias, .. } => Ok(SqlTableRef {
            name: object_name_to_string(name),
            alias: alias.clone().map(|alias| alias.name.value),
        }),
        _ => Err(sql_error("Unsupported FROM clause (expected a table name)")),
    }
}

fn parse_join(join: &Join) -> Result<SqlJoin> {
    let table = parse_table_ref(&join.relation)?;
    let (join_type, constraint) = match &join.join_operator {
        JoinOperator::Inner(constraint) => (SqlJoinType::Inner, constraint.clone()),
        JoinOperator::LeftOuter(constraint) => (SqlJoinType::Left, constraint.clone()),
        JoinOperator::CrossJoin => (SqlJoinType::Cross, JoinConstraint::None),
        _ => return Err(sql_error("Only INNER, LEFT, and CROSS joins are supported")),
    };
    Ok(SqlJoin {
        join_type,
        table,
        constraint,
    })
}

fn table_rows<'a>(tables: &'a HashMap<String, Vec<Value>>, name: &str) -> Result<&'a Vec<Value>> {
    let key = name.to_lowercase();
    tables
        .get(&key)
        .ok_or_else(|| sql_error(format!("Unknown table: {}", name)))
}

fn object_name_to_string(name: &ObjectName) -> String {
    name.0
        .last()
        .map(|ident| ident.value.clone())
        .unwrap_or_else(|| "notes".to_string())
}
#[derive(Debug, Clone)]
struct RowContext {
    tables: HashMap<String, Value>,
    id_map: HashMap<String, String>,
    output_names: HashMap<String, String>,
    base_key: String,
}

impl RowContext {
    fn new(table: &SqlTableRef, row: Value) -> Self {
        let canonical = canonical_table_key(table);
        let mut tables = HashMap::new();
        tables.insert(canonical.clone(), row);

        let mut id_map = HashMap::new();
        id_map.insert(table.name.to_lowercase(), canonical.clone());
        if let Some(alias) = &table.alias {
            id_map.insert(alias.to_lowercase(), canonical.clone());
        }

        let mut output_names = HashMap::new();
        output_names.insert(
            canonical.clone(),
            table.alias.clone().unwrap_or(table.name.clone()),
        );

        RowContext {
            tables,
            id_map,
            output_names,
            base_key: canonical,
        }
    }

    fn add_table(&mut self, table: &SqlTableRef, row: Value) {
        let canonical = canonical_table_key(table);
        self.tables.insert(canonical.clone(), row);
        self.id_map
            .insert(table.name.to_lowercase(), canonical.clone());
        if let Some(alias) = &table.alias {
            self.id_map.insert(alias.to_lowercase(), canonical.clone());
        }
        self.output_names.insert(
            canonical.clone(),
            table.alias.clone().unwrap_or(table.name.clone()),
        );
    }

    fn to_value(&self) -> Value {
        if self.tables.len() == 1 {
            return self
                .tables
                .get(&self.base_key)
                .cloned()
                .unwrap_or(Value::Null);
        }
        let mut map = serde_json::Map::new();
        for (key, value) in &self.tables {
            let name = self
                .output_names
                .get(key)
                .cloned()
                .unwrap_or_else(|| key.clone());
            map.insert(name, value.clone());
        }
        Value::Object(map)
    }
}

fn canonical_table_key(table: &SqlTableRef) -> String {
    table.alias.as_deref().unwrap_or(&table.name).to_lowercase()
}

fn matches_expr(context: &RowContext, expr: &Expr) -> Result<bool> {
    match expr {
        Expr::BinaryOp { left, op, right } => match op {
            BinaryOperator::And => {
                Ok(matches_expr(context, left)? && matches_expr(context, right)?)
            }
            BinaryOperator::Or => Ok(matches_expr(context, left)? || matches_expr(context, right)?),
            BinaryOperator::Eq
            | BinaryOperator::NotEq
            | BinaryOperator::Gt
            | BinaryOperator::GtEq
            | BinaryOperator::Lt
            | BinaryOperator::LtEq => {
                let left_value = resolve_operand(context, left)?;
                let right_value = resolve_operand(context, right)?;
                compare_values(&left_value, &right_value, op)
            }
            _ => Err(sql_error(format!("Unsupported SQL operator: {op}"))),
        },
        Expr::Nested(inner) => matches_expr(context, inner),
        Expr::UnaryOp { op, expr } if op.to_string().to_lowercase() == "not" => {
            Ok(!matches_expr(context, expr)?)
        }
        Expr::IsNull(expr) => {
            let value = resolve_operand(context, expr)?;
            Ok(value.is_null())
        }
        Expr::IsNotNull(expr) => {
            let value = resolve_operand(context, expr)?;
            Ok(!value.is_null())
        }
        Expr::InList {
            expr,
            list,
            negated,
        } => {
            let value = resolve_operand(context, expr)?;
            let mut matches = false;
            for item in list {
                let expected = resolve_operand(context, item)?;
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
            let value = resolve_operand(context, expr)?;
            let pattern_value = resolve_operand(context, pattern)?;
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
            let value = resolve_operand(context, expr)?;
            let pattern_value = resolve_operand(context, pattern)?;
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
            let value = resolve_operand(context, expr)?;
            let low_value = resolve_operand(context, low)?;
            let high_value = resolve_operand(context, high)?;
            let lower_ok = compare_values(&value, &low_value, &BinaryOperator::GtEq)?;
            let upper_ok = compare_values(&value, &high_value, &BinaryOperator::LtEq)?;
            let matches = lower_ok && upper_ok;
            Ok(if *negated { !matches } else { matches })
        }
        _ => Err(sql_error(format!("Unsupported SQL expression: {expr:?}"))),
    }
}

fn resolve_operand(context: &RowContext, expr: &Expr) -> Result<Value> {
    match expr {
        Expr::Identifier(ident) => Ok(resolve_identifier(context, std::slice::from_ref(ident))),
        Expr::CompoundIdentifier(idents) => Ok(resolve_identifier(context, idents)),
        Expr::Value(value) => Ok(sql_value_to_json(value)),
        Expr::UnaryOp { op, expr } if op.to_string() == "-" => {
            let value = resolve_operand(context, expr)?;
            if let Some(n) = value.as_f64() {
                Ok(Value::Number(serde_json::Number::from_f64(-n).unwrap()))
            } else {
                Ok(Value::Null)
            }
        }
        _ => Err(sql_error(format!("Unsupported SQL operand: {expr:?}"))),
    }
}

fn resolve_identifier(context: &RowContext, idents: &[Ident]) -> Value {
    if idents.is_empty() {
        return Value::Null;
    }

    let parts: Vec<String> = idents.iter().map(|i| i.value.clone()).collect();
    let (row, remaining) = if let Some(first) = parts.first() {
        let key = first.to_lowercase();
        if let Some(canonical) = context.id_map.get(&key) {
            let row = context
                .tables
                .get(canonical)
                .cloned()
                .unwrap_or(Value::Null);
            (row, &parts[1..])
        } else {
            let row = context
                .tables
                .get(&context.base_key)
                .cloned()
                .unwrap_or(Value::Null);
            (row, parts.as_slice())
        }
    } else {
        return Value::Null;
    };

    resolve_value_in_row(&row, remaining)
}

fn resolve_value_in_row(row: &Value, parts: &[String]) -> Value {
    if parts.is_empty() {
        return Value::Null;
    }

    if parts.len() >= 2 && parts[0].eq_ignore_ascii_case("properties") {
        let key = parts[1..].join(".");
        if let Some(props) = row.get("properties").and_then(|v| v.as_object()) {
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
        if let Some(obj) = row.as_object() {
            if let Some(value) = obj.get(key) {
                return value.clone();
            }
            if let Some(value) = find_case_insensitive(obj, key) {
                return value.clone();
            }
        }
        if let Some(props) = row.get("properties").and_then(|v| v.as_object()) {
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
        _ => Err(sql_error("Unsupported SQL comparison operator")),
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
    like_regex(pattern)
        .map(|regex| regex.is_match(value))
        .unwrap_or(false)
}

fn like_regex(pattern: &str) -> Option<regex::Regex> {
    static LIKE_CACHE: OnceLock<Mutex<HashMap<String, regex::Regex>>> = OnceLock::new();
    let cache = LIKE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut cache) = cache.lock() {
        if let Some(existing) = cache.get(pattern) {
            return Some(existing.clone());
        }

        let mut regex = String::from("^");
        for ch in pattern.chars() {
            match ch {
                '%' => regex.push_str(".*"),
                '_' => regex.push('.'),
                _ => regex.push_str(&regex::escape(&ch.to_string())),
            }
        }
        regex.push('$');

        if let Ok(compiled) = regex::Regex::new(&regex) {
            if cache.len() >= LIKE_REGEX_CACHE_LIMIT {
                cache.clear();
            }
            cache.insert(pattern.to_string(), compiled.clone());
            return Some(compiled);
        }
    }
    None
}

fn compare_rows(
    left: &RowContext,
    right: &RowContext,
    order_by: &[OrderByExpr],
) -> Result<Ordering> {
    for order in order_by {
        let (Expr::Identifier(_) | Expr::CompoundIdentifier(_) | Expr::Value(_)) = &order.expr
        else {
            return Err(sql_error("Unsupported ORDER BY expression"));
        };
        let left_value = resolve_operand(left, &order.expr)?;
        let right_value = resolve_operand(right, &order.expr)?;
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
