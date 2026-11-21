use crate::CapabilityEffect;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ugoite_domain::preview_label::safe_preview_label;

const MAX_PREVIEW_CHARS: usize = 1_200;

/// A bounded, value-masked description of a proposed Knowledge mutation.
/// The request snapshot remains in the host and is never included here.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WritePreview {
    pub request_id: String,
    pub work_id: String,
    pub space_id: String,
    pub operation: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub form: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_id_label: Option<String>,
    pub summary: String,
}

/// Resolve effects only from the known Ugoite capability contract. Metadata
/// cannot downgrade canonical writes, and unknown or unannotated capabilities
/// fail closed.
pub fn effective_capability_effect(
    operation: &str,
    declared: Option<CapabilityEffect>,
) -> Result<CapabilityEffect, String> {
    match operation {
        "ugoite.save" | "ugoite.undo" => Ok(CapabilityEffect::Write),
        "ugoite.search" | "resources/read" => match declared {
            Some(CapabilityEffect::Read) => Ok(CapabilityEffect::Read),
            Some(CapabilityEffect::Write) => {
                Err(format!("listed {operation} capability is not read-only"))
            }
            None => Err(format!(
                "capability {operation} has no trusted read-only effect metadata"
            )),
        },
        _ => Err(format!("unsupported Konase capability {operation}")),
    }
}

/// Build the confirmation preview shared by native and Browser Hosts.
pub fn write_preview(
    request_id: &str,
    work_id: &str,
    space_id: &str,
    operation: &str,
    arguments: &Map<String, Value>,
) -> Result<WritePreview, String> {
    let space_id = safe_preview_label(space_id);
    match operation {
        "ugoite.undo" => Ok(WritePreview {
            request_id: request_id.into(),
            work_id: work_id.into(),
            space_id,
            operation: operation.into(),
            action: "undo".into(),
            form: None,
            entry_id: None,
            entry_id_label: None,
            summary: "Undo the changes saved during this work.".into(),
        }),
        "ugoite.save" => save_preview(request_id, work_id, space_id, arguments),
        _ => Err(format!("write preview is unavailable for {operation}")),
    }
}

fn save_preview(
    request_id: &str,
    work_id: &str,
    space_id: String,
    arguments: &Map<String, Value>,
) -> Result<WritePreview, String> {
    let id = optional_nonempty_string(arguments.get("id"), "id")?;
    let form = optional_nonempty_string(arguments.get("form"), "form")?;
    let fields = arguments
        .get("fields")
        .and_then(Value::as_object)
        .ok_or_else(|| "save fields must be an object".to_string())?;
    if id.is_none() && form.is_none() {
        return Err("new save has no Form target".into());
    }

    let field_summary = fields
        .iter()
        .map(|(name, value)| {
            if is_sensitive_field(name) {
                format!("{}: [hidden]", safe_preview_label(name))
            } else {
                format!("{}: {}", safe_preview_label(name), value_summary(value))
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let mut parts = vec![format!(
        "Fields ({}): {field_summary}.",
        if id.is_some() {
            "replace the complete structured field map"
        } else {
            "new Entry values"
        }
    )];
    if let Some(tags) = arguments.get("tags").and_then(Value::as_array) {
        if !tags.is_empty() {
            parts.push(format!("Tags: {} tag(s).", tags.len()));
        }
    }
    if arguments
        .get("extra_attributes")
        .and_then(Value::as_object)
        .is_some_and(|values| !values.is_empty())
    {
        parts.push("Extra attributes: present.".into());
    }
    let summary = bounded_summary(parts.join(" "));
    let entry_id = id.map(str::to_owned);
    let form = form.map(safe_preview_label);
    let entry_id_label = entry_id.as_deref().map(safe_preview_label);
    Ok(WritePreview {
        request_id: request_id.into(),
        work_id: work_id.into(),
        space_id,
        operation: "ugoite.save".into(),
        action: if entry_id.is_some() {
            "update"
        } else {
            "create"
        }
        .into(),
        form,
        entry_id,
        entry_id_label,
        summary,
    })
}

fn optional_nonempty_string<'a>(
    value: Option<&'a Value>,
    name: &str,
) -> Result<Option<&'a str>, String> {
    match value {
        None => Ok(None),
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(Some(value)),
        Some(_) => Err(format!("save {name} must be a non-empty string")),
    }
}

/// Validate the evidence required before a successful mutation is reported.
pub fn mutation_receipt_is_valid(operation: &str, work_id: &str, value: &Value) -> bool {
    match operation {
        "ugoite.save" => {
            let id = value.get("id").and_then(Value::as_str);
            id.is_some_and(|id| !id.is_empty())
                && id.is_some_and(|id| {
                    value.get("uri").and_then(Value::as_str)
                        == Some(format!("ugoite://entry/{id}").as_str())
                })
                && matches!(
                    value.get("status").and_then(Value::as_str),
                    Some("created" | "updated")
                )
                && value.get("_untrusted_content").and_then(Value::as_bool) == Some(true)
        }
        "ugoite.undo" => {
            value.get("run_id").and_then(Value::as_str) == Some(work_id)
                && value
                    .get("reverted_change_count")
                    .and_then(Value::as_u64)
                    .is_some()
                && value.get("_untrusted_content").and_then(Value::as_bool) == Some(true)
        }
        _ => false,
    }
}

fn is_sensitive_field(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        "api_key",
        "api key",
        "token",
        "credential",
        "password",
        "secret",
    ]
    .iter()
    .any(|needle| name.contains(needle))
}

