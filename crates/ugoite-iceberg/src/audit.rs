use anyhow::{anyhow, bail, Result};
use chrono::{SecondsFormat, Utc};
use opendal::Operator;
use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use tokio::sync::Mutex;

const DEFAULT_AUDIT_LIMIT: usize = 100;
const MAX_AUDIT_LIMIT: usize = 500;
const DEFAULT_AUDIT_RETENTION: usize = 5000;
const MAX_AUDIT_RETENTION: usize = 50000;

#[derive(Debug, Clone)]
pub struct AuditListOptions {
    pub offset: usize,
    pub limit: usize,
    pub action: Option<String>,
    pub actor_principal_id: Option<String>,
    pub outcome: Option<String>,
}

impl Default for AuditListOptions {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: DEFAULT_AUDIT_LIMIT,
            action: None,
            actor_principal_id: None,
            outcome: None,
        }
    }
}

static SPACE_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
static SPACE_ID_PATTERN: OnceLock<Regex> = OnceLock::new();

fn lock_registry() -> &'static Mutex<HashMap<String, Arc<Mutex<()>>>> {
    SPACE_LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn space_lock(space_id: &str) -> Arc<Mutex<()>> {
    let mut registry = lock_registry().lock().await;
    if let Some(existing) = registry.get(space_id) {
        return existing.clone();
    }
    let created = Arc::new(Mutex::new(()));
    registry.insert(space_id.to_string(), created.clone());
    created
}

fn normalize_retention_limit(limit: Option<usize>) -> usize {
    let raw = limit.unwrap_or(DEFAULT_AUDIT_RETENTION);
    raw.clamp(100, MAX_AUDIT_RETENTION)
}

fn normalize_outcome(outcome: Option<&str>) -> String {
    let normalized = outcome.unwrap_or("success").trim().to_lowercase();
    match normalized.as_str() {
        "success" | "deny" | "error" => normalized,
        _ => "success".to_string(),
    }
}

fn validate_space_id(space_id: &str) -> Result<String> {
    let normalized = space_id.trim();
    if normalized.is_empty() {
        return Err(anyhow!("space_id must not be empty"));
    }
    let pattern = SPACE_ID_PATTERN.get_or_init(|| {
        Regex::new(r"^[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}$").expect("space id regex must be valid")
    });
    if !pattern.is_match(normalized) {
        return Err(anyhow!("invalid space_id"));
    }
    Ok(normalized.to_string())
}

fn audit_file_path(space_id: &str) -> String {
    format!("spaces/{space_id}/audit/events.jsonl")
}

fn audit_event_id_path(space_id: &str, event_id: &str) -> String {
    format!("spaces/{space_id}/audit/event-ids/{event_id}.json")
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn event_hash(payload: &Value, prev_hash: &str) -> Result<String> {
    let canonical = serde_json::to_string(payload)?;
    let material = format!("{prev_hash}:{canonical}");
    let digest = Sha256::digest(material.as_bytes());
    Ok(hex::encode(digest))
}

fn verify_chain(events: &[Value]) -> Result<()> {
    let mut prev_hash = "root".to_string();
    for event in events {
        let mut candidate = event.clone();
        let object = candidate
            .as_object_mut()
            .ok_or_else(|| anyhow!("Audit log contains malformed JSON"))?;
        let expected_hash = object
            .remove("event_hash")
            .and_then(|v| v.as_str().map(str::to_string))
            .ok_or_else(|| anyhow!("Audit event missing event_hash"))?;
        let candidate_prev_hash = object
            .get("prev_hash")
            .and_then(Value::as_str)
            .unwrap_or("root");
        if candidate_prev_hash != prev_hash {
            return Err(anyhow!("Audit chain prev_hash mismatch"));
        }
        let actual_hash = event_hash(&candidate, &prev_hash)?;
        if actual_hash != expected_hash {
            return Err(anyhow!("Audit chain integrity check failed"));
        }
        prev_hash = expected_hash;
    }
    Ok(())
}

fn rehash_chain(events: &mut [Value]) -> Result<()> {
    let mut prev_hash = "root".to_string();
    for event in events.iter_mut() {
        {
            let object = event
                .as_object_mut()
                .ok_or_else(|| anyhow!("Audit log contains malformed JSON"))?;
            object.insert("prev_hash".to_string(), Value::String(prev_hash.clone()));
            object.remove("event_hash");
        }
        let hash = event_hash(event, &prev_hash)?;
        let object = event
            .as_object_mut()
            .ok_or_else(|| anyhow!("Audit log contains malformed JSON"))?;
        object.insert("event_hash".to_string(), Value::String(hash.clone()));
        prev_hash = hash;
    }
    Ok(())
}

async fn read_events(op: &Operator, space_id: &str) -> Result<Vec<Value>> {
    let path = audit_file_path(space_id);
    if !op.exists(&path).await? {
        return Ok(Vec::new());
    }
    let bytes = op.read(&path).await?;
    let content = String::from_utf8(bytes.to_vec())?;
    let mut events = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parsed: Value = serde_json::from_str(trimmed)
            .map_err(|_| anyhow!("Audit log contains malformed JSON"))?;
        if parsed.is_object() {
            events.push(parsed);
        }
    }
    Ok(events)
}

