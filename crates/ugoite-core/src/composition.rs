//! Pure Composition parameter binding and source compilation.
//!
//! This module resolves typed Composition values into the existing bounded
//! EntryQuery and stateless SQL query contracts. It performs no storage reads,
//! grants no authorization, and creates no persistent query state.

use crate::entry_query::{
    entry_field_capability, entry_query_text_searches_field_type, EntryFieldRef, EntryFilter,
    EntryPage, EntryPageRequest, EntryProjection, EntryQuery, EntryQueryFieldKind, EntryQueryScope,
    EntrySort, EntrySortDirection, SearchOperator,
};
use crate::sql_query::{SavedSqlRevisionRef, SqlQueryPage, SqlQueryRequest};
use chrono::{DateTime, NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use ugoite_domain::composition::{
    CompositionComponent, CompositionDiagnosticCode, CompositionFieldSchemaEntry,
    CompositionLiteral, CompositionMetricValueField, CompositionParameter,
    CompositionParameterType, CompositionQueryOperator, CompositionResultFieldType,
    CompositionSortDirection, CompositionSource, CompositionSpec, CompositionValue,
    EntryQueryProjectionTemplate, EntryQueryTemplate, DEFAULT_COMPOSITION_PAGE_LIMIT,
};
use ugoite_domain::composition_metric::{
    composition_metric_result_type, evaluate_composition_metric_page,
    validate_composition_metric_result_type, CompositionMetricDiagnosticCode,
    CompositionMetricPage,
};
use ugoite_domain::form::{FieldType, FormDefinition};
use ugoite_domain::id::{EntryId, FieldId, FormId, RevisionId};

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
    compile_entry_query_source_with_metric_fields(
        form_id,
        field_schema,
        template,
        bindings,
        current_form,
        &[],
    )
}

fn compile_entry_query_source_with_metric_fields(
    form_id: FormId,
    field_schema: &[CompositionFieldSchemaEntry],
    template: &EntryQueryTemplate,
    bindings: &ParameterBindings,
    current_form: &FormDefinition,
    metric_fields: &[FieldId],
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
    let preview_requested = matches!(template.projection, EntryQueryProjectionTemplate::Preview);
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

    if !metric_fields.is_empty() {
        let projected_fields = match &request.projection {
            EntryProjection::Preview => None,
            EntryProjection::Fields { fields } => Some(fields),
        };
        if metric_fields.iter().any(|metric_field| {
            !projected_fields.is_some_and(|fields| {
                fields.contains(&EntryFieldRef::Property {
                    field_id: *metric_field,
                })
            })
        }) {
            return Err(vec![CompositionDiagnostic::without_parameter(
                CompositionDiagnosticCode::MetricFieldNotProjected,
            )]);
        }
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
    let preview_fields = if preview_requested {
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

    if preview_requested
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
        } else if current.reference_form != expected.reference_form
            || current.list_item != expected.list_item
        {
            diagnostics.push(CompositionDiagnostic::without_parameter(
                CompositionDiagnosticCode::SourceSchemaChanged,
            ));
        }
    }

    if !diagnostics.is_empty() {
        return Err(deduplicate_diagnostics(diagnostics));
    }

    let schema_material = used_fields
        .iter()
        .filter_map(|field_id| current_by_id.get(field_id))
        .map(|field| CompositionFieldSchemaEntry {
            field_id: field.id,
            field_type: field.field_type.clone(),
            reference_form: field.reference_form,
            list_item: field.list_item.clone(),
        })
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

/// Current metadata for one exact Saved SQL revision, projected by the
/// authorized storage boundary into the core resolver's transport-neutral
/// input. SQL text and variable descriptions are deliberately not included.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SavedSqlRevisionMetadata {
    pub id: String,
    pub revision_id: String,
    pub variable_types: BTreeMap<String, String>,
}

/// An exact Saved SQL source compiled to the existing stateless SQL request.
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledSavedSqlSource {
    pub request: SqlQueryRequest,
    /// SHA-256 over the exact revision identity, ordered result schema, used
    /// variable schema, and selected metric columns when present.
    pub source_schema_fingerprint: String,
}

/// Compile a Composition Saved SQL source against metadata read for its exact
/// current revision. The caller must obtain the descriptor through the
/// current ACL boundary. A missing descriptor (including a concealed denied
/// read) produces one stable unavailable-revision diagnostic. SQL execution
/// remains in the existing query path, which rechecks current authorization.
pub fn compile_saved_sql_source(
    source: &CompositionSource,
    current_revision: Option<&SavedSqlRevisionMetadata>,
    bindings: &ParameterBindings,
) -> Result<CompiledSavedSqlSource, Vec<CompositionDiagnostic>> {
    compile_saved_sql_source_with_metric_columns(source, current_revision, bindings, &[])
}

fn compile_saved_sql_source_with_metric_columns(
    source: &CompositionSource,
    current_revision: Option<&SavedSqlRevisionMetadata>,
    bindings: &ParameterBindings,
    selected_metric_columns: &[String],
) -> Result<CompiledSavedSqlSource, Vec<CompositionDiagnostic>> {
    let CompositionSource::SavedSql {
        entry_id,
        revision_id,
        expected_result,
        variables,
        ..
    } = source
    else {
        return Err(vec![CompositionDiagnostic::without_parameter(
            CompositionDiagnosticCode::InvalidComposition,
        )]);
    };

    let Some(current_revision) = current_revision else {
        return Err(vec![CompositionDiagnostic::without_parameter(
            CompositionDiagnosticCode::SourceUnavailable,
        )]);
    };

    let expected_id = entry_id.to_string();
    let expected_revision_id = revision_id.to_string();
    if current_revision.id != expected_id || current_revision.revision_id != expected_revision_id {
        return Err(vec![CompositionDiagnostic::without_parameter(
            CompositionDiagnosticCode::SourceUnavailable,
        )]);
    }

    let mut diagnostics = Vec::new();
    let mut parameters = serde_json::Map::new();
    let mut parameter_types = BTreeMap::new();
    let mut used_variable_types = BTreeMap::new();

    for (variable_name, value_template) in variables {
        let Some(variable_type) = current_revision.variable_types.get(variable_name) else {
            diagnostics.push(CompositionDiagnostic::without_parameter(
                CompositionDiagnosticCode::SourceSchemaChanged,
            ));
            continue;
        };
        let Some(parameter_type) = composition_parameter_type_for_sql(variable_type) else {
            diagnostics.push(CompositionDiagnostic::without_parameter(
                CompositionDiagnosticCode::SourceSchemaChanged,
            ));
            continue;
        };
        let bound = match resolve_value_template(value_template, bindings) {
            Ok(bound) => bound,
            Err(diagnostic) => {
                diagnostics.push(diagnostic);
                continue;
            }
        };
        if bound
            .parameter_type
            .is_some_and(|bound_type| bound_type != parameter_type)
        {
            diagnostics.push(CompositionDiagnostic::without_parameter(
                CompositionDiagnosticCode::ParameterTypeMismatch,
            ));
            continue;
        }
        // SQL query binding uses the descriptor's declared type to preserve a
        // typed NULL; non-null literals and named parameters still require an
        // exact Composition type/value match.
        if (bound.value.is_null() && bound.parameter_type.is_some())
            || (!bound.value.is_null() && !value_matches_parameter(&bound.value, parameter_type))
        {
            diagnostics.push(CompositionDiagnostic::without_parameter(
                if bound.parameter_type.is_some() {
                    CompositionDiagnosticCode::ParameterTypeMismatch
                } else {
                    CompositionDiagnosticCode::InvalidComposition
                },
            ));
            continue;
        }

        parameters.insert(variable_name.clone(), bound.value);
        parameter_types.insert(variable_name.clone(), variable_type.clone());
        used_variable_types.insert(variable_name.clone(), variable_type.clone());
    }

    if current_revision
        .variable_types
        .keys()
        .any(|name| !variables.contains_key(name))
    {
        diagnostics.push(CompositionDiagnostic::without_parameter(
            CompositionDiagnosticCode::ParameterMissing,
        ));
    }

    if !diagnostics.is_empty() {
        return Err(deduplicate_diagnostics(diagnostics));
    }

    let saved_sql = SavedSqlRevisionRef {
        id: expected_id,
        revision_id: expected_revision_id,
    };
    let fingerprint_material = serde_json::to_vec(&(
        &saved_sql,
        expected_result,
        &used_variable_types,
        selected_metric_columns,
    ))
    .map_err(|_| {
        vec![CompositionDiagnostic::without_parameter(
            CompositionDiagnosticCode::InvalidComposition,
        )]
    })?;
    let request = SqlQueryRequest {
        // The SQL text is resolved by the existing executor from this exact
        // revision reference. Sending a client copy would create a second
        // authority for the query text.
        sql: String::new(),
        parameters,
        parameter_types,
        limit: DEFAULT_COMPOSITION_PAGE_LIMIT,
        continuation: None,
        saved_sql: Some(saved_sql),
    };
    if request.validate().is_err() {
        return Err(vec![CompositionDiagnostic::without_parameter(
            CompositionDiagnosticCode::InvalidComposition,
        )]);
    }

    Ok(CompiledSavedSqlSource {
        request,
        source_schema_fingerprint: hex::encode(Sha256::digest(fingerprint_material)),
    })
}

/// Identity of the exact Composition revision being resolved.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionRevisionRef {
    pub entry_id: EntryId,
    pub revision_id: RevisionId,
}

/// Current source metadata obtained through the caller's current ACL boundary.
/// `None` represents either absence or a concealed denial and compiles to the
/// same `source_unavailable` diagnostic.
pub enum CurrentSourceDescriptor<'a> {
    EntryQuery {
        source_id: &'a str,
        current_form: Option<&'a FormDefinition>,
    },
    SavedSql {
        source_id: &'a str,
        current_revision: Option<&'a SavedSqlRevisionMetadata>,
    },
}

impl CurrentSourceDescriptor<'_> {
    fn source_id(&self) -> &str {
        match self {
            Self::EntryQuery { source_id, .. } | Self::SavedSql { source_id, .. } => source_id,
        }
    }
}

/// Inputs for one pure Composition resolution pass. The Composition has
/// already been parsed and version-checked; every supplied current descriptor
/// must have been read through the current authorization boundary.
pub struct ResolveInput<'a> {
    pub composition_revision: CompositionRevisionRef,
    pub spec: &'a CompositionSpec,
    pub parameters: &'a BTreeMap<String, Value>,
    /// Descriptors may arrive in any order; results always follow spec order.
    pub current_sources: &'a [CurrentSourceDescriptor<'a>],
}

/// One source-local request compiled into an existing bounded query contract.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResolvedSourceRequest {
    EntryQuery {
        source_id: String,
        request: EntryPageRequest,
        source_schema_fingerprint: String,
    },
    SavedSql {
        source_id: String,
        request: SqlQueryRequest,
        source_schema_fingerprint: String,
    },
}

/// Renderer-neutral component kind after the Composition layout has been
/// validated and ordered.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedComponentKind {
    Metric,
    #[serde(rename = "table")]
    Tabular,
}

/// One component's stable source binding in deterministic render order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedComponentBinding {
    pub component_id: String,
    pub kind: ResolvedComponentKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub source_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric_field_id: Option<FieldId>,
    /// Current EntryResult property key or exact Saved SQL output alias.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_property_key: Option<String>,
    /// Portable logical result type for scalar metric components.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_result_type: Option<CompositionResultFieldType>,
}

/// Complete plan for one immutable Composition revision.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedCompositionPlan {
    pub composition_revision: CompositionRevisionRef,
    /// The source order is the document order, regardless of descriptor order.
    pub sources: Vec<ResolvedSourceRequest>,
    /// The component order is the layout row and item order, regardless of declaration order.
    #[serde(default)]
    pub component_bindings: Vec<ResolvedComponentBinding>,
}

