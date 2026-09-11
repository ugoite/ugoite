//! Single CLI output/error contract (E0/E1).
//!
//! - `stdout` carries success data only; `stderr` carries diagnostics/errors.
//! - Piped output or explicit `--format json` is the machine schema (JSON).
//! - TTY output without an explicit format is the human rendering.
//! - Business semantics stay in the shared Rust boundary; the CLI only
//!   projects `AppError` and `ApiProtocolError` into one error shape. The CLI
//!   owns no validation taxonomy.
//!
//! Machine error envelope on `stderr` (exit code mapped, `stdout` empty):
//!
//! ```json
//! {"error": {"code": "REVISION_CONFLICT", "kind": "conflict",
//!   "message": "Entry revision is stale",
//!   "detail": {"current_revision_id": "…",
//!     "recovery_action": "reload_and_retry"}}}
//! ```

use std::io::IsTerminal;

use anyhow::Error;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ugoite_api_client::ApiProtocolError;
use ugoite_core::error::{AppError, ErrorKind};

/// Output format for CLI commands.
#[derive(ValueEnum, Clone, Debug, Default, PartialEq)]
pub enum Format {
    /// Pretty-printed JSON (default when piped)
    #[default]
    Json,
    /// Human-readable table (default when stdout is a TTY)
    Table,
    /// Key: value lines for single objects
    Plain,
}

/// Return the effective format: use explicit override, or auto-detect TTY.
pub fn effective_format(explicit: Option<Format>) -> Format {
    effective_format_for_stdout(explicit, std::io::stdout().is_terminal())
}

#[doc(hidden)]
pub fn effective_format_for_stdout(explicit: Option<Format>, stdout_is_terminal: bool) -> Format {
    if let Some(format) = explicit {
        return format;
    }
    if stdout_is_terminal {
        Format::Table
    } else {
        Format::Json
    }
}

pub fn print_json<T: Serialize>(value: &T) {
    let rendered = serde_json::to_string_pretty(value).unwrap_or_default();
    println!("{rendered}");
}

/// Print a list of string IDs as a single-column table.
pub fn print_list_table(header: &str, items: &[impl std::fmt::Display]) {
    let col_width = items
        .iter()
        .map(|item| item.to_string().len())
        .max()
        .unwrap_or(0)
        .max(header.len());
    println!("{header:<col_width$}");
    println!("{}", "-".repeat(col_width));
    for item in items {
        println!("{item}");
    }
}

/// Print a list of JSON objects as a table, selecting the given columns.
/// Columns is a slice of `(header, json_key)` pairs.
pub fn print_json_table(rows: &[Value], columns: &[(&str, &str)]) {
    let mut widths: Vec<usize> = columns.iter().map(|(header, _)| header.len()).collect();
    let cell_matrix: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            columns
                .iter()
                .enumerate()
                .map(|(index, (_, key))| {
                    let cell = match &row[key] {
                        Value::String(text) => text.clone(),
                        Value::Null => String::new(),
                        other => other.to_string(),
                    };
                    widths[index] = widths[index].max(cell.len());
                    cell
                })
                .collect()
        })
        .collect();
    let header: Vec<String> = columns
        .iter()
        .enumerate()
        .map(|(index, (text, _))| format!("{text:<width$}", width = widths[index]))
        .collect();
    println!("{}", header.join("  "));
    let separator: Vec<String> = widths.iter().map(|width| "-".repeat(*width)).collect();
    println!("{}", separator.join("  "));
    for row_cells in &cell_matrix {
        let row: Vec<String> = row_cells
            .iter()
            .enumerate()
            .map(|(index, cell)| format!("{cell:<width$}", width = widths[index]))
            .collect();
        println!("{}", row.join("  "));
    }
}

/// Machine `stderr` when piped (JSON envelope); human text on TTY.
pub fn is_machine_stderr() -> bool {
    !std::io::stderr().is_terminal()
}

/// Emit success data to `stdout` only.
///
/// Machine mode prints pretty JSON. Human mode prints the human rendering
/// when one is supplied, otherwise falls back to JSON so no data is lost.
pub fn emit_success(data: &Value, format: &Format, human: Option<String>) {
    match format {
        Format::Json => print_json(data),
        Format::Table | Format::Plain => {
            if let Some(rendered) = human {
                println!("{rendered}");
            } else {
                print_json(data);
            }
        }
    }
}

/// Stable CLI exit codes.
///
/// Exact failures are identified by `error.code`, never by the exit code
/// alone. The code only separates broad recovery classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    Success = 0,
    Internal = 1,
    Usage = 2,
    Forbidden = 3,
    NotFound = 4,
    Conflict = 5,
    DependencyUnavailable = 6,
    Unsupported = 7,
}

