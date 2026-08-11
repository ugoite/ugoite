//! Raw WebAssembly adapter over Ugoite's portable crates.
//!
//! The browser boundary deliberately uses UTF-8 JSON and a tiny C ABI instead
//! of binding network I/O into WASM. JavaScript performs `fetch`; this crate
//! prepares requests and decodes responses using the same Rust implementation
//! used by the native CLI.

pub use ugoite_api_client as api_client;
pub use ugoite_domain as domain;

pub fn invoke_json(input: &str) -> String {
    if let Ok(request) = serde_json::from_str::<serde_json::Value>(input) {
        if request
            .get("action")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|action| action.starts_with("domain."))
        {
            return invoke_domain(request);
        }
    }
    ugoite_api_client::invoke_json(input)
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
            r#"{"action":"domain.build_revision_draft","value":{"form":{"id":"00000000-0000-0000-0000-000000000001","version":1,"name":"Task","fields":[],"allow_extra_attributes":false},"draft":{"form_id":"00000000-0000-0000-0000-000000000001","entry_id":"00000000-0000-0000-0000-000000000002","revision_id":"00000000-0000-0000-0000-000000000003","operation":"upsert","committed_at_micros":1,"author_id":"human:owner","form_version":1,"source_kind":"wasm","source_id":null,"values":{}},"current":null}}"#,
        );
        let response: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"]["entry_version"], 1);
        assert_eq!(response["value"]["expected_version"], Value::Null);
        assert_eq!(response["value"]["parent_revision_id"], Value::Null);
        assert_eq!(response["value"]["entry"]["updated_by"], "human:owner");
    }
}
