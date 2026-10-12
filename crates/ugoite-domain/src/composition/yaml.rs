//! Restricted YAML parsing for the typed Composition v1 contract.

use super::CompositionVariableType;
use super::{CompositionDiagnosticCode, CompositionDocument};
use serde_json::Value;
use serde_saphyr::granit_parser::{Event, Parser, ScalarStyle};
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
    validate_float_default_lexemes(input, &document)?;
    super::canonical::normalize_composition_document(&document)
}

fn validate_float_default_lexemes(
    input: &str,
    document: &CompositionDocument,
) -> Result<(), CompositionDiagnosticCode> {
    if !document.spec.variables.iter().any(|variable| {
        variable.variable_type == CompositionVariableType::Float && variable.default.is_some()
    }) {
        return Ok(());
    }

    let mut default_scalars = Vec::new();
    let mut frames = Vec::new();
    for event in Parser::new_from_str(input) {
        let (event, span) = event.map_err(|_| CompositionDiagnosticCode::InvalidComposition)?;
        match event {
            Event::Scalar(value, style, _, _) => {
                if let Some(Frame::Mapping(key)) = frames.last_mut() {
                    if let Some(key) = key.take() {
                        if key == "default" {
                            let raw = span
                                .slice(input)
                                .ok_or(CompositionDiagnosticCode::InvalidComposition)?;
                            default_scalars.push((raw.to_owned(), style));
                        }
                    } else {
                        *key = Some(value.into_owned());
                    }
                }
            }
            Event::MappingStart(..) => {
                consume_mapping_value(&mut frames);
                frames.push(Frame::Mapping(None));
            }
            Event::SequenceStart(..) => {
                consume_mapping_value(&mut frames);
                frames.push(Frame::Sequence);
            }
            Event::MappingEnd => {
                frames.pop();
            }
            Event::SequenceEnd => {
                frames.pop();
            }
            _ => {}
        }
    }

    let mut defaults = default_scalars.into_iter();
    for variable in &document.spec.variables {
        if variable.default.is_none() {
            continue;
        }

        let (raw, style) = defaults
            .next()
            .ok_or(CompositionDiagnosticCode::InvalidComposition)?;
        if variable.variable_type == CompositionVariableType::Float
            && style == ScalarStyle::Plain
            && super::parse_variable_float(&raw).is_none()
        {
            return Err(CompositionDiagnosticCode::InvalidComposition);
        }
    }

    if defaults.next().is_some() {
        return Err(CompositionDiagnosticCode::InvalidComposition);
    }
    Ok(())
}

fn consume_mapping_value(frames: &mut [Frame]) {
    if let Some(Frame::Mapping(key)) = frames.last_mut() {
        key.take();
    }
}

enum Frame {
    Sequence,
    Mapping(Option<String>),
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
    const SAVED_SQL_METRIC_COLUMNS: &str =
        include_str!("../../tests/fixtures/composition/saved-sql-metric-columns.ugcomp.yaml");

