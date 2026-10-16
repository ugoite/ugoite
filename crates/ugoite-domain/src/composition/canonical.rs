//! Canonical normalization and semantic fingerprints for Composition v1.

use super::{
    CompositionComponent, CompositionDiagnosticCode, CompositionDocument, CompositionSource,
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
    for source in &mut normalized.spec.sources {
        if let CompositionSource::InlineSql { form_bindings, .. } = source {
            form_bindings.sort_by(|left, right| left.relation.cmp(&right.relation));
        }
    }

    Ok(normalized)
}

fn metric_bindings_are_well_formed(document: &CompositionDocument) -> bool {
    document.spec.components.iter().all(|component| {
        let CompositionComponent::Metric {
            source,
            column,
            presentation,
            ..
        } = component
        else {
            return true;
        };

        let mut matching_sources = document
            .spec
            .sources
            .iter()
            .filter(|candidate| candidate.id() == source);
        let Some(source) = matching_sources.next() else {
            return false;
        };
        if matching_sources.next().is_some() {
            return false;
        }
        if !source.has_complete_source_descriptors() {
            // The Core preview path supplies a live descriptor for an SQL
            // draft before it can be persisted. The draft still needs a
            // selected column, but its scalar type is not yet known here.
            return !column.trim().is_empty();
        }

        let mut matching_columns = source
            .expected_result()
            .iter()
            .filter(|result_column| result_column.name == *column);
        matching_columns.next().is_some_and(|result_column| {
            result_column.result_type.supports_metric()
                && (presentation.is_none()
                    || matches!(
                        result_column.result_type,
                        super::CompositionResultFieldType::Integer
                            | super::CompositionResultFieldType::Float
                            | super::CompositionResultFieldType::Decimal
                    ))
        }) && matching_columns.next().is_none()
    })
}

