use anyhow::{anyhow, bail, Result};
use chrono::{SecondsFormat, Utc};
use fs2::FileExt;
use opendal::Operator;
use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::{Arc, OnceLock};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    path::Path,
};
use tokio::sync::Mutex;
use ugoite_core::error::{AppError, ErrorCode};

const DEFAULT_AUDIT_LIMIT: usize = 100;
const MAX_AUDIT_LIMIT: usize = 500;
const DEFAULT_AUDIT_RETENTION: usize = 5000;
const MAX_AUDIT_RETENTION: usize = 50000;

/// Normalize a protocol-supplied audit page before any `usize` conversion.
///
/// The effective range after normalization/clamping is 1..=500: `limit`
/// clamps into range (`0` normalizes to `1`, never a validation error) and
/// `offset` saturates instead of wrapping, so even the largest protocol
/// integer cannot overflow before the 500 cap applies.
pub fn normalize_audit_page(limit: u64, offset: u64) -> (usize, usize) {
    let limit = limit.clamp(1, MAX_AUDIT_LIMIT as u64);
    (
        usize::try_from(limit).unwrap_or(MAX_AUDIT_LIMIT),
        usize::try_from(offset).unwrap_or(usize::MAX),
    )
}

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

fn audit_event_fingerprint(value: &Value) -> Result<String> {
    let source = value.get("payload").unwrap_or(value);
    let object = source
        .as_object()
        .ok_or_else(|| anyhow!("audit event fingerprint source must be an object"))?;
    serde_json::to_string(&json!({
        "action": object.get("action").cloned().unwrap_or(Value::Null),
        "space_uid": object.get("space_uid").cloned().unwrap_or(Value::Null),
        "challenge_id": object.get("challenge_id").cloned().unwrap_or(Value::Null),
        "issuer_principal_id": object
            .get("issuer_principal_id")
            .cloned()
            .unwrap_or(Value::Null),
        "issuer_account_id": object
            .get("issuer_account_id")
            .cloned()
            .unwrap_or(Value::Null),
        "issuer_credential_id": object
            .get("issuer_credential_id")
            .cloned()
            .unwrap_or(Value::Null),
        "subject_principal_id": object
            .get("subject_principal_id")
            .cloned()
            .unwrap_or(Value::Null),
        "subject_account_id": object
            .get("subject_account_id")
            .cloned()
            .unwrap_or(Value::Null),
        "actor_principal_id": object
            .get("actor_principal_id")
            .cloned()
            .unwrap_or(Value::Null),
        "actor_account_id": object
            .get("actor_account_id")
            .cloned()
            .unwrap_or(Value::Null),
        "credential_id": object
            .get("credential_id")
            .cloned()
            .unwrap_or(Value::Null),
        "outcome": normalize_outcome(object.get("outcome").and_then(Value::as_str)),
        "target_type": object.get("target_type").cloned().unwrap_or(Value::Null),
        "target_id": object.get("target_id").cloned().unwrap_or(Value::Null),
        "request_method": object
            .get("request_method")
            .cloned()
            .unwrap_or(Value::Null),
        "request_path": object
            .get("request_path")
            .cloned()
            .unwrap_or(Value::Null),
        "request_id": object.get("request_id").cloned().unwrap_or(Value::Null),
        "metadata": object.get("metadata").cloned().unwrap_or_else(|| json!({})),
    }))
    .map_err(Into::into)
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
    let Some(bytes) = crate::read_object_exact_optional(op, &path).await? else {
        return Ok(Vec::new());
    };
    let content = String::from_utf8(bytes)?;
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
    } else if op.info().capability().write_with_if_not_exists && !op.exists(&path).await? {
        op.write_with(&path, bytes).if_not_exists(true).await?;
    } else if matches!(op.info().scheme(), "memory" | "fs" | "file") {
        op.write(&path, bytes).await?;
    } else {
        bail!("audit append requires conditional storage capabilities");
    }
    Ok(())
}

fn local_audit_lock(op: &Operator, space_id: &str) -> Result<Option<std::fs::File>> {
    if !matches!(op.info().scheme(), "fs" | "file") {
        return Ok(None);
    }
    let path = Path::new(op.info().root().as_str())
        .join(audit_file_path(space_id))
        .with_extension("jsonl.lock");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    file.lock_exclusive()?;
    Ok(Some(file))
}

async fn read_event_marker(
    op: &Operator,
    space_id: &str,
    event_id: &str,
) -> Result<Option<(Value, Option<String>)>> {
    let path = audit_event_id_path(space_id, event_id);
    let Some((bytes, version)) = crate::read_object_exact_optional_with_etag(op, &path).await?
    else {
        return Ok(None);
    };
    let marker = serde_json::from_slice::<Value>(&bytes)?;
    Ok(Some((marker, version)))
}

