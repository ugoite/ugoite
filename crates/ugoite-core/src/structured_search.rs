//! Typed structured Search criteria for durable Knowledge lookup.
//!
//! This module owns the product semantics of "find by Form + field
//! conditions" without exposing any SQL relation/column names, SQL literals,
//! or DataFusion implementation types. Callers express logical identity
//! (`form` + `field`); trusted adapters resolve physical storage inside the
//! authorized boundary (see PR2).
//!
//! Validation happens before any Storage access and returns typed
//! [`AppError`] failures for unknown Forms/fields, unsupported
//! operator/type combinations, and invalid typed values.

use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ugoite_domain::form::{FieldType, FormDefinition};

use crate::error::{AppError, ErrorCode};

/// Maximum number of field conditions in one structured Search.
pub const MAX_STRUCTURED_SEARCH_CONDITIONS: usize = 32;
/// Maximum rows a structured Search may request.
pub const MAX_STRUCTURED_SEARCH_LIMIT: usize = 10_000;
/// Maximum bytes for logical `form` identity.
pub const MAX_STRUCTURED_SEARCH_FORM_BYTES: usize = 256;
/// Maximum bytes for logical `field` identity.
pub const MAX_STRUCTURED_SEARCH_FIELD_BYTES: usize = 256;
/// Maximum bytes for a raw string condition value.
pub const MAX_STRUCTURED_SEARCH_VALUE_BYTES: usize = 8 * 1024;

/// Typed field-match operators. Transport uses snake_case strings.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchOperator {
    Equals,
    Contains,
    Lt,
    Lte,
    Gt,
    Gte,
}

impl SearchOperator {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Equals => "equals",
            Self::Contains => "contains",
            Self::Lt => "lt",
            Self::Lte => "lte",
            Self::Gt => "gt",
            Self::Gte => "gte",
        }
    }
}

/// One logical field condition. `field` is the Ugoite logical field name;
/// never a SQL column name. `value` is the raw JSON value supplied by the
/// caller and normalized during validation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchCondition {
    pub field: String,
    pub operator: SearchOperator,
    pub value: Value,
}

/// Typed structured Search criteria. `form` is the logical Form name; never
/// a SQL relation name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StructuredSearch {
    pub form: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_to: Option<String>,
    #[serde(default)]
    pub conditions: Vec<SearchCondition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
}

/// Logical field kind used for operator/type validation. This is a product
/// classification, not a storage or DataFusion type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StructuredSearchFieldKind {
    String,
    Boolean,
    Integer,
    Numeric,
    Date,
    Timestamp,
}

impl StructuredSearchFieldKind {
    pub fn of(field_type: &FieldType) -> Option<Self> {
        match field_type {
            FieldType::String | FieldType::Markdown => Some(Self::String),
            FieldType::Boolean => Some(Self::Boolean),
            FieldType::Integer | FieldType::Long => Some(Self::Integer),
            FieldType::Float | FieldType::Double => Some(Self::Numeric),
            FieldType::Date => Some(Self::Date),
            FieldType::Timestamp
            | FieldType::TimestampTz
            | FieldType::TimestampNs
            | FieldType::TimestampTzNs => Some(Self::Timestamp),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Boolean => "boolean",
            Self::Integer => "integer",
            Self::Numeric => "numeric",
            Self::Date => "date",
            Self::Timestamp => "timestamp",
        }
    }

    pub fn supports(self, operator: SearchOperator) -> bool {
        match self {
            Self::String => matches!(operator, SearchOperator::Equals | SearchOperator::Contains),
            Self::Boolean => matches!(operator, SearchOperator::Equals),
            Self::Integer | Self::Numeric | Self::Date | Self::Timestamp => matches!(
                operator,
                SearchOperator::Equals
                    | SearchOperator::Lt
                    | SearchOperator::Lte
                    | SearchOperator::Gt
                    | SearchOperator::Gte
            ),
        }
    }
}

/// Canonical normalized condition value. Adapters consume this instead of
/// re-parsing raw JSON.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "value")]
pub enum NormalizedConditionValue {
    String(String),
    Boolean(bool),
    Integer(i64),
    Numeric(f64),
    Date(String),
    Timestamp(String),
}