fn value_summary(value: &Value) -> String {
    match value {
        Value::Null => "empty".into(),
        Value::Bool(_) => "boolean".into(),
        Value::Number(_) => "number".into(),
        Value::String(value) => format!("text ({} chars)", value.chars().count()),
        Value::Array(values) => format!("list ({} items)", values.len()),
        Value::Object(values) => format!("object ({} fields)", values.len()),
    }
}

fn bounded_summary(summary: String) -> String {
    let mut characters = summary.chars();
    let prefix = characters
        .by_ref()
        .take(MAX_PREVIEW_CHARS - 11)
        .collect::<String>();
    if characters.next().is_none() {
        summary
    } else {
        format!("{prefix}… [omitted]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn write_preview_masks_values_and_bounds_untrusted_labels() {
        let args = json!({
            "id": "entry-1", "form": "Profile\nForm",
            "fields": {"password": "do not show", "Body": "private body"},
            "tags": ["a", "b"]
        });
        let preview = write_preview(
            "request-1",
            "work-1",
            "space-1",
            "ugoite.save",
            args.as_object().unwrap(),
        )
        .unwrap();
        assert_eq!(preview.action, "update");
        assert!(preview.summary.contains("password: [hidden]"));
        assert!(preview.summary.contains("Body: text (12 chars)"));
        assert!(!preview.summary.contains("private body"));
        assert!(!preview.form.as_deref().unwrap().contains('\n'));
    }

    #[test]
    fn effect_and_receipt_contract_fail_closed() {
        assert_eq!(
            effective_capability_effect("ugoite.save", Some(CapabilityEffect::Read)),
            Ok(CapabilityEffect::Write)
        );
        assert!(effective_capability_effect("ugoite.search", None).is_err());
        assert_eq!(
            effective_capability_effect("ugoite.search", Some(CapabilityEffect::Read)),
            Ok(CapabilityEffect::Read)
        );
        assert!(mutation_receipt_is_valid(
            "ugoite.save",
            "work-1",
            &json!({"id":"e1", "uri":"ugoite://entry/e1", "status":"created", "_untrusted_content":true})
        ));
        assert!(!mutation_receipt_is_valid(
            "ugoite.save",
            "work-1",
            &json!({"id":"e1", "uri":"https://example/e1", "status":"created", "_untrusted_content":true})
        ));
        assert!(mutation_receipt_is_valid(
            "ugoite.undo",
            "work-1",
            &json!({"run_id":"work-1", "reverted_change_count":0, "_untrusted_content":true})
        ));
        assert!(!mutation_receipt_is_valid(
            "ugoite.undo",
            "work-1",
            &json!({"run_id":"other", "reverted_change_count":0, "_untrusted_content":true})
        ));
    }
}
