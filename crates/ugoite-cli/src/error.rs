use anyhow::Error;
use serde_json::Value;
use ugoite_core::error::AppError;

/// Renders an application error for a CLI user without duplicating validation
/// rules from core. Structured Form validation warnings are already produced
/// by core, so the CLI only turns those values into readable lines.
pub fn format_cli_error(error: &Error) -> String {
    let Some(app_error) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<AppError>())
    else {
        return format!("{error:#}");
    };

    let mut message = app_error.message().to_string();
    let Some(warnings) = app_error
        .detail()
        .and_then(|detail| detail.get("warnings"))
        .and_then(Value::as_array)
    else {
        return message;
    };

    for warning in warnings {
        let Some(line) = format_validation_warning(warning) else {
            continue;
        };
        message.push('\n');
        message.push_str("- ");
        message.push_str(&line);
    }
    message
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
