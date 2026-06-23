use crate::config::{effective_api_key, effective_bearer_token};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::OnceLock;
use ugoite_api_client::{
    decode_response, prepare_request, ApiResponse, AuthMode, Header, HttpMethod, PreparedRequest,
    RequestBodyKind,
};

const DEV_AUTH_PROXY_HEADER_NAME: &str = "x-ugoite-dev-auth-proxy-token";
const DEV_PASSKEY_CONTEXT_HEADER_NAME: &str = "x-ugoite-dev-passkey-context";
const DEV_AUTH_FILE_ENV_NAME: &str = "UGOITE_DEV_AUTH_FILE";

#[derive(Deserialize)]
struct CachedDevAuthFile {
    passkey_context: Option<String>,
}

/// Execute one named Ugoite API operation through the native reqwest transport.
/// Endpoint construction and response decoding are owned by
/// `ugoite-api-client`, which is also compiled into the browser WASM adapter.
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

async fn execute_prepared(base_url: &str, prepared: PreparedRequest) -> Result<Value> {
    let operation = prepared.operation.clone();
    let url = join_base_and_path(base_url, &prepared.path);
    ensure_safe_remote_request_url(&url)?;

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
    request = add_auth_headers(request);
    if prepared.auth_mode == AuthMode::DevProxy {
        request = add_dev_local_auth_headers(&url, request);
    }

    request = match (prepared.body_kind, prepared.body) {
        (RequestBodyKind::Multipart, _) => {
            bail!("operation {operation} requires the multipart transport")
        }
        (RequestBodyKind::Json, Some(body)) => request.body(body),
        (RequestBodyKind::Json, None) => {
            bail!("operation {operation} requires a JSON body")
        }
        (RequestBodyKind::None, _) => request,
    };

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
        &operation,
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

fn add_auth_headers(req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    let mut request = req;
    if let Some(token) = effective_bearer_token() {
        request = request.header("Authorization", format!("Bearer {token}"));
    } else if let Some(key) = effective_api_key() {
        request = request.header("X-Api-Key", key);
    }
    request
}

fn ensure_safe_remote_request_url(url: &str) -> Result<()> {
    crate::config::validate_server_endpoint_url(url, "Remote request")
}

fn is_loopback_host(host: &str) -> bool {
    let host = host.trim_end_matches('.');
    let normalized = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(host);
    normalized.eq_ignore_ascii_case("localhost")
        || normalized
            .parse::<IpAddr>()
            .map(|address| address.is_loopback())
            .unwrap_or(false)
}

fn is_local_dev_request_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some_and(is_loopback_host)
}

