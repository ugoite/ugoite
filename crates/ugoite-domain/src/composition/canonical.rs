//! Canonical normalization and semantic fingerprints for Composition v1.

use super::{
    CompositionDiagnosticCode, CompositionDocument, CompositionFieldSchemaEntry, CompositionSource,
};
use crate::form::{FieldType, ListItemDefinition};
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

fn composition_collection_limits_hold(document: &CompositionDocument) -> bool {
    let max_items = super::MAX_COMPOSITION_COLLECTION_ITEMS;
    let spec = &document.spec;
    if spec.parameters.len() > max_items
        || spec.sources.len() > max_items
        || spec.components.len() > max_items
        || spec.sections.len() > max_items
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
            CompositionSource::SavedSql { variables, .. } => {
                if variables.len() > max_items {
                    return false;
                }
            }
        }
    }

    spec.sections
        .iter()
        .all(|section| section.components.len() <= max_items)
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
    use crate::composition::{CompositionDiagnosticCode, CompositionDocument, CompositionSource};
    use crate::form::{FieldType, ListItemDefinition};
    use crate::id::FormId;

    const MONTHLY_EXPENSE: &str =
        include_str!("../../tests/fixtures/composition/monthly-expense.ugcomp.yaml");
    const MONTHLY_EXPENSE_LABELED: &str =
        include_str!("../../tests/fixtures/composition/monthly-expense-labeled.ugcomp.yaml");
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
                super::super::CompositionComponent::Metric { label, .. }
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
    fn formatting_comments_key_order_and_field_schema_order_do_not_change_fingerprint() {
        let reordered_envelope = MONTHLY_EXPENSE.replacen(
            "format_version: 1\nname: Monthly expenses\nkind: dashboard",
            "# a comment\nkind: dashboard\nname: 'Monthly expenses'   \nformat_version: 1",
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
        document.spec.sections[1].components.clear();

        assert_eq!(
            canonicalize_composition(&document),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
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
}
