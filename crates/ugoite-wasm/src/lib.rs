//! Raw WebAssembly adapter over Ugoite's portable crates.
//!
//! The browser boundary deliberately uses UTF-8 JSON and a tiny C ABI instead
//! of binding network I/O into WASM. JavaScript performs `fetch`; this crate
//! prepares requests and decodes responses using the same Rust implementation
//! used by the native CLI.

pub use ugoite_api_client as api_client;
pub use ugoite_domain as domain;

pub fn invoke_json(input: &str) -> String {
    ugoite_api_client::invoke_json(input)
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
        assert_eq!(response["ok"], true);
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
}
