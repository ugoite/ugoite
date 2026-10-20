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
    use super::{parse_composition_yaml, MAX_COMPOSITION_YAML_BYTES};
    use crate::composition::{
        CompositionDiagnosticCode, VariableGraphNodeKind, VariableGraphQueryDefinition,
    };

    const GRAPH: &str = include_str!("../../tests/fixtures/composition/sql-first.ugcomp.yaml");

    #[test]
    fn parses_strict_variable_graph_and_shared_query_descriptors() {
        let document = parse_composition_yaml(GRAPH).unwrap();
        assert_eq!(document.spec.variables.len(), 4);
        assert_eq!(document.spec.components.len(), 4);
        let VariableGraphNodeKind::Query { query, returns: _ } = &document.spec.variables[2].kind
        else {
            panic!("third graph node should be a query");
        };
        assert!(matches!(
            query.definition,
            VariableGraphQueryDefinition::InlineSql { .. }
        ));
        assert_eq!(query.arguments.len(), 2);
        assert_eq!(query.form_bindings.len(), 1);
        assert_eq!(query.schema_fingerprint.len(), 64);
        assert!(document.has_complete_query_descriptors());
        assert_eq!(
            document
                .spec
                .components_in_render_order()
                .unwrap()
                .iter()
                .map(|item| item.id())
                .collect::<Vec<_>>(),
            [
                "month_start_input",
                "month_end_input",
                "total",
                "transactions"
            ]
        );
    }

    #[test]
    fn rejects_source_shaped_documents_and_unknown_graph_fields() {
        let source_shaped = GRAPH.replace(
            "  variables:
",
            "  sources: []
  variables:
",
        );
        assert_eq!(
            parse_composition_yaml(&source_shaped),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
        let component_source = GRAPH.replace(
            "      variable: expense_rows
",
            "      source: expense_rows
",
        );
        assert_eq!(
            parse_composition_yaml(&component_source),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
        let root_tags = GRAPH.replace(
            "name: Monthly expenses\n",
            "name: Monthly expenses\ntags: []\n",
        );
        assert_eq!(
            parse_composition_yaml(&root_tags),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
        let variable_label = GRAPH.replace(
            "    - id: month_start\n",
            "    - id: month_start\n      label: Start month\n",
        );
        assert_eq!(
            parse_composition_yaml(&variable_label),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
        let component_label = GRAPH.replace(
            "    - id: month_start_input\n",
            "    - id: month_start_input\n      label: Start month\n",
        );
        assert_eq!(
            parse_composition_yaml(&component_label),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
        let unknown = GRAPH.replace(
            "      kind: query
",
            "      kind: query
      legacy_source: true
",
        );
        assert_eq!(
            parse_composition_yaml(&unknown),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn allows_only_empty_descriptor_pair_in_drafts() {
        let incomplete = GRAPH.replace(
            "        form_bindings:
          - relation: Expense
            form_id: \"00000000-0000-7000-8000-000000000010\"
        schema_fingerprint: 99a980a869d3c01c212a4d175e0616b3237ddfc19b35a6f027829f7cadad3d67",
            "        form_bindings: []
        schema_fingerprint: ''",
        );
        let document = parse_composition_yaml(&incomplete).unwrap();
        assert!(!document.has_complete_query_descriptors());
        let partial = incomplete.replace(
            "        form_bindings: []",
            "        form_bindings:\n          - relation: Expense\n            form_id: \"00000000-0000-7000-8000-000000000010\"",
        );
        assert_eq!(
            parse_composition_yaml(&partial),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn strict_yaml_rejects_duplicate_keys_aliases_multiple_documents_and_bad_versions() {
        for input in [
            GRAPH.replace(
                "name: Monthly expenses\n",
                "name: Monthly expenses\nname: Duplicate\n",
            ),
            format!("{GRAPH}---\nformat: ugoite.composition\nformat_version: 1\n"),
            GRAPH.replace("name: Monthly expenses", "name: &name Monthly expenses"),
            GRAPH.replace("name: Monthly expenses", "name: !custom Monthly expenses"),
        ] {
            assert_eq!(
                parse_composition_yaml(&input),
                Err(CompositionDiagnosticCode::InvalidComposition)
            );
        }
        assert_eq!(
            parse_composition_yaml(
                "format: ugoite.composition\nformat_version: 2\nfuture: !tag &x [one, two]\n"
            ),
            Err(CompositionDiagnosticCode::UnsupportedFormatVersion)
        );
    }

    #[test]
    fn enforces_input_and_collection_limits() {
        let over = format!("{GRAPH}{}", "#x\n".repeat(MAX_COMPOSITION_YAML_BYTES / 2));
        assert_eq!(
            parse_composition_yaml(&over),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }
}
