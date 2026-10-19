//! Pure Composition variable binding and source compilation.
//!
//! This module resolves typed Composition values into the existing stateless
//! SQL query contract. It performs no storage reads,
//! grants no authorization, and creates no persistent query state.

use crate::sql_query::{
    SavedSqlRevisionRef, SqlQueryRequest, MAX_SQL_COLUMN_NAME_BYTES, MAX_SQL_OUTPUT_COLUMNS,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use ugoite_domain::composition::{
    CompositionComponent, CompositionDiagnosticCode, CompositionFormBinding, CompositionLiteral,
    CompositionMetricPresentation, CompositionResultColumn, CompositionResultFieldType,
    CompositionSource, CompositionSpec, CompositionTextContent, CompositionValue,
    CompositionVariable, CompositionVariableType,
};
use ugoite_domain::composition_metric::validate_composition_metric_result_type;
use ugoite_domain::id::{EntryId, RevisionId};

/// Bounded default page size for Composition SQL sources.
const DEFAULT_COMPOSITION_SQL_PAGE_LIMIT: usize = 100;

/// One stable diagnostic emitted while binding a variable or value template.
///
/// Variable identifiers are included for caller-value binding errors.
/// Unknown or unset references embedded in a source template are code-only,
/// so resolving them does not disclose a source's SQL variable names.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompositionDiagnostic {
    pub code: CompositionDiagnosticCode,
    pub variable_id: Option<String>,
}

impl CompositionDiagnostic {
    fn for_variable(code: CompositionDiagnosticCode, variable_id: &str) -> Self {
        Self {
            code,
            variable_id: Some(variable_id.to_owned()),
        }
    }

    fn without_variable(code: CompositionDiagnosticCode) -> Self {
        Self {
            code,
            variable_id: None,
        }
    }
}

/// Bound values and deterministic diagnostics for one Composition request.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VariableBindings {
    /// Values supplied by the caller or selected from typed defaults.
    pub values: BTreeMap<String, Value>,
    /// Declared variables whose supplied value failed type validation.
    /// Keeping this state prevents a source reference from misreporting a rejected
    /// value as merely missing.
    pub invalid_values: BTreeSet<String>,
    /// Declared types, including optional variables that are currently unset.
    pub variable_types: BTreeMap<String, CompositionVariableType>,
    pub diagnostics: Vec<CompositionDiagnostic>,
}

/// A value after resolving a literal or named Composition variable.
#[derive(Clone, Debug, PartialEq)]
pub struct BoundCompositionValue {
    pub value: Value,
    /// `None` means the value was a literal in the Composition document.
    pub variable_type: Option<CompositionVariableType>,
}

/// Bind caller values and defaults against the document's variable schema.
///
/// Required variables without values produce `variable_missing`. Optional
/// unset variables remain absent until a source actually references them.
/// Unknown caller keys are reported in lexical order; declared variables are
/// processed in document order, making the complete diagnostic list stable.
pub fn bind_variables(
    variables: &[CompositionVariable],
    supplied: &BTreeMap<String, Value>,
) -> VariableBindings {
    bind_variables_with_unset(variables, supplied, &[])
}

/// Bind caller values and defaults while honoring variables explicitly left
/// unset by an Input. Explicitly unset names override declaration defaults;
/// omission from both collections continues to select the default.
pub fn bind_variables_with_unset(
    variables: &[CompositionVariable],
    supplied: &BTreeMap<String, Value>,
    unset_variables: &[String],
) -> VariableBindings {
    let mut bindings = VariableBindings::default();
    let mut declared = BTreeSet::new();
    let explicitly_unset = unset_variables.iter().cloned().collect::<BTreeSet<_>>();

    for variable in variables {
        if !declared.insert(variable.id.clone()) {
            bindings
                .diagnostics
                .push(CompositionDiagnostic::without_variable(
                    CompositionDiagnosticCode::InvalidComposition,
                ));
            return bindings;
        }
        if variable.default.as_ref().is_some_and(|default| {
            !variable
                .variable_type
                .accepts_value(default.as_json_value())
        }) {
            bindings
                .diagnostics
                .push(CompositionDiagnostic::without_variable(
                    CompositionDiagnosticCode::InvalidComposition,
                ));
            return bindings;
        }
        bindings
            .variable_types
            .insert(variable.id.clone(), variable.variable_type);
    }

    for variable in variables {
        if explicitly_unset.contains(&variable.id) {
            if supplied.contains_key(&variable.id) {
                bindings.invalid_values.insert(variable.id.clone());
                bindings
                    .diagnostics
                    .push(CompositionDiagnostic::for_variable(
                        CompositionDiagnosticCode::VariableTypeMismatch,
                        &variable.id,
                    ));
            } else if variable.required {
                bindings
                    .diagnostics
                    .push(CompositionDiagnostic::for_variable(
                        CompositionDiagnosticCode::VariableMissing,
                        &variable.id,
                    ));
            }
            continue;
        }

        let value = supplied.get(&variable.id).or_else(|| {
            variable
                .default
                .as_ref()
                .map(CompositionLiteral::as_json_value)
        });
        let Some(value) = value else {
            if variable.required {
                bindings
                    .diagnostics
                    .push(CompositionDiagnostic::for_variable(
                        CompositionDiagnosticCode::VariableMissing,
                        &variable.id,
                    ));
            }
            continue;
        };

        if variable.variable_type.accepts_value(value) {
            bindings.values.insert(variable.id.clone(), value.clone());
        } else {
            bindings.invalid_values.insert(variable.id.clone());
            bindings
                .diagnostics
                .push(CompositionDiagnostic::for_variable(
                    CompositionDiagnosticCode::VariableTypeMismatch,
                    &variable.id,
                ));
        }
    }

    let unknown_names = supplied
        .keys()
        .cloned()
        .chain(explicitly_unset.iter().cloned())
        .filter(|name| !declared.contains(name))
        .collect::<BTreeSet<_>>();
    for name in unknown_names {
        bindings
            .diagnostics
            .push(CompositionDiagnostic::for_variable(
                CompositionDiagnosticCode::VariableUnknown,
                &name,
            ));
    }

    bindings
}

