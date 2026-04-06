use crate::config::{effective_api_key, effective_bearer_token};
use anyhow::{bail, Result};

const DEV_AUTH_PROXY_HEADER_NAME: &str = "x-ugoite-dev-auth-proxy-token";
const DEV_PASSKEY_CONTEXT_HEADER_NAME: &str = "x-ugoite-dev-passkey-context";

pub async fn http_get(url: &str) -> Result<serde_json::Value> {
    let client = reqwest::Client::new();
    let req = add_auth_headers(client.get(url));
    let resp = req.send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        bail!("HTTP {}: {}", status, resp.text().await.unwrap_or_default());
    }
    Ok(resp.json().await?)
}

pub async fn http_post(url: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
    let client = reqwest::Client::new();
    let req = add_auth_headers(client.post(url).json(body));
    let resp = req.send().await?;
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
    let req = add_dev_local_auth_headers(add_auth_headers(client.post(url).json(body)));
    let resp = req.send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        bail!("HTTP {}: {}", status, resp.text().await.unwrap_or_default());
    }
    Ok(resp.json().await.unwrap_or(serde_json::Value::Null))
}

pub async fn http_put(url: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
    let client = reqwest::Client::new();
    let req = add_auth_headers(client.put(url).json(body));
    let resp = req.send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        bail!("HTTP {}: {}", status, resp.text().await.unwrap_or_default());
    }
    Ok(resp.json().await.unwrap_or(serde_json::Value::Null))
}

pub async fn http_patch(url: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
    let client = reqwest::Client::new();
    let req = add_auth_headers(client.patch(url).json(body));
    let resp = req.send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        bail!("HTTP {}: {}", status, resp.text().await.unwrap_or_default());
    }
    Ok(resp.json().await.unwrap_or(serde_json::Value::Null))
}

pub async fn http_delete(url: &str) -> Result<serde_json::Value> {
    let client = reqwest::Client::new();
    let req = add_auth_headers(client.delete(url));
    let resp = req.send().await?;
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

fn add_dev_local_auth_headers(req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    let req = if let Ok(token) = std::env::var("UGOITE_DEV_AUTH_PROXY_TOKEN") {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            req
        } else {
            req.header(DEV_AUTH_PROXY_HEADER_NAME, trimmed)
        }
    } else {
        req
    };

    let Ok(context) = std::env::var("UGOITE_DEV_PASSKEY_CONTEXT") else {
        return req;
    };
    let trimmed = context.trim();
    if trimmed.is_empty() {
        return req;
    }
    req.header(DEV_PASSKEY_CONTEXT_HEADER_NAME, trimmed)
}