    #[test]
    fn parses_the_shared_monthly_expense_fixture() {
        let document = parse_composition_yaml(MONTHLY_EXPENSE).unwrap();

        assert_eq!(
            document.format,
            crate::composition::CompositionFormat::UgoiteComposition
        );
        assert_eq!(document.name, "Monthly expenses");
        assert!(document.tags.is_empty());
        assert_eq!(document.spec.variables.len(), 2);
        assert_eq!(document.spec.sources.len(), 2);
        assert_eq!(document.spec.components.len(), 4);
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
        let unsupported_type = MONTHLY_EXPENSE.replace("type: float", "type: decimal128");
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
    fn parses_saved_sql_metric_columns_and_preserves_shared_source() {
        let parsed = parse_composition_yaml(SAVED_SQL_METRIC_COLUMNS).unwrap();
        let CompositionSource::SavedSql {
            expected_result, ..
        } = &parsed.spec.sources[1]
        else {
            panic!("second source should be Saved SQL")
        };
        assert_eq!(
            expected_result
                .iter()
                .map(|column| column.name.as_str())
                .collect::<Vec<_>>(),
            ["count", "total", "latest"]
        );
        let metric_columns = parsed.spec.components[..3]
            .iter()
            .map(|component| match component {
                crate::composition::CompositionComponent::Metric { source, column, .. } => {
                    assert_eq!(source, "monthly_summary");
                    column.as_str()
                }
                _ => panic!("expected a Metric component"),
            })
            .collect::<Vec<_>>();
        assert_eq!(metric_columns, ["count", "total", "latest"]);
        assert!(matches!(
            parsed.spec.components[3],
            crate::composition::CompositionComponent::Table { .. }
        ));
    }

    #[test]
    fn rejects_prerelease_metric_calculation_and_value_field_shapes() {
        let old_definition = MONTHLY_EXPENSE.replace(
            "      source: month_total\n      column: total",
            "      definition:\n        kind: sql_scalar\n        source: month_total\n        column: total",
        );
        let old_aggregate = MONTHLY_EXPENSE.replace(
            "      source: month_total\n      column: total",
            "      definition:\n        kind: aggregate\n        source: expense_rows\n        operation: sum\n        field_id: 102",
        );
        let value_field = MONTHLY_EXPENSE.replace(
            "      source: month_total\n      column: total",
            "      source: month_total\n      value_field:\n        kind: sql_column\n        name: total",
        );
        let hidden_calculation = MONTHLY_EXPENSE.replace(
            "      source: month_total\n      column: total",
            "      source: month_total\n      column: total\n      operation: sum\n      field_id: 102",
        );

        for input in [
            &old_definition,
            &old_aggregate,
            &value_field,
            &hidden_calculation,
        ] {
            assert_eq!(
                parse_composition_yaml(input),
                Err(CompositionDiagnosticCode::InvalidComposition),
                "accepted a prerelease Metric calculation shape: {input}"
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
            "format: ugoite.composition\nformat_version: 1\nname: Example\nkind: dashboard\ntags: []\nspec: {<<: {variables: [], sources: [], components: [], layout: {kind: flow, rows: []}}}\n",
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
    fn rejects_the_unreleased_parameter_grammar_without_aliases() {
        let legacy_parameters = "format: ugoite.composition\nformat_version: 1\nname: Example\nkind: dashboard\ntags: []\nspec:\n  parameters:\n    - id: heading\n      type: string\n      required: true\n  sources: []\n  components: []\n  layout: {kind: flow, rows: []}\n";
        assert_eq!(
            parse_composition_yaml(legacy_parameters),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );

        let legacy_text = "format: ugoite.composition\nformat_version: 1\nname: Example\nkind: dashboard\ntags: []\nspec:\n  variables: []\n  sources: []\n  components:\n    - id: heading\n      kind: text\n      text: Heading\n      style: heading\n  layout:\n    kind: flow\n    rows:\n      - id: main\n        items:\n          - kind: component\n            component: heading\n";
        assert_eq!(
            parse_composition_yaml(legacy_text),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );

        let legacy_layout_item = "format: ugoite.composition\nformat_version: 1\nname: Example\nkind: dashboard\ntags: []\nspec:\n  variables: []\n  sources: []\n  components: []\n  layout:\n    kind: flow\n    rows:\n      - id: main\n        items:\n          - kind: parameter\n            parameter: heading\n";
        assert_eq!(
            parse_composition_yaml(legacy_layout_item),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn float_defaults_reject_overflow_and_underflow_to_zero() {
        for numeric_token in ["1e309", "1e-4000"] {
            let input = format!(
                "format: ugoite.composition\nformat_version: 1\nname: Example\nkind: dashboard\ntags: []\nspec:\n  variables:\n    - id: ratio\n      type: float\n      required: false\n      default: {numeric_token}\n  sources: []\n  components:\n    - id: marker\n      kind: text\n      content: {{kind: literal, text: marker}}\n      style: body\n  layout:\n    kind: flow\n    rows:\n      - id: main\n        items:\n          - kind: component\n            component: marker\n"
            );
            assert_eq!(
                parse_composition_yaml(&input),
                Err(CompositionDiagnosticCode::InvalidComposition),
                "accepted float default {numeric_token}"
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
  variables:
    - id: date_default
      type: date
      required: false
      default: 2026-10-02
  components:
    - id: date_input
      kind: input
      variable: date_default
  layout:
    kind: flow
    rows:
      - id: main
        items:
          - kind: component
            component: date_input
"#;
        let document = parse_composition_yaml(input).unwrap();

        assert!(matches!(
            &document.spec.variables[0].default,
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
        let at_limit = variables_document(MAX_COMPOSITION_COLLECTION_ITEMS);
        assert!(parse_composition_yaml(&at_limit).is_ok());

        let over_limit = variables_document(MAX_COMPOSITION_COLLECTION_ITEMS + 1);
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

    fn variables_document(count: usize) -> String {
        let mut input = String::from(
            "format: ugoite.composition\nformat_version: 1\nname: Example\nkind: dashboard\ntags: []\nspec:\n  variables:\n",
        );
        for index in 0..count {
            input.push_str(&format!(
                "    - id: variable_{index}\n      type: string\n      required: false\n"
            ));
        }
        input.push_str(
            "  components:\n    - id: marker\n      kind: text\n      content:\n        kind: literal\n        text: marker\n      style: body\n  layout:\n    kind: flow\n    rows:\n      - id: main\n        items:\n          - kind: component\n            component: marker\n",
        );
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