/// Resolve a source value without coercion or expression evaluation.
pub fn resolve_value_template(
    template: &CompositionValue,
    bindings: &VariableBindings,
) -> Result<BoundCompositionValue, CompositionDiagnostic> {
    match template {
        CompositionValue::Literal(value) => Ok(BoundCompositionValue {
            value: value.as_json_value().clone(),
            variable_type: None,
        }),
        CompositionValue::Variable(reference) => {
            let Some(variable_type) = bindings.variable_types.get(&reference.variable).copied()
            else {
                return Err(CompositionDiagnostic::without_variable(
                    CompositionDiagnosticCode::VariableUnknown,
                ));
            };
            if bindings.invalid_values.contains(&reference.variable) {
                return Err(CompositionDiagnostic::without_variable(
                    CompositionDiagnosticCode::VariableTypeMismatch,
                ));
            }
            let Some(value) = bindings.values.get(&reference.variable) else {
                return Err(CompositionDiagnostic::without_variable(
                    CompositionDiagnosticCode::VariableMissing,
                ));
            };
            Ok(BoundCompositionValue {
                value: value.clone(),
                variable_type: Some(variable_type),
            })
        }
    }
}

fn value_matches_variable(value: &Value, variable_type: CompositionVariableType) -> bool {
    variable_type.accepts_value(value)
}

/// Metadata produced by the server's SQL planner for the current inline SQL
/// source. Core deliberately does not parse SQL: relation bindings, parameter
/// names, planned output columns, and the stable relation rewrite all come from
/// the authorized server boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineSqlSourceMetadata {
    /// SQL after the server resolves Form relations to their stable internal IDs.
    pub bound_sql: String,
    pub form_bindings: Vec<CompositionFormBinding>,
    pub parameter_names: BTreeSet<String>,
    pub expected_result: Vec<CompositionResultColumn>,
    pub schema_fingerprint: String,
}

/// Metadata for one exact Saved SQL revision, projected by the authorized
/// storage boundary into the transport-neutral Core resolver input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SavedSqlRevisionMetadata {
    pub id: String,
    pub revision_id: String,
    pub variable_types: BTreeMap<String, String>,
    pub expected_result: Vec<CompositionResultColumn>,
    pub schema_fingerprint: String,
}

/// An inline SQL source compiled to the existing stateless SQL request.
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledInlineSqlSource {
    pub request: SqlQueryRequest,
    pub form_bindings: Vec<CompositionFormBinding>,
    pub expected_result: Vec<CompositionResultColumn>,
    /// Composition variable IDs used by this source, sorted and deduplicated.
    pub variable_dependencies: Vec<String>,
    /// The schema fingerprint persisted with this source.
    pub source_schema_fingerprint: String,
}

/// Compile inline SQL against the server's current, authorized SQL plan.
/// Stored Form bindings and result schema are snapshots, never instructions to
/// rebind a relation. The request carries server-rewritten SQL; execution still
/// repeats the ordinary SQL ACL and resource checks.
pub fn compile_inline_sql_source(
    source: &CompositionSource,
    current_schema: Option<&InlineSqlSourceMetadata>,
    bindings: &VariableBindings,
    allow_incomplete_source_descriptors: bool,
) -> Result<CompiledInlineSqlSource, Vec<CompositionDiagnostic>> {
    let CompositionSource::InlineSql {
        form_bindings,
        expected_result,
        schema_fingerprint,
        ..
    } = source
    else {
        return Err(vec![CompositionDiagnostic::without_variable(
            CompositionDiagnosticCode::InvalidComposition,
        )]);
    };
    let Some(current_schema) = current_schema else {
        return Err(vec![CompositionDiagnostic::without_variable(
            CompositionDiagnosticCode::SourceUnavailable,
        )]);
    };
    if current_schema.bound_sql.trim().is_empty() {
        return Err(vec![CompositionDiagnostic::without_variable(
            CompositionDiagnosticCode::SourceUnavailable,
        )]);
    }
    if !result_schema_is_well_formed(
        &current_schema.expected_result,
        &current_schema.schema_fingerprint,
    ) || current_schema
        .form_bindings
        .iter()
        .any(|binding| binding.relation.trim().is_empty())
        || current_schema
            .form_bindings
            .iter()
            .map(|binding| binding.relation.as_str())
            .collect::<BTreeSet<_>>()
            .len()
            != current_schema.form_bindings.len()
        || current_schema
            .parameter_names
            .iter()
            .any(|name| name.trim().is_empty())
    {
        return Err(vec![CompositionDiagnostic::without_variable(
            CompositionDiagnosticCode::SourceUnavailable,
        )]);
    }
    let draft_placeholder =
        form_bindings.is_empty() && expected_result.is_empty() && schema_fingerprint.is_empty();
    if !(allow_incomplete_source_descriptors && draft_placeholder)
        && (&current_schema.form_bindings != form_bindings
            || &current_schema.expected_result != expected_result
            || &current_schema.schema_fingerprint != schema_fingerprint)
    {
        return Err(vec![CompositionDiagnostic::without_variable(
            CompositionDiagnosticCode::SourceSchemaChanged,
        )]);
    }

    let mut parameters = serde_json::Map::new();
    let mut parameter_types = BTreeMap::new();
    let mut variable_dependencies = BTreeSet::new();
    let mut diagnostics = Vec::new();
    for variable_id in &current_schema.parameter_names {
        let Some(variable_type) = bindings.variable_types.get(variable_id).copied() else {
            diagnostics.push(CompositionDiagnostic::without_variable(
                CompositionDiagnosticCode::VariableUnknown,
            ));
            continue;
        };
        let Some(value) = bindings.values.get(variable_id) else {
            diagnostics.push(CompositionDiagnostic::without_variable(
                CompositionDiagnosticCode::VariableMissing,
            ));
            continue;
        };
        if !value_matches_variable(value, variable_type) {
            diagnostics.push(CompositionDiagnostic::without_variable(
                CompositionDiagnosticCode::VariableTypeMismatch,
            ));
            continue;
        }
        variable_dependencies.insert(variable_id.clone());
        parameters.insert(variable_id.clone(), value.clone());
        parameter_types.insert(
            variable_id.clone(),
            sql_type_for_composition_variable(variable_type).to_owned(),
        );
    }
    if !diagnostics.is_empty() {
        return Err(deduplicate_diagnostics(diagnostics));
    }

    let request = SqlQueryRequest {
        sql: current_schema.bound_sql.clone(),
        parameters,
        parameter_types,
        include_exact_integer_values: false,
        limit: DEFAULT_COMPOSITION_SQL_PAGE_LIMIT,
        continuation: None,
        execution_id: None,
        saved_sql: None,
    };
    if request.validate().is_err() {
        return Err(vec![CompositionDiagnostic::without_variable(
            CompositionDiagnosticCode::InvalidComposition,
        )]);
    }
    Ok(CompiledInlineSqlSource {
        request,
        form_bindings: current_schema.form_bindings.clone(),
        expected_result: current_schema.expected_result.clone(),
        variable_dependencies: variable_dependencies.into_iter().collect(),
        source_schema_fingerprint: if draft_placeholder && allow_incomplete_source_descriptors {
            current_schema.schema_fingerprint.clone()
        } else {
            schema_fingerprint.clone()
        },
    })
}

