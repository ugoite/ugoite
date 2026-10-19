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

pub(super) const MAX_YAML_NODES: usize = 4096;
pub(super) const MAX_YAML_EVENTS: usize = 8192;
pub(super) const MAX_YAML_SCALAR_BYTES: usize = 32 * 1024;
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
        CompositionVariableType,
    };
    use serde_saphyr::DuplicateKeyPolicy;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Instant;

    const SQL_FIRST: &str = include_str!("../../tests/fixtures/composition/sql-first.ugcomp.yaml");

    #[test]
    fn parses_inline_sql_and_exact_saved_sql_descriptors() {
        let document = parse_composition_yaml(SQL_FIRST).unwrap();
        assert_eq!(document.spec.sources.len(), 2);
        assert!(matches!(
            document.spec.sources[0],
            CompositionSource::InlineSql { .. }
        ));
        let CompositionSource::SavedSql {
            expected_result,
            schema_fingerprint,
            ..
        } = &document.spec.sources[1]
        else {
            panic!("second source should be Saved SQL");
        };
        assert_eq!(expected_result[0].name, "total");
        assert_eq!(schema_fingerprint.len(), 64);
    }

    #[test]
    fn result_descriptors_keep_order_and_reject_duplicates_or_unknown_fields() {
        let ordered = SQL_FIRST.replace(
            "        - name: total\n          type: decimal",
            "        - name: count\n          type: integer\n        - name: total\n          type: decimal",
        );
        let parsed = parse_composition_yaml(&ordered).unwrap();
        let CompositionSource::SavedSql {
            expected_result, ..
        } = &parsed.spec.sources[1]
        else {
            panic!("second source should be Saved SQL");
        };
        assert_eq!(
            expected_result
                .iter()
                .map(|column| column.name.as_str())
                .collect::<Vec<_>>(),
            ["count", "total"]
        );

        let duplicate = ordered.replace(
            "        - name: total\n          type: decimal",
            "        - name: count\n          type: decimal",
        );
        assert_eq!(
            parse_composition_yaml(&duplicate),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );

        let unknown_column_field = SQL_FIRST.replace(
            "          type: decimal",
            "          type: decimal\n          nullable: true",
        );
        assert_eq!(
            parse_composition_yaml(&unknown_column_field),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn returns_unsupported_version_before_strict_v1_deserialization() {
        let input = "format: ugoite.composition\nformat_version: 2\nfuture_data: !future &shared [one, two]\nfuture_alias: *shared\n";
        assert_eq!(
            parse_composition_yaml(input),
            Err(CompositionDiagnosticCode::UnsupportedFormatVersion)
        );
    }

    #[test]
    fn rejects_duplicate_keys_documents_anchors_merges_and_unknown_fields() {
        let invalid = [
            SQL_FIRST.replace("name: Monthly expenses\n", "name: Monthly expenses\nname: Duplicate\n"),
            format!("{SQL_FIRST}---\nformat: ugoite.composition\nformat_version: 1\n"),
            SQL_FIRST.replace("name: Monthly expenses", "name: &name Monthly expenses"),
            SQL_FIRST.replace("spec:\n", "spec:\n  <<: {variables: [], sources: [], components: [], layout: {kind: flow, rows: []}}\n"),
            SQL_FIRST.replace("name: Monthly expenses", "name: !custom Monthly expenses"),
            SQL_FIRST.replace("name: Monthly expenses\n", "name: Monthly expenses\nfuture_field: true\n"),
        ];
        for input in invalid {
            assert_eq!(
                parse_composition_yaml(&input),
                Err(CompositionDiagnosticCode::InvalidComposition)
            );
        }
    }

    #[test]
    fn float_defaults_reject_overflow_and_underflow_and_accept_json_numbers() {
        for token in ["1e309", "1e-4000"] {
            let input = variable_document(&format!("default: {token}\n"), "float");
            assert_eq!(
                parse_composition_yaml(&input),
                Err(CompositionDiagnosticCode::InvalidComposition)
            );
        }
        for token in ["1", "1.0", ".5"] {
            let input = variable_document(&format!("default: {token}\n"), "float");
            assert!(parse_composition_yaml(&input).is_ok(), "rejected {token}");
        }
    }

    #[test]
    fn date_like_literals_remain_strings_and_defaults_are_type_checked() {
        let input = variable_document("default: 2026-10-02\n", "date");
        let document = parse_composition_yaml(&input).unwrap();
        assert!(matches!(
            &document.spec.variables[0].default,
            Some(value) if value.as_json_value() == &serde_json::Value::String("2026-10-02".to_owned())
        ));
        assert!(CompositionVariableType::Date.accepts_value(&serde_json::json!("2026-10-02")));

        let mismatched = variable_document("default: true\n", "date");
        assert_eq!(
            parse_composition_yaml(&mismatched),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
        let null_default = variable_document("default: null\n", "date");
        assert_eq!(
            parse_composition_yaml(&null_default),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
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
    fn enforces_per_collection_item_limit_without_legacy_collections() {
        assert!(
            parse_composition_yaml(&variables_document(MAX_COMPOSITION_COLLECTION_ITEMS)).is_ok()
        );
        assert_eq!(
            parse_composition_yaml(&variables_document(MAX_COMPOSITION_COLLECTION_ITEMS + 1)),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
        assert_eq!(
            parse_composition_yaml(&SQL_FIRST.replace("kind: inline_sql", "kind: entry_query")),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn enforces_version_probe_nesting_limit() {
        assert_eq!(
            parse_composition_yaml(&nested_future_document(MAX_COMPOSITION_YAML_DEPTH - 1)),
            Err(CompositionDiagnosticCode::UnsupportedFormatVersion)
        );
        assert_eq!(
            parse_composition_yaml(&nested_future_document(MAX_COMPOSITION_YAML_DEPTH)),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn parser_keeps_date_like_scalar_as_string_without_schema_coercion() {
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
        assert!(
            matches!(date, CompositionValue::Literal(CompositionLiteral(value)) if value == serde_json::Value::String("2026-10-02".to_owned()))
        );
    }

    #[test]
    #[ignore = "manual parser profile; run with --ignored --nocapture"]
    fn profile_sql_first_fixture() {
        let report = Rc::new(RefCell::new(None));
        let captured_report = Rc::clone(&report);
        let options = strict_v1_options().with_budget_report(move |summary| {
            *captured_report.borrow_mut() = Some(summary);
        });
        let _: super::super::CompositionDocument =
            serde_saphyr::from_slice_with_options(SQL_FIRST.as_bytes(), options).unwrap();
        let report = report.borrow();
        let report = report.as_ref().expect("parser budget report");
        eprintln!(
            "fixture_bytes={} nodes={} events={} max_depth={} scalar_bytes={} documents={}",
            SQL_FIRST.len(),
            report.nodes,
            report.events,
            report.max_depth,
            report.total_scalar_bytes,
            report.documents,
        );
        assert!(parse_composition_yaml(SQL_FIRST).is_ok());
        let iterations = 1000;
        let start = Instant::now();
        for _ in 0..iterations {
            assert!(parse_composition_yaml(SQL_FIRST).is_ok());
        }
        eprintln!(
            "parsed {iterations} fixture documents in {:?}",
            start.elapsed()
        );
    }

    fn variable_document(default: &str, kind: &str) -> String {
        format!(
            "format: ugoite.composition\nformat_version: 1\nname: Example\nkind: dashboard\nspec:\n  variables:\n    - id: variable\n      type: {kind}\n      required: false\n      {default}  components:\n    - id: marker\n      kind: text\n      content: {{kind: literal, text: marker}}\n      style: body\n  layout:\n    kind: flow\n    rows:\n      - id: main\n        items:\n          - kind: component\n            component: marker\n"
        )
    }

    fn padded_fixture(target_bytes: usize) -> String {
        let mut input = SQL_FIRST.to_owned();
        assert!(input.ends_with('\n'));
        input.push('#');
        input.push_str(&"x".repeat(target_bytes - input.len()));
        input
    }

    fn variables_document(count: usize) -> String {
        let mut input = String::from(
            "format: ugoite.composition\nformat_version: 1\nname: Example\nkind: dashboard\nspec:\n  variables:\n",
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