async fn create_pending_marker(
    op: &Operator,
    space_id: &str,
    event_id: &str,
    payload: &Value,
) -> Result<Option<String>> {
    let path = audit_event_id_path(space_id, event_id);
    let marker = json!({
        "status": "pending",
        "event_id": event_id,
        "payload": payload,
    });
    let bytes = serde_json::to_vec(&marker)?;
    let capabilities = op.info().capability();
    if capabilities.write_with_if_not_exists {
        if let Err(error) = op.write_with(&path, bytes).if_not_exists(true).await {
            let message = error.to_string().to_lowercase();
            if !message.contains("condition") && !message.contains("exists") {
                return Err(error.into());
            }
            return Ok(read_event_marker(op, space_id, event_id)
                .await?
                .and_then(|(_, version)| version));
        }
    } else if matches!(op.info().scheme(), "memory" | "fs" | "file") {
        if !op.exists(&path).await? {
            op.write(&path, bytes).await?;
        }
    } else {
        bail!("audit event marker requires conditional storage capabilities");
    }
    let metadata = op.stat(&path).await?;
    Ok(metadata
        .etag()
        .or_else(|| metadata.version())
        .map(str::to_string))
}

async fn commit_event_marker(
    op: &Operator,
    space_id: &str,
    event_id: &str,
    expected_version: Option<&str>,
    event: &Value,
) -> Result<()> {
    let path = audit_event_id_path(space_id, event_id);
    let marker = json!({"status": "committed", "event": event});
    let bytes = serde_json::to_vec(&marker)?;
    let capabilities = op.info().capability();
    if let Some(version) = expected_version {
        if capabilities.write_with_if_match {
            op.write_with(&path, bytes).if_match(version).await?;
        } else if matches!(op.info().scheme(), "memory" | "fs" | "file") {
            op.write(&path, bytes).await?;
        } else {
            bail!("audit event marker requires conditional storage capabilities");
        }
    } else if capabilities.write_with_if_not_exists {
        if op.exists(&path).await? {
            if matches!(op.info().scheme(), "memory" | "fs" | "file") {
                op.write(&path, bytes).await?;
            } else {
                let metadata = op.stat(&path).await?;
                let version = metadata
                    .etag()
                    .or_else(|| metadata.version())
                    .ok_or_else(|| anyhow!("audit event marker has no conditional version"))?;
                op.write_with(&path, bytes).if_match(version).await?;
            }
        } else {
            op.write_with(&path, bytes).if_not_exists(true).await?;
        }
    } else if matches!(op.info().scheme(), "memory" | "fs" | "file") {
        op.write(&path, bytes).await?;
    } else {
        bail!("audit event marker requires conditional storage capabilities");
    }
    Ok(())
}

