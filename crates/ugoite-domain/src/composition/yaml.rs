//! Restricted YAML parsing for the typed Composition v1 contract.

use super::{CompositionDiagnosticCode, CompositionDocument};
use serde_json::Value;
use serde_saphyr::{DuplicateKeyPolicy, MergeKeyPolicy};

/// Maximum UTF-8 input size accepted by the Composition YAML parser.
pub const MAX_COMPOSITION_YAML_BYTES: usize = 64 * 1024;
/// Maximum structural nesting depth accepted by the Composition YAML parser.
pub const MAX_COMPOSITION_YAML_DEPTH: usize = 64;
/// Maximum number of items accepted in any one Composition collection.
pub const MAX_COMPOSITION_COLLECTION_ITEMS: usize = 256;

const MAX_YAML_NODES: usize = 4096;
const MAX_YAML_EVENTS: usize = 8192;
const MAX_YAML_SCALAR_BYTES: usize = 32 * 1024;
const VERSION_PROBE_MAX_DOCUMENTS: usize = 2;
const VERSION_PROBE_MAX_ANCHORS: usize = 64;
const VERSION_PROBE_MAX_ALIASES: usize = 64;
const VERSION_PROBE_MAX_RECORDED_ANCHOR_EVENTS: usize = 4096;
const VERSION_PROBE_MAX_RECORDED_ANCHOR_BYTES: usize = MAX_COMPOSITION_YAML_BYTES;
const VERSION_PROBE_MAX_MERGE_KEYS: usize = 64;

/// Parse a restricted `.ugcomp.yaml` document into the shared Rust model.
///
/// The format version is inspected using bounded YAML parsing before strict v1
/// deserialization. Unsupported versions therefore do not get misreported as
/// malformed v1 documents. Raw inspection and revision recovery remain the
/// responsibility of the storage adapter and do not call this function.
pub fn parse_composition_yaml(
    input: &str,
) -> Result<CompositionDocument, CompositionDiagnosticCode> {
    if input.len() > MAX_COMPOSITION_YAML_BYTES {
        return Err(CompositionDiagnosticCode::InvalidComposition);
    }

    probe_document_envelope(input)?;

    let document: CompositionDocument =
        serde_saphyr::from_slice_with_options(input.as_bytes(), strict_v1_options())
            .map_err(|_| CompositionDiagnosticCode::InvalidComposition)?;

    document.validate_format_version()?;
    super::canonical::normalize_composition_document(&document)
}

fn probe_document_envelope(input: &str) -> Result<(), CompositionDiagnosticCode> {
    let documents: Vec<Value> =
        serde_saphyr::from_slice_multiple_with_options(input.as_bytes(), version_probe_options())
            .map_err(|_| CompositionDiagnosticCode::InvalidComposition)?;

    let Some(document) = documents.first() else {
        return Err(CompositionDiagnosticCode::InvalidComposition);
    };
    super::validate_composition_document_envelope(document)
}

fn strict_v1_options() -> serde_saphyr::Options {
    serde_saphyr::options! {
        budget: serde_saphyr::budget! {
            max_documents: 1,
            max_depth: MAX_COMPOSITION_YAML_DEPTH,
            max_nodes: MAX_YAML_NODES,
            max_events: MAX_YAML_EVENTS,
            max_total_scalar_bytes: MAX_YAML_SCALAR_BYTES,
            max_anchors: 0,
            max_aliases: 0,
            max_recorded_anchor_events: 0,
            max_recorded_anchor_bytes: 0,
            max_merge_keys: 0,
        },
        duplicate_keys: DuplicateKeyPolicy::Error,
        merge_keys: MergeKeyPolicy::Error,
        no_schema: true,
        strict_booleans: true,
        reject_unsupported_tags: true,
        emit_comments: false,
        with_snippet: false,
    }
}

