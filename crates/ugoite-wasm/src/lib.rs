//! Raw WebAssembly adapter over Ugoite's portable crates.
//!
//! The browser boundary deliberately uses UTF-8 JSON and a tiny C ABI instead
//! of binding network I/O into WASM. JavaScript performs `fetch`; this crate
//! prepares requests and decodes responses using the same Rust implementation
//! used by the native CLI.

use std::io::{self, Write};

pub use ugoite_api_client as api_client;
pub use ugoite_core as core;
pub use ugoite_domain as domain;
pub use ugoite_konase as konase;

const MAX_PROTOCOL_REQUEST_BYTES: usize = 256 * 1024;
const MAX_COMPOSITION_SAVE_REQUEST_BYTES: usize = ugoite_api_client::MAX_COMPOSITION_SAVE_WORK_BYTES
    + ugoite_domain::composition::MAX_COMPOSITION_YAML_BYTES * 6
    + 4096;
const MAX_COMPOSITION_SAVE_PROTOCOL_REQUEST_BYTES: usize =
    MAX_COMPOSITION_SAVE_REQUEST_BYTES + 1024;
// JSON can expand each control byte in a SQL source to a six-byte `\u00xx`
// escape. Allow that worst case plus a small action envelope for parsing.
const MAX_SQL_DOCUMENT_REQUEST_BYTES: usize =
    ugoite_domain::sql_document::MAX_SQL_DOCUMENT_BYTES * 6 + 1024;
// Parameter rename has two additional caller-provided names. Bound their
// combined JSON envelope to one ordinary protocol-sized request.
const MAX_SQL_DOCUMENT_RENAME_REQUEST_BYTES: usize =
    ugoite_domain::sql_document::MAX_SQL_DOCUMENT_BYTES * 6 + MAX_PROTOCOL_REQUEST_BYTES;
// Edit requests carry the current source, the transaction's expected source,
// guarded preimages, and replacements. Each can be as large as the document
// limit and JSON control-byte escaping expands each UTF-8 byte up to sixfold.
const MAX_SQL_DOCUMENT_EDIT_REQUEST_BYTES: usize =
    ugoite_domain::sql_document::MAX_SQL_DOCUMENT_BYTES * 24
        + ugoite_domain::sql_document::MAX_SQL_DOCUMENT_EDITS * 128
        + 4096;
// Typed node edits carry the exact source twice plus node replacements. They
// share the guarded edit envelope's conservative bound.
const MAX_SQL_DOCUMENT_NODE_EDIT_REQUEST_BYTES: usize = MAX_SQL_DOCUMENT_EDIT_REQUEST_BYTES;
const MAX_SQL_DOCUMENT_SEMANTIC_EDIT_REQUEST_BYTES: usize = MAX_SQL_DOCUMENT_EDIT_REQUEST_BYTES;
const MAX_SQL_DOCUMENT_RESPONSE_BYTES: usize =
    ugoite_domain::sql_document::MAX_SQL_DOCUMENT_BYTES * 4;
const ENTRY_OPERATIONS: &[&str] = &["entry.validate_draft"];

struct BoundedJsonWriter {
    bytes: Vec<u8>,
    max_bytes: usize,
}

impl BoundedJsonWriter {
    fn new(max_bytes: usize) -> Self {
        Self {
            bytes: Vec::new(),
            max_bytes,
        }
    }
}

impl Write for BoundedJsonWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.max_bytes.saturating_sub(self.bytes.len()) {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "bounded JSON output limit exceeded",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn is_entry_operation(action: &str) -> bool {
    ENTRY_OPERATIONS.contains(&action)
}

fn sql_document_request_limit(action: &str) -> Option<usize> {
    match action {
        "domain.parse_sql_document"
        | "domain.collect_quoted_form_name_references"
        | "domain.collect_quoted_form_relation_names" => Some(MAX_SQL_DOCUMENT_REQUEST_BYTES),
        "domain.rename_sql_document_parameter" => Some(MAX_SQL_DOCUMENT_RENAME_REQUEST_BYTES),
        "domain.apply_sql_document_edit_transaction" => Some(MAX_SQL_DOCUMENT_EDIT_REQUEST_BYTES),
        "domain.apply_sql_document_node_edit_transaction" => {
            Some(MAX_SQL_DOCUMENT_NODE_EDIT_REQUEST_BYTES)
        }
        "domain.apply_sql_document_semantic_edit_transaction" => {
            Some(MAX_SQL_DOCUMENT_SEMANTIC_EDIT_REQUEST_BYTES)
        }
        _ => None,
    }
}

fn is_composition_save_prepare(request: &serde_json::Value) -> bool {
    request.get("action").and_then(serde_json::Value::as_str) == Some("prepare")
        && request.get("operation").and_then(serde_json::Value::as_str) == Some("composition.save")
}

pub fn invoke_json(input: &str) -> String {
    if input.len() > MAX_PROTOCOL_REQUEST_BYTES {
        // Bound JSON decoding before inspecting the action. Only documented
        // SQL Document transactions and Composition save Work can exceed the
        // ordinary protocol request cap.
        let max_special_request_bytes = MAX_SQL_DOCUMENT_RENAME_REQUEST_BYTES
            .max(MAX_SQL_DOCUMENT_EDIT_REQUEST_BYTES)
            .max(MAX_COMPOSITION_SAVE_PROTOCOL_REQUEST_BYTES);
        if input.len() <= max_special_request_bytes {
            if let Ok(request) = serde_json::from_str::<serde_json::Value>(input) {
                if let Some(action) = request.get("action").and_then(serde_json::Value::as_str) {
                    if sql_document_request_limit(action).is_some_and(|limit| input.len() <= limit)
                    {
                        return invoke_domain(request);
                    }
                }
                if is_composition_save_prepare(&request)
                    && input.len() <= MAX_COMPOSITION_SAVE_PROTOCOL_REQUEST_BYTES
                {
                    return ugoite_api_client::invoke_json(input);
                }
            }
        }
        return protocol_input_too_large_error();
    }
    if let Ok(request) = serde_json::from_str::<serde_json::Value>(input) {
        if let Some(action) = request.get("action").and_then(serde_json::Value::as_str) {
            if action.starts_with("domain.") {
                return invoke_domain(request);
            }
            if action.starts_with("konase.") {
                return invoke_konase(request);
            }
            if is_entry_operation(action) {
                return invoke_entry(request);
            }
        }
    }
    ugoite_api_client::invoke_json(input)
}

fn protocol_input_too_large_error() -> String {
    serde_json::json!({
        "ok": false,
        "error": {
            "kind": "input_too_large",
            "message": "JSON protocol input exceeds its size limit",
        },
    })
    .to_string()
}

fn konase_error(message: &str) -> String {
    serde_json::json!({
        "ok": false,
        "error": {"kind": "konase_protocol", "message": message},
    })
    .to_string()
}

fn ensure_json_size(value: &serde_json::Value, max_bytes: usize) -> Result<(), String> {
    let size = serde_json::to_vec(value)
        .map_err(|error| error.to_string())?
        .len();
    if size > max_bytes {
        return Err(format!("JSON value exceeds the {max_bytes}-byte limit"));
    }
    Ok(())
}

fn invoke_konase(request: serde_json::Value) -> String {
    let result = (|| -> Result<serde_json::Value, String> {
        let action = request
            .get("action")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "action is required".to_string())?;
        let payload = request
            .get("value")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        match action {
            "konase.version" => Ok(serde_json::json!({
                "protocol_version": ugoite_konase::KONASE_PROTOCOL_VERSION,
            })),
            "konase.new" => {
                if payload.is_null() {
                    return serde_json::to_value(ugoite_konase::KonaseState::default())
                        .map_err(|error| error.to_string());
                }
                let state = payload
                    .get("state")
                    .cloned()
                    .ok_or_else(|| "konase.new accepts no value or a state object".to_string())?;
                ensure_json_size(&state, ugoite_konase::MAX_STATE_JSON_BYTES)?;
                serde_json::from_value::<ugoite_konase::KonaseState>(state)
                    .map_err(|error| error.to_string())
                    .and_then(|state| {
                        serde_json::to_value(
                            ugoite_konase::normalize_state(state)
                                .map_err(|error| error.to_string())?,
                        )
                        .map_err(|error| error.to_string())
                    })
            }
            "konase.step" => {
                ensure_json_size(&payload, MAX_PROTOCOL_REQUEST_BYTES)?;
                let state = serde_json::from_value(
                    payload
                        .get("state")
                        .cloned()
                        .ok_or_else(|| "state is required".to_string())?,
                )
                .map_err(|error| error.to_string())?;
                ensure_json_size(
                    payload
                        .get("state")
                        .ok_or_else(|| "state is required".to_string())?,
                    ugoite_konase::MAX_STATE_JSON_BYTES,
                )?;
                let event = serde_json::from_value(
                    payload
                        .get("event")
                        .cloned()
                        .ok_or_else(|| "event is required".to_string())?,
                )
                .map_err(|error| error.to_string())?;
                let result = serde_json::to_value(ugoite_konase::step(state, event))
                    .map_err(|error| error.to_string())?;
                ensure_json_size(&result, ugoite_konase::MAX_STATE_JSON_BYTES)?;
                Ok(result)
            }
            "konase.context" => {
                ensure_json_size(&payload, ugoite_konase::MAX_STATE_JSON_BYTES)?;
                let input = serde_json::from_value::<ugoite_konase::ContextBuildRequest>(payload)
                    .map_err(|error| error.to_string())?;
                let context = ugoite_konase::ContextBuilder::default().build(input);
                let context = serde_json::to_value(context).map_err(|error| error.to_string())?;
                ensure_json_size(&context, ugoite_konase::MAX_STATE_JSON_BYTES)?;
                Ok(context)
            }
            _ => Err(format!("unsupported Konase action: {action}")),
        }
    })();

    let envelope = match result {
        Ok(value) => serde_json::json!({"ok": true, "value": value}),
        Err(message) => serde_json::json!({
            "ok": false,
            "error": {"kind": "konase_protocol", "message": message},
        }),
    };
    if ensure_json_size(&envelope, ugoite_konase::MAX_STATE_JSON_BYTES).is_err() {
        return konase_error("Konase protocol output exceeds the size limit");
    }
    envelope.to_string()
}

fn invoke_domain(request: serde_json::Value) -> String {
    match request.get("action").and_then(serde_json::Value::as_str) {
        Some("domain.compile_variable_graph.v1") => return compile_variable_graph_v1(request),
        Some("domain.parse_sql_document") => return invoke_sql_document(request),
        Some("domain.collect_quoted_form_name_references") => {
            return invoke_sql_form_name_references(request);
        }
        Some("domain.collect_quoted_form_relation_names") => {
            return invoke_sql_form_relation_names(request);
        }
        Some("domain.rename_sql_document_parameter") => {
            return invoke_sql_document_parameter_rename(request);
        }
        Some("domain.apply_sql_document_edit_transaction") => {
            return invoke_sql_document_edit_transaction(request);
        }
        Some("domain.apply_sql_document_node_edit_transaction") => {
            return invoke_sql_document_node_edit_transaction(request);
        }
        Some("domain.apply_sql_document_semantic_edit_transaction") => {
            return invoke_sql_document_semantic_edit_transaction(request);
        }
        _ => {}
    }

    if request.get("action").and_then(serde_json::Value::as_str)
        == Some("domain.canonicalize_composition_document")
    {
        let Some(document) = request
            .get("value")
            .and_then(|value| value.get("document"))
            .cloned()
        else {
            return composition_canonicalization_error(
                ugoite_domain::composition::CompositionDiagnosticCode::InvalidComposition,
            );
        };

        let result = ugoite_domain::composition::canonicalize_composition_document_value(document);
        return match result {
            Ok(canonical) => serde_json::json!({
                "ok": true,
                "value": {
                    "document": canonical.document,
                    "canonical_yaml": canonical.yaml,
                    "fingerprint": canonical.fingerprint,
                },
            })
            .to_string(),
            Err(code) => composition_canonicalization_error(code),
        };
    }

    if request.get("action").and_then(serde_json::Value::as_str)
        == Some("domain.canonicalize_composition")
    {
        let Some(yaml) = request
            .get("value")
            .and_then(|value| value.get("yaml"))
            .and_then(serde_json::Value::as_str)
        else {
            return serde_json::json!({
                "ok": false,
                "error": {"kind": "domain_validation", "message": "value.yaml must be a string"},
            })
            .to_string();
        };

        return match ugoite_domain::composition::canonicalize_composition_yaml(yaml) {
            Ok(canonical) => serde_json::json!({
                "ok": true,
                "value": {
                    "document": canonical.document,
                    "canonical_yaml": canonical.yaml,
                    "fingerprint": canonical.fingerprint,
                },
            })
            .to_string(),
            Err(code) => composition_canonicalization_error(code),
        };
    }

    let result = (|| -> Result<serde_json::Value, String> {
        let action = request
            .get("action")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "action is required".to_string())?;
        let payload = request
            .get("value")
            .cloned()
            .ok_or_else(|| "value is required".to_string())?;
        match action {
            "domain.composition_form_query_sql" => {
                let query = serde_json::from_value::<
                    ugoite_domain::composition::CompositionFormQuery,
                >(payload)
                .map_err(|error| error.to_string())?;
                let sql = ugoite_domain::composition::composition_form_query_sql(query)
                    .map_err(str::to_owned)?;
                Ok(serde_json::json!({ "sql": sql }))
            }
            "domain.analyze_sql_structure" => {
                let sql = payload
                    .get("sql")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| "sql must be a string".to_string())?;
                let context: ugoite_domain::sql_structure::SqlStructureContext =
                    serde_json::from_value(
                        payload
                            .get("context")
                            .cloned()
                            .ok_or_else(|| "context is required".to_string())?,
                    )
                    .map_err(|error| error.to_string())?;
                serde_json::to_value(ugoite_domain::sql_structure::analyze_sql_structure(
                    sql, &context,
                ))
                .map_err(|error| error.to_string())
            }
            "domain.propose_sql_structure_operation" => {
                let sql = payload
                    .get("sql")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| "sql must be a string".to_string())?;
                let context: ugoite_domain::sql_structure::SqlStructureContext =
                    serde_json::from_value(
                        payload
                            .get("context")
                            .cloned()
                            .ok_or_else(|| "context is required".to_string())?,
                    )
                    .map_err(|error| error.to_string())?;
                let operation: ugoite_domain::sql_structure::SqlStructureOperation =
                    serde_json::from_value(
                        payload
                            .get("operation")
                            .cloned()
                            .ok_or_else(|| "operation is required".to_string())?,
                    )
                    .map_err(|error| error.to_string())?;
                serde_json::to_value(
                    ugoite_domain::sql_structure::propose_sql_structure_operation(
                        sql, &context, operation,
                    ),
                )
                .map_err(|error| error.to_string())
            }
            "domain.parse_variable_graph_input" => {
                let descriptor = serde_json::from_value::<
                    ugoite_domain::composition::VariableGraphScalarDescriptor,
                >(
                    payload
                        .get("value_type")
                        .cloned()
                        .ok_or_else(|| "value_type is required".to_string())?,
                )
                .map_err(|error| error.to_string())?;
                let raw = payload
                    .get("raw")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| "raw must be a string".to_string())?;
                let outcome =
                    ugoite_domain::composition::parse_variable_graph_input(descriptor, raw);
                Ok(match outcome {
                    ugoite_domain::composition::VariableGraphInputValue::Unset => {
                        serde_json::json!({"status": "unset"})
                    }
                    ugoite_domain::composition::VariableGraphInputValue::Valid(value) => {
                        serde_json::json!({"status": "valid", "value": value.to_json_value()})
                    }
                    ugoite_domain::composition::VariableGraphInputValue::Invalid => {
                        serde_json::json!({"status": "invalid"})
                    }
                })
            }
            "domain.encode_spreadsheet_csv" => {
                let rows: Vec<Vec<String>> =
                    serde_json::from_value(payload).map_err(|error| error.to_string())?;
                Ok(serde_json::json!(
                    ugoite_domain::spreadsheet::encode_spreadsheet_csv(&rows)
                ))
            }
            "domain.validate_asset_reference" => {
                let reference: ugoite_domain::entry::AssetReference =
                    serde_json::from_value(payload).map_err(|error| error.to_string())?;
                reference.validate().map_err(|error| error.to_string())?;
                serde_json::to_value(reference).map_err(|error| error.to_string())
            }
            "domain.validate_form" => {
                let form: ugoite_domain::form::FormDefinition =
                    serde_json::from_value(payload).map_err(|error| error.to_string())?;
                form.validate().map_err(|error| error.to_string())?;
                serde_json::to_value(form).map_err(|error| error.to_string())
            }
            "domain.preview_form_changes" => {
                let current: ugoite_domain::form::FormDefinition = serde_json::from_value(
                    payload
                        .get("current")
                        .cloned()
                        .ok_or_else(|| "current is required".to_string())?,
                )
                .map_err(|error| error.to_string())?;
                let changes: ugoite_domain::form::FormChangeSet = serde_json::from_value(
                    payload
                        .get("changes")
                        .cloned()
                        .ok_or_else(|| "changes is required".to_string())?,
                )
                .map_err(|error| error.to_string())?;
                let compatibility = changes
                    .compatibility(&current)
                    .map_err(|error| error.to_string())?;
                let evolved = current.apply(&changes).map_err(|error| error.to_string())?;
                Ok(serde_json::json!({"compatibility": compatibility, "evolved": evolved}))
            }
            "domain.validate_revision" => {
                let form: ugoite_domain::form::FormDefinition = serde_json::from_value(
                    payload
                        .get("form")
                        .cloned()
                        .ok_or_else(|| "form is required".to_string())?,
                )
                .map_err(|error| error.to_string())?;
                let revision: ugoite_domain::entry::EntryRevision = serde_json::from_value(
                    payload
                        .get("revision")
                        .cloned()
                        .ok_or_else(|| "revision is required".to_string())?,
                )
                .map_err(|error| error.to_string())?;
                let current: Option<ugoite_domain::entry::EntryRevision> = payload
                    .get("current")
                    .filter(|value| !value.is_null())
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(|error| error.to_string())?;
                revision
                    .validate(&form, current.as_ref())
                    .map_err(|error| error.to_string())?;
                serde_json::to_value(revision).map_err(|error| error.to_string())
            }
            "domain.build_revision_draft" => {
                let form: ugoite_domain::form::FormDefinition = serde_json::from_value(
                    payload
                        .get("form")
                        .cloned()
                        .ok_or_else(|| "form is required".to_string())?,
                )
                .map_err(|error| error.to_string())?;
                let draft: ugoite_domain::entry::EntryRevisionDraft = serde_json::from_value(
                    payload
                        .get("draft")
                        .cloned()
                        .ok_or_else(|| "draft is required".to_string())?,
                )
                .map_err(|error| error.to_string())?;
                let current: Option<ugoite_domain::entry::EntryRevision> = payload
                    .get("current")
                    .filter(|value| !value.is_null())
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(|error| error.to_string())?;
                let revision = draft
                    .build(&form, current.as_ref())
                    .map_err(|error| error.to_string())?;
                serde_json::to_value(revision).map_err(|error| error.to_string())
            }
            _ => Err(format!("unsupported portable domain action: {action}")),
        }
    })();
    match result {
        Ok(value) => serde_json::json!({"ok": true, "value": value}).to_string(),
        Err(message) => serde_json::json!({"ok": false, "error": {"kind": "domain_validation", "message": message}}).to_string(),
    }
}

/// Compile the strict typed Composition variable graph protocol payload.
///
/// The action is pure: it validates the graph and returns its deterministic
/// dependency plan without executing queries or accessing host state.
fn compile_variable_graph_v1(request: serde_json::Value) -> String {
    let Some(value) = request.get("value").cloned() else {
        return serde_json::json!({
            "ok": false,
            "error": {
                "kind": "domain_validation",
                "code": "invalid_variable_graph_payload",
                "message": "value must be a strict VariableGraph object",
            },
        })
        .to_string();
    };

    let graph = match serde_json::from_value::<ugoite_domain::composition::VariableGraph>(value) {
        Ok(graph) => graph,
        Err(_) => {
            return serde_json::json!({
                "ok": false,
                "error": {
                    "kind": "domain_validation",
                    "code": "invalid_variable_graph_payload",
                    "message": "value must be a strict VariableGraph object",
                },
            })
            .to_string();
        }
    };

    match ugoite_domain::composition::compile_variable_graph(&graph) {
        Ok(compiled) => {
            let envelope = serde_json::json!({
                "ok": true,
                "value": {
                    "contract_version": 1,
                    "topological_order": compiled.topological_order,
                    "dependencies": compiled.dependencies,
                },
            });
            if ensure_json_size(&envelope, MAX_PROTOCOL_REQUEST_BYTES).is_err() {
                return variable_graph_result_too_large();
            }
            envelope.to_string()
        }
        Err(diagnostics) => {
            let envelope = serde_json::json!({
                "ok": false,
                "error": {
                    "kind": "variable_graph_diagnostics",
                    "message": "variable graph validation failed",
                    "contract_version": 1,
                    "diagnostics": diagnostics,
                },
            });
            if ensure_json_size(&envelope, MAX_PROTOCOL_REQUEST_BYTES).is_err() {
                return variable_graph_result_too_large();
            }
            envelope.to_string()
        }
    }
}

fn variable_graph_result_too_large() -> String {
    serde_json::json!({
        "ok": false,
        "error": {
            "kind": "domain_validation",
            "code": "variable_graph_result_too_large",
            "message": "variable graph compilation result exceeds the protocol size limit",
        },
    })
    .to_string()
}

fn invoke_sql_document(request: serde_json::Value) -> String {
    let result = (|| -> Result<ugoite_domain::sql_document::SqlDocument, String> {
        let payload = request
            .get("value")
            .cloned()
            .ok_or_else(|| "value is required".to_string())?;
        let sql = payload
            .get("sql")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "sql must be a string".to_string())?;
        Ok(ugoite_domain::sql_document::parse_sql_document(sql))
    })();

    match result {
        Ok(document) => serialize_sql_document_response(&document)
            .unwrap_or_else(|()| sql_document_response_too_large()),
        Err(message) => serde_json::json!({
            "ok": false,
            "error": {"kind": "domain_validation", "message": message},
        })
        .to_string(),
    }
}

fn invoke_sql_form_name_references(request: serde_json::Value) -> String {
    let result = (|| -> Result<std::collections::BTreeSet<String>, String> {
        let payload = request
            .get("value")
            .cloned()
            .ok_or_else(|| "value is required".to_string())?;
        let sql = payload
            .get("sql")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "sql must be a string".to_string())?;
        ugoite_domain::sql_form_references::collect_quoted_form_name_references(sql)
    })();

    match result {
        Ok(references) => {
            let mut writer = BoundedJsonWriter::new(MAX_SQL_DOCUMENT_RESPONSE_BYTES);
            if writer.write_all(b"{\"ok\":true,\"value\":").is_err()
                || serde_json::to_writer(&mut writer, &references).is_err()
                || writer.write_all(b"}").is_err()
            {
                return sql_document_response_too_large();
            }
            String::from_utf8(writer.bytes).unwrap_or_else(|_| sql_document_response_too_large())
        }
        Err(message) => serde_json::json!({
            "ok": false,
            "error": { "kind": "domain_validation", "message": message },
        })
        .to_string(),
    }
}

fn invoke_sql_form_relation_names(request: serde_json::Value) -> String {
    let result = (|| -> Result<std::collections::BTreeSet<String>, String> {
        let payload = request
            .get("value")
            .cloned()
            .ok_or_else(|| "value is required".to_string())?;
        let sql = payload
            .get("sql")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "sql must be a string".to_string())?;
        ugoite_domain::sql_form_references::collect_quoted_form_relation_names(sql)
    })();

    match result {
        Ok(references) => {
            let mut writer = BoundedJsonWriter::new(MAX_SQL_DOCUMENT_RESPONSE_BYTES);
            if writer.write_all(b"{\"ok\":true,\"value\":").is_err()
                || serde_json::to_writer(&mut writer, &references).is_err()
                || writer.write_all(b"}").is_err()
            {
                return sql_document_response_too_large();
            }
            String::from_utf8(writer.bytes).unwrap_or_else(|_| sql_document_response_too_large())
        }
        Err(message) => serde_json::json!({
            "ok": false,
            "error": { "kind": "domain_validation", "message": message },
        })
        .to_string(),
    }
}

