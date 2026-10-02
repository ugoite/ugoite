//! Pure Composition parameter binding.
//!
//! This module resolves typed Composition values only. Source compilation and
//! query execution stay in their existing core contracts and are added by the
//! corresponding resolver layers.

use crate::entry_query::{
    entry_field_capability, entry_query_text_searches_field_type, EntryFieldRef, EntryFilter,
    EntryPageRequest, EntryProjection, EntryQuery, EntryQueryFieldKind, EntryQueryScope, EntrySort,
    EntrySortDirection, SearchOperator,
};
use chrono::{DateTime, NaiveDate, NaiveDateTime};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use ugoite_domain::composition::{
    CompositionDiagnosticCode, CompositionFieldSchemaEntry, CompositionLiteral,
    CompositionParameter, CompositionParameterType, CompositionQueryOperator,
    CompositionSortDirection, CompositionValue, EntryQueryProjectionTemplate, EntryQueryTemplate,
};
use ugoite_domain::form::{FieldType, FormDefinition};
use ugoite_domain::id::FormId;

/// One stable diagnostic emitted while binding a parameter or value template.
///
/// Parameter identifiers are included for caller-value binding errors.
/// Unknown or unset references embedded in a source template are code-only,
/// so resolving them does not disclose a source's parameter names.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompositionDiagnostic {
    pub code: CompositionDiagnosticCode,
    pub parameter_id: Option<String>,
}

impl CompositionDiagnostic {
    fn for_parameter(code: CompositionDiagnosticCode, parameter_id: &str) -> Self {
        Self {
            code,
            parameter_id: Some(parameter_id.to_owned()),
        }
    }

    fn without_parameter(code: CompositionDiagnosticCode) -> Self {
        Self {
            code,
            parameter_id: None,
        }
    }
}

/// Bound values and deterministic diagnostics for one Composition request.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParameterBindings {
    /// Values supplied by the caller or selected from typed defaults.
    pub values: BTreeMap<String, Value>,
    /// Declared parameters whose supplied value or default failed type validation.
    /// Keeping this state prevents a source reference from misreporting a rejected
    /// value as merely missing.
    pub invalid_values: BTreeSet<String>,
    /// Declared types, including optional parameters that are currently unset.
    pub parameter_types: BTreeMap<String, CompositionParameterType>,
    pub diagnostics: Vec<CompositionDiagnostic>,
}

/// A value after resolving a literal or named Composition parameter.
#[derive(Clone, Debug, PartialEq)]
pub struct BoundCompositionValue {
    pub value: Value,
    /// `None` means the value was a literal in the Composition document.
    pub parameter_type: Option<CompositionParameterType>,
}

/// Bind caller values and defaults against the document's parameter schema.
///
/// Required parameters without values produce `parameter_missing`. Optional
/// unset parameters remain absent until a source actually references them.
/// Unknown caller keys are reported in lexical order; declared parameters are
/// processed in document order, making the complete diagnostic list stable.
pub fn bind_parameters(
    parameters: &[CompositionParameter],
    supplied: &BTreeMap<String, Value>,
) -> ParameterBindings {
    let mut bindings = ParameterBindings::default();
    let mut declared = BTreeSet::new();

    for parameter in parameters {
        if !declared.insert(parameter.id.clone()) {
            bindings
                .diagnostics
                .push(CompositionDiagnostic::without_parameter(
                    CompositionDiagnosticCode::InvalidComposition,
                ));
            return bindings;
        }
        if parameter.default.as_ref().is_some_and(|default| {
            !value_matches_parameter(default.as_json_value(), parameter.parameter_type)
        }) {
            bindings
                .diagnostics
                .push(CompositionDiagnostic::without_parameter(
                    CompositionDiagnosticCode::InvalidComposition,
                ));
            return bindings;
        }
        bindings
            .parameter_types
            .insert(parameter.id.clone(), parameter.parameter_type);
    }

    for parameter in parameters {
        let value = supplied.get(&parameter.id).or_else(|| {
            parameter
                .default
                .as_ref()
                .map(CompositionLiteral::as_json_value)
        });
        let Some(value) = value else {
            if parameter.required {
                bindings
                    .diagnostics
                    .push(CompositionDiagnostic::for_parameter(
                        CompositionDiagnosticCode::ParameterMissing,
                        &parameter.id,
                    ));
            }
            continue;
        };

        if value_matches_parameter(value, parameter.parameter_type) {
            bindings.values.insert(parameter.id.clone(), value.clone());
        } else {
            bindings.invalid_values.insert(parameter.id.clone());
            bindings
                .diagnostics
                .push(CompositionDiagnostic::for_parameter(
                    CompositionDiagnosticCode::ParameterTypeMismatch,
                    &parameter.id,
                ));
        }
    }

    for name in supplied.keys() {
        if !declared.contains(name) {
            bindings
                .diagnostics
                .push(CompositionDiagnostic::for_parameter(
                    CompositionDiagnosticCode::ParameterUnknown,
                    name,
                ));
        }
    }

    bindings
}

/// Resolve a source value without coercion or expression evaluation.
pub fn resolve_value_template(
    template: &CompositionValue,
    bindings: &ParameterBindings,
) -> Result<BoundCompositionValue, CompositionDiagnostic> {
    match template {
        CompositionValue::Literal(value) => Ok(BoundCompositionValue {
            value: value.as_json_value().clone(),
            parameter_type: None,
        }),
        CompositionValue::Parameter(reference) => {
            let Some(parameter_type) = bindings.parameter_types.get(&reference.parameter).copied()
            else {
                return Err(CompositionDiagnostic::without_parameter(
                    CompositionDiagnosticCode::ParameterUnknown,
                ));
            };
            if bindings.invalid_values.contains(&reference.parameter) {
                return Err(CompositionDiagnostic::without_parameter(
                    CompositionDiagnosticCode::ParameterTypeMismatch,
                ));
            }
            let Some(value) = bindings.values.get(&reference.parameter) else {
                return Err(CompositionDiagnostic::without_parameter(
                    CompositionDiagnosticCode::ParameterMissing,
                ));
            };
            Ok(BoundCompositionValue {
                value: value.clone(),
                parameter_type: Some(parameter_type),
            })
        }
    }
}