async fn write_events(
    op: &Operator,
    space_id: &str,
    events: &[Value],
    expected_version: Option<&str>,
) -> Result<()> {
    let dir_path = format!("spaces/{space_id}/audit/");
    op.create_dir(&dir_path).await?;
    let path = audit_file_path(space_id);
    let mut lines = Vec::with_capacity(events.len());
    for item in events {
        lines.push(serde_json::to_string(item)?);
    }
    let mut payload = lines.join("\n");
    if !payload.is_empty() {
        payload.push('\n');
    }
    let bytes = payload.into_bytes();
    if let Some(version) = expected_version {
        op.write_with(&path, bytes).if_match(version).await?;
    } else if op.info().full_capability().write_with_if_not_exists && !op.exists(&path).await? {
        op.write_with(&path, bytes).if_not_exists(true).await?;
    } else if matches!(op.info().scheme(), "memory" | "fs" | "file") {
        op.write(&path, bytes).await?;
    } else {
        bail!("audit append requires conditional storage capabilities");
    }
    Ok(())
}

pub async fn append_audit_event(
    op: &Operator,
    space_id: &str,
    payload: &Value,
    retention_limit: Option<usize>,
) -> Result<Value> {
    let mut last_conflict = None;
    for _attempt in 0..3 {
        match append_audit_event_once(op, space_id, payload, retention_limit).await {
            Ok(event) => return Ok(event),
            Err(error)
                if {
                    let message = error.to_string().to_lowercase();
                    message.contains("precondition")
                        || message.contains("condition")
                        || message.contains("already exists")
                } =>
            {
                last_conflict = Some(error);
                continue;
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_conflict.unwrap_or_else(|| anyhow!("audit append conflicted after bounded retries")))
}

async fn append_audit_event_once(
    op: &Operator,
    space_id: &str,
    payload: &Value,
    retention_limit: Option<usize>,
) -> Result<Value> {
    let safe_space_id = validate_space_id(space_id)?;
    let payload_obj = payload
        .as_object()
        .ok_or_else(|| anyhow!("audit payload must be an object"))?;

    let action = payload_obj
        .get("action")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("audit action must not be empty"))?
        .to_string();

    let subject_principal_id = payload_obj
        .get("subject_principal_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("subject_principal_id must not be empty"))?
        .to_string();
    let actor_principal_id = payload_obj
        .get("actor_principal_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let lock = space_lock(&safe_space_id).await;
    let _guard = lock.lock().await;

    let path = audit_file_path(&safe_space_id);
    let capabilities = op.info().full_capability();
    let path_exists = op.exists(&path).await?;
    let local_process_store = matches!(op.info().scheme(), "memory" | "fs" | "file");
    if !local_process_store
        && ((path_exists && !capabilities.write_with_if_match)
            || (!path_exists && !capabilities.write_with_if_not_exists))
    {
        bail!("audit append requires conditional storage capabilities");
    }
    let expected_version = if path_exists && capabilities.write_with_if_match {
        let metadata = op.stat(&path).await?;
        metadata
            .etag()
            .or_else(|| metadata.version())
            .map(str::to_string)
    } else {
        None
    };
    let requested_event_id = payload_obj
        .get("event_id")
        .and_then(Value::as_str)
        .and_then(|value| uuid::Uuid::parse_str(value).ok())
        .map(|value| value.to_string());
    if let Some(event_id) = &requested_event_id {
        let marker_path = audit_event_id_path(&safe_space_id, event_id);
        if op.exists(&marker_path).await? {
            let marker = op.read(&marker_path).await?;
            return Ok(serde_json::from_slice(&marker.to_vec())?);
        }
    }
    let mut events = read_events(op, &safe_space_id).await?;
    verify_chain(&events)?;

    // Recovery outbox delivery supplies a stable event_id. Treat it as the
    // idempotency key before touching the hash chain so a retried delivery
    // never creates a second visible event.
    if let Some(event_id) = &requested_event_id {
        if let Some(existing) = events
            .iter()
            .find(|event| event.get("event_id").and_then(Value::as_str) == Some(event_id.as_str()))
        {
            return Ok(existing.clone());
        }
    }

    let prev_hash = events
        .last()
        .and_then(Value::as_object)
        .and_then(|item| item.get("event_hash"))
        .and_then(Value::as_str)
        .unwrap_or("root")
        .to_string();

    let metadata = payload_obj
        .get("metadata")
        .and_then(Value::as_object)
        .map(|_| {
            payload_obj
                .get("metadata")
                .cloned()
                .unwrap_or_else(|| json!({}))
        })
        .unwrap_or_else(|| json!({}));
    validate_safe_metadata(&metadata)?;

    let mut event = json!({
        "event_id": requested_event_id.unwrap_or_else(|| uuid::Uuid::now_v7().to_string()),
        "timestamp": now_iso(),
        "space_id": safe_space_id,
        "action": action,
        "subject_principal_id": subject_principal_id,
        "actor_principal_id": actor_principal_id,
        "credential_id": payload_obj.get("credential_id").cloned().unwrap_or(Value::Null),
        "outcome": normalize_outcome(payload_obj.get("outcome").and_then(Value::as_str)),
        "target_type": payload_obj.get("target_type").cloned().unwrap_or(Value::Null),
        "target_id": payload_obj.get("target_id").cloned().unwrap_or(Value::Null),
        "request_method": payload_obj.get("request_method").cloned().unwrap_or(Value::Null),
        "request_path": payload_obj.get("request_path").cloned().unwrap_or(Value::Null),
        "request_id": payload_obj.get("request_id").cloned().unwrap_or(Value::Null),
        "metadata": metadata,
        "prev_hash": prev_hash,
    });

    let hash = event_hash(&event, event["prev_hash"].as_str().unwrap_or("root"))?;
    event["event_hash"] = Value::String(hash);
    events.push(event.clone());

    let retention = normalize_retention_limit(retention_limit);
    if events.len() > retention {
        let start_index = events.len() - retention;
        events = events.split_off(start_index);
        rehash_chain(&mut events)?;
        if let Some(last) = events.last() {
            event = last.clone();
        }
    }

    write_events(op, &safe_space_id, &events, expected_version.as_deref()).await?;
    if let Some(event_id) = event["event_id"].as_str() {
        let marker_path = audit_event_id_path(&safe_space_id, event_id);
        let marker_bytes = serde_json::to_vec(&event)?;
        op.create_dir(&format!("spaces/{}/audit/event-ids/", safe_space_id))
            .await?;
        if capabilities.write_with_if_not_exists {
            op.write_with(&marker_path, marker_bytes)
                .if_not_exists(true)
                .await?;
        } else if !local_process_store {
            bail!("audit event marker requires conditional storage capabilities");
        } else if !op.exists(&marker_path).await? {
            op.write(&marker_path, marker_bytes).await?;
        }
    }
    Ok(event)
}

fn validate_safe_metadata(value: &Value) -> Result<()> {
    let Some(object) = value.as_object() else {
        return Ok(());
    };
    for (key, child) in object {
        let normalized = key.to_ascii_lowercase();
        if matches!(
            normalized.as_str(),
            "token"
                | "owner_approval_token"
                | "recovery_codes"
                | "code_hashes"
                | "totp_secret"
                | "totp_secret_encrypted"
                | "token_hash"
                | "secret"
                | "credential_material"
        ) {
            bail!("audit metadata contains secret material");
        }
        match child {
            Value::Object(_) => validate_safe_metadata(child)?,
            Value::Array(items) => {
                for item in items {
                    validate_safe_metadata(item)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub async fn list_audit_events(
    op: &Operator,
    space_id: &str,
    options: AuditListOptions,
) -> Result<Value> {
    let safe_space_id = validate_space_id(space_id)?;
    let lock = space_lock(&safe_space_id).await;
    let _guard = lock.lock().await;

    let mut events = read_events(op, &safe_space_id).await?;
    verify_chain(&events)?;

    let action = options
        .action
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let actor = options
        .actor_principal_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let outcome = options
        .outcome
        .as_deref()
        .map(str::trim)
        .map(str::to_lowercase)
        .filter(|value| !value.is_empty());

    events.retain(|event| {
        let Some(obj) = event.as_object() else {
            return false;
        };
        if let Some(action_value) = action {
            if obj.get("action").and_then(Value::as_str) != Some(action_value) {
                return false;
            }
        }
        if let Some(actor_value) = actor {
            if obj.get("actor_principal_id").and_then(Value::as_str) != Some(actor_value) {
                return false;
            }
        }
        if let Some(ref outcome_value) = outcome {
            if obj.get("outcome").and_then(Value::as_str) != Some(outcome_value.as_str()) {
                return false;
            }
        }
        true
    });

    events.sort_by(|left, right| {
        let left_ts = left
            .as_object()
            .and_then(|obj| obj.get("timestamp"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let right_ts = right
            .as_object()
            .and_then(|obj| obj.get("timestamp"))
            .and_then(Value::as_str)
            .unwrap_or("");
        right_ts.cmp(left_ts)
    });

    let normalized_limit = options.limit.clamp(1, MAX_AUDIT_LIMIT);
    let normalized_offset = options.offset;
    let total = events.len();
    let items: Vec<Value> = events
        .into_iter()
        .skip(normalized_offset)
        .take(normalized_limit)
        .collect();

    Ok(json!({
        "items": items,
        "total": total,
        "offset": normalized_offset,
        "limit": normalized_limit,
    }))
}

pub fn default_retention_from_env() -> usize {
    let parsed = std::env::var("UGOITE_AUDIT_RETENTION_MAX_EVENTS")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok());
    normalize_retention_limit(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ugoite_storage::operator_from_uri;

    #[tokio::test]
    async fn test_req_sec_013_cross_process_conditional_append_deduplicates_event() -> Result<()> {
        let op = operator_from_uri("memory://audit-dedupe")?;
        let event_id = uuid::Uuid::now_v7().to_string();
        let payload = json!({
            "event_id": event_id,
            "action": "recovery.owner_reset_completed",
            "subject_principal_id": uuid::Uuid::now_v7().to_string(),
            "actor_principal_id": uuid::Uuid::now_v7().to_string(),
            "metadata": {"safe": true}
        });
        let first = append_audit_event(&op, "demo", &payload, None).await?;
        let second = append_audit_event(&op, "demo", &payload, None).await?;
        assert_eq!(first["event_id"], event_id);
        assert_eq!(second["event_id"], event_id);
        assert_eq!(
            list_audit_events(&op, "demo", AuditListOptions::default()).await?["total"],
            1
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_req_sec_013_retained_event_id_survives_log_compaction() -> Result<()> {
        let op = operator_from_uri("memory://audit-retention")?;
        let retained = json!({
            "event_id": uuid::Uuid::now_v7().to_string(),
            "action": "recovery.backup_codes_rotated",
            "subject_principal_id": uuid::Uuid::now_v7().to_string(),
            "actor_principal_id": uuid::Uuid::now_v7().to_string()
        });
        let first = append_audit_event(&op, "demo", &retained, Some(1)).await?;
        append_audit_event(
            &op,
            "demo",
            &json!({
                "event_id": uuid::Uuid::now_v7().to_string(),
                "action": "recovery.owner_approval_issued",
                "subject_principal_id": uuid::Uuid::now_v7().to_string(),
                "actor_principal_id": uuid::Uuid::now_v7().to_string()
            }),
            Some(1),
        )
        .await?;
        let replay = append_audit_event(&op, "demo", &retained, Some(1)).await?;
        assert_eq!(first["event_id"], retained["event_id"]);
        assert_eq!(replay["event_id"], retained["event_id"]);
        Ok(())
    }
}