fn composition_collection_limits_hold(document: &CompositionDocument) -> bool {
    let max_items = super::MAX_COMPOSITION_COLLECTION_ITEMS;
    let spec = &document.spec;
    if spec.variables.len() > max_items
        || spec.sources.len() > max_items
        || spec.components.len() > max_items
        || spec.layout.rows.len() > max_items
    {
        return false;
    }

    for source in &spec.sources {
        match source {
            CompositionSource::InlineSql {
                form_bindings,
                expected_result,
                ..
            } => {
                if form_bindings.len() > max_items || expected_result.len() > max_items {
                    return false;
                }
            }
            CompositionSource::SavedSql {
                variables,
                expected_result,
                ..
            } => {
                if variables.len() > max_items || expected_result.len() > max_items {
                    return false;
                }
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
        CompositionComponent, CompositionDiagnosticCode, CompositionMetricPresentation,
        CompositionSource,
    };

    const SQL_FIRST: &str = include_str!("../../tests/fixtures/composition/sql-first.ugcomp.yaml");

    #[test]
    fn canonical_bytes_and_fingerprint_match_the_sql_first_fixture() {
        let result = canonicalize_composition_yaml(SQL_FIRST).unwrap();
        assert_eq!(
            result.yaml,
            include_str!("../../tests/fixtures/composition/sql-first.canonical.ugcomp.yaml")
        );
        assert_eq!(
            result.fingerprint,
            include_str!("../../tests/fixtures/composition/sql-first.fingerprint.txt").trim()
        );
        assert!(result.yaml.ends_with('\n'));
        assert!(!result.yaml.contains('\r'));
        assert_eq!(canonicalize_composition_yaml(&result.yaml).unwrap(), result);
        assert!(result
            .document
            .spec
            .sources
            .iter()
            .all(|source| match source {
                CompositionSource::InlineSql { .. } | CompositionSource::SavedSql { .. } => true,
            }));
    }

    #[test]
    fn metrics_accept_either_sql_source_when_the_selected_column_is_scalar() {
        let mut document = canonicalize_composition_yaml(SQL_FIRST).unwrap().document;
        let inline = document.spec.sources[0].id().to_owned();
        let CompositionComponent::Metric { source, column, .. } = &mut document.spec.components[2]
        else {
            panic!("fixture component should be a Metric");
        };
        *source = inline;
        *column = "amount".to_owned();
        assert!(canonicalize_composition(&document).is_ok());

        let CompositionComponent::Metric { column, .. } = &mut document.spec.components[2] else {
            unreachable!();
        };
        *column = "payload".to_owned();
        assert_eq!(
            canonicalize_composition(&document),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn metric_rejects_missing_columns_and_non_numeric_number_presentation() {
        let baseline = canonicalize_composition_yaml(SQL_FIRST).unwrap().document;
        let mut missing_column = baseline.clone();
        let CompositionComponent::Metric { column, .. } = &mut missing_column.spec.components[2]
        else {
            panic!("fixture component should be a Metric");
        };
        *column = "missing".to_owned();
        assert_eq!(
            canonicalize_composition(&missing_column),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );

        let mut number_on_date = baseline;
        let CompositionComponent::Metric {
            source,
            column,
            presentation,
            ..
        } = &mut number_on_date.spec.components[2]
        else {
            panic!("fixture component should be a Metric");
        };
        *source = "expense_rows".to_owned();
        *column = "Date".to_owned();
        *presentation = Some(CompositionMetricPresentation::Number {
            unit: None,
            fraction_digits: None,
        });
        assert_eq!(
            canonicalize_composition(&number_on_date),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn form_bindings_are_canonicalized_without_changing_sql_semantics() {
        let mut document = canonicalize_composition_yaml(SQL_FIRST).unwrap().document;
        let CompositionSource::InlineSql { form_bindings, .. } = &mut document.spec.sources[0]
        else {
            panic!("first source should be Inline SQL");
        };
        form_bindings.push(form_bindings[0].clone());
        assert_eq!(
            canonicalize_composition(&document),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn semantic_edits_change_the_document_fingerprint() {
        let baseline = canonicalize_composition_yaml(SQL_FIRST).unwrap();
        let renamed = SQL_FIRST.replace("name: Monthly expenses", "name: Expenses");
        let changed = canonicalize_composition_yaml(&renamed).unwrap();
        assert_ne!(baseline.fingerprint, changed.fingerprint);

        let changed_sql_name =
            SQL_FIRST.replace("      name: Expense rows", "      name: Recent expenses");
        assert_ne!(
            baseline.fingerprint,
            canonicalize_composition_yaml(&changed_sql_name)
                .unwrap()
                .fingerprint
        );
    }

    #[test]
    fn canonicalization_rejects_empty_or_unreferenced_layouts() {
        let mut document = canonicalize_composition_yaml(SQL_FIRST).unwrap().document;
        document.spec.layout.rows[0].items.pop();
        assert_eq!(
            canonicalize_composition(&document),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );

        let mut document = canonicalize_composition_yaml(SQL_FIRST).unwrap().document;
        document.spec.layout.rows.clear();
        assert_eq!(
            canonicalize_composition(&document),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn value_validation_checks_version_then_rejects_unknown_v1_fields() {
        let document = canonicalize_composition_yaml(SQL_FIRST).unwrap().document;
        let mut future_version = serde_json::to_value(&document).unwrap();
        future_version["format_version"] = serde_json::json!(2);
        future_version["future_field"] = serde_json::json!({"enabled": true});
        assert_eq!(
            super::canonicalize_composition_document_value(future_version),
            Err(CompositionDiagnosticCode::UnsupportedFormatVersion)
        );

        let mut unknown_v1_field = serde_json::to_value(document).unwrap();
        unknown_v1_field["future_field"] = serde_json::json!({"enabled": true});
        assert_eq!(
            super::canonicalize_composition_document_value(unknown_v1_field),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }
}
