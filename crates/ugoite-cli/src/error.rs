use anyhow::Error;
use serde_json::Value;
use ugoite_api_client::ApiProtocolError;
use ugoite_core::error::AppError;

/// Renders an application error for a CLI user without duplicating validation
/// rules from core. Structured Form validation warnings are already produced
/// by core, so the CLI only turns those values into readable lines. Remote
/// failures carry the same shared payload without an `AppError`, so protocol
/// errors with a warnings detail render through the same warning formatter.
pub fn format_cli_error(error: &Error) -> String {
    if let Some(app_error) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<AppError>())
    {
        return render_warnings(app_error.message(), app_error.detail());
    }

    if let Some(protocol_error) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<ApiProtocolError>())
    {
        if let Some(rendered) = render_protocol_warnings(protocol_error) {
            return rendered;
        }
    }

    format!("{error:#}")
}

fn render_warnings(message: &str, detail: Option<&Value>) -> String {
    let mut rendered = message.to_string();
    let Some(warnings) = detail
        .and_then(|detail| detail.get("warnings"))
        .and_then(Value::as_array)
    else {
        return rendered;
    };

    for warning in warnings {
        let Some(line) = format_validation_warning(warning) else {
            continue;
        };
        rendered.push('\n');
        rendered.push_str("- ");
        rendered.push_str(&line);
    }
    rendered
}

/// Render remote validation failures with the same lines as core.
///
/// The protocol message embeds the raw shared payload, so the headline comes
/// from the server message when the payload carries one. Returns `None` when
/// the protocol error carries no warnings detail, preserving the existing raw
/// rendering for all other remote failures.
fn render_protocol_warnings(protocol_error: &ApiProtocolError) -> Option<String> {
    let warnings = protocol_error
        .detail
        .as_deref()
        .and_then(|detail| detail.get("warnings"))
        .and_then(Value::as_array)?;
    if warnings.is_empty() {
        return None;
    }
    let headline = protocol_error
        .payload
        .as_deref()
        .and_then(|payload| payload.get("message"))
        .and_then(Value::as_str)
        .filter(|message| !message.trim().is_empty())?;
    Some(render_warnings(headline, protocol_error.detail.as_deref()))
}

fn format_validation_warning(warning: &Value) -> Option<String> {
    let object = warning.as_object()?;
    let field = object.get("field").and_then(Value::as_str);
    let expected = object
        .get("expected_format")
        .and_then(Value::as_str)
        .or_else(|| object.get("expected_type").and_then(Value::as_str));
    let reason = object.get("reason").and_then(Value::as_str);
    let fallback = object.get("message").and_then(Value::as_str);

    match (field, expected, reason) {
        (Some(field), Some(expected), Some(reason)) => {
            Some(format!("{field}: expected {expected}; {reason}"))
        }
        _ => fallback.map(str::to_owned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn warnings_detail() -> Value {
        json!({
            "warnings": [
                {
                    "field": "Count",
                    "expected_type": "a numeric value",
                    "reason": "got lap count as text",
                },
                { "message": "Entry title is required" },
            ],
        })
    }

    fn protocol_error() -> ApiProtocolError {
        ApiProtocolError {
            kind: "invalid_arguments".to_string(),
            message: "Failed to create entry: {\"warnings\":[]}".to_string(),
            operation: Some("entry.create".to_string()),
            status: Some(422),
            detail: Some(Box::new(warnings_detail())),
            payload: Some(Box::new(json!({
                "code": "FORM_VALIDATION_FAILED",
                "message": "Entry form validation failed",
                "detail": warnings_detail(),
            }))),
        }
    }

    #[test]
    fn remote_validation_warnings_render_like_core() {
        let rendered = format_cli_error(&anyhow::Error::from(protocol_error()));
        assert_eq!(
            rendered,
            "Entry form validation failed\n- Count: expected a numeric value; got lap count as text\n- Entry title is required"
        );
    }

    #[test]
    fn remote_and_core_warnings_share_one_rendering() {
        use ugoite_core::error::{AppError, ErrorCode};
        let core = AppError::invalid_input_with_detail(
            ErrorCode::FormValidationFailed,
            "Entry form validation failed",
            warnings_detail(),
        );
        assert_eq!(
            format_cli_error(&anyhow::Error::from(protocol_error())),
            format_cli_error(&anyhow::Error::new(core)),
        );
    }

    #[test]
    fn remote_error_without_warnings_keeps_raw_rendering() {
        let mut error = protocol_error();
        error.detail = None;
        error.payload = None;
        let rendered = format_cli_error(&anyhow::Error::from(error));
        assert!(rendered.contains("Failed to create entry"), "{rendered}");
    }

    #[test]
    fn remote_error_without_server_message_keeps_raw_rendering() {
        let mut error = protocol_error();
        error.payload = Some(Box::new(json!({"code": "FORM_VALIDATION_FAILED"})));
        let rendered = format_cli_error(&anyhow::Error::from(error));
        assert!(rendered.contains("Failed to create entry"), "{rendered}");
    }
}
