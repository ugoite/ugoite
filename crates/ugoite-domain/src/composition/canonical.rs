//! Canonical normalization and semantic fingerprints for Composition v1.

use super::{
    CompositionComponent, CompositionDiagnosticCode, CompositionDocument, VariableGraphNodeKind,
    VariableGraphResultType, VariableGraphScalarType,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// A normalized Composition and its portable canonical representations.
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalComposition {
    pub document: CompositionDocument,
    pub yaml: String,
    /// Lowercase hexadecimal SHA-256 over the normalized typed JSON value.
    pub fingerprint: String,
}

/// Normalize typed Composition values and produce canonical YAML and a
/// semantic SHA-256 fingerprint.
pub fn canonicalize_composition(
    document: &CompositionDocument,
) -> Result<CanonicalComposition, CompositionDiagnosticCode> {
    let normalized = normalize_composition_document(document)?;
    let mut yaml = serde_saphyr::to_string_with_options(
        &normalized,
        serde_saphyr::ser_options! {
            indent_step: 2,
            compact_list_indent: false,
            empty_as_braces: true,
            tagged_enums: false,
            anchor_generator: None,
            yaml_12: false,
        },
    )
    .map_err(|_| CompositionDiagnosticCode::InvalidComposition)?;

    while yaml.ends_with("\n\n") {
        yaml.pop();
    }
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    if yaml.len() > super::MAX_COMPOSITION_YAML_BYTES {
        return Err(CompositionDiagnosticCode::InvalidComposition);
    }
    if super::parse_composition_yaml(&yaml)? != normalized {
        return Err(CompositionDiagnosticCode::InvalidComposition);
    }

    let semantic_bytes = serde_json::to_vec(&normalized)
        .map_err(|_| CompositionDiagnosticCode::InvalidComposition)?;
    let fingerprint = hex::encode(Sha256::digest(semantic_bytes));

    Ok(CanonicalComposition {
        document: normalized,
        yaml,
        fingerprint,
    })
}

/// Validate an already decoded document value, then apply the strict v1 model
/// and canonicalization rules.
///
/// The envelope check deliberately runs before deserialization so a future
/// format version with fields unknown to v1 reports
/// `unsupported_format_version` instead of `invalid_composition`.
pub fn canonicalize_composition_document_value(
    value: Value,
) -> Result<CanonicalComposition, CompositionDiagnosticCode> {
    super::validate_composition_document_envelope(&value)?;
    let document: CompositionDocument =
        serde_json::from_value(value).map_err(|_| CompositionDiagnosticCode::InvalidComposition)?;
    canonicalize_composition(&document)
}

/// Parse restricted YAML and return the normalized model, canonical YAML, and
/// semantic fingerprint through one deterministic domain operation.
pub fn canonicalize_composition_yaml(
    input: &str,
) -> Result<CanonicalComposition, CompositionDiagnosticCode> {
    let document = super::parse_composition_yaml(input)?;
    canonicalize_composition(&document)
}

pub(super) fn normalize_composition_document(
    document: &CompositionDocument,
) -> Result<CompositionDocument, CompositionDiagnosticCode> {
    document.validate_format_version()?;
    if document.name.trim().is_empty() || !composition_collection_limits_hold(document) {
        return Err(CompositionDiagnosticCode::InvalidComposition);
    }
    document.spec.components_in_render_order()?;
    if document
        .spec
        .components
        .iter()
        .any(|component| !component.metric_shape_is_well_formed())
        || !metric_bindings_are_well_formed(document)
    {
        return Err(CompositionDiagnosticCode::InvalidComposition);
    }

    let mut normalized = document.clone();
    for variable in &mut normalized.spec.variables {
        if let VariableGraphNodeKind::Query { query, .. } = &mut variable.kind {
            query
                .form_bindings
                .sort_by(|left, right| left.relation.cmp(&right.relation));
        }
    }
    Ok(normalized)
}

fn metric_bindings_are_well_formed(document: &CompositionDocument) -> bool {
    document.spec.components.iter().all(|component| {
        let CompositionComponent::Metric {
            variable,
            presentation,
            ..
        } = component
        else {
            return true;
        };

        let mut matching_variables = document
            .spec
            .variables
            .iter()
            .filter(|candidate| candidate.id == *variable);
        let Some(node) = matching_variables.next() else {
            return false;
        };
        if matching_variables.next().is_some() {
            return false;
        }
        let scalar_type = match &node.kind {
            VariableGraphNodeKind::Constant { value_type, .. }
            | VariableGraphNodeKind::Input { value_type, .. } => value_type.scalar_type,
            VariableGraphNodeKind::Query {
                returns: VariableGraphResultType::Scalar { value_type },
                ..
            } => value_type.scalar_type,
            VariableGraphNodeKind::Query {
                returns: VariableGraphResultType::Table { .. },
                ..
            } => return false,
        };
        presentation.is_none()
            || matches!(
                scalar_type,
                VariableGraphScalarType::Integer
                    | VariableGraphScalarType::Float
                    | VariableGraphScalarType::Decimal
            )
    })
}

fn composition_collection_limits_hold(document: &CompositionDocument) -> bool {
    let max_items = super::MAX_COMPOSITION_COLLECTION_ITEMS;
    let spec = &document.spec;
    if spec.variables.len() > max_items
        || spec.components.len() > max_items
        || spec.layout.rows.len() > max_items
    {
        return false;
    }

    for variable in &spec.variables {
        if let VariableGraphNodeKind::Query { returns, query } = &variable.kind {
            if query.arguments.len() > max_items
                || query.form_bindings.len() > max_items
                || matches!(returns, VariableGraphResultType::Table { columns } if columns.len() > max_items)
            {
                return false;
            }
        }
    }

    spec.layout
        .rows
        .iter()
        .all(|row| row.items.len() <= max_items)
}

#[cfg(test)]
mod tests {
    use super::{canonicalize_composition, canonicalize_composition_yaml};
    use crate::composition::{
        CompositionDiagnosticCode, VariableGraphFormBinding, VariableGraphNodeKind,
    };

    const GRAPH: &str = include_str!("../../tests/fixtures/composition/sql-first.ugcomp.yaml");

    #[test]
    fn canonical_yaml_and_fingerprint_are_stable_for_variable_graph() {
        let result = canonicalize_composition_yaml(GRAPH).unwrap();
        assert!(result.yaml.ends_with('\n'));
        assert!(!result.yaml.contains('\r'));
        assert_eq!(canonicalize_composition_yaml(&result.yaml).unwrap(), result);
        assert!(result.document.has_complete_query_descriptors());
    }

    #[test]
    fn duplicate_form_bindings_and_invalid_metric_types_are_rejected() {
        let mut document = canonicalize_composition_yaml(GRAPH).unwrap().document;
        let VariableGraphNodeKind::Query { query, .. } = &mut document.spec.variables[2].kind
        else {
            panic!("fixture node should be a query");
        };
        query.form_bindings.push(VariableGraphFormBinding {
            relation: "Expense".to_owned(),
            form_id: query.form_bindings[0].form_id,
        });
        assert_eq!(
            canonicalize_composition(&document),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );

        let mut invalid_metric = canonicalize_composition_yaml(GRAPH).unwrap().document;
        let VariableGraphNodeKind::Query { returns, .. } =
            &mut invalid_metric.spec.variables[2].kind
        else {
            panic!("fixture node should be a query");
        };
        *returns = crate::composition::VariableGraphResultType::Table { columns: vec![] };
        assert_eq!(
            canonicalize_composition(&invalid_metric),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn semantic_edits_change_the_fingerprint_and_unknown_fields_fail_closed() {
        let baseline = canonicalize_composition_yaml(GRAPH).unwrap();
        let renamed = GRAPH.replace("name: Monthly expenses", "name: Expenses");
        assert_ne!(
            baseline.fingerprint,
            canonicalize_composition_yaml(&renamed).unwrap().fingerprint
        );
        let mut value = serde_json::to_value(baseline.document).unwrap();
        value["future_field"] = serde_json::json!(true);
        assert_eq!(
            super::canonicalize_composition_document_value(value),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }
}
