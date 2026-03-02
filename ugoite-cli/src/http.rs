use anyhow::{bail, Result};

pub fn http_get(url: &str) -> Result<serde_json::Value> {
    let client = reqwest::blocking::Client::new();
    let req = add_auth_headers(client.get(url));
    let resp = req.send()?;
    if !resp.status().is_success() {
        bail!(
            "HTTP {}: {}",
            resp.status(),
            resp.text().unwrap_or_default()
        );
    }
    Ok(resp.json()?)
}

pub fn http_post(url: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
    let client = reqwest::blocking::Client::new();
    let req = add_auth_headers(client.post(url).json(body));
    let resp = req.send()?;
    if !resp.status().is_success() {
        bail!(
            "HTTP {}: {}",
            resp.status(),
            resp.text().unwrap_or_default()
        );
    }
    Ok(resp.json().unwrap_or(serde_json::Value::Null))
}

pub fn http_put(url: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
    let client = reqwest::blocking::Client::new();
    let req = add_auth_headers(client.put(url).json(body));
    let resp = req.send()?;
    if !resp.status().is_success() {
        bail!(
            "HTTP {}: {}",
            resp.status(),
            resp.text().unwrap_or_default()
        );
    }
    Ok(resp.json().unwrap_or(serde_json::Value::Null))
}

pub fn http_patch(url: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
    let client = reqwest::blocking::Client::new();
    let req = add_auth_headers(client.patch(url).json(body));
    let resp = req.send()?;
    if !resp.status().is_success() {
        bail!(
            "HTTP {}: {}",
            resp.status(),
            resp.text().unwrap_or_default()
        );
    }
    Ok(resp.json().unwrap_or(serde_json::Value::Null))
}

pub fn http_delete(url: &str) -> Result<serde_json::Value> {
    let client = reqwest::blocking::Client::new();
    let req = add_auth_headers(client.delete(url));
    let resp = req.send()?;
    if !resp.status().is_success() {
        bail!(
            "HTTP {}: {}",
            resp.status(),
            resp.text().unwrap_or_default()
        );
    }
    Ok(resp.json().unwrap_or(serde_json::Value::Null))
}

fn add_auth_headers(req: reqwest::blocking::RequestBuilder) -> reqwest::blocking::RequestBuilder {
    let mut r = req;
    if let Ok(token) = std::env::var("UGOITE_AUTH_BEARER_TOKEN") {
        r = r.header("Authorization", format!("Bearer {token}"));
    } else if let Ok(key) = std::env::var("UGOITE_AUTH_API_KEY") {
        r = r.header("X-Api-Key", key);
    }
    r
}