pub async fn append_audit_event(
    op: &Operator,
    space_id: &str,
    payload: &Value,
    retention_limit: Option<usize>,
) -> Result<Value> {
    crate::authorization::Authorizer::new(op.clone()).ensure_authoritative_mutation_contract()?;
    ugoite_storage::verify_publication_mutation_contract(op)
        .await
        .map_err(crate::iceberg_store::storage_mutation_unavailable)?;
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

/// Appends a group of audit events after reading and verifying the Space's
/// hash chain once. This is used by recovery sweeps, where each event already
/// has a stable idempotency key and the complete committed history is known.
/// Pending event markers are written before the chain update and committed
/// after it, preserving the single-event crash-recovery protocol.
pub(crate) async fn append_audit_events(
    op: &Operator,
    space_id: &str,
    payloads: &[Value],
) -> Result<Vec<Value>> {
    if payloads.is_empty() {
        return Ok(Vec::new());
    }
    crate::authorization::Authorizer::new(op.clone()).ensure_authoritative_mutation_contract()?;
    ugoite_storage::verify_publication_mutation_contract(op)
        .await
        .map_err(crate::iceberg_store::storage_mutation_unavailable)?;
    let mut last_conflict = None;
    for _attempt in 0..3 {
        match append_audit_events_once(op, space_id, payloads).await {
            Ok(events) => return Ok(events),
            Err(error)
                if {
                    let message = error.to_string().to_lowercase();
                    message.contains("precondition")
                        || message.contains("condition")
                        || message.contains("already exists")
                } =>
            {
                last_conflict = Some(error);
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_conflict.unwrap_or_else(|| anyhow!("audit append conflicted after bounded retries")))
}

async fn append_audit_events_once(
    op: &Operator,
    space_id: &str,
    payloads: &[Value],
) -> Result<Vec<Value>> {
    let safe_space_id = validate_space_id(space_id)?;
    let lock = space_lock(&safe_space_id).await;
    let _guard = lock.lock().await;
    let _local_lock = local_audit_lock(op, &safe_space_id)?;

    let path = audit_file_path(&safe_space_id);
    let path_exists = op.exists(&path).await?;
    let capabilities = op.info().capability();
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
    let mut events = read_events(op, &safe_space_id).await?;
    verify_chain(&events)?;
    let persisted_event_count = events.len();
    let mut event_indexes = HashMap::with_capacity(events.len() + payloads.len());
    for (index, event) in events.iter().enumerate() {
        if let Some(event_id) = event.get("event_id").and_then(Value::as_str) {
            event_indexes.insert(event_id.to_string(), index);
        }
    }

    let mut output = Vec::with_capacity(payloads.len());
    let mut pending_markers = Vec::with_capacity(payloads.len());
    let mut appended = false;
    let mut marker_directory_created = false;
    for payload in payloads {
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
        let actor_account_id = payload_obj
            .get("actor_account_id")
            .cloned()
            .unwrap_or(Value::Null);
        let space_uid = payload_obj.get("space_uid").cloned().unwrap_or(Value::Null);
        let challenge_id = payload_obj
            .get("challenge_id")
            .cloned()
            .unwrap_or(Value::Null);
        let issuer_principal_id = payload_obj
            .get("issuer_principal_id")
            .cloned()
            .unwrap_or(Value::Null);
        let issuer_account_id = payload_obj
            .get("issuer_account_id")
            .cloned()
            .unwrap_or(Value::Null);
        let issuer_credential_id = payload_obj
            .get("issuer_credential_id")
            .cloned()
            .unwrap_or(Value::Null);
        let subject_account_id = payload_obj
            .get("subject_account_id")
            .cloned()
            .unwrap_or(Value::Null);
        let metadata = payload_obj
            .get("metadata")
            .filter(|value| value.is_object())
            .cloned()
            .unwrap_or_else(|| json!({}));
        validate_safe_metadata(&metadata)?;
        let requested_event_id = payload_obj
            .get("event_id")
            .and_then(Value::as_str)
            .and_then(|value| uuid::Uuid::parse_str(value).ok())
            .map(|value| value.to_string());

        let mut marker_version = None;
        if let Some(event_id) = requested_event_id.as_deref() {
            if let Some((marker, version)) = read_event_marker(op, &safe_space_id, event_id).await?
            {
                if marker.get("status").and_then(Value::as_str) == Some("committed") {
                    let canonical = marker.get("event").unwrap_or(&marker);
                    if audit_event_fingerprint(canonical)? != audit_event_fingerprint(payload)? {
                        bail!("audit event id conflicts with canonical payload");
                    }
                    output.push(canonical.clone());
                    continue;
                }
                if marker.get("event_id").is_some() && marker.get("event_hash").is_some() {
                    if audit_event_fingerprint(&marker)? != audit_event_fingerprint(payload)? {
                        bail!("audit event id conflicts with canonical payload");
                    }
                    output.push(marker.get("event").cloned().unwrap_or(marker));
                    continue;
                }
                if marker.get("status").and_then(Value::as_str) == Some("pending")
                    && audit_event_fingerprint(&marker)? != audit_event_fingerprint(payload)?
                {
                    bail!("audit event id conflicts with pending payload");
                }
                marker_version = version;
            }
            if let Some(index) = event_indexes.get(event_id).copied() {
                let existing = events[index].clone();
                if audit_event_fingerprint(&existing)? != audit_event_fingerprint(payload)? {
                    bail!("audit event id conflicts with canonical payload");
                }
                // Existing chain entries can repair their event marker
                // immediately. Entries appended earlier in this batch must
                // keep the marker pending until the single chain write has
                // succeeded, or a crash could leave a committed marker for
                // an event that never became visible in the hash chain.
                if index < persisted_event_count {
                    let _ = commit_event_marker(
                        op,
                        &safe_space_id,
                        event_id,
                        marker_version.as_deref(),
                        &existing,
                    )
                    .await;
                }
                output.push(existing);
                continue;
            }
        }

        let event_id = requested_event_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
        let prev_hash = events
            .last()
            .and_then(Value::as_object)
            .and_then(|item| item.get("event_hash"))
            .and_then(Value::as_str)
            .unwrap_or("root")
            .to_string();
        let mut event = json!({
            "event_id": event_id.clone(),
            "timestamp": now_iso(),
            "space_id": safe_space_id.clone(),
            "space_uid": space_uid.clone(),
            "challenge_id": challenge_id.clone(),
            "issuer_principal_id": issuer_principal_id.clone(),
            "issuer_account_id": issuer_account_id.clone(),
            "issuer_credential_id": issuer_credential_id.clone(),
            "action": action.clone(),
            "subject_principal_id": subject_principal_id.clone(),
            "subject_account_id": subject_account_id.clone(),
            "actor_principal_id": actor_principal_id.clone(),
            "actor_account_id": actor_account_id.clone(),
            "credential_id": payload_obj.get("credential_id").cloned().unwrap_or(Value::Null),
            "outcome": normalize_outcome(payload_obj.get("outcome").and_then(Value::as_str)),
            "target_type": payload_obj.get("target_type").cloned().unwrap_or(Value::Null),
            "target_id": payload_obj.get("target_id").cloned().unwrap_or(Value::Null),
            "request_method": payload_obj.get("request_method").cloned().unwrap_or(Value::Null),
            "request_path": payload_obj.get("request_path").cloned().unwrap_or(Value::Null),
            "request_id": payload_obj.get("request_id").cloned().unwrap_or(Value::Null),
            "metadata": metadata.clone(),
            "prev_hash": prev_hash,
        });
        let hash = event_hash(&event, event["prev_hash"].as_str().unwrap_or("root"))?;
        event["event_hash"] = Value::String(hash);

        if !marker_directory_created {
            op.create_dir(&format!("spaces/{}/audit/event-ids/", safe_space_id))
                .await?;
            marker_directory_created = true;
        }
        if marker_version.is_none() {
            marker_version = create_pending_marker(
                op,
                &safe_space_id,
                &event_id,
                &json!({
                    "event_id": event_id.clone(),
                    "action": action.clone(),
                    "space_uid": space_uid.clone(),
                    "challenge_id": challenge_id.clone(),
                    "issuer_principal_id": issuer_principal_id.clone(),
                    "issuer_account_id": issuer_account_id.clone(),
                    "issuer_credential_id": issuer_credential_id.clone(),
                    "subject_principal_id": subject_principal_id.clone(),
                    "subject_account_id": subject_account_id.clone(),
                    "actor_principal_id": actor_principal_id.clone(),
                    "actor_account_id": actor_account_id.clone(),
                    "credential_id": payload_obj.get("credential_id").cloned().unwrap_or(Value::Null),
                    "outcome": normalize_outcome(payload_obj.get("outcome").and_then(Value::as_str)),
                    "target_type": payload_obj.get("target_type").cloned().unwrap_or(Value::Null),
                    "target_id": payload_obj.get("target_id").cloned().unwrap_or(Value::Null),
                    "request_method": payload_obj.get("request_method").cloned().unwrap_or(Value::Null),
                    "request_path": payload_obj.get("request_path").cloned().unwrap_or(Value::Null),
                    "request_id": payload_obj.get("request_id").cloned().unwrap_or(Value::Null),
                    "metadata": metadata.clone(),
                }),
            ).await?;
        }
        if let Some((marker, _)) = read_event_marker(op, &safe_space_id, &event_id).await? {
            if marker.get("status").and_then(Value::as_str) == Some("committed") {
                let canonical = marker.get("event").unwrap_or(&marker);
                if audit_event_fingerprint(canonical)? != audit_event_fingerprint(payload)? {
                    bail!("audit event id conflicts with canonical payload");
                }
                output.push(canonical.clone());
                continue;
            }
            if audit_event_fingerprint(&marker)? != audit_event_fingerprint(payload)? {
                bail!("audit event id conflicts with pending payload");
            }
        }
        let index = events.len();
        events.push(event.clone());
        event_indexes.insert(event_id.clone(), index);
        pending_markers.push((event_id, marker_version, event.clone()));
        output.push(event);
        appended = true;
    }

    if appended {
        let retention = normalize_retention_limit(None);
        if events.len() > retention {
            let start_index = events.len() - retention;
            events = events.split_off(start_index);
            rehash_chain(&mut events)?;
        }
        let canonical_by_id: HashMap<&str, &Value> = events
            .iter()
            .filter_map(|event| Some((event.get("event_id")?.as_str()?, event)))
            .collect();
        for (event_id, _, event) in &mut pending_markers {
            if let Some(canonical) = canonical_by_id.get(event_id.as_str()) {
                *event = (*canonical).clone();
            }
        }
        write_events(op, &safe_space_id, &events, expected_version.as_deref()).await?;
    }
    for (event_id, marker_version, event) in pending_markers {
        commit_event_marker(
            op,
            &safe_space_id,
            &event_id,
            marker_version.as_deref(),
            &event,
        )
        .await?;
    }
    Ok(output)
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
    let actor_account_id = payload_obj
        .get("actor_account_id")
        .cloned()
        .unwrap_or(Value::Null);
    let space_uid = payload_obj.get("space_uid").cloned().unwrap_or(Value::Null);
    let challenge_id = payload_obj
        .get("challenge_id")
        .cloned()
        .unwrap_or(Value::Null);
    let issuer_principal_id = payload_obj
        .get("issuer_principal_id")
        .cloned()
        .unwrap_or(Value::Null);
    let issuer_account_id = payload_obj
        .get("issuer_account_id")
        .cloned()
        .unwrap_or(Value::Null);
    let issuer_credential_id = payload_obj
        .get("issuer_credential_id")
        .cloned()
        .unwrap_or(Value::Null);
    let subject_account_id = payload_obj
        .get("subject_account_id")
        .cloned()
        .unwrap_or(Value::Null);
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

    let lock = space_lock(&safe_space_id).await;
    let _guard = lock.lock().await;
    let _local_lock = local_audit_lock(op, &safe_space_id)?;

    let path = audit_file_path(&safe_space_id);
    let capabilities = op.info().capability();
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
    let mut marker_version = None;
    if let Some(event_id) = &requested_event_id {
        if let Some((marker, version)) = read_event_marker(op, &safe_space_id, event_id).await? {
            if marker.get("status").and_then(Value::as_str) == Some("committed") {
                let canonical = marker.get("event").unwrap_or(&marker);
                if audit_event_fingerprint(canonical)? != audit_event_fingerprint(payload)? {
                    bail!("audit event id conflicts with canonical payload");
                }
                return Ok(canonical.clone());
            }
            if marker.get("event_id").is_some() && marker.get("event_hash").is_some() {
                if audit_event_fingerprint(&marker)? != audit_event_fingerprint(payload)? {
                    bail!("audit event id conflicts with canonical payload");
                }
                return Ok(marker.get("event").cloned().unwrap_or(marker));
            }
            if marker.get("status").and_then(Value::as_str) == Some("pending")
                && audit_event_fingerprint(&marker)? != audit_event_fingerprint(payload)?
            {
                bail!("audit event id conflicts with pending payload");
            }
            marker_version = version;
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
            if audit_event_fingerprint(existing)? != audit_event_fingerprint(payload)? {
                bail!("audit event id conflicts with canonical payload");
            }
            let _ = commit_event_marker(
                op,
                &safe_space_id,
                event_id,
                marker_version.as_deref(),
                existing,
            )
            .await;
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

    let mut event = json!({
        "event_id": requested_event_id.unwrap_or_else(|| uuid::Uuid::now_v7().to_string()),
        "timestamp": now_iso(),
        "space_id": safe_space_id.clone(),
        "space_uid": space_uid.clone(),
        "challenge_id": challenge_id.clone(),
        "issuer_principal_id": issuer_principal_id.clone(),
        "issuer_account_id": issuer_account_id.clone(),
        "issuer_credential_id": issuer_credential_id.clone(),
        "action": action.clone(),
        "subject_principal_id": subject_principal_id.clone(),
        "subject_account_id": subject_account_id.clone(),
        "actor_principal_id": actor_principal_id.clone(),
        "actor_account_id": actor_account_id.clone(),
        "credential_id": payload_obj.get("credential_id").cloned().unwrap_or(Value::Null),
        "outcome": normalize_outcome(payload_obj.get("outcome").and_then(Value::as_str)),
        "target_type": payload_obj.get("target_type").cloned().unwrap_or(Value::Null),
        "target_id": payload_obj.get("target_id").cloned().unwrap_or(Value::Null),
        "request_method": payload_obj.get("request_method").cloned().unwrap_or(Value::Null),
        "request_path": payload_obj.get("request_path").cloned().unwrap_or(Value::Null),
        "request_id": payload_obj.get("request_id").cloned().unwrap_or(Value::Null),
        "metadata": metadata.clone(),
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

    let Some(event_id) = event["event_id"].as_str() else {
        bail!("audit event id is missing");
    };
    op.create_dir(&format!("spaces/{}/audit/event-ids/", safe_space_id))
        .await?;
    if marker_version.is_none() {
        marker_version = create_pending_marker(
            op,
            &safe_space_id,
            event_id,
            &json!({
                "event_id": event_id,
                "action": action,
                "space_uid": space_uid,
                "challenge_id": challenge_id,
                "issuer_principal_id": issuer_principal_id,
                "issuer_account_id": issuer_account_id,
                "issuer_credential_id": issuer_credential_id,
                "subject_principal_id": subject_principal_id,
                "subject_account_id": subject_account_id,
                "actor_principal_id": actor_principal_id,
                "actor_account_id": actor_account_id,
                "credential_id": payload_obj.get("credential_id").cloned().unwrap_or(Value::Null),
                "outcome": normalize_outcome(payload_obj.get("outcome").and_then(Value::as_str)),
                "target_type": payload_obj.get("target_type").cloned().unwrap_or(Value::Null),
                "target_id": payload_obj.get("target_id").cloned().unwrap_or(Value::Null),
                "request_method": payload_obj.get("request_method").cloned().unwrap_or(Value::Null),
                "request_path": payload_obj.get("request_path").cloned().unwrap_or(Value::Null),
                "request_id": payload_obj.get("request_id").cloned().unwrap_or(Value::Null),
                "metadata": metadata
            }),
        )
        .await?;
    }
    if let Some((marker, _)) = read_event_marker(op, &safe_space_id, event_id).await? {
        if marker.get("status").and_then(Value::as_str) == Some("committed") {
            let canonical = marker.get("event").unwrap_or(&marker);
            if audit_event_fingerprint(canonical)? != audit_event_fingerprint(payload)? {
                bail!("audit event id conflicts with canonical payload");
            }
            return Ok(canonical.clone());
        }
        if audit_event_fingerprint(&marker)? != audit_event_fingerprint(payload)? {
            bail!("audit event id conflicts with pending payload");
        }
    }
    write_events(op, &safe_space_id, &events, expected_version.as_deref()).await?;
    if let Err(error) = commit_event_marker(
        op,
        &safe_space_id,
        event_id,
        marker_version.as_deref(),
        &event,
    )
    .await
    {
        if let Some((marker, _)) = read_event_marker(op, &safe_space_id, event_id).await? {
            if marker.get("status").and_then(Value::as_str) == Some("committed") {
                let canonical = marker.get("event").unwrap_or(&marker);
                if audit_event_fingerprint(canonical)? != audit_event_fingerprint(payload)? {
                    bail!("audit event id conflicts with canonical payload");
                }
                return Ok(canonical.clone());
            }
        }
        return Err(error);
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

/// Typed missing-target check for audit enumeration and reconciliation.
///
/// Returns true when the error chain carries a typed "absent" signal:
/// an [`AppError`] with a `FormNotFound`, `EntryNotFound`, or
/// `RevisionNotFound` code, or an OpenDAL `NotFound`. Anything else
/// (corrupt Forms, storage failures, permission errors) is not missing
/// and must propagate fail-closed. String matching on error text is
/// deliberately not used here: it over-matches unrelated failures and
/// hides them as empty evidence.
pub(crate) fn is_missing_audit_target(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        if let Some(app) = cause.downcast_ref::<AppError>() {
            return matches!(
                app.code(),
                ErrorCode::FormNotFound | ErrorCode::EntryNotFound | ErrorCode::RevisionNotFound
            );
        }
        if let Some(opendal) = cause.downcast_ref::<opendal::Error>() {
            return opendal.kind() == opendal::ErrorKind::NotFound;
        }
        false
    })
}

pub async fn list_audit_events(
    op: &Operator,
    space_id: &str,
    options: AuditListOptions,
) -> Result<Value> {
    // Light read by contract: reports committed evidence only (read plus
    // hash-chain verification). It never repairs, reconciles, or appends:
    // crash-missing evidence heals via the server startup hook and the
    // core `open_space` hook, plus the bounded pre-read converge on the
    // server audit route for same-process commit→delivery gaps.
    let safe_space_id = validate_space_id(space_id)?;
    let lock = space_lock(&safe_space_id).await;
    let _guard = lock.lock().await;
    let _local_lock = local_audit_lock(op, &safe_space_id)?;

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

    // Effective range 1..=500 after normalization/clamping: `0` becomes
    // `1` (never a validation error) and anything above 500 caps at 500.
    // Protocol adapters must additionally clamp before `usize` conversion
    // via [`normalize_audit_page`].
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
    use std::{env, process::Command};
    use ugoite_storage::operator_from_uri;

    #[test]
    fn audit_page_normalizes_before_usize_conversion() {
        // Effective range 1..=500: 0 becomes 1, never a validation error.
        assert_eq!(normalize_audit_page(0, 0), (1, 0));
        assert_eq!(normalize_audit_page(1, 7), (1, 7));
        assert_eq!(normalize_audit_page(500, 0), (500, 0));
        assert_eq!(normalize_audit_page(501, 0), (500, 0));
        // The largest protocol integer clamps to 500 without overflowing
        // before the cap applies.
        assert_eq!(normalize_audit_page(u64::MAX, u64::MAX), (500, usize::MAX));
    }

    #[tokio::test]
    async fn audit_list_normalizes_zero_limit_to_one() -> Result<()> {
        let op = operator_from_uri("memory://audit-page-normalization")?;
        let listed = list_audit_events(
            &op,
            "demo",
            AuditListOptions {
                limit: 0,
                ..AuditListOptions::default()
            },
        )
        .await?;
        assert_eq!(listed["limit"], 1);
        Ok(())
    }

    #[tokio::test]
    /// REQ-SEC-008
    async fn req_sec_008_authorization_audit_is_append_only_and_attributed() -> Result<()> {
        let op = operator_from_uri("memory://authorization-audit-contract")?;
        let event_id = uuid::Uuid::now_v7();
        let actor = uuid::Uuid::now_v7();
        let event = json!({
            "event_id": event_id,
            "action": "member.revoked",
            "subject_principal_id": uuid::Uuid::now_v7(),
            "actor_principal_id": actor,
            "credential_id": uuid::Uuid::now_v7(),
            "outcome": "success",
            "metadata": {"role": "viewer"}
        });

        let persisted = append_audit_event(&op, "demo", &event, None).await?;
        let replay = append_audit_event(&op, "demo", &event, None).await?;
        assert_eq!(persisted["event_id"], json!(event_id));
        assert_eq!(replay["actor_principal_id"], json!(actor));
        assert_eq!(
            list_audit_events(&op, "demo", AuditListOptions::default()).await?["total"],
            1
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_req_sec_013_in_process_event_id_deduplication() -> Result<()> {
        let op = operator_from_uri("memory://audit-dedupe")?;
        let event_id = uuid::Uuid::now_v7().to_string();
        let payload = json!({
            "event_id": event_id,
            "action": "recovery.space_binding_replaced",
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
    async fn audit_recovery_batch_appends_and_replays_idempotently() -> Result<()> {
        let op = operator_from_uri("memory://audit-recovery-batch")?;
        let payloads: Vec<Value> = (0..128)
            .map(|index| {
                json!({
                    "event_id": uuid::Uuid::now_v7().to_string(),
                    "action": "entry.updated",
                    "space_uid": "01900000-0000-7000-8000-000000000001",
                    "subject_principal_id": "01900000-0000-7000-8000-000000000001",
                    "actor_principal_id": "01900000-0000-7000-8000-000000000001",
                    "target_type": "entry",
                    "target_id": format!("entry-{index}"),
                    "metadata": {"revision_id": format!("revision-{index}")}
                })
            })
            .collect();

        // Model a process crash after the pending marker write but before
        // the audit-chain write; the recovery batch must resume this marker.
        let pending_event_id = payloads[0]["event_id"].as_str().expect("stable event id");
        op.create_dir("spaces/demo/audit/event-ids/").await?;
        create_pending_marker(&op, "demo", pending_event_id, &payloads[0]).await?;

        let appended = append_audit_events(&op, "demo", &payloads).await?;
        assert_eq!(appended.len(), payloads.len());
        let (resumed_marker, _) = read_event_marker(&op, "demo", pending_event_id)
            .await?
            .expect("resumed batch marker");
        assert_eq!(resumed_marker["status"], "committed");
        let replayed = append_audit_events(&op, "demo", &payloads).await?;
        assert_eq!(replayed, appended);
        let events = read_events(&op, "demo").await?;
        verify_chain(&events)?;
        assert_eq!(events.len(), payloads.len());
        assert_eq!(
            list_audit_events(&op, "demo", AuditListOptions::default()).await?["total"],
            json!(payloads.len())
        );

        let duplicate_event_id = uuid::Uuid::now_v7().to_string();
        let duplicate = json!({
            "event_id": duplicate_event_id,
            "action": "entry.updated",
            "space_uid": "01900000-0000-7000-8000-000000000001",
            "subject_principal_id": "01900000-0000-7000-8000-000000000001",
            "actor_principal_id": "01900000-0000-7000-8000-000000000001",
            "target_type": "entry",
            "target_id": "entry-duplicate",
            "metadata": {"revision_id": "revision-duplicate"}
        });
        let duplicate_result =
            append_audit_events(&op, "demo", &[duplicate.clone(), duplicate]).await?;
        assert_eq!(duplicate_result.len(), 2);
        assert_eq!(duplicate_result[0], duplicate_result[1]);
        let (duplicate_marker, _) = read_event_marker(
            &op,
            "demo",
            &duplicate_result[0]["event_id"]
                .as_str()
                .expect("duplicate event id"),
        )
        .await?
        .expect("committed duplicate marker");
        assert_eq!(duplicate_marker["status"], "committed");
        assert_eq!(
            list_audit_events(&op, "demo", AuditListOptions::default()).await?["total"],
            json!(payloads.len() + 1)
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_req_sec_013_replay_with_same_event_id_but_different_payload_is_rejected(
    ) -> Result<()> {
        let op = operator_from_uri("memory://audit-conflicting-replay")?;
        let event_id = uuid::Uuid::now_v7().to_string();
        let first = json!({
            "event_id": event_id,
            "action": "recovery.space_binding_replaced",
            "space_uid": uuid::Uuid::now_v7().to_string(),
            "challenge_id": uuid::Uuid::now_v7().to_string(),
            "issuer_principal_id": uuid::Uuid::now_v7().to_string(),
            "issuer_account_id": uuid::Uuid::now_v7().to_string(),
            "subject_principal_id": uuid::Uuid::now_v7().to_string(),
            "actor_principal_id": uuid::Uuid::now_v7().to_string(),
            "metadata": {"space_uid": uuid::Uuid::now_v7(), "old_account_id": uuid::Uuid::now_v7(), "new_account_id": uuid::Uuid::now_v7(), "recovery_request_id": event_id}
        });
        append_audit_event(&op, "demo", &first, None).await?;
        let conflicting = json!({
            "event_id": event_id,
            "action": first["action"].clone(),
            "space_uid": uuid::Uuid::now_v7().to_string(),
            "challenge_id": uuid::Uuid::now_v7().to_string(),
            "issuer_principal_id": uuid::Uuid::now_v7().to_string(),
            "issuer_account_id": uuid::Uuid::now_v7().to_string(),
            "subject_principal_id": first["subject_principal_id"].clone(),
            "actor_principal_id": first["actor_principal_id"].clone(),
            "metadata": first["metadata"].clone()
        });
        let error = append_audit_event(&op, "demo", &conflicting, None)
            .await
            .expect_err("conflicting event id must fail closed");
        assert!(error.to_string().contains("conflicts"));
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
            "action": "recovery.space_binding_replaced",
            "subject_principal_id": uuid::Uuid::now_v7().to_string(),
            "actor_principal_id": uuid::Uuid::now_v7().to_string()
        });
        let first = append_audit_event(&op, "demo", &retained, Some(1)).await?;
        append_audit_event(
            &op,
            "demo",
            &json!({
                "event_id": uuid::Uuid::now_v7().to_string(),
                "action": "recovery.space_binding_replaced",
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

    #[tokio::test]
    async fn test_req_sec_013_pending_marker_resumes_to_committed_event() -> Result<()> {
        let op = operator_from_uri("memory://audit-pending-marker")?;
        let event_id = uuid::Uuid::now_v7().to_string();
        let payload = json!({
            "event_id": event_id,
            "action": "recovery.space_binding_replaced",
            "subject_principal_id": uuid::Uuid::now_v7().to_string(),
            "actor_principal_id": uuid::Uuid::now_v7().to_string(),
            "metadata": {"space_uid": uuid::Uuid::now_v7(), "old_account_id": uuid::Uuid::now_v7(), "new_account_id": uuid::Uuid::now_v7(), "recovery_request_id": event_id}
        });
        op.create_dir("spaces/demo/audit/event-ids/").await?;
        create_pending_marker(&op, "demo", &event_id, &payload).await?;
        let resumed = append_audit_event(&op, "demo", &payload, None).await?;
        assert_eq!(resumed["event_id"], event_id);
        let (marker, _) = read_event_marker(&op, "demo", &event_id)
            .await?
            .expect("resumed marker");
        assert_eq!(marker["status"], "committed");
        assert_eq!(
            list_audit_events(&op, "demo", AuditListOptions::default()).await?["total"],
            1
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_req_sec_013_cross_process_conditional_append_deduplicates_event() -> Result<()> {
        let child_root = env::var_os("UGOITE_AUDIT_CHILD_ROOT");
        let event_id = env::var("UGOITE_AUDIT_CHILD_EVENT_ID")
            .unwrap_or_else(|_| uuid::Uuid::now_v7().to_string());
        let payload = json!({
            "event_id": event_id,
            "action": "recovery.space_binding_replaced",
            "subject_principal_id": "01900000-0000-7000-8000-000000000001",
            "actor_principal_id": "01900000-0000-7000-8000-000000000002",
            "metadata": {"credential_generation": 2}
        });
        if let Some(root) = child_root {
            let op = operator_from_uri(&format!("fs://{}", root.to_string_lossy()))?;
            append_audit_event(&op, "demo", &payload, None).await?;
            return Ok(());
        }

        let root = tempfile::tempdir()?;
        let executable = env::current_exe()?;
        let test_filter =
            "audit::tests::test_req_sec_013_cross_process_conditional_append_deduplicates_event";
        let mut children = Vec::new();
        for _ in 0..2 {
            children.push(
                Command::new(&executable)
                    .args(["--exact", test_filter, "--nocapture"])
                    .env("UGOITE_AUDIT_CHILD_ROOT", root.path())
                    .env("UGOITE_AUDIT_CHILD_EVENT_ID", &event_id)
                    .spawn()?,
            );
        }
        for mut child in children {
            let status = child.wait()?;
            assert!(status.success(), "audit child exited with {status}");
        }
        let op = operator_from_uri(&format!("fs://{}", root.path().display()))?;
        let listing = list_audit_events(&op, "demo", AuditListOptions::default()).await?;
        assert_eq!(listing["total"], 1);
        Ok(())
    }

    #[tokio::test]
    async fn test_req_sec_013_delivered_event_preserves_issuer_credential() -> Result<()> {
        let op = operator_from_uri("memory://audit-attribution")?;
        let issuer_credential_id = uuid::Uuid::now_v7();
        let event_id = uuid::Uuid::now_v7();
        let payload = json!({
            "event_id": event_id,
            "action": "recovery.space_binding_replaced",
            "space_uid": uuid::Uuid::now_v7(),
            "challenge_id": uuid::Uuid::now_v7(),
            "issuer_principal_id": uuid::Uuid::now_v7(),
            "issuer_account_id": uuid::Uuid::now_v7(),
            "issuer_credential_id": issuer_credential_id,
            "subject_principal_id": uuid::Uuid::now_v7(),
            "actor_principal_id": null,
            "actor_account_id": null,
            "credential_id": null
        });
        let event = append_audit_event(&op, "demo", &payload, None).await?;
        assert_eq!(event["issuer_credential_id"], json!(issuer_credential_id));
        let listed = list_audit_events(&op, "demo", AuditListOptions::default()).await?;
        assert_eq!(
            listed["items"][0]["issuer_credential_id"],
            json!(issuer_credential_id)
        );
        Ok(())
    }

    #[test]
    fn missing_audit_target_uses_typed_signals_not_message_text() {
        use ugoite_core::error::ErrorCode;
        // Typed domain absent signals converge to missing.
        for code in [
            ErrorCode::FormNotFound,
            ErrorCode::EntryNotFound,
            ErrorCode::RevisionNotFound,
        ] {
            let error: anyhow::Error = AppError::not_found(code, "gone").into();
            assert!(
                is_missing_audit_target(&error),
                "typed {code:?} must count as missing"
            );
            // Context wrapping must not hide the typed signal.
            let wrapped = error.context("audit enumerate");
            assert!(is_missing_audit_target(&wrapped));
        }
        let opendal_missing: anyhow::Error =
            opendal::Error::new(opendal::ErrorKind::NotFound, "no such file").into();
        assert!(is_missing_audit_target(&opendal_missing));
        // Unrelated failures must stay fail-closed, even when their text
        // happens to mention absence.
        let misleading: anyhow::Error =
            anyhow::anyhow!("authorization store not found the expected quorum");
        assert!(!is_missing_audit_target(&misleading));
        let forbidden: anyhow::Error =
            AppError::forbidden("resource is not authorized for this action").into();
        assert!(!is_missing_audit_target(&forbidden));
    }
}