fn invoke_sql_document_parameter_rename(request: serde_json::Value) -> String {
    let result = (|| -> Result<ugoite_domain::sql_document::SqlDocument, String> {
        let payload = request
            .get("value")
            .cloned()
            .ok_or_else(|| "value is required".to_string())?;
        let sql = payload
            .get("sql")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "sql must be a string".to_string())?;
        let old_name = payload
            .get("old_name")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "old_name must be a string".to_string())?;
        let new_name = payload
            .get("new_name")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "new_name must be a string".to_string())?;
        ugoite_domain::sql_document::rename_sql_document_parameter(sql, old_name, new_name)
            .map_err(|error| error.to_string())
    })();

    match result {
        Ok(document) => serialize_sql_document_response(&document)
            .unwrap_or_else(|()| sql_document_response_too_large()),
        Err(message) => serde_json::json!({
            "ok": false,
            "error": {"kind": "domain_validation", "message": message},
        })
        .to_string(),
    }
}

fn serialize_sql_document_response(
    document: &ugoite_domain::sql_document::SqlDocument,
) -> Result<String, ()> {
    let mut writer = BoundedJsonWriter::new(MAX_SQL_DOCUMENT_RESPONSE_BYTES);
    writer
        .write_all(b"{\"ok\":true,\"value\":")
        .map_err(|_| ())?;
    serde_json::to_writer(&mut writer, document).map_err(|_| ())?;
    writer.write_all(b"}").map_err(|_| ())?;
    String::from_utf8(writer.bytes).map_err(|_| ())
}

fn sql_document_response_too_large() -> String {
    r#"{"ok":false,"error":{"kind":"domain_output_too_large","message":"SQL Document response exceeds the 1 MiB limit"}}"#.to_owned()
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SqlDocumentEditProtocolRequest {
    current_source: String,
    transaction: ugoite_domain::sql_document::SqlDocumentEditTransaction,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SqlDocumentNodeEditProtocolRequest {
    current_source: String,
    transaction: ugoite_domain::sql_document::SqlDocumentNodeEditTransaction,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SqlDocumentSemanticEditProtocolRequest {
    current_source: String,
    transaction: ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction,
}

fn invoke_sql_document_edit_transaction(request: serde_json::Value) -> String {
    let Some(payload) = request.get("value").cloned() else {
        return sql_document_edit_invalid_request();
    };
    let Ok(protocol_request) = serde_json::from_value::<SqlDocumentEditProtocolRequest>(payload)
    else {
        return sql_document_edit_invalid_request();
    };

    match ugoite_domain::sql_document::apply_sql_document_edit_transaction(
        &protocol_request.current_source,
        &protocol_request.transaction,
    ) {
        Ok(result) => serialize_sql_document_edit_response(&result)
            .unwrap_or_else(|()| sql_document_response_too_large()),
        Err(error) => sql_document_edit_error(error),
    }
}

fn invoke_sql_document_node_edit_transaction(request: serde_json::Value) -> String {
    let Some(payload) = request.get("value").cloned() else {
        return sql_document_edit_invalid_request();
    };
    let Ok(protocol_request) =
        serde_json::from_value::<SqlDocumentNodeEditProtocolRequest>(payload)
    else {
        return sql_document_edit_invalid_request();
    };

    match ugoite_domain::sql_document::apply_sql_document_node_edit_transaction(
        &protocol_request.current_source,
        &protocol_request.transaction,
    ) {
        Ok(result) => serialize_sql_document_edit_response(&result)
            .unwrap_or_else(|()| sql_document_response_too_large()),
        Err(error) => sql_document_edit_error(error),
    }
}

fn invoke_sql_document_semantic_edit_transaction(request: serde_json::Value) -> String {
    let Some(payload) = request.get("value").cloned() else {
        return sql_document_edit_invalid_request();
    };
    let Ok(protocol_request) =
        serde_json::from_value::<SqlDocumentSemanticEditProtocolRequest>(payload)
    else {
        return sql_document_edit_invalid_request();
    };

    match ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
        &protocol_request.current_source,
        &protocol_request.transaction,
    ) {
        Ok(result) => serialize_sql_document_edit_response(&result)
            .unwrap_or_else(|()| sql_document_response_too_large()),
        Err(error) => sql_document_edit_error(error),
    }
}

fn sql_document_edit_invalid_request() -> String {
    r#"{"ok":false,"error":{"kind":"sql_document_edit","code":"invalid_request","message":"SQL Document edit request is invalid"}}"#.to_owned()
}

fn serialize_sql_document_edit_response(
    result: &ugoite_domain::sql_document::SqlDocumentEditResult,
) -> Result<String, ()> {
    let mut writer = BoundedJsonWriter::new(MAX_SQL_DOCUMENT_RESPONSE_BYTES);
    writer
        .write_all(b"{\"ok\":true,\"value\":")
        .map_err(|_| ())?;
    serde_json::to_writer(&mut writer, result).map_err(|_| ())?;
    writer.write_all(b"}").map_err(|_| ())?;
    String::from_utf8(writer.bytes).map_err(|_| ())
}

fn sql_document_edit_error(error: ugoite_domain::sql_document::SqlDocumentEditError) -> String {
    let code = match error {
        ugoite_domain::sql_document::SqlDocumentEditError::InvalidOldParameterName => {
            "invalid_old_parameter_name"
        }
        ugoite_domain::sql_document::SqlDocumentEditError::InvalidNewParameterName => {
            "invalid_new_parameter_name"
        }
        ugoite_domain::sql_document::SqlDocumentEditError::StaleSource => "stale_source",
        ugoite_domain::sql_document::SqlDocumentEditError::SourceTooLarge => "source_too_large",
        ugoite_domain::sql_document::SqlDocumentEditError::SourceTooComplex => "source_too_complex",
        ugoite_domain::sql_document::SqlDocumentEditError::TooManyEdits => "too_many_edits",
        ugoite_domain::sql_document::SqlDocumentEditError::DocumentNotParsed => {
            "document_not_parsed"
        }
        ugoite_domain::sql_document::SqlDocumentEditError::ContractVersionMismatch => {
            "contract_version_mismatch"
        }
        ugoite_domain::sql_document::SqlDocumentEditError::NodeNotFound => "node_not_found",
        ugoite_domain::sql_document::SqlDocumentEditError::NodeKindMismatch => "node_kind_mismatch",
        ugoite_domain::sql_document::SqlDocumentEditError::NodeRangeUnavailable => {
            "node_range_unavailable"
        }
        ugoite_domain::sql_document::SqlDocumentEditError::InvalidSemanticReplacement => {
            "invalid_semantic_replacement"
        }
        ugoite_domain::sql_document::SqlDocumentEditError::ProjectionPositionConflict => {
            "projection_position_conflict"
        }
        ugoite_domain::sql_document::SqlDocumentEditError::ProjectionAliasConflict => {
            "projection_alias_conflict"
        }
        ugoite_domain::sql_document::SqlDocumentEditError::DuplicateNodeTarget => {
            "duplicate_node_target"
        }
        ugoite_domain::sql_document::SqlDocumentEditError::InvalidRange => "invalid_range",
        ugoite_domain::sql_document::SqlDocumentEditError::RangeOutOfBounds => {
            "range_out_of_bounds"
        }
        ugoite_domain::sql_document::SqlDocumentEditError::NotUtf8Boundary => "not_utf8_boundary",
        ugoite_domain::sql_document::SqlDocumentEditError::OverlappingEdits => "overlapping_edits",
        ugoite_domain::sql_document::SqlDocumentEditError::PreimageMismatch => "preimage_mismatch",
        ugoite_domain::sql_document::SqlDocumentEditError::OutputTooLarge => "output_too_large",
    };
    serde_json::json!({
        "ok": false,
        "error": {
            "kind": "sql_document_edit",
            "code": code,
            "message": error.to_string(),
        },
    })
    .to_string()
}

fn composition_canonicalization_error(
    code: ugoite_domain::composition::CompositionDiagnosticCode,
) -> String {
    serde_json::json!({
        "ok": false,
        "error": {"kind": "composition_diagnostic", "code": code.as_str()},
    })
    .to_string()
}

fn parse_entry_draft(
    value: &serde_json::Value,
) -> Result<ugoite_core::entry::StructuredEntryDraft, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "draft must be an object".to_string())?;
    // Accept only the canonical StructuredEntryDraft JSON shape. Omitted
    // collections remain valid, but transitional casing/name aliases do not.
    // Present-but-malformed values are INVALID_INPUT so diagnostics parity
    // holds where it matters most (wrong code/detail is worse than strict).
    if object.contains_key("form") {
        return Err("draft.form is not supported; use draft.form_name".to_string());
    }
    if object.contains_key("extraAttributes") {
        return Err(
            "draft.extraAttributes is not supported; use draft.extra_attributes".to_string(),
        );
    }
    let form_name = object
        .get("form_name")
        .filter(|value| !value.is_null())
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| "draft.form_name must be a string".to_string())
        })
        .transpose()?;
    let tags = match object.get("tags") {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| "draft.tags must be an array of strings".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err("draft.tags must be an array of strings".to_string()),
    };
    let fields = match object.get("fields") {
        None | Some(serde_json::Value::Null) => Default::default(),
        Some(serde_json::Value::Object(map)) => {
            map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        }
        Some(_) => return Err("draft.fields must be an object".to_string()),
    };
    let extra_attributes = match object.get("extra_attributes") {
        None | Some(serde_json::Value::Null) => Default::default(),
        Some(serde_json::Value::Object(map)) => {
            map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        }
        Some(_) => return Err("draft.extra_attributes must be an object".to_string()),
    };
    Ok(ugoite_core::entry::StructuredEntryDraft {
        form_name,
        tags,
        fields,
        extra_attributes,
    })
}

fn entry_validation_error(error: &ugoite_core::error::AppError) -> serde_json::Value {
    let mut envelope = serde_json::json!({
        "kind": "entry_validation",
        "code": error.code_str(),
        "message": error.message(),
    });
    if let Some(detail) = error.detail() {
        envelope["detail"] = detail.clone();
    }
    envelope
}

fn entry_error_envelope(error: serde_json::Value) -> String {
    let envelope = serde_json::json!({"ok": false, "error": error});
    if ensure_json_size(&envelope, MAX_PROTOCOL_REQUEST_BYTES).is_err() {
        return serde_json::json!({
            "ok": false,
            "error": {
                "kind": "entry_validation",
                "code": "INVALID_INPUT",
                "message": "Entry protocol error output exceeds the size limit"
            }
        })
        .to_string();
    }
    envelope.to_string()
}

/// Portable Entry authoring boundary (read-only, no Storage).
///
/// - `entry.validate_draft` validates `{form, draft}` with the same
///   `preview_structured_draft` implementation used natively, preserving
///   `code` / `detail` so browser diagnostics match server mutations.
fn invoke_entry(request: serde_json::Value) -> String {
    let action = request
        .get("action")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let payload = request
        .get("value")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let result = (|| -> Result<serde_json::Value, serde_json::Value> {
        match action.as_str() {
            "entry.validate_draft" => {
                let form: ugoite_domain::form::FormDefinition = serde_json::from_value(
                    payload
                        .get("form")
                        .cloned()
                        .ok_or_else(|| serde_json::json!({"kind": "entry_validation", "code": "INVALID_INPUT", "message": "form is required"}))?,
                )
                .map_err(|error| serde_json::json!({"kind": "entry_validation", "code": "INVALID_INPUT", "message": error.to_string()}))?;
                let draft = parse_entry_draft(
                    &payload
                        .get("draft")
                        .cloned()
                        .ok_or_else(|| serde_json::json!({"kind": "entry_validation", "code": "INVALID_INPUT", "message": "draft is required"}))?,
                )
                .map_err(|message| serde_json::json!({"kind": "entry_validation", "code": "INVALID_INPUT", "message": message}))?;
                match ugoite_core::entry::preview_structured_draft(&form, &draft) {
                    Ok(normalized) => serde_json::to_value(normalized)
                        .map_err(|error| serde_json::json!({"kind": "entry_validation", "code": "INVALID_INPUT", "message": error.to_string()})),
                    Err(error) => Err(entry_validation_error(&error)),
                }
            }
            _ => Err(
                serde_json::json!({"kind": "entry_validation", "code": "INVALID_INPUT", "message": format!("unsupported portable entry action: {action}")}),
            ),
        }
    })();
    match result {
        Ok(value) => {
            let envelope = serde_json::json!({"ok": true, "value": value});
            if ensure_json_size(&envelope, MAX_PROTOCOL_REQUEST_BYTES).is_err() {
                return serde_json::json!({"ok": false, "error": {"kind": "entry_validation", "code": "INVALID_INPUT", "message": "Entry protocol output exceeds the size limit"}}).to_string();
            }
            envelope.to_string()
        }
        Err(error) => entry_error_envelope(error),
    }
}

#[cfg(target_arch = "wasm32")]
mod abi {
    use std::cell::RefCell;

    thread_local! {
        static LAST_RESULT: RefCell<Vec<u8>> = RefCell::new(Vec::new());
    }

    #[no_mangle]
    pub extern "C" fn ugoite_protocol_version() -> u32 {
        ugoite_api_client::PROTOCOL_VERSION
    }

    #[no_mangle]
    pub extern "C" fn ugoite_alloc(length: usize) -> *mut u8 {
        Box::into_raw(vec![0_u8; length].into_boxed_slice()) as *mut u8
    }

    /// # Safety
    ///
    /// `pointer` and `length` must identify a buffer returned by
    /// [`ugoite_alloc`] that has not already been freed.
    #[no_mangle]
    pub unsafe extern "C" fn ugoite_dealloc(pointer: *mut u8, length: usize) {
        if pointer.is_null() {
            return;
        }
        let slice = std::ptr::slice_from_raw_parts_mut(pointer, length);
        drop(unsafe { Box::from_raw(slice) });
    }

