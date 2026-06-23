//! Portable Ugoite HTTP protocol client.
//!
//! This crate owns the parts of remote API access that must be identical in
//! native CLI and browser/WASM clients: operation names, HTTP methods, encoded
//! paths and queries, JSON request bodies, authentication intent, and response
//! / error decoding. It intentionally does not perform network I/O.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::fmt;
use url::Url;

pub const PROTOCOL_VERSION: u32 = 1;

/// Stable operation names understood by both native and WASM adapters.
pub const SUPPORTED_OPERATIONS: &[&str] = &[
    "auth.get_config",
    "auth.get_session",
    "auth.login",
    "auth.mock_oauth",
    "auth.clear_session",
    "preferences.get",
    "preferences.patch",
    "space.list",
    "space.create",
    "space.get",
    "space.patch",
    "space.test_connection",
    "space.members.list",
    "space.members.invite",
    "space.members.accept",
    "space.members.update_role",
    "space.members.revoke",
    "form.list_types",
    "form.list",
    "form.get",
    "form.upsert",
    "entry.list",
    "entry.get",
    "entry.create",
    "entry.update",
    "entry.delete",
    "entry.history",
    "entry.revision",
    "entry.restore",
    "entry.options",
    "search.keyword",
    "search.query",
    "sql.list",
    "sql.get",
    "sql.create",
    "sql.update",
    "sql.delete",
    "sql_session.create",
    "sql_session.get",
    "sql_session.count",
    "sql_session.rows",
    "asset.list",
    "asset.upload",
    "asset.delete",
];

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl HttpMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestBodyKind {
    None,
    Json,
    Multipart,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    Standard,
    DevProxy,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PreparedRequest {
    pub operation: String,
    pub method: HttpMethod,
    pub path: String,
    pub headers: Vec<Header>,
    pub body: Option<String>,
    pub body_kind: RequestBodyKind,
    pub auth_mode: AuthMode,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ApiResponse {
    pub status: u16,
    #[serde(default)]
    pub status_text: String,
    #[serde(default)]
    pub headers: Vec<Header>,
    #[serde(default)]
    pub body: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ApiProtocolError {
    pub kind: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<Box<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Box<Value>>,
}

impl ApiProtocolError {
    fn invalid_arguments(operation: &str, message: impl Into<String>) -> Self {
        Self {
            kind: "invalid_arguments".to_string(),
            message: message.into(),
            operation: Some(operation.to_string()),
            status: None,
            detail: None,
            payload: None,
        }
    }

    fn invalid_operation(operation: &str) -> Self {
        Self {
            kind: "invalid_operation".to_string(),
            message: format!("Unknown Ugoite API operation: {operation}"),
            operation: Some(operation.to_string()),
            status: None,
            detail: None,
            payload: None,
        }
    }

    fn invalid_response(operation: &str, status: u16, message: impl Into<String>) -> Self {
        Self {
            kind: "invalid_response".to_string(),
            message: message.into(),
            operation: Some(operation.to_string()),
            status: Some(status),
            detail: None,
            payload: None,
        }
    }
}

impl fmt::Display for ApiProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ApiProtocolError {}

#[derive(Clone, Copy)]
struct OperationSpec {
    method: HttpMethod,
    failure_context: &'static str,
    auth_mode: AuthMode,
    body_kind: RequestBodyKind,
}

impl OperationSpec {
    const fn get(failure_context: &'static str) -> Self {
        Self {
            method: HttpMethod::Get,
            failure_context,
            auth_mode: AuthMode::Standard,
            body_kind: RequestBodyKind::None,
        }
    }

    const fn json(method: HttpMethod, failure_context: &'static str) -> Self {
        Self {
            method,
            failure_context,
            auth_mode: AuthMode::Standard,
            body_kind: RequestBodyKind::Json,
        }
    }

    const fn no_body(method: HttpMethod, failure_context: &'static str) -> Self {
        Self {
            method,
            failure_context,
            auth_mode: AuthMode::Standard,
            body_kind: RequestBodyKind::None,
        }
    }

    const fn dev_json(failure_context: &'static str) -> Self {
        Self {
            method: HttpMethod::Post,
            failure_context,
            auth_mode: AuthMode::DevProxy,
            body_kind: RequestBodyKind::Json,
        }
    }

    const fn multipart(failure_context: &'static str) -> Self {
        Self {
            method: HttpMethod::Post,
            failure_context,
            auth_mode: AuthMode::Standard,
            body_kind: RequestBodyKind::Multipart,
        }
    }
}

/// Build one transport-neutral request from a stable operation name.
pub fn prepare_request(
    operation: &str,
    arguments: &Value,
    body: Option<&Value>,
) -> Result<PreparedRequest, ApiProtocolError> {
    let args = arguments.as_object().ok_or_else(|| {
        ApiProtocolError::invalid_arguments(operation, "operation arguments must be a JSON object")
    })?;

    let (spec, segments, query): (OperationSpec, Vec<String>, Vec<(String, String)>) =
        match operation {
            "auth.get_config" => (
                OperationSpec::get("Failed to load auth config"),
                vec!["auth".into(), "config".into()],
                vec![],
            ),
            "auth.get_session" => (
                OperationSpec::get("Failed to load auth session"),
                vec!["auth".into(), "session".into()],
                vec![],
            ),
            "auth.login" => (
                OperationSpec::dev_json("Failed to log in"),
                vec!["auth".into(), "login".into()],
                vec![],
            ),
            "auth.mock_oauth" => (
                OperationSpec::dev_json("Failed to start mock OAuth login"),
                vec!["auth".into(), "mock-oauth".into()],
                vec![],
            ),
            "auth.clear_session" => (
                OperationSpec::no_body(HttpMethod::Delete, "Failed to clear auth session"),
                vec!["auth".into(), "session".into()],
                vec![],
            ),

            "preferences.get" => (
                OperationSpec::get("Failed to load preferences"),
                vec!["preferences".into(), "me".into()],
                vec![],
            ),
            "preferences.patch" => (
                OperationSpec::json(HttpMethod::Patch, "Failed to update preferences"),
                vec!["preferences".into(), "me".into()],
                vec![],
            ),

            "space.list" => (
                OperationSpec::get("Failed to list spaces"),
                vec!["spaces".into()],
                vec![],
            ),
            "space.create" => (
                OperationSpec::json(HttpMethod::Post, "Failed to create space"),
                vec!["spaces".into()],
                vec![],
            ),
            "space.get" => (
                OperationSpec::get("Failed to get space"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                ],
                vec![],
            ),
            "space.patch" => (
                OperationSpec::json(HttpMethod::Patch, "Failed to patch space"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                ],
                vec![],
            ),
            "space.test_connection" => (
                OperationSpec::json(HttpMethod::Post, "Failed to test connection"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "test-connection".into(),
                ],
                vec![],
            ),
            "space.members.list" => (
                OperationSpec::get("Failed to list members"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "members".into(),
                ],
                vec![],
            ),
            "space.members.invite" => (
                OperationSpec::json(HttpMethod::Post, "Failed to invite member"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "members".into(),
                    "invitations".into(),
                ],
                vec![],
            ),
            "space.members.accept" => (
                OperationSpec::json(HttpMethod::Post, "Failed to accept invitation"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "members".into(),
                    "accept".into(),
                ],
                vec![],
            ),
            "space.members.update_role" => (
                OperationSpec::json(HttpMethod::Post, "Failed to update role"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "members".into(),
                    required_string(operation, args, "member_user_id")?,
                    "role".into(),
                ],
                vec![],
            ),
            "space.members.revoke" => (
                OperationSpec::no_body(HttpMethod::Delete, "Failed to revoke member"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "members".into(),
                    required_string(operation, args, "member_user_id")?,
                ],
                vec![],
            ),

            "form.list_types" => (
                OperationSpec::get("Failed to list form types"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "forms".into(),
                    "types".into(),
                ],
                vec![],
            ),
            "form.list" => (
                OperationSpec::get("Failed to list forms"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "forms".into(),
                ],
                vec![],
            ),
            "form.get" => (
                OperationSpec::get("Failed to get form"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "forms".into(),
                    required_string(operation, args, "form_name")?,
                ],
                vec![],
            ),
            "form.upsert" => (
                OperationSpec::json(HttpMethod::Post, "Failed to create form"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "forms".into(),
                ],
                vec![],
            ),

            "entry.list" => (
                OperationSpec::get("Failed to list entries"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "entries".into(),
                ],
                vec![],
            ),
            "entry.get" => (
                OperationSpec::get("Failed to get entry"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "entries".into(),
                    required_string(operation, args, "entry_id")?,
                ],
                vec![],
            ),
            "entry.create" => (
                OperationSpec::json(HttpMethod::Post, "Failed to create entry"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "entries".into(),
                ],
                vec![],
            ),
            "entry.update" => (
                OperationSpec::json(HttpMethod::Put, "Failed to update entry"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "entries".into(),
                    required_string(operation, args, "entry_id")?,
                ],
                vec![],
            ),
            "entry.delete" => {
                let mut query = Vec::new();
                if optional_bool(operation, args, "hard_delete")?.unwrap_or(false) {
                    query.push(("hard_delete".into(), "true".into()));
                }
                (
                    OperationSpec::no_body(HttpMethod::Delete, "Failed to delete entry"),
                    vec![
                        "spaces".into(),
                        required_string(operation, args, "space_id")?,
                        "entries".into(),
                        required_string(operation, args, "entry_id")?,
                    ],
                    query,
                )
            }
            "entry.history" => (
                OperationSpec::get("Failed to get entry history"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "entries".into(),
                    required_string(operation, args, "entry_id")?,
                    "history".into(),
                ],
                vec![],
            ),
            "entry.revision" => (
                OperationSpec::get("Failed to get entry revision"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "entries".into(),
                    required_string(operation, args, "entry_id")?,
                    "history".into(),
                    required_string(operation, args, "revision_id")?,
                ],
                vec![],
            ),
            "entry.restore" => (
                OperationSpec::json(HttpMethod::Post, "Failed to restore entry"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "entries".into(),
                    required_string(operation, args, "entry_id")?,
                    "restore".into(),
                ],
                vec![],
            ),
            "entry.options" => {
                let mut query = vec![
                    ("form".into(), required_string(operation, args, "form")?),
                    (
                        "limit".into(),
                        required_u64(operation, args, "limit")?.to_string(),
                    ),
                ];
                if let Some(value) = optional_string(operation, args, "q")? {
                    if !value.trim().is_empty() {
                        query.push(("q".into(), value.trim().to_string()));
                    }
                }
                (
                    OperationSpec::get("Failed to load row_reference options"),
                    vec![
                        "spaces".into(),
                        required_string(operation, args, "space_id")?,
                        "entries".into(),
                        "options".into(),
                    ],
                    query,
                )
            }

            "search.keyword" => (
                OperationSpec::get("Failed to search entries"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "search".into(),
                ],
                vec![("q".into(), required_string(operation, args, "q")?)],
            ),
            "search.query" => (
                OperationSpec::json(HttpMethod::Post, "Failed to query space"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "query".into(),
                ],
                vec![],
            ),

            "sql.list" => (
                OperationSpec::get("Failed to list saved SQL"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "sql".into(),
                ],
                vec![],
            ),
            "sql.get" => (
                OperationSpec::get("Failed to get saved SQL"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "sql".into(),
                    required_string(operation, args, "sql_id")?,
                ],
                vec![],
            ),
            "sql.create" => (
                OperationSpec::json(HttpMethod::Post, "Failed to create saved SQL"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "sql".into(),
                ],
                vec![],
            ),
            "sql.update" => (
                OperationSpec::json(HttpMethod::Put, "Failed to update saved SQL"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "sql".into(),
                    required_string(operation, args, "sql_id")?,
                ],
                vec![],
            ),
            "sql.delete" => (
                OperationSpec::no_body(HttpMethod::Delete, "Failed to delete saved SQL"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "sql".into(),
                    required_string(operation, args, "sql_id")?,
                ],
                vec![],
            ),

            "sql_session.create" => (
                OperationSpec::json(HttpMethod::Post, "Failed to create SQL session"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "sql-sessions".into(),
                ],
                vec![],
            ),
            "sql_session.get" => (
                OperationSpec::get("Failed to load SQL session"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "sql-sessions".into(),
                    required_string(operation, args, "session_id")?,
                ],
                vec![],
            ),
            "sql_session.count" => (
                OperationSpec::get("Failed to load SQL session count"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "sql-sessions".into(),
                    required_string(operation, args, "session_id")?,
                    "count".into(),
                ],
                vec![],
            ),
            "sql_session.rows" => (
                OperationSpec::get("Failed to load SQL session rows"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "sql-sessions".into(),
                    required_string(operation, args, "session_id")?,
                    "rows".into(),
                ],
                vec![
                    (
                        "offset".into(),
                        required_u64(operation, args, "offset")?.to_string(),
                    ),
                    (
                        "limit".into(),
                        required_u64(operation, args, "limit")?.to_string(),
                    ),
                ],
            ),

            "asset.list" => (
                OperationSpec::get("Failed to list assets"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "assets".into(),
                ],
                vec![],
            ),
            "asset.upload" => (
                OperationSpec::multipart("Failed to upload asset"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "assets".into(),
                ],
                vec![],
            ),
            "asset.delete" => (
                OperationSpec::no_body(HttpMethod::Delete, "Failed to delete asset"),
                vec![
                    "spaces".into(),
                    required_string(operation, args, "space_id")?,
                    "assets".into(),
                    required_string(operation, args, "asset_id")?,
                ],
                vec![],
            ),

            _ => return Err(ApiProtocolError::invalid_operation(operation)),
        };

    let path = encoded_path(&segments, &query).map_err(|message| {
        ApiProtocolError::invalid_arguments(operation, format!("failed to build URL: {message}"))
    })?;

    let (headers, serialized_body) = match spec.body_kind {
        RequestBodyKind::None => {
            if body.is_some() {
                return Err(ApiProtocolError::invalid_arguments(
                    operation,
                    "operation does not accept a JSON body",
                ));
            }
            (Vec::new(), None)
        }
        RequestBodyKind::Multipart => {
            if body.is_some() {
                return Err(ApiProtocolError::invalid_arguments(
                    operation,
                    "multipart body must be supplied by the runtime transport",
                ));
            }
            (Vec::new(), None)
        }
        RequestBodyKind::Json => {
            let payload = body.ok_or_else(|| {
                ApiProtocolError::invalid_arguments(operation, "operation requires a JSON body")
            })?;
            let serialized = serde_json::to_string(payload).map_err(|error| {
                ApiProtocolError::invalid_arguments(
                    operation,
                    format!("request body is not serializable JSON: {error}"),
                )
            })?;
            (
                vec![Header {
                    name: "content-type".to_string(),
                    value: "application/json".to_string(),
                }],
                Some(serialized),
            )
        }
    };

    Ok(PreparedRequest {
        operation: operation.to_string(),
        method: spec.method,
        path,
        headers,
        body: serialized_body,
        body_kind: spec.body_kind,
        auth_mode: spec.auth_mode,
    })
}

/// Decode one transport response with the same semantics in CLI and browser.
pub fn decode_response(operation: &str, response: ApiResponse) -> Result<Value, ApiProtocolError> {
    let spec =
        operation_spec(operation).ok_or_else(|| ApiProtocolError::invalid_operation(operation))?;
    let body = response.body.trim();
    let parsed = if body.is_empty() {
        None
    } else {
        match serde_json::from_str::<Value>(body) {
            Ok(value) => Some(value),
            Err(error) => {
                if looks_like_html(&response.headers, body) {
                    return Err(ApiProtocolError::invalid_response(
                        operation,
                        response.status,
                        "API endpoint returned HTML instead of JSON. If this is the single-image app root, configure the client with an API base ending in `/api`.",
                    ));
                }
                if (200..300).contains(&response.status) {
                    return Err(ApiProtocolError::invalid_response(
                        operation,
                        response.status,
                        format!(
                            "{}: response was not valid JSON: {error}",
                            spec.failure_context
                        ),
                    ));
                }
                None
            }
        }
    };

    if (200..300).contains(&response.status) {
        return Ok(parsed.unwrap_or(Value::Null));
    }

    let detail = parsed
        .as_ref()
        .and_then(|payload| payload.get("detail"))
        .cloned()
        .map(Box::new);
    let detail_message = detail
        .as_ref()
        .and_then(|detail| read_detail_message(detail.as_ref()))
        .or_else(|| {
            parsed.as_ref().and_then(|payload| match payload {
                Value::String(message) if !message.trim().is_empty() => Some(message.clone()),
                _ => None,
            })
        })
        .or_else(|| (!response.status_text.trim().is_empty()).then(|| response.status_text.clone()))
        .or_else(|| (parsed.is_none() && !body.is_empty()).then(|| body.to_string()))
        .unwrap_or_else(|| format!("HTTP {}", response.status));

    Err(ApiProtocolError {
        kind: "api".to_string(),
        message: format!("{}: {detail_message}", spec.failure_context),
        operation: Some(operation.to_string()),
        status: Some(response.status),
        detail,
        payload: parsed.map(Box::new),
    })
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum ProtocolCommand {
    Prepare {
        operation: String,
        #[serde(default = "empty_object")]
        arguments: Value,
        body: Option<Value>,
    },
    Decode {
        operation: String,
        response: ApiResponse,
    },
    Operations,
    Version,
}

#[derive(Debug, Serialize)]
struct ProtocolEnvelope {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ApiProtocolError>,
}

/// JSON command boundary used by the raw WASM adapter.
pub fn invoke_json(input: &str) -> String {
    let envelope = match serde_json::from_str::<ProtocolCommand>(input) {
        Ok(ProtocolCommand::Prepare {
            operation,
            arguments,
            body,
        }) => match prepare_request(&operation, &arguments, body.as_ref()) {
            Ok(request) => success(serde_json::to_value(request).unwrap_or(Value::Null)),
            Err(error) => failure(error),
        },
        Ok(ProtocolCommand::Decode {
            operation,
            response,
        }) => match decode_response(&operation, response) {
            Ok(value) => success(value),
            Err(error) => failure(error),
        },
        Ok(ProtocolCommand::Operations) => success(json!(SUPPORTED_OPERATIONS)),
        Ok(ProtocolCommand::Version) => success(json!({ "protocol_version": PROTOCOL_VERSION })),
        Err(error) => failure(ApiProtocolError {
            kind: "invalid_command".to_string(),
            message: format!("Invalid Ugoite API protocol command: {error}"),
            operation: None,
            status: None,
            detail: None,
            payload: None,
        }),
    };

    serde_json::to_string(&envelope).unwrap_or_else(|error| {
        format!(
            "{{\"ok\":false,\"error\":{{\"kind\":\"serialization\",\"message\":{}}}}}",
            serde_json::to_string(&error.to_string())
                .unwrap_or_else(|_| "\"serialization failure\"".to_string())
        )
    })
}

fn operation_spec(operation: &str) -> Option<OperationSpec> {
    let (method, failure_context, auth_mode, body_kind) = match operation {
        "auth.get_config" => (
            HttpMethod::Get,
            "Failed to load auth config",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        "auth.get_session" => (
            HttpMethod::Get,
            "Failed to load auth session",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        "auth.login" => (
            HttpMethod::Post,
            "Failed to log in",
            AuthMode::DevProxy,
            RequestBodyKind::Json,
        ),
        "auth.mock_oauth" => (
            HttpMethod::Post,
            "Failed to start mock OAuth login",
            AuthMode::DevProxy,
            RequestBodyKind::Json,
        ),
        "auth.clear_session" => (
            HttpMethod::Delete,
            "Failed to clear auth session",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        "preferences.get" => (
            HttpMethod::Get,
            "Failed to load preferences",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        "preferences.patch" => (
            HttpMethod::Patch,
            "Failed to update preferences",
            AuthMode::Standard,
            RequestBodyKind::Json,
        ),
        "space.list" => (
            HttpMethod::Get,
            "Failed to list spaces",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        "space.create" => (
            HttpMethod::Post,
            "Failed to create space",
            AuthMode::Standard,
            RequestBodyKind::Json,
        ),
        "space.get" => (
            HttpMethod::Get,
            "Failed to get space",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        "space.patch" => (
            HttpMethod::Patch,
            "Failed to patch space",
            AuthMode::Standard,
            RequestBodyKind::Json,
        ),
        "space.test_connection" => (
            HttpMethod::Post,
            "Failed to test connection",
            AuthMode::Standard,
            RequestBodyKind::Json,
        ),
        "space.members.list" => (
            HttpMethod::Get,
            "Failed to list members",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        "space.members.invite" => (
            HttpMethod::Post,
            "Failed to invite member",
            AuthMode::Standard,
            RequestBodyKind::Json,
        ),
        "space.members.accept" => (
            HttpMethod::Post,
            "Failed to accept invitation",
            AuthMode::Standard,
            RequestBodyKind::Json,
        ),
        "space.members.update_role" => (
            HttpMethod::Post,
            "Failed to update role",
            AuthMode::Standard,
            RequestBodyKind::Json,
        ),
        "space.members.revoke" => (
            HttpMethod::Delete,
            "Failed to revoke member",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        "form.list_types" => (
            HttpMethod::Get,
            "Failed to list form types",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        "form.list" => (
            HttpMethod::Get,
            "Failed to list forms",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        "form.get" => (
            HttpMethod::Get,
            "Failed to get form",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        "form.upsert" => (
            HttpMethod::Post,
            "Failed to create form",
            AuthMode::Standard,
            RequestBodyKind::Json,
        ),
        "entry.list" => (
            HttpMethod::Get,
            "Failed to list entries",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        "entry.get" => (
            HttpMethod::Get,
            "Failed to get entry",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        "entry.create" => (
            HttpMethod::Post,
            "Failed to create entry",
            AuthMode::Standard,
            RequestBodyKind::Json,
        ),
        "entry.update" => (
            HttpMethod::Put,
            "Failed to update entry",
            AuthMode::Standard,
            RequestBodyKind::Json,
        ),
        "entry.delete" => (
            HttpMethod::Delete,
            "Failed to delete entry",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        "entry.history" => (
            HttpMethod::Get,
            "Failed to get entry history",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        "entry.revision" => (
            HttpMethod::Get,
            "Failed to get entry revision",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        "entry.restore" => (
            HttpMethod::Post,
            "Failed to restore entry",
            AuthMode::Standard,
            RequestBodyKind::Json,
        ),
        "entry.options" => (
            HttpMethod::Get,
            "Failed to load row_reference options",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        "search.keyword" => (
            HttpMethod::Get,
            "Failed to search entries",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        "search.query" => (
            HttpMethod::Post,
            "Failed to query space",
            AuthMode::Standard,
            RequestBodyKind::Json,
        ),
        "sql.list" => (
            HttpMethod::Get,
            "Failed to list saved SQL",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        "sql.get" => (
            HttpMethod::Get,
            "Failed to get saved SQL",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        "sql.create" => (
            HttpMethod::Post,
            "Failed to create saved SQL",
            AuthMode::Standard,
            RequestBodyKind::Json,
        ),
        "sql.update" => (
            HttpMethod::Put,
            "Failed to update saved SQL",
            AuthMode::Standard,
            RequestBodyKind::Json,
        ),
        "sql.delete" => (
            HttpMethod::Delete,
            "Failed to delete saved SQL",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        "sql_session.create" => (
            HttpMethod::Post,
            "Failed to create SQL session",
            AuthMode::Standard,
            RequestBodyKind::Json,
        ),
        "sql_session.get" => (
            HttpMethod::Get,
            "Failed to load SQL session",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        "sql_session.count" => (
            HttpMethod::Get,
            "Failed to load SQL session count",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        "sql_session.rows" => (
            HttpMethod::Get,
            "Failed to load SQL session rows",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        "asset.list" => (
            HttpMethod::Get,
            "Failed to list assets",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        "asset.upload" => (
            HttpMethod::Post,
            "Failed to upload asset",
            AuthMode::Standard,
            RequestBodyKind::Multipart,
        ),
        "asset.delete" => (
            HttpMethod::Delete,
            "Failed to delete asset",
            AuthMode::Standard,
            RequestBodyKind::None,
        ),
        _ => return None,
    };
    Some(OperationSpec {
        method,
        failure_context,
        auth_mode,
        body_kind,
    })
}

fn required_string(
    operation: &str,
    args: &Map<String, Value>,
    key: &str,
) -> Result<String, ApiProtocolError> {
    match args.get(key) {
        Some(Value::String(value)) if !value.is_empty() => Ok(value.clone()),
        _ => Err(ApiProtocolError::invalid_arguments(
            operation,
            format!("argument `{key}` must be a non-empty string"),
        )),
    }
}

fn optional_string(
    operation: &str,
    args: &Map<String, Value>,
    key: &str,
) -> Result<Option<String>, ApiProtocolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        _ => Err(ApiProtocolError::invalid_arguments(
            operation,
            format!("argument `{key}` must be a string when provided"),
        )),
    }
}

fn required_u64(
    operation: &str,
    args: &Map<String, Value>,
    key: &str,
) -> Result<u64, ApiProtocolError> {
    args.get(key).and_then(Value::as_u64).ok_or_else(|| {
        ApiProtocolError::invalid_arguments(
            operation,
            format!("argument `{key}` must be a non-negative integer"),
        )
    })
}

fn optional_bool(
    operation: &str,
    args: &Map<String, Value>,
    key: &str,
) -> Result<Option<bool>, ApiProtocolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        _ => Err(ApiProtocolError::invalid_arguments(
            operation,
            format!("argument `{key}` must be a boolean when provided"),
        )),
    }
}

fn encoded_path(segments: &[String], query: &[(String, String)]) -> Result<String, String> {
    let mut url = Url::parse("https://ugoite.invalid").map_err(|error| error.to_string())?;
    {
        let mut path = url
            .path_segments_mut()
            .map_err(|_| "base URL cannot contain path segments".to_string())?;
        path.clear();
        for segment in segments {
            path.push(segment);
        }
    }
    if !query.is_empty() {
        let mut pairs = url.query_pairs_mut();
        for (key, value) in query {
            pairs.append_pair(key, value);
        }
    }
    let mut result = url.path().to_string();
    if let Some(query) = url.query() {
        result.push('?');
        result.push_str(query);
    }
    Ok(result)
}

fn read_detail_message(value: &Value) -> Option<String> {
    match value {
        Value::String(message) if !message.trim().is_empty() => Some(message.clone()),
        Value::Array(items) => {
            let messages: Vec<String> = items.iter().filter_map(read_detail_message).collect();
            (!messages.is_empty()).then(|| messages.join("\n"))
        }
        Value::Object(object) => {
            if let Some(Value::String(message)) = object.get("msg") {
                if !message.trim().is_empty() {
                    return Some(message.clone());
                }
            }
            serde_json::to_string(value).ok()
        }
        Value::Number(_) | Value::Bool(_) => None,
        _ => None,
    }
}

fn looks_like_html(headers: &[Header], body: &str) -> bool {
    let content_type_is_html = headers.iter().any(|header| {
        header.name.eq_ignore_ascii_case("content-type")
            && header.value.to_ascii_lowercase().contains("text/html")
    });
    let lower = body.trim_start().to_ascii_lowercase();
    content_type_is_html || lower.starts_with("<!doctype html") || lower.starts_with("<html")
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

fn success(value: Value) -> ProtocolEnvelope {
    ProtocolEnvelope {
        ok: true,
        value: Some(value),
        error: None,
    }
}

fn failure(error: ApiProtocolError) -> ProtocolEnvelope {
    ProtocolEnvelope {
        ok: false,
        value: None,
        error: Some(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_req_api_001_encodes_path_segments_and_query_values() {
        let request = prepare_request(
            "search.keyword",
            &json!({"space_id": "team/東京", "q": "a & b#c"}),
            None,
        )
        .expect("request");

        assert_eq!(request.method, HttpMethod::Get);
        assert_eq!(
            request.path,
            "/spaces/team%2F%E6%9D%B1%E4%BA%AC/search?q=a+%26+b%23c"
        );
    }

    #[test]
    fn test_api_req_api_002_builds_json_body_once() {
        let request = prepare_request(
            "entry.create",
            &json!({"space_id": "demo"}),
            Some(&json!({"id": "entry-1", "markdown": "# Hello"})),
        )
        .expect("request");

        assert_eq!(request.body_kind, RequestBodyKind::Json);
        assert_eq!(request.headers[0].value, "application/json");
        assert_eq!(
            serde_json::from_str::<Value>(request.body.as_deref().expect("body")).unwrap(),
            json!({"id": "entry-1", "markdown": "# Hello"})
        );
    }

    #[test]
    fn test_api_req_api_001_decodes_validation_detail_without_object_placeholders() {
        let error = decode_response(
            "space.create",
            ApiResponse {
                status: 422,
                status_text: "Unprocessable Entity".into(),
                headers: vec![Header {
                    name: "content-type".into(),
                    value: "application/json".into(),
                }],
                body: json!({"detail": [{"msg": "Input should be at least 1 character"}]})
                    .to_string(),
            },
        )
        .expect_err("must fail");

        assert!(error
            .message
            .contains("Input should be at least 1 character"));
        assert!(!error.message.contains("[object Object]"));
    }

    #[test]
    fn test_api_req_api_001_uses_status_text_for_non_message_detail_values() {
        let error = decode_response(
            "auth.mock_oauth",
            ApiResponse {
                status: 409,
                status_text: "Conflict".into(),
                headers: vec![Header {
                    name: "content-type".into(),
                    value: "application/json".into(),
                }],
                body: json!({"detail": 123}).to_string(),
            },
        )
        .expect_err("must fail");

        assert_eq!(error.message, "Failed to start mock OAuth login: Conflict");
    }

    #[test]
    fn test_api_req_api_001_rejects_html_api_base_response() {
        let error = decode_response(
            "space.list",
            ApiResponse {
                status: 200,
                status_text: "OK".into(),
                headers: vec![Header {
                    name: "content-type".into(),
                    value: "text/html".into(),
                }],
                body: "<!doctype html><title>Ugoite</title>".into(),
            },
        )
        .expect_err("must fail");

        assert_eq!(error.kind, "invalid_response");
        assert!(error.message.contains("ending in `/api`"));
    }

    #[test]
    fn test_api_req_api_002_accepts_empty_success_response() {
        let value = decode_response(
            "entry.delete",
            ApiResponse {
                status: 204,
                status_text: "No Content".into(),
                headers: vec![],
                body: String::new(),
            },
        )
        .expect("success");
        assert_eq!(value, Value::Null);
    }

    #[test]
    fn test_api_req_api_001_json_boundary_round_trips_prepare_and_decode() {
        let prepared = invoke_json(
            r#"{"action":"prepare","operation":"form.get","arguments":{"space_id":"demo","form_name":"Meeting Notes"}}"#,
        );
        let prepared: Value = serde_json::from_str(&prepared).unwrap();
        assert_eq!(prepared["ok"], true);
        assert_eq!(
            prepared["value"]["path"],
            "/spaces/demo/forms/Meeting%20Notes"
        );

        let decoded = invoke_json(
            r#"{"action":"decode","operation":"form.get","response":{"status":200,"status_text":"OK","headers":[],"body":"{\"name\":\"Meeting Notes\"}"}}"#,
        );
        let decoded: Value = serde_json::from_str(&decoded).unwrap();
        assert_eq!(decoded["value"]["name"], "Meeting Notes");
    }

    #[test]
    fn test_api_req_api_001_supported_operation_manifest_matches_prepare_and_decode_specs() {
        for operation in SUPPORTED_OPERATIONS {
            let spec = operation_spec(operation)
                .unwrap_or_else(|| panic!("missing decoder metadata for {operation}"));
            let arguments = sample_arguments(operation);
            let body = (spec.body_kind == RequestBodyKind::Json).then(|| json!({}));
            let request =
                prepare_request(operation, &arguments, body.as_ref()).unwrap_or_else(|error| {
                    panic!("missing prepare definition for {operation}: {error}")
                });
            assert_eq!(request.operation.as_str(), *operation);
            assert_eq!(request.method, spec.method, "method drift for {operation}");
            assert_eq!(
                request.body_kind, spec.body_kind,
                "body kind drift for {operation}"
            );
            assert_eq!(
                request.auth_mode, spec.auth_mode,
                "auth mode drift for {operation}"
            );
        }

        let response = invoke_json(r#"{"action":"operations"}"#);
        let response: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["value"], json!(SUPPORTED_OPERATIONS));
    }

    fn sample_arguments(operation: &str) -> Value {
        let mut arguments = Map::new();
        let needs_space_id = operation.starts_with("space.")
            || operation.starts_with("form.")
            || operation.starts_with("entry.")
            || operation.starts_with("search.")
            || operation.starts_with("sql.")
            || operation.starts_with("sql_session.")
            || operation.starts_with("asset.");
        if needs_space_id && !matches!(operation, "space.list" | "space.create") {
            arguments.insert("space_id".into(), json!("demo"));
        }
        if matches!(
            operation,
            "space.members.update_role" | "space.members.revoke"
        ) {
            arguments.insert("member_user_id".into(), json!("user-1"));
        }
        if operation == "form.get" {
            arguments.insert("form_name".into(), json!("Meeting Notes"));
        }
        if matches!(
            operation,
            "entry.get"
                | "entry.update"
                | "entry.delete"
                | "entry.history"
                | "entry.revision"
                | "entry.restore"
        ) {
            arguments.insert("entry_id".into(), json!("entry-1"));
        }
        if operation == "entry.revision" {
            arguments.insert("revision_id".into(), json!("revision-1"));
        }
        if operation == "entry.options" {
            arguments.insert("form".into(), json!("Meeting"));
            arguments.insert("limit".into(), json!(20));
            arguments.insert("q".into(), json!("weekly"));
        }
        if operation == "search.keyword" {
            arguments.insert("q".into(), json!("alpha & beta"));
        }
        if matches!(operation, "sql.get" | "sql.update" | "sql.delete") {
            arguments.insert("sql_id".into(), json!("saved-1"));
        }
        if matches!(
            operation,
            "sql_session.get" | "sql_session.count" | "sql_session.rows"
        ) {
            arguments.insert("session_id".into(), json!("session-1"));
        }
        if operation == "sql_session.rows" {
            arguments.insert("offset".into(), json!(0));
            arguments.insert("limit".into(), json!(100));
        }
        if operation == "asset.delete" {
            arguments.insert("asset_id".into(), json!("asset-1"));
        }
        Value::Object(arguments)
    }
}
