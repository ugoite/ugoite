use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use serde_json::{json, Map, Value};
use sha2::Sha256;
use std::collections::{HashMap, HashSet};
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

const AUTH_HEADER_PARTS: usize = 2;
const SIGNED_TOKEN_PARTS: usize = 3;

#[derive(Debug, Clone)]
pub struct CoreAuthError {
    pub code: String,
    pub detail: String,
    pub status_code: i32,
}

impl CoreAuthError {
    fn new(code: &str, detail: &str) -> Self {
        Self {
            code: code.to_string(),
            detail: detail.to_string(),
            status_code: 401,
        }
    }

    pub fn as_json(&self) -> Value {
        json!({
            "code": self.code,
            "detail": self.detail,
            "status_code": self.status_code,
        })
    }
}

#[derive(Debug, Clone)]
struct CredentialRecord {
    user_id: String,
    principal_type: String,
    display_name: Option<String>,
    key_id: Option<String>,
    disabled: bool,
    scopes: Vec<String>,
    scope_enforced: bool,
    service_account_id: Option<String>,
}

fn verify_digest(stored: &str, computed: &str) -> bool {
    if stored.len() != computed.len() {
        return false;
    }
    bool::from(stored.as_bytes().ct_eq(computed.as_bytes()))
}

fn parse_json_map(raw: Option<&str>) -> Map<String, Value> {
    let Some(raw_text) = raw else {
        return Map::new();
    };
    let Ok(parsed) = serde_json::from_str::<Value>(raw_text) else {
        return Map::new();
    };
    parsed.as_object().cloned().unwrap_or_default()
}

fn parse_key_value_map(raw: Option<&str>) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let Some(raw_text) = raw else {
        return result;
    };
    for pair in raw_text.split(',') {
        let item = pair.trim();
        if item.is_empty() {
            continue;
        }
        let mut parts = item.splitn(2, ':');
        let Some(key) = parts.next() else {
            continue;
        };
        let Some(value) = parts.next() else {
            continue;
        };
        let key_trimmed = key.trim();
        let value_trimmed = value.trim();
        if !key_trimmed.is_empty() && !value_trimmed.is_empty() {
            result.insert(key_trimmed.to_string(), value_trimmed.to_string());
        }
    }
    result
}

fn parse_string_set(raw: Option<&str>) -> HashSet<String> {
    let mut result = HashSet::new();
    let Some(raw_text) = raw else {
        return result;
    };
    for item in raw_text.split(',') {
        let token = item.trim();
        if !token.is_empty() {
            result.insert(token.to_string());
        }
    }
    result
}

fn parse_scopes(value: Option<&Value>) -> Vec<String> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    let mut scopes: Vec<String> = items
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|scope| !scope.is_empty())
        .map(ToString::to_string)
        .collect();
    scopes.sort();
    scopes.dedup();
    scopes
}

