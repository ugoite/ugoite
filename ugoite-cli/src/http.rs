use crate::config::{effective_api_key, effective_bearer_token};
use anyhow::{bail, Result};
use serde::Deserialize;
use std::net::IpAddr;
use std::path::PathBuf;

const DEV_AUTH_PROXY_HEADER_NAME: &str = "x-ugoite-dev-auth-proxy-token";
const DEV_PASSKEY_CONTEXT_HEADER_NAME: &str = "x-ugoite-dev-passkey-context";
const DEV_AUTH_FILE_ENV_NAME: &str = "UGOITE_DEV_AUTH_FILE";

#[derive(Deserialize)]
struct CachedDevAuthFile {
    passkey_context: Option<String>,
}

pub async fn http_get(url: &str) -> Result<serde_json::Value> {
    ensure_safe_remote_request_url(url)?;
    let client = reqwest::Client::new();
    let req = add_auth_headers(client.get(url));
    let resp = match req.send().await {
        Ok(resp) => resp,
        Err(error) => return Err(error.into()),
    };
    if !resp.status().is_success() {
        let status = resp.status();
        bail!("HTTP {}: {}", status, resp.text().await.unwrap_or_default());
    }
    Ok(resp.json().await?)
}

pub async fn http_post(url: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
    ensure_safe_remote_request_url(url)?;
    let client = reqwest::Client::new();
    let req = add_auth_headers(client.post(url).json(body));
    let resp = match req.send().await {
        Ok(resp) => resp,
        Err(error) => return Err(error.into()),
    };
    if !resp.status().is_success() {
        let status = resp.status();
        bail!("HTTP {}: {}", status, resp.text().await.unwrap_or_default());
    }
    Ok(resp.json().await.unwrap_or(serde_json::Value::Null))
}

pub async fn http_post_with_dev_auth_proxy(
    url: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value> {
    let client = reqwest::Client::new();
    let req = add_dev_local_auth_headers(url, add_auth_headers(client.post(url).json(body)));
    ensure_safe_remote_request_url(url)?;
    let resp = match req.send().await {
        Ok(resp) => resp,
        Err(error) => return Err(error.into()),
    };
    if !resp.status().is_success() {
        let status = resp.status();
        bail!("HTTP {}: {}", status, resp.text().await.unwrap_or_default());
    }
    Ok(resp.json().await.unwrap_or(serde_json::Value::Null))
}

pub async fn http_put(url: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
    ensure_safe_remote_request_url(url)?;
    let client = reqwest::Client::new();
    let req = add_auth_headers(client.put(url).json(body));
    let resp = match req.send().await {
        Ok(resp) => resp,
        Err(error) => return Err(error.into()),
    };
    if !resp.status().is_success() {
        let status = resp.status();
        bail!("HTTP {}: {}", status, resp.text().await.unwrap_or_default());
    }
    Ok(resp.json().await.unwrap_or(serde_json::Value::Null))
}

pub async fn http_patch(url: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
    ensure_safe_remote_request_url(url)?;
    let client = reqwest::Client::new();
    let req = add_auth_headers(client.patch(url).json(body));
    let resp = match req.send().await {
        Ok(resp) => resp,
        Err(error) => return Err(error.into()),
    };
    if !resp.status().is_success() {
        let status = resp.status();
        bail!("HTTP {}: {}", status, resp.text().await.unwrap_or_default());
    }
    Ok(resp.json().await.unwrap_or(serde_json::Value::Null))
}

pub async fn http_delete(url: &str) -> Result<serde_json::Value> {
    ensure_safe_remote_request_url(url)?;
    let client = reqwest::Client::new();
    let req = add_auth_headers(client.delete(url));
    let resp = match req.send().await {
        Ok(resp) => resp,
        Err(error) => return Err(error.into()),
    };
    if !resp.status().is_success() {
        let status = resp.status();
        bail!("HTTP {}: {}", status, resp.text().await.unwrap_or_default());
    }
    Ok(resp.json().await.unwrap_or(serde_json::Value::Null))
}

fn add_auth_headers(req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    let mut r = req;
    if let Some(token) = effective_bearer_token() {
        r = r.header("Authorization", format!("Bearer {token}"));
    } else if let Some(key) = effective_api_key() {
        r = r.header("X-Api-Key", key);
    }
    r
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

    let req = if let Some(token) = non_empty_env_var("UGOITE_DEV_AUTH_PROXY_TOKEN") {
        req.header(DEV_AUTH_PROXY_HEADER_NAME, token)
    } else {
        req
    };

    if let Some(context) =
        non_empty_env_var("UGOITE_DEV_PASSKEY_CONTEXT").or_else(cached_dev_passkey_context)
    {
        req.header(DEV_PASSKEY_CONTEXT_HEADER_NAME, context)
    } else {
        req
    }
}

#[cfg(test)]
mod tests {
    use super::{
        add_dev_local_auth_headers, is_local_dev_request_url, DEV_AUTH_PROXY_HEADER_NAME,
        DEV_PASSKEY_CONTEXT_HEADER_NAME,
    };

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