fn non_empty_env_var(key: &str) -> Option<String> {
    std::env::var(key).ok().and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

pub(crate) fn dev_auth_file_path() -> Option<PathBuf> {
    non_empty_env_var(DEV_AUTH_FILE_ENV_NAME)
        .map(PathBuf::from)
        .or_else(|| {
            non_empty_env_var("HOME")
                .map(|home| PathBuf::from(home).join(".ugoite").join("dev-auth.json"))
        })
}

fn cached_dev_passkey_context() -> Option<String> {
    let path = dev_auth_file_path()?;
    let payload = std::fs::read_to_string(path).ok()?;
    let cached: CachedDevAuthFile = serde_json::from_str(&payload).ok()?;
    cached.passkey_context.and_then(|context| {
        let trimmed = context.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn add_dev_local_auth_headers(url: &str, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    if !is_local_dev_request_url(url) {
        return req;
    }

    let request = if let Some(token) = non_empty_env_var("UGOITE_DEV_AUTH_PROXY_TOKEN") {
        req.header(DEV_AUTH_PROXY_HEADER_NAME, token)
    } else {
        req
    };

    if let Some(context) =
        non_empty_env_var("UGOITE_DEV_PASSKEY_CONTEXT").or_else(cached_dev_passkey_context)
    {
        request.header(DEV_PASSKEY_CONTEXT_HEADER_NAME, context)
    } else {
        request
    }
}

#[cfg(test)]
mod tests {
    use super::{
        add_dev_local_auth_headers, is_local_dev_request_url, join_base_and_path,
        DEV_AUTH_PROXY_HEADER_NAME, DEV_PASSKEY_CONTEXT_HEADER_NAME,
    };

    #[test]
    fn test_api_req_api_001_joins_prepared_paths_without_double_slashes() {
        assert_eq!(
            join_base_and_path("https://example.com/api/", "/spaces/demo"),
            "https://example.com/api/spaces/demo"
        );
    }

    #[test]
    fn test_dev_local_auth_headers_req_ops_015_only_allow_loopback_hosts() {
        assert!(is_local_dev_request_url("http://localhost:8000/auth/login"));
        assert!(is_local_dev_request_url("https://127.0.0.1/auth/login"));
        assert!(is_local_dev_request_url("http://[::1]:3000/api/auth/login"));

        assert!(!is_local_dev_request_url("https://example.com/auth/login"));
        assert!(!is_local_dev_request_url("http://example.com/auth/login"));
        assert!(!is_local_dev_request_url("not-a-url"));
    }

    #[test]
    fn test_dev_local_auth_headers_req_ops_015_skip_non_loopback_https_hosts() {
        let _guard = crate::test_support::env_lock().lock().expect("env lock");
        std::env::set_var("UGOITE_DEV_AUTH_PROXY_TOKEN", "proxy-secret");
        std::env::set_var("UGOITE_DEV_PASSKEY_CONTEXT", "passkey-context");

        let client = reqwest::Client::new();
        let request = add_dev_local_auth_headers(
            "https://example.com/auth/login",
            client.post("https://example.com/auth/login"),
        )
        .build()
        .expect("build request");

        assert!(request.headers().get(DEV_AUTH_PROXY_HEADER_NAME).is_none());
        assert!(request
            .headers()
            .get(DEV_PASSKEY_CONTEXT_HEADER_NAME)
            .is_none());

        std::env::remove_var("UGOITE_DEV_AUTH_PROXY_TOKEN");
        std::env::remove_var("UGOITE_DEV_PASSKEY_CONTEXT");
    }

    #[test]
    fn test_dev_local_auth_headers_req_ops_015_add_loopback_headers() {
        let _guard = crate::test_support::env_lock().lock().expect("env lock");
        std::env::set_var("UGOITE_DEV_AUTH_PROXY_TOKEN", "proxy-secret");
        std::env::set_var("UGOITE_DEV_PASSKEY_CONTEXT", "passkey-context");

        let client = reqwest::Client::new();
        let request = add_dev_local_auth_headers(
            "http://127.0.0.1:8000/auth/login",
            client.post("http://127.0.0.1:8000/auth/login"),
        )
        .build()
        .expect("build request");

        assert_eq!(
            request
                .headers()
                .get(DEV_AUTH_PROXY_HEADER_NAME)
                .expect("proxy header"),
            "proxy-secret"
        );
        assert_eq!(
            request
                .headers()
                .get(DEV_PASSKEY_CONTEXT_HEADER_NAME)
                .expect("passkey context header"),
            "passkey-context"
        );

        std::env::remove_var("UGOITE_DEV_AUTH_PROXY_TOKEN");
        std::env::remove_var("UGOITE_DEV_PASSKEY_CONTEXT");
    }

    #[test]
    fn test_dev_local_auth_headers_req_ops_015_skip_invalid_urls() {
        let _guard = crate::test_support::env_lock().lock().expect("env lock");
        std::env::set_var("UGOITE_DEV_AUTH_PROXY_TOKEN", "proxy-secret");
        std::env::set_var("UGOITE_DEV_PASSKEY_CONTEXT", "passkey-context");

        let client = reqwest::Client::new();
        let request = add_dev_local_auth_headers(
            "not-a-url",
            client.post("http://127.0.0.1:8000/auth/login"),
        )
        .build()
        .expect("build request");

        assert!(request.headers().get(DEV_AUTH_PROXY_HEADER_NAME).is_none());
        assert!(request
            .headers()
            .get(DEV_PASSKEY_CONTEXT_HEADER_NAME)
            .is_none());

        std::env::remove_var("UGOITE_DEV_AUTH_PROXY_TOKEN");
        std::env::remove_var("UGOITE_DEV_PASSKEY_CONTEXT");
    }
}