impl ExitCode {
    pub const fn as_i32(self) -> i32 {
        self as i32
    }
}

pub fn exit_code_for_kind(kind: &str) -> ExitCode {
    match kind {
        "invalid_input" | "invalid_arguments" | "invalid_operation" | "invalid_response" => {
            ExitCode::Usage
        }
        "forbidden" => ExitCode::Forbidden,
        "not_found" => ExitCode::NotFound,
        "conflict" => ExitCode::Conflict,
        "expired" => ExitCode::NotFound,
        "dependency_unavailable" => ExitCode::DependencyUnavailable,
        "unimplemented" | "unsupported" => ExitCode::Unsupported,
        _ => ExitCode::Internal,
    }
}

fn exit_code_for_error_kind(kind: ErrorKind) -> ExitCode {
    match kind {
        ErrorKind::InvalidInput => ExitCode::Usage,
        ErrorKind::Forbidden => ExitCode::Forbidden,
        ErrorKind::NotFound => ExitCode::NotFound,
        ErrorKind::Conflict => ExitCode::Conflict,
        ErrorKind::Expired => ExitCode::NotFound,
        ErrorKind::Unimplemented => ExitCode::Unsupported,
        ErrorKind::DependencyUnavailable => ExitCode::DependencyUnavailable,
        ErrorKind::Internal => ExitCode::Internal,
    }
}

/// Projected CLI error shared by core and remote transports.
///
/// Both `AppError` (core) and `ApiProtocolError` (remote) funnel here so
/// validation warnings never render as raw JSON on one transport (fixes the
/// C4 remote-rendering drift without copying validation into the CLI).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CliError {
    pub code: String,
    pub kind: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<Value>,
    #[serde(skip)]
    pub exit: ExitCode,
}

impl CliError {
    pub fn exit_code(&self) -> i32 {
        self.exit.as_i32()
    }

    /// Machine envelope written to `stderr` with an empty `stdout`.
    pub fn envelope(&self) -> Value {
        serde_json::json!({
            "error": {
                "code": self.code,
                "kind": self.kind,
                "message": self.message,
                "detail": self.detail.clone().unwrap_or(Value::Null),
            }
        })
    }

    /// Human rendering for TTY `stderr`. The 409 hint explains reload/retry;
    /// machines read `detail.current_revision_id` instead of parsing text.
    pub fn human(&self) -> String {
        let mut rendered = format!("Error: {}", self.message);
        for line in validation_warning_lines(self.detail.as_ref()) {
            rendered.push('\n');
            rendered.push_str("- ");
            rendered.push_str(&line);
        }
        if self.code == "REVISION_CONFLICT" {
            rendered.push_str(
                "\n- reload the entry to get current_revision_id, then retry with --parent-revision-id",
            );
        }
        rendered
    }
}

/// Usage error (invalid flags/arguments) mapping to exit 2.
///
/// Clap parse failures already exit 2; use this for semantic usage errors
/// detected after parsing (mutually exclusive flags, missing required input,
///
/// mode-restricted flags) so shell/CI can separate usage from runtime failures.
#[derive(Debug)]
pub struct UsageError(pub String);

impl std::fmt::Display for UsageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for UsageError {}

/// Project any CLI failure into the shared error shape.
pub fn project_error(error: &Error) -> CliError {
    if let Some(usage) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<UsageError>())
    {
        return CliError {
            code: "INVALID_INPUT".to_string(),
            kind: "invalid_input".to_string(),
            message: usage.0.clone(),
            detail: None,
            exit: ExitCode::Usage,
        };
    }
    if let Some(app_error) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<AppError>())
    {
        return from_app_error(app_error);
    }
    if let Some(protocol_error) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<ApiProtocolError>())
    {
        return from_protocol_error(protocol_error);
    }
    CliError {
        code: "INTERNAL_ERROR".to_string(),
        kind: "internal".to_string(),
        message: format!("{error:#}"),
        detail: None,
        exit: ExitCode::Internal,
    }
}

/// Legacy human-only rendering kept for callers that format `stderr` text.
pub fn format_cli_error(error: &Error) -> String {
    project_error(error).human().replace("Error: ", "")
}

fn from_app_error(error: &AppError) -> CliError {
    let kind = error_kind_str(error.kind());
    CliError {
        code: error.code_str().to_string(),
        kind: kind.to_string(),
        message: error.message().to_string(),
        detail: error.detail().cloned(),
        exit: exit_code_for_error_kind(error.kind()),
    }
}