/// Complete plan for one unsaved Composition draft.
///
/// Identity is the semantic draft fingerprint: there is no entry or revision
/// reference, and the plan must never be stored, published, or given history.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewCompositionPlan {
    pub draft_fingerprint: String,
    /// The source order is the document order, regardless of descriptor order.
    pub sources: Vec<ResolvedSourceRequest>,
    /// The component order is the layout row and item order, regardless of declaration order.
    #[serde(default)]
    pub component_bindings: Vec<ResolvedComponentBinding>,
}

/// Resolve all sources as one all-or-nothing plan.
///
/// Caller parameter diagnostics are a global gate: no source is compiled when
/// any value is missing, unknown, or has the wrong type. Source diagnostics
/// are then aggregated in Composition document order. No query is executed by
/// this function; the returned requests still go through their ordinary ACL,
/// bounds, and continuation checks when executed.
pub fn resolve_composition(
    input: ResolveInput<'_>,
) -> Result<ResolvedCompositionPlan, Vec<CompositionDiagnostic>> {
    let (sources, component_bindings) =
        resolve_composition_parts(input.spec, input.parameters, input.current_sources)?;
    Ok(ResolvedCompositionPlan {
        composition_revision: input.composition_revision,
        sources,
        component_bindings,
    })
}

/// Resolve one unsaved candidate document through the same semantics as a
/// saved revision. Identity is the draft fingerprint, never a revision: the
/// plan carries no entry or revision reference and must not be stored,
/// published, or given history.
pub fn resolve_composition_preview(
    spec: &CompositionSpec,
    parameters: &BTreeMap<String, Value>,
    current_sources: &[CurrentSourceDescriptor<'_>],
    draft_fingerprint: String,
) -> Result<PreviewCompositionPlan, Vec<CompositionDiagnostic>> {
    let (sources, component_bindings) =
        resolve_composition_parts(spec, parameters, current_sources)?;
    Ok(PreviewCompositionPlan {
        draft_fingerprint,
        sources,
        component_bindings,
    })
}

/// Shared plan-building core for saved revisions and unsaved drafts.
fn resolve_composition_parts(
    spec: &CompositionSpec,
    parameters: &BTreeMap<String, Value>,
    current_sources: &[CurrentSourceDescriptor<'_>],
) -> Result<(Vec<ResolvedSourceRequest>, Vec<ResolvedComponentBinding>), Vec<CompositionDiagnostic>>
{
    let bindings = bind_parameters(&spec.parameters, parameters);
    if !bindings.diagnostics.is_empty() {
        return Err(bindings.diagnostics);
    }

    let render_components = spec
        .components_in_render_order()
        .map_err(|code| vec![CompositionDiagnostic::without_parameter(code)])?;

    let source_ids = spec
        .sources
        .iter()
        .map(|source| source.id().to_owned())
        .collect::<BTreeSet<_>>();
    if source_ids.len() != spec.sources.len() {
        return Err(vec![CompositionDiagnostic::without_parameter(
            CompositionDiagnosticCode::InvalidComposition,
        )]);
    }
    let sources_by_id = spec
        .sources
        .iter()
        .map(|source| (source.id(), source))
        .collect::<BTreeMap<_, _>>();
    let mut metric_fields_by_source: BTreeMap<&str, Vec<FieldId>> = BTreeMap::new();
    let mut metric_sql_columns_by_source: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    let mut metric_diagnostics = Vec::new();
    for component in &render_components {
        let Some(source_id) = component.source_id() else {
            // Text carries no source binding and emits no source request.
            continue;
        };
        let value_field = component.value_field();
        if !source_ids.contains(source_id) {
            return Err(vec![CompositionDiagnostic::without_parameter(
                CompositionDiagnosticCode::InvalidComposition,
            )]);
        }
        match (sources_by_id.get(source_id).copied(), value_field) {
            (Some(CompositionSource::EntryQuery { .. }), Some(value_field)) => {
                let CompositionMetricValueField::EntryField { field_id } = value_field else {
                    return Err(vec![CompositionDiagnostic::without_parameter(
                        CompositionDiagnosticCode::InvalidComposition,
                    )]);
                };
                metric_fields_by_source
                    .entry(source_id)
                    .or_default()
                    .push(*field_id);
            }
            (
                Some(CompositionSource::SavedSql {
                    expected_result, ..
                }),
                Some(CompositionMetricValueField::SqlColumn { name }),
            ) => {
                let mut matching_columns =
                    expected_result.iter().filter(|column| column.name == *name);
                let Some(column) = matching_columns.next() else {
                    metric_diagnostics.push(CompositionDiagnostic::without_parameter(
                        CompositionDiagnosticCode::MetricResultColumnMissing,
                    ));
                    continue;
                };
                if matching_columns.next().is_some() {
                    metric_diagnostics.push(CompositionDiagnostic::without_parameter(
                        CompositionDiagnosticCode::MetricResultColumnAmbiguous,
                    ));
                    continue;
                }
                if let Err(code) = validate_composition_metric_result_type(column.result_type) {
                    metric_diagnostics.push(CompositionDiagnostic::without_parameter(code.into()));
                    continue;
                }
                metric_sql_columns_by_source
                    .entry(source_id)
                    .or_default()
                    .insert(name.clone());
            }
            (Some(_), None) => {}
            _ => {
                return Err(vec![CompositionDiagnostic::without_parameter(
                    CompositionDiagnosticCode::InvalidComposition,
                )]);
            }
        }
    }
    if !metric_diagnostics.is_empty() {
        return Err(metric_diagnostics);
    }

    let mut current_by_id = BTreeMap::new();
    for descriptor in current_sources {
        let source_id = descriptor.source_id();
        if !source_ids.contains(source_id) || current_by_id.insert(source_id, descriptor).is_some()
        {
            return Err(vec![CompositionDiagnostic::without_parameter(
                CompositionDiagnosticCode::InvalidComposition,
            )]);
        }
    }

    let mut resolved_sources = Vec::with_capacity(spec.sources.len());
    let mut diagnostics = Vec::new();
    for source in &spec.sources {
        let source_id = source.id();
        match (source, current_by_id.get(source_id).copied()) {
            (
                CompositionSource::EntryQuery {
                    form_id,
                    field_schema,
                    query,
                    ..
                },
                Some(CurrentSourceDescriptor::EntryQuery {
                    current_form: Some(current_form),
                    ..
                }),
            ) => match compile_entry_query_source_with_metric_fields(
                *form_id,
                field_schema,
                query,
                &bindings,
                current_form,
                metric_fields_by_source
                    .get(source_id)
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
            ) {
                Ok(compiled) => resolved_sources.push(ResolvedSourceRequest::EntryQuery {
                    source_id: source_id.to_owned(),
                    request: compiled.request,
                    source_schema_fingerprint: compiled.source_schema_fingerprint,
                }),
                Err(source_diagnostics) => diagnostics.extend(source_diagnostics),
            },
            (
                CompositionSource::SavedSql { .. },
                Some(CurrentSourceDescriptor::SavedSql {
                    current_revision, ..
                }),
            ) => {
                let selected_metric_columns = metric_sql_columns_by_source
                    .get(source_id)
                    .into_iter()
                    .flat_map(|columns| columns.iter().cloned())
                    .collect::<Vec<_>>();
                match compile_saved_sql_source_with_metric_columns(
                    source,
                    *current_revision,
                    &bindings,
                    &selected_metric_columns,
                ) {
                    Ok(compiled) => resolved_sources.push(ResolvedSourceRequest::SavedSql {
                        source_id: source_id.to_owned(),
                        request: compiled.request,
                        source_schema_fingerprint: compiled.source_schema_fingerprint,
                    }),
                    Err(source_diagnostics) => diagnostics.extend(source_diagnostics),
                }
            }
            _ => diagnostics.push(CompositionDiagnostic::without_parameter(
                CompositionDiagnosticCode::SourceUnavailable,
            )),
        }
    }

    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let component_bindings =
        resolve_component_bindings(&render_components, &sources_by_id, &current_by_id)?;

    Ok((resolved_sources, component_bindings))
}

fn resolve_component_bindings(
    components: &[&CompositionComponent],
    sources_by_id: &BTreeMap<&str, &CompositionSource>,
    current_by_id: &BTreeMap<&str, &CurrentSourceDescriptor<'_>>,
) -> Result<Vec<ResolvedComponentBinding>, Vec<CompositionDiagnostic>> {
    let mut resolved = Vec::with_capacity(components.len());
    for component in components {
        let Some(source_id) = component.source_id() else {
            // Text carries no source binding and emits no source request.
            continue;
        };
        let component_id = component.id();
        let source_definition = sources_by_id.get(source_id).copied().ok_or_else(|| {
            vec![CompositionDiagnostic::without_parameter(
                CompositionDiagnosticCode::InvalidComposition,
            )]
        })?;
        let (kind, metric_field_id, result_property_key, expected_result_type) =
            match (source_definition, component.value_field()) {
                (
                    CompositionSource::EntryQuery { .. },
                    Some(CompositionMetricValueField::EntryField { field_id }),
                ) => {
                    let current_form = match current_by_id.get(source_id).copied() {
                        Some(CurrentSourceDescriptor::EntryQuery {
                            current_form: Some(form),
                            ..
                        }) => form,
                        _ => {
                            return Err(vec![CompositionDiagnostic::without_parameter(
                                CompositionDiagnosticCode::SourceUnavailable,
                            )]);
                        }
                    };
                    let Some(field) = current_form
                        .fields
                        .iter()
                        .find(|field| field.id == *field_id)
                    else {
                        return Err(vec![CompositionDiagnostic::without_parameter(
                            CompositionDiagnosticCode::MissingField,
                        )]);
                    };
                    let expected_result_type = composition_metric_result_type(&field.field_type)
                        .map_err(|code| {
                            vec![CompositionDiagnostic::without_parameter(code.into())]
                        })?;
                    (
                        ResolvedComponentKind::Metric,
                        Some(*field_id),
                        Some(field.name.clone()),
                        Some(expected_result_type),
                    )
                }
                (
                    CompositionSource::SavedSql {
                        expected_result, ..
                    },
                    Some(CompositionMetricValueField::SqlColumn { name }),
                ) => {
                    let mut matching_columns =
                        expected_result.iter().filter(|column| column.name == *name);
                    let Some(column) = matching_columns.next() else {
                        return Err(vec![CompositionDiagnostic::without_parameter(
                            CompositionDiagnosticCode::MetricResultColumnMissing,
                        )]);
                    };
                    if matching_columns.next().is_some() {
                        return Err(vec![CompositionDiagnostic::without_parameter(
                            CompositionDiagnosticCode::MetricResultColumnAmbiguous,
                        )]);
                    }
                    validate_composition_metric_result_type(column.result_type).map_err(
                        |code| vec![CompositionDiagnostic::without_parameter(code.into())],
                    )?;
                    (
                        ResolvedComponentKind::Metric,
                        None,
                        Some(name.clone()),
                        Some(column.result_type),
                    )
                }
                (_, None) => (ResolvedComponentKind::Tabular, None, None, None),
                _ => {
                    return Err(vec![CompositionDiagnostic::without_parameter(
                        CompositionDiagnosticCode::InvalidComposition,
                    )]);
                }
            };
        resolved.push(ResolvedComponentBinding {
            component_id: component_id.to_owned(),
            kind,
            label: component.label().map(str::to_owned),
            source_id: source_id.to_owned(),
            metric_field_id,
            result_property_key,
            expected_result_type,
        });
    }
    Ok(resolved)
}

/// Evaluate one already-authorized EntryQuery page for a resolved scalar
/// metric. This function does not fetch another page or aggregate rows.
pub fn evaluate_entry_metric_page(
    binding: &ResolvedComponentBinding,
    page: &EntryPage,
) -> Result<Value, CompositionDiagnostic> {
    let (expected_type, property_key) = metric_binding(binding, true)?;
    let selected_value = if page.rows.len() == 1 {
        page.rows[0]
            .properties
            .as_ref()
            .and_then(Value::as_object)
            .and_then(|properties| properties.get(property_key))
    } else {
        None
    };
    let selected_column_count = if page.rows.len() == 1 && selected_value.is_none() {
        0
    } else {
        1
    };
    evaluate_composition_metric_page(
        expected_type,
        CompositionMetricPage {
            is_complete: !page.has_more && page.next.is_none(),
            row_count: page.rows.len(),
            selected_column_count,
            selected_value,
        },
    )
    .map_err(metric_page_diagnostic)
}

/// Evaluate one already-authorized exact-revision Saved SQL page for a
/// resolved scalar metric. Result aliases are compared exactly as returned by
/// the SQL engine; duplicate aliases remain ambiguous even if a JSON row map
/// has collapsed them to one property.
pub fn evaluate_saved_sql_metric_page(
    binding: &ResolvedComponentBinding,
    page: &SqlQueryPage,
) -> Result<Value, CompositionDiagnostic> {
    let (expected_type, column_name) = metric_binding(binding, false)?;
    let selected_column_count = page
        .columns
        .iter()
        .filter(|column| column.as_str() == column_name)
        .count();
    let selected_value = if page.rows.len() == 1 && selected_column_count == 1 {
        page.rows[0]
            .as_object()
            .and_then(|row| row.get(column_name))
    } else {
        None
    };
    evaluate_composition_metric_page(
        expected_type,
        CompositionMetricPage {
            is_complete: !page.has_more && page.next.is_none(),
            row_count: page.rows.len(),
            selected_column_count,
            selected_value,
        },
    )
    .map_err(metric_page_diagnostic)
}

fn metric_binding(
    binding: &ResolvedComponentBinding,
    entry_source: bool,
) -> Result<(CompositionResultFieldType, &str), CompositionDiagnostic> {
    if binding.kind != ResolvedComponentKind::Metric
        || binding.metric_field_id.is_some() != entry_source
    {
        return Err(CompositionDiagnostic::without_parameter(
            CompositionDiagnosticCode::InvalidComposition,
        ));
    }
    let expected_type = binding.expected_result_type.ok_or_else(|| {
        CompositionDiagnostic::without_parameter(CompositionDiagnosticCode::InvalidComposition)
    })?;
    let property_key = binding.result_property_key.as_deref().ok_or_else(|| {
        CompositionDiagnostic::without_parameter(CompositionDiagnosticCode::InvalidComposition)
    })?;
    Ok((expected_type, property_key))
}

fn metric_page_diagnostic(code: CompositionMetricDiagnosticCode) -> CompositionDiagnostic {
    CompositionDiagnostic::without_parameter(code.into())
}

fn composition_parameter_type_for_sql(variable_type: &str) -> Option<CompositionParameterType> {
    match variable_type {
        "string" => Some(CompositionParameterType::String),
        "boolean" => Some(CompositionParameterType::Boolean),
        "integer" => Some(CompositionParameterType::Integer),
        "float" => Some(CompositionParameterType::Float),
        "date" => Some(CompositionParameterType::Date),
        "timestamp" => Some(CompositionParameterType::Timestamp),
        _ => None,
    }
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
        bind_parameters, compile_entry_query_source, compile_entry_query_source_with_metric_fields,
        compile_saved_sql_source, evaluate_entry_metric_page, evaluate_saved_sql_metric_page,
        resolve_composition, resolve_composition_preview, resolve_value_template,
        CompositionDiagnostic, CompositionRevisionRef, CurrentSourceDescriptor, ParameterBindings,
        ResolveInput, ResolvedComponentBinding, ResolvedComponentKind, ResolvedSourceRequest,
        SavedSqlRevisionMetadata,
    };
    use crate::entry_query::{EntryFieldRef, EntryPage, EntryResult};
    use crate::sql_query::SqlQueryPage;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    use ugoite_domain::composition::{
        CompositionComponent, CompositionDiagnosticCode, CompositionFieldSchemaEntry,
        CompositionLiteral, CompositionMetricValueField, CompositionParameter,
        CompositionParameterFormat, CompositionParameterReference, CompositionParameterType,
        CompositionQueryOperator, CompositionResultColumn, CompositionResultFieldType,
        CompositionSortDirection, CompositionSource, CompositionSpec, CompositionValue,
        DashboardFlowLayout, EntryQueryFilterTemplate, EntryQueryProjectionTemplate,
        EntryQuerySortTemplate, EntryQueryTemplate, FlowItem, FlowLayoutKind, FlowRow, TextStyle,
    };
    use ugoite_domain::form::{
        FieldType, FormDefinition, FormField, FormVersion, ListItemDefinition,
    };
    use ugoite_domain::id::{EntryId, FieldId, FormId, RevisionId};

    fn parameter(
        id: &str,
        parameter_type: CompositionParameterType,
        required: bool,
        default: Option<CompositionLiteral>,
        format: Option<CompositionParameterFormat>,
    ) -> CompositionParameter {
        CompositionParameter {
            id: id.to_owned(),
            label: None,
            parameter_type,
            required,
            default,
            format,
        }
    }

    fn tabular_component(id: &str, label: Option<&str>, source: &str) -> CompositionComponent {
        serde_json::from_value(json!({
            "kind": "table",
            "id": id,
            "label": label,
            "source": source,
        }))
        .expect("the domain component representation deserializes")
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
                reference_form: field.reference_form,
                list_item: field.list_item.clone(),
            })
            .collect()
    }

    fn empty_bindings() -> ParameterBindings {
        ParameterBindings::default()
    }

    fn saved_sql_source(
        entry_id: EntryId,
        revision_id: RevisionId,
        variables: BTreeMap<String, CompositionValue>,
    ) -> CompositionSource {
        CompositionSource::SavedSql {
            id: "monthly-expenses".to_owned(),
            entry_id,
            revision_id,
            expected_result: expected_result(),
            variables,
        }
    }

    fn expected_result() -> Vec<CompositionResultColumn> {
        vec![CompositionResultColumn {
            name: "total".to_owned(),
            result_type: CompositionResultFieldType::Float,
        }]
    }

    fn saved_sql_metadata(
        entry_id: EntryId,
        revision_id: RevisionId,
        variable_types: BTreeMap<String, &str>,
    ) -> SavedSqlRevisionMetadata {
        SavedSqlRevisionMetadata {
            id: entry_id.to_string(),
            revision_id: revision_id.to_string(),
            variable_types: variable_types
                .into_iter()
                .map(|(name, value)| (name, value.to_owned()))
                .collect(),
        }
    }

    fn empty_entry_query_template() -> EntryQueryTemplate {
        EntryQueryTemplate {
            text: None,
            filters: Vec::new(),
            sort: Vec::new(),
            page_limit: 100,
            projection: EntryQueryProjectionTemplate::Preview,
        }
    }

    fn entry_query_source(
        id: &str,
        form: &FormDefinition,
        query: EntryQueryTemplate,
    ) -> CompositionSource {
        CompositionSource::EntryQuery {
            id: id.to_owned(),
            form_id: form.id,
            field_schema: schema(form),
            query,
        }
    }

    fn composition_spec(
        parameters: Vec<CompositionParameter>,
        sources: Vec<CompositionSource>,
    ) -> CompositionSpec {
        CompositionSpec {
            parameters,
            sources,
            components: Vec::new(),
            layout: DashboardFlowLayout {
                kind: FlowLayoutKind::Flow,
                rows: Vec::new(),
            },
        }
    }

    fn layout_row(id: &str, items: Vec<FlowItem>) -> FlowRow {
        FlowRow {
            id: id.to_owned(),
            items,
        }
    }

    fn component_item(id: &str) -> FlowItem {
        FlowItem::Component {
            component: id.to_owned(),
        }
    }

    fn flow_layout(rows: Vec<FlowRow>) -> DashboardFlowLayout {
        DashboardFlowLayout {
            kind: FlowLayoutKind::Flow,
            rows,
        }
    }

    fn single_row_layout(row_id: &str, components: &[&str]) -> DashboardFlowLayout {
        flow_layout(vec![layout_row(
            row_id,
            components.iter().map(|id| component_item(id)).collect(),
        )])
    }

    /// Attach a sourceless text component so a source-focused resolve test
    /// carries a valid non-empty layout without adding source bindings.
    fn spec_with_note(mut spec: CompositionSpec) -> CompositionSpec {
        spec.components = vec![CompositionComponent::Text {
            id: "note".to_owned(),
            label: None,
            text: "Note".to_owned(),
            style: TextStyle::Body,
        }];
        spec.layout = single_row_layout("main", &["note"]);
        spec
    }

    fn id_pair() -> (EntryId, RevisionId) {
        (
            EntryId::from(uuid::Uuid::from_u128(42)),
            RevisionId::from(uuid::Uuid::from_u128(43)),
        )
    }

    fn metric_binding(
        metric_field_id: Option<FieldId>,
        property_key: &str,
        expected_result_type: CompositionResultFieldType,
    ) -> ResolvedComponentBinding {
        ResolvedComponentBinding {
            component_id: "metric".to_owned(),
            kind: ResolvedComponentKind::Metric,
            label: None,
            source_id: "source".to_owned(),
            metric_field_id,
            result_property_key: Some(property_key.to_owned()),
            expected_result_type: Some(expected_result_type),
        }
    }

    fn entry_result(properties: Value) -> EntryResult {
        let (entry_id, revision_id) = id_pair();
        EntryResult {
            id: entry_id.to_string(),
            form_id: FormId::from(uuid::Uuid::from_u128(44)),
            revision_id,
            created_at_micros: 0,
            updated_at_micros: 0,
            properties: Some(properties),
            preview: None,
        }
    }

    fn entry_page(rows: Vec<EntryResult>, has_more: bool, next: Option<&str>) -> EntryPage {
        EntryPage {
            rows,
            has_more,
            next: next.map(str::to_owned),
        }
    }

    fn sql_page(
        columns: &[&str],
        rows: Vec<Value>,
        has_more: bool,
        next: Option<&str>,
    ) -> SqlQueryPage {
        SqlQueryPage {
            columns: columns.iter().map(|column| (*column).to_owned()).collect(),
            rows,
            has_more,
            next: next.map(str::to_owned),
            result_schema: None,
        }
    }

    fn sql_metric_diagnostics(
        expected_result: Vec<CompositionResultColumn>,
        selector: &str,
        current_revision_available: bool,
    ) -> Vec<CompositionDiagnosticCode> {
        let (entry_id, revision_id) = id_pair();
        let source = CompositionSource::SavedSql {
            id: "report".to_owned(),
            entry_id,
            revision_id,
            expected_result,
            variables: BTreeMap::new(),
        };
        let mut spec = composition_spec(Vec::new(), vec![source]);
        spec.components = vec![CompositionComponent::Metric {
            id: "total".to_owned(),
            label: None,
            source: "report".to_owned(),
            value_field: CompositionMetricValueField::SqlColumn {
                name: selector.to_owned(),
            },
        }];
        spec.layout = single_row_layout("summary", &["total"]);
        let metadata = saved_sql_metadata(entry_id, revision_id, BTreeMap::new());
        let current_sources = [CurrentSourceDescriptor::SavedSql {
            source_id: "report",
            current_revision: current_revision_available.then_some(&metadata),
        }];
        diagnostic_codes(resolve_composition(ResolveInput {
            composition_revision: CompositionRevisionRef {
                entry_id,
                revision_id,
            },
            spec: &spec,
            parameters: &BTreeMap::new(),
            current_sources: &current_sources,
        }))
    }

    fn diagnostic_codes<T>(
        result: Result<T, Vec<CompositionDiagnostic>>,
    ) -> Vec<CompositionDiagnosticCode> {
        result
            .err()
            .expect("composition source should be rejected")
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
    fn metric_fields_preserve_the_source_projection_and_fingerprint() {
        let form = form(&[
            (100, FieldType::String),
            (101, FieldType::Integer),
            (102, FieldType::Double),
        ]);
        let template = EntryQueryTemplate {
            projection: EntryQueryProjectionTemplate::Fields {
                fields: vec![FieldId::new(100).unwrap(), FieldId::new(101).unwrap()],
            },
            ..empty_entry_query_template()
        };

        let original = compile_entry_query_source(
            form.id,
            &schema(&form),
            &template,
            &empty_bindings(),
            &form,
        )
        .expect("the declared query projection compiles");
        let with_metric = compile_entry_query_source_with_metric_fields(
            form.id,
            &schema(&form),
            &template,
            &empty_bindings(),
            &form,
            &[FieldId::new(101).unwrap()],
        )
        .expect("a metric may use a field already selected by its source");

        assert_eq!(with_metric.request, original.request);
        assert_eq!(
            with_metric.source_schema_fingerprint,
            original.source_schema_fingerprint
        );
        assert_eq!(
            with_metric.source_schema_fingerprint,
            compile_entry_query_source_with_metric_fields(
                form.id,
                &schema(&form),
                &template,
                &empty_bindings(),
                &form,
                &[FieldId::new(101).unwrap()],
            )
            .expect("repeated resolution has the same fingerprint")
            .source_schema_fingerprint
        );

        assert_eq!(
            diagnostic_codes(compile_entry_query_source_with_metric_fields(
                form.id,
                &schema(&form),
                &template,
                &empty_bindings(),
                &form,
                &[FieldId::new(102).unwrap()],
            )),
            [CompositionDiagnosticCode::MetricFieldNotProjected]
        );
    }

    #[test]
    fn metric_field_does_not_rewrite_a_preview_projection() {
        let form = form(&[(100, FieldType::String), (101, FieldType::Double)]);
        let template = empty_entry_query_template();
        let preview = compile_entry_query_source(
            form.id,
            &schema(&form),
            &template,
            &empty_bindings(),
            &form,
        )
        .expect("preview compiles without a metric");
        let with_metric = compile_entry_query_source_with_metric_fields(
            form.id,
            &schema(&form),
            &template,
            &empty_bindings(),
            &form,
            &[FieldId::new(101).unwrap()],
        )
        .expect_err("Preview does not promise a scalar metric field");

        assert_eq!(
            preview.request.projection,
            crate::entry_query::EntryProjection::Preview
        );
        assert_eq!(
            with_metric
                .iter()
                .map(|diagnostic| diagnostic.code)
                .collect::<Vec<_>>(),
            [CompositionDiagnosticCode::MetricFieldNotProjected]
        );
    }

    #[test]
    fn metric_fields_do_not_expand_the_entry_query_projection_limit() {
        let fields = (100..=164)
            .map(|id| (id, FieldType::String))
            .collect::<Vec<_>>();
        let form = form(&fields);
        let bounded_template = EntryQueryTemplate {
            projection: EntryQueryProjectionTemplate::Fields {
                fields: (100..164).map(|id| FieldId::new(id).unwrap()).collect(),
            },
            ..empty_entry_query_template()
        };

        assert_eq!(
            diagnostic_codes(compile_entry_query_source_with_metric_fields(
                form.id,
                &schema(&form),
                &bounded_template,
                &empty_bindings(),
                &form,
                &[FieldId::new(164).unwrap()],
            )),
            [CompositionDiagnosticCode::MetricFieldNotProjected]
        );

        let over_limit_template = EntryQueryTemplate {
            projection: EntryQueryProjectionTemplate::Fields {
                fields: (100..=164).map(|id| FieldId::new(id).unwrap()).collect(),
            },
            ..empty_entry_query_template()
        };
        assert_eq!(
            diagnostic_codes(compile_entry_query_source(
                form.id,
                &schema(&form),
                &over_limit_template,
                &empty_bindings(),
                &form,
            )),
            [CompositionDiagnosticCode::InvalidComposition]
        );
    }

    #[test]
    fn entry_query_fingerprint_ignores_unreferenced_schema_fields() {
        let mut current = form(&[
            (100, FieldType::String),
            (101, FieldType::Integer),
            (102, FieldType::List),
        ]);
        current.fields[2].list_item = Some(ListItemDefinition {
            field_type: FieldType::String,
            reference_form: None,
        });
        let changed_unrelated = form(&[
            (100, FieldType::String),
            (101, FieldType::Integer),
            (102, FieldType::Double),
        ]);
        let mut changed_unreferenced_list_item = current.clone();
        changed_unreferenced_list_item.fields[2].list_item = Some(ListItemDefinition {
            field_type: FieldType::Integer,
            reference_form: None,
        });
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
        let third = compile_entry_query_source(
            changed_unreferenced_list_item.id,
            &expected,
            &template,
            &empty_bindings(),
            &changed_unreferenced_list_item,
        )
        .expect("unreferenced List item metadata does not invalidate projection");
        assert_eq!(
            first.source_schema_fingerprint,
            third.source_schema_fingerprint
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
            reference_form: None,
            list_item: None,
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
            reference_form: None,
            list_item: None,
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
    fn changed_list_item_schema_is_reported_for_a_used_field() {
        let mut original = form(&[(100, FieldType::List)]);
        original.fields[0].list_item = Some(ListItemDefinition {
            field_type: FieldType::String,
            reference_form: None,
        });
        let expected = schema(&original);
        let mut current = original.clone();
        current.fields[0].list_item = Some(ListItemDefinition {
            field_type: FieldType::Integer,
            reference_form: None,
        });
        let template = EntryQueryTemplate {
            text: None,
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
    fn changed_row_reference_target_is_reported_for_a_used_field() {
        let mut original = form(&[(100, FieldType::RowReference)]);
        original.fields[0].reference_form = Some(FormId::from(uuid::Uuid::from_u128(1000)));
        let expected = schema(&original);
        let mut current = original.clone();
        current.fields[0].reference_form = Some(FormId::from(uuid::Uuid::from_u128(1001)));
        let template = EntryQueryTemplate {
            text: None,
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
    fn source_schema_fingerprint_includes_typed_list_and_reference_metadata() {
        let template = EntryQueryTemplate {
            text: None,
            filters: vec![],
            sort: vec![],
            page_limit: 10,
            projection: EntryQueryProjectionTemplate::Fields {
                fields: vec![FieldId::new(100).unwrap()],
            },
        };

        let mut string_list = form(&[(100, FieldType::List)]);
        string_list.fields[0].list_item = Some(ListItemDefinition {
            field_type: FieldType::String,
            reference_form: None,
        });
        let mut integer_list = string_list.clone();
        integer_list.fields[0].list_item = Some(ListItemDefinition {
            field_type: FieldType::Integer,
            reference_form: None,
        });
        let string_list_fingerprint = compile_entry_query_source(
            string_list.id,
            &schema(&string_list),
            &template,
            &empty_bindings(),
            &string_list,
        )
        .unwrap()
        .source_schema_fingerprint;
        let integer_list_fingerprint = compile_entry_query_source(
            integer_list.id,
            &schema(&integer_list),
            &template,
            &empty_bindings(),
            &integer_list,
        )
        .unwrap()
        .source_schema_fingerprint;
        assert_ne!(string_list_fingerprint, integer_list_fingerprint);

        let mut first_reference = form(&[(100, FieldType::RowReference)]);
        first_reference.fields[0].reference_form = Some(FormId::from(uuid::Uuid::from_u128(1000)));
        let mut second_reference = first_reference.clone();
        second_reference.fields[0].reference_form = Some(FormId::from(uuid::Uuid::from_u128(1001)));
        let first_reference_fingerprint = compile_entry_query_source(
            first_reference.id,
            &schema(&first_reference),
            &template,
            &empty_bindings(),
            &first_reference,
        )
        .unwrap()
        .source_schema_fingerprint;
        let second_reference_fingerprint = compile_entry_query_source(
            second_reference.id,
            &schema(&second_reference),
            &template,
            &empty_bindings(),
            &second_reference,
        )
        .unwrap()
        .source_schema_fingerprint;
        assert_ne!(first_reference_fingerprint, second_reference_fingerprint);
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

    #[test]
    fn saved_sql_source_compiles_to_the_exact_stateless_sql_request() {
        let (entry_id, revision_id) = id_pair();
        let source = saved_sql_source(
            entry_id,
            revision_id,
            BTreeMap::from([
                ("enabled".to_owned(), value(json!(true))),
                ("period_start".to_owned(), parameter_ref("start")),
            ]),
        );
        let parameters = [parameter(
            "start",
            CompositionParameterType::Date,
            true,
            None,
            None,
        )];
        let bindings = bind_parameters(
            &parameters,
            &BTreeMap::from([("start".to_owned(), json!("2026-10-01"))]),
        );
        let current_revision = saved_sql_metadata(
            entry_id,
            revision_id,
            BTreeMap::from([
                ("enabled".to_owned(), "boolean"),
                ("period_start".to_owned(), "date"),
            ]),
        );

        let compiled = compile_saved_sql_source(&source, Some(&current_revision), &bindings)
            .expect("the exact Saved SQL revision and its parameters compile");

        assert_eq!(compiled.request.sql, "");
        assert_eq!(
            compiled.request.saved_sql.as_ref().unwrap().id,
            entry_id.to_string()
        );
        assert_eq!(
            compiled.request.saved_sql.as_ref().unwrap().revision_id,
            revision_id.to_string()
        );
        assert_eq!(compiled.request.parameters["enabled"], json!(true));
        assert_eq!(
            compiled.request.parameters["period_start"],
            json!("2026-10-01")
        );
        assert_eq!(compiled.request.parameter_types["enabled"], "boolean");
        assert_eq!(compiled.request.parameter_types["period_start"], "date");
        assert_eq!(
            compiled.request.limit,
            ugoite_domain::composition::DEFAULT_COMPOSITION_PAGE_LIMIT
        );
        assert!(compiled.request.continuation.is_none());
        assert!(compiled.request.validate().is_ok());

        let repeated = compile_saved_sql_source(&source, Some(&current_revision), &bindings)
            .expect("the same source and descriptor compile deterministically");
        assert_eq!(
            compiled.source_schema_fingerprint,
            repeated.source_schema_fingerprint
        );
    }

    #[test]
    fn saved_sql_source_requires_the_exact_current_revision_descriptor() {
        let (entry_id, revision_id) = id_pair();
        let source = saved_sql_source(entry_id, revision_id, BTreeMap::new());
        let bindings = empty_bindings();

        assert_eq!(
            diagnostic_codes(compile_saved_sql_source(&source, None, &bindings)),
            vec![CompositionDiagnosticCode::SourceUnavailable]
        );

        let wrong_revision = saved_sql_metadata(
            entry_id,
            RevisionId::from(uuid::Uuid::from_u128(44)),
            BTreeMap::new(),
        );
        assert_eq!(
            diagnostic_codes(compile_saved_sql_source(
                &source,
                Some(&wrong_revision),
                &bindings
            )),
            vec![CompositionDiagnosticCode::SourceUnavailable]
        );
    }

    #[test]
    fn saved_sql_source_requires_declared_variables_to_be_bound_once() {
        let (entry_id, revision_id) = id_pair();
        let source = saved_sql_source(
            entry_id,
            revision_id,
            BTreeMap::from([("not_declared".to_owned(), value(json!(5)))]),
        );
        let current_revision = saved_sql_metadata(
            entry_id,
            revision_id,
            BTreeMap::from([("required".to_owned(), "integer")]),
        );

        assert_eq!(
            diagnostic_codes(compile_saved_sql_source(
                &source,
                Some(&current_revision),
                &empty_bindings()
            )),
            vec![
                CompositionDiagnosticCode::SourceSchemaChanged,
                CompositionDiagnosticCode::ParameterMissing,
            ]
        );
    }

    #[test]
    fn saved_sql_source_binds_every_supported_sql_variable_type() {
        let (entry_id, revision_id) = id_pair();
        let parameters = [
            parameter("text", CompositionParameterType::String, true, None, None),
            parameter("flag", CompositionParameterType::Boolean, true, None, None),
            parameter("count", CompositionParameterType::Integer, true, None, None),
            parameter("ratio", CompositionParameterType::Float, true, None, None),
            parameter("date", CompositionParameterType::Date, true, None, None),
            parameter(
                "timestamp",
                CompositionParameterType::Timestamp,
                true,
                None,
                None,
            ),
        ];
        let values = BTreeMap::from([
            ("text".to_owned(), json!("october")),
            ("flag".to_owned(), json!(true)),
            ("count".to_owned(), json!(3)),
            ("ratio".to_owned(), json!(1.25)),
            ("date".to_owned(), json!("2026-10-02")),
            ("timestamp".to_owned(), json!("2026-10-02T12:30:00")),
        ]);
        let bindings = bind_parameters(&parameters, &values);
        let source = saved_sql_source(
            entry_id,
            revision_id,
            BTreeMap::from([
                ("sql_text".to_owned(), parameter_ref("text")),
                ("sql_flag".to_owned(), parameter_ref("flag")),
                ("sql_count".to_owned(), parameter_ref("count")),
                ("sql_ratio".to_owned(), parameter_ref("ratio")),
                ("sql_date".to_owned(), parameter_ref("date")),
                ("sql_timestamp".to_owned(), parameter_ref("timestamp")),
            ]),
        );
        let current_revision = saved_sql_metadata(
            entry_id,
            revision_id,
            BTreeMap::from([
                ("sql_text".to_owned(), "string"),
                ("sql_flag".to_owned(), "boolean"),
                ("sql_count".to_owned(), "integer"),
                ("sql_ratio".to_owned(), "float"),
                ("sql_date".to_owned(), "date"),
                ("sql_timestamp".to_owned(), "timestamp"),
            ]),
        );

        let compiled = compile_saved_sql_source(&source, Some(&current_revision), &bindings)
            .expect("all existing Saved SQL variable types are supported");

        assert_eq!(compiled.request.parameters.len(), 6);
        assert_eq!(compiled.request.parameter_types.len(), 6);
        assert_eq!(compiled.request.parameters["sql_date"], json!("2026-10-02"));
        assert_eq!(
            compiled.request.parameters["sql_timestamp"],
            json!("2026-10-02T12:30:00")
        );
    }

    #[test]
    fn saved_sql_source_checks_parameter_types_and_literal_values() {
        let (entry_id, revision_id) = id_pair();
        let parameters = [parameter(
            "amount",
            CompositionParameterType::Integer,
            true,
            None,
            None,
        )];
        let bindings = bind_parameters(
            &parameters,
            &BTreeMap::from([("amount".to_owned(), json!(5))]),
        );
        let source = saved_sql_source(
            entry_id,
            revision_id,
            BTreeMap::from([("threshold".to_owned(), parameter_ref("amount"))]),
        );
        let current_revision = saved_sql_metadata(
            entry_id,
            revision_id,
            BTreeMap::from([("threshold".to_owned(), "float")]),
        );
        assert_eq!(
            diagnostic_codes(compile_saved_sql_source(
                &source,
                Some(&current_revision),
                &bindings
            )),
            vec![CompositionDiagnosticCode::ParameterTypeMismatch]
        );

        let invalid_literal = saved_sql_source(
            entry_id,
            revision_id,
            BTreeMap::from([("start".to_owned(), value(json!("not-a-date")))]),
        );
        let date_revision = saved_sql_metadata(
            entry_id,
            revision_id,
            BTreeMap::from([("start".to_owned(), "date")]),
        );
        assert_eq!(
            diagnostic_codes(compile_saved_sql_source(
                &invalid_literal,
                Some(&date_revision),
                &empty_bindings()
            )),
            vec![CompositionDiagnosticCode::InvalidComposition]
        );
    }

    #[test]
    fn saved_sql_source_preserves_typed_null_literals() {
        let (entry_id, revision_id) = id_pair();
        let source = saved_sql_source(
            entry_id,
            revision_id,
            BTreeMap::from([("optional_limit".to_owned(), value(Value::Null))]),
        );
        let current_revision = saved_sql_metadata(
            entry_id,
            revision_id,
            BTreeMap::from([("optional_limit".to_owned(), "integer")]),
        );

        let compiled =
            compile_saved_sql_source(&source, Some(&current_revision), &empty_bindings())
                .expect("the exact SQL variable type supplies the null parameter type");

        assert_eq!(compiled.request.parameters["optional_limit"], Value::Null);
        assert_eq!(
            compiled.request.parameter_types["optional_limit"],
            "integer"
        );
    }

    #[test]
    fn saved_sql_source_rejects_null_from_a_named_parameter() {
        let (entry_id, revision_id) = id_pair();
        let source = saved_sql_source(
            entry_id,
            revision_id,
            BTreeMap::from([("optional_limit".to_owned(), parameter_ref("limit"))]),
        );
        let current_revision = saved_sql_metadata(
            entry_id,
            revision_id,
            BTreeMap::from([("optional_limit".to_owned(), "integer")]),
        );
        let bindings = ParameterBindings {
            values: BTreeMap::from([("limit".to_owned(), Value::Null)]),
            parameter_types: BTreeMap::from([(
                "limit".to_owned(),
                CompositionParameterType::Integer,
            )]),
            ..ParameterBindings::default()
        };

        assert_eq!(
            diagnostic_codes(compile_saved_sql_source(
                &source,
                Some(&current_revision),
                &bindings
            )),
            vec![CompositionDiagnosticCode::ParameterTypeMismatch]
        );
    }

    #[test]
    fn any_parameter_error_stops_resolution_before_compiling_any_source() {
        let current_form = form(&[(100, FieldType::String)]);
        let (entry_id, revision_id) = id_pair();
        let spec = composition_spec(
            vec![parameter(
                "limit",
                CompositionParameterType::Integer,
                true,
                None,
                None,
            )],
            vec![
                entry_query_source("entries", &current_form, empty_entry_query_template()),
                CompositionSource::SavedSql {
                    id: "report".to_owned(),
                    entry_id,
                    revision_id,
                    expected_result: expected_result(),
                    variables: BTreeMap::from([("limit".to_owned(), parameter_ref("limit"))]),
                },
            ],
        );
        let supplied = BTreeMap::from([("limit".to_owned(), json!("not-an-integer"))]);
        let current_sources = [
            CurrentSourceDescriptor::EntryQuery {
                source_id: "entries",
                current_form: Some(&current_form),
            },
            // If resolution starts compiling, this unavailable second source
            // would add a source diagnostic to the binding error.
            CurrentSourceDescriptor::SavedSql {
                source_id: "report",
                current_revision: None,
            },
        ];
        let (composition_entry_id, composition_revision_id) = id_pair();

        let diagnostics = resolve_composition(ResolveInput {
            composition_revision: CompositionRevisionRef {
                entry_id: composition_entry_id,
                revision_id: composition_revision_id,
            },
            spec: &spec,
            parameters: &supplied,
            current_sources: &current_sources,
        })
        .expect_err("one invalid parameter rejects the complete plan");

        assert_eq!(
            diagnostics,
            vec![CompositionDiagnostic {
                code: CompositionDiagnosticCode::ParameterTypeMismatch,
                parameter_id: Some("limit".to_owned()),
            }]
        );
    }

    #[test]
    fn resolve_plan_preserves_document_order_and_exact_revision_identity() {
        let current_form = form(&[(100, FieldType::String)]);
        let (sql_entry_id, sql_revision_id) = id_pair();
        let saved_sql = CompositionSource::SavedSql {
            id: "report".to_owned(),
            entry_id: sql_entry_id,
            revision_id: sql_revision_id,
            expected_result: expected_result(),
            variables: BTreeMap::new(),
        };
        let spec = spec_with_note(composition_spec(
            Vec::new(),
            vec![
                saved_sql,
                entry_query_source("entries", &current_form, empty_entry_query_template()),
            ],
        ));
        let sql_metadata = saved_sql_metadata(sql_entry_id, sql_revision_id, BTreeMap::new());
        // Metadata lookup order is independent of Composition source order.
        let current_sources = [
            CurrentSourceDescriptor::EntryQuery {
                source_id: "entries",
                current_form: Some(&current_form),
            },
            CurrentSourceDescriptor::SavedSql {
                source_id: "report",
                current_revision: Some(&sql_metadata),
            },
        ];
        let (composition_entry_id, composition_revision_id) = id_pair();

        let plan = resolve_composition(ResolveInput {
            composition_revision: CompositionRevisionRef {
                entry_id: composition_entry_id,
                revision_id: composition_revision_id,
            },
            spec: &spec,
            parameters: &BTreeMap::new(),
            current_sources: &current_sources,
        })
        .expect("both sources compile");

        assert_eq!(
            plan.composition_revision,
            CompositionRevisionRef {
                entry_id: composition_entry_id,
                revision_id: composition_revision_id,
            }
        );
        assert!(matches!(
            &plan.sources[0],
            ResolvedSourceRequest::SavedSql {
                source_id,
                request,
                source_schema_fingerprint,
            } if source_id == "report"
                && request.saved_sql.as_ref().is_some_and(|saved_sql| {
                    saved_sql.id == sql_entry_id.to_string()
                        && saved_sql.revision_id == sql_revision_id.to_string()
                })
                && source_schema_fingerprint.len() == 64
        ));
        assert!(matches!(
            &plan.sources[1],
            ResolvedSourceRequest::EntryQuery {
                source_id,
                source_schema_fingerprint,
                ..
            } if source_id == "entries" && source_schema_fingerprint.len() == 64
        ));
    }

    #[test]
    fn preview_resolves_through_the_same_semantics_without_revision_identity() {
        let current_form = form(&[(100, FieldType::String)]);
        let (sql_entry_id, sql_revision_id) = id_pair();
        let saved_sql = CompositionSource::SavedSql {
            id: "report".to_owned(),
            entry_id: sql_entry_id,
            revision_id: sql_revision_id,
            expected_result: expected_result(),
            variables: BTreeMap::new(),
        };
        let spec = spec_with_note(composition_spec(
            Vec::new(),
            vec![
                saved_sql,
                entry_query_source("entries", &current_form, empty_entry_query_template()),
            ],
        ));
        let sql_metadata = saved_sql_metadata(sql_entry_id, sql_revision_id, BTreeMap::new());
        let current_sources = [
            CurrentSourceDescriptor::EntryQuery {
                source_id: "entries",
                current_form: Some(&current_form),
            },
            CurrentSourceDescriptor::SavedSql {
                source_id: "report",
                current_revision: Some(&sql_metadata),
            },
        ];
        let (composition_entry_id, composition_revision_id) = id_pair();

        let saved = resolve_composition(ResolveInput {
            composition_revision: CompositionRevisionRef {
                entry_id: composition_entry_id,
                revision_id: composition_revision_id,
            },
            spec: &spec,
            parameters: &BTreeMap::new(),
            current_sources: &current_sources,
        })
        .expect("saved revision compiles");
        let preview = resolve_composition_preview(
            &spec,
            &BTreeMap::new(),
            &current_sources,
            "draft-fingerprint".to_owned(),
        )
        .expect("same draft compiles");

        // Identical sources and bindings; identity is the fingerprint.
        assert_eq!(preview.sources, saved.sources);
        assert_eq!(preview.component_bindings, saved.component_bindings);
        assert_eq!(preview.draft_fingerprint, "draft-fingerprint");
    }

    #[test]
    fn resolve_plan_binds_components_in_render_order_with_stable_metric_identity() {
        let mut current_form = form(&[(100, FieldType::String), (101, FieldType::Integer)]);
        current_form.fields[1].name = "total_amount".to_owned();
        let entry_source = entry_query_source(
            "entries",
            &current_form,
            EntryQueryTemplate {
                projection: EntryQueryProjectionTemplate::Fields {
                    fields: vec![FieldId::new(100).unwrap(), FieldId::new(101).unwrap()],
                },
                ..empty_entry_query_template()
            },
        );
        let (sql_entry_id, sql_revision_id) = id_pair();
        let sql_source = CompositionSource::SavedSql {
            id: "report".to_owned(),
            entry_id: sql_entry_id,
            revision_id: sql_revision_id,
            expected_result: expected_result(),
            variables: BTreeMap::new(),
        };
        let mut spec = composition_spec(Vec::new(), vec![entry_source, sql_source]);
        spec.components = vec![
            CompositionComponent::Metric {
                id: "entry-total".to_owned(),
                label: Some("Entry total".to_owned()),
                source: "entries".to_owned(),
                value_field: CompositionMetricValueField::EntryField {
                    field_id: FieldId::new(101).unwrap(),
                },
            },
            tabular_component("rows", None, "entries"),
            CompositionComponent::Metric {
                id: "sql-total".to_owned(),
                label: Some("SQL total".to_owned()),
                source: "report".to_owned(),
                value_field: CompositionMetricValueField::SqlColumn {
                    name: "total".to_owned(),
                },
            },
        ];
        spec.layout = flow_layout(vec![
            layout_row(
                "summary",
                vec![component_item("sql-total"), component_item("entry-total")],
            ),
            layout_row("details", vec![component_item("rows")]),
        ]);
        let sql_metadata = saved_sql_metadata(sql_entry_id, sql_revision_id, BTreeMap::new());
        let current_sources = [
            CurrentSourceDescriptor::SavedSql {
                source_id: "report",
                current_revision: Some(&sql_metadata),
            },
            CurrentSourceDescriptor::EntryQuery {
                source_id: "entries",
                current_form: Some(&current_form),
            },
        ];

        let plan = resolve_composition(ResolveInput {
            composition_revision: CompositionRevisionRef {
                entry_id: sql_entry_id,
                revision_id: sql_revision_id,
            },
            spec: &spec,
            parameters: &BTreeMap::new(),
            current_sources: &current_sources,
        })
        .expect("sources and ordered component references resolve");

        assert_eq!(
            plan.component_bindings
                .iter()
                .map(|binding| binding.component_id.as_str())
                .collect::<Vec<_>>(),
            ["sql-total", "entry-total", "rows"]
        );
        assert_eq!(plan.component_bindings[0].source_id, "report");
        assert_eq!(
            plan.component_bindings[0].label.as_deref(),
            Some("SQL total")
        );
        assert_eq!(plan.component_bindings[0].metric_field_id, None);
        assert_eq!(
            plan.component_bindings[0].expected_result_type,
            Some(CompositionResultFieldType::Float)
        );
        assert_eq!(
            plan.component_bindings[0].result_property_key.as_deref(),
            Some("total")
        );
        assert_eq!(plan.component_bindings[1].source_id, "entries");
        let ResolvedSourceRequest::EntryQuery { request, .. } = &plan.sources[0] else {
            panic!("the first source is the declared EntryQuery source");
        };
        assert_eq!(
            request.projection,
            crate::entry_query::EntryProjection::Fields {
                fields: vec![
                    EntryFieldRef::Property {
                        field_id: FieldId::new(100).unwrap(),
                    },
                    EntryFieldRef::Property {
                        field_id: FieldId::new(101).unwrap(),
                    },
                ],
            }
        );
        assert_eq!(
            plan.component_bindings[1].metric_field_id,
            Some(FieldId::new(101).unwrap())
        );
        assert_eq!(
            plan.component_bindings[1].expected_result_type,
            Some(CompositionResultFieldType::Integer)
        );
        assert_eq!(
            plan.component_bindings[1].result_property_key.as_deref(),
            Some("total_amount")
        );
        assert_eq!(
            plan.component_bindings[2].kind,
            super::ResolvedComponentKind::Tabular
        );
        assert_eq!(plan.component_bindings[2].metric_field_id, None);
        assert_eq!(plan.component_bindings[2].result_property_key, None);
        assert_eq!(plan.component_bindings[2].expected_result_type, None);
        let plan_json = serde_json::to_value(&plan).unwrap();
        assert_eq!(
            plan_json["component_bindings"][1]["metric_field_id"],
            json!(101)
        );
        assert_eq!(
            plan_json["component_bindings"][1]["result_property_key"],
            json!("total_amount")
        );
        assert_eq!(
            plan_json["component_bindings"][1]["expected_result_type"],
            json!("integer")
        );
        assert_eq!(plan_json["component_bindings"][2]["kind"], json!("table"));
        assert!(matches!(
            &plan.sources[0],
            ResolvedSourceRequest::EntryQuery { request, .. }
                if request.projection == crate::entry_query::EntryProjection::Fields {
                    fields: vec![
                        crate::entry_query::EntryFieldRef::Property { field_id: FieldId::new(100).unwrap() },
                        crate::entry_query::EntryFieldRef::Property { field_id: FieldId::new(101).unwrap() },
                    ]
                }
        ));
        assert!(matches!(
            &plan.sources[1],
            ResolvedSourceRequest::SavedSql { source_id, request, .. }
                if source_id == "report"
                    && request.saved_sql.as_ref().is_some_and(|saved_sql| {
                        saved_sql.id == sql_entry_id.to_string()
                            && saved_sql.revision_id == sql_revision_id.to_string()
                    })
        ));
    }

    #[test]
    fn text_components_emit_no_source_request_or_binding() {
        let current_form = form(&[(100, FieldType::String)]);
        let mut spec = composition_spec(
            Vec::new(),
            vec![entry_query_source(
                "entries",
                &current_form,
                empty_entry_query_template(),
            )],
        );
        spec.components = vec![
            CompositionComponent::Text {
                id: "note".to_owned(),
                label: None,
                text: "Note".to_owned(),
                style: TextStyle::Body,
            },
            tabular_component("rows", None, "entries"),
        ];
        spec.layout = flow_layout(vec![layout_row(
            "main",
            vec![component_item("note"), component_item("rows")],
        )]);
        let current_sources = [CurrentSourceDescriptor::EntryQuery {
            source_id: "entries",
            current_form: Some(&current_form),
        }];

        let plan = resolve_composition(ResolveInput {
            composition_revision: CompositionRevisionRef {
                entry_id: id_pair().0,
                revision_id: id_pair().1,
            },
            spec: &spec,
            parameters: &BTreeMap::new(),
            current_sources: &current_sources,
        })
        .expect("text alongside a table resolves");

        assert_eq!(plan.sources.len(), 1);
        assert_eq!(
            plan.component_bindings
                .iter()
                .map(|binding| binding.component_id.as_str())
                .collect::<Vec<_>>(),
            ["rows"]
        );
    }

    #[test]
    fn required_parameter_layout_control_gates_resolution() {
        let current_form = form(&[(100, FieldType::String)]);
        let mut spec = composition_spec(
            vec![parameter(
                "month",
                CompositionParameterType::String,
                true,
                None,
                None,
            )],
            vec![entry_query_source(
                "entries",
                &current_form,
                empty_entry_query_template(),
            )],
        );
        spec.components = vec![tabular_component("rows", None, "entries")];
        spec.layout = flow_layout(vec![layout_row(
            "main",
            vec![
                FlowItem::Parameter {
                    parameter: "month".to_owned(),
                },
                component_item("rows"),
            ],
        )]);
        let current_sources = [CurrentSourceDescriptor::EntryQuery {
            source_id: "entries",
            current_form: Some(&current_form),
        }];
        let supplied = BTreeMap::from([("month".to_owned(), json!("2026-10"))]);

        let plan = resolve_composition(ResolveInput {
            composition_revision: CompositionRevisionRef {
                entry_id: id_pair().0,
                revision_id: id_pair().1,
            },
            spec: &spec,
            parameters: &supplied,
            current_sources: &current_sources,
        })
        .expect("a placed required parameter resolves");
        assert_eq!(plan.component_bindings.len(), 1);

        spec.layout = single_row_layout("main", &["rows"]);
        let diagnostics = resolve_composition(ResolveInput {
            composition_revision: CompositionRevisionRef {
                entry_id: id_pair().0,
                revision_id: id_pair().1,
            },
            spec: &spec,
            parameters: &supplied,
            current_sources: &current_sources,
        })
        .expect_err("a required parameter without a layout control is invalid");
        assert_eq!(
            diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.code)
                .collect::<Vec<_>>(),
            vec![CompositionDiagnosticCode::InvalidComposition]
        );
    }

    #[test]
    fn saved_sql_metric_selector_requires_one_declared_scalar_column_before_revision_lookup() {
        assert_eq!(
            sql_metric_diagnostics(expected_result(), "not_declared", false,),
            vec![CompositionDiagnosticCode::MetricResultColumnMissing]
        );
        assert_eq!(
            sql_metric_diagnostics(
                vec![
                    CompositionResultColumn {
                        name: "total".to_owned(),
                        result_type: CompositionResultFieldType::Float,
                    },
                    CompositionResultColumn {
                        name: "total".to_owned(),
                        result_type: CompositionResultFieldType::Integer,
                    },
                ],
                "total",
                true,
            ),
            vec![CompositionDiagnosticCode::MetricResultColumnAmbiguous]
        );
        assert_eq!(
            sql_metric_diagnostics(
                vec![CompositionResultColumn {
                    name: "payload".to_owned(),
                    result_type: CompositionResultFieldType::Json,
                }],
                "payload",
                true,
            ),
            vec![CompositionDiagnosticCode::MetricResultNotScalar]
        );
    }

    #[test]
    fn entry_metric_rejects_a_current_non_scalar_form_field() {
        let mut current_form = form(&[(101, FieldType::List)]);
        current_form.fields[0].list_item = Some(ListItemDefinition {
            field_type: FieldType::String,
            reference_form: None,
        });
        let source = entry_query_source(
            "entries",
            &current_form,
            EntryQueryTemplate {
                projection: EntryQueryProjectionTemplate::Fields {
                    fields: vec![FieldId::new(101).unwrap()],
                },
                ..empty_entry_query_template()
            },
        );
        let mut spec = composition_spec(Vec::new(), vec![source]);
        spec.components = vec![CompositionComponent::Metric {
            id: "total".to_owned(),
            label: None,
            source: "entries".to_owned(),
            value_field: CompositionMetricValueField::EntryField {
                field_id: FieldId::new(101).unwrap(),
            },
        }];
        spec.layout = single_row_layout("summary", &["total"]);
        let current_sources = [CurrentSourceDescriptor::EntryQuery {
            source_id: "entries",
            current_form: Some(&current_form),
        }];
        let (entry_id, revision_id) = id_pair();

        assert_eq!(
            diagnostic_codes(resolve_composition(ResolveInput {
                composition_revision: CompositionRevisionRef {
                    entry_id,
                    revision_id,
                },
                spec: &spec,
                parameters: &BTreeMap::new(),
                current_sources: &current_sources,
            })),
            vec![CompositionDiagnosticCode::MetricResultNotScalar]
        );
    }

    #[test]
    fn entry_metric_page_adapter_checks_one_complete_scalar_row() {
        let binding = metric_binding(
            Some(FieldId::new(101).unwrap()),
            "amount",
            CompositionResultFieldType::Integer,
        );
        assert_eq!(
            evaluate_entry_metric_page(
                &binding,
                &entry_page(vec![entry_result(json!({"amount": 42}))], false, None),
            ),
            Ok(json!(42))
        );

        let failures = [
            (
                entry_page(Vec::new(), false, None),
                CompositionDiagnosticCode::MetricResultEmpty,
            ),
            (
                entry_page(
                    vec![
                        entry_result(json!({"amount": 42})),
                        entry_result(json!({"amount": 43})),
                    ],
                    false,
                    None,
                ),
                CompositionDiagnosticCode::MetricResultMultipleRows,
            ),
            (
                entry_page(vec![entry_result(json!({}))], false, None),
                CompositionDiagnosticCode::MetricResultColumnMissing,
            ),
            (
                entry_page(vec![entry_result(json!({"amount": [42]}))], false, None),
                CompositionDiagnosticCode::MetricResultNotScalar,
            ),
            (
                entry_page(vec![entry_result(json!({"amount": "42"}))], false, None),
                CompositionDiagnosticCode::MetricResultTypeMismatch,
            ),
            (
                entry_page(
                    vec![entry_result(json!({"amount": 42}))],
                    false,
                    Some("next"),
                ),
                CompositionDiagnosticCode::MetricResultPageIncomplete,
            ),
        ];
        for (page, expected_code) in failures {
            assert_eq!(
                evaluate_entry_metric_page(&binding, &page)
                    .expect_err("invalid metric page must have a stable diagnostic")
                    .code,
                expected_code
            );
        }
    }

    #[test]
    fn saved_sql_metric_page_adapter_checks_exact_alias_and_scalar_shape() {
        let binding = metric_binding(None, "total", CompositionResultFieldType::Float);
        assert_eq!(
            evaluate_saved_sql_metric_page(
                &binding,
                &sql_page(&["total"], vec![json!({"total": 42.5})], false, None),
            ),
            Ok(json!(42.5))
        );

        let failures = [
            (
                sql_page(&["other"], vec![json!({"other": 42.5})], false, None),
                CompositionDiagnosticCode::MetricResultColumnMissing,
            ),
            (
                sql_page(&["total"], vec![json!({"other": 42.5})], false, None),
                CompositionDiagnosticCode::MetricResultColumnMissing,
            ),
            (
                sql_page(
                    &["total", "total"],
                    vec![json!({"total": 42.5})],
                    false,
                    None,
                ),
                CompositionDiagnosticCode::MetricResultColumnAmbiguous,
            ),
            (
                sql_page(&["total"], Vec::new(), false, None),
                CompositionDiagnosticCode::MetricResultEmpty,
            ),
            (
                sql_page(
                    &["total"],
                    vec![json!({"total": 1.0}), json!({"total": 2.0})],
                    false,
                    None,
                ),
                CompositionDiagnosticCode::MetricResultMultipleRows,
            ),
            (
                sql_page(
                    &["total"],
                    vec![json!({"total": {"value": 42.5}})],
                    false,
                    None,
                ),
                CompositionDiagnosticCode::MetricResultNotScalar,
            ),
            (
                sql_page(&["total"], vec![json!({"total": "42.5"})], false, None),
                CompositionDiagnosticCode::MetricResultTypeMismatch,
            ),
            (
                sql_page(&["total"], vec![json!({"total": 42.5})], true, Some("next")),
                CompositionDiagnosticCode::MetricResultPageIncomplete,
            ),
        ];
        for (page, expected_code) in failures {
            assert_eq!(
                evaluate_saved_sql_metric_page(&binding, &page)
                    .expect_err("invalid metric page must have a stable diagnostic")
                    .code,
                expected_code
            );
        }
    }

    #[test]
    fn metric_page_adapter_rejects_wrong_binding_kind() {
        let mut binding = metric_binding(
            Some(FieldId::new(101).unwrap()),
            "amount",
            CompositionResultFieldType::Integer,
        );
        binding.kind = ResolvedComponentKind::Tabular;
        let diagnostic = evaluate_entry_metric_page(
            &binding,
            &entry_page(vec![entry_result(json!({"amount": 42}))], false, None),
        )
        .expect_err("tabular bindings cannot be evaluated as metrics");
        assert_eq!(
            diagnostic.code,
            CompositionDiagnosticCode::InvalidComposition
        );
    }

    #[test]
    fn missing_component_source_is_an_invalid_composition() {
        let mut spec = composition_spec(Vec::new(), Vec::new());
        spec.components = vec![tabular_component("rows", None, "missing")];
        spec.layout = single_row_layout("main", &["rows"]);

        let diagnostics = resolve_composition(ResolveInput {
            composition_revision: CompositionRevisionRef {
                entry_id: id_pair().0,
                revision_id: id_pair().1,
            },
            spec: &spec,
            parameters: &BTreeMap::new(),
            current_sources: &[],
        })
        .expect_err("a component cannot point to an undeclared source");

        assert_eq!(
            diagnostics,
            vec![CompositionDiagnostic::without_parameter(
                CompositionDiagnosticCode::InvalidComposition
            )]
        );
    }

    #[test]
    fn metric_value_field_variant_must_match_its_source_kind() {
        let current_form = form(&[(100, FieldType::Integer)]);
        let entry_source =
            entry_query_source("entries", &current_form, empty_entry_query_template());
        let (entry_id, revision_id) = id_pair();
        let sql_source = CompositionSource::SavedSql {
            id: "report".to_owned(),
            entry_id,
            revision_id,
            expected_result: expected_result(),
            variables: BTreeMap::new(),
        };
        let sql_metadata = saved_sql_metadata(entry_id, revision_id, BTreeMap::new());
        let (composition_entry_id, composition_revision_id) = id_pair();

        for (source_id, value_field) in [
            (
                "entries",
                CompositionMetricValueField::SqlColumn {
                    name: "total".to_owned(),
                },
            ),
            (
                "report",
                CompositionMetricValueField::EntryField {
                    field_id: FieldId::new(100).unwrap(),
                },
            ),
        ] {
            let mut spec =
                composition_spec(Vec::new(), vec![entry_source.clone(), sql_source.clone()]);
            spec.components = vec![CompositionComponent::Metric {
                id: "total".to_owned(),
                label: None,
                source: source_id.to_owned(),
                value_field,
            }];
            spec.layout = single_row_layout("main", &["total"]);
            let current_sources = [
                CurrentSourceDescriptor::EntryQuery {
                    source_id: "entries",
                    current_form: Some(&current_form),
                },
                CurrentSourceDescriptor::SavedSql {
                    source_id: "report",
                    current_revision: Some(&sql_metadata),
                },
            ];

            assert_eq!(
                diagnostic_codes(resolve_composition(ResolveInput {
                    composition_revision: CompositionRevisionRef {
                        entry_id: composition_entry_id,
                        revision_id: composition_revision_id,
                    },
                    spec: &spec,
                    parameters: &BTreeMap::new(),
                    current_sources: &current_sources,
                })),
                [CompositionDiagnosticCode::InvalidComposition]
            );
        }
    }

    #[test]
    fn denied_metric_source_is_concealed_as_source_unavailable() {
        let current_form = form(&[(100, FieldType::Integer)]);
        let mut spec = composition_spec(
            Vec::new(),
            vec![entry_query_source(
                "private-source-name",
                &current_form,
                empty_entry_query_template(),
            )],
        );
        spec.components = vec![CompositionComponent::Metric {
            id: "private-component".to_owned(),
            label: None,
            source: "private-source-name".to_owned(),
            value_field: CompositionMetricValueField::EntryField {
                field_id: FieldId::new(100).unwrap(),
            },
        }];
        spec.layout = single_row_layout("private-section", &["private-component"]);
        let current_sources = [CurrentSourceDescriptor::EntryQuery {
            source_id: "private-source-name",
            current_form: None,
        }];

        let diagnostics = resolve_composition(ResolveInput {
            composition_revision: CompositionRevisionRef {
                entry_id: id_pair().0,
                revision_id: id_pair().1,
            },
            spec: &spec,
            parameters: &BTreeMap::new(),
            current_sources: &current_sources,
        })
        .expect_err("denied sources cannot produce a partial renderer plan");

        assert_eq!(
            diagnostics,
            vec![CompositionDiagnostic::without_parameter(
                CompositionDiagnosticCode::SourceUnavailable
            )]
        );
        let diagnostic_debug = format!("{diagnostics:?}");
        assert!(!diagnostic_debug.contains("private-source-name"));
        assert!(!diagnostic_debug.contains("private-component"));
    }

    #[test]
    fn field_level_diagnostics_require_an_authorized_source_form_read() {
        let saved_form = form(&[(101, FieldType::Integer)]);
        let changed_form = form(&[(101, FieldType::String)]);
        let source_id = "private-source-name";
        let field_id = FieldId::new(101).unwrap();
        let mut spec = composition_spec(
            Vec::new(),
            vec![entry_query_source(
                source_id,
                &saved_form,
                EntryQueryTemplate {
                    projection: EntryQueryProjectionTemplate::Fields {
                        fields: vec![field_id],
                    },
                    ..empty_entry_query_template()
                },
            )],
        );
        spec.components = vec![CompositionComponent::Metric {
            id: "private-component".to_owned(),
            label: None,
            source: source_id.to_owned(),
            value_field: CompositionMetricValueField::EntryField { field_id },
        }];
        spec.layout = single_row_layout("private-section", &["private-component"]);

        let denied_sources = [CurrentSourceDescriptor::EntryQuery {
            source_id,
            current_form: None,
        }];
        let denied_diagnostics = resolve_composition(ResolveInput {
            composition_revision: CompositionRevisionRef {
                entry_id: id_pair().0,
                revision_id: id_pair().1,
            },
            spec: &spec,
            parameters: &BTreeMap::new(),
            current_sources: &denied_sources,
        })
        .expect_err("denied source Form reads cannot produce field diagnostics");

        assert_eq!(
            denied_diagnostics,
            vec![CompositionDiagnostic::without_parameter(
                CompositionDiagnosticCode::SourceUnavailable
            )]
        );
        let denied_debug = format!("{denied_diagnostics:?}");
        let saved_form_id = saved_form.id.to_string();
        let field_id_string = field_id.get().to_string();
        for concealed_value in [
            source_id,
            "private-component",
            "private-section",
            saved_form_id.as_str(),
            field_id_string.as_str(),
            "field_101",
        ] {
            assert!(
                !denied_debug.contains(concealed_value),
                "denied source diagnostics disclosed {concealed_value}"
            );
        }

        let authorized_sources = [CurrentSourceDescriptor::EntryQuery {
            source_id,
            current_form: Some(&changed_form),
        }];
        let authorized_diagnostics = resolve_composition(ResolveInput {
            composition_revision: CompositionRevisionRef {
                entry_id: id_pair().0,
                revision_id: id_pair().1,
            },
            spec: &spec,
            parameters: &BTreeMap::new(),
            current_sources: &authorized_sources,
        })
        .expect_err("an authorized schema read exposes deterministic field drift");

        assert_eq!(
            authorized_diagnostics,
            vec![CompositionDiagnostic::without_parameter(
                CompositionDiagnosticCode::FieldTypeChanged
            )]
        );
    }

    #[test]
    fn missing_metric_field_is_reported_without_returning_a_partial_plan() {
        let saved_form = form(&[(100, FieldType::Integer), (101, FieldType::String)]);
        let current_form = form(&[(100, FieldType::Integer)]);
        let mut spec = composition_spec(
            Vec::new(),
            vec![entry_query_source(
                "entries",
                &saved_form,
                EntryQueryTemplate {
                    projection: EntryQueryProjectionTemplate::Fields {
                        fields: vec![FieldId::new(101).unwrap()],
                    },
                    ..empty_entry_query_template()
                },
            )],
        );
        spec.components = vec![CompositionComponent::Metric {
            id: "total".to_owned(),
            label: None,
            source: "entries".to_owned(),
            value_field: CompositionMetricValueField::EntryField {
                field_id: FieldId::new(101).unwrap(),
            },
        }];
        spec.layout = single_row_layout("main", &["total"]);
        let current_sources = [CurrentSourceDescriptor::EntryQuery {
            source_id: "entries",
            current_form: Some(&current_form),
        }];

        let diagnostics = resolve_composition(ResolveInput {
            composition_revision: CompositionRevisionRef {
                entry_id: id_pair().0,
                revision_id: id_pair().1,
            },
            spec: &spec,
            parameters: &BTreeMap::new(),
            current_sources: &current_sources,
        })
        .expect_err("a missing projected metric field cannot produce a partial plan");

        assert_eq!(
            diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.code)
                .collect::<Vec<_>>(),
            [CompositionDiagnosticCode::MissingField]
        );
    }

    #[test]
    fn unprojected_entry_query_metric_reports_a_stable_diagnostic() {
        let current_form = form(&[(100, FieldType::String), (101, FieldType::Integer)]);
        let mut query_schema = schema(&current_form);
        query_schema.retain(|field| field.field_id == FieldId::new(100).unwrap());
        let source = CompositionSource::EntryQuery {
            id: "entries".to_owned(),
            form_id: current_form.id,
            field_schema: query_schema,
            query: EntryQueryTemplate {
                projection: EntryQueryProjectionTemplate::Fields {
                    fields: vec![FieldId::new(100).unwrap()],
                },
                ..empty_entry_query_template()
            },
        };
        let mut spec = composition_spec(Vec::new(), vec![source]);
        spec.components = vec![CompositionComponent::Metric {
            id: "total".to_owned(),
            label: None,
            source: "entries".to_owned(),
            value_field: CompositionMetricValueField::EntryField {
                field_id: FieldId::new(101).unwrap(),
            },
        }];
        spec.layout = single_row_layout("main", &["total"]);
        let current_sources = [CurrentSourceDescriptor::EntryQuery {
            source_id: "entries",
            current_form: Some(&current_form),
        }];

        let diagnostics = resolve_composition(ResolveInput {
            composition_revision: CompositionRevisionRef {
                entry_id: id_pair().0,
                revision_id: id_pair().1,
            },
            spec: &spec,
            parameters: &BTreeMap::new(),
            current_sources: &current_sources,
        })
        .expect_err("the metric cannot change the EntryQuery projection");

        assert_eq!(
            diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.code)
                .collect::<Vec<_>>(),
            [CompositionDiagnosticCode::MetricFieldNotProjected]
        );
    }

    #[test]
    fn changed_metric_field_type_is_reported_by_the_source_schema_guard() {
        let saved_form = form(&[(101, FieldType::Integer)]);
        let current_form = form(&[(101, FieldType::String)]);
        let mut spec = composition_spec(
            Vec::new(),
            vec![entry_query_source(
                "entries",
                &saved_form,
                EntryQueryTemplate {
                    projection: EntryQueryProjectionTemplate::Fields {
                        fields: vec![FieldId::new(101).unwrap()],
                    },
                    ..empty_entry_query_template()
                },
            )],
        );
        spec.components = vec![CompositionComponent::Metric {
            id: "total".to_owned(),
            label: None,
            source: "entries".to_owned(),
            value_field: CompositionMetricValueField::EntryField {
                field_id: FieldId::new(101).unwrap(),
            },
        }];
        spec.layout = single_row_layout("main", &["total"]);
        let current_sources = [CurrentSourceDescriptor::EntryQuery {
            source_id: "entries",
            current_form: Some(&current_form),
        }];

        let diagnostics = resolve_composition(ResolveInput {
            composition_revision: CompositionRevisionRef {
                entry_id: id_pair().0,
                revision_id: id_pair().1,
            },
            spec: &spec,
            parameters: &BTreeMap::new(),
            current_sources: &current_sources,
        })
        .expect_err("a changed metric field type invalidates the source binding");

        assert_eq!(
            diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.code)
                .collect::<Vec<_>>(),
            [CompositionDiagnosticCode::FieldTypeChanged]
        );
    }

    #[test]
    fn resolve_aggregates_source_diagnostics_in_document_order_without_a_partial_plan() {
        let expected_form = form(&[(100, FieldType::String)]);
        let current_form = form(&[(100, FieldType::Integer)]);
        let (sql_entry_id, sql_revision_id) = id_pair();
        let spec = spec_with_note(composition_spec(
            Vec::new(),
            vec![
                entry_query_source("entries", &expected_form, empty_entry_query_template()),
                CompositionSource::SavedSql {
                    id: "report".to_owned(),
                    entry_id: sql_entry_id,
                    revision_id: sql_revision_id,
                    expected_result: expected_result(),
                    variables: BTreeMap::new(),
                },
            ],
        ));
        let unavailable_sql = SavedSqlRevisionMetadata {
            id: "concealed-or-missing".to_owned(),
            revision_id: sql_revision_id.to_string(),
            variable_types: BTreeMap::new(),
        };
        let current_sources = [
            CurrentSourceDescriptor::SavedSql {
                source_id: "report",
                current_revision: Some(&unavailable_sql),
            },
            CurrentSourceDescriptor::EntryQuery {
                source_id: "entries",
                current_form: Some(&current_form),
            },
        ];
        let (composition_entry_id, composition_revision_id) = id_pair();

        let diagnostics = resolve_composition(ResolveInput {
            composition_revision: CompositionRevisionRef {
                entry_id: composition_entry_id,
                revision_id: composition_revision_id,
            },
            spec: &spec,
            parameters: &BTreeMap::new(),
            current_sources: &current_sources,
        })
        .expect_err("a failed source prevents returning a partial plan");

        assert_eq!(
            diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.code)
                .collect::<Vec<_>>(),
            vec![
                CompositionDiagnosticCode::FieldTypeChanged,
                CompositionDiagnosticCode::SourceUnavailable,
            ]
        );
    }

    #[test]
    fn saved_sql_schema_fingerprint_tracks_variable_types_deterministically() {
        let (entry_id, revision_id) = id_pair();
        let source = saved_sql_source(
            entry_id,
            revision_id,
            BTreeMap::from([("threshold".to_owned(), value(json!(5)))]),
        );
        let bindings = empty_bindings();
        let integer_revision = saved_sql_metadata(
            entry_id,
            revision_id,
            BTreeMap::from([("threshold".to_owned(), "integer")]),
        );
        let float_revision = saved_sql_metadata(
            entry_id,
            revision_id,
            BTreeMap::from([("threshold".to_owned(), "float")]),
        );

        let integer = compile_saved_sql_source(&source, Some(&integer_revision), &bindings)
            .expect("integer literal binds");
        let float = compile_saved_sql_source(&source, Some(&float_revision), &bindings)
            .expect("the same JSON number is valid for float");
        let repeated = compile_saved_sql_source(&source, Some(&integer_revision), &bindings)
            .expect("the same source fingerprint is deterministic");
        assert_ne!(
            integer.source_schema_fingerprint,
            float.source_schema_fingerprint
        );
        assert_eq!(
            integer.source_schema_fingerprint,
            repeated.source_schema_fingerprint
        );
    }

    #[test]
    fn saved_sql_schema_fingerprint_tracks_ordered_result_descriptors() {
        let (entry_id, revision_id) = id_pair();
        let columns = vec![
            CompositionResultColumn {
                name: "total".to_owned(),
                result_type: CompositionResultFieldType::Float,
            },
            CompositionResultColumn {
                name: "count".to_owned(),
                result_type: CompositionResultFieldType::Integer,
            },
        ];
        let source_for = |expected_result| CompositionSource::SavedSql {
            id: "monthly-expenses".to_owned(),
            entry_id,
            revision_id,
            expected_result,
            variables: BTreeMap::new(),
        };
        let baseline_source = source_for(columns.clone());
        let renamed_source = source_for(vec![
            CompositionResultColumn {
                name: "amount".to_owned(),
                result_type: CompositionResultFieldType::Float,
            },
            columns[1].clone(),
        ]);
        let retyped_source = source_for(vec![
            CompositionResultColumn {
                name: "total".to_owned(),
                result_type: CompositionResultFieldType::Integer,
            },
            columns[1].clone(),
        ]);
        let reordered_source = source_for(vec![columns[1].clone(), columns[0].clone()]);
        let current_revision = saved_sql_metadata(entry_id, revision_id, BTreeMap::new());
        let bindings = empty_bindings();

        let baseline =
            compile_saved_sql_source(&baseline_source, Some(&current_revision), &bindings)
                .expect("the source compiles");
        let renamed = compile_saved_sql_source(&renamed_source, Some(&current_revision), &bindings)
            .expect("the renamed result schema compiles");
        let retyped = compile_saved_sql_source(&retyped_source, Some(&current_revision), &bindings)
            .expect("the retyped result schema compiles");
        let reordered =
            compile_saved_sql_source(&reordered_source, Some(&current_revision), &bindings)
                .expect("the reordered result schema compiles");
        let repeated =
            compile_saved_sql_source(&baseline_source, Some(&current_revision), &bindings)
                .expect("the same source fingerprint is deterministic");

        assert_ne!(
            baseline.source_schema_fingerprint,
            renamed.source_schema_fingerprint
        );
        assert_ne!(
            baseline.source_schema_fingerprint,
            retyped.source_schema_fingerprint
        );
        assert_ne!(
            baseline.source_schema_fingerprint,
            reordered.source_schema_fingerprint
        );
        assert_eq!(
            baseline.source_schema_fingerprint,
            repeated.source_schema_fingerprint
        );
    }

    #[test]
    fn saved_sql_result_fingerprint_tracks_selected_metric_column() {
        let (entry_id, revision_id) = id_pair();
        let source = CompositionSource::SavedSql {
            id: "monthly-expenses".to_owned(),
            entry_id,
            revision_id,
            expected_result: vec![
                CompositionResultColumn {
                    name: "total".to_owned(),
                    result_type: CompositionResultFieldType::Float,
                },
                CompositionResultColumn {
                    name: "count".to_owned(),
                    result_type: CompositionResultFieldType::Integer,
                },
            ],
            variables: BTreeMap::new(),
        };
        let current_revision = saved_sql_metadata(entry_id, revision_id, BTreeMap::new());
        let current_sources = [CurrentSourceDescriptor::SavedSql {
            source_id: "monthly-expenses",
            current_revision: Some(&current_revision),
        }];
        let resolve_selected_column = |column: &str| {
            let mut spec = composition_spec(Vec::new(), vec![source.clone()]);
            spec.components = vec![CompositionComponent::Metric {
                id: "summary".to_owned(),
                label: None,
                source: "monthly-expenses".to_owned(),
                value_field: CompositionMetricValueField::SqlColumn {
                    name: column.to_owned(),
                },
            }];
            spec.layout = single_row_layout("main", &["summary"]);

            let plan = resolve_composition(ResolveInput {
                composition_revision: CompositionRevisionRef {
                    entry_id,
                    revision_id,
                },
                spec: &spec,
                parameters: &BTreeMap::new(),
                current_sources: &current_sources,
            })
            .expect("the selected Saved SQL column resolves");
            let ResolvedSourceRequest::SavedSql {
                source_schema_fingerprint,
                ..
            } = &plan.sources[0]
            else {
                panic!("the plan retains its Saved SQL source");
            };
            source_schema_fingerprint.clone()
        };

        let total = resolve_selected_column("total");
        let count = resolve_selected_column("count");
        let repeated_total = resolve_selected_column("total");

        assert_ne!(total, count);
        assert_eq!(total, repeated_total);
    }
}