fn value_matches_parameter(value: &Value, parameter_type: CompositionParameterType) -> bool {
    match parameter_type {
        CompositionParameterType::String => value.is_string(),
        CompositionParameterType::Boolean => value.is_boolean(),
        CompositionParameterType::Integer => value.as_i64().is_some(),
        CompositionParameterType::Float => value.as_f64().is_some_and(f64::is_finite),
        CompositionParameterType::Date => value
            .as_str()
            .is_some_and(|value| parse_date(value).is_some()),
        CompositionParameterType::Timestamp => value.as_str().is_some_and(|value| {
            parse_wall_timestamp(value).is_some() || parse_zoned_timestamp(value).is_some()
        }),
    }
}

fn parse_date(value: &str) -> Option<NaiveDate> {
    let parsed = NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()?;
    (parsed.format("%Y-%m-%d").to_string() == value).then_some(parsed)
}

fn parse_wall_timestamp(value: &str) -> Option<NaiveDateTime> {
    let parsed = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M"))
        .ok()?;
    let seconds = parsed.format("%Y-%m-%dT%H:%M:%S%.f").to_string();
    let minutes = parsed.format("%Y-%m-%dT%H:%M").to_string();
    (seconds == value || minutes == value).then_some(parsed)
}

fn parse_zoned_timestamp(value: &str) -> Option<DateTime<chrono::FixedOffset>> {
    DateTime::parse_from_rfc3339(value).ok()
}

/// An EntryQuery template compiled to the existing bounded query contract.
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledEntryQuery {
    pub request: EntryPageRequest,
    /// SHA-256 over the source Form ID and used field IDs/schema snapshots.
    pub source_schema_fingerprint: String,
}