/// One validated condition with resolved field kind and normalized value.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidatedSearchCondition {
    pub field: String,
    pub operator: SearchOperator,
    pub kind: StructuredSearchFieldKind,
    pub value: NormalizedConditionValue,
}

/// Validated structured Search with canonical bounds and values.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidatedStructuredSearch {
    pub form: String,
    pub updated_from: Option<DateTime<Utc>>,
    pub updated_to: Option<DateTime<Utc>>,
    pub conditions: Vec<ValidatedSearchCondition>,
    pub limit: Option<u64>,
}

fn invalid_input(message: impl Into<String>) -> AppError {
    AppError::invalid_input(ErrorCode::InvalidInput, message)
}

fn validate_form_identity(form: &str) -> Result<String, AppError> {
    if form.trim().is_empty() {
        return Err(invalid_input("structured search form must not be empty"));
    }
    if form.len() > MAX_STRUCTURED_SEARCH_FORM_BYTES {
        return Err(invalid_input(
            "structured search form exceeds the configured byte limit",
        ));
    }
    Ok(form.to_owned())
}

fn validate_field_identity(field: &str) -> Result<String, AppError> {
    if field.trim().is_empty() {
        return Err(invalid_input("structured search field must not be empty"));
    }
    if field.len() > MAX_STRUCTURED_SEARCH_FIELD_BYTES {
        return Err(invalid_input(
            "structured search field exceeds the configured byte limit",
        ));
    }
    Ok(field.to_owned())
}

/// Parse an updated bound. Accepts `YYYY-MM-DD` (midnight UTC) or RFC3339.
/// `YYYY-MM-DDTHH:MM` (datetime-local) is also accepted as UTC for form UI parity.
pub fn parse_updated_bound(raw: &str, bound_name: &str) -> Result<DateTime<Utc>, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(invalid_input(format!(
            "structured search {bound_name} must not be empty"
        )));
    }
    if let Ok(date) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        let naive = date.and_hms_opt(0, 0, 0).ok_or_else(|| {
            invalid_input(format!(
                "structured search {bound_name} is not a valid date"
            ))
        })?;
        return Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
    }
    if let Ok(dt) = trimmed.parse::<DateTime<Utc>>() {
        return Ok(dt);
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        return Ok(dt.with_timezone(&Utc));
    }
    // datetime-local `YYYY-MM-DDTHH:MM` emitted by browser inputs.
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M") {
        return Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S") {
        return Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
    }
    Err(invalid_input(format!(
        "structured search {bound_name} must be YYYY-MM-DD or RFC3339"
    )))
}

fn normalize_string_value(value: &Value, field: &str) -> Result<String, AppError> {
    match value {
        Value::String(text) => {
            if text.len() > MAX_STRUCTURED_SEARCH_VALUE_BYTES {
                return Err(invalid_input(format!(
                    "structured search value for field '{field}' exceeds the byte limit"
                )));
            }
            Ok(text.clone())
        }
        _ => Err(invalid_input(format!(
            "structured search value for field '{field}' must be a string"
        ))),
    }
}

fn normalize_boolean_value(value: &Value, field: &str) -> Result<bool, AppError> {
    match value {
        Value::Bool(flag) => Ok(*flag),
        Value::String(text) => {
            let lowered = text.trim().to_ascii_lowercase();
            match lowered.as_str() {
                "true" => Ok(true),
                "false" => Ok(false),
                _ => Err(invalid_input(format!(
                    "structured search value for field '{field}' must be a boolean"
                ))),
            }
        }
        _ => Err(invalid_input(format!(
            "structured search value for field '{field}' must be a boolean"
        ))),
    }
}

fn normalize_integer_value(value: &Value, field: &str) -> Result<i64, AppError> {
    match value {
        Value::Number(number) => number.as_i64().ok_or_else(|| {
            invalid_input(format!(
                "structured search value for field '{field}' must be an integer"
            ))
        }),
        Value::String(text) => {
            let trimmed = text.trim();
            if !trimmed
                .strip_prefix('-')
                .unwrap_or(trimmed)
                .chars()
                .all(|c| c.is_ascii_digit())
                || trimmed.is_empty()
                || trimmed == "-"
            {
                return Err(invalid_input(format!(
                    "structured search value for field '{field}' must be an integer"
                )));
            }
            trimmed.parse::<i64>().map_err(|_| {
                invalid_input(format!(
                    "structured search value for field '{field}' must be an integer"
                ))
            })
        }
        _ => Err(invalid_input(format!(
            "structured search value for field '{field}' must be an integer"
        ))),
    }
}