    /// Invoke the JSON protocol and store the UTF-8 result in an internal
    /// buffer. The return value is `0` on success and non-zero only when the
    /// input pointer itself is invalid. Protocol-level errors are returned as
    /// normal JSON envelopes.
    ///
    /// # Safety
    ///
    /// `pointer` and `length` must identify readable bytes in this module's
    /// linear memory for the duration of the call.
    #[no_mangle]
    pub unsafe extern "C" fn ugoite_protocol_invoke(pointer: *const u8, length: usize) -> u32 {
        if pointer.is_null() && length != 0 {
            LAST_RESULT.with(|slot| {
                *slot.borrow_mut() = br#"{"ok":false,"error":{"kind":"invalid_pointer","message":"input pointer was null"}}"#.to_vec();
            });
            return 1;
        }

        let bytes: &[u8] = if length == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(pointer, length) }
        };
        let input = match std::str::from_utf8(bytes) {
            Ok(input) => input,
            Err(error) => {
                let result = serde_json::json!({
                    "ok": false,
                    "error": {
                        "kind": "invalid_utf8",
                        "message": format!("protocol input was not UTF-8: {error}"),
                    }
                })
                .to_string();
                LAST_RESULT.with(|slot| *slot.borrow_mut() = result.into_bytes());
                return 0;
            }
        };

        let result = crate::invoke_json(input);
        LAST_RESULT.with(|slot| *slot.borrow_mut() = result.into_bytes());
        0
    }

    #[no_mangle]
    pub extern "C" fn ugoite_protocol_result_pointer() -> *const u8 {
        LAST_RESULT.with(|slot| slot.borrow().as_ptr())
    }

    #[no_mangle]
    pub extern "C" fn ugoite_protocol_result_length() -> usize {
        LAST_RESULT.with(|slot| slot.borrow().len())
    }

    #[no_mangle]
    pub extern "C" fn ugoite_protocol_clear_result() {
        LAST_RESULT.with(|slot| slot.borrow_mut().clear());
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    const SQL_FIRST_COMPOSITION: &str =
        include_str!("../../ugoite-domain/tests/fixtures/composition/sql-first.ugcomp.yaml");

    #[test]
    fn test_api_req_api_001_wasm_adapter_exposes_protocol_version() {
        let response = super::invoke_json(r#"{"action":"version"}"#);
        let response: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"]["protocol_version"], 1);
    }

    #[test]
    fn oversized_protocol_requests_use_limit_neutral_diagnostic() {
        let generic_request = serde_json::json!({
            "action": "version",
            "padding": "x".repeat(super::MAX_PROTOCOL_REQUEST_BYTES + 1),
        })
        .to_string();
        assert!(generic_request.len() > super::MAX_PROTOCOL_REQUEST_BYTES);
        assert!(generic_request.len() <= super::MAX_SQL_DOCUMENT_REQUEST_BYTES);

        let sql_document_request = serde_json::json!({
            "action": "domain.parse_sql_document",
            "value": {
                "sql": "x".repeat(super::MAX_SQL_DOCUMENT_REQUEST_BYTES + 1),
            },
        })
        .to_string();
        assert!(sql_document_request.len() > super::MAX_SQL_DOCUMENT_REQUEST_BYTES);

        let sql_document_parameter_rename_request = serde_json::json!({
            "action": "domain.rename_sql_document_parameter",
            "value": {
                "sql": "x".repeat(8 * 1024 * 1024),
                "old_name": "old",
                "new_name": "new",
            },
        })
        .to_string();
        assert!(
            sql_document_parameter_rename_request.len() > super::MAX_SQL_DOCUMENT_REQUEST_BYTES
        );

        for (action, request) in [
            ("generic", generic_request),
            ("SQL Document", sql_document_request),
            (
                "SQL Document parameter rename",
                sql_document_parameter_rename_request,
            ),
        ] {
            let response: Value = serde_json::from_str(&super::invoke_json(&request)).unwrap();
            assert_eq!(response["ok"], false, "{action}: {response}");
            assert_eq!(response["error"]["kind"], "input_too_large", "{action}");
            assert_eq!(
                response["error"]["message"], "JSON protocol input exceeds its size limit",
                "{action}"
            );
        }
    }

    #[test]
    fn variable_graph_protocol_returns_deterministic_native_compilation() {
        let integer = serde_json::json!({
            "scalar_type": "integer",
            "nullable": false,
        });
        let query = |id: &str, sql: &str, arguments: Value| {
            serde_json::json!({
                "id": id,
                "kind": "query",
                "returns": {"kind": "scalar", "value_type": integer},
                "query": {
                    "kind": "inline_sql",
                    "sql": sql,
                    "arguments": arguments,
                },
            })
        };
        let input = serde_json::json!({
            "id": "input",
            "kind": "input",
            "value_type": integer,
            "required": true,
        });
        let binding = serde_json::json!([{
            "name": "value",
            "variable": "input",
        }]);
        let graph_value = serde_json::json!({
            "variables": [
                query("z_query", "SELECT $value", binding),
                input,
                query("b_query", "SELECT 1", serde_json::json!([])),
            ],
        });

        let graph = serde_json::from_value::<ugoite_domain::composition::VariableGraph>(
            graph_value.clone(),
        )
        .expect("strict graph fixture");
        let native =
            ugoite_domain::composition::compile_variable_graph(&graph).expect("graph compiles");
        assert_eq!(native.topological_order, ["b_query", "input", "z_query"]);

        let compile = |value| {
            let request = serde_json::json!({
                "action": "domain.compile_variable_graph.v1",
                "value": value,
            });
            let response: Value =
                serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
            assert_eq!(response["ok"], true, "{response}");
            response["value"].clone()
        };
        let first = compile(graph_value.clone());
        let mut reversed = graph_value;
        reversed["variables"].as_array_mut().unwrap().reverse();
        let second = compile(reversed);

        assert_eq!(first, second);
        assert_eq!(first["contract_version"], 1);
        assert_eq!(
            first,
            serde_json::json!({
                "contract_version": 1,
                "topological_order": native.topological_order,
                "dependencies": native.dependencies,
            })
        );
    }

    #[test]
    fn variable_graph_protocol_returns_stable_reference_and_type_diagnostics() {
        let query_graph = |reference: &str, input: Value| {
            serde_json::json!({
                "variables": [
                    input,
                    {
                        "id": "result",
                        "kind": "query",
                        "returns": {
                            "kind": "scalar",
                            "value_type": {"scalar_type": "integer", "nullable": false},
                        },
                        "query": {
                            "kind": "inline_sql",
                            "sql": "SELECT $value",
                            "arguments": [{
                                "name": "value",
                                "variable": reference,
                            }],
                        },
                    },
                ],
            })
        };

        for graph_value in [
            query_graph(
                "missing",
                serde_json::json!({
                    "id": "input",
                    "kind": "input",
                    "value_type": {"scalar_type": "integer", "nullable": false},
                    "required": true,
                }),
            ),
            query_graph(
                "input",
                serde_json::json!({
                    "id": "input",
                    "kind": "query",
                    "returns": {
                        "kind": "table",
                        "columns": [{
                            "name": "value",
                            "value_type": {"scalar_type": "integer", "nullable": false},
                        }],
                    },
                    "query": {"kind": "inline_sql", "sql": "SELECT 1"},
                }),
            ),
        ] {
            let graph = serde_json::from_value::<ugoite_domain::composition::VariableGraph>(
                graph_value.clone(),
            )
            .expect("strict graph fixture");
            let native = ugoite_domain::composition::compile_variable_graph(&graph)
                .expect_err("graph reference or type is invalid");
            let request = serde_json::json!({
                "action": "domain.compile_variable_graph.v1",
                "value": graph_value,
            });
            let response: Value =
                serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
            assert_eq!(response["ok"], false, "{response}");
            assert_eq!(response["error"]["kind"], "variable_graph_diagnostics");
            assert_eq!(response["error"]["contract_version"], 1);
            assert_eq!(
                response["error"]["diagnostics"],
                serde_json::to_value(native).unwrap()
            );
        }
    }

    #[test]
    fn sql_document_protocol_matches_native_parser_for_draft_states_and_ranges() {
        let oversized_sql = "x".repeat(ugoite_domain::sql_document::MAX_SQL_DOCUMENT_BYTES + 1);
        let too_complex_sql =
            "(".repeat(ugoite_domain::sql_document::MAX_SQL_DOCUMENT_COMPLEXITY_TOKENS + 1);
        let cases = [
            (
                "parsed",
                "-- 日付\nSELECT \"日付\", $day FROM \"G\" WHERE note = '$day'",
                "parsed",
            ),
            ("invalid", "SELECT (", "invalid_sql"),
            ("multiple", "SELECT 1; SELECT 2", "multiple_statements"),
            ("too complex", too_complex_sql.as_str(), "too_complex"),
            ("oversized", oversized_sql.as_str(), "too_large"),
        ];

        for (name, sql, status) in cases {
            let request = serde_json::json!({
                "action": "domain.parse_sql_document",
                "value": {"sql": sql},
            })
            .to_string();
            if name == "oversized" {
                assert!(request.len() > super::MAX_PROTOCOL_REQUEST_BYTES);
                assert!(request.len() <= super::MAX_SQL_DOCUMENT_REQUEST_BYTES);
            }

            let response: Value = serde_json::from_str(&super::invoke_json(&request)).unwrap();
            assert_eq!(response["ok"], true, "{name}: {response}");
            assert_eq!(response["value"]["source"], sql, "{name}: source");
            assert_eq!(response["value"]["status"], status, "{name}: status");

            // The portable adapter returns the exact domain document, including
            // UTF-8 byte ranges and diagnostics; it does not plan or execute SQL.
            let native = ugoite_domain::sql_document::parse_sql_document(sql);
            assert_eq!(response["value"], serde_json::to_value(native).unwrap());
        }
    }

    #[test]
    fn sql_form_reference_protocol_uses_native_cte_scoped_classifier() {
        let sql = "SELECT * FROM (WITH \"Entry\" AS (SELECT 1) SELECT * FROM \"Entry\") AS nested JOIN \"Entry\" AS actual ON TRUE";
        let request = serde_json::json!({
            "action": "domain.collect_quoted_form_name_references",
            "value": { "sql": sql },
        })
        .to_string();
        let response: Value = serde_json::from_str(&super::invoke_json(&request)).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::json!(["Entry"]));
        assert_eq!(
            response["value"],
            serde_json::to_value(
                ugoite_domain::sql_form_references::collect_quoted_form_name_references(sql)
                    .unwrap(),
            )
            .unwrap(),
        );

        let legacy_relation = "form_0123456789abcdef0123456789abcdef";
        let relation_sql = format!("SELECT * FROM \"{legacy_relation}\"");
        let relation_request = serde_json::json!({
            "action": "domain.collect_quoted_form_relation_names",
            "value": { "sql": relation_sql },
        })
        .to_string();
        let relation_response: Value =
            serde_json::from_str(&super::invoke_json(&relation_request)).unwrap();
        assert_eq!(relation_response["ok"], true, "{relation_response}");
        assert_eq!(
            relation_response["value"],
            serde_json::json!([legacy_relation]),
        );

        let invalid: Value = serde_json::from_str(&super::invoke_json(
            &serde_json::json!({
                "action": "domain.collect_quoted_form_name_references",
                "value": { "sql": "SELECT (" },
            })
            .to_string(),
        ))
        .unwrap();
        assert_eq!(invalid["ok"], false);
        assert_eq!(invalid["error"]["kind"], "domain_validation");

        let oversized = serde_json::json!({
            "action": "domain.collect_quoted_form_name_references",
            "value": { "sql": "x".repeat(super::MAX_SQL_DOCUMENT_REQUEST_BYTES) },
        })
        .to_string();
        assert!(oversized.len() > super::MAX_SQL_DOCUMENT_REQUEST_BYTES);
        let rejected: Value = serde_json::from_str(&super::invoke_json(&oversized)).unwrap();
        assert_eq!(rejected["ok"], false);
        assert_eq!(rejected["error"]["kind"], "input_too_large");
    }

    #[test]
    fn variable_graph_protocol_reports_cycles_and_rejects_unknown_fields() {
        let integer = serde_json::json!({
            "scalar_type": "integer",
            "nullable": false,
        });
        let cycle_graph = serde_json::json!({
            "variables": [
                {
                    "id": "alpha",
                    "kind": "query",
                    "returns": {"kind": "scalar", "value_type": integer},
                    "query": {
                        "kind": "inline_sql",
                        "sql": "SELECT $value",
                        "arguments": [{"name": "value", "variable": "beta"}],
                    },
                },
                {
                    "id": "beta",
                    "kind": "query",
                    "returns": {"kind": "scalar", "value_type": integer},
                    "query": {
                        "kind": "inline_sql",
                        "sql": "SELECT $value",
                        "arguments": [{"name": "value", "variable": "alpha"}],
                    },
                },
            ],
        });
        let graph = serde_json::from_value::<ugoite_domain::composition::VariableGraph>(
            cycle_graph.clone(),
        )
        .unwrap();
        let native = ugoite_domain::composition::compile_variable_graph(&graph)
            .expect_err("cycle must fail");
        let request = serde_json::json!({
            "action": "domain.compile_variable_graph.v1",
            "value": cycle_graph,
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], false, "{response}");
        assert_eq!(
            response["error"]["diagnostics"],
            serde_json::to_value(native).unwrap()
        );
        assert_eq!(
            response["error"]["diagnostics"][0]["code"],
            "dependency_cycle"
        );

        let mut unknown_graph = serde_json::json!({"variables": []});
        unknown_graph["future_field"] = serde_json::json!(true);
        let unknown_request = serde_json::json!({
            "action": "domain.compile_variable_graph.v1",
            "value": unknown_graph,
        });
        let unknown_response: Value =
            serde_json::from_str(&super::invoke_json(&unknown_request.to_string())).unwrap();
        assert_eq!(unknown_response["ok"], false, "{unknown_response}");
        assert_eq!(
            unknown_response["error"]["code"],
            "invalid_variable_graph_payload"
        );
        assert_eq!(
            unknown_response["error"]["message"],
            "value must be a strict VariableGraph object"
        );

        let nested_unknown_request = serde_json::json!({
            "action": "domain.compile_variable_graph.v1",
            "value": {"variables": [{"id": "x", "kind": "input", "value_type": integer, "required": true, "future_field": 1}]},
        });
        let nested_unknown_response: Value =
            serde_json::from_str(&super::invoke_json(&nested_unknown_request.to_string())).unwrap();
        assert_eq!(nested_unknown_response["ok"], false);
        assert_eq!(
            nested_unknown_response["error"]["code"],
            "invalid_variable_graph_payload"
        );
    }

    #[test]
    fn sql_document_parameter_rename_protocol_matches_native_tokens_and_ranges() {
        let source = "-- $day\nSELECT $day + $day AS total, '$day' AS literal, $day2 AS other FROM \"G\" /* $day */";
        let native = ugoite_domain::sql_document::rename_sql_document_parameter(
            source,
            "day",
            "billing_day",
        )
        .expect("valid parameter rename");
        let request = serde_json::json!({
            "action": "domain.rename_sql_document_parameter",
            "value": {"sql": source, "old_name": "day", "new_name": "billing_day"},
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).expect("JSON response");

        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            native.source,
            "-- $day\nSELECT $billing_day + $billing_day AS total, '$day' AS literal, $day2 AS other FROM \"G\" /* $day */"
        );
        let renamed: ugoite_domain::sql_document::SqlDocument =
            serde_json::from_value(response["value"].clone()).expect("domain document");
        let placeholders = renamed
            .tokens
            .iter()
            .filter(|token| {
                token.kind == ugoite_domain::sql_document::SqlDocumentTokenKind::Placeholder
            })
            .map(|token| renamed.source_slice(token.range).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(placeholders, ["$billing_day", "$billing_day", "$day2"]);
    }

    #[test]
    fn sql_document_parameter_rename_protocol_preserves_no_match_documents() {
        let source = "-- retain $other\nSELECT '$other' AS literal, $other FROM \"G\"";
        let native = ugoite_domain::sql_document::rename_sql_document_parameter(
            source,
            "missing",
            "replacement",
        )
        .expect("valid names with no matching placeholder");
        let request = serde_json::json!({
            "action": "domain.rename_sql_document_parameter",
            "value": {"sql": source, "old_name": "missing", "new_name": "replacement"},
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).expect("JSON response");

        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(native.source, source);
    }

    #[test]
    fn sql_document_parameter_rename_protocol_matches_native_invalid_name_errors() {
        for (old_name, new_name) in [("bad-name", "new_name"), ("old", "$new")] {
            let native_error = ugoite_domain::sql_document::rename_sql_document_parameter(
                "SELECT $old",
                old_name,
                new_name,
            )
            .expect_err("invalid parameter names must fail");
            let request = serde_json::json!({
                "action": "domain.rename_sql_document_parameter",
                "value": {
                    "sql": "SELECT $old",
                    "old_name": old_name,
                    "new_name": new_name,
                },
            });
            let response: Value = serde_json::from_str(&super::invoke_json(&request.to_string()))
                .expect("JSON response");

            assert_eq!(response["ok"], false, "{response}");
            assert_eq!(response["error"]["kind"], "domain_validation");
            assert_eq!(response["error"]["message"], native_error.to_string());
        }
    }

    #[test]
    fn sql_structure_protocol_proposes_form_relation_selection() {
        let relation = "form_0123456789abcdef0123456789abcdef";
        for (sql, expected) in [
            ("SELECT 1", format!("SELECT * FROM \"{relation}\"")),
            (
                "SELECT \"_ugoite_id\" FROM \"old_relation\"",
                format!("SELECT \"_ugoite_id\" FROM \"{relation}\""),
            ),
        ] {
            let request = serde_json::json!({
                "action": "domain.propose_sql_structure_operation",
                "value": {
                    "sql": sql,
                    "context": {
                        "relation": relation,
                        "columns": [
                            {"name": "_ugoite_id", "type": "string"}
                        ],
                        "variables": [],
                        "stable_key": ["_ugoite_id"]
                    },
                    "operation": {
                        "kind": "set_relation",
                        "relation": relation
                    }
                }
            })
            .to_string();
            let response: Value = serde_json::from_str(&super::invoke_json(&request)).unwrap();
            assert_eq!(response["ok"], true, "{response}");
            assert_eq!(response["value"]["status"], "supported");
            assert_eq!(response["value"]["proposed_sql"], expected);
        }
    }

    #[test]
    fn sql_structure_protocol_exposes_row_limit_analysis_and_edit() {
        let request = serde_json::json!({
            "action": "domain.analyze_sql_structure",
            "value": {
                "sql": "SELECT * FROM \"Task\"",
                "context": {
                    "relation": "Task",
                    "columns": [{"name": "_ugoite_id", "type": "string"}],
                    "variables": [],
                    "stable_key": ["_ugoite_id"]
                }
            }
        });
        let analysis: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).expect("analysis JSON");
        assert_eq!(analysis["ok"], true, "{analysis}");
        assert_eq!(analysis["value"]["contract_version"], 7);
        assert_eq!(analysis["value"]["row_limit"], Value::Null);
        assert_eq!(
            analysis["value"]["capabilities"]["row_limit"]["enabled"],
            true
        );

        let request = serde_json::json!({
            "action": "domain.propose_sql_structure_operation",
            "value": {
                "sql": "SELECT * FROM \"Task\" ORDER BY \"_ugoite_id\"",
                "context": {
                    "relation": "Task",
                    "columns": [{"name": "_ugoite_id", "type": "string"}],
                    "variables": [],
                    "stable_key": ["_ugoite_id"]
                },
                "operation": {
                    "kind": "set_row_limit",
                    "limit": "50",
                    "offset": "10"
                }
            }
        });
        let proposal: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).expect("proposal JSON");
        assert_eq!(proposal["ok"], true, "{proposal}");
        assert_eq!(proposal["value"]["status"], "supported");
        assert_eq!(
            proposal["value"]["proposed_sql"],
            "SELECT * FROM \"Task\" ORDER BY \"_ugoite_id\" LIMIT 50 OFFSET 10"
        );
        assert_eq!(
            proposal["value"]["document_node_edit"]["edits"][0]["expected_kind"],
            "query"
        );
    }

    #[test]
    fn sql_structure_protocol_preserves_explicit_null_sort_placement() {
        let request = serde_json::json!({
            "action": "domain.propose_sql_structure_operation",
            "value": {
                "sql": "SELECT * FROM \"Task\" ORDER BY \"_ugoite_id\" ASC NULLS FIRST",
                "context": {
                    "relation": "Task",
                    "columns": [
                        {"name": "field_101", "type": "string"},
                        {"name": "_ugoite_id", "type": "string"}
                    ],
                    "variables": [],
                    "stable_key": ["_ugoite_id"]
                },
                "operation": {
                    "kind": "set_sort",
                    "order_by": [{
                        "column": "field_101",
                        "qualifier": null,
                        "projection_alias": null,
                        "direction": "desc",
                        "nulls_first": false
                    }]
                }
            }
        });
        let proposal: Value = serde_json::from_str(&super::invoke_json(&request.to_string()))
            .expect("Sort proposal JSON");
        assert_eq!(proposal["ok"], true, "{proposal}");
        assert_eq!(proposal["value"]["status"], "supported");
        assert_eq!(
            proposal["value"]["proposed_sql"],
            "SELECT * FROM \"Task\" ORDER BY \"field_101\" DESC NULLS LAST, \"_ugoite_id\" ASC"
        );
    }

    #[test]
    fn sql_structure_protocol_proposes_group_by_edit() {
        let request = serde_json::json!({
            "action": "domain.propose_sql_structure_operation",
            "value": {
                "sql": "SELECT \"field_102\" FROM \"Task\"",
                "context": {
                    "relation": "Task",
                    "columns": [{"name": "field_102", "type": "integer"}],
                    "variables": [],
                    "stable_key": []
                },
                "operation": {
                    "kind": "set_group_by",
                    "columns": ["field_102"]
                }
            }
        });
        let proposal: Value = serde_json::from_str(&super::invoke_json(&request.to_string()))
            .expect("GROUP BY proposal JSON");
        assert_eq!(proposal["ok"], true, "{proposal}");
        assert_eq!(proposal["value"]["status"], "supported");
        assert_eq!(
            proposal["value"]["proposed_sql"],
            "SELECT \"field_102\" FROM \"Task\" GROUP BY \"field_102\""
        );
    }

    #[test]
    fn sql_structure_protocol_proposes_join_edit() {
        let request = serde_json::json!({
            "action": "domain.propose_sql_structure_operation",
            "value": {
                "sql": "SELECT * FROM \"Task\"",
                "context": {
                    "relation": "Task",
                    "columns": [{"name": "field_101", "type": "string"}],
                    "variables": [],
                    "stable_key": []
                },
                "operation": {
                    "kind": "set_join",
                    "join_kind": "full",
                    "left_column": "field_101",
                    "right_column": "field_101"
                }
            }
        });
        let proposal: Value = serde_json::from_str(&super::invoke_json(&request.to_string()))
            .expect("JOIN proposal JSON");
        assert_eq!(proposal["ok"], true, "{proposal}");
        assert_eq!(proposal["value"]["status"], "supported");
        assert_eq!(
            proposal["value"]["proposed_sql"],
            "SELECT * FROM \"Task\" AS \"_ugoite_left\" FULL JOIN \"Task\" AS \"_ugoite_right\" ON \"_ugoite_left\".\"field_101\" = \"_ugoite_right\".\"field_101\""
        );

        let legacy_request = serde_json::json!({
            "action": "domain.propose_sql_structure_operation",
            "value": {
                "sql": "SELECT * FROM \"Task\"",
                "context": {
                    "relation": "Task",
                    "columns": [{"name": "field_101", "type": "string"}],
                    "variables": [],
                    "stable_key": []
                },
                "operation": {
                    "kind": "set_join",
                    "left_column": "field_101",
                    "right_column": "field_101"
                }
            }
        });
        let legacy_proposal: Value =
            serde_json::from_str(&super::invoke_json(&legacy_request.to_string()))
                .expect("legacy INNER JOIN proposal JSON");
        assert_eq!(legacy_proposal["ok"], true, "{legacy_proposal}");
        assert!(legacy_proposal["value"]["proposed_sql"]
            .as_str()
            .unwrap()
            .contains(" INNER JOIN "));
    }

    #[test]
    fn sql_structure_having_protocol_matches_domain_round_trip() {
        let sql = "SELECT SUM(\"amount\") FROM \"Task\"";
        let context = ugoite_domain::sql_structure::SqlStructureContext {
            relation: "Task".to_owned(),
            columns: vec![ugoite_domain::sql_structure::SqlStructureColumn {
                name: "amount".to_owned(),
                column_type: ugoite_domain::sql_structure::SqlStructureColumnType::Integer,
            }],
            variables: vec![],
            stable_key: vec![],
        };
        let operation = ugoite_domain::sql_structure::SqlStructureOperation::SetHaving {
            expression: Some(
                ugoite_domain::sql_structure::SqlStructureHavingExpression::Predicate {
                    aggregate: ugoite_domain::sql_structure::SqlStructureAggregate {
                        function: ugoite_domain::sql_structure::SqlStructureAggregateFunction::Sum,
                        column: Some("amount".to_owned()),
                        arguments: vec![],
                        distinct: false,
                        alias: None,
                    },
                    operator: ugoite_domain::sql_structure::SqlStructureHavingOperator::Gt,
                    value: ugoite_domain::sql_structure::SqlStructureHavingValue::Integer(
                        "0".to_owned(),
                    ),
                },
            ),
        };
        let expected_analysis = serde_json::to_value(
            ugoite_domain::sql_structure::analyze_sql_structure(sql, &context),
        )
        .expect("Domain analysis serializes");
        let analysis_request = serde_json::json!({
            "action": "domain.analyze_sql_structure",
            "value": {"sql": sql, "context": context.clone()},
        })
        .to_string();
        let analysis_response: Value =
            serde_json::from_str(&super::invoke_json(&analysis_request)).unwrap();
        assert_eq!(analysis_response["ok"], true, "{analysis_response}");
        assert_eq!(analysis_response["value"], expected_analysis);

        let expected_proposal = serde_json::to_value(
            ugoite_domain::sql_structure::propose_sql_structure_operation(
                sql,
                &context,
                operation.clone(),
            ),
        )
        .expect("Domain proposal serializes");
        let proposal_request = serde_json::json!({
            "action": "domain.propose_sql_structure_operation",
            "value": {"sql": sql, "context": context, "operation": operation},
        })
        .to_string();
        let proposal_response: Value =
            serde_json::from_str(&super::invoke_json(&proposal_request)).unwrap();
        assert_eq!(proposal_response["ok"], true, "{proposal_response}");
        assert_eq!(proposal_response["value"], expected_proposal);
        assert_eq!(
            proposal_response["value"]["proposed_sql"],
            "SELECT SUM(\"amount\") FROM \"Task\" HAVING SUM(\"amount\") > 0"
        );
    }

    #[test]
    fn sql_structure_protocol_proposes_count_aggregate() {
        let request = serde_json::json!({
            "action": "domain.propose_sql_structure_operation",
            "value": {
                "sql": "SELECT * FROM \"Task\"",
                "context": {
                    "relation": "Task",
                    "columns": [{"name": "field_101", "type": "string"}],
                    "variables": [],
                    "stable_key": []
                },
                "operation": {
                    "kind": "set_aggregates",
                    "aggregates": [{
                        "function": "count",
                        "column": null,
                        "arguments": [],
                        "distinct": false,
                        "alias": {"name": "row_count", "quoted": true}
                    }]
                }
            }
        });
        let proposal: Value = serde_json::from_str(&super::invoke_json(&request.to_string()))
            .expect("aggregate proposal JSON");
        assert_eq!(proposal["ok"], true, "{proposal}");
        assert_eq!(proposal["value"]["status"], "supported");
        assert_eq!(
            proposal["value"]["proposed_sql"],
            "SELECT COUNT(*) AS \"row_count\" FROM \"Task\""
        );
    }

    #[test]
    fn sql_structure_protocol_rejects_missing_filter_expression() {
        let request = serde_json::json!({
            "action": "domain.propose_sql_structure_operation",
            "value": {
                "sql": "SELECT * FROM \"Task\" WHERE \"field_101\" = $status",
                "context": {
                    "relation": "Task",
                    "columns": [
                        {"name": "field_101", "type": "string"}
                    ],
                    "variables": [
                        {"name": "status", "type": "string"}
                    ],
                    "stable_key": []
                },
                "operation": {
                    "kind": "set_filter_expression"
                }
            }
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).expect("JSON response");

        assert_eq!(
            response["ok"], false,
            "missing expression must not clear WHERE"
        );
        assert_eq!(response["error"]["kind"], "domain_validation");
        assert!(response["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("expression")));
    }

    #[test]
    fn sql_structure_protocol_proposes_a_typed_filter_parameter() {
        let request = serde_json::json!({
            "action": "domain.propose_sql_structure_operation",
            "value": {
                "sql": "SELECT * FROM \"Task\"",
                "context": {
                    "relation": "Task",
                    "columns": [{"name": "field_101", "type": "string"}],
                    "variables": [{"name": "status", "type": "string"}],
                    "stable_key": []
                },
                "operation": {
                    "kind": "set_filters",
                    "filters": [{
                        "column": "field_101",
                        "qualifier": null,
                        "operator": "eq",
                        "parameter": "status"
                    }]
                }
            }
        });
        let proposal: Value = serde_json::from_str(&super::invoke_json(&request.to_string()))
            .expect("Filter proposal JSON");
        assert_eq!(proposal["ok"], true, "{proposal}");
        assert_eq!(proposal["value"]["status"], "supported");
        assert_eq!(
            proposal["value"]["proposed_sql"],
            "SELECT * FROM \"Task\" WHERE \"field_101\" = $status"
        );
    }

    #[test]
    fn sql_structure_protocol_proposes_a_typed_column_comparison() {
        let request = serde_json::json!({
            "action": "domain.propose_sql_structure_operation",
            "value": {
                "sql": "SELECT * FROM \"Task\"",
                "context": {
                    "relation": "Task",
                    "columns": [
                        {"name": "field_101", "type": "string"},
                        {"name": "_ugoite_id", "type": "string"}
                    ],
                    "variables": [],
                    "stable_key": []
                },
                "operation": {
                    "kind": "set_filter_expression",
                    "expression": {
                        "kind": "column_comparison",
                        "left": {"column": "field_101"},
                        "operator": "eq",
                        "right": {"column": "_ugoite_id"}
                    }
                }
            }
        });
        let proposal: Value = serde_json::from_str(&super::invoke_json(&request.to_string()))
            .expect("column comparison proposal JSON");
        assert_eq!(proposal["ok"], true, "{proposal}");
        assert_eq!(proposal["value"]["status"], "supported");
        assert!(proposal["value"]["proposed_sql"]
            .as_str()
            .is_some_and(|sql| sql.contains("\"field_101\" = \"_ugoite_id\"")));
    }

    #[test]
    fn sql_structure_protocol_proposes_two_typed_between_bounds() {
        let request = serde_json::json!({
            "action": "domain.propose_sql_structure_operation",
            "value": {
                "sql": "SELECT * FROM \"Task\"",
                "context": {
                    "relation": "Task",
                    "columns": [{"name": "field_102", "type": "integer"}],
                    "variables": [
                        {"name": "minimum", "type": "integer"},
                        {"name": "maximum", "type": "integer"}
                    ],
                    "stable_key": []
                },
                "operation": {
                    "kind": "set_filter_expression",
                    "expression": {
                        "kind": "predicate",
                        "column": "field_102",
                        "operator": "between",
                        "parameter": "minimum",
                        "second_parameter": "maximum"
                    }
                }
            }
        });
        let proposal: Value = serde_json::from_str(&super::invoke_json(&request.to_string()))
            .expect("BETWEEN proposal JSON");
        assert_eq!(proposal["ok"], true, "{proposal}");
        assert_eq!(proposal["value"]["status"], "supported");
        assert_eq!(
            proposal["value"]["proposed_sql"],
            "SELECT * FROM \"Task\" WHERE \"field_102\" BETWEEN $minimum AND $maximum"
        );
    }

    #[test]
    fn sql_structure_projection_aliases_match_domain_analysis_and_proposals() {
        let sql = "SELECT \"field_102\" AS \"Total Amount\" FROM \"Task\"";
        let context = ugoite_domain::sql_structure::SqlStructureContext {
            relation: "Task".to_owned(),
            columns: vec![ugoite_domain::sql_structure::SqlStructureColumn {
                name: "field_102".to_owned(),
                column_type: ugoite_domain::sql_structure::SqlStructureColumnType::Integer,
            }],
            variables: vec![],
            stable_key: vec![],
        };
        let expected_analysis = serde_json::to_value(
            ugoite_domain::sql_structure::analyze_sql_structure(sql, &context),
        )
        .expect("Domain analysis serializes");
        let analysis_request = serde_json::json!({
            "action": "domain.analyze_sql_structure",
            "value": {"sql": sql, "context": context.clone()},
        })
        .to_string();
        let analysis_response: Value =
            serde_json::from_str(&super::invoke_json(&analysis_request)).unwrap();
        assert_eq!(analysis_response["ok"], true, "{analysis_response}");
        assert_eq!(analysis_response["value"], expected_analysis);

        let operation = ugoite_domain::sql_structure::SqlStructureOperation::SetColumns {
            columns: vec!["field_102".to_owned()],
            aliases: vec![Some(ugoite_domain::sql_structure::SqlStructureAlias {
                name: "Net Total".to_owned(),
                quoted: true,
            })],
        };
        let expected_proposal = serde_json::to_value(
            ugoite_domain::sql_structure::propose_sql_structure_operation(
                sql,
                &context,
                operation.clone(),
            ),
        )
        .expect("Domain proposal serializes");
        let proposal_request = serde_json::json!({
            "action": "domain.propose_sql_structure_operation",
            "value": {"sql": sql, "context": context, "operation": operation},
        })
        .to_string();
        let proposal_response: Value =
            serde_json::from_str(&super::invoke_json(&proposal_request)).unwrap();
        assert_eq!(proposal_response["ok"], true, "{proposal_response}");
        assert_eq!(proposal_response["value"], expected_proposal);
        assert_eq!(
            proposal_response["value"]["proposed_sql"],
            "SELECT \"field_102\" AS \"Net Total\" FROM \"Task\""
        );
    }

    #[test]
    fn sql_structure_projection_alias_sort_binding_rejections_match_domain() {
        let sql = "SELECT \"field_102\" AS \"field_101\" FROM \"Task\" ORDER BY \"field_101\"";
        let context = ugoite_domain::sql_structure::SqlStructureContext {
            relation: "Task".to_owned(),
            columns: vec![
                ugoite_domain::sql_structure::SqlStructureColumn {
                    name: "field_101".to_owned(),
                    column_type: ugoite_domain::sql_structure::SqlStructureColumnType::String,
                },
                ugoite_domain::sql_structure::SqlStructureColumn {
                    name: "field_102".to_owned(),
                    column_type: ugoite_domain::sql_structure::SqlStructureColumnType::Integer,
                },
            ],
            variables: vec![],
            stable_key: vec![],
        };
        let operation = ugoite_domain::sql_structure::SqlStructureOperation::SetColumns {
            columns: vec!["field_102".to_owned()],
            aliases: vec![Some(ugoite_domain::sql_structure::SqlStructureAlias {
                name: "total".to_owned(),
                quoted: false,
            })],
        };
        let expected = serde_json::to_value(
            ugoite_domain::sql_structure::propose_sql_structure_operation(
                sql,
                &context,
                operation.clone(),
            ),
        )
        .expect("Domain proposal serializes");
        assert_eq!(expected["status"], "unsupported");
        assert_eq!(expected["reason"], "sort_unsupported");
        assert_eq!(expected["proposed_sql"], Value::Null);

        let request = serde_json::json!({
            "action": "domain.propose_sql_structure_operation",
            "value": {"sql": sql, "context": context, "operation": operation},
        })
        .to_string();
        let response: Value = serde_json::from_str(&super::invoke_json(&request)).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], expected);
    }

    #[test]
    fn sql_structure_projection_slots_are_proposed_through_the_wasm_protocol() {
        let sql = "SELECT \"field_102\" AS first, \"field_102\" AS second FROM \"Task\"";
        let context = ugoite_domain::sql_structure::SqlStructureContext {
            relation: "Task".to_owned(),
            columns: vec![ugoite_domain::sql_structure::SqlStructureColumn {
                name: "field_102".to_owned(),
                column_type: ugoite_domain::sql_structure::SqlStructureColumnType::Integer,
            }],
            variables: vec![],
            stable_key: vec![],
        };
        let analysis_request = serde_json::json!({
            "action": "domain.analyze_sql_structure",
            "value": {"sql": sql, "context": context.clone()},
        })
        .to_string();
        let analysis: Value = serde_json::from_str(&super::invoke_json(&analysis_request)).unwrap();
        assert_eq!(analysis["ok"], true, "{analysis}");
        assert_eq!(analysis["value"]["contract_version"], 7);
        assert_eq!(
            analysis["value"]["projection_aliases"]
                .as_array()
                .unwrap()
                .len(),
            2
        );

        let operation = ugoite_domain::sql_structure::SqlStructureOperation::SetProjectionSlots {
            slots: vec![
                ugoite_domain::sql_structure::SqlStructureProjectionSlotEdit {
                    id: Some("projection-1".to_owned()),
                    column: "field_102".to_owned(),
                    alias: Some(ugoite_domain::sql_structure::SqlStructureAlias {
                        name: "second_copy".to_owned(),
                        quoted: false,
                    }),
                },
                ugoite_domain::sql_structure::SqlStructureProjectionSlotEdit {
                    id: Some("projection-0".to_owned()),
                    column: "field_102".to_owned(),
                    alias: Some(ugoite_domain::sql_structure::SqlStructureAlias {
                        name: "first_copy".to_owned(),
                        quoted: false,
                    }),
                },
            ],
        };
        let expected = serde_json::to_value(
            ugoite_domain::sql_structure::propose_sql_structure_operation(
                sql,
                &context,
                operation.clone(),
            ),
        )
        .expect("Domain proposal serializes");
        let request = serde_json::json!({
            "action": "domain.propose_sql_structure_operation",
            "value": {"sql": sql, "context": context, "operation": operation},
        })
        .to_string();
        let proposal: Value = serde_json::from_str(&super::invoke_json(&request)).unwrap();
        assert_eq!(proposal["ok"], true, "{proposal}");
        assert_eq!(proposal["value"], expected);
        assert!(proposal["value"]["proposed_sql"]
            .as_str()
            .unwrap()
            .contains("AS second_copy, \"field_102\" AS first_copy"));
    }

    #[test]
    fn sql_document_protocol_accepts_maximum_source_with_json_escaped_trivia() {
        let sql = format!(
            "SELECT 1 /*{}*/",
            "\n".repeat(ugoite_domain::sql_document::MAX_SQL_DOCUMENT_BYTES - 13)
        );
        assert_eq!(
            sql.len(),
            ugoite_domain::sql_document::MAX_SQL_DOCUMENT_BYTES
        );
        let request = serde_json::json!({
            "action": "domain.parse_sql_document",
            "value": {"sql": sql},
        })
        .to_string();
        assert!(request.len() > super::MAX_PROTOCOL_REQUEST_BYTES);
        assert!(request.len() <= super::MAX_SQL_DOCUMENT_REQUEST_BYTES);

        let response: Value = serde_json::from_str(&super::invoke_json(&request)).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"]["status"], "parsed");
        assert_eq!(response["value"]["source"], sql);
    }

    #[test]
    fn sql_document_parameter_rename_accepts_maximum_escaped_source() {
        let sql = format!(
            "SELECT 1 /*{}*/",
            "\n".repeat(ugoite_domain::sql_document::MAX_SQL_DOCUMENT_BYTES - 13)
        );
        let request = serde_json::json!({
            "action": "domain.rename_sql_document_parameter",
            "value": {"sql": sql, "old_name": "old", "new_name": "new"},
        })
        .to_string();
        assert!(request.len() > super::MAX_PROTOCOL_REQUEST_BYTES);
        assert!(request.len() <= super::MAX_SQL_DOCUMENT_RENAME_REQUEST_BYTES);

        let response: Value = serde_json::from_str(&super::invoke_json(&request)).unwrap();
        let native =
            ugoite_domain::sql_document::rename_sql_document_parameter(&sql, "old", "new").unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(native).unwrap());
    }

    #[test]
    fn sql_document_parameter_rename_rejects_request_over_limit() {
        let request = serde_json::json!({
            "action": "domain.rename_sql_document_parameter",
            "value": {"sql": "SELECT 1", "old_name": "old", "new_name": "new"},
            "padding": "x".repeat(super::MAX_SQL_DOCUMENT_RENAME_REQUEST_BYTES),
        })
        .to_string();
        assert!(request.len() > super::MAX_SQL_DOCUMENT_RENAME_REQUEST_BYTES);

        let response: Value = serde_json::from_str(&super::invoke_json(&request)).unwrap();
        assert_eq!(response["ok"], false, "{response}");
        assert_eq!(response["error"]["kind"], "input_too_large");
    }

    #[test]
    fn sql_document_protocol_bounds_amplified_parser_output() {
        let sql = format!("SELECT {}", vec!["1"; 20_000].join(","));
        let request = serde_json::json!({
            "action": "domain.parse_sql_document",
            "value": {"sql": sql},
        })
        .to_string();
        let response_text = super::invoke_json(&request);
        assert!(response_text.len() <= super::MAX_SQL_DOCUMENT_RESPONSE_BYTES);

        let response: Value = serde_json::from_str(&response_text).unwrap();
        assert_eq!(response["ok"], false, "{response}");
        assert_eq!(response["error"]["kind"], "domain_output_too_large");
    }

    #[test]
    fn sql_document_parameter_rename_bounds_amplified_output() {
        let sql = format!("SELECT {}", vec!["$x"; 20_000].join(","));
        let request = serde_json::json!({
            "action": "domain.rename_sql_document_parameter",
            "value": {"sql": sql, "old_name": "x", "new_name": "renamed"},
        })
        .to_string();
        let response_text = super::invoke_json(&request);
        assert!(response_text.len() <= super::MAX_SQL_DOCUMENT_RESPONSE_BYTES);

        let response: Value = serde_json::from_str(&response_text).unwrap();
        assert_eq!(response["ok"], false, "{response}");
        assert_eq!(response["error"]["kind"], "domain_output_too_large");
    }

    #[test]
    fn sql_document_edit_protocol_matches_native_transaction_and_undo() {
        let source = "-- 日本語\nSELECT 1 FROM \"G\" WHERE n = 2";
        let first = source.find('1').unwrap();
        let second = source.find('2').unwrap();
        let transaction = ugoite_domain::sql_document::SqlDocumentEditTransaction {
            expected_source: source.to_string(),
            // Reverse source order to prove ranges are simultaneous, not sequential.
            edits: vec![
                ugoite_domain::sql_document::SqlDocumentEdit {
                    range: ugoite_domain::sql_document::SqlSourceRange {
                        start: second,
                        end: second + 1,
                    },
                    expected_text: "2".to_string(),
                    replacement: "3".to_string(),
                },
                ugoite_domain::sql_document::SqlDocumentEdit {
                    range: ugoite_domain::sql_document::SqlSourceRange {
                        start: first,
                        end: first + 1,
                    },
                    expected_text: "1".to_string(),
                    replacement: "10".to_string(),
                },
            ],
        };
        let native =
            ugoite_domain::sql_document::apply_sql_document_edit_transaction(source, &transaction)
                .unwrap();
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "-- 日本語\nSELECT 10 FROM \"G\" WHERE n = 3"
        );

        let undo_request = serde_json::json!({
            "action": "domain.apply_sql_document_edit_transaction",
            "value": {
                "current_source": native.document.source,
                "transaction": native.undo,
            },
        });
        let undo_response: Value =
            serde_json::from_str(&super::invoke_json(&undo_request.to_string())).unwrap();
        assert_eq!(undo_response["ok"], true, "{undo_response}");
        assert_eq!(undo_response["value"]["document"]["source"], source);
    }

    #[test]
    fn sql_document_node_edit_protocol_matches_native_transaction_and_undo() {
        use ugoite_domain::sql_document::{
            SqlDocumentNodeEdit, SqlDocumentNodeEditTransaction, SqlDocumentNodeKind,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "-- report\nSELECT /* left */ amount AS total FROM \"G\" WHERE amount > 1 ORDER BY total DESC /* tail */";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let predicate = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::BinaryExpression)
            .expect("the WHERE comparison has a typed node");
        let transaction = SqlDocumentNodeEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentNodeEdit {
                node_id: predicate.id,
                expected_kind: SqlDocumentNodeKind::BinaryExpression,
                replacement: "amount >= 2".to_owned(),
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_node_edit_transaction(
            source,
            &transaction,
        )
        .unwrap();
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_node_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "-- report\nSELECT /* left */ amount AS total FROM \"G\" WHERE amount >= 2 ORDER BY total DESC /* tail */"
        );

        let undo_request = serde_json::json!({
            "action": "domain.apply_sql_document_edit_transaction",
            "value": {
                "current_source": native.document.source,
                "transaction": native.undo,
            },
        });
        let undo_response: Value =
            serde_json::from_str(&super::invoke_json(&undo_request.to_string())).unwrap();
        assert_eq!(undo_response["ok"], true, "{undo_response}");
        assert_eq!(undo_response["value"]["document"]["source"], source);
    }

    #[test]
    fn sql_document_semantic_edit_protocol_matches_native_and_undo() {
        use ugoite_domain::sql_document::{
            SqlDocumentIdentifier, SqlDocumentNodeKind, SqlDocumentSemanticEdit,
            SqlDocumentSemanticEditTransaction, SqlDocumentSemanticLiteral,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "-- 日本語\nSELECT amount FROM \"G\" WHERE amount > 1";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let identifier = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::IdentifierExpression
                    && matches!(
                        &node.semantic,
                        Some(ugoite_domain::sql_document::SqlDocumentNodeData::Identifier {
                            value,
                            ..
                        }) if value == "amount"
                    )
            })
            .expect("identifier leaf is present");
        let literal = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::Value
                    && matches!(
                        &node.semantic,
                        Some(ugoite_domain::sql_document::SqlDocumentNodeData::Literal {
                            raw: Some(value),
                            ..
                        }) if value == "1"
                    )
            })
            .expect("literal leaf is present");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![
                SqlDocumentSemanticEdit::ReplaceIdentifier {
                    node_id: identifier.id,
                    expected_kind: SqlDocumentNodeKind::IdentifierExpression,
                    parts: vec![SqlDocumentIdentifier {
                        value: "total".to_owned(),
                        quote_style: Some('"'),
                    }],
                },
                SqlDocumentSemanticEdit::ReplaceLiteral {
                    node_id: literal.id,
                    expected_kind: SqlDocumentNodeKind::Value,
                    literal: SqlDocumentSemanticLiteral::Number {
                        value: "9007199254740993".to_owned(),
                    },
                },
            ],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .unwrap();
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "-- 日本語\nSELECT \"total\" FROM \"G\" WHERE amount > 9007199254740993"
        );

        let undo_request = serde_json::json!({
            "action": "domain.apply_sql_document_edit_transaction",
            "value": {
                "current_source": native.document.source,
                "transaction": native.undo,
            },
        });
        let undo_response: Value =
            serde_json::from_str(&super::invoke_json(&undo_request.to_string())).unwrap();
        assert_eq!(undo_response["ok"], true, "{undo_response}");
        assert_eq!(undo_response["value"]["document"]["source"], source);
    }

    #[test]
    fn sql_document_filter_condition_protocol_matches_native_view_edit_and_undo() {
        use ugoite_domain::sql_document::{
            SqlDocumentColumnReference, SqlDocumentConditionGroupOperator,
            SqlDocumentFilterCondition, SqlDocumentFilterOperator, SqlDocumentIdentifier,
            SqlDocumentNodeKind, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
            SqlDocumentSemanticLiteral, SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT * FROM \"G\" WHERE age >= 18 AND name IS NOT NULL";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        assert!(matches!(
            document.conditions[0].expression,
            Some(ugoite_domain::sql_document::SqlDocumentConditionNode::Group { .. })
        ));
        let select = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Select)
            .unwrap();
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::InsertFilterCondition {
                node_id: select.id,
                expected_kind: SqlDocumentNodeKind::Select,
                condition: SqlDocumentFilterCondition::Group {
                    operator: SqlDocumentConditionGroupOperator::Or,
                    children: vec![
                        SqlDocumentFilterCondition::Comparison {
                            column: SqlDocumentColumnReference {
                                qualifier: None,
                                column: SqlDocumentIdentifier {
                                    value: "score".into(),
                                    quote_style: None,
                                },
                            },
                            operator: SqlDocumentFilterOperator::Gte,
                            value: Some(SqlDocumentSemanticLiteral::Number { value: "10".into() }),
                        },
                        SqlDocumentFilterCondition::Comparison {
                            column: SqlDocumentColumnReference {
                                qualifier: None,
                                column: SqlDocumentIdentifier {
                                    value: "active".into(),
                                    quote_style: None,
                                },
                            },
                            operator: SqlDocumentFilterOperator::Eq,
                            value: Some(SqlDocumentSemanticLiteral::Boolean { value: true }),
                        },
                    ],
                },
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .unwrap();
        let request = serde_json::json!({ "action": "domain.apply_sql_document_semantic_edit_transaction", "value": { "current_source": source, "transaction": transaction } });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert!(native
            .document
            .source
            .contains("age >= 18 AND name IS NOT NULL"));
        assert!(native
            .document
            .source
            .contains("AND ((score >= 10 OR active = TRUE))"));
        let undo_request = serde_json::json!({ "action": "domain.apply_sql_document_edit_transaction", "value": { "current_source": native.document.source, "transaction": native.undo } });
        let undo_response: Value =
            serde_json::from_str(&super::invoke_json(&undo_request.to_string())).unwrap();
        assert_eq!(undo_response["ok"], true, "{undo_response}");
        assert_eq!(undo_response["value"]["document"]["source"], source);

        let projection_source = "SELECT amount + 1 FROM \"G\" WHERE active = TRUE";
        let projection_document =
            ugoite_domain::sql_document::parse_sql_document(projection_source);
        let projection_expression = projection_document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::BinaryExpression
                    && projection_document.source_slice(node.range.unwrap()) == Some("amount + 1")
            })
            .unwrap();
        let invalid_transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: projection_source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::UpdateFilterCondition {
                node_id: projection_expression.id,
                expected_kind: SqlDocumentNodeKind::BinaryExpression,
                condition: SqlDocumentFilterCondition::Comparison {
                    column: SqlDocumentColumnReference {
                        qualifier: None,
                        column: SqlDocumentIdentifier {
                            value: "amount".into(),
                            quote_style: None,
                        },
                    },
                    operator: SqlDocumentFilterOperator::Gte,
                    value: Some(SqlDocumentSemanticLiteral::Number { value: "10".into() }),
                },
            }],
        };
        assert_eq!(
            ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
                projection_source,
                &invalid_transaction
            ),
            Err(ugoite_domain::sql_document::SqlDocumentEditError::InvalidSemanticReplacement)
        );
        let invalid_request = serde_json::json!({ "action": "domain.apply_sql_document_semantic_edit_transaction", "value": { "current_source": projection_source, "transaction": invalid_transaction } });
        let invalid_response: Value =
            serde_json::from_str(&super::invoke_json(&invalid_request.to_string())).unwrap();
        assert_eq!(invalid_response["ok"], false);
        assert_eq!(
            invalid_response["error"]["code"],
            "invalid_semantic_replacement"
        );
    }

    #[test]
    fn sql_document_signed_numeric_filter_literals_match_native_and_preserve_unknown_forms() {
        use ugoite_domain::sql_document::{
            SqlDocumentColumnReference, SqlDocumentFilterCondition, SqlDocumentFilterOperator,
            SqlDocumentIdentifier, SqlDocumentNodeKind, SqlDocumentSemanticEdit,
            SqlDocumentSemanticEditTransaction, SqlDocumentSemanticLiteral,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT * FROM G WHERE amount >= -92233720368547758081234567890";
        let parsed = ugoite_domain::sql_document::parse_sql_document(source);
        let parse_request = serde_json::json!({
            "action": "domain.parse_sql_document",
            "value": { "sql": source },
        });
        let parse_response: Value =
            serde_json::from_str(&super::invoke_json(&parse_request.to_string())).unwrap();
        assert_eq!(parse_response["ok"], true, "{parse_response}");
        assert_eq!(
            parse_response["value"],
            serde_json::to_value(&parsed).unwrap()
        );
        assert!(format!("{parsed:?}").contains("-92233720368547758081234567890"));

        let unsupported = "SELECT * FROM G WHERE amount = 0xFF";
        let unsupported_request = serde_json::json!({
            "action": "domain.parse_sql_document",
            "value": { "sql": unsupported },
        });
        let unsupported_response: Value =
            serde_json::from_str(&super::invoke_json(&unsupported_request.to_string())).unwrap();
        assert_eq!(unsupported_response["ok"], true, "{unsupported_response}");
        assert_eq!(
            unsupported_response["value"]["conditions"][0]["expression"]["kind"],
            "advanced"
        );

        let base = "SELECT * FROM G WHERE active = TRUE";
        let base_doc = ugoite_domain::sql_document::parse_sql_document(base);
        let select = base_doc
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Select)
            .unwrap();
        let value = "-922337203685477580812345678901234567890";
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: base.into(),
            edits: vec![SqlDocumentSemanticEdit::InsertFilterCondition {
                node_id: select.id,
                expected_kind: SqlDocumentNodeKind::Select,
                condition: SqlDocumentFilterCondition::Comparison {
                    column: SqlDocumentColumnReference {
                        qualifier: None,
                        column: SqlDocumentIdentifier {
                            value: "amount".into(),
                            quote_style: None,
                        },
                    },
                    operator: SqlDocumentFilterOperator::Gte,
                    value: Some(SqlDocumentSemanticLiteral::Number {
                        value: value.into(),
                    }),
                },
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            base,
            &transaction,
        )
        .unwrap();
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": { "current_source": base, "transaction": transaction },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert!(native.document.source.contains(value));
        let undo = serde_json::json!({
            "action": "domain.apply_sql_document_edit_transaction",
            "value": { "current_source": native.document.source, "transaction": native.undo },
        });
        let undo_response: Value =
            serde_json::from_str(&super::invoke_json(&undo.to_string())).unwrap();
        assert_eq!(undo_response["ok"], true, "{undo_response}");
        assert_eq!(undo_response["value"]["document"]["source"], base);
    }

    #[test]
    fn projection_alias_semantic_edit_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentIdentifier, SqlDocumentNodeKind, SqlDocumentSemanticEdit,
            SqlDocumentSemanticEditTransaction, SQL_DOCUMENT_CONTRACT_VERSION,
        };
        let source = "SELECT amount AS TOTAL FROM \"G\" ORDER BY \"total\" DESC";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let item = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .unwrap();
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::SetProjectionAlias {
                node_id: item.id,
                expected_kind: SqlDocumentNodeKind::SelectItem,
                alias: Some(SqlDocumentIdentifier {
                    value: "gross total".into(),
                    quote_style: Some('"'),
                }),
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .unwrap();
        let request = serde_json::json!({"action":"domain.apply_sql_document_semantic_edit_transaction","value":{"current_source":source,"transaction":transaction}});
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT amount AS \"gross total\" FROM \"G\" ORDER BY \"gross total\" DESC"
        );
    }

    #[test]
    fn projection_alias_order_expression_protocol_matches_native_and_undo() {
        use ugoite_domain::sql_document::{
            SqlDocumentIdentifier, SqlDocumentNodeKind, SqlDocumentSemanticEdit,
            SqlDocumentSemanticEditTransaction, SQL_DOCUMENT_CONTRACT_VERSION,
        };
        let source = "SELECT amount AS total FROM \"G\" ORDER BY total + 1";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let item = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .unwrap();
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::SetProjectionAlias {
                node_id: item.id,
                expected_kind: SqlDocumentNodeKind::SelectItem,
                alias: Some(SqlDocumentIdentifier {
                    value: "renamed".into(),
                    quote_style: Some('"'),
                }),
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .unwrap();
        let request = serde_json::json!({"action":"domain.apply_sql_document_semantic_edit_transaction","value":{"current_source":source,"transaction":transaction}});
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT amount AS \"renamed\" FROM \"G\" ORDER BY \"renamed\" + 1"
        );

        let undo = serde_json::json!({
            "action": "domain.apply_sql_document_edit_transaction",
            "value": { "current_source": native.document.source, "transaction": native.undo },
        });
        let undo_response: Value =
            serde_json::from_str(&super::invoke_json(&undo.to_string())).unwrap();
        assert_eq!(undo_response["ok"], true, "{undo_response}");
        assert_eq!(undo_response["value"]["document"]["source"], source);
    }

    #[test]
    fn sql_document_relation_edit_protocol_matches_native_and_undo() {
        use ugoite_domain::sql_document::{
            SqlDocumentIdentifier, SqlDocumentJoin, SqlDocumentJoinConstraint,
            SqlDocumentJoinOperator, SqlDocumentNodeKind, SqlDocumentRelationFactor,
            SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT (SELECT 1) FROM \"G\"";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let relation_tree = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::RelationTree)
            .expect("the FROM tree is present");
        let form_factor = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::TableFactor
                    && node.range.and_then(|range| document.source_slice(range)) == Some("\"G\"")
            })
            .expect("the selected Form factor is present");
        let derived_query = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::Query
                    && node.range.and_then(|range| document.source_slice(range)) == Some("SELECT 1")
            })
            .expect("the derived relation query is parser-backed");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![
                SqlDocumentSemanticEdit::ReplaceTableFactor {
                    node_id: form_factor.id,
                    expected_kind: SqlDocumentNodeKind::TableFactor,
                    factor: SqlDocumentRelationFactor::Derived {
                        query_node_id: derived_query.id,
                        alias: SqlDocumentIdentifier {
                            value: "d".to_owned(),
                            quote_style: None,
                        },
                    },
                },
                SqlDocumentSemanticEdit::AddJoin {
                    node_id: relation_tree.id,
                    expected_kind: SqlDocumentNodeKind::RelationTree,
                    join: SqlDocumentJoin {
                        operator: SqlDocumentJoinOperator::Left,
                        relation: SqlDocumentRelationFactor::Table {
                            name: vec![SqlDocumentIdentifier {
                                value: "H".to_owned(),
                                quote_style: Some('"'),
                            }],
                            alias: None,
                        },
                        constraint: SqlDocumentJoinConstraint::Using {
                            columns: vec![SqlDocumentIdentifier {
                                value: "id".to_owned(),
                                quote_style: Some('"'),
                            }],
                        },
                    },
                },
            ],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("typed relation edits apply atomically");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT (SELECT 1) FROM (SELECT 1) AS d LEFT JOIN \"H\" USING (\"id\")"
        );

        let undo_request = serde_json::json!({
            "action": "domain.apply_sql_document_edit_transaction",
            "value": {
                "current_source": native.document.source,
                "transaction": native.undo,
            },
        });
        let undo_response: Value =
            serde_json::from_str(&super::invoke_json(&undo_request.to_string())).unwrap();
        assert_eq!(undo_response["ok"], true, "{undo_response}");
        assert_eq!(undo_response["value"]["document"]["source"], source);
    }

    #[test]
    fn sql_document_relational_constructors_protocol_match_native_and_undo() {
        use ugoite_domain::sql_document::{
            SqlDocumentIdentifier, SqlDocumentJoin, SqlDocumentJoinConstraint,
            SqlDocumentJoinOperator, SqlDocumentNodeKind, SqlDocumentQueryLimit,
            SqlDocumentRelationFactor, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };

        fn assert_protocol(source: &str, transaction: SqlDocumentSemanticEditTransaction) {
            let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
                source,
                &transaction,
            )
            .expect("the typed relational edit applies natively");
            let request = serde_json::json!({
                "action": "domain.apply_sql_document_semantic_edit_transaction",
                "value": { "current_source": source, "transaction": transaction },
            });
            let response: Value =
                serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
            assert_eq!(response["ok"], true, "{response}");
            assert_eq!(response["value"], serde_json::to_value(&native).unwrap());

            let undo_request = serde_json::json!({
                "action": "domain.apply_sql_document_edit_transaction",
                "value": { "current_source": native.document.source, "transaction": native.undo },
            });
            let undo_response: Value =
                serde_json::from_str(&super::invoke_json(&undo_request.to_string())).unwrap();
            assert_eq!(undo_response["ok"], true, "{undo_response}");
            assert_eq!(undo_response["value"]["document"]["source"], source);
        }

        let source = "SELECT 1 + 2";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let query = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Query)
            .expect("the query is parser-backed");
        let expression = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::BinaryExpression)
            .expect("the LIMIT expression source is typed");
        assert_protocol(
            source,
            SqlDocumentSemanticEditTransaction {
                contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                expected_source: source.to_owned(),
                edits: vec![SqlDocumentSemanticEdit::SetLimitExpression {
                    node_id: query.id,
                    expected_kind: query.kind,
                    value: SqlDocumentQueryLimit::Expression {
                        expression_node_id: expression.id,
                        expression_expected_kind: expression.kind,
                    },
                }],
            },
        );

        let source = "WITH c (old_name) AS (SELECT 1) SELECT c.old_name FROM c";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let cte = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::CommonTableExpression)
            .expect("the CTE is parser-backed");
        assert_protocol(
            source,
            SqlDocumentSemanticEditTransaction {
                contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                expected_source: source.to_owned(),
                edits: vec![SqlDocumentSemanticEdit::SetRelationColumnAliases {
                    node_id: cte.id,
                    expected_kind: cte.kind,
                    aliases: vec![SqlDocumentIdentifier {
                        value: "new_name".to_owned(),
                        quote_style: None,
                    }],
                }],
            },
        );

        let source = "SELECT 1 FROM \"G\" WHERE 1 = 1";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let tree = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::RelationTree)
            .expect("the relation tree is parser-backed");
        let condition = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::BinaryExpression)
            .expect("the ON expression source is typed");
        assert_protocol(
            source,
            SqlDocumentSemanticEditTransaction {
                contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                expected_source: source.to_owned(),
                edits: vec![SqlDocumentSemanticEdit::AddJoin {
                    node_id: tree.id,
                    expected_kind: tree.kind,
                    join: SqlDocumentJoin {
                        operator: SqlDocumentJoinOperator::LeftSemi,
                        relation: SqlDocumentRelationFactor::Table {
                            name: vec![SqlDocumentIdentifier {
                                value: "H".to_owned(),
                                quote_style: None,
                            }],
                            alias: None,
                        },
                        constraint: SqlDocumentJoinConstraint::On {
                            expression_node_id: condition.id,
                            expression_expected_kind: condition.kind,
                        },
                    },
                }],
            },
        );
    }

    #[test]
    fn sql_document_cast_target_semantic_edit_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentCastType, SqlDocumentNodeKind, SqlDocumentSemanticEdit,
            SqlDocumentSemanticEditTransaction, SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT TRY_CAST(amount + 1 AS BIGINT)";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let cast = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::CastExpression)
            .expect("the parser exposes the CAST expression node");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::ReplaceCastTargetType {
                node_id: cast.id,
                expected_kind: SqlDocumentNodeKind::CastExpression,
                target_type: SqlDocumentCastType::Double,
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("native typed CAST edit succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT TRY_CAST(amount + 1 AS DOUBLE)"
        );
    }

    #[test]
    fn sql_document_parameterized_cast_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentCastTarget, SqlDocumentCastType, SqlDocumentNodeKind,
            SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };
        let source = "SELECT CAST(1 AS DECIMAL)";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let cast = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::CastExpression)
            .unwrap();
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::SetParameterizedCastTarget {
                node_id: cast.id,
                expected_kind: SqlDocumentNodeKind::CastExpression,
                target: SqlDocumentCastTarget {
                    kind: SqlDocumentCastType::Decimal,
                    length: None,
                    precision: Some("9007199254740993".to_owned()),
                    scale: Some("-4".to_owned()),
                },
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .unwrap();
        let request = serde_json::json!({"action":"domain.apply_sql_document_semantic_edit_transaction","value":{"current_source":source,"transaction":transaction}});
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT CAST(1 AS DECIMAL(9007199254740993, -4))"
        );
    }

    #[test]
    fn sql_document_cte_rename_semantic_edit_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentIdentifier, SqlDocumentNodeKind, SqlDocumentSemanticEdit,
            SqlDocumentSemanticEditTransaction, SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "WITH c AS (SELECT 1 AS n) SELECT n FROM c";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let cte = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::CommonTableExpression)
            .expect("the parser exposes the CTE declaration node");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::RenameCommonTableExpression {
                node_id: cte.id,
                expected_kind: SqlDocumentNodeKind::CommonTableExpression,
                name: SqlDocumentIdentifier {
                    value: "rows".to_owned(),
                    quote_style: None,
                },
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("native typed CTE rename succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "WITH rows AS (SELECT 1 AS n) SELECT n FROM rows"
        );
    }

    #[test]
    fn sql_document_cte_add_and_remove_semantic_edit_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentIdentifier, SqlDocumentNodeKind, SqlDocumentSemanticEdit,
            SqlDocumentSemanticEditTransaction, SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT 1";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let query = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Query)
            .expect("the parser exposes the query node");
        let add_transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::AddCommonTableExpression {
                node_id: query.id,
                expected_kind: SqlDocumentNodeKind::Query,
                name: SqlDocumentIdentifier {
                    value: "rows".to_owned(),
                    quote_style: None,
                },
            }],
        };
        let added = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &add_transaction,
        )
        .expect("native typed CTE addition succeeds");
        let add_request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": add_transaction,
            },
        });
        let add_response: Value =
            serde_json::from_str(&super::invoke_json(&add_request.to_string())).unwrap();
        assert_eq!(add_response["ok"], true, "{add_response}");
        assert_eq!(add_response["value"], serde_json::to_value(&added).unwrap());
        assert_eq!(
            add_response["value"]["document"]["source"],
            "WITH rows AS (SELECT 1) SELECT 1"
        );

        let cte = added
            .document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::CommonTableExpression)
            .expect("the added CTE has a typed declaration");
        let remove_transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: added.document.source.clone(),
            edits: vec![SqlDocumentSemanticEdit::RemoveCommonTableExpression {
                node_id: cte.id,
                expected_kind: SqlDocumentNodeKind::CommonTableExpression,
            }],
        };
        let removed = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            &added.document.source,
            &remove_transaction,
        )
        .expect("native typed CTE removal succeeds");
        let remove_request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": added.document.source,
                "transaction": remove_transaction,
            },
        });
        let remove_response: Value =
            serde_json::from_str(&super::invoke_json(&remove_request.to_string())).unwrap();
        assert_eq!(remove_response["ok"], true, "{remove_response}");
        assert_eq!(
            remove_response["value"],
            serde_json::to_value(&removed).unwrap()
        );
        assert_eq!(remove_response["value"]["document"]["source"], "SELECT 1");
    }

    #[test]
    fn sql_document_predicate_negation_semantic_edit_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentNodeKind, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT amount FROM \"G\" WHERE amount IS /* keep */ NULL";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let predicate = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::PredicateExpression)
            .expect("the parser exposes the IS NULL predicate");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::ReplacePredicateNegation {
                node_id: predicate.id,
                expected_kind: SqlDocumentNodeKind::PredicateExpression,
                negated: true,
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("native typed predicate negation succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT amount FROM \"G\" WHERE amount IS NOT /* keep */ NULL"
        );
    }

    #[test]
    fn sql_document_unary_sign_semantic_edit_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentNodeKind, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
            SqlDocumentUnarySign, SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT - /* keep */ amount";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let unary = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::UnaryExpression
                    && matches!(
                        node.semantic,
                        Some(ugoite_domain::sql_document::SqlDocumentNodeData::UnaryOperator {
                            ref operator
                        }) if operator == "-"
                    )
            })
            .expect("the parser exposes the arithmetic unary expression");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::ReplaceUnarySign {
                node_id: unary.id,
                expected_kind: SqlDocumentNodeKind::UnaryExpression,
                sign: SqlDocumentUnarySign::Not,
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("native typed unary sign edit succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT NOT /* keep */ amount"
        );
    }

    #[test]
    fn sql_document_bitwise_operator_semantic_edit_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentBinaryOperator, SqlDocumentNodeKind, SqlDocumentSemanticEdit,
            SqlDocumentSemanticEditTransaction, SQL_DOCUMENT_CONTRACT_VERSION,
        };
        let source = "SELECT 5 + 1";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let binary = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::BinaryExpression)
            .unwrap();
        for (operator, expected) in [
            (SqlDocumentBinaryOperator::BitwiseAnd, "SELECT 5 & 1"),
            (SqlDocumentBinaryOperator::BitwiseOr, "SELECT 5 | 1"),
            (SqlDocumentBinaryOperator::ShiftLeft, "SELECT 5 << 1"),
            (SqlDocumentBinaryOperator::ShiftRight, "SELECT 5 >> 1"),
        ] {
            let transaction = SqlDocumentSemanticEditTransaction {
                contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                expected_source: source.to_owned(),
                edits: vec![SqlDocumentSemanticEdit::ReplaceBinaryOperator {
                    node_id: binary.id,
                    expected_kind: SqlDocumentNodeKind::BinaryExpression,
                    operator,
                }],
            };
            let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
                source,
                &transaction,
            )
            .unwrap();
            let request = serde_json::json!({"action":"domain.apply_sql_document_semantic_edit_transaction","value":{"current_source":source,"transaction":transaction}});
            let response: Value =
                serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
            assert_eq!(response["ok"], true, "{response}");
            assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
            assert_eq!(response["value"]["document"]["source"], expected);
        }
    }

    #[test]
    fn sql_document_set_operation_semantic_edit_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentNodeKind, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
            SqlDocumentSetOperator, SqlDocumentSetQuantifier, SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT 1 UNION ALL SELECT 2";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let node = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SetOperation)
            .expect("the parser exposes the set operation node");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::ReplaceSetOperation {
                node_id: node.id,
                expected_kind: SqlDocumentNodeKind::SetOperation,
                operator: SqlDocumentSetOperator::Intersect,
                quantifier: SqlDocumentSetQuantifier::Distinct,
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("native typed edit succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT 1 INTERSECT DISTINCT SELECT 2"
        );
        assert_eq!(
            response["value"]["undo"]["edits"][0]["replacement"],
            "UNION ALL"
        );
    }

    #[test]
    fn sql_document_set_operation_swap_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentNodeKind, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT 1 UNION ALL SELECT 2";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let node = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SetOperation)
            .expect("the parser exposes the set operation node");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::SwapSetOperationOperands {
                node_id: node.id,
                expected_kind: SqlDocumentNodeKind::SetOperation,
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("native typed operand swap succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT 2 UNION ALL SELECT 1"
        );
    }

    #[test]
    fn sql_document_set_operation_add_remove_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentNodeKind, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
            SqlDocumentSetOperand, SqlDocumentSetOperator, SqlDocumentSetQuantifier,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT 1";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let query = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Query)
            .expect("the parser exposes the query node");
        let add_transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::AddSetOperationOperand {
                node_id: query.id,
                expected_kind: SqlDocumentNodeKind::Query,
                operator: SqlDocumentSetOperator::Union,
                quantifier: SqlDocumentSetQuantifier::All,
            }],
        };
        let added = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &add_transaction,
        )
        .expect("native typed set-operation append succeeds");
        let add_request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": add_transaction,
            },
        });
        let add_response: Value =
            serde_json::from_str(&super::invoke_json(&add_request.to_string())).unwrap();
        assert_eq!(add_response["ok"], true, "{add_response}");
        assert_eq!(add_response["value"], serde_json::to_value(&added).unwrap());
        assert_eq!(
            add_response["value"]["document"]["source"],
            "(SELECT 1\n) UNION ALL SELECT 1"
        );

        let operation = added
            .document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SetOperation)
            .expect("the appended set operation is represented");
        let remove_transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: added.document.source.clone(),
            edits: vec![SqlDocumentSemanticEdit::RemoveSetOperationOperand {
                node_id: operation.id,
                expected_kind: SqlDocumentNodeKind::SetOperation,
                keep: SqlDocumentSetOperand::Left,
            }],
        };
        let removed = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            &added.document.source,
            &remove_transaction,
        )
        .expect("native typed set-operation removal succeeds");
        let remove_request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": added.document.source,
                "transaction": remove_transaction,
            },
        });
        let remove_response: Value =
            serde_json::from_str(&super::invoke_json(&remove_request.to_string())).unwrap();
        assert_eq!(remove_response["ok"], true, "{remove_response}");
        assert_eq!(
            remove_response["value"],
            serde_json::to_value(&removed).unwrap()
        );
        assert_eq!(remove_response["value"]["document"]["source"], "SELECT 1");
    }

    #[test]
    fn sql_document_nested_structure_edit_protocol_matches_native_and_undo() {
        use ugoite_domain::sql_document::{
            SqlDocumentIdentifier, SqlDocumentNodeKind, SqlDocumentRelationFactor,
            SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction, SqlDocumentSetOperator,
            SqlDocumentSetQuantifier, SqlDocumentSubqueryPredicateTemplate,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };

        fn assert_matches_native(
            source: &str,
            transaction: SqlDocumentSemanticEditTransaction,
            expected: &str,
        ) {
            let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
                source,
                &transaction,
            )
            .expect("the native typed structural edit succeeds");
            assert_eq!(native.document.source, expected);
            let request = serde_json::json!({
                "action": "domain.apply_sql_document_semantic_edit_transaction",
                "value": {
                    "current_source": source,
                    "transaction": transaction,
                },
            });
            let response: Value =
                serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
            assert_eq!(response["ok"], true, "{response}");
            assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
            let undo_request = serde_json::json!({
                "action": "domain.apply_sql_document_edit_transaction",
                "value": {
                    "current_source": native.document.source,
                    "transaction": native.undo,
                },
            });
            let undo_response: Value =
                serde_json::from_str(&super::invoke_json(&undo_request.to_string())).unwrap();
            assert_eq!(undo_response["ok"], true, "{undo_response}");
            assert_eq!(undo_response["value"]["document"]["source"], source);
        }

        let cte_source = "WITH c AS (SELECT 1 AS n), d AS (SELECT 2 AS n) SELECT n FROM c";
        let cte_document = ugoite_domain::sql_document::parse_sql_document(cte_source);
        let cte = cte_document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::CommonTableExpression
                    && matches!(node.semantic, Some(ugoite_domain::sql_document::SqlDocumentNodeData::CommonTableExpression { ref name, .. }) if name.value == "c")
            })
            .unwrap();
        let query = cte_document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::Query
                    && node.parent.is_some_and(|parent| {
                        cte_document.nodes.get(parent as usize).is_some_and(|cte| {
                            matches!(cte.semantic, Some(ugoite_domain::sql_document::SqlDocumentNodeData::CommonTableExpression { ref name, .. }) if name.value == "d")
                        })
                    })
            })
            .unwrap();
        assert_matches_native(
            cte_source,
            SqlDocumentSemanticEditTransaction {
                contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                expected_source: cte_source.to_owned(),
                edits: vec![SqlDocumentSemanticEdit::ReplaceCommonTableExpressionQuery {
                    node_id: cte.id,
                    expected_kind: SqlDocumentNodeKind::CommonTableExpression,
                    query_node_id: query.id,
                }],
            },
            "WITH c AS (SELECT 2 AS n), d AS (SELECT 2 AS n) SELECT n FROM c",
        );
        let cte_d = cte_document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::CommonTableExpression
                    && matches!(node.semantic, Some(ugoite_domain::sql_document::SqlDocumentNodeData::CommonTableExpression { ref name, .. }) if name.value == "d")
            })
            .unwrap();
        assert_matches_native(
            cte_source,
            SqlDocumentSemanticEditTransaction {
                contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                expected_source: cte_source.to_owned(),
                edits: vec![SqlDocumentSemanticEdit::MoveCommonTableExpression {
                    node_id: cte_d.id,
                    expected_kind: SqlDocumentNodeKind::CommonTableExpression,
                    to_index: 0,
                }],
            },
            "WITH d AS (SELECT 2 AS n), c AS (SELECT 1 AS n) SELECT n FROM c",
        );

        let set_source = "SELECT 1 AS v UNION ALL SELECT 2 AS v";
        let set_document = ugoite_domain::sql_document::parse_sql_document(set_source);
        let right_operand = set_document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SetOperationRight)
            .unwrap();
        let left_select = set_document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::Select
                    && node
                        .range
                        .and_then(|range| set_document.source_slice(range))
                        == Some("SELECT 1 AS v")
            })
            .unwrap();
        assert_matches_native(
            set_source,
            SqlDocumentSemanticEditTransaction {
                contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                expected_source: set_source.to_owned(),
                edits: vec![SqlDocumentSemanticEdit::ReplaceSetOperationOperand {
                    node_id: right_operand.id,
                    expected_kind: SqlDocumentNodeKind::SetOperationRight,
                    expression_node_id: left_select.id,
                    expression_expected_kind: SqlDocumentNodeKind::Select,
                }],
            },
            "SELECT 1 AS v UNION ALL (SELECT 1 AS v)",
        );

        let by_name_source = "SELECT 1 AS a UNION SELECT 2 AS a";
        let by_name_document = ugoite_domain::sql_document::parse_sql_document(by_name_source);
        let operation = by_name_document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SetOperation)
            .unwrap();
        assert_matches_native(
            by_name_source,
            SqlDocumentSemanticEditTransaction {
                contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                expected_source: by_name_source.to_owned(),
                edits: vec![SqlDocumentSemanticEdit::ReplaceSetOperation {
                    node_id: operation.id,
                    expected_kind: SqlDocumentNodeKind::SetOperation,
                    operator: SqlDocumentSetOperator::Union,
                    quantifier: SqlDocumentSetQuantifier::AllByName,
                }],
            },
            "SELECT 1 AS a UNION ALL BY NAME SELECT 2 AS a",
        );

        let nested_source = "SELECT 1";
        let nested_document = ugoite_domain::sql_document::parse_sql_document(nested_source);
        assert_matches_native(
            nested_source,
            SqlDocumentSemanticEditTransaction {
                contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                expected_source: nested_source.to_owned(),
                edits: vec![SqlDocumentSemanticEdit::AddScalarSubqueryProjection {
                    node_id: nested_document
                        .nodes
                        .iter()
                        .find(|node| node.kind == SqlDocumentNodeKind::SelectList)
                        .unwrap()
                        .id,
                    expected_kind: SqlDocumentNodeKind::SelectList,
                }],
            },
            "SELECT 1, (SELECT 1) AS __ugoite_scalar_subquery",
        );
        let with_scalar =
            ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
                nested_source,
                &SqlDocumentSemanticEditTransaction {
                    contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                    expected_source: nested_source.to_owned(),
                    edits: vec![SqlDocumentSemanticEdit::AddScalarSubqueryProjection {
                        node_id: nested_document
                            .nodes
                            .iter()
                            .find(|node| node.kind == SqlDocumentNodeKind::SelectList)
                            .unwrap()
                            .id,
                        expected_kind: SqlDocumentNodeKind::SelectList,
                    }],
                },
            )
            .unwrap();
        let nested_select = with_scalar
            .document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::Select
                    && node
                        .range
                        .and_then(|range| with_scalar.document.source_slice(range))
                        == Some("SELECT 1")
                    && node.parent.is_some_and(|parent| {
                        with_scalar
                            .document
                            .nodes
                            .get(parent as usize)
                            .is_some_and(|query| {
                                query.parent.is_some_and(|grandparent| {
                                    with_scalar
                                        .document
                                        .nodes
                                        .get(grandparent as usize)
                                        .is_some_and(|subquery| {
                                            subquery.kind == SqlDocumentNodeKind::SubqueryExpression
                                        })
                                })
                            })
                    })
            })
            .unwrap();
        assert_matches_native(
            &with_scalar.document.source,
            SqlDocumentSemanticEditTransaction {
                contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                expected_source: with_scalar.document.source.clone(),
                edits: vec![SqlDocumentSemanticEdit::AddFromRelation {
                    node_id: nested_select.id,
                    expected_kind: SqlDocumentNodeKind::Select,
                    factor: SqlDocumentRelationFactor::Table {
                        name: vec![SqlDocumentIdentifier {
                            value: "tasks".to_owned(),
                            quote_style: Some('"'),
                        }],
                        alias: None,
                    },
                }],
            },
            "SELECT 1, (SELECT 1 FROM \"tasks\") AS __ugoite_scalar_subquery",
        );

        let predicate_source = "SELECT 1";
        let predicate_document = ugoite_domain::sql_document::parse_sql_document(predicate_source);
        let operand = predicate_document
            .nodes
            .iter()
            .find(|node| {
                node.range
                    .and_then(|range| predicate_document.source_slice(range))
                    == Some("1")
                    && node.parent.is_some_and(|parent| {
                        predicate_document
                            .nodes
                            .get(parent as usize)
                            .is_some_and(|item| item.kind == SqlDocumentNodeKind::SelectItem)
                    })
            })
            .unwrap();
        assert_matches_native(
            predicate_source,
            SqlDocumentSemanticEditTransaction {
                contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                expected_source: predicate_source.to_owned(),
                edits: vec![SqlDocumentSemanticEdit::WrapExpressionInSubqueryPredicate {
                    node_id: operand.id,
                    expected_kind: operand.kind,
                    template: SqlDocumentSubqueryPredicateTemplate::NotIn,
                }],
            },
            "SELECT (1) NOT IN (SELECT 1)",
        );

        let exists_source = "SELECT 1 WHERE TRUE";
        let exists_document = ugoite_domain::sql_document::parse_sql_document(exists_source);
        let truth = exists_document
            .nodes
            .iter()
            .find(|node| {
                node.range
                    .and_then(|range| exists_document.source_slice(range))
                    == Some("TRUE")
            })
            .unwrap();
        assert_matches_native(
            exists_source,
            SqlDocumentSemanticEditTransaction {
                contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                expected_source: exists_source.to_owned(),
                edits: vec![
                    SqlDocumentSemanticEdit::ReplaceExpressionWithExistsSubquery {
                        node_id: truth.id,
                        expected_kind: truth.kind,
                    },
                ],
            },
            "SELECT 1 WHERE EXISTS (SELECT 1)",
        );
    }

    #[test]
    fn sql_document_case_structure_edit_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentNodeKind, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT CASE WHEN amount > 0 THEN 1 ELSE 0 END";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let node = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::CaseExpression)
            .expect("the parser exposes the CASE node");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::AddCaseBranch {
                node_id: node.id,
                expected_kind: SqlDocumentNodeKind::CaseExpression,
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("native typed edit succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT CASE WHEN amount > 0 THEN 1 WHEN TRUE THEN NULL ELSE 0 END"
        );
    }

    #[test]
    fn sql_document_case_operand_semantic_edit_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentNodeKind, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };
        let source = "SELECT amount, CASE WHEN amount = 1 THEN 2 END";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let case_node = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::CaseExpression)
            .unwrap();
        let expression = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::IdentifierExpression
                    && node.range.and_then(|range| document.source_slice(range)) == Some("amount")
                    && node
                        .range
                        .is_some_and(|range| range.start < case_node.range.unwrap().start)
            })
            .unwrap();
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::SetCaseOperand {
                node_id: case_node.id,
                expected_kind: SqlDocumentNodeKind::CaseExpression,
                expression_node_id: Some(expression.id),
                expression_expected_kind: Some(expression.kind),
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .unwrap();
        let request = serde_json::json!({"action":"domain.apply_sql_document_semantic_edit_transaction","value":{"current_source":source,"transaction":transaction}});
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT amount, CASE amount WHEN amount = 1 THEN 2 END"
        );
    }

    #[test]
    fn sql_document_function_argument_edit_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentNodeKind, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT COUNT(field_100) AS item_count, field_100 FROM \"G\"";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let function = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::FunctionExpression
                    && matches!(
                        &node.semantic,
                        Some(ugoite_domain::sql_document::SqlDocumentNodeData::Function {
                            name,
                            ..
                        }) if name.last().is_some_and(|part| part.value.eq_ignore_ascii_case("COUNT"))
                    )
            })
            .expect("the parser exposes COUNT");
        let argument = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::IdentifierExpression
                    && node.parent == Some(function.id)
            })
            .expect("COUNT exposes its typed argument");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::SetFunctionArgument {
                node_id: function.id,
                expected_kind: SqlDocumentNodeKind::FunctionExpression,
                argument_index: 1,
                expression_node_id: argument.id,
                expression_expected_kind: SqlDocumentNodeKind::IdentifierExpression,
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("native typed argument edit succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT COUNT(field_100, field_100) AS item_count, field_100 FROM \"G\""
        );
    }

    #[test]
    fn sql_document_projection_expression_edit_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentNodeKind, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT field_100, CASE WHEN field_100 = 'alpha' THEN field_100 ELSE 'other' END AS label FROM \"G\"";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let mut items = document
            .nodes
            .iter()
            .filter(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .collect::<Vec<_>>();
        items.sort_by_key(|node| node.range.map(|range| range.start));
        let expression = document
            .nodes
            .iter()
            .find(|node| {
                node.parent == Some(items[1].id) && node.kind == SqlDocumentNodeKind::CaseExpression
            })
            .expect("the parser exposes the selected CASE expression");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::SetProjectionExpression {
                node_id: items[0].id,
                expected_kind: SqlDocumentNodeKind::SelectItem,
                expression_node_id: expression.id,
                expression_expected_kind: SqlDocumentNodeKind::CaseExpression,
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("native projection expression edit succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT CASE WHEN field_100 = 'alpha' THEN field_100 ELSE 'other' END, CASE WHEN field_100 = 'alpha' THEN field_100 ELSE 'other' END AS label FROM \"G\""
        );
    }

    #[test]
    fn sql_document_common_toolbar_semantic_operations_have_wasm_parity() {
        use ugoite_domain::sql_document::{
            SqlDocumentAggregateFunction, SqlDocumentNodeData, SqlDocumentNodeKind,
            SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
            SqlDocumentSemanticLiteral, SqlDocumentValueAccess, SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let assert_parity = |source: &str, edit: SqlDocumentSemanticEdit| {
            let transaction = SqlDocumentSemanticEditTransaction {
                contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
                expected_source: source.to_owned(),
                edits: vec![edit],
            };
            let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
                source,
                &transaction,
            )
            .expect("native semantic edit succeeds");
            let request = serde_json::json!({
                "action": "domain.apply_sql_document_semantic_edit_transaction",
                "value": {
                    "current_source": source,
                    "transaction": &transaction,
                },
            });
            let response: Value =
                serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
            assert_eq!(response["ok"], true, "{response}");
            assert_eq!(response["value"], serde_json::to_value(&native).unwrap());

            let undo_request = serde_json::json!({
                "action": "domain.apply_sql_document_edit_transaction",
                "value": {
                    "current_source": native.document.source,
                    "transaction": &native.undo,
                },
            });
            let undo_response: Value =
                serde_json::from_str(&super::invoke_json(&undo_request.to_string())).unwrap();
            assert_eq!(undo_response["ok"], true, "{undo_response}");
            assert_eq!(undo_response["value"]["document"]["source"], source);
            native
        };

        let array_source = "SELECT 4 /* preserve */";
        let array_document = ugoite_domain::sql_document::parse_sql_document(array_source);
        let value = array_document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::Value
                    && node
                        .range
                        .and_then(|range| array_document.source_slice(range))
                        == Some("4")
            })
            .expect("the ARRAY element is a typed value");
        let array = assert_parity(
            array_source,
            SqlDocumentSemanticEdit::WrapExpressionInArray {
                node_id: value.id,
                expected_kind: value.kind,
            },
        );
        assert_eq!(array.document.source, "SELECT ARRAY[4] /* preserve */");
        let array_document =
            ugoite_domain::sql_document::parse_sql_document(&array.document.source);
        let array_expression = array_document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::SelectItem
                    && node
                        .range
                        .and_then(|range| array_document.source_slice(range))
                        == Some("ARRAY[4]")
            })
            .expect("the constructed ARRAY is selectable");
        let appended = assert_parity(
            &array.document.source,
            SqlDocumentSemanticEdit::AppendArrayLiteralElement {
                node_id: array_expression.id,
                expected_kind: array_expression.kind,
                literal: SqlDocumentSemanticLiteral::Number {
                    value: "9".to_owned(),
                },
            },
        );
        assert_eq!(
            appended.document.source,
            "SELECT ARRAY[4, 9] /* preserve */"
        );

        let filter_source = "SELECT amount > 0 FROM \"G\" WHERE amount = 1";
        let filter_document = ugoite_domain::sql_document::parse_sql_document(filter_source);
        let where_clause = filter_document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::WhereClause)
            .expect("WHERE node is emitted");
        let replacement = filter_document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::BinaryExpression
                    && filter_document.source_slice(node.range.unwrap()) == Some("amount > 0")
            })
            .expect("a same-scope predicate is available");
        let replaced_filter = assert_parity(
            filter_source,
            SqlDocumentSemanticEdit::SetFilterPredicate {
                node_id: where_clause.id,
                expected_kind: where_clause.kind,
                expression_node_id: replacement.id,
                expression_expected_kind: replacement.kind,
            },
        );
        assert!(replaced_filter
            .document
            .source
            .ends_with("WHERE amount > 0"));
        let filter_document =
            ugoite_domain::sql_document::parse_sql_document(&replaced_filter.document.source);
        let where_clause = filter_document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::WhereClause)
            .expect("replaced WHERE remains represented");
        let removed_filter = assert_parity(
            &replaced_filter.document.source,
            SqlDocumentSemanticEdit::RemoveFilterClause {
                node_id: where_clause.id,
                expected_kind: where_clause.kind,
            },
        );
        assert!(!removed_filter.document.source.contains("WHERE"));

        let group_source = "SELECT a, b FROM \"G\" GROUP BY a, b";
        let group_document = ugoite_domain::sql_document::parse_sql_document(group_source);
        let group_clause = group_document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::GroupByClause)
            .expect("GROUP BY is represented");
        let mut group_items = group_document
            .nodes
            .iter()
            .filter(|node| {
                node.parent == Some(group_clause.id)
                    && node.kind == SqlDocumentNodeKind::GroupByItem
            })
            .collect::<Vec<_>>();
        group_items.sort_by_key(|item| item.range.map(|range| range.start));
        let moved_group = assert_parity(
            group_source,
            SqlDocumentSemanticEdit::MoveGroupByItem {
                node_id: group_items[1].id,
                expected_kind: group_items[1].kind,
                to_index: 0,
            },
        );
        assert!(moved_group.document.source.ends_with("GROUP BY b, a"));
        let group_document =
            ugoite_domain::sql_document::parse_sql_document(&moved_group.document.source);
        let group_clause = group_document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::GroupByClause)
            .expect("moved GROUP BY remains represented");
        let item = group_document
            .nodes
            .iter()
            .find(|node| {
                node.parent == Some(group_clause.id)
                    && node.kind == SqlDocumentNodeKind::GroupByItem
                    && group_document.source_slice(node.range.unwrap()) == Some("a")
            })
            .expect("the selected group key is available");
        let removed_group = assert_parity(
            &moved_group.document.source,
            SqlDocumentSemanticEdit::RemoveGroupByItem {
                node_id: item.id,
                expected_kind: item.kind,
            },
        );
        assert!(removed_group.document.source.ends_with("GROUP BY b"));

        let aggregate_source = "SELECT SUM(amount) FROM \"G\"";
        let aggregate_document = ugoite_domain::sql_document::parse_sql_document(aggregate_source);
        let sum = aggregate_document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::FunctionExpression
                    && matches!(
                        node.semantic,
                        Some(SqlDocumentNodeData::Function { ref name, .. })
                            if name.last().is_some_and(|part| part.value.eq_ignore_ascii_case("sum"))
                    )
            })
            .expect("SUM is represented as a typed function");
        let average = assert_parity(
            aggregate_source,
            SqlDocumentSemanticEdit::SetAggregateFunction {
                node_id: sum.id,
                expected_kind: sum.kind,
                function: SqlDocumentAggregateFunction::Avg,
            },
        );
        let filter_seed = assert_parity(
            aggregate_source,
            SqlDocumentSemanticEdit::SetAggregateFilter {
                node_id: sum.id,
                expected_kind: sum.kind,
                enabled: true,
            },
        );
        assert!(filter_seed.document.source.contains("FILTER (WHERE TRUE)"));
        let average_document =
            ugoite_domain::sql_document::parse_sql_document(&average.document.source);
        let avg = average_document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::FunctionExpression
                    && matches!(
                        node.semantic,
                        Some(SqlDocumentNodeData::Function { ref name, .. })
                            if name.last().is_some_and(|part| part.value.eq_ignore_ascii_case("avg"))
                    )
            })
            .expect("AVG is represented after the function change");
        let distinct = assert_parity(
            &average.document.source,
            SqlDocumentSemanticEdit::SetAggregateDistinct {
                node_id: avg.id,
                expected_kind: avg.kind,
                distinct: true,
            },
        );
        let distinct_document =
            ugoite_domain::sql_document::parse_sql_document(&distinct.document.source);
        let avg = distinct_document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::FunctionExpression
                    && matches!(
                        node.semantic,
                        Some(SqlDocumentNodeData::Function { ref name, .. })
                            if name.last().is_some_and(|part| part.value.eq_ignore_ascii_case("avg"))
                    )
            })
            .expect("AVG remains represented after DISTINCT");
        let removed_aggregate = assert_parity(
            &distinct.document.source,
            SqlDocumentSemanticEdit::RemoveAggregate {
                node_id: avg.id,
                expected_kind: avg.kind,
            },
        );
        assert_eq!(
            removed_aggregate.document.source,
            "SELECT amount FROM \"G\""
        );

        let metadata_source =
            "SELECT DISTINCT * FROM \"G\" ORDER BY amount DESC NULLS LAST LIMIT 5 OFFSET 2";
        let parsed = ugoite_domain::sql_document::parse_sql_document(metadata_source);
        let parse_request = serde_json::json!({
            "action": "domain.parse_sql_document",
            "value": { "sql": metadata_source },
        });
        let parse_response: Value =
            serde_json::from_str(&super::invoke_json(&parse_request.to_string())).unwrap();
        assert_eq!(
            parse_response["value"],
            serde_json::to_value(&parsed).unwrap()
        );
        assert!(parsed.nodes.iter().any(|node| matches!(
            node.semantic,
            Some(SqlDocumentNodeData::Select {
                distinct: true,
                distinct_on: false
            })
        )));
        assert!(parsed.nodes.iter().any(|node| matches!(
            node.semantic,
            Some(SqlDocumentNodeData::OrderByItem {
                direction: Some(ugoite_domain::sql_document::SqlDocumentOrderDirection::Desc),
                nulls_order: Some(ugoite_domain::sql_document::SqlDocumentNullsOrder::Last),
            })
        )));
        assert!(parsed.nodes.iter().any(|node| matches!(
            node.semantic,
            Some(SqlDocumentNodeData::QueryClauseValue { value: Some(5) })
        )));
        assert!(parsed.nodes.iter().any(|node| matches!(
            node.semantic,
            Some(SqlDocumentNodeData::Wildcard { ref qualifier, qualifier_is_expression: false, has_options: false })
                if qualifier.is_empty()
        )));

        {
            let (source, access, expected) = (
                "SELECT ARRAY[4, 9] /* keep */",
                SqlDocumentValueAccess::ArrayIndex {
                    one_based_index: "2".to_owned(),
                },
                "SELECT (ARRAY[4, 9])[2] /* keep */",
            );
            let document = ugoite_domain::sql_document::parse_sql_document(source);
            let value = document
                .nodes
                .iter()
                .find(|node| {
                    node.range.and_then(|range| document.source_slice(range)) == Some("ARRAY[4, 9]")
                })
                .expect("the selected array expression has a node");
            let edited = assert_parity(
                source,
                SqlDocumentSemanticEdit::WrapExpressionInValueAccess {
                    node_id: value.id,
                    expected_kind: value.kind,
                    access,
                },
            );
            assert_eq!(edited.document.source, expected);
        }
        {
            let (source, access, expected) = (
                "SELECT ARRAY[4, 9] /* keep */",
                SqlDocumentValueAccess::ArraySlice {
                    one_based_start: "1".to_owned(),
                    one_based_end: "2".to_owned(),
                },
                "SELECT (ARRAY[4, 9])[1:2] /* keep */",
            );
            let document = ugoite_domain::sql_document::parse_sql_document(source);
            let value = document
                .nodes
                .iter()
                .find(|node| {
                    node.range.and_then(|range| document.source_slice(range)) == Some("ARRAY[4, 9]")
                })
                .expect("the selected array expression has a node");
            let edited = assert_parity(
                source,
                SqlDocumentSemanticEdit::WrapExpressionInValueAccess {
                    node_id: value.id,
                    expected_kind: value.kind,
                    access,
                },
            );
            assert_eq!(edited.document.source, expected);
        }
    }

    #[test]
    fn sql_document_nested_comment_alias_edit_matches_native_and_undoes_exactly() {
        use ugoite_domain::sql_document::{
            SqlDocumentIdentifier, SqlDocumentNodeKind, SqlDocumentSemanticEdit,
            SqlDocumentSemanticEditTransaction, SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "WITH /* lead */ source AS (SELECT amount AS \"gross\" FROM \"G\" WHERE note = 'x -- literal' /* where keep */) SELECT gross AS \"total\" FROM source ORDER BY total DESC /* tail */";
        let native_document = ugoite_domain::sql_document::parse_sql_document(source);
        let parsed_request = serde_json::json!({
            "action": "domain.parse_sql_document",
            "value": { "sql": source },
        });
        let parsed_response: Value =
            serde_json::from_str(&super::invoke_json(&parsed_request.to_string())).unwrap();
        assert_eq!(parsed_response["ok"], true, "{parsed_response}");
        assert_eq!(
            parsed_response["value"],
            serde_json::to_value(&native_document).unwrap()
        );
        assert!(native_document.diagnostics.is_empty());

        let item = native_document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::SelectItem
                    && node
                        .range
                        .and_then(|range| native_document.source_slice(range))
                        == Some("gross AS \"total\"")
            })
            .expect("the outer aliased projection has an exact source range");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::SetProjectionAlias {
                node_id: item.id,
                expected_kind: item.kind,
                alias: Some(SqlDocumentIdentifier {
                    value: "net total".to_owned(),
                    quote_style: Some('"'),
                }),
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("the outer alias can be edited while CTE source remains untouched");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": { "current_source": source, "transaction": &transaction },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            native.document.source,
            "WITH /* lead */ source AS (SELECT amount AS \"gross\" FROM \"G\" WHERE note = 'x -- literal' /* where keep */) SELECT gross AS \"net total\" FROM source ORDER BY \"net total\" DESC /* tail */"
        );
        assert_eq!(native.document.diagnostics, native_document.diagnostics);

        let undo_request = serde_json::json!({
            "action": "domain.apply_sql_document_edit_transaction",
            "value": { "current_source": native.document.source, "transaction": &native.undo },
        });
        let undo_response: Value =
            serde_json::from_str(&super::invoke_json(&undo_request.to_string())).unwrap();
        assert_eq!(undo_response["ok"], true, "{undo_response}");
        assert_eq!(undo_response["value"]["document"]["source"], source);
        assert_eq!(
            undo_response["value"]["document"]["diagnostics"],
            parsed_response["value"]["diagnostics"]
        );
    }

    #[test]
    fn sql_document_array_index_slice_construction_and_bound_edits_match_native_and_undo() {
        use ugoite_domain::sql_document::{
            apply_sql_document_semantic_edit_transaction, parse_sql_document,
            SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction, SqlDocumentValueAccess,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT ARRAY[4, 9] /* keep */";
        let document = parse_sql_document(source);
        let array = document
            .nodes
            .iter()
            .find(|node| {
                node.range.and_then(|range| document.source_slice(range)) == Some("ARRAY[4, 9]")
            })
            .expect("array has a source-backed projection expression");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::WrapExpressionInValueAccess {
                node_id: array.id,
                expected_kind: array.kind,
                access: SqlDocumentValueAccess::ArrayIndex {
                    one_based_index: "2".to_owned(),
                },
            }],
        };
        let native = apply_sql_document_semantic_edit_transaction(source, &transaction)
            .expect("native array-index edit succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": { "current_source": source, "transaction": &transaction },
        });
        let response: Value = serde_json::from_str(&super::invoke_json(&request.to_string()))
            .expect("WASM returns JSON");
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(native.document.source, "SELECT (ARRAY[4, 9])[2] /* keep */");

        let undo = serde_json::json!({
            "action": "domain.apply_sql_document_edit_transaction",
            "value": {
                "current_source": native.document.source,
                "transaction": &native.undo,
            },
        });
        let undone: Value = serde_json::from_str(&super::invoke_json(&undo.to_string()))
            .expect("WASM returns guarded undo JSON");
        assert_eq!(undone["ok"], true, "{undone}");
        assert_eq!(undone["value"]["document"]["source"], source);

        let field_transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::WrapExpressionInValueAccess {
                node_id: array.id,
                expected_kind: array.kind,
                access: SqlDocumentValueAccess::StructField {
                    name: "display'name".to_owned(),
                },
            }],
        };
        let native_field = apply_sql_document_semantic_edit_transaction(source, &field_transaction)
            .expect("native named Struct-field edit succeeds");
        let field_request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": { "current_source": source, "transaction": &field_transaction },
        });
        let field_response: Value =
            serde_json::from_str(&super::invoke_json(&field_request.to_string()))
                .expect("WASM returns named Struct-field edit JSON");
        assert_eq!(field_response["ok"], true, "{field_response}");
        assert_eq!(
            field_response["value"],
            serde_json::to_value(&native_field).unwrap()
        );
        assert_eq!(
            native_field.document.source,
            "SELECT (ARRAY[4, 9])['display''name'] /* keep */"
        );
        let field_undo = serde_json::json!({
            "action": "domain.apply_sql_document_edit_transaction",
            "value": {
                "current_source": native_field.document.source,
                "transaction": &native_field.undo,
            },
        });
        let undone_field: Value =
            serde_json::from_str(&super::invoke_json(&field_undo.to_string()))
                .expect("WASM returns guarded Struct-field undo JSON");
        assert_eq!(undone_field["ok"], true, "{undone_field}");
        assert_eq!(undone_field["value"]["document"]["source"], source);

        let slice_transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::WrapExpressionInValueAccess {
                node_id: array.id,
                expected_kind: array.kind,
                access: SqlDocumentValueAccess::ArraySlice {
                    one_based_start: "1".to_owned(),
                    one_based_end: "2".to_owned(),
                },
            }],
        };
        let native_slice = apply_sql_document_semantic_edit_transaction(source, &slice_transaction)
            .expect("native array-slice edit succeeds");
        let slice_request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": { "current_source": source, "transaction": &slice_transaction },
        });
        let slice_response: Value =
            serde_json::from_str(&super::invoke_json(&slice_request.to_string()))
                .expect("WASM returns JSON");
        assert_eq!(slice_response["ok"], true, "{slice_response}");
        assert_eq!(
            slice_response["value"],
            serde_json::to_value(&native_slice).unwrap()
        );
        assert_eq!(
            native_slice.document.source,
            "SELECT (ARRAY[4, 9])[1:2] /* keep */"
        );
        let slice_undo = serde_json::json!({
            "action": "domain.apply_sql_document_edit_transaction",
            "value": {
                "current_source": native_slice.document.source,
                "transaction": &native_slice.undo,
            },
        });
        let undone_slice: Value =
            serde_json::from_str(&super::invoke_json(&slice_undo.to_string()))
                .expect("WASM returns guarded undo JSON");
        assert_eq!(undone_slice["ok"], true, "{undone_slice}");
        assert_eq!(undone_slice["value"]["document"]["source"], source);

        let existing_slice_source = native_slice.document.source.as_str();
        let existing_slice_document = parse_sql_document(existing_slice_source);
        let existing_slice = existing_slice_document
            .nodes
            .iter()
            .find(|node| {
                node.semantic.as_ref().is_some_and(|semantic| {
                    matches!(
                        semantic,
                        ugoite_domain::sql_document::SqlDocumentNodeData::ArraySlice { .. }
                    )
                })
            })
            .expect("constructed slice exposes typed existing bounds");
        let bound_transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: existing_slice_source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::SetArraySliceBounds {
                node_id: existing_slice.id,
                expected_kind: existing_slice.kind,
                one_based_start: "2".to_owned(),
                one_based_end: "2".to_owned(),
            }],
        };
        let native_bound_edit =
            apply_sql_document_semantic_edit_transaction(existing_slice_source, &bound_transaction)
                .expect("native existing slice bounds edit succeeds");
        let bound_request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": { "current_source": existing_slice_source, "transaction": &bound_transaction },
        });
        let bound_response: Value =
            serde_json::from_str(&super::invoke_json(&bound_request.to_string()))
                .expect("WASM returns typed existing slice bounds edit");
        assert_eq!(bound_response["ok"], true, "{bound_response}");
        assert_eq!(
            bound_response["value"],
            serde_json::to_value(&native_bound_edit).unwrap()
        );
        assert_eq!(
            native_bound_edit.document.source,
            "SELECT (ARRAY[4, 9])[2:2] /* keep */"
        );
        let bound_undo = serde_json::json!({
            "action": "domain.apply_sql_document_edit_transaction",
            "value": {
                "current_source": native_bound_edit.document.source,
                "transaction": &native_bound_edit.undo,
            },
        });
        let undone_bounds: Value =
            serde_json::from_str(&super::invoke_json(&bound_undo.to_string()))
                .expect("WASM returns guarded slice-bound undo");
        assert_eq!(undone_bounds["ok"], true, "{undone_bounds}");
        assert_eq!(
            undone_bounds["value"]["document"]["source"],
            existing_slice_source
        );
    }

    #[test]
    fn sql_document_binary_expression_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentBinaryOperator, SqlDocumentNodeData, SqlDocumentNodeKind,
            SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT field_100, 'x' FROM \"G\"";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let mut items = document
            .nodes
            .iter()
            .filter(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .collect::<Vec<_>>();
        items.sort_by_key(|node| node.range.map(|range| range.start));
        let left = document
            .nodes
            .iter()
            .find(|node| {
                node.parent == Some(items[0].id)
                    && node.kind == SqlDocumentNodeKind::IdentifierExpression
            })
            .expect("the first projection exposes its identifier");
        let right = document
            .nodes
            .iter()
            .find(|node| {
                matches!(
                    &node.semantic,
                    Some(SqlDocumentNodeData::Literal { raw: Some(raw), .. }) if raw == "'x'"
                )
            })
            .expect("the second projection exposes its string literal");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::ComposeBinaryExpression {
                node_id: items[0].id,
                expected_kind: SqlDocumentNodeKind::SelectItem,
                left_expression_node_id: left.id,
                left_expression_expected_kind: left.kind,
                operator: SqlDocumentBinaryOperator::Concat,
                right_expression_node_id: right.id,
                right_expression_expected_kind: right.kind,
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("native projection expression construction succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT (field_100\n) || ('x'\n), 'x' FROM \"G\""
        );
    }

    #[test]
    fn sql_document_non_projection_binary_expression_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentBinaryOperator, SqlDocumentNodeKind, SqlDocumentSemanticEdit,
            SqlDocumentSemanticEditTransaction, SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT 1 WHERE 1 = 2";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let where_offset = source.find("WHERE").unwrap();
        let values = document
            .nodes
            .iter()
            .filter(|node| node.kind == SqlDocumentNodeKind::Value)
            .collect::<Vec<_>>();
        let value_text = |node: &&ugoite_domain::sql_document::SqlDocumentNode| {
            document.source_slice(node.range.unwrap()).unwrap()
        };
        let projection_value = values
            .iter()
            .find(|node| node.range.unwrap().start < where_offset && value_text(node) == "1")
            .expect("the projection has a typed literal");
        let where_left = values
            .iter()
            .find(|node| node.range.unwrap().start > where_offset && value_text(node) == "1")
            .expect("the WHERE expression has a typed left literal");
        let where_right = values
            .iter()
            .find(|node| node.range.unwrap().start > where_offset && value_text(node) == "2")
            .expect("the WHERE expression has a typed right literal");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::ComposeBinaryExpression {
                node_id: where_left.id,
                expected_kind: where_left.kind,
                left_expression_node_id: projection_value.id,
                left_expression_expected_kind: projection_value.kind,
                operator: SqlDocumentBinaryOperator::Add,
                right_expression_node_id: where_right.id,
                right_expression_expected_kind: where_right.kind,
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("native non-projection expression construction succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT 1 WHERE (1\n) + (2\n) = 2"
        );
    }

    #[test]
    fn sql_document_group_by_expression_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentGroupByMode, SqlDocumentNodeKind, SqlDocumentSemanticEdit,
            SqlDocumentSemanticEditTransaction, SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT field_100 FROM \"G\" ORDER BY field_100";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let select = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Select)
            .expect("the parser exposes the SELECT body");
        let expression = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::IdentifierExpression
                    && node.parent.is_some_and(|parent| {
                        document.nodes[parent as usize].kind == SqlDocumentNodeKind::SelectItem
                    })
            })
            .expect("the projection exposes its typed identifier");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::AddGroupByExpression {
                node_id: select.id,
                expected_kind: SqlDocumentNodeKind::Select,
                expression_node_id: expression.id,
                expression_expected_kind: expression.kind,
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("native GROUP BY expression edit succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT field_100 FROM \"G\" GROUP BY field_100 ORDER BY field_100"
        );

        let group_sets = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::SetGroupByStructure {
                node_id: select.id,
                expected_kind: SqlDocumentNodeKind::Select,
                mode: SqlDocumentGroupByMode::GroupingSets,
                groups: vec![vec![expression.id], vec![]],
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &group_sets,
        )
        .expect("native grouping-set construction succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": { "current_source": source, "transaction": group_sets },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            native.document.source,
            "SELECT field_100 FROM \"G\" GROUP BY GROUPING SETS ((field_100), ()) ORDER BY field_100"
        );
    }

    #[test]
    fn sql_document_order_by_expression_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentNodeKind, SqlDocumentNullsOrder, SqlDocumentOrderDirection,
            SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT field_100 FROM \"G\" LIMIT 10";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let query = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Query)
            .expect("the parser exposes the query");
        let expression = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::IdentifierExpression
                    && node.parent.is_some_and(|parent| {
                        document.nodes[parent as usize].kind == SqlDocumentNodeKind::SelectItem
                    })
            })
            .expect("the projection exposes its typed identifier");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::AddOrderByExpression {
                node_id: query.id,
                expected_kind: SqlDocumentNodeKind::Query,
                expression_node_id: expression.id,
                expression_expected_kind: expression.kind,
                direction: SqlDocumentOrderDirection::Desc,
                nulls_order: Some(SqlDocumentNullsOrder::Last),
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("native ORDER BY expression edit succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT field_100 FROM \"G\" ORDER BY field_100 DESC NULLS LAST LIMIT 10"
        );
    }

    #[test]
    fn sql_document_task_filter_sort_constructors_match_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentColumnReference, SqlDocumentFilterOperator, SqlDocumentIdentifier,
            SqlDocumentNodeKind, SqlDocumentOrderDirection, SqlDocumentSemanticEdit,
            SqlDocumentSemanticEditTransaction, SqlDocumentSemanticLiteral,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT * FROM \"Form Expenses\" AS e WHERE a = 1 OR b = 2";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let select = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Select)
            .unwrap();
        let query = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Query)
            .unwrap();
        let column = SqlDocumentColumnReference {
            qualifier: Some(SqlDocumentIdentifier {
                value: "e".into(),
                quote_style: None,
            }),
            column: SqlDocumentIdentifier {
                value: "field amount".into(),
                quote_style: Some('"'),
            },
        };
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![
                SqlDocumentSemanticEdit::AddFilterComparison {
                    node_id: select.id,
                    expected_kind: select.kind,
                    column: column.clone(),
                    operator: SqlDocumentFilterOperator::Gte,
                    literal: Some(SqlDocumentSemanticLiteral::Number { value: "3".into() }),
                },
                SqlDocumentSemanticEdit::AddOrderByColumn {
                    node_id: query.id,
                    expected_kind: query.kind,
                    column,
                    direction: SqlDocumentOrderDirection::Desc,
                    nulls_order: None,
                },
            ],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("the native typed edits succeed");
        assert!(native
            .document
            .source
            .contains("WHERE (a = 1 OR b = 2) AND (e.\"field amount\" >= 3)"));
        assert!(native
            .document
            .source
            .ends_with("ORDER BY e.\"field amount\" DESC"));
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": { "current_source": source, "transaction": transaction },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
    }

    #[test]
    fn sql_document_expression_leaf_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentNodeKind, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT field_100 + 1 FROM \"G\"";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let target = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::BinaryExpression)
            .expect("the arithmetic expression is parsed");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::ReplaceLiteral {
                node_id: target.id,
                expected_kind: target.kind,
                literal: ugoite_domain::sql_document::SqlDocumentSemanticLiteral::Number {
                    value: "7".to_owned(),
                },
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("native expression leaf edit succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT 7 FROM \"G\""
        );
    }

    #[test]
    fn sql_document_expression_cast_wrapper_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentCastType, SqlDocumentNodeKind, SqlDocumentSemanticEdit,
            SqlDocumentSemanticEditTransaction, SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT field_100 + 1 FROM \"G\"";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let expression = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::BinaryExpression)
            .expect("the arithmetic expression is parsed");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::WrapExpressionInCast {
                node_id: expression.id,
                expected_kind: expression.kind,
                target_type: SqlDocumentCastType::Double,
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("native CAST wrapper edit succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT CAST(field_100 + 1 AS DOUBLE) FROM \"G\""
        );
    }

    #[test]
    fn sql_document_expression_aggregate_wrapper_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentAggregateFunction, SqlDocumentNodeKind, SqlDocumentSemanticEdit,
            SqlDocumentSemanticEditTransaction, SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT amount + 1 FROM \"G\"";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let expression = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::BinaryExpression)
            .expect("the arithmetic expression is parsed");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::WrapExpressionInAggregate {
                node_id: expression.id,
                expected_kind: expression.kind,
                function: SqlDocumentAggregateFunction::Sum,
                distinct: true,
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("native aggregate wrapper edit succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT SUM(DISTINCT amount + 1) FROM \"G\""
        );
    }

    #[test]
    fn sql_document_expression_case_wrapper_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentNodeKind, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT amount + 1 FROM \"G\"";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let expression = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::BinaryExpression)
            .expect("the arithmetic expression is parsed");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::WrapExpressionInCase {
                node_id: expression.id,
                expected_kind: expression.kind,
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("native CASE wrapper edit succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT CASE WHEN TRUE THEN amount + 1 ELSE NULL END FROM \"G\""
        );
    }

    #[test]
    fn sql_document_add_where_clause_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentNodeKind, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT 1 FROM \"G\" GROUP BY 1 ORDER BY 1 LIMIT 2";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let select = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::Select)
            .expect("the SELECT is parsed");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::AddWhereClause {
                node_id: select.id,
                expected_kind: select.kind,
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("native WHERE seed edit succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": { "current_source": source, "transaction": transaction },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT 1 FROM \"G\" WHERE TRUE GROUP BY 1 ORDER BY 1 LIMIT 2"
        );
    }

    #[test]
    fn sql_document_predicate_template_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentNodeKind, SqlDocumentPredicateTemplate, SqlDocumentSemanticEdit,
            SqlDocumentSemanticEditTransaction, SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT amount + 1 FROM \"G\"";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let expression = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::BinaryExpression)
            .expect("the arithmetic expression is parsed");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::WrapExpressionInPredicate {
                node_id: expression.id,
                expected_kind: expression.kind,
                template: SqlDocumentPredicateTemplate::Between,
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("native predicate template edit succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": { "current_source": source, "transaction": transaction },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT (amount + 1) BETWEEN NULL AND NULL FROM \"G\""
        );
    }

    #[test]
    fn sql_document_postfix_predicate_and_typed_literal_protocol_match_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentNodeKind, SqlDocumentPredicateTemplate, SqlDocumentSemanticEdit,
            SqlDocumentSemanticEditTransaction, SqlDocumentSemanticLiteral,
            SqlDocumentTypedLiteralType, SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT DATE '2026-01-01'";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let literal = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::LiteralExpression)
            .expect("DATE literal is an expression node");
        let literal_edit = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::ReplaceLiteral {
                node_id: literal.id,
                expected_kind: literal.kind,
                literal: SqlDocumentSemanticLiteral::TypedString {
                    data_type: SqlDocumentTypedLiteralType::Date,
                    value: "2026-02-03".into(),
                },
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &literal_edit,
        )
        .expect("native typed literal edit succeeds");
        let literal_request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": { "current_source": source, "transaction": literal_edit },
        });
        let literal_response: Value =
            serde_json::from_str(&super::invoke_json(&literal_request.to_string())).unwrap();
        assert_eq!(literal_response["ok"], true, "{literal_response}");
        assert_eq!(
            literal_response["value"],
            serde_json::to_value(&native).unwrap()
        );

        let source = "SELECT enabled FROM \"G\"";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let expression = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::IdentifierExpression)
            .expect("the selected expression is parsed");
        let predicate_edit = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::WrapExpressionInPredicate {
                node_id: expression.id,
                expected_kind: expression.kind,
                template: SqlDocumentPredicateTemplate::IsNotDistinctFrom,
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &predicate_edit,
        )
        .expect("native typed predicate edit succeeds");
        let predicate_request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": { "current_source": source, "transaction": predicate_edit },
        });
        let predicate_response: Value =
            serde_json::from_str(&super::invoke_json(&predicate_request.to_string())).unwrap();
        assert_eq!(predicate_response["ok"], true, "{predicate_response}");
        assert_eq!(
            predicate_response["value"],
            serde_json::to_value(&native).unwrap()
        );
    }

    #[test]
    fn sql_document_in_list_value_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentNodeKind, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT amount FROM \"G\" WHERE amount IN (1, 2) /* keep */";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let predicate = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::PredicateExpression
                    && matches!(
                        node.semantic,
                        Some(ugoite_domain::sql_document::SqlDocumentNodeData::Predicate {
                            ref operator,
                            ..
                        }) if operator == "in"
                    )
            })
            .expect("the IN predicate is parsed");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::AppendInListValue {
                node_id: predicate.id,
                expected_kind: predicate.kind,
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("native IN-list append succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": { "current_source": source, "transaction": transaction },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT amount FROM \"G\" WHERE amount IN (1, 2, NULL) /* keep */"
        );
    }

    #[test]
    fn sql_document_expression_copy_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentNodeKind, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT field_100 + 1, field_100 FROM \"G\"";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let target = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::BinaryExpression
                    && node.range.and_then(|range| document.source_slice(range))
                        == Some("field_100 + 1")
            })
            .expect("the first projection exposes its binary expression");
        let expression = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::IdentifierExpression
                    && node
                        .range
                        .is_some_and(|range| range.start > target.range.unwrap().end)
            })
            .expect("the second projection exposes a same-query identifier");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::ReplaceExpressionFromNode {
                node_id: target.id,
                expected_kind: SqlDocumentNodeKind::BinaryExpression,
                expression_node_id: expression.id,
                expression_expected_kind: SqlDocumentNodeKind::IdentifierExpression,
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("native expression replacement succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT field_100, field_100 FROM \"G\""
        );
    }

    #[test]
    fn sql_document_projection_expression_add_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentNodeKind, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT field_100 FROM \"G\" WHERE field_100 > 10";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let list = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SelectList)
            .expect("the parser exposes the SELECT list");
        let expression = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::BinaryExpression)
            .expect("the parser exposes the typed WHERE expression");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::AddProjectionExpression {
                node_id: list.id,
                expected_kind: SqlDocumentNodeKind::SelectList,
                expression_node_id: expression.id,
                expression_expected_kind: SqlDocumentNodeKind::BinaryExpression,
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("native projection insertion succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT field_100, field_100 > 10 FROM \"G\" WHERE field_100 > 10"
        );
    }

    #[test]
    fn sql_document_scalar_subquery_projection_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentNodeKind, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT 1 FROM \"G\"";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let list = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SelectList)
            .expect("the parser exposes the SELECT list");
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::AddScalarSubqueryProjection {
                node_id: list.id,
                expected_kind: SqlDocumentNodeKind::SelectList,
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &transaction,
        )
        .expect("native scalar subquery insertion succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT 1, (SELECT 1) AS __ugoite_scalar_subquery FROM \"G\""
        );
    }

    #[test]
    fn sql_document_projection_item_edits_protocol_matches_native() {
        use ugoite_domain::sql_document::{
            SqlDocumentNodeKind, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };

        let source = "SELECT 1, 2, 3 FROM \"G\"";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let mut items = document
            .nodes
            .iter()
            .filter(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .collect::<Vec<_>>();
        items.sort_by_key(|node| node.range.map(|range| range.start));
        let move_transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::MoveProjectionItem {
                node_id: items[0].id,
                expected_kind: SqlDocumentNodeKind::SelectItem,
                to_index: 2,
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &move_transaction,
        )
        .expect("native projection reorder succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": move_transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT 2, 3, 1 FROM \"G\""
        );

        let moved_source = native.document.to_sql();
        let moved_document = ugoite_domain::sql_document::parse_sql_document(moved_source);
        let mut moved_items = moved_document
            .nodes
            .iter()
            .filter(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .collect::<Vec<_>>();
        moved_items.sort_by_key(|node| node.range.map(|range| range.start));
        let remove_transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: moved_source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::RemoveProjectionItem {
                node_id: moved_items[1].id,
                expected_kind: SqlDocumentNodeKind::SelectItem,
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            moved_source,
            &remove_transaction,
        )
        .expect("native projection removal succeeds");
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": {
                "current_source": moved_source,
                "transaction": remove_transaction,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(
            response["value"]["document"]["source"],
            "SELECT 2, 1 FROM \"G\""
        );
    }

    #[test]
    fn sql_document_projection_alias_removal_conflict_is_exposed_to_wasm() {
        use ugoite_domain::sql_document::{
            SqlDocumentNodeKind, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
            SQL_DOCUMENT_CONTRACT_VERSION,
        };
        let source =
            "SELECT 'fixed' AS field_100, field_100 AS actual FROM tasks ORDER BY field_100";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let item = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .unwrap();
        let transaction = SqlDocumentSemanticEditTransaction {
            contract_version: SQL_DOCUMENT_CONTRACT_VERSION,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::RemoveProjectionItem {
                node_id: item.id,
                expected_kind: SqlDocumentNodeKind::SelectItem,
            }],
        };
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_semantic_edit_transaction",
            "value": { "current_source": source, "transaction": transaction },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], false, "{response}");
        assert_eq!(response["error"]["code"], "projection_alias_conflict");
    }

    #[test]
    fn sql_document_node_edit_protocol_bounds_amplified_result_and_oversized_requests() {
        let source = format!("SELECT {}", vec!["1"; 20_000].join(","));
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_node_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": {
                    "contract_version": ugoite_domain::sql_document::SQL_DOCUMENT_CONTRACT_VERSION,
                    "expected_source": source,
                    "edits": [],
                }
            }
        })
        .to_string();
        let response_text = super::invoke_json(&request);
        assert!(response_text.len() <= super::MAX_SQL_DOCUMENT_RESPONSE_BYTES);
        let response: Value = serde_json::from_str(&response_text).unwrap();
        assert_eq!(response["ok"], false, "{response}");
        assert_eq!(response["error"]["kind"], "domain_output_too_large");

        let oversized_request = serde_json::json!({
            "action": "domain.apply_sql_document_node_edit_transaction",
            "value": {
                "current_source": "x".repeat(super::MAX_SQL_DOCUMENT_NODE_EDIT_REQUEST_BYTES),
                "transaction": {
                    "contract_version": ugoite_domain::sql_document::SQL_DOCUMENT_CONTRACT_VERSION,
                    "expected_source": "",
                    "edits": [],
                }
            }
        })
        .to_string();
        assert!(oversized_request.len() > super::MAX_SQL_DOCUMENT_NODE_EDIT_REQUEST_BYTES);
        let rejected: Value =
            serde_json::from_str(&super::invoke_json(&oversized_request)).unwrap();
        assert_eq!(rejected["ok"], false, "{rejected}");
        assert_eq!(rejected["error"]["kind"], "input_too_large");
        assert_eq!(
            rejected["error"]["message"],
            "JSON protocol input exceeds its size limit"
        );
    }

    #[test]
    fn sql_document_edit_error_codes_cover_typed_node_failures() {
        use ugoite_domain::sql_document::SqlDocumentEditError as EditError;

        let cases = [
            (EditError::DocumentNotParsed, "document_not_parsed"),
            (
                EditError::ContractVersionMismatch,
                "contract_version_mismatch",
            ),
            (EditError::NodeNotFound, "node_not_found"),
            (EditError::NodeKindMismatch, "node_kind_mismatch"),
            (EditError::NodeRangeUnavailable, "node_range_unavailable"),
            (EditError::DuplicateNodeTarget, "duplicate_node_target"),
        ];

        for (error, expected_code) in cases {
            let response: Value =
                serde_json::from_str(&super::sql_document_edit_error(error)).unwrap();
            assert_eq!(response["ok"], false);
            assert_eq!(response["error"]["kind"], "sql_document_edit");
            assert_eq!(response["error"]["code"], expected_code);
            assert_eq!(response["error"]["message"], error.to_string());
        }
    }

    #[test]
    fn sql_document_edit_protocol_reports_drafts_and_stable_atomic_errors() {
        let incomplete = serde_json::json!({
            "action": "domain.apply_sql_document_edit_transaction",
            "value": {
                "current_source": "SELECT 1",
                "transaction": {
                    "expected_source": "SELECT 1",
                    "edits": [{
                        "range": {"start": 7, "end": 8},
                        "expected_text": "1",
                        "replacement": "("
                    }]
                }
            }
        });
        let draft: Value =
            serde_json::from_str(&super::invoke_json(&incomplete.to_string())).unwrap();
        assert_eq!(draft["ok"], true, "{draft}");
        assert_eq!(draft["value"]["document"]["status"], "invalid_sql");
        assert_eq!(
            draft["value"]["document"]["diagnostics"][0]["code"],
            "parse_failed"
        );

        let stale = serde_json::json!({
            "action": "domain.apply_sql_document_edit_transaction",
            "value": {
                "current_source": "SELECT 2",
                "transaction": {"expected_source": "SELECT 1", "edits": []}
            }
        });
        let stale: Value = serde_json::from_str(&super::invoke_json(&stale.to_string())).unwrap();
        assert_eq!(stale["ok"], false, "{stale}");
        assert_eq!(stale["error"]["kind"], "sql_document_edit");
        assert_eq!(stale["error"]["code"], "stale_source");

        let out_of_bounds = serde_json::json!({
            "action": "domain.apply_sql_document_edit_transaction",
            "value": {
                "current_source": "SELECT 1",
                "transaction": {
                    "expected_source": "SELECT 1",
                    "edits": [{
                        "range": {"start": 8, "end": 9},
                        "expected_text": "1",
                        "replacement": "2"
                    }]
                }
            }
        });
        let out_of_bounds: Value =
            serde_json::from_str(&super::invoke_json(&out_of_bounds.to_string())).unwrap();
        assert_eq!(out_of_bounds["ok"], false, "{out_of_bounds}");
        assert_eq!(out_of_bounds["error"]["code"], "range_out_of_bounds");

        let invalid_request = serde_json::json!({
            "action": "domain.apply_sql_document_edit_transaction",
            "value": {"current_source": "SELECT 1", "transaction": {"edits": []}}
        });
        let invalid_request: Value =
            serde_json::from_str(&super::invoke_json(&invalid_request.to_string())).unwrap();
        assert_eq!(invalid_request["ok"], false, "{invalid_request}");
        assert_eq!(invalid_request["error"]["code"], "invalid_request");
    }

    #[test]
    fn sql_document_edit_protocol_bounds_amplified_result_and_oversized_requests() {
        let source = format!("SELECT {}", vec!["1"; 20_000].join(","));
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_edit_transaction",
            "value": {
                "current_source": source,
                "transaction": {"expected_source": source, "edits": []}
            }
        })
        .to_string();
        let response_text = super::invoke_json(&request);
        assert!(response_text.len() <= super::MAX_SQL_DOCUMENT_RESPONSE_BYTES);
        let response: Value = serde_json::from_str(&response_text).unwrap();
        assert_eq!(response["ok"], false, "{response}");
        assert_eq!(response["error"]["kind"], "domain_output_too_large");

        let oversized_request = serde_json::json!({
            "action": "domain.apply_sql_document_edit_transaction",
            "value": {
                "current_source": "x".repeat(super::MAX_SQL_DOCUMENT_EDIT_REQUEST_BYTES),
                "transaction": {"expected_source": "", "edits": []}
            }
        })
        .to_string();
        assert!(oversized_request.len() > super::MAX_SQL_DOCUMENT_EDIT_REQUEST_BYTES);
        let rejected: Value =
            serde_json::from_str(&super::invoke_json(&oversized_request)).unwrap();
        assert_eq!(rejected["ok"], false, "{rejected}");
        assert_eq!(rejected["error"]["kind"], "input_too_large");
        assert_eq!(
            rejected["error"]["message"],
            "JSON protocol input exceeds its size limit"
        );

        let control_source = "\0".repeat(ugoite_domain::sql_document::MAX_SQL_DOCUMENT_BYTES);
        let maximum_escaped_request = serde_json::json!({
            "action": "domain.apply_sql_document_edit_transaction",
            "value": {
                "current_source": control_source,
                "transaction": {
                    "expected_source": control_source,
                    "edits": [{
                        "range": {
                            "start": 0,
                            "end": ugoite_domain::sql_document::MAX_SQL_DOCUMENT_BYTES,
                        },
                        "expected_text": control_source,
                        "replacement": control_source,
                    }],
                },
            },
        })
        .to_string();
        assert!(maximum_escaped_request.len() > super::MAX_PROTOCOL_REQUEST_BYTES);
        assert!(maximum_escaped_request.len() <= super::MAX_SQL_DOCUMENT_EDIT_REQUEST_BYTES);
        let bounded_result: Value =
            serde_json::from_str(&super::invoke_json(&maximum_escaped_request)).unwrap();
        assert_eq!(bounded_result["ok"], false, "{bounded_result}");
        assert_eq!(bounded_result["error"]["kind"], "domain_output_too_large");
    }
    #[test]
    fn composition_canonicalization_matches_the_native_domain_contract() {
        let native =
            ugoite_domain::composition::canonicalize_composition_yaml(SQL_FIRST_COMPOSITION)
                .unwrap();
        let request = serde_json::json!({
            "action": "domain.canonicalize_composition",
            "value": {"yaml": SQL_FIRST_COMPOSITION},
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(
            response["value"]["document"],
            serde_json::to_value(&native.document).unwrap()
        );
        assert_eq!(response["value"]["canonical_yaml"], native.yaml);
        assert_eq!(response["value"]["fingerprint"], native.fingerprint);

        let normalized_document = serde_json::to_value(&native.document).unwrap();
        let mut unnormalized_document = normalized_document.clone();
        let bindings = unnormalized_document["spec"]["variables"][2]["query"]["form_bindings"]
            .as_array_mut()
            .expect("the inline SQL source has Form bindings");
        let mut second_binding = bindings[0].clone();
        second_binding["relation"] = serde_json::json!("Other");
        bindings.push(second_binding);
        bindings.reverse();
        assert_ne!(unnormalized_document, normalized_document);
        let native_typed = ugoite_domain::composition::canonicalize_composition_document_value(
            unnormalized_document.clone(),
        )
        .unwrap();
        let ugoite_domain::composition::VariableGraphNode {
            kind: ugoite_domain::composition::VariableGraphNodeKind::Query { query, .. },
            ..
        } = &native_typed.document.spec.variables[2]
        else {
            panic!("expense_rows should be a query variable");
        };
        assert_eq!(query.form_bindings[0].relation, "Expense");
        assert_eq!(query.form_bindings[1].relation, "Other");
        let typed_request = serde_json::json!({
            "action": "domain.canonicalize_composition_document",
            "value": {"document": unnormalized_document},
        });
        let typed_response: Value =
            serde_json::from_str(&super::invoke_json(&typed_request.to_string())).unwrap();
        assert_eq!(typed_response["ok"], true, "{typed_response}");
        assert_eq!(
            typed_response["value"]["document"],
            serde_json::to_value(native_typed.document).unwrap()
        );
        assert_eq!(typed_response["value"]["canonical_yaml"], native_typed.yaml);
        assert_eq!(
            typed_response["value"]["fingerprint"],
            native_typed.fingerprint
        );

        let invalid_reference_yaml =
            SQL_FIRST_COMPOSITION.replace("      variable: month_total", "      variable: missing");
        let native_invalid_reference =
            ugoite_domain::composition::canonicalize_composition_yaml(&invalid_reference_yaml)
                .unwrap_err();
        let invalid_metric_request = serde_json::json!({
            "action": "domain.canonicalize_composition",
            "value": {"yaml": invalid_reference_yaml},
        });
        let invalid_metric_response: Value =
            serde_json::from_str(&super::invoke_json(&invalid_metric_request.to_string())).unwrap();
        assert_eq!(native_invalid_reference.as_str(), "invalid_composition");
        assert_eq!(invalid_metric_response["ok"], false);
        assert_eq!(
            invalid_metric_response["error"]["code"],
            native_invalid_reference.as_str()
        );

        let invalid_typed_request = serde_json::json!({
            "action": "domain.canonicalize_composition_document",
            "value": {"document": {"format": "unknown"}},
        });
        let invalid_typed_response: Value =
            serde_json::from_str(&super::invoke_json(&invalid_typed_request.to_string())).unwrap();
        assert_eq!(invalid_typed_response["ok"], false);
        assert_eq!(
            invalid_typed_response["error"]["kind"],
            "composition_diagnostic"
        );
        assert_eq!(
            invalid_typed_response["error"]["code"],
            "invalid_composition"
        );

        let mut unknown_version_document = serde_json::to_value(&native.document).unwrap();
        unknown_version_document["format_version"] = serde_json::json!(99);
        unknown_version_document["future_field"] = serde_json::json!({"enabled": true});
        let native_unknown_version_error =
            ugoite_domain::composition::canonicalize_composition_document_value(
                unknown_version_document.clone(),
            )
            .unwrap_err();
        let unknown_version_request = serde_json::json!({
            "action": "domain.canonicalize_composition_document",
            "value": {"document": unknown_version_document},
        });
        let unknown_version_response: Value =
            serde_json::from_str(&super::invoke_json(&unknown_version_request.to_string()))
                .unwrap();
        assert_eq!(unknown_version_response["ok"], false);
        assert_eq!(
            unknown_version_response["error"]["kind"],
            "composition_diagnostic"
        );
        assert_eq!(
            native_unknown_version_error.as_str(),
            "unsupported_format_version"
        );
        assert_eq!(
            unknown_version_response["error"]["code"],
            native_unknown_version_error.as_str()
        );

        let mut unknown_v1_field_document = serde_json::to_value(&native.document).unwrap();
        unknown_v1_field_document["spec"]["layout"]["rows"][0]["items"][0]["future_field"] =
            serde_json::json!({"enabled": true});
        let native_unknown_v1_error =
            ugoite_domain::composition::canonicalize_composition_document_value(
                unknown_v1_field_document.clone(),
            )
            .unwrap_err();
        let unknown_v1_field_request = serde_json::json!({
            "action": "domain.canonicalize_composition_document",
            "value": {"document": unknown_v1_field_document},
        });
        let unknown_v1_field_response: Value =
            serde_json::from_str(&super::invoke_json(&unknown_v1_field_request.to_string()))
                .unwrap();
        assert_eq!(native_unknown_v1_error.as_str(), "invalid_composition");
        assert_eq!(unknown_v1_field_response["ok"], false);
        assert_eq!(
            unknown_v1_field_response["error"]["code"],
            native_unknown_v1_error.as_str()
        );

        let invalid_layout_yaml = SQL_FIRST_COMPOSITION.replace(
            "          - kind: component\n            component: transactions\n",
            "",
        );
        assert_ne!(invalid_layout_yaml, SQL_FIRST_COMPOSITION);
        let native_invalid_layout =
            ugoite_domain::composition::canonicalize_composition_yaml(&invalid_layout_yaml)
                .unwrap_err();
        let invalid_layout_request = serde_json::json!({
            "action": "domain.canonicalize_composition",
            "value": {"yaml": invalid_layout_yaml},
        });
        let invalid_layout_response: Value =
            serde_json::from_str(&super::invoke_json(&invalid_layout_request.to_string())).unwrap();
        assert_eq!(native_invalid_layout.as_str(), "invalid_composition");
        assert_eq!(invalid_layout_response["ok"], false);
        assert_eq!(
            invalid_layout_response["error"]["code"],
            native_invalid_layout.as_str()
        );
    }

    #[test]
    fn variable_graph_input_protocol_matches_native_domain_contract() {
        use ugoite_domain::composition::{
            parse_variable_graph_input, VariableGraphInputValue, VariableGraphScalarDescriptor,
            VariableGraphScalarType,
        };

        let cases = [
            (
                VariableGraphScalarType::String,
                "",
                serde_json::json!({"status": "unset"}),
            ),
            (
                VariableGraphScalarType::String,
                "日本語",
                serde_json::json!({"status": "valid", "value": "日本語"}),
            ),
            (
                VariableGraphScalarType::String,
                "\0",
                serde_json::json!({"status": "invalid"}),
            ),
            (
                VariableGraphScalarType::Boolean,
                "true",
                serde_json::json!({"status": "valid", "value": true}),
            ),
            (
                VariableGraphScalarType::Boolean,
                "TRUE",
                serde_json::json!({"status": "invalid"}),
            ),
            (
                VariableGraphScalarType::Integer,
                "+9007199254740991",
                serde_json::json!({"status": "valid", "value": 9_007_199_254_740_991_i64}),
            ),
            (
                VariableGraphScalarType::Integer,
                "1.0",
                serde_json::json!({"status": "invalid"}),
            ),
            (
                VariableGraphScalarType::Integer,
                "9007199254740992",
                serde_json::json!({"status": "valid", "value": 9_007_199_254_740_992_i64}),
            ),
            (
                VariableGraphScalarType::Integer,
                "9223372036854775807",
                serde_json::json!({"status": "valid", "value": i64::MAX}),
            ),
            (
                VariableGraphScalarType::Integer,
                "-9223372036854775808",
                serde_json::json!({"status": "valid", "value": i64::MIN}),
            ),
            (
                VariableGraphScalarType::Integer,
                "9223372036854775808",
                serde_json::json!({"status": "invalid"}),
            ),
            (
                VariableGraphScalarType::Float,
                "1",
                serde_json::json!({"status": "valid", "value": 1.0}),
            ),
            (
                VariableGraphScalarType::Float,
                "1e-4000",
                serde_json::json!({"status": "invalid"}),
            ),
            (
                VariableGraphScalarType::Float,
                "1e309",
                serde_json::json!({"status": "invalid"}),
            ),
            (
                VariableGraphScalarType::Decimal,
                "12.3400",
                serde_json::json!({"status": "valid", "value": "12.3400"}),
            ),
            (
                VariableGraphScalarType::Decimal,
                "1e3",
                serde_json::json!({"status": "invalid"}),
            ),
            (
                VariableGraphScalarType::Date,
                "2024-02-29",
                serde_json::json!({"status": "valid", "value": "2024-02-29"}),
            ),
            (
                VariableGraphScalarType::Date,
                "2025-02-29",
                serde_json::json!({"status": "invalid"}),
            ),
            (
                VariableGraphScalarType::Timestamp,
                "2026-10-13T23:59:59.123456789+09:00",
                serde_json::json!({"status": "valid", "value": "2026-10-13T23:59:59.123456789+09:00"}),
            ),
            (
                VariableGraphScalarType::Timestamp,
                "2026-10-13T14:59:59",
                serde_json::json!({"status": "invalid"}),
            ),
            (
                VariableGraphScalarType::Json,
                "{\"n\":1}",
                serde_json::json!({"status": "valid", "value": {"n": 1}}),
            ),
            (
                VariableGraphScalarType::Json,
                "null",
                serde_json::json!({"status": "invalid"}),
            ),
        ];

        for (scalar_type, raw, expected) in cases {
            let value_type = VariableGraphScalarDescriptor {
                scalar_type,
                nullable: false,
            };
            let native = match parse_variable_graph_input(value_type, raw) {
                VariableGraphInputValue::Unset => serde_json::json!({"status": "unset"}),
                VariableGraphInputValue::Valid(value) => {
                    serde_json::json!({"status": "valid", "value": value.to_json_value()})
                }
                VariableGraphInputValue::Invalid => serde_json::json!({"status": "invalid"}),
            };
            assert_eq!(
                native, expected,
                "native parser case {scalar_type:?} {raw:?}"
            );

            let request = serde_json::json!({
                "action": "domain.parse_variable_graph_input",
                "value": {"value_type": value_type, "raw": raw},
            });
            let response: Value =
                serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
            assert_eq!(response["ok"], true, "{scalar_type:?} {raw:?}: {response}");
            assert_eq!(response["value"], expected, "{scalar_type:?} {raw:?}");
        }

        let oversized_string =
            "x".repeat(ugoite_domain::composition::MAX_COMPOSITION_VARIABLE_STRING_BYTES + 1);
        let oversized_request = serde_json::json!({
            "action": "domain.parse_variable_graph_input",
            "value": {"value_type": {"scalar_type": "string", "nullable": false}, "raw": oversized_string},
        });
        let oversized_response: Value =
            serde_json::from_str(&super::invoke_json(&oversized_request.to_string())).unwrap();
        assert_eq!(oversized_response["ok"], true, "{oversized_response}");
        assert_eq!(
            oversized_response["value"],
            serde_json::json!({"status": "invalid"})
        );

        for payload in [
            serde_json::json!({"raw": "1"}),
            serde_json::json!({"value_type": {"scalar_type": "unknown", "nullable": false}, "raw": "1"}),
            serde_json::json!({"value_type": {"scalar_type": "integer", "nullable": false}, "raw": 1}),
        ] {
            let request = serde_json::json!({
                "action": "domain.parse_variable_graph_input",
                "value": payload,
            });
            let response: Value =
                serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
            assert_eq!(response["ok"], false, "{response}");
            assert_eq!(response["error"]["kind"], "domain_validation", "{response}");
        }
    }

    #[test]
    fn spreadsheet_protocol_uses_the_shared_domain_encoder() {
        let request = serde_json::json!({
            "action": "domain.encode_spreadsheet_csv",
            "value": [["=SUM(A1:A2)", "a,b", "line\nbreak", "日本語"]]
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();

        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(
            response["value"],
            "\"'=SUM(A1:A2)\",\"a,b\",\"line\nbreak\",\"日本語\""
        );
    }

    #[test]
    fn test_api_req_api_001_wasm_adapter_exposes_operation_manifest() {
        let response = super::invoke_json(r#"{"action":"operations"}"#);
        let response: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["ok"], true);
        assert_eq!(
            response["value"],
            serde_json::json!(ugoite_api_client::SUPPORTED_OPERATIONS)
        );
    }

    #[test]
    fn entry_query_page_prepare_round_trips_through_wasm() {
        let request = serde_json::json!({
            "action": "prepare",
            "operation": "entry.query",
            "arguments": {"space_id": "demo"},
            "body": {
                "query": {"scope": {"kind": "all"}, "text": "open"},
                "projection": {"kind": "preview"},
                "limit": 50
            }
        })
        .to_string();
        let response: Value = serde_json::from_str(&super::invoke_json(&request)).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"]["path"], "/spaces/demo/entries/query");
        assert_eq!(
            response["value"]["body"],
            serde_json::json!({
                "query": {"scope": {"kind": "all"}, "text": "open"},
                "projection": {"kind": "preview"},
                "limit": 50
            })
            .to_string()
        );
    }

    #[test]
    fn composition_snapshot_check_uses_the_portable_exact_revision_route() {
        let request = serde_json::json!({
            "action": "prepare",
            "operation": "composition.snapshot_check",
            "arguments": {
                "space_id": "space-1",
                "composition_id": "composition-1",
                "revision_id": "revision-1",
            },
        })
        .to_string();
        let response: Value = serde_json::from_str(&super::invoke_json(&request)).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"]["method"], "POST");
        assert_eq!(
            response["value"]["path"],
            "/spaces/space-1/compositions/composition-1/history/revision-1/snapshot"
        );
        assert_eq!(response["value"]["body_kind"], "none");
    }

    #[test]
    fn portable_form_validation_is_available_without_storage() {
        let response = super::invoke_json(
            r#"{"action":"domain.validate_form","value":{"id":"00000000-0000-0000-0000-000000000001","version":1,"name":"Task","fields":[],"allow_extra_attributes":false}}"#,
        );
        let response: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["ok"], true);
        assert_eq!(response["value"]["name"], "Task");
    }

    #[test]
    fn portable_revision_draft_derives_optimistic_concurrency_fields() {
        let response = super::invoke_json(
            r#"{"action":"domain.build_revision_draft","value":{"form":{"id":"00000000-0000-0000-0000-000000000001","version":1,"name":"Task","fields":[],"allow_extra_attributes":false},"draft":{"form_id":"00000000-0000-0000-0000-000000000001","entry_id":"00000000-0000-0000-0000-000000000002","revision_id":"00000000-0000-0000-0000-000000000003","change_id":"00000000-0000-0000-0000-000000000004","operation":"upsert","committed_at_micros":1,"author_id":"human:owner","form_version":1,"source_kind":"wasm","source_id":null,"values":{}},"current":null}}"#,
        );
        let response: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"]["entry_version"], 1);
        assert_eq!(response["value"]["expected_version"], Value::Null);
        assert_eq!(response["value"]["parent_revision_id"], Value::Null);
        assert_eq!(response["value"]["entry"]["updated_by"], "human:owner");
    }

    #[test]
    fn konase_protocol_creates_deterministic_state_and_steps_without_io() {
        let version_response =
            serde_json::from_str::<Value>(&super::invoke_json(r#"{"action":"konase.version"}"#))
                .unwrap();
        assert_eq!(version_response["ok"], true);
        assert_eq!(version_response["value"]["protocol_version"], 3);

        let new_response =
            serde_json::from_str::<Value>(&super::invoke_json(r#"{"action":"konase.new"}"#))
                .unwrap();
        assert_eq!(new_response["ok"], true);
        assert_eq!(new_response["value"]["status"], "idle");

        let step_request = serde_json::json!({
            "action": "konase.step",
            "value": {
                "state": new_response["value"],
                "event": {
                    "user_submitted": {
                        "work_id": "work-1",
                        "job_id": "job-1",
                        "goal": "find notes",
                        "available_capabilities": [{
                            "name": "ugoite.search",
                            "description": "search knowledge"
                        }],
                        "safety_hints": ["save only after confirmation"],
                        "selected_resource_contents": [{
                            "uri": "ugoite://form/00000000-0000-0000-0000-0000000000a1",
                            "content": "{\"id\":\"00000000-0000-0000-0000-0000000000a1\",\"name\":\"Expense\",\"fields\":{\"amount\":{\"type\":\"number\",\"required\":true}},\"_untrusted_content\":true}"
                        }]
                    }
                }
            }
        })
        .to_string();
        let first = super::invoke_json(&step_request);
        let second = super::invoke_json(&step_request);
        assert_eq!(first, second);
        let response: Value = serde_json::from_str(&first).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"]["state"]["status"], "working");
        assert_eq!(
            response["value"]["effects"][0]["start_job"]["job"]["id"],
            "job-1"
        );
        assert_eq!(
            response["value"]["effects"][0]["start_job"]["context"]["selected_resource_contents"]
                [0]["uri"],
            "ugoite://form/00000000-0000-0000-0000-0000000000a1"
        );
    }

    #[test]
    fn konase_protocol_rejects_unknown_actions_and_keeps_network_out_of_wasm() {
        let response: Value =
            serde_json::from_str(&super::invoke_json(r#"{"action":"konase.unknown"}"#)).unwrap();
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"]["kind"], "konase_protocol");
    }

    #[test]
    fn konase_protocol_normalizes_loaded_state_and_rejects_oversized_requests() {
        let oversized = "x".repeat(ugoite_konase::MAX_STATE_JSON_BYTES - 256);
        let loaded = serde_json::json!({
            "action": "konase.new",
            "value": {
                "state": {
                    "status": "completed",
                    "work": {
                        "id": oversized,
                        "goal": "goal",
                        "status": "completed",
                        "job_count": 1
                    }
                }
            }
        });
        let loaded: Value = serde_json::from_str(&super::invoke_json(&loaded.to_string())).unwrap();
        assert_eq!(loaded["ok"], false, "{loaded}");
        assert_eq!(loaded["error"]["kind"], "konase_protocol");

        let too_large = serde_json::json!({
            "action": "konase.context",
            "value": {
                "work_goal": "x".repeat(ugoite_konase::MAX_STATE_JSON_BYTES),
                "job_goal": "job"
            }
        });
        let too_large: Value =
            serde_json::from_str(&super::invoke_json(&too_large.to_string())).unwrap();
        assert_eq!(too_large["ok"], false);
        assert_eq!(too_large["error"]["kind"], "konase_protocol");
    }

    fn entry_test_form() -> serde_json::Value {
        serde_json::json!({
            "id": "00000000-0000-0000-0000-0000000000a1",
            "version": 1,
            "name": "Note",
            "fields": [
                {"id": 100, "name": "Body", "field_type": "string", "required": true},
                {"id": 101, "name": "Done", "field_type": "boolean", "required": false},
                {"id": 102, "name": "Count", "field_type": "integer", "required": false}
            ],
            "allow_extra_attributes": false
        })
    }

    #[test]
    fn entry_validate_draft_matches_native_preview() {
        let form = entry_test_form();
        let draft = serde_json::json!({
            "form_name": "Note",
            "tags": [],
            "fields": {"Body": "hello", "Done": true, "Count": 42},
            "extra_attributes": {}
        });
        let request = serde_json::json!({
            "action": "entry.validate_draft",
            "value": {"form": form, "draft": draft}
        })
        .to_string();
        let response: Value = serde_json::from_str(&super::invoke_json(&request)).unwrap();
        assert_eq!(response["ok"], true, "{response}");

        // Native preview on the same inputs must produce the identical value.
        let form_def: ugoite_domain::form::FormDefinition =
            serde_json::from_value(form.clone()).unwrap();
        let native_draft = ugoite_core::entry::structured_fields_to_draft(
            Some("Note"),
            Vec::new(),
            [
                (
                    "Body".to_string(),
                    serde_json::Value::String("hello".to_string()),
                ),
                ("Done".to_string(), serde_json::Value::Bool(true)),
                ("Count".to_string(), serde_json::Value::Number(42.into())),
            ]
            .into_iter()
            .collect(),
            Default::default(),
        );
        let native =
            ugoite_core::entry::preview_structured_draft(&form_def, &native_draft).unwrap();
        let native_json = serde_json::to_value(native).unwrap();
        assert_eq!(response["value"], native_json);
    }

    #[test]
    fn entry_validate_draft_preserves_field_diagnostics() {
        let form = entry_test_form();
        let draft = serde_json::json!({
            "form_name": "Note",
            "tags": [],
            "fields": {"Body": "hello", "Done": "maybe", "Count": 1}
        });
        let request = serde_json::json!({
            "action": "entry.validate_draft",
            "value": {"form": form.clone(), "draft": draft.clone()}
        })
        .to_string();
        let via_wasm: Value = serde_json::from_str(&super::invoke_json(&request)).unwrap();
        assert_eq!(via_wasm["ok"], false, "{via_wasm}");
        assert_eq!(via_wasm["error"]["kind"], "entry_validation");
        assert_eq!(via_wasm["error"]["code"], "FORM_VALIDATION_FAILED");

        let form_def: ugoite_domain::form::FormDefinition = serde_json::from_value(form).unwrap();
        let native_draft = super::parse_entry_draft(&draft).unwrap();
        let native_err = ugoite_core::entry::preview_structured_draft(&form_def, &native_draft)
            .expect_err("invalid boolean");
        assert_eq!(native_err.code_str(), "FORM_VALIDATION_FAILED");
        assert_eq!(
            via_wasm["error"]["detail"],
            serde_json::to_value(native_err.detail().unwrap()).unwrap()
        );
        let warnings = via_wasm["error"]["detail"]["warnings"]
            .as_array()
            .expect("warnings");
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0]["field"], "Done");
        assert_eq!(warnings[0]["code"], "invalid_type");
    }

    #[test]
    fn entry_validate_draft_rejects_malformed_drafts_as_invalid_input() {
        let form = entry_test_form();
        let malformed = vec![
            serde_json::json!({"form_name": 42, "fields": {}}),
            serde_json::json!({"tags": "not-an-array", "fields": {}}),
            serde_json::json!({"tags": [1, 2], "fields": {}}),
            serde_json::json!({"fields": "oops"}),
            serde_json::json!({"fields": {}, "extra_attributes": "oops"}),
            serde_json::json!({
                "form_name": "A",
                "form": "B",
                "fields": {}
            }),
            serde_json::json!("just-a-string"),
        ];
        for draft in malformed {
            let request = serde_json::json!({
                "action": "entry.validate_draft",
                "value": {"form": form.clone(), "draft": draft}
            })
            .to_string();
            let response: Value = serde_json::from_str(&super::invoke_json(&request)).unwrap();
            assert_eq!(response["ok"], false, "{response}");
            assert_eq!(response["error"]["kind"], "entry_validation", "{response}");
            assert_eq!(response["error"]["code"], "INVALID_INPUT", "{response}");
        }
    }

    #[test]
    fn entry_dispatch_is_exact_and_unknown_future_operations_fall_through() {
        let future: Value = serde_json::from_str(&super::invoke_json(
            r#"{"action":"entry.future_operation","value":{}}"#,
        ))
        .unwrap();
        assert_eq!(future["ok"], false, "{future}");
        assert_eq!(future["error"]["kind"], "invalid_command", "{future}");

        let known: Value = serde_json::from_str(&super::invoke_json(
            r#"{"action":"entry.validate_draft","value":{"draft":{}}}"#,
        ))
        .unwrap();
        assert_eq!(known["ok"], false, "{known}");
        assert_eq!(known["error"]["kind"], "entry_validation", "{known}");
        assert_eq!(known["error"]["code"], "INVALID_INPUT", "{known}");
    }

    #[test]
    fn entry_protocol_exposes_typed_negative_envelopes() {
        let form = entry_test_form();
        let cases = [
            (
                "missing form",
                serde_json::json!({
                    "action": "entry.validate_draft",
                    "value": {"draft": {}}
                }),
                "INVALID_INPUT",
            ),
            (
                "missing draft",
                serde_json::json!({
                    "action": "entry.validate_draft",
                    "value": {"form": form.clone()}
                }),
                "INVALID_INPUT",
            ),
            (
                "malformed payload",
                serde_json::json!({
                    "action": "entry.validate_draft",
                    "value": {"form": form.clone(), "draft": {"fields": []}}
                }),
                "INVALID_INPUT",
            ),
            (
                "unknown form field",
                serde_json::json!({
                    "action": "entry.validate_draft",
                    "value": {
                        "form": form.clone(),
                        "draft": {"fields": {"Body": "ok", "Unknown": "value"}}
                    }
                }),
                "UNKNOWN_FORM_FIELDS",
            ),
            (
                "field validation failure",
                serde_json::json!({
                    "action": "entry.validate_draft",
                    "value": {
                        "form": form.clone(),
                        "draft": {"fields": {"Body": "ok", "Done": "maybe"}}
                    }
                }),
                "FORM_VALIDATION_FAILED",
            ),
        ];
        for (name, request, code) in cases {
            let response: Value =
                serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
            assert_eq!(response["ok"], false, "{name}: {response}");
            assert_eq!(
                response["error"]["kind"], "entry_validation",
                "{name}: {response}"
            );
            assert_eq!(response["error"]["code"], code, "{name}: {response}");
        }
    }

    #[test]
    fn lane1_parity_fixture_agrees_between_native_and_wasm() {
        // Same consolidated Lane 1 fixture the browser, CLI core, and CLI
        // remote converge on: WASM invoke results equal native preview.
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let dir = root.join("../../fixtures/entry/structured-compat");
        let raw =
            std::fs::read_to_string(dir.join("10-structured-authoring-parity.json")).expect("read");
        let fixture: Value = serde_json::from_str(&raw).expect("json");
        let form = fixture["form"].clone();
        let structured = fixture["structured"].clone();

        let validate_request = serde_json::json!({
            "action": "entry.validate_draft",
            "value": {"form": form.clone(), "draft": structured}
        })
        .to_string();
        let validated: Value =
            serde_json::from_str(&super::invoke_json(&validate_request)).unwrap();
        assert_eq!(validated["ok"], true, "{validated}");

        let form_def: ugoite_domain::form::FormDefinition = serde_json::from_value(form).unwrap();
        let draft = super::parse_entry_draft(&fixture["structured"]).unwrap();
        let native = ugoite_core::entry::preview_structured_draft(&form_def, &draft).unwrap();
        assert_eq!(validated["value"], serde_json::to_value(native).unwrap());

        // Invalid logical inputs carry the same code on both sides.
        for case in fixture["invalid_cases"]
            .as_array()
            .cloned()
            .unwrap_or_default()
        {
            let mut fields = std::collections::BTreeMap::new();
            for (key, value) in case["fields"].as_object().cloned().unwrap_or_default() {
                fields.insert(key, value);
            }
            let draft = ugoite_core::entry::structured_fields_to_draft(
                Some("Parity"),
                Vec::new(),
                fields.clone(),
                Default::default(),
            );
            let native_err = ugoite_core::entry::preview_structured_draft(&form_def, &draft)
                .expect_err("must fail");
            let request = serde_json::json!({
                "action": "entry.validate_draft",
                "value": {
                    "form": serde_json::to_value(&form_def).unwrap(),
                    "draft": serde_json::to_value(&draft).unwrap(),
                }
            })
            .to_string();
            let via_wasm: Value = serde_json::from_str(&super::invoke_json(&request)).unwrap();
            assert_eq!(via_wasm["ok"], false, "{via_wasm}");
            assert_eq!(via_wasm["error"]["code"], native_err.code_str());
        }
    }
}