/// Compile one Composition EntryQuery against a current, already-authorized
/// Form descriptor. This function performs no storage reads and grants no
/// authorization; callers must obtain `current_form` through the current ACL
/// boundary and query execution must recheck authorization independently.
pub fn compile_entry_query_source(
    form_id: FormId,
    field_schema: &[CompositionFieldSchemaEntry],
    template: &EntryQueryTemplate,
    bindings: &ParameterBindings,
    current_form: &FormDefinition,
) -> Result<CompiledEntryQuery, Vec<CompositionDiagnostic>> {
    if current_form.id != form_id || current_form.validate().is_err() {
        return Err(vec![CompositionDiagnostic::without_parameter(
            CompositionDiagnosticCode::SourceUnavailable,
        )]);
    }

    let mut diagnostics = Vec::new();
    let text = template.text.as_ref().and_then(|value| {
        let bound = match resolve_value_template(value, bindings) {
            Ok(bound) => bound,
            Err(diagnostic) => {
                diagnostics.push(diagnostic);
                return None;
            }
        };
        if bound
            .parameter_type
            .is_some_and(|kind| kind != CompositionParameterType::String)
        {
            diagnostics.push(CompositionDiagnostic::without_parameter(
                CompositionDiagnosticCode::ParameterTypeMismatch,
            ));
            return None;
        }
        match bound.value.as_str() {
            Some(text) => Some(text.to_owned()),
            None => {
                diagnostics.push(CompositionDiagnostic::without_parameter(
                    if bound.parameter_type.is_some() {
                        CompositionDiagnosticCode::ParameterTypeMismatch
                    } else {
                        CompositionDiagnosticCode::InvalidComposition
                    },
                ));
                None
            }
        }
    });

    let mut filters = Vec::with_capacity(template.filters.len());
    for filter in &template.filters {
        match resolve_value_template(&filter.value, bindings) {
            Ok(bound) => filters.push((filter, bound)),
            Err(diagnostic) => diagnostics.push(diagnostic),
        }
    }
    if !diagnostics.is_empty() {
        return Err(deduplicate_diagnostics(diagnostics));
    }

    let query = EntryQuery {
        scope: EntryQueryScope::Form { form_id },
        text,
        filters: filters
            .iter()
            .map(|(filter, bound)| EntryFilter {
                field: EntryFieldRef::Property {
                    field_id: filter.field_id,
                },
                operator: map_operator(filter.operator),
                value: bound.value.clone(),
            })
            .collect(),
        sort: template
            .sort
            .iter()
            .map(|sort| EntrySort {
                field: EntryFieldRef::Property {
                    field_id: sort.field_id,
                },
                direction: match sort.direction {
                    CompositionSortDirection::Asc => EntrySortDirection::Asc,
                    CompositionSortDirection::Desc => EntrySortDirection::Desc,
                },
            })
            .collect(),
    };
    let projection = match &template.projection {
        EntryQueryProjectionTemplate::Preview => EntryProjection::Preview,
        EntryQueryProjectionTemplate::Fields { fields } => EntryProjection::Fields {
            fields: fields
                .iter()
                .copied()
                .map(|field_id| EntryFieldRef::Property { field_id })
                .collect(),
        },
    };
    let request = EntryPageRequest {
        query,
        projection,
        limit: template.page_limit,
        after: None,
    };
    if request.validate().is_err() {
        return Err(vec![CompositionDiagnostic::without_parameter(
            CompositionDiagnosticCode::InvalidComposition,
        )]);
    }

    let current_by_id = current_form
        .fields
        .iter()
        .map(|field| (field.id, field))
        .collect::<BTreeMap<_, _>>();
    let expected_by_id = field_schema
        .iter()
        .map(|field| (field.field_id, field))
        .collect::<BTreeMap<_, _>>();
    if expected_by_id.len() != field_schema.len() {
        return Err(vec![CompositionDiagnostic::without_parameter(
            CompositionDiagnosticCode::InvalidComposition,
        )]);
    }

    let mut explicit_fields = BTreeSet::new();
    explicit_fields.extend(filters.iter().map(|(filter, _)| filter.field_id));
    explicit_fields.extend(template.sort.iter().map(|sort| sort.field_id));
    if let EntryProjection::Fields { fields } = &request.projection {
        explicit_fields.extend(fields.iter().filter_map(|field| match field {
            EntryFieldRef::Property { field_id } => Some(*field_id),
            EntryFieldRef::Form | EntryFieldRef::CreatedAt | EntryFieldRef::UpdatedAt => None,
        }));
    }

    let text_fields = request
        .query
        .text
        .as_ref()
        .map(|_| {
            current_form
                .fields
                .iter()
                .filter(|field| entry_query_text_searches_field_type(&field.field_type))
                .map(|field| field.id)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let preview_fields = if matches!(request.projection, EntryProjection::Preview) {
        current_by_id.keys().copied().collect::<BTreeSet<_>>()
    } else {
        BTreeSet::new()
    };

    let mut used_fields = explicit_fields.clone();
    used_fields.extend(text_fields.iter().copied());
    used_fields.extend(preview_fields.iter().copied());

    for (field, bound) in &filters {
        match current_by_id.get(&field.field_id) {
            Some(definition)
                if expected_by_id
                    .get(&field.field_id)
                    .is_some_and(|expected| expected.field_type == definition.field_type) =>
            {
                if !filter_value_matches_field(
                    &bound.value,
                    bound.parameter_type,
                    field.operator,
                    &definition.field_type,
                ) {
                    diagnostics.push(CompositionDiagnostic::without_parameter(
                        if bound.parameter_type.is_some() {
                            CompositionDiagnosticCode::ParameterTypeMismatch
                        } else {
                            CompositionDiagnosticCode::InvalidComposition
                        },
                    ));
                }
            }
            Some(_) => {}
            None => diagnostics.push(CompositionDiagnostic::without_parameter(
                CompositionDiagnosticCode::MissingField,
            )),
        }
    }

    for sort in &template.sort {
        match current_by_id.get(&sort.field_id) {
            Some(definition)
                if expected_by_id
                    .get(&sort.field_id)
                    .is_some_and(|expected| expected.field_type == definition.field_type)
                    && !entry_field_capability(definition).sortable =>
            {
                diagnostics.push(CompositionDiagnostic::without_parameter(
                    CompositionDiagnosticCode::InvalidComposition,
                ));
            }
            Some(_) | None => {}
        }
    }

    for field_id in &explicit_fields {
        if !current_by_id.contains_key(field_id) {
            diagnostics.push(CompositionDiagnostic::without_parameter(
                CompositionDiagnosticCode::MissingField,
            ));
        }
    }

    if !text_fields.is_empty() || request.query.text.is_some() {
        let expected_text_fields = expected_by_id
            .iter()
            .filter(|(_, field)| entry_query_text_searches_field_type(&field.field_type))
            .map(|(field_id, _)| *field_id)
            .collect::<BTreeSet<_>>();
        if expected_text_fields != text_fields {
            diagnostics.push(CompositionDiagnostic::without_parameter(
                CompositionDiagnosticCode::SourceSchemaChanged,
            ));
        }
    }

    if matches!(request.projection, EntryProjection::Preview)
        && expected_by_id.keys().copied().collect::<BTreeSet<_>>() != preview_fields
    {
        diagnostics.push(CompositionDiagnostic::without_parameter(
            CompositionDiagnosticCode::SourceSchemaChanged,
        ));
    }

    for field_id in &used_fields {
        let Some(current) = current_by_id.get(field_id) else {
            continue;
        };
        let Some(expected) = expected_by_id.get(field_id) else {
            diagnostics.push(CompositionDiagnostic::without_parameter(
                CompositionDiagnosticCode::SourceSchemaChanged,
            ));
            continue;
        };
        if current.field_type != expected.field_type {
            diagnostics.push(CompositionDiagnostic::without_parameter(
                CompositionDiagnosticCode::FieldTypeChanged,
            ));
        }
    }

    if !diagnostics.is_empty() {
        return Err(deduplicate_diagnostics(diagnostics));
    }

    let schema_material = used_fields
        .iter()
        .filter_map(|field_id| expected_by_id.get(field_id))
        .collect::<Vec<_>>();
    let schema_bytes = serde_json::to_vec(&(form_id, schema_material)).map_err(|_| {
        vec![CompositionDiagnostic::without_parameter(
            CompositionDiagnosticCode::InvalidComposition,
        )]
    })?;
    Ok(CompiledEntryQuery {
        request,
        source_schema_fingerprint: hex::encode(Sha256::digest(schema_bytes)),
    })
}

fn deduplicate_diagnostics(diagnostics: Vec<CompositionDiagnostic>) -> Vec<CompositionDiagnostic> {
    let mut seen = BTreeSet::new();
    diagnostics
        .into_iter()
        .filter(|diagnostic| {
            seen.insert((
                diagnostic.code.as_str().to_owned(),
                diagnostic.parameter_id.clone(),
            ))
        })
        .collect()
}

fn map_operator(operator: CompositionQueryOperator) -> SearchOperator {
    match operator {
        CompositionQueryOperator::Equals => SearchOperator::Equals,
        CompositionQueryOperator::Contains => SearchOperator::Contains,
        CompositionQueryOperator::Lt => SearchOperator::Lt,
        CompositionQueryOperator::Lte => SearchOperator::Lte,
        CompositionQueryOperator::Gt => SearchOperator::Gt,
        CompositionQueryOperator::Gte => SearchOperator::Gte,
    }
}

fn filter_value_matches_field(
    value: &Value,
    parameter_type: Option<CompositionParameterType>,
    operator: CompositionQueryOperator,
    field_type: &FieldType,
) -> bool {
    let Some(kind) = EntryQueryFieldKind::of(field_type) else {
        return false;
    };
    if !kind.supports(map_operator(operator)) {
        return false;
    }
    if value.is_null() {
        return operator == CompositionQueryOperator::Equals && parameter_type.is_none();
    }

    let expected_type = match field_type {
        FieldType::String | FieldType::Markdown => CompositionParameterType::String,
        FieldType::Boolean => CompositionParameterType::Boolean,
        FieldType::Integer | FieldType::Long => CompositionParameterType::Integer,
        FieldType::Float | FieldType::Double => CompositionParameterType::Float,
        FieldType::Date => CompositionParameterType::Date,
        FieldType::Timestamp
        | FieldType::TimestampTz
        | FieldType::TimestampNs
        | FieldType::TimestampTzNs => CompositionParameterType::Timestamp,
        FieldType::Sql
        | FieldType::Time
        | FieldType::Uuid
        | FieldType::Binary
        | FieldType::List
        | FieldType::ObjectList
        | FieldType::RowReference
        | FieldType::AssetReference => return false,
    };
    if parameter_type.is_some_and(|actual| actual != expected_type) {
        return false;
    }
    if operator == CompositionQueryOperator::Contains {
        return value.is_string();
    }
    match field_type {
        FieldType::String | FieldType::Markdown => value.is_string(),
        FieldType::Boolean => value.is_boolean(),
        FieldType::Integer => value
            .as_i64()
            .is_some_and(|value| i32::try_from(value).is_ok()),
        FieldType::Long => value.as_i64().is_some(),
        FieldType::Float => value
            .as_f64()
            .is_some_and(|value| value.is_finite() && (value as f32).is_finite()),
        FieldType::Double => value.as_f64().is_some_and(f64::is_finite),
        FieldType::Date => value
            .as_str()
            .is_some_and(|value| parse_date(value).is_some()),
        FieldType::Timestamp => value
            .as_str()
            .is_some_and(|value| parse_wall_timestamp(value).is_some()),
        FieldType::TimestampNs => value.as_str().is_some_and(|value| {
            parse_wall_timestamp(value).is_some_and(wall_timestamp_nanos_are_representable)
        }),
        FieldType::TimestampTz => value
            .as_str()
            .is_some_and(|value| parse_zoned_timestamp(value).is_some()),
        FieldType::TimestampTzNs => value.as_str().is_some_and(|value| {
            parse_zoned_timestamp(value)
                .is_some_and(|timestamp| timestamp.timestamp_nanos_opt().is_some())
        }),
        FieldType::Sql
        | FieldType::Time
        | FieldType::Uuid
        | FieldType::Binary
        | FieldType::List
        | FieldType::ObjectList
        | FieldType::RowReference
        | FieldType::AssetReference => false,
    }
}

fn wall_timestamp_nanos_are_representable(timestamp: NaiveDateTime) -> bool {
    let Some(epoch) =
        NaiveDate::from_ymd_opt(1970, 1, 1).and_then(|date| date.and_hms_opt(0, 0, 0))
    else {
        return false;
    };
    timestamp
        .signed_duration_since(epoch)
        .num_nanoseconds()
        .is_some()
}

#[cfg(test)]
mod tests {
    use super::{
        bind_parameters, compile_entry_query_source, resolve_value_template, CompositionDiagnostic,
        ParameterBindings,
    };
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    use ugoite_domain::composition::{
        CompositionDiagnosticCode, CompositionFieldSchemaEntry, CompositionLiteral,
        CompositionParameter, CompositionParameterFormat, CompositionParameterReference,
        CompositionParameterType, CompositionQueryOperator, CompositionSortDirection,
        CompositionValue, EntryQueryFilterTemplate, EntryQueryProjectionTemplate,
        EntryQuerySortTemplate, EntryQueryTemplate,
    };
    use ugoite_domain::form::{FieldType, FormDefinition, FormField, FormVersion};
    use ugoite_domain::id::{FieldId, FormId};

    fn parameter(
        id: &str,
        parameter_type: CompositionParameterType,
        required: bool,
        default: Option<CompositionLiteral>,
        format: Option<CompositionParameterFormat>,
    ) -> CompositionParameter {
        CompositionParameter {
            id: id.to_owned(),
            parameter_type,
            required,
            default,
            format,
        }
    }

    fn parameter_ref(id: &str) -> CompositionValue {
        CompositionValue::Parameter(CompositionParameterReference {
            parameter: id.to_owned(),
        })
    }

    fn literal(value: Value) -> CompositionLiteral {
        serde_json::from_value(value).unwrap()
    }

    fn value(value: Value) -> CompositionValue {
        CompositionValue::Literal(literal(value))
    }

    fn form(fields: &[(i32, FieldType)]) -> FormDefinition {
        FormDefinition {
            id: FormId::from(uuid::Uuid::from_u128(42)),
            version: FormVersion::new(1).unwrap(),
            name: "Example".to_owned(),
            description: None,
            fields: fields
                .iter()
                .map(|(id, field_type)| FormField {
                    id: FieldId::new(*id).unwrap(),
                    name: format!("field_{id}"),
                    field_type: field_type.clone(),
                    required: false,
                    label: None,
                    description: None,
                    semantic_role: None,
                    reference_form: None,
                    list_item: None,
                    validation: None,
                    enum_values: Vec::new(),
                    deprecated: false,
                })
                .collect(),
            allow_extra_attributes: false,
            extension_metadata: BTreeMap::new(),
        }
    }

    fn schema(form: &FormDefinition) -> Vec<CompositionFieldSchemaEntry> {
        form.fields
            .iter()
            .map(|field| CompositionFieldSchemaEntry {
                field_id: field.id,
                field_type: field.field_type.clone(),
            })
            .collect()
    }

    fn empty_bindings() -> ParameterBindings {
        ParameterBindings::default()
    }

    fn diagnostic_codes(
        result: Result<super::CompiledEntryQuery, Vec<CompositionDiagnostic>>,
    ) -> Vec<CompositionDiagnosticCode> {
        result
            .expect_err("composition query should be rejected")
            .into_iter()
            .map(|diagnostic| diagnostic.code)
            .collect()
    }

    #[test]
    fn defaults_are_bound_without_applying_format_hints() {
        let parameters = [parameter(
            "month_start",
            CompositionParameterType::Date,
            true,
            Some(literal(json!("2026-10-01"))),
            Some(CompositionParameterFormat::YearMonth),
        )];

        let bindings = bind_parameters(&parameters, &BTreeMap::new());

        assert!(bindings.diagnostics.is_empty());
        assert_eq!(bindings.values["month_start"], json!("2026-10-01"));
        let bound = resolve_value_template(&parameter_ref("month_start"), &bindings).unwrap();
        assert_eq!(bound.value, json!("2026-10-01"));
        assert_eq!(bound.parameter_type, Some(CompositionParameterType::Date));
    }

    #[test]
    fn required_missing_and_unknown_names_have_stable_order() {
        let parameters = [
            parameter("start", CompositionParameterType::Date, true, None, None),
            parameter(
                "optional",
                CompositionParameterType::String,
                false,
                None,
                None,
            ),
        ];
        let supplied = BTreeMap::from([
            ("unknown_z".to_owned(), json!(1)),
            ("unknown_a".to_owned(), json!(2)),
        ]);

        let bindings = bind_parameters(&parameters, &supplied);

        assert_eq!(
            bindings.diagnostics,
            vec![
                super::CompositionDiagnostic {
                    code: CompositionDiagnosticCode::ParameterMissing,
                    parameter_id: Some("start".to_owned()),
                },
                super::CompositionDiagnostic {
                    code: CompositionDiagnosticCode::ParameterUnknown,
                    parameter_id: Some("unknown_a".to_owned()),
                },
                super::CompositionDiagnostic {
                    code: CompositionDiagnosticCode::ParameterUnknown,
                    parameter_id: Some("unknown_z".to_owned()),
                },
            ]
        );
    }

    #[test]
    fn invalid_supplied_value_does_not_fall_back_to_a_default() {
        let parameters = [parameter(
            "limit",
            CompositionParameterType::Integer,
            true,
            Some(literal(json!(5))),
            None,
        )];
        let supplied = BTreeMap::from([("limit".to_owned(), json!("five"))]);

        let bindings = bind_parameters(&parameters, &supplied);

        assert_eq!(
            bindings.diagnostics,
            vec![super::CompositionDiagnostic {
                code: CompositionDiagnosticCode::ParameterTypeMismatch,
                parameter_id: Some("limit".to_owned()),
            }]
        );
        assert!(!bindings.values.contains_key("limit"));
        assert!(bindings.invalid_values.contains("limit"));
        let reference_error =
            resolve_value_template(&parameter_ref("limit"), &bindings).unwrap_err();
        assert_eq!(
            reference_error.code,
            CompositionDiagnosticCode::ParameterTypeMismatch
        );
        assert_eq!(reference_error.parameter_id, None);
    }

    #[test]
    fn invalid_default_is_rejected_even_when_caller_supplies_a_valid_value() {
        let parameters = [parameter(
            "limit",
            CompositionParameterType::Integer,
            true,
            Some(literal(json!("not an integer"))),
            None,
        )];
        let supplied = BTreeMap::from([("limit".to_owned(), json!(5))]);

        let bindings = bind_parameters(&parameters, &supplied);

        assert_eq!(
            bindings.diagnostics,
            vec![super::CompositionDiagnostic {
                code: CompositionDiagnosticCode::InvalidComposition,
                parameter_id: None,
            }]
        );
        assert!(bindings.values.is_empty());
        assert!(bindings.invalid_values.is_empty());
    }

    #[test]
    fn optional_unset_values_are_missing_only_when_referenced() {
        let parameters = [parameter(
            "search_text",
            CompositionParameterType::String,
            false,
            None,
            None,
        )];
        let bindings = bind_parameters(&parameters, &BTreeMap::new());

        assert!(bindings.diagnostics.is_empty());
        let error = resolve_value_template(&parameter_ref("search_text"), &bindings).unwrap_err();
        assert_eq!(error.code, CompositionDiagnosticCode::ParameterMissing);
        assert_eq!(error.parameter_id, None);
    }

    #[test]
    fn undeclared_reference_is_unknown_and_literal_is_passed_through() {
        let bindings = bind_parameters(&[], &BTreeMap::new());

        let error = resolve_value_template(&parameter_ref("typo"), &bindings).unwrap_err();
        assert_eq!(error.code, CompositionDiagnosticCode::ParameterUnknown);
        assert_eq!(error.parameter_id, None);

        let literal_template = CompositionValue::Literal(literal(json!("unchanged")));
        assert_eq!(
            resolve_value_template(&literal_template, &bindings)
                .unwrap()
                .value,
            json!("unchanged")
        );
    }

    #[test]
    fn invalid_date_timestamp_and_float_values_are_not_coerced() {
        let parameters = [
            parameter("date", CompositionParameterType::Date, false, None, None),
            parameter(
                "timestamp",
                CompositionParameterType::Timestamp,
                false,
                None,
                None,
            ),
            parameter("float", CompositionParameterType::Float, false, None, None),
        ];
        let supplied = BTreeMap::from([
            ("date".to_owned(), json!("2026-02-30")),
            ("timestamp".to_owned(), json!("next Tuesday")),
            ("float".to_owned(), json!("1.0")),
        ]);

        let bindings = bind_parameters(&parameters, &supplied);

        assert_eq!(
            bindings.diagnostics,
            vec![
                super::CompositionDiagnostic {
                    code: CompositionDiagnosticCode::ParameterTypeMismatch,
                    parameter_id: Some("date".to_owned()),
                },
                super::CompositionDiagnostic {
                    code: CompositionDiagnosticCode::ParameterTypeMismatch,
                    parameter_id: Some("timestamp".to_owned()),
                },
                super::CompositionDiagnostic {
                    code: CompositionDiagnosticCode::ParameterTypeMismatch,
                    parameter_id: Some("float".to_owned()),
                },
            ]
        );
    }

    #[test]
    fn signed_integer_range_and_timestamp_forms_are_typed_without_coercion() {
        let parameters = [
            parameter(
                "signed",
                CompositionParameterType::Integer,
                true,
                None,
                None,
            ),
            parameter(
                "wall_time",
                CompositionParameterType::Timestamp,
                true,
                None,
                None,
            ),
            parameter(
                "zoned_time",
                CompositionParameterType::Timestamp,
                true,
                None,
                None,
            ),
            parameter("ratio", CompositionParameterType::Float, true, None, None),
            parameter(
                "whole_number_ratio",
                CompositionParameterType::Float,
                true,
                None,
                None,
            ),
        ];
        let supplied = BTreeMap::from([
            ("signed".to_owned(), json!(i64::MAX)),
            ("wall_time".to_owned(), json!("2026-10-02T12:30:00.125")),
            ("zoned_time".to_owned(), json!("2026-10-02T12:30:00+09:00")),
            ("ratio".to_owned(), json!(1.25)),
            ("whole_number_ratio".to_owned(), json!(1)),
        ]);

        let bindings = bind_parameters(&parameters, &supplied);

        assert!(bindings.diagnostics.is_empty());
        assert_eq!(bindings.values, supplied);
    }

    #[test]
    fn duplicate_declarations_are_invalid_composition() {
        let parameters = [
            parameter("same", CompositionParameterType::String, true, None, None),
            parameter("same", CompositionParameterType::String, true, None, None),
        ];

        let bindings = bind_parameters(&parameters, &BTreeMap::new());

        assert_eq!(bindings.diagnostics.len(), 1);
        assert_eq!(
            bindings.diagnostics[0].code,
            CompositionDiagnosticCode::InvalidComposition
        );
        assert_eq!(bindings.diagnostics[0].parameter_id, None);
    }

    #[test]
    fn entry_query_template_compiles_to_the_existing_bounded_request() {
        let form = form(&[
            (100, FieldType::String),
            (101, FieldType::Integer),
            (102, FieldType::Boolean),
        ]);
        let parameters = [
            parameter(
                "search",
                CompositionParameterType::String,
                false,
                None,
                None,
            ),
            parameter(
                "minimum",
                CompositionParameterType::Integer,
                true,
                None,
                None,
            ),
        ];
        let bindings = bind_parameters(
            &parameters,
            &BTreeMap::from([
                ("search".to_owned(), json!("rent")),
                ("minimum".to_owned(), json!(10)),
            ]),
        );
        let template = EntryQueryTemplate {
            text: Some(parameter_ref("search")),
            filters: vec![EntryQueryFilterTemplate {
                field_id: FieldId::new(101).unwrap(),
                operator: CompositionQueryOperator::Gte,
                value: parameter_ref("minimum"),
            }],
            sort: vec![EntryQuerySortTemplate {
                field_id: FieldId::new(101).unwrap(),
                direction: CompositionSortDirection::Desc,
            }],
            page_limit: 25,
            projection: EntryQueryProjectionTemplate::Fields {
                fields: vec![FieldId::new(100).unwrap(), FieldId::new(101).unwrap()],
            },
        };

        let compiled =
            compile_entry_query_source(form.id, &schema(&form), &template, &bindings, &form)
                .expect("EntryQuery template compiles");

        assert_eq!(compiled.request.limit, 25);
        assert_eq!(compiled.request.after, None);
        assert_eq!(compiled.request.query.text.as_deref(), Some("rent"));
        assert_eq!(
            compiled.request.query.scope,
            crate::entry_query::EntryQueryScope::Form { form_id: form.id }
        );
        assert_eq!(compiled.request.query.filters.len(), 1);
        assert_eq!(compiled.request.query.filters[0].value, json!(10));
        assert_eq!(compiled.request.query.sort.len(), 1);
        assert_eq!(
            compiled.request.projection,
            crate::entry_query::EntryProjection::Fields {
                fields: vec![
                    crate::entry_query::EntryFieldRef::Property {
                        field_id: FieldId::new(100).unwrap()
                    },
                    crate::entry_query::EntryFieldRef::Property {
                        field_id: FieldId::new(101).unwrap()
                    },
                ]
            }
        );
        assert_eq!(compiled.source_schema_fingerprint.len(), 64);
    }

    #[test]
    fn entry_query_fingerprint_ignores_unreferenced_schema_fields() {
        let current = form(&[
            (100, FieldType::String),
            (101, FieldType::Integer),
            (102, FieldType::Boolean),
        ]);
        let changed_unrelated = form(&[
            (100, FieldType::String),
            (101, FieldType::Integer),
            (102, FieldType::Double),
        ]);
        let expected = schema(&current);
        let template = EntryQueryTemplate {
            text: None,
            filters: vec![],
            sort: vec![],
            page_limit: 10,
            projection: EntryQueryProjectionTemplate::Fields {
                fields: vec![FieldId::new(100).unwrap()],
            },
        };

        let first = compile_entry_query_source(
            current.id,
            &expected,
            &template,
            &empty_bindings(),
            &current,
        )
        .expect("first schema compiles");
        let second = compile_entry_query_source(
            changed_unrelated.id,
            &expected,
            &template,
            &empty_bindings(),
            &changed_unrelated,
        )
        .expect("unrelated schema change does not invalidate projection");

        assert_eq!(
            first.source_schema_fingerprint,
            second.source_schema_fingerprint
        );

        let mut same_schema_different_form = current.clone();
        same_schema_different_form.id = FormId::from(uuid::Uuid::from_u128(43));
        let third = compile_entry_query_source(
            same_schema_different_form.id,
            &expected,
            &template,
            &empty_bindings(),
            &same_schema_different_form,
        )
        .expect("same schema in a different Form compiles");
        assert_ne!(
            first.source_schema_fingerprint,
            third.source_schema_fingerprint
        );
    }

    #[test]
    fn text_search_fingerprints_only_fields_searched_by_entry_query() {
        let current = form(&[
            (100, FieldType::String),
            (101, FieldType::Binary),
            (102, FieldType::List),
        ]);
        let expected = vec![CompositionFieldSchemaEntry {
            field_id: FieldId::new(100).unwrap(),
            field_type: FieldType::String,
        }];
        let template = EntryQueryTemplate {
            text: Some(value(json!("hello"))),
            filters: vec![],
            sort: vec![],
            page_limit: 10,
            projection: EntryQueryProjectionTemplate::Fields {
                fields: vec![FieldId::new(100).unwrap()],
            },
        };

        let compiled = compile_entry_query_source(
            current.id,
            &expected,
            &template,
            &empty_bindings(),
            &current,
        )
        .expect("binary and list fields are not used by EntryQuery text search");
        assert_eq!(compiled.source_schema_fingerprint.len(), 64);
    }

    #[test]
    fn new_text_search_field_changes_source_schema() {
        let current = form(&[(100, FieldType::String), (101, FieldType::String)]);
        let expected = vec![CompositionFieldSchemaEntry {
            field_id: FieldId::new(100).unwrap(),
            field_type: FieldType::String,
        }];
        let template = EntryQueryTemplate {
            text: Some(value(json!("hello"))),
            filters: vec![],
            sort: vec![],
            page_limit: 10,
            projection: EntryQueryProjectionTemplate::Fields {
                fields: vec![FieldId::new(100).unwrap()],
            },
        };

        assert_eq!(
            diagnostic_codes(compile_entry_query_source(
                current.id,
                &expected,
                &template,
                &empty_bindings(),
                &current,
            )),
            vec![CompositionDiagnosticCode::SourceSchemaChanged]
        );
    }

    #[test]
    fn changed_or_missing_query_field_has_a_stable_diagnostic() {
        let original = form(&[(100, FieldType::Integer)]);
        let changed = form(&[(100, FieldType::Long)]);
        let removed = form(&[]);
        let expected = schema(&original);
        let template = EntryQueryTemplate {
            text: None,
            filters: vec![EntryQueryFilterTemplate {
                field_id: FieldId::new(100).unwrap(),
                operator: CompositionQueryOperator::Equals,
                value: value(json!(1)),
            }],
            sort: vec![],
            page_limit: 10,
            projection: EntryQueryProjectionTemplate::Fields {
                fields: vec![FieldId::new(100).unwrap()],
            },
        };

        assert_eq!(
            diagnostic_codes(compile_entry_query_source(
                changed.id,
                &expected,
                &template,
                &empty_bindings(),
                &changed,
            )),
            vec![CompositionDiagnosticCode::FieldTypeChanged]
        );
        assert_eq!(
            diagnostic_codes(compile_entry_query_source(
                removed.id,
                &expected,
                &template,
                &empty_bindings(),
                &removed,
            )),
            vec![CompositionDiagnosticCode::MissingField]
        );
    }

    #[test]
    fn query_parameter_and_literal_types_must_match_filter_fields() {
        let form = form(&[(100, FieldType::Integer)]);
        let expected = schema(&form);
        let parameters = [parameter(
            "value",
            CompositionParameterType::String,
            true,
            None,
            None,
        )];
        let bindings = bind_parameters(
            &parameters,
            &BTreeMap::from([("value".to_owned(), json!("1"))]),
        );
        let filter_template = |value| EntryQueryTemplate {
            text: None,
            filters: vec![EntryQueryFilterTemplate {
                field_id: FieldId::new(100).unwrap(),
                operator: CompositionQueryOperator::Equals,
                value,
            }],
            sort: vec![],
            page_limit: 10,
            projection: EntryQueryProjectionTemplate::Fields {
                fields: vec![FieldId::new(100).unwrap()],
            },
        };

        assert_eq!(
            diagnostic_codes(compile_entry_query_source(
                form.id,
                &expected,
                &filter_template(parameter_ref("value")),
                &bindings,
                &form,
            )),
            vec![CompositionDiagnosticCode::ParameterTypeMismatch]
        );
        assert_eq!(
            diagnostic_codes(compile_entry_query_source(
                form.id,
                &expected,
                &filter_template(value(json!("1"))),
                &empty_bindings(),
                &form,
            )),
            vec![CompositionDiagnosticCode::InvalidComposition]
        );
    }

    #[test]
    fn query_sort_rejects_fields_outside_entry_query_capabilities() {
        for (field_id, field_type) in [
            (100, FieldType::Binary),
            (102, FieldType::List),
            (103, FieldType::ObjectList),
            (104, FieldType::AssetReference),
        ] {
            let form = form(&[(field_id, field_type), (101, FieldType::String)]);
            let template = EntryQueryTemplate {
                text: None,
                filters: vec![],
                sort: vec![EntryQuerySortTemplate {
                    field_id: FieldId::new(field_id).unwrap(),
                    direction: CompositionSortDirection::Asc,
                }],
                page_limit: 10,
                projection: EntryQueryProjectionTemplate::Fields {
                    fields: vec![FieldId::new(101).unwrap()],
                },
            };

            assert_eq!(
                diagnostic_codes(compile_entry_query_source(
                    form.id,
                    &schema(&form),
                    &template,
                    &empty_bindings(),
                    &form,
                )),
                vec![CompositionDiagnosticCode::InvalidComposition]
            );
        }
    }

    #[test]
    fn nanosecond_timestamp_filters_reject_unrepresentable_values_at_resolve_time() {
        for (field_type, timestamp) in [
            (FieldType::TimestampNs, "0001-01-01T00:00:00"),
            (FieldType::TimestampTzNs, "0001-01-01T00:00:00Z"),
        ] {
            let form = form(&[(100, field_type)]);
            let template = EntryQueryTemplate {
                text: None,
                filters: vec![EntryQueryFilterTemplate {
                    field_id: FieldId::new(100).unwrap(),
                    operator: CompositionQueryOperator::Equals,
                    value: value(json!(timestamp)),
                }],
                sort: vec![],
                page_limit: 10,
                projection: EntryQueryProjectionTemplate::Fields {
                    fields: vec![FieldId::new(100).unwrap()],
                },
            };

            assert_eq!(
                diagnostic_codes(compile_entry_query_source(
                    form.id,
                    &schema(&form),
                    &template,
                    &empty_bindings(),
                    &form,
                )),
                vec![CompositionDiagnosticCode::InvalidComposition]
            );
        }
    }

    #[test]
    fn minute_precision_timestamp_matches_entry_query_filter_syntax() {
        let parameters = [parameter(
            "at",
            CompositionParameterType::Timestamp,
            true,
            None,
            None,
        )];
        let supplied = BTreeMap::from([("at".to_owned(), json!("2026-10-02T12:30"))]);
        let bindings = bind_parameters(&parameters, &supplied);
        assert!(bindings.diagnostics.is_empty());

        let form = form(&[(100, FieldType::TimestampNs)]);
        let template = EntryQueryTemplate {
            text: None,
            filters: vec![EntryQueryFilterTemplate {
                field_id: FieldId::new(100).unwrap(),
                operator: CompositionQueryOperator::Equals,
                value: parameter_ref("at"),
            }],
            sort: vec![],
            page_limit: 10,
            projection: EntryQueryProjectionTemplate::Fields {
                fields: vec![FieldId::new(100).unwrap()],
            },
        };

        let compiled =
            compile_entry_query_source(form.id, &schema(&form), &template, &bindings, &form)
                .expect("minute-precision timestamps are accepted by EntryQuery");
        assert_eq!(
            compiled.request.query.filters[0].value,
            json!("2026-10-02T12:30")
        );
    }

    #[test]
    fn composition_query_limits_are_enforced_by_entry_query() {
        let form = form(&[(100, FieldType::String)]);
        let base = EntryQueryTemplate {
            text: None,
            filters: vec![],
            sort: vec![],
            page_limit: 10,
            projection: EntryQueryProjectionTemplate::Fields {
                fields: vec![FieldId::new(100).unwrap()],
            },
        };
        let mut over_limit = Vec::new();

        let mut too_much_text = base.clone();
        too_much_text.text = Some(value(json!(
            "x".repeat(crate::entry_query::MAX_ENTRY_QUERY_TEXT_BYTES + 1)
        )));
        over_limit.push(too_much_text);

        let mut too_many_filters = base.clone();
        too_many_filters.filters = (0..=crate::entry_query::MAX_ENTRY_FILTERS)
            .map(|_| EntryQueryFilterTemplate {
                field_id: FieldId::new(100).unwrap(),
                operator: CompositionQueryOperator::Equals,
                value: value(json!("value")),
            })
            .collect();
        over_limit.push(too_many_filters);

        let mut too_many_sorts = base.clone();
        too_many_sorts.sort = (0..=crate::entry_query::MAX_ENTRY_SORTS)
            .map(|_| EntryQuerySortTemplate {
                field_id: FieldId::new(100).unwrap(),
                direction: CompositionSortDirection::Asc,
            })
            .collect();
        over_limit.push(too_many_sorts);

        let mut too_many_projection_fields = base.clone();
        too_many_projection_fields.projection = EntryQueryProjectionTemplate::Fields {
            fields: (0..=crate::entry_query::MAX_ENTRY_PROJECTION_FIELDS)
                .map(|_| FieldId::new(100).unwrap())
                .collect(),
        };
        over_limit.push(too_many_projection_fields);

        let mut too_large_page = base;
        too_large_page.page_limit = crate::entry_query::MAX_ENTRY_PAGE_LIMIT + 1;
        over_limit.push(too_large_page);

        for template in over_limit {
            assert_eq!(
                diagnostic_codes(compile_entry_query_source(
                    form.id,
                    &schema(&form),
                    &template,
                    &empty_bindings(),
                    &form,
                )),
                vec![CompositionDiagnosticCode::InvalidComposition]
            );
        }
    }
}