fn parse_record_map(raw: Option<&str>) -> HashMap<String, CredentialRecord> {
    let mut records = HashMap::new();
    for (credential, entry) in parse_json_map(raw) {
        let Some(obj) = entry.as_object() else {
            continue;
        };
        let Some(user_id) = obj.get("user_id").and_then(Value::as_str) else {
            continue;
        };
        if user_id.is_empty() {
            continue;
        }

        let principal_type = obj
            .get("principal_type")
            .and_then(Value::as_str)
            .unwrap_or("user");
        if principal_type != "user" && principal_type != "service" {
            continue;
        }

        let display_name = obj
            .get("display_name")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        let key_id = obj
            .get("key_id")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        let service_account_id = obj
            .get("service_account_id")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        let disabled = obj
            .get("disabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let scopes = parse_scopes(obj.get("scopes"));
        let scope_enforced = obj
            .get("scope_enforced")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        records.insert(
            credential,
            CredentialRecord {
                user_id: user_id.to_string(),
                principal_type: principal_type.to_string(),
                display_name,
                key_id,
                disabled,
                scopes,
                scope_enforced,
                service_account_id,
            },
        );
    }
    records
}

fn identity_from_record(record: &CredentialRecord, auth_method: &str) -> Value {
    json!({
        "user_id": record.user_id,
        "principal_type": record.principal_type,
        "display_name": record.display_name,
        "auth_method": auth_method,
        "key_id": record.key_id,
        "scopes": record.scopes,
        "scope_enforced": record.scope_enforced,
        "service_account_id": record.service_account_id,
    })
}

fn authenticate_signed_bearer(
    token: &str,
    signing_secrets: &HashMap<String, String>,
    active_kids: &HashSet<String>,
    revoked_key_ids: &HashSet<String>,
) -> Result<Value, CoreAuthError> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != SIGNED_TOKEN_PARTS {
        return Err(CoreAuthError::new(
            "invalid_signature",
            "Malformed signed bearer token",
        ));
    }

    let payload_segment = parts[1];
    let signature_segment = parts[2];
    let payload_bytes = URL_SAFE_NO_PAD
        .decode(payload_segment)
        .map_err(|_| CoreAuthError::new("invalid_signature", "Malformed signed bearer token"))?;
    let signature_bytes = URL_SAFE_NO_PAD
        .decode(signature_segment)
        .map_err(|_| CoreAuthError::new("invalid_signature", "Malformed signed bearer token"))?;

    let payload: Value = serde_json::from_slice(&payload_bytes)
        .map_err(|_| CoreAuthError::new("invalid_signature", "Invalid signed token payload"))?;
    let payload_obj = payload
        .as_object()
        .ok_or_else(|| CoreAuthError::new("invalid_signature", "Invalid signed token payload"))?;

    let kid = payload_obj
        .get("kid")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CoreAuthError::new("invalid_signature", "Signed token missing key id"))?;

    if !active_kids.is_empty() && !active_kids.contains(kid) {
        return Err(CoreAuthError::new(
            "revoked_key",
            "Token signed by inactive key",
        ));
    }
    if revoked_key_ids.contains(kid) {
        return Err(CoreAuthError::new(
            "revoked_key",
            "Token key id has been revoked",
        ));
    }

    let secret = signing_secrets
        .get(kid)
        .ok_or_else(|| CoreAuthError::new("invalid_signature", "Unknown token signing key"))?;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|_| CoreAuthError::new("invalid_signature", "Invalid token signing key"))?;
    mac.update(payload_segment.as_bytes());
    let expected = mac.finalize().into_bytes();
    let expected_hex = hex::encode(expected);
    let actual_hex = hex::encode(signature_bytes);
    if !verify_digest(&expected_hex, &actual_hex) {
        return Err(CoreAuthError::new(
            "invalid_signature",
            "Invalid bearer token signature",
        ));
    }

    let exp = payload_obj
        .get("exp")
        .and_then(Value::as_f64)
        .ok_or_else(|| CoreAuthError::new("invalid_credentials", "Signed token missing exp"))?;
    let now = chrono::Utc::now().timestamp() as f64;
    if exp < now {
        return Err(CoreAuthError::new(
            "expired_token",
            "Bearer token has expired",
        ));
    }

    let user_id = payload_obj
        .get("sub")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CoreAuthError::new("invalid_credentials", "Signed token missing subject"))?;
    if payload_obj
        .get("disabled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(CoreAuthError::new(
            "disabled_identity",
            "Principal is disabled",
        ));
    }

    let principal_type = payload_obj
        .get("principal_type")
        .and_then(Value::as_str)
        .unwrap_or("user");
    if principal_type != "user" && principal_type != "service" {
        return Err(CoreAuthError::new(
            "invalid_credentials",
            "Invalid principal type",
        ));
    }

    let display_name = payload_obj
        .get("display_name")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let service_account_id = payload_obj
        .get("service_account_id")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let scopes = parse_scopes(payload_obj.get("scopes"));
    let scope_enforced = payload_obj
        .get("scope_enforced")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    Ok(json!({
        "user_id": user_id,
        "principal_type": principal_type,
        "display_name": display_name,
        "auth_method": "bearer",
        "key_id": kid,
        "scopes": scopes,
        "scope_enforced": scope_enforced,
        "service_account_id": service_account_id,
    }))
}

