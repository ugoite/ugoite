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
        "domain.parse_sql_document" => Some(MAX_SQL_DOCUMENT_REQUEST_BYTES),
        "domain.rename_sql_document_parameter" => Some(MAX_SQL_DOCUMENT_RENAME_REQUEST_BYTES),
        "domain.apply_sql_document_edit_transaction" => Some(MAX_SQL_DOCUMENT_EDIT_REQUEST_BYTES),
        "domain.apply_sql_document_replacement_transaction" => {
            Some(MAX_SQL_DOCUMENT_EDIT_REQUEST_BYTES)
        }
        "domain.apply_sql_document_node_edit_transaction" => {
            Some(MAX_SQL_DOCUMENT_EDIT_REQUEST_BYTES)
        }
        _ => None,
    }
}

pub fn invoke_json(input: &str) -> String {
    if input.len() > MAX_PROTOCOL_REQUEST_BYTES {
        // Bound JSON decoding before inspecting the action. The edit
        // transaction envelope is the largest SQL Document request.
        let max_sql_document_request_bytes =
            MAX_SQL_DOCUMENT_RENAME_REQUEST_BYTES.max(MAX_SQL_DOCUMENT_EDIT_REQUEST_BYTES);
        if input.len() <= max_sql_document_request_bytes {
            if let Ok(request) = serde_json::from_str::<serde_json::Value>(input) {
                if let Some(action) = request.get("action").and_then(serde_json::Value::as_str) {
                    if sql_document_request_limit(action).is_some_and(|limit| input.len() <= limit)
                    {
                        return invoke_domain(request);
                    }
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
        Some("domain.rename_sql_document_parameter") => {
            return invoke_sql_document_parameter_rename(request)
        }
        Some("domain.apply_sql_document_edit_transaction") => {
            return invoke_sql_document_edit_transaction(request)
        }
        Some("domain.apply_sql_document_replacement_transaction") => {
            return invoke_sql_document_replacement_transaction(request)
        }
        Some("domain.apply_sql_document_node_edit_transaction") => {
            return invoke_sql_document_node_edit_transaction(request)
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

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SqlDocumentReplacementProtocolRequest {
    current_source: String,
    replacement_source: String,
}

fn invoke_sql_document_replacement_transaction(request: serde_json::Value) -> String {
    let Some(payload) = request.get("value").cloned() else {
        return sql_document_edit_invalid_request();
    };
    let Ok(protocol_request) =
        serde_json::from_value::<SqlDocumentReplacementProtocolRequest>(payload)
    else {
        return sql_document_edit_invalid_request();
    };

    match ugoite_domain::sql_document::apply_sql_document_replacement_transaction(
        &protocol_request.current_source,
        &protocol_request.replacement_source,
    ) {
        Ok(result) => serialize_sql_document_edit_response(&result)
            .unwrap_or_else(|()| sql_document_response_too_large()),
        Err(error) => sql_document_edit_error(error),
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SqlDocumentNodeEditProtocolRequest {
    current_source: String,
    transaction: ugoite_domain::sql_document::SqlDocumentNodeEditTransaction,
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
    fn sql_structure_protocol_preserves_trivia_for_aggregate_and_distinct_edits() {
        let context = serde_json::json!({
            "relation": "Task",
            "columns": [
                {"name": "_ugoite_id", "type": "string"},
                {"name": "field_101", "type": "string"}
            ],
            "variables": [],
            "stable_key": []
        });
        for (sql, operation, expected) in [
            (
                "-- heading\nSELECT /* projection lead */ * FROM \"Task\" /* tail */",
                serde_json::json!({
                    "kind": "set_aggregates",
                    "aggregates": [{
                        "function": "count",
                        "column": null,
                        "arguments": [],
                        "distinct": false,
                        "alias": null
                    }]
                }),
                "-- heading\nSELECT /* projection lead */ COUNT(*) FROM \"Task\" /* tail */",
            ),
            (
                "-- heading\nSELECT /* after select */ \"field_101\" FROM \"Task\" /* tail */",
                serde_json::json!({"kind": "set_distinct", "distinct": true}),
                "-- heading\nSELECT DISTINCT /* after select */ \"field_101\" FROM \"Task\" /* tail */",
            ),
        ] {
            let request = serde_json::json!({
                "action": "domain.propose_sql_structure_operation",
                "value": {"sql": sql, "context": context, "operation": operation}
            });
            let response: Value = serde_json::from_str(&super::invoke_json(&request.to_string()))
                .expect("JSON response");
            assert_eq!(response["ok"], true, "{response}");
            assert_eq!(response["value"]["status"], "supported", "{response}");
            assert_eq!(response["value"]["proposed_sql"], expected);
        }
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
        assert_eq!(analysis["value"]["contract_version"], 3);
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
    fn sql_document_replacement_protocol_preserves_unchanged_source_and_matches_native() {
        let source = "SELECT a /* keep */, b FROM \"G\" WHERE id = 1 -- tail";
        let replacement = "SELECT total_a /* keep */, total_b FROM \"G\" WHERE id = 2 -- tail";
        let native = ugoite_domain::sql_document::apply_sql_document_replacement_transaction(
            source,
            replacement,
        )
        .unwrap();
        let request = serde_json::json!({
            "action": "domain.apply_sql_document_replacement_transaction",
            "value": {
                "current_source": source,
                "replacement_source": replacement,
            },
        });
        let response: Value =
            serde_json::from_str(&super::invoke_json(&request.to_string())).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"], serde_json::to_value(&native).unwrap());
        assert_eq!(response["value"]["document"]["source"], replacement);

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
    fn sql_document_node_edit_protocol_matches_native_typed_transaction() {
        use ugoite_domain::sql_document::{
            parse_sql_document, SqlDocumentNodeEdit, SqlDocumentNodeEditTransaction,
        };

        let source = "SELECT * FROM \"G\" ORDER BY value -- keep trailing comment";
        let document = parse_sql_document(source);
        let item = document
            .nodes
            .iter()
            .find(|node| node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::SelectItem)
            .expect("parser exposes the projection as a typed node");
        let transaction = SqlDocumentNodeEditTransaction {
            contract_version: document.contract_version,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentNodeEdit {
                node_id: item.id,
                expected_kind: item.kind,
                replacement: "value AS selected".to_owned(),
            }],
        };
        let native = ugoite_domain::sql_document::apply_sql_document_node_edit_transaction(
            source,
            &transaction,
        )
        .expect("native node edit succeeds");
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
            "SELECT value AS selected FROM \"G\" ORDER BY value -- keep trailing comment"
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
