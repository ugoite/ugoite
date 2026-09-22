//! Raw WebAssembly adapter over Ugoite's portable crates.
//!
//! The browser boundary deliberately uses UTF-8 JSON and a tiny C ABI instead
//! of binding network I/O into WASM. JavaScript performs `fetch`; this crate
//! prepares requests and decodes responses using the same Rust implementation
//! used by the native CLI.

pub use ugoite_api_client as api_client;
pub use ugoite_core as core;
pub use ugoite_domain as domain;
pub use ugoite_konase as konase;

const MAX_PROTOCOL_REQUEST_BYTES: usize = 256 * 1024;
const ENTRY_OPERATIONS: &[&str] = &["entry.validate_draft"];

fn is_entry_operation(action: &str) -> bool {
    ENTRY_OPERATIONS.contains(&action)
}

pub fn invoke_json(input: &str) -> String {
    if input.len() > MAX_PROTOCOL_REQUEST_BYTES {
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
            "message": "JSON protocol input exceeds the 256 KiB limit",
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

    #[test]
    fn test_api_req_api_001_wasm_adapter_exposes_protocol_version() {
        let response = super::invoke_json(r#"{"action":"version"}"#);
        let response: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"]["protocol_version"], 1);
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
        assert_eq!(version_response["value"]["protocol_version"], 2);

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
                        "safety_hints": ["save only after confirmation"]
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
            "title": "T",
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
            serde_json::json!({"title": "T", "tags": "not-an-array", "fields": {}}),
            serde_json::json!({"title": "T", "tags": [1, 2], "fields": {}}),
            serde_json::json!({"title": "T", "fields": "oops"}),
            serde_json::json!({"title": "T", "fields": {}, "extra_attributes": "oops"}),
            serde_json::json!({
                "title": "T",
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