fn from_protocol_error(error: &ApiProtocolError) -> CliError {
    // ApiProtocolError carries the server `{code,message,detail}` body as the
    // full `payload`, with `detail` holding just the nested detail object.
    // Read code/message from the full body (never from the stringified detail
    // in `error.message`, which embeds machine objects for 409 recovery).
    let payload: Option<&Value> = error.payload.as_deref();
    let detail_body: Option<&Value> = error.detail.as_deref();
    let code = payload
        .and_then(|body| body.get("code"))
        .and_then(Value::as_str)
        .or_else(|| {
            detail_body
                .and_then(|body| body.get("code"))
                .and_then(Value::as_str)
        })
        .unwrap_or("INTERNAL_ERROR")
        .to_string();
    let message = payload
        .and_then(|body| body.get("message"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| error.message.clone());
    let detail = detail_body
        .cloned()
        .or_else(|| payload.and_then(|body| body.get("detail")).cloned());
    // Compatibility: older readers look for `current_revision_id` at the top
    // level of the payload. The server sends it under `detail`; promote it so
    // both readers agree without breaking either.
    let detail = promote_conflict_detail(detail, payload);
    let kind = protocol_kind(&code, error.kind.as_str());
    CliError {
        code,
        kind: kind.to_string(),
        message,
        detail,
        exit: exit_code_for_kind(kind),
    }
}

fn promote_conflict_detail(detail: Option<Value>, body: Option<&Value>) -> Option<Value> {
    let mut detail = detail;
    let top_level = body
        .and_then(|body| body.get("current_revision_id"))
        .cloned();
    if let (Some(current), Some(object)) =
        (top_level, detail.as_mut().and_then(Value::as_object_mut))
    {
        object.entry("current_revision_id").or_insert(current);
        if !object.contains_key("recovery_action") {
            object.insert(
                "recovery_action".to_string(),
                Value::String("reload_and_retry".to_string()),
            );
        }
    }
    detail
}

fn protocol_kind(code: &str, fallback: &str) -> &'static str {
    match code {
        "INVALID_IDENTIFIER"
        | "INVALID_INPUT"
        | "FORM_VALIDATION_FAILED"
        | "UNKNOWN_FORM_FIELDS"
        | "SEARCH_QUERY_EMPTY"
        | "READ_ONLY_SQL_REQUIRED"
        | "UNSUPPORTED_SPACE_VERSION"
        | "UNSUPPORTED_SPACE_PATCH_FIELD"
        | "FORM_FIELD_TYPE_CHANGE_NOT_SUPPORTED"
        | "FORM_FIELD_REMOVAL_NOT_SUPPORTED" => "invalid_input",
        "FORBIDDEN" | "LAST_ADMIN_REQUIRED" => "forbidden",
        "SPACE_NOT_FOUND"
        | "FORM_NOT_FOUND"
        | "ENTRY_NOT_FOUND"
        | "REVISION_NOT_FOUND"
        | "ASSET_NOT_FOUND"
        | "MEMBER_NOT_FOUND"
        | "INVITATION_NOT_FOUND" => "not_found",
        "REVISION_CONFLICT"
        | "SPACE_ALREADY_EXISTS"
        | "MEMBER_ALREADY_ACTIVE"
        | "ASSET_REFERENCED"
        | "CHECKPOINT_ALREADY_EXISTS"
        | "INVITATION_NOT_PENDING" => "conflict",
        "INVITATION_EXPIRED" | "SQL_SESSION_EXPIRED" => "expired",
        "STORAGE_CONNECTION_FAILED" | "STORAGE_MUTATION_UNAVAILABLE" | "CHECKPOINT_UNAVAILABLE" => {
            "dependency_unavailable"
        }
        _ => match fallback {
            "invalid_arguments" | "invalid_operation" | "invalid_response" | "invalid_input" => {
                "invalid_input"
            }
            "forbidden" => "forbidden",
            "not_found" => "not_found",
            "conflict" => "conflict",
            "expired" => "expired",
            "dependency_unavailable" => "dependency_unavailable",
            "unimplemented" | "unsupported" => "unimplemented",
            "internal" => "internal",
            _ => "internal",
        },
    }
}

fn error_kind_str(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::InvalidInput => "invalid_input",
        ErrorKind::Forbidden => "forbidden",
        ErrorKind::NotFound => "not_found",
        ErrorKind::Conflict => "conflict",
        ErrorKind::Expired => "expired",
        ErrorKind::Unimplemented => "unimplemented",
        ErrorKind::DependencyUnavailable => "dependency_unavailable",
        ErrorKind::Internal => "internal",
    }
}

