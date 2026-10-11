//! Canonical normalization and semantic fingerprints for Composition v1.

use super::{
    CompositionComponent, CompositionDiagnosticCode, CompositionDocument,
    CompositionFieldSchemaEntry, CompositionMetricDefinition, CompositionSource,
};
use crate::form::{FieldType, ListItemDefinition};
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

    // serde-saphyr emits LF on all platforms. Normalize the final document
    // terminator explicitly so golden bytes do not depend on serializer EOF
    // behavior.
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
    if !composition_collection_limits_hold(document) {
        return Err(CompositionDiagnosticCode::InvalidComposition);
    }
    document.spec.components_in_render_order()?;
    if document
        .spec
        .components
        .iter()
        .any(|component| !component.metric_shape_is_well_formed())
        || !typed_metric_bindings_are_well_formed(document)
    {
        return Err(CompositionDiagnosticCode::InvalidComposition);
    }

    let mut normalized = document.clone();
    for source in &mut normalized.spec.sources {
        let CompositionSource::EntryQuery { field_schema, .. } = source else {
            continue;
        };

        if field_schema
            .iter()
            .any(|field| !field_schema_entry_is_valid(field))
        {
            return Err(CompositionDiagnosticCode::InvalidComposition);
        }
        field_schema.sort_by_key(|field| field.field_id);
        if field_schema
            .windows(2)
            .any(|pair| pair[0].field_id == pair[1].field_id)
        {
            return Err(CompositionDiagnosticCode::InvalidComposition);
        }
    }

    Ok(normalized)
}

fn typed_metric_bindings_are_well_formed(document: &CompositionDocument) -> bool {
    document.spec.components.iter().all(|component| {
        let CompositionComponent::Metric {
            definition: Some(definition),
            presentation,
            ..
        } = component
        else {
            return true;
        };

        let matching_sources = document
            .spec
            .sources
            .iter()
            .filter(|source| source.id() == definition.source_id())
            .collect::<Vec<_>>();
        if matching_sources.len() != 1 {
            return false;
        }

        match (definition, matching_sources[0]) {
            (
                CompositionMetricDefinition::Aggregate {
                    operation,
                    field_id,
                    ..
                },
                CompositionSource::EntryQuery { field_schema, .. },
            ) => match (operation, field_id) {
                (super::CompositionAggregateOperation::CountRows, None) => true,
                (super::CompositionAggregateOperation::CountValues, Some(field_id)) => {
                    field_schema.iter().any(|field| field.field_id == *field_id)
                }
                (
                    super::CompositionAggregateOperation::Sum
                    | super::CompositionAggregateOperation::Avg,
                    Some(field_id),
                ) => field_schema
                    .iter()
                    .find(|field| field.field_id == *field_id)
                    .is_some_and(|field| is_numeric_field_type(&field.field_type)),
                (operation, Some(field_id)) => {
                    let Some(field) = field_schema
                        .iter()
                        .find(|field| field.field_id == *field_id)
                    else {
                        return false;
                    };
                    match operation {
                        super::CompositionAggregateOperation::Min
                        | super::CompositionAggregateOperation::Max => {
                            matches!(
                                &field.field_type,
                                FieldType::Date
                                    | FieldType::Timestamp
                                    | FieldType::TimestampTz
                                    | FieldType::TimestampNs
                                    | FieldType::TimestampTzNs
                            ) && presentation.is_none()
                                || is_numeric_field_type(&field.field_type)
                        }
                        super::CompositionAggregateOperation::CountRows
                        | super::CompositionAggregateOperation::CountValues
                        | super::CompositionAggregateOperation::Sum
                        | super::CompositionAggregateOperation::Avg => false,
                    }
                }
                _ => false,
            },
            (
                CompositionMetricDefinition::SqlScalar { column, .. },
                CompositionSource::SavedSql {
                    expected_result, ..
                },
            ) => {
                let mut matching_columns = expected_result
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
            }
            _ => false,
        }
    })
}

fn is_numeric_field_type(field_type: &FieldType) -> bool {
    matches!(
        field_type,
        FieldType::Integer | FieldType::Long | FieldType::Float | FieldType::Double
    )
}