/// An exact Saved SQL source compiled to the existing stateless SQL request.
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledSavedSqlSource {
    pub request: SqlQueryRequest,
    pub expected_result: Vec<CompositionResultColumn>,
    /// Composition variable IDs bound into this source, sorted and deduplicated.
    pub variable_dependencies: Vec<String>,
    /// The schema fingerprint pinned by this exact revision.
    pub source_schema_fingerprint: String,
}

/// Compile a Composition Saved SQL source against metadata read for its exact
/// pinned revision. Missing or concealed reads share one stable diagnostic.
pub fn compile_saved_sql_source(
    source: &CompositionSource,
    current_revision: Option<&SavedSqlRevisionMetadata>,
    bindings: &VariableBindings,
) -> Result<CompiledSavedSqlSource, Vec<CompositionDiagnostic>> {
    let CompositionSource::SavedSql {
        entry_id,
        revision_id,
        expected_result,
        schema_fingerprint,
        variables,
        ..
    } = source
    else {
        return Err(vec![CompositionDiagnostic::without_variable(
            CompositionDiagnosticCode::InvalidComposition,
        )]);
    };
    let Some(current_revision) = current_revision else {
        return Err(vec![CompositionDiagnostic::without_variable(
            CompositionDiagnosticCode::SourceUnavailable,
        )]);
    };

    let expected_id = entry_id.to_string();
    let expected_revision_id = revision_id.to_string();
    if current_revision.id != expected_id || current_revision.revision_id != expected_revision_id {
        return Err(vec![CompositionDiagnostic::without_variable(
            CompositionDiagnosticCode::SourceUnavailable,
        )]);
    }
    if !result_schema_is_well_formed(
        &current_revision.expected_result,
        &current_revision.schema_fingerprint,
    ) || current_revision.expected_result != *expected_result
        || current_revision.schema_fingerprint != *schema_fingerprint
    {
        return Err(vec![CompositionDiagnostic::without_variable(
            CompositionDiagnosticCode::SourceSchemaChanged,
        )]);
    }

    let mut diagnostics = Vec::new();
    let mut parameters = serde_json::Map::new();
    let mut parameter_types = BTreeMap::new();
    let mut variable_dependencies = BTreeSet::new();
    for (variable_name, value_template) in variables {
        let Some(sql_type_name) = current_revision.variable_types.get(variable_name) else {
            diagnostics.push(CompositionDiagnostic::without_variable(
                CompositionDiagnosticCode::SourceSchemaChanged,
            ));
            continue;
        };
        let Some(variable_type) = composition_variable_type_for_sql(sql_type_name) else {
            diagnostics.push(CompositionDiagnostic::without_variable(
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
            .variable_type
            .is_some_and(|bound_type| bound_type != variable_type)
        {
            diagnostics.push(CompositionDiagnostic::without_variable(
                CompositionDiagnosticCode::VariableTypeMismatch,
            ));
            continue;
        }
        if (bound.value.is_null() && bound.variable_type.is_some())
            || (!bound.value.is_null() && !value_matches_variable(&bound.value, variable_type))
        {
            diagnostics.push(CompositionDiagnostic::without_variable(
                if bound.variable_type.is_some() {
                    CompositionDiagnosticCode::VariableTypeMismatch
                } else {
                    CompositionDiagnosticCode::InvalidComposition
                },
            ));
            continue;
        }
        if let CompositionValue::Variable(reference) = value_template {
            variable_dependencies.insert(reference.variable.clone());
        }
        parameters.insert(variable_name.clone(), bound.value);
        parameter_types.insert(variable_name.clone(), sql_type_name.clone());
    }
    if current_revision
        .variable_types
        .keys()
        .any(|name| !variables.contains_key(name))
    {
        diagnostics.push(CompositionDiagnostic::without_variable(
            CompositionDiagnosticCode::VariableMissing,
        ));
    }
    if !diagnostics.is_empty() {
        return Err(deduplicate_diagnostics(diagnostics));
    }

    let request = SqlQueryRequest {
        // Saved SQL text is owned by the exact immutable revision. Supplying an
        // empty body prevents a second copy from becoming query authority.
        sql: String::new(),
        parameters,
        parameter_types,
        include_exact_integer_values: false,
        limit: DEFAULT_COMPOSITION_SQL_PAGE_LIMIT,
        continuation: None,
        execution_id: None,
        saved_sql: Some(SavedSqlRevisionRef {
            id: expected_id,
            revision_id: expected_revision_id,
        }),
    };
    if request.validate().is_err() {
        return Err(vec![CompositionDiagnostic::without_variable(
            CompositionDiagnosticCode::InvalidComposition,
        )]);
    }

    Ok(CompiledSavedSqlSource {
        request,
        expected_result: current_revision.expected_result.clone(),
        variable_dependencies: variable_dependencies.into_iter().collect(),
        source_schema_fingerprint: schema_fingerprint.clone(),
    })
}

fn composition_variable_type_for_sql(variable_type: &str) -> Option<CompositionVariableType> {
    match variable_type {
        "string" => Some(CompositionVariableType::String),
        "boolean" => Some(CompositionVariableType::Boolean),
        "integer" => Some(CompositionVariableType::Integer),
        "float" => Some(CompositionVariableType::Float),
        "date" => Some(CompositionVariableType::Date),
        "timestamp" => Some(CompositionVariableType::Timestamp),
        _ => None,
    }
}

fn result_schema_is_well_formed(columns: &[CompositionResultColumn], fingerprint: &str) -> bool {
    !columns.is_empty()
        && columns.len() <= MAX_SQL_OUTPUT_COLUMNS
        && columns.iter().all(|column| {
            !column.name.trim().is_empty() && column.name.len() <= MAX_SQL_COLUMN_NAME_BYTES
        })
        && columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<BTreeSet<_>>()
            .len()
            == columns.len()
        && fingerprint.len() == 64
        && fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn sql_type_for_composition_variable(variable_type: CompositionVariableType) -> &'static str {
    match variable_type {
        CompositionVariableType::String => "string",
        CompositionVariableType::Boolean => "boolean",
        CompositionVariableType::Integer => "integer",
        CompositionVariableType::Float => "float",
        CompositionVariableType::Date => "date",
        CompositionVariableType::Timestamp => "timestamp",
    }
}

fn deduplicate_diagnostics(diagnostics: Vec<CompositionDiagnostic>) -> Vec<CompositionDiagnostic> {
    let mut seen = BTreeSet::new();
    diagnostics
        .into_iter()
        .filter(|diagnostic| {
            seen.insert((
                diagnostic.code.as_str().to_owned(),
                diagnostic.variable_id.clone(),
            ))
        })
        .collect()
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
#[derive(Clone, Copy)]
pub enum CurrentSourceDescriptor<'a> {
    InlineSql {
        source_id: &'a str,
        current_schema: Option<&'a InlineSqlSourceMetadata>,
    },
    SavedSql {
        source_id: &'a str,
        current_revision: Option<&'a SavedSqlRevisionMetadata>,
    },
}

impl CurrentSourceDescriptor<'_> {
    fn source_id(&self) -> &str {
        match self {
            Self::InlineSql { source_id, .. } | Self::SavedSql { source_id, .. } => source_id,
        }
    }
}

/// Inputs for one pure Composition resolution pass. The Composition has
/// already been parsed and version-checked; every supplied current descriptor
/// must have been read through the current authorization boundary.
pub struct ResolveInput<'a> {
    pub composition_revision: CompositionRevisionRef,
    pub spec: &'a CompositionSpec,
    pub variables: &'a BTreeMap<String, Value>,
    /// Descriptors may arrive in any order; results always follow spec order.
    pub current_sources: &'a [CurrentSourceDescriptor<'a>],
}