fn validation_warning_lines(detail: Option<&Value>) -> Vec<String> {
    let Some(warnings) = detail
        .and_then(|detail| {
            // Both transports: core puts warnings under `detail.warnings`,
            // remote protocol bodies nest under `detail.detail.warnings`.
            detail
                .get("warnings")
                .or_else(|| detail.get("detail").and_then(|inner| inner.get("warnings")))
        })
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    warnings
        .iter()
        .filter_map(format_validation_warning)
        .collect()
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

/// Common mutation receipt (E1).
///
/// `change_id`/`run_id` are `None` when the mutation has no Change/Run. The
/// CLI never fabricates them; durable Knowledge Change IDs come from the
/// commit boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MutationReceipt {
    pub kind: String,
    pub id: String,
    #[serde(default)]
    pub revision_id: Option<String>,
    #[serde(default)]
    pub change_id: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
}

impl MutationReceipt {
    pub fn entry(id: String, revision_id: Option<String>, change_id: Option<String>) -> Self {
        Self {
            kind: "entry".to_string(),
            id,
            revision_id,
            change_id,
            run_id: None,
        }
    }

    pub fn value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    pub fn human(&self) -> String {
        let mut lines = vec![format!("{} {}", self.kind, self.id)];
        if let Some(revision) = self.revision_id.as_deref() {
            lines.push(format!("revision: {revision}"));
        }
        if let Some(change) = self.change_id.as_deref() {
            lines.push(format!("change: {change}"));
        }
        if let Some(run) = self.run_id.as_deref() {
            lines.push(format!("run: {run}"));
        }
        lines.join("\n")
    }
}

/// Read a Markdown compatibility ingress from `--file` (E2).
///
/// - `--content`/`--markdown` stays inline.
/// - `--file PATH` reads the file; `--file -` reads explicit stdin.
/// - Inline and file together are rejected deterministically.
/// - Stdin is never consumed implicitly, so output pipes never collide.
pub fn read_compat_input(
    inline: Option<String>,
    inline_flag: &str,
    file: Option<String>,
) -> anyhow::Result<String> {
    match (inline, file) {
        (Some(_), Some(_)) => Err(UsageError(format!(
            "{inline_flag} and --file cannot be combined; specify exactly one"
        ))
        .into()),
        (Some(text), None) => Ok(text),
        (None, Some(path)) if path == "-" => {
            use std::io::Read;
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .map_err(|error| anyhow::anyhow!("read stdin: {error}"))?;
            Ok(text)
        }
        (None, Some(path)) => std::fs::read_to_string(&path)
            .map_err(|error| UsageError(format!("read --file {path}: {error}")).into()),
        (None, None) => Err(UsageError(format!("{inline_flag} or --file is required")).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_envelope_uses_stable_code_and_detail() {
        let error = anyhow::anyhow!(AppError::revision_conflict("rev-9", "rev-8", "rev-9"));
        let projected = project_error(&error);
        assert_eq!(projected.code, "REVISION_CONFLICT");
        assert_eq!(projected.kind, "conflict");
        assert_eq!(projected.exit_code(), 5);
        let envelope = projected.envelope();
        assert_eq!(
            envelope["error"]["detail"]["current_revision_id"],
            serde_json::Value::String("rev-9".to_string())
        );
        assert_eq!(
            envelope["error"]["detail"]["recovery_action"],
            serde_json::Value::String("reload_and_retry".to_string())
        );
        assert!(projected.human().contains("reload"));
    }

    #[test]
    fn protocol_warnings_render_like_core_warnings() {
        let protocol = ApiProtocolError {
            kind: "invalid_arguments".to_string(),
            message: "Failed to create entry".to_string(),
            operation: Some("entry.create".to_string()),
            status: Some(422),
            detail: Some(Box::new(serde_json::json!({
                "code": "FORM_VALIDATION_FAILED",
                "message": "Entry form validation failed",
                "detail": {"warnings": [
                    {"field": "Count", "expected_format": "a numeric value",
                     "reason": "value does not match", "message": "bad"}
                ]},
            }))),
            payload: None,
        };
        let projected = project_error(&anyhow::Error::from(protocol));
        assert_eq!(projected.code, "FORM_VALIDATION_FAILED");
        assert_eq!(projected.exit_code(), 2);
        assert!(projected.human().contains("Count"));
        assert!(!projected.human().contains('{'));
    }

    #[test]
    fn inline_and_file_inputs_are_mutually_exclusive() {
        assert!(
            read_compat_input(Some("a".to_string()), "--content", Some("b.md".to_string()))
                .is_err()
        );
        assert_eq!(
            read_compat_input(Some("a".to_string()), "--content", None).unwrap(),
            "a"
        );
    }
}