fn normalize_numeric_value(value: &Value, field: &str) -> Result<f64, AppError> {
    let number = match value {
        Value::Number(number) => number.as_f64().ok_or_else(|| {
            invalid_input(format!(
                "structured search value for field '{field}' must be a number"
            ))
        })?,
        Value::String(text) => text.trim().parse::<f64>().map_err(|_| {
            invalid_input(format!(
                "structured search value for field '{field}' must be a number"
            ))
        })?,
        _ => {
            return Err(invalid_input(format!(
                "structured search value for field '{field}' must be a number"
            )));
        }
    };
    if !number.is_finite() {
        return Err(invalid_input(format!(
            "structured search value for field '{field}' must be a finite number"
        )));
    }
    Ok(number)
}

fn normalize_date_value(value: &Value, field: &str) -> Result<String, AppError> {
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            let date = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d").map_err(|_| {
                invalid_input(format!(
                    "structured search value for field '{field}' must be YYYY-MM-DD"
                ))
            })?;
            Ok(date.format("%Y-%m-%d").to_string())
        }
        _ => Err(invalid_input(format!(
            "structured search value for field '{field}' must be YYYY-MM-DD"
        ))),
    }
}

fn normalize_timestamp_value(value: &Value, field: &str) -> Result<String, AppError> {
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return Err(invalid_input(format!(
                    "structured search value for field '{field}' must be RFC3339"
                )));
            }
            if let Ok(date) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
                let naive = date.and_hms_opt(0, 0, 0).ok_or_else(|| {
                    invalid_input(format!(
                        "structured search value for field '{field}' must be RFC3339"
                    ))
                })?;
                let dt = DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc);
                return Ok(dt.to_rfc3339_opts(SecondsFormat::Millis, true));
            }
            if let Ok(dt) = trimmed.parse::<DateTime<Utc>>() {
                return Ok(dt.to_rfc3339_opts(SecondsFormat::Millis, true));
            }
            if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
                return Ok(dt
                    .with_timezone(&Utc)
                    .to_rfc3339_opts(SecondsFormat::Millis, true));
            }
            if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M") {
                let dt = DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc);
                return Ok(dt.to_rfc3339_opts(SecondsFormat::Millis, true));
            }
            if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S") {
                let dt = DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc);
                return Ok(dt.to_rfc3339_opts(SecondsFormat::Millis, true));
            }
            Err(invalid_input(format!(
                "structured search value for field '{field}' must be RFC3339"
            )))
        }
        _ => Err(invalid_input(format!(
            "structured search value for field '{field}' must be RFC3339"
        ))),
    }
}

/// Validate syntax-only invariants without touching Storage or Form state.
/// Full type checking requires [`resolve_structured_search`].
pub fn validate_structured_search_syntax(search: &StructuredSearch) -> Result<(), AppError> {
    validate_form_identity(&search.form)?;
    if search.conditions.len() > MAX_STRUCTURED_SEARCH_CONDITIONS {
        return Err(invalid_input(
            "structured search exceeds the maximum condition count",
        ));
    }
    if let Some(limit) = search.limit {
        if limit == 0 || limit as usize > MAX_STRUCTURED_SEARCH_LIMIT {
            return Err(invalid_input("structured search limit is out of range"));
        }
    }
    if let Some(raw) = search.updated_from.as_deref() {
        parse_updated_bound(raw, "updated_from")?;
    }
    if let Some(raw) = search.updated_to.as_deref() {
        parse_updated_bound(raw, "updated_to")?;
    }
    if let (Some(from_raw), Some(to_raw)) =
        (search.updated_from.as_deref(), search.updated_to.as_deref())
    {
        let from = parse_updated_bound(from_raw, "updated_from")?;
        let to = parse_updated_bound(to_raw, "updated_to")?;
        if from > to {
            return Err(invalid_input(
                "structured search updated_from must not be after updated_to",
            ));
        }
    }
    for condition in &search.conditions {
        validate_field_identity(&condition.field)?;
        if let Value::String(text) = &condition.value {
            if text.len() > MAX_STRUCTURED_SEARCH_VALUE_BYTES {
                return Err(invalid_input(
                    "structured search value exceeds the byte limit",
                ));
            }
        }
    }
    Ok(())
}