fn version_probe_options() -> serde_saphyr::Options {
    serde_saphyr::options! {
        budget: serde_saphyr::budget! {
            max_documents: VERSION_PROBE_MAX_DOCUMENTS,
            max_depth: MAX_COMPOSITION_YAML_DEPTH,
            max_nodes: MAX_YAML_NODES,
            max_events: MAX_YAML_EVENTS,
            max_total_scalar_bytes: MAX_YAML_SCALAR_BYTES,
            max_anchors: VERSION_PROBE_MAX_ANCHORS,
            max_aliases: VERSION_PROBE_MAX_ALIASES,
            max_recorded_anchor_events: VERSION_PROBE_MAX_RECORDED_ANCHOR_EVENTS,
            max_recorded_anchor_bytes: VERSION_PROBE_MAX_RECORDED_ANCHOR_BYTES,
            max_merge_keys: VERSION_PROBE_MAX_MERGE_KEYS,
            max_property_expansion_depth: MAX_COMPOSITION_YAML_DEPTH,
        },
        duplicate_keys: DuplicateKeyPolicy::Error,
        merge_keys: MergeKeyPolicy::Merge,
        strict_booleans: false,
        no_schema: false,
        reject_unsupported_tags: false,
        emit_comments: false,
        with_snippet: false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        parse_composition_yaml, strict_v1_options, MAX_COMPOSITION_COLLECTION_ITEMS,
        MAX_COMPOSITION_YAML_BYTES, MAX_COMPOSITION_YAML_DEPTH,
    };
    use crate::composition::{
        CompositionDiagnosticCode, CompositionLiteral, CompositionSource, CompositionValue,
        DEFAULT_COMPOSITION_PAGE_LIMIT,
    };
    use serde_saphyr::DuplicateKeyPolicy;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Instant;

    const MONTHLY_EXPENSE: &str =
        include_str!("../../tests/fixtures/composition/monthly-expense.ugcomp.yaml");

    #[test]
    fn parses_the_shared_monthly_expense_fixture() {
        let document = parse_composition_yaml(MONTHLY_EXPENSE).unwrap();

        assert_eq!(
            document.format,
            crate::composition::CompositionFormat::UgoiteComposition
        );
        assert_eq!(document.name, "Monthly expenses");
        assert!(document.tags.is_empty());
        assert_eq!(document.spec.parameters.len(), 2);
        assert_eq!(document.spec.sources.len(), 2);
        assert_eq!(document.spec.components.len(), 2);
        assert_eq!(document.spec.layout.rows.len(), 1);
        let CompositionSource::EntryQuery { query, .. } = &document.spec.sources[0] else {
            panic!("first sample source should be an EntryQuery")
        };
        assert_eq!(query.page_limit, DEFAULT_COMPOSITION_PAGE_LIMIT);
    }

    #[test]
    fn preserves_ordered_saved_sql_result_descriptors_and_rejects_ambiguous_names() {
        let ordered = MONTHLY_EXPENSE.replace(
            "        - name: total\n          type: float",
            "        - name: count\n          type: integer\n        - name: total\n          type: float",
        );
        let parsed = parse_composition_yaml(&ordered).unwrap();
        let CompositionSource::SavedSql {
            expected_result, ..
        } = &parsed.spec.sources[1]
        else {
            panic!("second sample source should be Saved SQL")
        };
        assert_eq!(
            expected_result
                .iter()
                .map(|column| column.name.as_str())
                .collect::<Vec<_>>(),
            ["count", "total"]
        );
        assert!(expected_result[0].result_type.supports_metric());
        assert!(!crate::composition::CompositionResultFieldType::Json.supports_metric());
        let canonical = crate::composition::canonicalize_composition_yaml(&ordered).unwrap();
        let count_position = canonical
            .yaml
            .find("        - name: count")
            .expect("ordered descriptor contains count");
        let total_position = canonical
            .yaml
            .find("        - name: total")
            .expect("ordered descriptor contains total");
        assert!(count_position < total_position);

        let duplicate = ordered.replace(
            "        - name: total\n          type: float",
            "        - name: count\n          type: float",
        );
        assert_eq!(
            parse_composition_yaml(&duplicate),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
        let empty_name = MONTHLY_EXPENSE.replace("name: total", "name: '   '");
        assert_eq!(
            parse_composition_yaml(&empty_name),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
        let unsupported_type = MONTHLY_EXPENSE.replace("type: float", "type: decimal");
        assert_eq!(
            parse_composition_yaml(&unsupported_type),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
        let unknown_column_field = MONTHLY_EXPENSE.replace(
            "          type: float",
            "          type: float\n          nullable: true",
        );
        assert_eq!(
            parse_composition_yaml(&unknown_column_field),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn rejects_ambiguous_or_unknown_metric_value_field_shapes() {
        let invalid = [
            MONTHLY_EXPENSE.replace("kind: sql_column", "kind: unknown_column"),
            MONTHLY_EXPENSE.replace("name: total", "name: total\n        field_id: 100"),
            MONTHLY_EXPENSE.replace(
                "kind: sql_column\n        name: total",
                "kind: entry_field\n        name: total",
            ),
            MONTHLY_EXPENSE.replace(
                "kind: sql_column\n        name: total",
                "kind: sql_column\n        name: total\n        extra: true",
            ),
            MONTHLY_EXPENSE.replace("kind: sql_column\n        name: total", "total"),
        ];

        for input in invalid {
            assert_eq!(
                parse_composition_yaml(&input),
                Err(CompositionDiagnosticCode::InvalidComposition),
                "accepted invalid metric value field shape: {input}"
            );
        }
    }

    #[test]
    fn returns_unsupported_version_before_strict_v1_deserialization() {
        let input = r#"
format: ugoite.composition
format_version: 2
future_data: !future &shared [one, two]
future_alias: *shared
"#;

        assert_eq!(
            parse_composition_yaml(input),
            Err(CompositionDiagnosticCode::UnsupportedFormatVersion)
        );
    }

    #[test]
    fn rejects_restricted_or_ambiguous_yaml_constructs() {
        let invalid = [
            "format: ugoite.composition\nformat_version: 1\nname: Example\nname: Duplicate\nkind: dashboard\ntags: []\nspec: {}\n",
            "format: ugoite.composition\nformat_version: 1\nname: Example\nkind: dashboard\ntags: []\nspec: {}\n---\nformat: ugoite.composition\nformat_version: 1\n",
            "format: ugoite.composition\nformat_version: 1\nname: &name Example\nkind: dashboard\ntags: []\nspec: {}\n",
            "format: ugoite.composition\nformat_version: 1\nname: Example\nkind: dashboard\ntags: []\nspec: {<<: {parameters: [], sources: [], components: [], layout: {kind: flow, rows: []}}}\n",
            "format: ugoite.composition\nformat_version: 1\nname: !custom Example\nkind: dashboard\ntags: []\nspec: {}\n",
            "format: ugoite.composition\nformat_version: 1\nname: Example\nkind: dashboard\ntags: []\nspec: {}\nextra: true\n",
            "format: ugoite.composition\nformat_version: 1\nname: 2026\nkind: dashboard\ntags: []\nspec: {}\n",
            "format: ugoite.composition\nformat_version: 1\nname: [not, a, string]\nkind: dashboard\ntags: []\nspec: {}\n",
            "format: other.document\nformat_version: 2\nname: Example\nkind: dashboard\ntags: []\nspec: {}\n",
            "format: ugoite.composition\nformat_version: 1\nname: Example\nkind: dashboard\nspec: {}\n",
        ];

        for input in invalid {
            assert_eq!(
                parse_composition_yaml(input),
                Err(CompositionDiagnosticCode::InvalidComposition),
                "accepted restricted YAML: {input}"
            );
        }
    }

    #[test]
    fn leaves_date_like_literals_as_strings() {
        let input = r#"
format: ugoite.composition
format_version: 1
name: Example
kind: dashboard
tags: []
spec:
  parameters:
    - id: date_default
      type: date
      required: false
      default: 2026-10-02
  layout:
    kind: flow
    rows:
      - id: main
        items:
          - kind: parameter
            parameter: date_default
"#;
        let document = parse_composition_yaml(input).unwrap();

        assert!(matches!(
            &document.spec.parameters[0].default,
            Some(CompositionLiteral(value)) if value == &serde_json::Value::String("2026-10-02".to_owned())
        ));
    }

    #[test]
    fn rejects_input_over_the_byte_limit_and_accepts_the_limit() {
        let at_limit = padded_fixture(MAX_COMPOSITION_YAML_BYTES);
        assert!(parse_composition_yaml(&at_limit).is_ok());

        let over_limit = padded_fixture(MAX_COMPOSITION_YAML_BYTES + 1);
        assert_eq!(
            parse_composition_yaml(&over_limit),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn enforces_the_per_collection_item_limit() {
        let at_limit = parameters_document(MAX_COMPOSITION_COLLECTION_ITEMS);
        assert!(parse_composition_yaml(&at_limit).is_ok());

        let over_limit = parameters_document(MAX_COMPOSITION_COLLECTION_ITEMS + 1);
        assert_eq!(
            parse_composition_yaml(&over_limit),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );

        let tagged_document = |count: usize| {
            let tags = (0..count)
                .map(|index| format!("tag_{index}"))
                .collect::<Vec<_>>()
                .join(", ");
            MONTHLY_EXPENSE.replace("tags: []", &format!("tags: [{tags}]"))
        };
        assert!(parse_composition_yaml(&tagged_document(MAX_COMPOSITION_COLLECTION_ITEMS)).is_ok());
        assert_eq!(
            parse_composition_yaml(&tagged_document(MAX_COMPOSITION_COLLECTION_ITEMS + 1)),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn enforces_the_version_probe_nesting_limit() {
        let at_limit = nested_future_document(MAX_COMPOSITION_YAML_DEPTH - 1);
        assert_eq!(
            parse_composition_yaml(&at_limit),
            Err(CompositionDiagnosticCode::UnsupportedFormatVersion)
        );

        let over_limit = nested_future_document(MAX_COMPOSITION_YAML_DEPTH);
        assert_eq!(
            parse_composition_yaml(&over_limit),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn date_like_scalar_is_not_implicitly_converted_by_the_parser() {
        let options = serde_saphyr::options! {
            budget: serde_saphyr::budget! {
                max_documents: 1,
                max_depth: MAX_COMPOSITION_YAML_DEPTH,
                max_nodes: 4096,
                max_events: 8192,
                max_total_scalar_bytes: MAX_COMPOSITION_YAML_BYTES,
            },
            duplicate_keys: DuplicateKeyPolicy::Error,
            no_schema: true,
            strict_booleans: true,
            reject_unsupported_tags: true,
            emit_comments: false,
            with_snippet: false,
        };
        let date: CompositionValue =
            serde_saphyr::from_str_with_options("2026-10-02", options).unwrap();

        assert!(matches!(
            date,
            CompositionValue::Literal(CompositionLiteral(value))
                if value == serde_json::Value::String("2026-10-02".to_owned())
        ));
    }

    #[test]
    #[ignore = "manual parser profile; run with --ignored --nocapture"]
    fn profile_monthly_expense_fixture() {
        let report = Rc::new(RefCell::new(None));
        let captured_report = Rc::clone(&report);
        let options = strict_v1_options().with_budget_report(move |summary| {
            *captured_report.borrow_mut() = Some(summary);
        });
        let _: super::super::CompositionDocument =
            serde_saphyr::from_slice_with_options(MONTHLY_EXPENSE.as_bytes(), options).unwrap();
        let report = report.borrow();
        let report = report.as_ref().expect("parser budget report");
        eprintln!(
            "fixture_bytes={} nodes={} events={} max_depth={} scalar_bytes={} documents={}",
            MONTHLY_EXPENSE.len(),
            report.nodes,
            report.events,
            report.max_depth,
            report.total_scalar_bytes,
            report.documents,
        );
        assert!(parse_composition_yaml(MONTHLY_EXPENSE).is_ok());

        let iterations = 1000;
        let start = Instant::now();
        for _ in 0..iterations {
            assert!(parse_composition_yaml(MONTHLY_EXPENSE).is_ok());
        }
        eprintln!(
            "parsed {iterations} fixture documents in {:?}",
            start.elapsed()
        );
    }

    fn padded_fixture(target_bytes: usize) -> String {
        let mut input = MONTHLY_EXPENSE.to_owned();
        assert!(input.ends_with('\n'));
        input.push('#');
        input.push_str(&"x".repeat(target_bytes - input.len()));
        input
    }

    fn parameters_document(count: usize) -> String {
        let mut input = String::from(
            "format: ugoite.composition\nformat_version: 1\nname: Example\nkind: dashboard\ntags: []\nspec:\n  parameters:\n",
        );
        for index in 0..count {
            input.push_str(&format!(
                "    - id: parameter_{index}\n      type: string\n      required: true\n"
            ));
        }
        input.push_str("  layout:\n    kind: flow\n    rows:\n      - id: main\n        items:\n");
        for index in 0..count {
            input.push_str(&format!(
                "          - kind: parameter\n            parameter: parameter_{index}\n"
            ));
        }
        input
    }

    fn nested_future_document(depth: usize) -> String {
        format!(
            "format: ugoite.composition\nformat_version: 2\nvalue: {}0{}\n",
            "[".repeat(depth),
            "]".repeat(depth)
        )
    }
}