/// One source-local request compiled into an existing bounded query contract.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResolvedSourceRequest {
    Sql {
        source_id: String,
        request: SqlQueryRequest,
        source_schema_fingerprint: String,
        form_bindings: Vec<CompositionFormBinding>,
        expected_result: Vec<CompositionResultColumn>,
    },
}

/// A variable's direct runtime consumers, independent of layout position.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariableConsumers {
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub input_component_ids: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub text_component_ids: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub source_ids: BTreeSet<String>,
}

/// One source's visual consumers, independent of layout position.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceConsumers {
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub metric_component_ids: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub table_component_ids: BTreeSet<String>,
}

/// Typed variable-to-consumer and source-to-component edges for one plan.
///
/// These semantic edges are built by Core from the validated Composition
/// definition. Browser adapters can schedule only affected work without
/// reparsing YAML or inferring dependencies from component layout order.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionDependencyIndex {
    #[serde(default)]
    pub variable_consumers: BTreeMap<String, VariableConsumers>,
    #[serde(default)]
    pub source_consumers: BTreeMap<String, SourceConsumers>,
}

/// Renderer-neutral component kind after the Composition layout has been
/// validated and ordered.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedComponentKind {
    Input,
    Metric,
    #[serde(rename = "table")]
    Tabular,
    Text,
}

/// One component's stable binding in deterministic layout render order.
///
/// Metric and table bindings carry their source request identity. Input and
/// variable-backed Text bindings carry only their declared variable identity;
/// neither creates a source request. Literal Text carries neither binding.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedComponentBinding {
    pub component_id: String,
    pub kind: ResolvedComponentKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variable_id: Option<String>,
    /// Selected SQL output column alias for a Metric component.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_property_key: Option<String>,
    /// Portable logical result type for scalar metric components.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_result_type: Option<CompositionResultFieldType>,
    /// Display-only metric formatting carried through the resolution plan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<CompositionMetricPresentation>,
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
    /// Optional on the portable plan when there are no reactive dependencies.
    #[serde(default)]
    pub dependencies: CompositionDependencyIndex,
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
    /// Optional on the portable plan when there are no reactive dependencies.
    #[serde(default)]
    pub dependencies: CompositionDependencyIndex,
}

/// Resolve all sources as one all-or-nothing plan.
///
/// Caller variable diagnostics are a global gate: no source is compiled when
/// any value is missing, unknown, or has the wrong type. Source diagnostics
/// are then aggregated in Composition document order. No query is executed by
/// this function; the returned requests still go through their ordinary ACL,
/// bounds, and continuation checks when executed.
pub fn resolve_composition(
    input: ResolveInput<'_>,
) -> Result<ResolvedCompositionPlan, Vec<CompositionDiagnostic>> {
    resolve_composition_with_unset(input, &[])
}

/// Resolve a saved Composition while distinguishing explicit unset values
/// from absent values that should use their declared defaults.
pub fn resolve_composition_with_unset(
    input: ResolveInput<'_>,
    unset_variables: &[String],
) -> Result<ResolvedCompositionPlan, Vec<CompositionDiagnostic>> {
    let (sources, component_bindings, dependencies) = resolve_composition_parts(
        input.spec,
        input.variables,
        unset_variables,
        input.current_sources,
        false,
    )?;
    Ok(ResolvedCompositionPlan {
        composition_revision: input.composition_revision,
        sources,
        component_bindings,
        dependencies,
    })
}

