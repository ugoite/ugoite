use anyhow::{anyhow, Result};
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
    pub actor_user_id: Option<String>,
    pub outcome: Option<String>,
}

impl Default for AuditListOptions {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: DEFAULT_AUDIT_LIMIT,
            action: None,
            actor_user_id: None,
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

async fn write_events(op: &Operator, space_id: &str, events: &[Value]) -> Result<()> {
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
    op.write(&path, payload.into_bytes()).await?;
    Ok(())
}

pub async fn append_audit_event(
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

    let actor_user_id = payload_obj
        .get("actor_user_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("actor_user_id must not be empty"))?
        .to_string();

    let lock = space_lock(&safe_space_id).await;
    let _guard = lock.lock().await;

    let mut events = read_events(op, &safe_space_id).await?;
    verify_chain(&events)?;

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

    let mut event = json!({
        "id": format!("audit-{}", uuid::Uuid::new_v4().simple()),
        "timestamp": now_iso(),
        "space_id": safe_space_id,
        "action": action,
        "actor_user_id": actor_user_id,
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

    write_events(op, &safe_space_id, &events).await?;
    Ok(event)
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
        .actor_user_id
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
            if obj.get("actor_user_id").and_then(Value::as_str) != Some(actor_value) {
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