fn composition_collection_limits_hold(document: &CompositionDocument) -> bool {
    let max_items = super::MAX_COMPOSITION_COLLECTION_ITEMS;
    let spec = &document.spec;
    if document.tags.len() > max_items
        || spec.parameters.len() > max_items
        || spec.sources.len() > max_items
        || spec.components.len() > max_items
        || spec.layout.rows.len() > max_items
    {
        return false;
    }

    for source in &spec.sources {
        match source {
            CompositionSource::EntryQuery {
                field_schema,
                query,
                ..
            } => {
                if field_schema.len() > max_items
                    || query.filters.len() > max_items
                    || query.sort.len() > max_items
                    || matches!(
                        &query.projection,
                        super::EntryQueryProjectionTemplate::Fields { fields }
                            if fields.len() > max_items
                    )
                {
                    return false;
                }
            }
            CompositionSource::SavedSql {
                variables,
                expected_result,
                ..
            } => {
                if variables.len() > max_items
                    || expected_result.is_empty()
                    || expected_result.len() > max_items
                    || expected_result.iter().any(|column| {
                        column.name.trim().is_empty()
                            || column.name.len() > super::MAX_COMPOSITION_YAML_BYTES
                    })
                    || expected_result
                        .iter()
                        .map(|column| column.name.as_str())
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        != expected_result.len()
                {
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

fn field_schema_entry_is_valid(field: &CompositionFieldSchemaEntry) -> bool {
    match (&field.field_type, &field.reference_form, &field.list_item) {
        (FieldType::RowReference, Some(_), None) => true,
        (FieldType::List, None, None) => true,
        (FieldType::List, None, Some(item)) => list_item_is_valid(item),
        (FieldType::RowReference, None, _) => false,
        (FieldType::List, Some(_), _) => false,
        (_, None, None) => true,
        _ => false,
    }
}

fn list_item_is_valid(item: &ListItemDefinition) -> bool {
    match (&item.field_type, &item.reference_form) {
        (FieldType::List | FieldType::ObjectList, _) => false,
        (FieldType::RowReference, Some(_)) => true,
        (FieldType::RowReference, None) => false,
        (_, None) => true,
        (_, Some(_)) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        canonicalize_composition, canonicalize_composition_yaml, normalize_composition_document,
    };
    use crate::composition::{
        CompositionDiagnosticCode, CompositionDocument, CompositionMetricDefinition,
        CompositionMetricPresentation, CompositionMetricValueField, CompositionSource,
    };
    use crate::form::{FieldType, ListItemDefinition};
    use crate::id::FormId;

    const MONTHLY_EXPENSE: &str =
        include_str!("../../tests/fixtures/composition/monthly-expense.ugcomp.yaml");
    const MONTHLY_EXPENSE_LABELED: &str =
        include_str!("../../tests/fixtures/composition/monthly-expense-labeled.ugcomp.yaml");
    const METRIC_VALUE_FIELDS: &str =
        include_str!("../../tests/fixtures/composition/metric-value-fields.ugcomp.yaml");
    const TYPED_METRIC_DEFINITIONS: &str =
        include_str!("../../tests/fixtures/composition/typed-metric-definitions.ugcomp.yaml");
    const UNKNOWN_LIST_ITEM_FIELD: &str =
        include_str!("../../tests/fixtures/composition/unknown-list-item-field.ugcomp.yaml");

    #[test]
    fn monthly_expense_canonical_bytes_and_fingerprint_match_golden_fixtures() {
        let result = canonicalize_composition_yaml(MONTHLY_EXPENSE).unwrap();
        assert_eq!(
            result.yaml,
            include_str!("../../tests/fixtures/composition/monthly-expense.canonical.ugcomp.yaml")
        );
        assert_eq!(
            result.fingerprint,
            include_str!("../../tests/fixtures/composition/monthly-expense.fingerprint.txt").trim()
        );
        assert!(result.yaml.ends_with('\n'));
        assert!(!result.yaml.contains('\r'));
        assert!(!result.yaml.starts_with('\u{feff}'));
        let roundtrip = canonicalize_composition_yaml(&result.yaml).unwrap();
        assert_eq!(roundtrip.document, result.document);
        assert_eq!(roundtrip.yaml, result.yaml);
        assert_eq!(roundtrip.fingerprint, result.fingerprint);
        assert!(result
            .document
            .spec
            .parameters
            .iter()
            .all(|parameter| parameter.label.is_none()));
        assert!(result
            .document
            .spec
            .components
            .iter()
            .all(|component| match component {
                super::super::CompositionComponent::Text { label, .. }
                | super::super::CompositionComponent::Metric { label, .. }
                | super::super::CompositionComponent::Table { label, .. } => label.is_none(),
            }));
    }

    #[test]
    fn labeled_monthly_expense_canonical_bytes_and_fingerprint_match_golden_fixtures() {
        let result = canonicalize_composition_yaml(MONTHLY_EXPENSE_LABELED).unwrap();
        assert_eq!(
            result.yaml,
            include_str!(
                "../../tests/fixtures/composition/monthly-expense-labeled.canonical.ugcomp.yaml"
            )
        );
        assert_eq!(
            result.fingerprint,
            include_str!(
                "../../tests/fixtures/composition/monthly-expense-labeled.fingerprint.txt"
            )
            .trim()
        );
        assert_eq!(result.document.spec.parameters[0].id, "month_start");
        assert_eq!(
            result.document.spec.parameters[0].label.as_deref(),
            Some("Start month")
        );
        assert!(result
            .document
            .spec
            .components
            .iter()
            .any(|component| matches!(
                component,
                super::super::CompositionComponent::Metric { id, label: Some(label), .. }
                    if id == "total" && label == "Monthly total"
            )));
        assert!(result
            .document
            .spec
            .components
            .iter()
            .any(|component| matches!(
                component,
                super::super::CompositionComponent::Table { id, label: Some(label), .. }
                    if id == "transactions" && label == "Expense transactions"
            )));

        let changed_label =
            MONTHLY_EXPENSE_LABELED.replace("label: Monthly total", "label: Total expenses");
        let changed = canonicalize_composition_yaml(&changed_label).unwrap();
        assert_ne!(result.fingerprint, changed.fingerprint);
    }

    #[test]
    fn metric_value_fields_canonical_bytes_and_fingerprint_match_golden_fixtures() {
        let result = canonicalize_composition_yaml(METRIC_VALUE_FIELDS).unwrap();
        assert_eq!(
            result.yaml,
            include_str!(
                "../../tests/fixtures/composition/metric-value-fields.canonical.ugcomp.yaml"
            )
        );
        assert_eq!(
            result.fingerprint,
            include_str!("../../tests/fixtures/composition/metric-value-fields.fingerprint.txt")
                .trim()
        );
        assert!(matches!(
            &result.document.spec.components[0],
            super::super::CompositionComponent::Metric {
                value_field: Some(CompositionMetricValueField::EntryField { field_id }),
                ..
            } if field_id.get() == 102
        ));
        assert!(matches!(
            &result.document.spec.components[1],
            super::super::CompositionComponent::Metric {
                value_field: Some(CompositionMetricValueField::SqlColumn { name }),
                ..
            } if name == "total"
        ));
    }

    #[test]
    fn typed_metric_definitions_canonical_bytes_and_fingerprint_match_golden_fixtures() {
        let result = canonicalize_composition_yaml(TYPED_METRIC_DEFINITIONS).unwrap();
        assert_eq!(
            result.yaml,
            include_str!(
                "../../tests/fixtures/composition/typed-metric-definitions.canonical.ugcomp.yaml"
            )
        );
        assert_eq!(
            result.fingerprint,
            include_str!(
                "../../tests/fixtures/composition/typed-metric-definitions.fingerprint.txt"
            )
            .trim()
        );
        assert_eq!(canonicalize_composition_yaml(&result.yaml).unwrap(), result);
        assert!(matches!(
            &result.document.spec.components[0],
            super::super::CompositionComponent::Metric {
                definition: Some(CompositionMetricDefinition::Aggregate {
                    operation: super::super::CompositionAggregateOperation::CountRows,
                    field_id: None,
                    ..
                }),
                presentation: Some(CompositionMetricPresentation::Number {
                    unit: Some(unit),
                    fraction_digits: Some(0),
                }),
                ..
            } if unit == "件"
        ));
        assert!(matches!(
            &result.document.spec.components[1],
            super::super::CompositionComponent::Metric {
                definition: Some(CompositionMetricDefinition::Aggregate {
                    operation: super::super::CompositionAggregateOperation::Sum,
                    field_id: Some(field_id),
                    ..
                }),
                ..
            } if field_id.get() == 102
        ));
        assert!(matches!(
            &result.document.spec.components[2],
            super::super::CompositionComponent::Metric {
                definition: Some(CompositionMetricDefinition::Aggregate {
                    operation: super::super::CompositionAggregateOperation::CountValues,
                    field_id: Some(field_id),
                    ..
                }),
                ..
            } if field_id.get() == 103
        ));
        assert!(matches!(
            &result.document.spec.components[3],
            super::super::CompositionComponent::Metric {
                definition: Some(CompositionMetricDefinition::Aggregate {
                    operation: super::super::CompositionAggregateOperation::Min,
                    field_id: Some(field_id),
                    ..
                }),
                presentation: None,
                ..
            } if field_id.get() == 104
        ));
        assert!(matches!(
            &result.document.spec.components[4],
            super::super::CompositionComponent::Metric {
                definition: Some(CompositionMetricDefinition::Aggregate {
                    operation: super::super::CompositionAggregateOperation::Max,
                    field_id: Some(field_id),
                    ..
                }),
                presentation: None,
                ..
            } if field_id.get() == 105
        ));
        assert!(matches!(
            &result.document.spec.components[5],
            super::super::CompositionComponent::Metric {
                definition: Some(CompositionMetricDefinition::SqlScalar { column, .. }),
                presentation: None,
                ..
            } if column == "total"
        ));
        let CompositionSource::SavedSql {
            expected_result, ..
        } = &result.document.spec.sources[1]
        else {
            panic!("second typed metric source should be Saved SQL")
        };
        assert_eq!(
            expected_result[0].result_type,
            super::super::CompositionResultFieldType::Decimal
        );
        let CompositionSource::EntryQuery {
            field_schema,
            query,
            ..
        } = &result.document.spec.sources[0]
        else {
            panic!("first typed metric source should be an EntryQuery")
        };
        assert!(matches!(
            &query.projection,
            super::super::EntryQueryProjectionTemplate::Fields { fields }
                if fields == &[crate::id::FieldId::new(100).unwrap()]
        ));
        assert!(field_schema.iter().any(|field| field.field_id.get() == 102));
        assert!(field_schema.iter().any(|field| field.field_id.get() == 103));
        assert!(field_schema.iter().any(|field| field.field_id.get() == 104));
        assert!(field_schema.iter().any(|field| field.field_id.get() == 105));
    }

    #[test]
    fn formatting_comments_key_order_and_field_schema_order_do_not_change_fingerprint() {
        let reordered_envelope = MONTHLY_EXPENSE.replacen(
            "format: ugoite.composition\nformat_version: 1\nname: Monthly expenses\nkind: dashboard\ntags: []",
            "# a comment\nkind: dashboard\nname: 'Monthly expenses'   \nformat_version: 1\nformat: ugoite.composition\ntags: []",
            1,
        );
        let reordered_schema = reordered_envelope
            .replacen(
            "        - field_id: 100\n          field_type: date\n        - field_id: 101\n          field_type: string\n        - field_id: 102\n          field_type: double",
            "        - field_id: 102\n          field_type: double\n        - field_id: 100\n          field_type: date\n        - field_id: 101\n          field_type: string",
            1,
            )
            .replacen(
                "      variables:\n        month_start:\n          parameter: month_start\n        month_end:\n          parameter: month_end",
                "      variables:\n        month_end:\n          parameter: month_end\n        month_start:\n          parameter: month_start",
                1,
            );

        let baseline = canonicalize_composition_yaml(MONTHLY_EXPENSE).unwrap();
        let reformatted = canonicalize_composition_yaml(&reordered_schema).unwrap();
        assert_eq!(baseline.document, reformatted.document);
        assert_eq!(baseline.yaml, reformatted.yaml);
        assert_eq!(baseline.fingerprint, reformatted.fingerprint);
    }

    #[test]
    fn tags_and_saved_sql_result_descriptor_are_semantic_fingerprint_inputs() {
        let baseline = canonicalize_composition_yaml(MONTHLY_EXPENSE).unwrap();
        let changed_tags = canonicalize_composition_yaml(
            &MONTHLY_EXPENSE.replace("tags: []", "tags: [monthly, finance]"),
        )
        .unwrap();
        let changed_result_type =
            canonicalize_composition_yaml(&MONTHLY_EXPENSE.replace("type: float", "type: integer"))
                .unwrap();

        assert_ne!(baseline.fingerprint, changed_tags.fingerprint);
        assert_ne!(baseline.fingerprint, changed_result_type.fingerprint);
        assert_eq!(baseline.document.tags, Vec::<String>::new());
        assert_eq!(
            changed_tags.document.tags,
            ["monthly".to_string(), "finance".to_string()]
        );
    }

    #[test]
    fn semantic_change_changes_fingerprint() {
        let changed = MONTHLY_EXPENSE.replace("name: Monthly expenses", "name: Expenses");
        let baseline = canonicalize_composition_yaml(MONTHLY_EXPENSE).unwrap();
        let changed = canonicalize_composition_yaml(&changed).unwrap();

        assert_ne!(baseline.fingerprint, changed.fingerprint);
    }

    #[test]
    fn canonicalization_rejects_unreferenced_components() {
        let mut document = canonicalize_composition_yaml(MONTHLY_EXPENSE)
            .unwrap()
            .document;
        document.spec.layout.rows[0].items.pop();

        assert_eq!(
            canonicalize_composition(&document),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn canonicalization_rejects_empty_layouts() {
        let mut document = canonicalize_composition_yaml(MONTHLY_EXPENSE)
            .unwrap()
            .document;
        document.spec.layout.rows.clear();

        assert_eq!(
            canonicalize_composition(&document),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn layout_and_text_changes_update_the_document_fingerprint() {
        let baseline = canonicalize_composition_yaml(MONTHLY_EXPENSE_LABELED).unwrap();
        let reordered_items = MONTHLY_EXPENSE_LABELED.replace(
            "            component: summary_title\n          - kind: component\n            component: total",
            "            component: total\n          - kind: component\n            component: summary_title",
        );
        assert_ne!(reordered_items, MONTHLY_EXPENSE_LABELED);
        let reordered = canonicalize_composition_yaml(&reordered_items).unwrap();
        assert_ne!(baseline.fingerprint, reordered.fingerprint);

        let changed_text =
            MONTHLY_EXPENSE_LABELED.replace("text: Monthly summary", "text: Summary");
        assert_ne!(changed_text, MONTHLY_EXPENSE_LABELED);
        let changed = canonicalize_composition_yaml(&changed_text).unwrap();
        assert_ne!(baseline.fingerprint, changed.fingerprint);
    }

    #[test]
    fn field_schema_preserves_list_item_and_reference_targets() {
        let yaml = MONTHLY_EXPENSE.replace(
            "        - field_id: 102\n          field_type: double",
            "        - field_id: 102\n          field_type: list\n          items:\n            type: string\n        - field_id: 103\n          field_type: row_reference\n          reference_form: \"00000000-0000-7000-8000-000000000010\"",
        );
        let result = canonicalize_composition_yaml(&yaml).unwrap();
        let baseline = canonicalize_composition_yaml(MONTHLY_EXPENSE).unwrap();
        assert_ne!(result.fingerprint, baseline.fingerprint);
        let CompositionSource::EntryQuery { field_schema, .. } = &result.document.spec.sources[0]
        else {
            panic!("expected EntryQuery source");
        };
        assert_eq!(field_schema[2].field_type, FieldType::List);
        assert_eq!(
            field_schema[2].list_item,
            Some(ListItemDefinition {
                field_type: FieldType::String,
                reference_form: None,
            })
        );
        assert_eq!(field_schema[3].field_type, FieldType::RowReference);
        assert_eq!(
            field_schema[3].reference_form,
            Some(FormId::from_uuid(
                uuid::Uuid::parse_str("00000000-0000-7000-8000-000000000010").unwrap()
            ))
        );
        assert!(result
            .yaml
            .contains("          items:\n            type: string\n"));
        assert!(result
            .yaml
            .contains("          reference_form: 00000000-0000-7000-8000-000000000010"));
    }

    #[test]
    fn field_schema_duplicates_and_inconsistent_metadata_are_rejected() {
        let mut document: CompositionDocument = serde_saphyr::from_str(MONTHLY_EXPENSE).unwrap();
        {
            let CompositionSource::EntryQuery { field_schema, .. } = &mut document.spec.sources[0]
            else {
                panic!("expected EntryQuery source");
            };
            field_schema.push(field_schema[0].clone());
        }
        assert_eq!(
            normalize_composition_document(&document),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );

        {
            let CompositionSource::EntryQuery { field_schema, .. } = &mut document.spec.sources[0]
            else {
                panic!("expected EntryQuery source");
            };
            field_schema.pop();
            field_schema[0].reference_form = Some(FormId::from_uuid(uuid::Uuid::nil()));
        }
        assert_eq!(
            normalize_composition_document(&document),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
        document.format_version = 2;
        assert_eq!(
            normalize_composition_document(&document),
            Err(CompositionDiagnosticCode::UnsupportedFormatVersion)
        );
    }

    #[test]
    fn unknown_list_item_schema_fields_are_rejected() {
        assert_eq!(
            canonicalize_composition_yaml(UNKNOWN_LIST_ITEM_FIELD),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn typed_value_checks_version_before_the_strict_v1_schema() {
        let document = canonicalize_composition_yaml(MONTHLY_EXPENSE)
            .unwrap()
            .document;

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