#[allow(clippy::too_many_arguments)]
pub fn authenticate_headers_core(
    authorization: Option<&str>,
    api_key: Option<&str>,
    bearer_tokens_json: Option<&str>,
    api_keys_json: Option<&str>,
    bearer_secrets: Option<&str>,
    active_kids_raw: Option<&str>,
    revoked_key_ids_raw: Option<&str>,
    bootstrap_token: Option<&str>,
    bootstrap_user_id: Option<&str>,
) -> Value {
    let mut bearer_tokens = parse_record_map(bearer_tokens_json);
    if bearer_tokens.is_empty() {
        if let Some(token) = bootstrap_token.filter(|value| !value.trim().is_empty()) {
            bearer_tokens.insert(
                token.to_string(),
                CredentialRecord {
                    user_id: bootstrap_user_id
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or("bootstrap-user")
                        .to_string(),
                    principal_type: "user".to_string(),
                    display_name: Some("Local Bootstrap User".to_string()),
                    key_id: Some("bootstrap".to_string()),
                    disabled: false,
                    scopes: Vec::new(),
                    scope_enforced: false,
                    service_account_id: None,
                },
            );
        }
    }

    let api_keys = parse_record_map(api_keys_json);
    let signing_secrets = parse_key_value_map(bearer_secrets);
    let active_kids = parse_string_set(active_kids_raw);
    let revoked_key_ids = parse_string_set(revoked_key_ids_raw);

    let result = if let Some(auth_header) = authorization.filter(|value| !value.trim().is_empty()) {
        let parts: Vec<&str> = auth_header.splitn(AUTH_HEADER_PARTS, ' ').collect();
        if parts.len() != AUTH_HEADER_PARTS || parts[0].to_lowercase() != "bearer" {
            Err(CoreAuthError::new(
                "invalid_credentials",
                "Authorization header must use Bearer scheme",
            ))
        } else {
            let token = parts[1].trim();
            if token.is_empty() {
                Err(CoreAuthError::new(
                    "missing_credentials",
                    "Missing bearer token",
                ))
            } else if token.starts_with("v1.") {
                authenticate_signed_bearer(token, &signing_secrets, &active_kids, &revoked_key_ids)
            } else {
                let record = bearer_tokens.get(token).ok_or_else(|| {
                    CoreAuthError::new("invalid_credentials", "Invalid bearer token")
                });
                match record {
                    Ok(record) => {
                        if record
                            .key_id
                            .as_ref()
                            .is_some_and(|key_id| revoked_key_ids.contains(key_id))
                        {
                            Err(CoreAuthError::new(
                                "revoked_key",
                                "Bearer token has been revoked",
                            ))
                        } else if record.disabled {
                            Err(CoreAuthError::new(
                                "disabled_identity",
                                "Principal is disabled",
                            ))
                        } else {
                            Ok(identity_from_record(record, "bearer"))
                        }
                    }
                    Err(err) => Err(err),
                }
            }
        }
    } else if let Some(raw_key) = api_key.filter(|value| !value.trim().is_empty()) {
        let key_value = raw_key.trim();
        let record = api_keys
            .get(key_value)
            .ok_or_else(|| CoreAuthError::new("invalid_credentials", "Invalid API key"));
        match record {
            Ok(record) => {
                if record
                    .key_id
                    .as_ref()
                    .is_some_and(|key_id| revoked_key_ids.contains(key_id))
                {
                    Err(CoreAuthError::new(
                        "revoked_key",
                        "API key has been revoked",
                    ))
                } else if record.disabled {
                    Err(CoreAuthError::new(
                        "disabled_identity",
                        "Principal is disabled",
                    ))
                } else {
                    Ok(identity_from_record(record, "api_key"))
                }
            }
            Err(err) => Err(err),
        }
    } else {
        Err(CoreAuthError::new(
            "missing_credentials",
            "Authentication required. Provide Authorization: Bearer <token> or X-API-Key.",
        ))
    };

    match result {
        Ok(identity) => json!({"ok": true, "identity": identity}),
        Err(error) => json!({"ok": false, "error": error.as_json()}),
    }
}

pub fn auth_capabilities_snapshot(
    bearer_tokens_json: Option<&str>,
    api_keys_json: Option<&str>,
    bearer_secrets: Option<&str>,
    active_kids_raw: Option<&str>,
    revoked_key_ids_raw: Option<&str>,
) -> Value {
    let bearer_tokens = parse_record_map(bearer_tokens_json);
    let api_keys = parse_record_map(api_keys_json);
    let signing_secrets = parse_key_value_map(bearer_secrets);
    let mut active_kids: Vec<String> = parse_string_set(active_kids_raw).into_iter().collect();
    active_kids.sort();
    let mut revoked_key_ids: Vec<String> =
        parse_string_set(revoked_key_ids_raw).into_iter().collect();
    revoked_key_ids.sort();

    json!({
        "version": "m4-auth-rust-base-v1",
        "enforcement": {
            "mandatory_authentication": true,
            "localhost_requires_authentication": true,
            "remote_requires_authentication": true
        },
        "providers": {
            "bearer": {
                "supports_static_tokens": true,
                "supports_signed_tokens": true,
                "configured_static_token_count": bearer_tokens.len(),
                "configured_signing_kid_count": signing_secrets.len(),
                "active_kids": active_kids
            },
            "api_key": {
                "supports_static_api_keys": true,
                "supports_space_service_account_keys": true,
                "configured_static_api_key_count": api_keys.len(),
                "revoked_key_ids": revoked_key_ids
            }
        },
        "identity_model": {
            "principal_types": ["user", "service"],
            "fields": [
                "user_id",
                "principal_type",
                "display_name",
                "auth_method",
                "key_id",
                "scopes",
                "scope_enforced",
                "service_account_id"
            ]
        }
    })
}