/// Resolve logical `form`/`field` identity against a Form definition,
/// validate operator/type compatibility, and return canonical normalized
/// values. Must be called before any Storage access.
pub fn resolve_structured_search(
    search: &StructuredSearch,
    form: &FormDefinition,
) -> Result<ValidatedStructuredSearch, AppError> {
    validate_structured_search_syntax(search)?;
    if search.form != form.name {
        return Err(AppError::not_found(
            ErrorCode::FormNotFound,
            format!("structured search form '{}' was not found", search.form),
        ));
    }
    let updated_from = search
        .updated_from
        .as_deref()
        .map(|raw| parse_updated_bound(raw, "updated_from"))
        .transpose()?;
    let updated_to = search
        .updated_to
        .as_deref()
        .map(|raw| parse_updated_bound(raw, "updated_to"))
        .transpose()?;

    let mut conditions = Vec::with_capacity(search.conditions.len());
    for condition in &search.conditions {
        let field_name = validate_field_identity(&condition.field)?;
        let Some(definition) = form.fields.iter().find(|f| f.name == field_name) else {
            return Err(AppError::invalid_input(
                ErrorCode::UnknownFormFields,
                format!("structured search field '{field_name}' was not found"),
            ));
        };
        let Some(kind) = StructuredSearchFieldKind::of(&definition.field_type) else {
            return Err(invalid_input(format!(
                "structured search field '{field_name}' of type '{}' does not support conditions",
                definition.field_type.as_str()
            )));
        };
        if !kind.supports(condition.operator) {
            return Err(invalid_input(format!(
                "structured search operator '{}' is not supported for field '{field_name}' of type '{}'",
                condition.operator.as_str(),
                definition.field_type.as_str()
            )));
        }
        let value = match kind {
            StructuredSearchFieldKind::String => NormalizedConditionValue::String(
                normalize_string_value(&condition.value, &field_name)?,
            ),
            StructuredSearchFieldKind::Boolean => NormalizedConditionValue::Boolean(
                normalize_boolean_value(&condition.value, &field_name)?,
            ),
            StructuredSearchFieldKind::Integer => NormalizedConditionValue::Integer(
                normalize_integer_value(&condition.value, &field_name)?,
            ),
            StructuredSearchFieldKind::Numeric => NormalizedConditionValue::Numeric(
                normalize_numeric_value(&condition.value, &field_name)?,
            ),
            StructuredSearchFieldKind::Date => {
                NormalizedConditionValue::Date(normalize_date_value(&condition.value, &field_name)?)
            }
            StructuredSearchFieldKind::Timestamp => NormalizedConditionValue::Timestamp(
                normalize_timestamp_value(&condition.value, &field_name)?,
            ),
        };
        conditions.push(ValidatedSearchCondition {
            field: field_name,
            operator: condition.operator,
            kind,
            value,
        });
    }

    Ok(ValidatedStructuredSearch {
        form: search.form.clone(),
        updated_from,
        updated_to,
        conditions,
        limit: search.limit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use ugoite_domain::form::{FormField, FormVersion};
    use ugoite_domain::id::{FieldId, FormId};

    fn field(id: i32, name: &str, field_type: FieldType) -> FormField {
        FormField {
            id: FieldId::new(id).expect("field id"),
            name: name.to_owned(),
            field_type,
            required: false,
            label: None,
            description: None,
            semantic_role: None,
            reference_form: None,
            list_item: None,
            validation: None,
            enum_values: Vec::new(),
            deprecated: false,
        }
    }

    fn fixture_form() -> FormDefinition {
        FormDefinition {
            id: FormId::from_uuid(Uuid::nil()),
            version: FormVersion::new(1).expect("version"),
            name: "Task".to_owned(),
            description: None,
            fields: vec![
                field(100, "title", FieldType::String),
                field(101, "notes", FieldType::Markdown),
                field(102, "done", FieldType::Boolean),
                field(103, "priority", FieldType::Integer),
                field(104, "score", FieldType::Float),
                field(105, "due", FieldType::Date),
                field(106, "remind_at", FieldType::Timestamp),
                field(107, "blob", FieldType::Binary),
            ],
            allow_extra_attributes: false,
            extension_metadata: Default::default(),
        }
    }

    fn search(form: &str, conditions: Vec<SearchCondition>) -> StructuredSearch {
        StructuredSearch {
            form: form.to_owned(),
            updated_from: None,
            updated_to: None,
            conditions,
            limit: None,
        }
    }

    fn condition(field: &str, operator: SearchOperator, value: Value) -> SearchCondition {
        SearchCondition {
            field: field.to_owned(),
            operator,
            value,
        }
    }

    #[test]
    fn string_fields_support_equals_and_contains() {
        let form = fixture_form();
        for operator in [SearchOperator::Equals, SearchOperator::Contains] {
            let resolved = resolve_structured_search(
                &search("Task", vec![condition("title", operator, json!("release"))]),
                &form,
            )
            .expect("string operator must validate");
            assert_eq!(resolved.conditions.len(), 1);
        }
    }

    #[test]
    fn string_fields_reject_ordered_operators() {
        let form = fixture_form();
        for operator in [
            SearchOperator::Lt,
            SearchOperator::Lte,
            SearchOperator::Gt,
            SearchOperator::Gte,
        ] {
            let error = resolve_structured_search(
                &search("Task", vec![condition("title", operator, json!("a"))]),
                &form,
            )
            .expect_err("ordered operator on string must fail");
            assert_eq!(error.code(), ErrorCode::InvalidInput);
        }
    }

    #[test]
    fn boolean_supports_only_equals() {
        let form = fixture_form();
        resolve_structured_search(
            &search(
                "Task",
                vec![condition("done", SearchOperator::Equals, json!(true))],
            ),
            &form,
        )
        .expect("boolean equals must validate");
        let error = resolve_structured_search(
            &search(
                "Task",
                vec![condition("done", SearchOperator::Contains, json!("true"))],
            ),
            &form,
        )
        .expect_err("boolean contains must fail");
        assert_eq!(error.code(), ErrorCode::InvalidInput);
    }

    #[test]
    fn numeric_date_timestamp_support_ordered_operators() {
        let form = fixture_form();
        let cases = vec![
            ("priority", json!(3)),
            ("score", json!(1.5)),
            ("due", json!("2026-09-01")),
            ("remind_at", json!("2026-09-01T00:00:00Z")),
        ];
        for (field_name, value) in cases {
            for operator in [
                SearchOperator::Equals,
                SearchOperator::Lt,
                SearchOperator::Lte,
                SearchOperator::Gt,
                SearchOperator::Gte,
            ] {
                resolve_structured_search(
                    &search("Task", vec![condition(field_name, operator, value.clone())]),
                    &form,
                )
                .unwrap_or_else(|_| panic!("{field_name} {operator:?} must validate"));
            }
        }
        // contains is rejected for non-string kinds.
        for (field_name, value) in [("priority", json!(3)), ("due", json!("2026-09-01"))] {
            let error = resolve_structured_search(
                &search(
                    "Task",
                    vec![condition(field_name, SearchOperator::Contains, value)],
                ),
                &form,
            )
            .expect_err("contains on ordered field must fail");
            assert_eq!(error.code(), ErrorCode::InvalidInput);
        }
    }

    #[test]
    fn unknown_field_is_rejected_with_typed_error() {
        let form = fixture_form();
        let error = resolve_structured_search(
            &search(
                "Task",
                vec![condition("missing", SearchOperator::Equals, json!("x"))],
            ),
            &form,
        )
        .expect_err("unknown field must fail");
        assert_eq!(error.code(), ErrorCode::UnknownFormFields);
    }

    #[test]
    fn unknown_form_is_rejected_with_typed_error() {
        let form = fixture_form();
        let error = resolve_structured_search(
            &search(
                "Other",
                vec![condition("title", SearchOperator::Equals, json!("x"))],
            ),
            &form,
        )
        .expect_err("unknown form must fail");
        assert_eq!(error.code(), ErrorCode::FormNotFound);
    }

    #[test]
    fn unsupported_field_type_cannot_create_condition() {
        let form = fixture_form();
        let error = resolve_structured_search(
            &search(
                "Task",
                vec![condition("blob", SearchOperator::Equals, json!("x"))],
            ),
            &form,
        )
        .expect_err("unsupported type must fail");
        assert_eq!(error.code(), ErrorCode::InvalidInput);
    }

    #[test]
    fn invalid_typed_values_are_rejected_before_storage() {
        let form = fixture_form();
        let cases = vec![
            condition("done", SearchOperator::Equals, json!("maybe")),
            condition("priority", SearchOperator::Equals, json!("3.5")),
            condition("priority", SearchOperator::Equals, json!(1.5)),
            condition("score", SearchOperator::Equals, json!("nan-value")),
            condition("due", SearchOperator::Equals, json!("2026-13-01")),
            condition("due", SearchOperator::Equals, json!("not-a-date")),
            condition("remind_at", SearchOperator::Equals, json!("not-a-time")),
            condition("title", SearchOperator::Equals, json!(3)),
        ];
        for invalid in cases {
            let field_name = invalid.field.clone();
            let error = resolve_structured_search(&search("Task", vec![invalid]), &form)
                .expect_err(&format!("{field_name} invalid value must fail"));
            assert_eq!(
                error.code(),
                ErrorCode::InvalidInput,
                "field {field_name} must be InvalidInput"
            );
        }
    }

    #[test]
    fn timestamp_normalization_is_canonical() {
        let form = fixture_form();
        let resolved = resolve_structured_search(
            &search(
                "Task",
                vec![condition(
                    "remind_at",
                    SearchOperator::Equals,
                    json!("2026-09-01"),
                )],
            ),
            &form,
        )
        .expect("date shorthand for timestamp must normalize");
        assert_eq!(
            resolved.conditions[0].value,
            NormalizedConditionValue::Timestamp("2026-09-01T00:00:00.000Z".to_owned())
        );
    }

    #[test]
    fn updated_bounds_accept_date_and_rfc3339() {
        let form = fixture_form();
        let mut criteria = search("Task", Vec::new());
        criteria.updated_from = Some("2026-09-01".to_owned());
        criteria.updated_to = Some("2026-09-02T00:00:00Z".to_owned());
        let resolved = resolve_structured_search(&criteria, &form).expect("bounds parse");
        assert!(resolved.updated_from.expect("from") < resolved.updated_to.expect("to"));
    }

    #[test]
    fn inverted_updated_bounds_are_rejected() {
        let form = fixture_form();
        let criteria = StructuredSearch {
            form: "Task".to_owned(),
            updated_from: Some("2026-09-02".to_owned()),
            updated_to: Some("2026-09-01".to_owned()),
            conditions: Vec::new(),
            limit: None,
        };
        let error = resolve_structured_search(&criteria, &form).expect_err("inverted");
        assert_eq!(error.code(), ErrorCode::InvalidInput);
    }

    #[test]
    fn special_field_names_remain_logical_identity() {
        let mut form = fixture_form();
        form.fields
            .push(field(108, "we\"ird%'_\\field", FieldType::String));
        let resolved = resolve_structured_search(
            &search(
                "Task",
                vec![condition(
                    "we\"ird%'_\\field",
                    SearchOperator::Contains,
                    json!("%_quote'\\"),
                )],
            ),
            &form,
        )
        .expect("special chars stay logical");
        assert_eq!(resolved.conditions[0].field, "we\"ird%'_\\field");
    }

    #[test]
    fn empty_form_and_bad_limit_are_rejected() {
        let form = fixture_form();
        let error =
            resolve_structured_search(&search("", Vec::new()), &form).expect_err("empty form");
        assert_eq!(error.code(), ErrorCode::InvalidInput);
        let mut criteria = search("Task", Vec::new());
        criteria.limit = Some(0);
        assert!(resolve_structured_search(&criteria, &form).is_err());
    }

    use uuid::Uuid;
}
