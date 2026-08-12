use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::sync::OnceLock;
use ugoite_api_client::{
    decode_response, prepare_request, ApiResponse, Header, HttpMethod, PreparedRequest,
    RequestBodyKind,
};

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

/// Execute a multipart operation while keeping path, authentication, and
/// response decoding in the same native transport as JSON operations.
pub async fn execute_multipart(
    base_url: &str,
    operation: &str,
    arguments: Value,
    field_name: &str,
    filename: &str,
    data: Vec<u8>,
    media_type: &str,
) -> Result<Value> {
    let prepared = prepare_request(operation, &arguments, None)?;
    if prepared.body_kind != RequestBodyKind::Multipart {
        bail!("operation {operation} does not require a multipart body");
    }
    let operation_name = prepared.operation.clone();
    let (_, request) = authenticated_request(base_url, &prepared).await?;
    let part = reqwest::multipart::Part::bytes(data)
        .file_name(filename.to_string())
        .mime_str(media_type)?;
    let form = reqwest::multipart::Form::new().part(field_name.to_string(), part);
    send_and_decode(&operation_name, request.multipart(form)).await
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

async fn authenticated_request(
    base_url: &str,
    prepared: &PreparedRequest,
) -> Result<(String, reqwest::RequestBuilder)> {
    let url = join_base_and_path(base_url, &prepared.path);
    crate::config::validate_server_endpoint_url(&url, "Remote request")?;
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
    if let Some(session) = crate::commands::auth::active_session(base_url).await? {
        request = request
            .header("Authorization", format!("DPoP {}", session.access_token))
            .header(
                "DPoP",
                crate::commands::auth::dpop_proof(&session, prepared.method.as_str(), &url)?,
            );
    }
    Ok((url, request))
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
