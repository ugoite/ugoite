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
            if action.starts_with("entry.") {
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
    // Accept the canonical StructuredEntryDraft shape, tolerating the
    // frontend shorthand `form` for `form_name` and omitted collections.
    // Present-but-malformed values are INVALID_INPUT so diagnostics parity
    // holds where it matters most (wrong code/detail is worse than strict).
    let title = match object.get("title") {
        None | Some(serde_json::Value::Null) => String::new(),
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(_) => return Err("draft.title must be a string".to_string()),
    };
    let form_name_value = object.get("form_name");
    let form_alias_value = object.get("form");
    for candidate in [form_name_value, form_alias_value].into_iter().flatten() {
        if !candidate.is_null() && candidate.as_str().is_none() {
            return Err("draft.form must be a string".to_string());
        }
    }
    let form_name_text = form_name_value
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let form_alias_text = form_alias_value
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    if let (Some(canonical), Some(alias)) = (&form_name_text, &form_alias_text) {
        if canonical != alias {
            return Err("draft.form_name and draft.form must agree".to_string());
        }
    }
    let form_name = form_name_text.or(form_alias_text);
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
    let extra_key = if object.contains_key("extra_attributes") {
        "extra_attributes"
    } else {
        "extraAttributes"
    };
    if object.contains_key("extra_attributes") && object.contains_key("extraAttributes") {
        let canonical = &object["extra_attributes"];
        let alias = &object["extraAttributes"];
        if canonical != alias {
            return Err("draft.extra_attributes and draft.extraAttributes must agree".to_string());
        }
    }
    let extra_attributes = match object.get(extra_key) {
        None | Some(serde_json::Value::Null) => Default::default(),
        Some(serde_json::Value::Object(map)) => {
            map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        }
        Some(_) => return Err("draft.extra_attributes must be an object".to_string()),
    };
    Ok(ugoite_core::entry::StructuredEntryDraft {
        title,
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

/// Portable Entry authoring boundary (read-only, no Storage).
///
/// - `entry.validate_draft` validates `{form, draft}` with the same
///   `preview_structured_draft` implementation used natively, preserving
///   `code` / `detail` so browser diagnostics match server mutations.
/// - `entry.compat.parse_markdown` converts legacy Markdown to a
///   `StructuredEntryDraft` via `legacy_markdown_to_draft` (no WASM parser).
/// - `entry.compat.render_markdown` converts `{form, draft}` back to the
///   current 0.1 representation via `draft_to_legacy_representation`.
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
            "entry.compat.parse_markdown" => {
                let (markdown, fallback_title) = if let Some(text) = payload.as_str() {
                    (text.to_string(), String::new())
                } else {
                    let markdown = payload
                        .get("markdown")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| serde_json::json!({"kind": "entry_validation", "code": "INVALID_INPUT", "message": "markdown is required"}))?
                        .to_string();
                    let fallback = payload
                        .get("fallback_title")
                        .or_else(|| payload.get("fallbackTitle"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    (markdown, fallback)
                };
                let draft =
                    ugoite_core::entry::legacy_markdown_to_draft(&markdown, &fallback_title);
                serde_json::to_value(draft)
                    .map_err(|error| serde_json::json!({"kind": "entry_validation", "code": "INVALID_INPUT", "message": error.to_string()}))
            }
            "entry.compat.render_markdown" => {
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
                let markdown = ugoite_core::entry::draft_to_legacy_representation(&form, &draft);
                Ok(serde_json::json!({"markdown": markdown}))
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
        Err(error) => serde_json::json!({"ok": false, "error": error}).to_string(),
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
            "title": "Title",
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
            "Title",
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
            "form": "Note",
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
            serde_json::json!({"title": 42, "fields": {}}),
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
    fn entry_compat_parse_render_round_trips_through_canonical_boundary() {
        let form = entry_test_form();
        let markdown =
            "---\nform: Note\n---\n# Title\n\n## Body\n\nhello\n\n## Done\nyes\n\n## Count\n42\n";
        let parse_request = serde_json::json!({
            "action": "entry.compat.parse_markdown",
            "value": {"markdown": markdown, "fallback_title": "fallback"}
        })
        .to_string();
        let parsed: Value = serde_json::from_str(&super::invoke_json(&parse_request)).unwrap();
        assert_eq!(parsed["ok"], true, "{parsed}");
        assert_eq!(parsed["value"]["title"], "Title");
        assert_eq!(parsed["value"]["form_name"], "Note");
        // No WASM-only parser: result equals the native Rust adapter output.
        let native_draft = ugoite_core::entry::legacy_markdown_to_draft(markdown, "fallback");
        assert_eq!(
            parsed["value"],
            serde_json::to_value(native_draft.clone()).unwrap()
        );

        let render_request = serde_json::json!({
            "action": "entry.compat.render_markdown",
            "value": {"form": form.clone(), "draft": parsed["value"]}
        })
        .to_string();
        let rendered: Value = serde_json::from_str(&super::invoke_json(&render_request)).unwrap();
        assert_eq!(rendered["ok"], true, "{rendered}");
        let rendered_markdown = rendered["value"]["markdown"].as_str().unwrap();

        // parse(render(draft)) normalizes to the same canonical value.
        let form_def: ugoite_domain::form::FormDefinition = serde_json::from_value(form).unwrap();
        let first = ugoite_core::entry::preview_structured_draft(
            &form_def,
            &super::parse_entry_draft(&parsed["value"]).unwrap(),
        )
        .unwrap();
        let reparsed = ugoite_core::entry::legacy_markdown_to_draft(rendered_markdown, "fallback");
        let second = ugoite_core::entry::preview_structured_draft(&form_def, &reparsed).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn entry_compat_fixtures_agree_between_native_and_wasm() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let dir = root.join("../../fixtures/entry/structured-compat");
        let raw = std::fs::read_to_string(dir.join("01-core-scalars.json")).expect("read");
        let fixture: Value = serde_json::from_str(&raw).expect("json");
        let form = fixture["form"].clone();
        let markdown = fixture["markdown"].as_str().expect("markdown");

        // WASM parse must equal native parse; WASM validate must equal native.
        let parse_request = serde_json::json!({
            "action": "entry.compat.parse_markdown",
            "value": {"markdown": markdown, "fallback_title": "fallback"}
        })
        .to_string();
        let parsed: Value = serde_json::from_str(&super::invoke_json(&parse_request)).unwrap();
        assert_eq!(parsed["ok"], true, "{parsed}");
        let native_draft = ugoite_core::entry::legacy_markdown_to_draft(markdown, "fallback");
        assert_eq!(parsed["value"], serde_json::to_value(native_draft).unwrap());

        let validate_request = serde_json::json!({
            "action": "entry.validate_draft",
            "value": {"form": form.clone(), "draft": parsed["value"]}
        })
        .to_string();
        let validated: Value =
            serde_json::from_str(&super::invoke_json(&validate_request)).unwrap();
        assert_eq!(validated["ok"], true, "{validated}");

        let form_def: ugoite_domain::form::FormDefinition = serde_json::from_value(form).unwrap();
        let draft = super::parse_entry_draft(&parsed["value"]).unwrap();
        let native = ugoite_core::entry::preview_structured_draft(&form_def, &draft).unwrap();
        assert_eq!(validated["value"], serde_json::to_value(native).unwrap());

        // Existing 0.1 representation is preserved: render then re-parse keeps
        // the same normalized values (no silent rewrite of stored Entries).
        let render_request = serde_json::json!({
            "action": "entry.compat.render_markdown",
            "value": {"form": fixture["form"], "draft": parsed["value"]}
        })
        .to_string();
        let rendered: Value = serde_json::from_str(&super::invoke_json(&render_request)).unwrap();
        assert_eq!(rendered["ok"], true, "{rendered}");
        let reparsed = ugoite_core::entry::legacy_markdown_to_draft(
            rendered["value"]["markdown"].as_str().unwrap(),
            "fallback",
        );
        let renormalized =
            ugoite_core::entry::preview_structured_draft(&form_def, &reparsed).unwrap();
        let draft_again = super::parse_entry_draft(&parsed["value"]).unwrap();
        let original =
            ugoite_core::entry::preview_structured_draft(&form_def, &draft_again).unwrap();
        assert_eq!(original.values, renormalized.values);
    }
}
