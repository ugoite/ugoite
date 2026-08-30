//! Raw WebAssembly adapter over Ugoite's portable crates.
//!
//! The browser boundary deliberately uses UTF-8 JSON and a tiny C ABI instead
//! of binding network I/O into WASM. JavaScript performs `fetch`; this crate
//! prepares requests and decodes responses using the same Rust implementation
//! used by the native CLI.

pub use ugoite_api_client as api_client;
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
}
