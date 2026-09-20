use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::sync::OnceLock;
use ugoite_api_client::{
    decode_response, prepare_request, ApiResponse, Header, HttpMethod, PreparedRequest,
    RequestBodyKind,
};

/// Execute a portable API operation whose success body is raw bytes.
///
/// Asset content is never JSON: success returns the exact response bytes
/// while failures still decode through the canonical error projection so
/// machine output keeps stable error codes. Only the server response is
/// ever surfaced: never local paths, credentials, or request headers.
pub async fn execute_bytes(base_url: &str, operation: &str, arguments: Value) -> Result<Vec<u8>> {
    let prepared = prepare_request(operation, &arguments, None)?;
    if prepared.body_kind != RequestBodyKind::None {
        bail!("operation {operation} does not return raw bytes");
    }
    let (_, request) = authenticated_request(base_url, &prepared).await?;
    let response = request
        .send()
        .await
        .with_context(|| format!("send {operation} request"))?;
    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or_default().to_string();
    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("read {operation} response"))?
        .to_vec();
    if !status.is_success() {
        let decoded = decode_response(
            operation,
            ApiResponse {
                status: status.as_u16(),
                status_text,
                headers: Vec::new(),
                body: String::from_utf8_lossy(&bytes).into_owned(),
            },
        );
        match decoded {
            Ok(_) => bail!("{operation} failed with status {}", status.as_u16()),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(bytes)
}

/// Execute a portable API operation through the native reqwest transport.
pub async fn execute(
    base_url: &str,
    operation: &str,
    arguments: Value,
    body: Option<Value>,
) -> Result<Value> {
    let prepared = prepare_request(operation, &arguments, body.as_ref())?;
    if prepared.body_kind == RequestBodyKind::Multipart {
        bail!("operation {operation} requires the multipart transport");
    }
    execute_prepared(base_url, prepared).await
}

/// Execute against an explicit [`crate::cli_config::SpaceTarget`].
///
/// - `Core` is a programming error (callers handle local transports).
/// - Remote targets are always resolved from the canonical named context; the
///   named credential profile for exactly that connection is used.
pub async fn execute_for_target(
    target: &crate::cli_config::SpaceTarget,
    operation: &str,
    arguments: Value,
    body: Option<Value>,
) -> Result<Value> {
    let (base, connection, credential) = match target {
        crate::cli_config::SpaceTarget::Core { .. } => {
            bail!("operation {operation} does not use the remote transport")
        }
        crate::cli_config::SpaceTarget::Remote {
            base,
            connection,
            credential,
            ..
        } => (base.clone(), connection.clone(), credential.clone()),
    };
    execute_for_connection(
        &base,
        &connection,
        credential.as_deref(),
        operation,
        arguments,
        body,
    )
    .await
}

/// Execute a remote operation for a named connection before a Space context
/// exists. This is used by connection-scoped commands such as `space create`
/// and `space list`; it still resolves the credential by the named connection
/// and never falls back to an unscoped session.
pub async fn execute_for_connection(
    base_url: &str,
    connection: &str,
    credential: Option<&str>,
    operation: &str,
    arguments: Value,
    body: Option<Value>,
) -> Result<Value> {
    let prepared = prepare_request(operation, &arguments, body.as_ref())?;
    if prepared.body_kind == RequestBodyKind::Multipart {
        bail!("operation {operation} requires the multipart transport");
    }
    execute_prepared_for_target(base_url, Some(connection), credential, prepared).await
}

/// Bytes variant of [`execute_for_target`] with the same safety boundary.
pub async fn execute_bytes_for_target(
    target: &crate::cli_config::SpaceTarget,
    operation: &str,
    arguments: Value,
) -> Result<Vec<u8>> {
    let (base, connection, credential) = match target {
        crate::cli_config::SpaceTarget::Core { .. } => {
            bail!("operation {operation} does not use the remote transport")
        }
        crate::cli_config::SpaceTarget::Remote {
            base,
            connection,
            credential,
            ..
        } => (base.clone(), connection.clone(), credential.clone()),
    };
    let prepared = prepare_request(operation, &arguments, None)?;
    if prepared.body_kind != RequestBodyKind::None {
        bail!("operation {operation} does not return raw bytes");
    }
    let (_, request) = authenticated_request_for_target(
        &base,
        Some(connection.as_str()),
        credential.as_deref(),
        &prepared,
    )
    .await?;
    let response = request
        .send()
        .await
        .with_context(|| format!("send {operation} request"))?;
    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or_default().to_string();
    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("read {operation} response"))?
        .to_vec();
    if !status.is_success() {
        let decoded = decode_response(
            operation,
            ApiResponse {
                status: status.as_u16(),
                status_text,
                headers: Vec::new(),
                body: String::from_utf8_lossy(&bytes).into_owned(),
            },
        );
        match decoded {
            Ok(_) => bail!("{operation} failed with status {}", status.as_u16()),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(bytes)
}

/// Multipart variant of [`execute_for_target`] with the same safety boundary.
pub async fn execute_multipart_for_target(
    target: &crate::cli_config::SpaceTarget,
    operation: &str,
    arguments: Value,
    filename: String,
    bytes: Vec<u8>,
) -> Result<Value> {
    let (base, connection, credential) = match target {
        crate::cli_config::SpaceTarget::Core { .. } => {
            bail!("operation {operation} does not use the remote transport")
        }
        crate::cli_config::SpaceTarget::Remote {
            base,
            connection,
            credential,
            ..
        } => (base.clone(), connection.clone(), credential.clone()),
    };
    let prepared = prepare_request(operation, &arguments, None)?;
    if prepared.body_kind != RequestBodyKind::Multipart {
        bail!("operation {operation} does not use the multipart transport");
    }
    let (_, request) = authenticated_request_for_target(
        &base,
        Some(connection.as_str()),
        credential.as_deref(),
        &prepared,
    )
    .await?;
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(filename)
        .mime_str("application/octet-stream")
        .with_context(|| format!("prepare {operation} upload"))?;
    let form = reqwest::multipart::Form::new().part("file", part);
    send_and_decode(&prepared.operation, request.multipart(form)).await
}

/// Execute a multipart API operation with a single `file` part.
///
/// The portable protocol names the operation and path; the CLI attaches the
/// file bytes under the `file` field the REST contract requires. Only the
/// server response is ever printed or logged: never file contents, local
/// paths, credentials, or request headers.
pub async fn execute_multipart(
    base_url: &str,
    operation: &str,
    arguments: Value,
    filename: String,
    bytes: Vec<u8>,
) -> Result<Value> {
    let prepared = prepare_request(operation, &arguments, None)?;
    if prepared.body_kind != RequestBodyKind::Multipart {
        bail!("operation {operation} does not use the multipart transport");
    }
    let (_, request) = authenticated_request(base_url, &prepared).await?;
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(filename)
        .mime_str("application/octet-stream")
        .with_context(|| format!("prepare {operation} upload"))?;
    let form = reqwest::multipart::Form::new().part("file", part);
    send_and_decode(&prepared.operation, request.multipart(form)).await
}

async fn execute_prepared(base_url: &str, prepared: PreparedRequest) -> Result<Value> {
    let operation = prepared.operation.clone();
    let (_, mut request) = authenticated_request(base_url, &prepared).await?;
    request = match (prepared.body_kind, prepared.body) {
        (RequestBodyKind::Multipart, _) => bail!("operation {operation} requires multipart"),
        (RequestBodyKind::Json, Some(body)) => request.body(body),
        (RequestBodyKind::Json, None) => bail!("operation {operation} requires a JSON body"),
        (RequestBodyKind::None, _) => request,
    };
    send_and_decode(&operation, request).await
}

async fn execute_prepared_for_target(
    base_url: &str,
    connection: Option<&str>,
    credential: Option<&str>,
    prepared: PreparedRequest,
) -> Result<Value> {
    let operation = prepared.operation.clone();
    let (_, mut request) =
        authenticated_request_for_target(base_url, connection, credential, &prepared).await?;
    request = match (prepared.body_kind, prepared.body) {
        (RequestBodyKind::Multipart, _) => bail!("operation {operation} requires multipart"),
        (RequestBodyKind::Json, Some(body)) => request.body(body),
        (RequestBodyKind::Json, None) => bail!("operation {operation} requires a JSON body"),
        (RequestBodyKind::None, _) => request,
    };
    send_and_decode(&operation, request).await
}

/// Context-first authentication: resolve `credential` for exactly
/// `connection` from the user-global credential store.
async fn authenticated_request_for_target(
    base_url: &str,
    connection: Option<&str>,
    credential: Option<&str>,
    prepared: &PreparedRequest,
) -> Result<(String, reqwest::RequestBuilder)> {
    let Some(connection_name) = connection else {
        return authenticated_request(base_url, prepared).await;
    };
    let url = join_base_and_path(base_url, &prepared.path);
    crate::config::validate_server_endpoint_url(base_url, "Remote request")?;
    let mut request = match prepared.method {
        HttpMethod::Get => client().get(&url),
        HttpMethod::Post => client().post(&url),
        HttpMethod::Put => client().put(&url),
        HttpMethod::Patch => client().patch(&url),
        HttpMethod::Delete => client().delete(&url),
    };
    for header in &prepared.headers {
        request = request.header(header.name.as_str(), header.value.as_str());
    }
    if let Some(session) = named_session_for_target(base_url, connection_name, credential).await? {
        request = request
            .header("Authorization", format!("DPoP {}", session.access_token))
            .header(
                "DPoP",
                crate::commands::auth::dpop_proof(&session, prepared.method.as_str(), &url)?,
            );
    }
    Ok((url, request))
}

/// Load the named credential profile for exactly `connection_name`.
///
/// Returns `None` when the context carries no credential (anonymous remote).
/// A present profile must exist and its stored `connection` must match;
/// cross-connection reuse is an actionable error.
pub(crate) async fn named_session_for_target(
    base_url: &str,
    connection_name: &str,
    credential_name: Option<&str>,
) -> Result<Option<crate::config::AuthSession>> {
    let Some(name) = credential_name else {
        return Ok(None);
    };
    let store = crate::cli_config::credentials::load_credentials()?;
    let profile =
        crate::cli_config::credentials::resolve_named_profile(&store, connection_name, Some(name))?
            .ok_or_else(|| anyhow::anyhow!("Credential profile {name:?} is not paired."))?;
    // Strip the `connection` bookkeeping field before parsing the session.
    let mut value = profile.clone();
    if let Some(object) = value.as_object_mut() {
        object.remove("connection");
    }
    let mut session: crate::config::AuthSession = serde_json::from_value(value)
        .with_context(|| format!("invalid credential profile {name:?}"))?;
    // The profile is bound to its connection's server: refuse to send it to
    // a different base URL (normalized trailing slash).
    let expected = base_url.trim_end_matches('/');
    let stored = session.base_url.trim_end_matches('/');
    if stored != expected {
        bail!(
            "Credential profile {name:?} belongs to a different server; run `ugoite auth login --connection {connection_name} --credential {name}`"
        );
    }
    // Refresh expired tokens using the named credential policy, then
    // persist the rotation back to the named profile.
    if session.expires_at <= chrono::Utc::now().timestamp() + 30 {
        if let Some(refreshed) = crate::commands::auth::refresh_session(&session, base_url).await? {
            session = refreshed;
            let mut store = crate::cli_config::credentials::load_credentials()?;
            let mut profile =
                serde_json::to_value(&session).context("serialize refreshed credential")?;
            profile["connection"] = serde_json::Value::String(connection_name.to_string());
            store.credentials.insert(name.to_string(), profile);
            crate::cli_config::credentials::write_credentials(&store)?;
        } else {
            return Ok(Some(session));
        }
    }
    Ok(Some(session))
}

async fn authenticated_request(
    base_url: &str,
    prepared: &PreparedRequest,
) -> Result<(String, reqwest::RequestBuilder)> {
    let url = join_base_and_path(base_url, &prepared.path);
    crate::config::validate_server_endpoint_url(base_url, "Remote request")?;
    let mut request = match prepared.method {
        HttpMethod::Get => client().get(&url),
        HttpMethod::Post => client().post(&url),
        HttpMethod::Put => client().put(&url),
        HttpMethod::Patch => client().patch(&url),
        HttpMethod::Delete => client().delete(&url),
    };
    for header in &prepared.headers {
        request = request.header(header.name.as_str(), header.value.as_str());
    }
    let session = if let Some(target) = configured_target_for_base(base_url) {
        named_session_for_target(
            base_url,
            target.connection_name().expect("remote target connection"),
            target.credential_name(),
        )
        .await?
    } else {
        crate::commands::auth::active_session(base_url).await?
    };
    if let Some(session) = session {
        request = request
            .header("Authorization", format!("DPoP {}", session.access_token))
            .header(
                "DPoP",
                crate::commands::auth::dpop_proof(&session, prepared.method.as_str(), &url)?,
            );
    }
    Ok((url, request))
}

/// Resolve the active canonical context for call sites that still only carry
/// a validated base URL (for example read-only SQL and asset helpers). This
/// keeps their transport boundary on the same named credential as the
/// context-aware mutation paths.
fn configured_target_for_base(base_url: &str) -> Option<crate::cli_config::SpaceTarget> {
    let config = std::env::var_os("UGOITE_CLI_ACTIVE_CONFIG").map(std::path::PathBuf::from);
    let context = std::env::var("UGOITE_CLI_ACTIVE_CONTEXT").ok();
    let target = crate::cli_config::resolve_command_target(
        config.as_deref(),
        context.as_deref(),
        "remote request",
    )
    .ok()?;
    match &target {
        crate::cli_config::SpaceTarget::Remote { base, .. }
            if base.trim_end_matches('/') == base_url.trim_end_matches('/') =>
        {
            Some(target)
        }
        _ => None,
    }
}

async fn send_and_decode(operation: &str, request: reqwest::RequestBuilder) -> Result<Value> {
    let response = request
        .send()
        .await
        .with_context(|| format!("send {operation} request"))?;
    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or_default().to_string();
    let headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value.to_str().ok().map(|value| Header {
                name: name.as_str().to_string(),
                value: value.to_string(),
            })
        })
        .collect();
    let body = response.text().await?;
    decode_response(
        operation,
        ApiResponse {
            status: status.as_u16(),
            status_text,
            headers,
            body,
        },
    )
    .map_err(anyhow::Error::from)
}

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

fn join_base_and_path(base_url: &str, path: &str) -> String {
    format!(
        "{}{}",
        base_url.trim_end_matches('/'),
        if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        }
    )
}

#[cfg(test)]
mod tests {
    use super::join_base_and_path;
    #[test]
    fn joins_prepared_paths_without_double_slashes() {
        assert_eq!(
            join_base_and_path("https://example.com/api/", "/spaces/demo"),
            "https://example.com/api/spaces/demo"
        );
    }
}