/// Resolve one unsaved candidate document through the same semantics as a
/// saved revision. Identity is the draft fingerprint, never a revision: the
/// plan carries no entry or revision reference and must not be stored,
/// published, or given history.
pub fn resolve_composition_preview(
    spec: &CompositionSpec,
    variables: &BTreeMap<String, Value>,
    current_sources: &[CurrentSourceDescriptor<'_>],
    draft_fingerprint: String,
) -> Result<PreviewCompositionPlan, Vec<CompositionDiagnostic>> {
    resolve_composition_preview_with_unset(spec, variables, &[], current_sources, draft_fingerprint)
}

/// Resolve a draft Composition while distinguishing explicit unset values
/// from absent values that should use their declared defaults.
pub fn resolve_composition_preview_with_unset(
    spec: &CompositionSpec,
    variables: &BTreeMap<String, Value>,
    unset_variables: &[String],
    current_sources: &[CurrentSourceDescriptor<'_>],
    draft_fingerprint: String,
) -> Result<PreviewCompositionPlan, Vec<CompositionDiagnostic>> {
    let (sources, component_bindings, dependencies) =
        resolve_composition_parts(spec, variables, unset_variables, current_sources, true)?;
    Ok(PreviewCompositionPlan {
        draft_fingerprint,
        sources,
        component_bindings,
        dependencies,
    })
}

/// Shared plan-building core for saved revisions and unsaved drafts.
fn resolve_composition_parts(
    spec: &CompositionSpec,
    variables: &BTreeMap<String, Value>,
    unset_variables: &[String],
    current_sources: &[CurrentSourceDescriptor<'_>],
    allow_incomplete_source_descriptors: bool,
) -> Result<
    (
        Vec<ResolvedSourceRequest>,
        Vec<ResolvedComponentBinding>,
        CompositionDependencyIndex,
    ),
    Vec<CompositionDiagnostic>,
> {
    let bindings = bind_variables_with_unset(&spec.variables, variables, unset_variables);
    if !bindings.diagnostics.is_empty() {
        return Err(bindings.diagnostics);
    }

    let render_components = spec
        .components_in_render_order()
        .map_err(|code| vec![CompositionDiagnostic::without_variable(code)])?;

    let source_ids = spec
        .sources
        .iter()
        .map(|source| source.id().to_owned())
        .collect::<BTreeSet<_>>();
    if source_ids.len() != spec.sources.len() {
        return Err(vec![CompositionDiagnostic::without_variable(
            CompositionDiagnosticCode::InvalidComposition,
        )]);
    }
    let mut dependencies = CompositionDependencyIndex {
        variable_consumers: spec
            .variables
            .iter()
            .map(|variable| (variable.id.clone(), VariableConsumers::default()))
            .collect(),
        source_consumers: spec
            .sources
            .iter()
            .map(|source| (source.id().to_owned(), SourceConsumers::default()))
            .collect(),
    };
    let sources_by_id = spec
        .sources
        .iter()
        .map(|source| (source.id(), source))
        .collect::<BTreeMap<_, _>>();
    let mut current_by_id = BTreeMap::new();
    for descriptor in current_sources {
        let source_id = descriptor.source_id();
        if !source_ids.contains(source_id) || current_by_id.insert(source_id, descriptor).is_some()
        {
            return Err(vec![CompositionDiagnostic::without_variable(
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
                CompositionSource::InlineSql { .. },
                Some(CurrentSourceDescriptor::InlineSql { current_schema, .. }),
            ) => match compile_inline_sql_source(
                source,
                *current_schema,
                &bindings,
                allow_incomplete_source_descriptors,
            ) {
                Ok(compiled) => {
                    record_source_dependencies(
                        &mut dependencies,
                        source_id,
                        &compiled.variable_dependencies,
                    )
                    .map_err(|diagnostic| vec![diagnostic])?;
                    resolved_sources.push(ResolvedSourceRequest::Sql {
                        source_id: source_id.to_owned(),
                        request: compiled.request,
                        source_schema_fingerprint: compiled.source_schema_fingerprint,
                        form_bindings: compiled.form_bindings,
                        expected_result: compiled.expected_result,
                    });
                }
                Err(source_diagnostics) => diagnostics.extend(source_diagnostics),
            },
            (
                CompositionSource::SavedSql { .. },
                Some(CurrentSourceDescriptor::SavedSql {
                    current_revision, ..
                }),
            ) => match compile_saved_sql_source(source, *current_revision, &bindings) {
                Ok(compiled) => {
                    record_source_dependencies(
                        &mut dependencies,
                        source_id,
                        &compiled.variable_dependencies,
                    )
                    .map_err(|diagnostic| vec![diagnostic])?;
                    resolved_sources.push(ResolvedSourceRequest::Sql {
                        source_id: source_id.to_owned(),
                        request: compiled.request,
                        source_schema_fingerprint: compiled.source_schema_fingerprint,
                        form_bindings: Vec::new(),
                        expected_result: compiled.expected_result,
                    });
                }
                Err(source_diagnostics) => diagnostics.extend(source_diagnostics),
            },
            _ => diagnostics.push(CompositionDiagnostic::without_variable(
                CompositionDiagnosticCode::SourceUnavailable,
            )),
        }
    }

    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let component_bindings =
        resolve_component_bindings(&render_components, &sources_by_id, &mut dependencies)?;

    Ok((resolved_sources, component_bindings, dependencies))
}

fn record_source_dependencies(
    dependencies: &mut CompositionDependencyIndex,
    source_id: &str,
    variable_ids: &[String],
) -> Result<(), CompositionDiagnostic> {
    for variable_id in variable_ids {
        let Some(consumers) = dependencies.variable_consumers.get_mut(variable_id) else {
            return Err(CompositionDiagnostic::without_variable(
                CompositionDiagnosticCode::InvalidComposition,
            ));
        };
        consumers.source_ids.insert(source_id.to_owned());
    }
    Ok(())
}

fn resolve_component_bindings(
    components: &[&CompositionComponent],
    sources_by_id: &BTreeMap<&str, &CompositionSource>,
    dependencies: &mut CompositionDependencyIndex,
) -> Result<Vec<ResolvedComponentBinding>, Vec<CompositionDiagnostic>> {
    let mut resolved = Vec::with_capacity(components.len());
    for component in components {
        let component_id = component.id();
        match component {
            CompositionComponent::Input { variable, .. } => {
                let Some(consumers) = dependencies.variable_consumers.get_mut(variable) else {
                    return Err(vec![CompositionDiagnostic::without_variable(
                        CompositionDiagnosticCode::InvalidComposition,
                    )]);
                };
                consumers
                    .input_component_ids
                    .insert(component_id.to_owned());
                resolved.push(ResolvedComponentBinding {
                    component_id: component_id.to_owned(),
                    kind: ResolvedComponentKind::Input,
                    source_id: None,
                    variable_id: Some(variable.clone()),
                    result_property_key: None,
                    expected_result_type: None,
                    presentation: None,
                });
            }
            CompositionComponent::Text { content, .. } => {
                let variable_id = match content {
                    CompositionTextContent::Literal { .. } => None,
                    CompositionTextContent::Variable { variable } => {
                        let Some(consumers) = dependencies.variable_consumers.get_mut(variable)
                        else {
                            return Err(vec![CompositionDiagnostic::without_variable(
                                CompositionDiagnosticCode::InvalidComposition,
                            )]);
                        };
                        consumers.text_component_ids.insert(component_id.to_owned());
                        Some(variable.clone())
                    }
                };
                resolved.push(ResolvedComponentBinding {
                    component_id: component_id.to_owned(),
                    kind: ResolvedComponentKind::Text,
                    source_id: None,
                    variable_id,
                    result_property_key: None,
                    expected_result_type: None,
                    presentation: None,
                });
            }
            CompositionComponent::Metric {
                source,
                column,
                presentation,
                ..
            } => {
                let source_id = source.as_str();
                let source_definition = sources_by_id.get(source_id).copied().ok_or_else(|| {
                    vec![CompositionDiagnostic::without_variable(
                        CompositionDiagnosticCode::InvalidComposition,
                    )]
                })?;
                let expected_result = source_definition.expected_result();
                let matching_columns = expected_result
                    .iter()
                    .filter(|result_column| result_column.name == *column)
                    .collect::<Vec<_>>();
                let result_column = match matching_columns.as_slice() {
                    [] => {
                        return Err(vec![CompositionDiagnostic::without_variable(
                            CompositionDiagnosticCode::MetricResultColumnMissing,
                        )]);
                    }
                    [result_column] => *result_column,
                    _ => {
                        return Err(vec![CompositionDiagnostic::without_variable(
                            CompositionDiagnosticCode::MetricResultColumnAmbiguous,
                        )]);
                    }
                };
                validate_composition_metric_result_type(result_column.result_type)
                    .map_err(|code| vec![CompositionDiagnostic::without_variable(code.into())])?;
                if presentation.is_some()
                    && !matches!(
                        result_column.result_type,
                        CompositionResultFieldType::Integer
                            | CompositionResultFieldType::Float
                            | CompositionResultFieldType::Decimal
                    )
                {
                    return Err(vec![CompositionDiagnostic::without_variable(
                        CompositionDiagnosticCode::MetricResultNotScalar,
                    )]);
                }
                let Some(consumers) = dependencies.source_consumers.get_mut(source_id) else {
                    return Err(vec![CompositionDiagnostic::without_variable(
                        CompositionDiagnosticCode::InvalidComposition,
                    )]);
                };
                consumers
                    .metric_component_ids
                    .insert(component_id.to_owned());
                resolved.push(ResolvedComponentBinding {
                    component_id: component_id.to_owned(),
                    kind: ResolvedComponentKind::Metric,
                    source_id: Some(source_id.to_owned()),
                    variable_id: None,
                    result_property_key: Some(column.clone()),
                    expected_result_type: Some(result_column.result_type),
                    presentation: presentation.clone(),
                });
            }
            CompositionComponent::Table { source, .. } => {
                let source_id = source.as_str();
                if !sources_by_id.contains_key(source_id) {
                    return Err(vec![CompositionDiagnostic::without_variable(
                        CompositionDiagnosticCode::InvalidComposition,
                    )]);
                }
                let Some(consumers) = dependencies.source_consumers.get_mut(source_id) else {
                    return Err(vec![CompositionDiagnostic::without_variable(
                        CompositionDiagnosticCode::InvalidComposition,
                    )]);
                };
                consumers
                    .table_component_ids
                    .insert(component_id.to_owned());
                resolved.push(ResolvedComponentBinding {
                    component_id: component_id.to_owned(),
                    kind: ResolvedComponentKind::Tabular,
                    source_id: Some(source_id.to_owned()),
                    variable_id: None,
                    result_property_key: None,
                    expected_result_type: None,
                    presentation: None,
                });
            }
        }
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::{
        bind_variables, compile_inline_sql_source, compile_saved_sql_source, resolve_composition,
        resolve_composition_preview, CompositionRevisionRef, CurrentSourceDescriptor,
        InlineSqlSourceMetadata, ResolveInput, ResolvedSourceRequest, SavedSqlRevisionMetadata,
    };
    use serde_json::{json, Value};
    use std::collections::{BTreeMap, BTreeSet};
    use ugoite_domain::composition::{
        CompositionComponent, CompositionDiagnosticCode, CompositionFormBinding,
        CompositionResultColumn, CompositionResultFieldType, CompositionSource, CompositionSpec,
        CompositionVariable, CompositionVariableType, DashboardFlowLayout, FlowItem,
        FlowLayoutKind, FlowRow,
    };
    use ugoite_domain::id::{EntryId, FormId, RevisionId};

    fn result(name: &str, result_type: CompositionResultFieldType) -> Vec<CompositionResultColumn> {
        vec![CompositionResultColumn {
            name: name.to_owned(),
            result_type,
        }]
    }

    fn form_binding(relation: &str) -> Vec<CompositionFormBinding> {
        vec![CompositionFormBinding {
            relation: relation.to_owned(),
            form_id: FormId::from(uuid::Uuid::from_u128(42)),
        }]
    }

    fn inline_source(
        bindings: Vec<CompositionFormBinding>,
        columns: Vec<CompositionResultColumn>,
        fingerprint: &str,
    ) -> CompositionSource {
        CompositionSource::InlineSql {
            id: "rows".to_owned(),
            name: "Rows".to_owned(),
            sql: "SELECT * FROM \"Records\"".to_owned(),
            form_bindings: bindings,
            expected_result: columns,
            schema_fingerprint: fingerprint.to_owned(),
        }
    }

    fn inline_metadata(
        bindings: Vec<CompositionFormBinding>,
        columns: Vec<CompositionResultColumn>,
        fingerprint: &str,
        parameter_names: &[&str],
    ) -> InlineSqlSourceMetadata {
        InlineSqlSourceMetadata {
            bound_sql: "SELECT * FROM \"__form_42\"".to_owned(),
            form_bindings: bindings,
            parameter_names: parameter_names
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
            expected_result: columns,
            schema_fingerprint: fingerprint.to_owned(),
        }
    }

    fn variable(id: &str, variable_type: CompositionVariableType) -> CompositionVariable {
        CompositionVariable {
            id: id.to_owned(),
            variable_type,
            required: false,
            default: None,
            format: None,
        }
    }

    fn bindings(
        variables: &[CompositionVariable],
        values: &[(&str, Value)],
    ) -> super::VariableBindings {
        let supplied = values
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect::<BTreeMap<_, _>>();
        bind_variables(variables, &supplied)
    }

    fn composition_spec(
        variables: Vec<CompositionVariable>,
        source: CompositionSource,
        components: Vec<CompositionComponent>,
    ) -> CompositionSpec {
        let items = components
            .iter()
            .map(|component| FlowItem::Component {
                component: component.id().to_owned(),
            })
            .collect();
        CompositionSpec {
            variables,
            sources: vec![source],
            components,
            layout: DashboardFlowLayout {
                kind: FlowLayoutKind::Flow,
                rows: vec![FlowRow {
                    id: "main".to_owned(),
                    items,
                }],
            },
        }
    }

    fn source_revision() -> (EntryId, RevisionId) {
        (
            EntryId::from_uuid(uuid::Uuid::from_u128(100)),
            RevisionId::from_uuid(uuid::Uuid::from_u128(200)),
        )
    }

    fn saved_source(
        entry_id: EntryId,
        revision_id: RevisionId,
        columns: Vec<CompositionResultColumn>,
        fingerprint: &str,
        variables: BTreeMap<String, super::CompositionValue>,
    ) -> CompositionSource {
        CompositionSource::SavedSql {
            id: "summary".to_owned(),
            entry_id,
            revision_id,
            expected_result: columns,
            schema_fingerprint: fingerprint.to_owned(),
            variables,
        }
    }

    fn saved_metadata(
        entry_id: EntryId,
        revision_id: RevisionId,
        columns: Vec<CompositionResultColumn>,
        fingerprint: &str,
        variable_types: &[(&str, &str)],
    ) -> SavedSqlRevisionMetadata {
        SavedSqlRevisionMetadata {
            id: entry_id.to_string(),
            revision_id: revision_id.to_string(),
            variable_types: variable_types
                .iter()
                .map(|(name, kind)| ((*name).to_owned(), (*kind).to_owned()))
                .collect(),
            expected_result: columns,
            schema_fingerprint: fingerprint.to_owned(),
        }
    }

    #[test]
    fn inline_sql_uses_server_bound_sql_and_typed_parameters() {
        let definitions = [variable("active", CompositionVariableType::Boolean)];
        let bound = bindings(&definitions, &[("active", json!(true))]);
        let columns = result("active", CompositionResultFieldType::Boolean);
        let forms = form_binding("Records");
        let source = inline_source(forms.clone(), columns.clone(), &"a".repeat(64));
        let metadata = inline_metadata(forms, columns, &"a".repeat(64), &["active"]);

        let compiled = compile_inline_sql_source(&source, Some(&metadata), &bound, false)
            .expect("the current server plan matches the stored snapshot");

        assert_eq!(compiled.request.sql, metadata.bound_sql);
        assert_eq!(
            Value::Object(compiled.request.parameters.clone()),
            json!({"active": true})
        );
        assert_eq!(compiled.request.parameter_types["active"], "boolean");
        assert!(compiled.request.saved_sql.is_none());
        assert_eq!(compiled.variable_dependencies, ["active"]);
        assert_eq!(compiled.source_schema_fingerprint, "a".repeat(64));
    }

    #[test]
    fn inline_sql_rejects_stale_server_schema_and_unavailable_sources() {
        let columns = result("record_id", CompositionResultFieldType::String);
        let forms = form_binding("Records");
        let source = inline_source(forms.clone(), columns.clone(), &"a".repeat(64));
        let changed = inline_metadata(forms, columns, &"b".repeat(64), &[]);
        let empty = bindings(&[], &[]);

        assert_eq!(
            compile_inline_sql_source(&source, Some(&changed), &empty, false).unwrap_err()[0].code,
            CompositionDiagnosticCode::SourceSchemaChanged,
        );
        assert_eq!(
            compile_inline_sql_source(&source, None, &empty, false).unwrap_err()[0].code,
            CompositionDiagnosticCode::SourceUnavailable,
        );
    }

    #[test]
    fn inline_sql_reports_unknown_and_missing_typed_parameters() {
        let definitions = [variable("active", CompositionVariableType::Boolean)];
        let source = inline_source(
            vec![],
            result("active", CompositionResultFieldType::Boolean),
            &"a".repeat(64),
        );
        let metadata = inline_metadata(
            vec![],
            result("active", CompositionResultFieldType::Boolean),
            &"a".repeat(64),
            &["missing"],
        );

        assert_eq!(
            compile_inline_sql_source(
                &source,
                Some(&metadata),
                &bindings(&definitions, &[]),
                false
            )
            .unwrap_err()[0]
                .code,
            CompositionDiagnosticCode::VariableUnknown,
        );

        let metadata = inline_metadata(
            vec![],
            result("active", CompositionResultFieldType::Boolean),
            &"a".repeat(64),
            &["active"],
        );
        assert_eq!(
            compile_inline_sql_source(
                &source,
                Some(&metadata),
                &bindings(&definitions, &[]),
                false
            )
            .unwrap_err()[0]
                .code,
            CompositionDiagnosticCode::VariableMissing,
        );
    }

    #[test]
    fn saved_sql_uses_exact_revision_and_keeps_parameters_stateless() {
        let (entry_id, revision_id) = source_revision();
        let columns = vec![
            CompositionResultColumn {
                name: "total".to_owned(),
                result_type: CompositionResultFieldType::Integer,
            },
            CompositionResultColumn {
                name: "currency".to_owned(),
                result_type: CompositionResultFieldType::String,
            },
        ];
        let fingerprint = "c".repeat(64);
        let variables = BTreeMap::from([(
            "minimum".to_owned(),
            super::CompositionValue::Variable(
                ugoite_domain::composition::CompositionVariableReference {
                    variable: "min_total".to_owned(),
                },
            ),
        )]);
        let source = saved_source(
            entry_id,
            revision_id,
            columns.clone(),
            &fingerprint,
            variables,
        );
        let metadata = saved_metadata(
            entry_id,
            revision_id,
            columns,
            &fingerprint,
            &[("minimum", "integer")],
        );
        let definitions = [variable("min_total", CompositionVariableType::Integer)];
        let bound = bindings(&definitions, &[("min_total", json!(3))]);

        let compiled = compile_saved_sql_source(&source, Some(&metadata), &bound)
            .expect("the exact revision and its schema match");

        assert!(compiled.request.sql.is_empty());
        assert_eq!(
            compiled.request.saved_sql,
            Some(crate::sql_query::SavedSqlRevisionRef {
                id: entry_id.to_string(),
                revision_id: revision_id.to_string(),
            })
        );
        assert_eq!(
            Value::Object(compiled.request.parameters.clone()),
            json!({"minimum": 3})
        );
        assert_eq!(compiled.request.parameter_types["minimum"], "integer");
        assert_eq!(compiled.variable_dependencies, ["min_total"]);
        assert_eq!(compiled.source_schema_fingerprint, fingerprint);
    }

    #[test]
    fn saved_sql_rejects_changed_schema_and_non_exact_revision() {
        let (entry_id, revision_id) = source_revision();
        let columns = result("total", CompositionResultFieldType::Integer);
        let fingerprint = "c".repeat(64);
        let source = saved_source(
            entry_id,
            revision_id,
            columns.clone(),
            &fingerprint,
            BTreeMap::new(),
        );
        let changed = saved_metadata(
            entry_id,
            revision_id,
            result("count", CompositionResultFieldType::Integer),
            &fingerprint,
            &[],
        );
        assert_eq!(
            compile_saved_sql_source(&source, Some(&changed), &bindings(&[], &[])).unwrap_err()[0]
                .code,
            CompositionDiagnosticCode::SourceSchemaChanged,
        );

        let wrong_revision = saved_metadata(
            entry_id,
            RevisionId::from_uuid(uuid::Uuid::from_u128(201)),
            columns,
            &fingerprint,
            &[],
        );
        assert_eq!(
            compile_saved_sql_source(&source, Some(&wrong_revision), &bindings(&[], &[]),)
                .unwrap_err()[0]
                .code,
            CompositionDiagnosticCode::SourceUnavailable,
        );
    }

    #[test]
    fn preview_resolves_an_empty_inline_descriptor_from_current_server_metadata() {
        let source = CompositionSource::InlineSql {
            id: "rows".to_owned(),
            name: "Rows".to_owned(),
            sql: "SELECT * FROM \"Records\"".to_owned(),
            form_bindings: vec![],
            expected_result: vec![],
            schema_fingerprint: String::new(),
        };
        let spec = composition_spec(
            vec![],
            source,
            vec![CompositionComponent::Table {
                id: "records".to_owned(),
                source: "rows".to_owned(),
            }],
        );
        let columns = result("record_id", CompositionResultFieldType::String);
        let metadata = inline_metadata(form_binding("Records"), columns, &"d".repeat(64), &[]);
        let descriptors = [CurrentSourceDescriptor::InlineSql {
            source_id: "rows",
            current_schema: Some(&metadata),
        }];

        let preview =
            resolve_composition_preview(&spec, &BTreeMap::new(), &descriptors, "draft".to_owned())
                .expect("the exact empty draft placeholder may be planned");

        assert_eq!(preview.sources.len(), 1);
        let ResolvedSourceRequest::Sql {
            request,
            source_schema_fingerprint,
            ..
        } = &preview.sources[0];
        assert_eq!(request.sql, metadata.bound_sql);
        assert_eq!(source_schema_fingerprint, &"d".repeat(64));
    }

    #[test]
    fn saved_resolution_rejects_incomplete_inline_snapshot() {
        let source = CompositionSource::InlineSql {
            id: "rows".to_owned(),
            name: "Rows".to_owned(),
            sql: "SELECT * FROM \"Records\"".to_owned(),
            form_bindings: vec![],
            expected_result: vec![],
            schema_fingerprint: String::new(),
        };
        let spec = composition_spec(
            vec![],
            source,
            vec![CompositionComponent::Table {
                id: "records".to_owned(),
                source: "rows".to_owned(),
            }],
        );
        let metadata = inline_metadata(
            form_binding("Records"),
            result("record_id", CompositionResultFieldType::String),
            &"d".repeat(64),
            &[],
        );
        let descriptors = [CurrentSourceDescriptor::InlineSql {
            source_id: "rows",
            current_schema: Some(&metadata),
        }];

        assert_eq!(
            resolve_composition(ResolveInput {
                composition_revision: CompositionRevisionRef {
                    entry_id: EntryId::from_uuid(uuid::Uuid::from_u128(1)),
                    revision_id: RevisionId::from_uuid(uuid::Uuid::from_u128(2)),
                },
                spec: &spec,
                variables: &BTreeMap::new(),
                current_sources: &descriptors,
            },)
            .unwrap_err()[0]
                .code,
            CompositionDiagnosticCode::SourceSchemaChanged,
        );
    }

    #[test]
    fn resolver_returns_only_sql_requests_and_indexes_dependencies() {
        let definitions = vec![variable("active", CompositionVariableType::Boolean)];
        let forms = form_binding("Records");
        let columns = result("active", CompositionResultFieldType::Boolean);
        let source = inline_source(forms.clone(), columns.clone(), &"e".repeat(64));
        let components = vec![
            CompositionComponent::Input {
                id: "active_input".to_owned(),
                variable: "active".to_owned(),
            },
            CompositionComponent::Table {
                id: "records".to_owned(),
                source: "rows".to_owned(),
            },
        ];
        let spec = composition_spec(definitions, source, components);
        let metadata = inline_metadata(forms, columns, &"e".repeat(64), &["active"]);
        let descriptors = [CurrentSourceDescriptor::InlineSql {
            source_id: "rows",
            current_schema: Some(&metadata),
        }];
        let variables = BTreeMap::from([("active".to_owned(), json!(false))]);

        let plan = resolve_composition(ResolveInput {
            composition_revision: CompositionRevisionRef {
                entry_id: EntryId::from_uuid(uuid::Uuid::from_u128(1)),
                revision_id: RevisionId::from_uuid(uuid::Uuid::from_u128(2)),
            },
            spec: &spec,
            variables: &variables,
            current_sources: &descriptors,
        })
        .expect("the SQL source and component graph are valid");

        assert!(matches!(plan.sources[0], ResolvedSourceRequest::Sql { .. }));
        assert_eq!(
            plan.dependencies.variable_consumers["active"].source_ids,
            BTreeSet::from(["rows".to_owned()]),
        );
        assert_eq!(
            plan.dependencies.variable_consumers["active"].input_component_ids,
            BTreeSet::from(["active_input".to_owned()]),
        );
        let wire = serde_json::to_value(&plan.component_bindings[0]).unwrap();
        assert!(wire.get("label").is_none());
    }

    #[test]
    fn metric_component_selects_one_typed_scalar_column_from_shared_source() {
        let forms = form_binding("Records");
        let columns = result("total", CompositionResultFieldType::Integer);
        let source = inline_source(forms.clone(), columns.clone(), &"f".repeat(64));
        let spec = composition_spec(
            vec![],
            source,
            vec![
                CompositionComponent::Metric {
                    id: "total_metric".to_owned(),
                    source: "rows".to_owned(),
                    column: "total".to_owned(),
                    presentation: None,
                },
                CompositionComponent::Table {
                    id: "rows_table".to_owned(),
                    source: "rows".to_owned(),
                },
            ],
        );
        let metadata = inline_metadata(forms.clone(), columns, &"f".repeat(64), &[]);
        let descriptors = [CurrentSourceDescriptor::InlineSql {
            source_id: "rows",
            current_schema: Some(&metadata),
        }];
        let plan = resolve_composition(ResolveInput {
            composition_revision: CompositionRevisionRef {
                entry_id: EntryId::from_uuid(uuid::Uuid::from_u128(1)),
                revision_id: RevisionId::from_uuid(uuid::Uuid::from_u128(2)),
            },
            spec: &spec,
            variables: &BTreeMap::new(),
            current_sources: &descriptors,
        })
        .expect("a selected integer output supports a metric alongside other columns");
        assert_eq!(
            plan.component_bindings
                .iter()
                .find(|binding| binding.component_id == "total_metric")
                .and_then(|binding| binding.expected_result_type),
            Some(CompositionResultFieldType::Integer),
        );
        assert_eq!(
            plan.dependencies.source_consumers["rows"].metric_component_ids,
            BTreeSet::from(["total_metric".to_owned()]),
        );
        assert_eq!(
            plan.dependencies.source_consumers["rows"].table_component_ids,
            BTreeSet::from(["rows_table".to_owned()]),
        );
    }
}
