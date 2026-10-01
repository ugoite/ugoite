//! Commit-coupled audit evidence for content mutations.
//!
//! Issue #2508: ordinary Entry and saved-SQL mutations committed Knowledge
//! without leaving audit evidence, while authorization mutations were wired
//! to [`crate::audit`]. Simply appending after commit is not enough: a crash
//! between the Knowledge commit and the audit append would leave Knowledge
//! without evidence forever.
//!
//! Contract: **if an authoritative mutation committed, its audit intent is
//! recoverable.** This module implements that without creating a second
//! business authority:
//!
//! - The committed revision (Entry history / saved-SQL row) is the only
//!   source of truth. No outbox table, no second Knowledge record.
//! - Every content-mutation event carries a deterministic UUIDv5 `event_id`
//!   derived from `(space_uid, action, target_id, revision_id)`. Generic audit
//!   append stays strict: a different payload under the same event ID fails
//!   closed instead of overwriting evidence.
//! - Reconciliation verifies the canonical audit chain and validates each
//!   persisted event's binding to committed revision identity. It preserves a
//!   matching event, including its historical attribution, and builds a new
//!   event only for missing evidence.
//! - Payloads are allow-listed: actor/action/target/revision/change IDs
//!   only. Entry bodies, SQL text, variables, credentials, and tokens never
//!   enter audit events (enforced by [`crate::audit`] secret rejection plus
//!   construction that never accepts content).
//!
//! Delivery after commit is best-effort (consistent with the existing
//! authorization wiring): a failed delivery never fails an already-committed
//! mutation. The deterministic IDs plus reconcile keep that gap recoverable.

use anyhow::{bail, Context, Result};
use opendal::Operator;
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Instant;
use uuid::Uuid;

use crate::audit;
use crate::service::UgoiteService;

/// Fixed namespace for deterministic UUIDv5 content-mutation audit event IDs.
const MUTATION_AUDIT_EVENT_NAMESPACE: Uuid =
    Uuid::from_u128(0x7567_6f69_7465_5f61_7564_6974_5f76_3141);

/// Actions emitted for Entry content mutations.
pub const ENTRY_CREATED_ACTION: &str = "entry.created";
/// Actions emitted for Entry content mutations.
pub const ENTRY_UPDATED_ACTION: &str = "entry.updated";
/// Actions emitted for Entry content mutations.
pub const ENTRY_DELETED_ACTION: &str = "entry.deleted";
/// Actions emitted for saved-SQL content mutations.
pub const SAVED_SQL_CREATED_ACTION: &str = "saved_sql.created";
/// Actions emitted for saved-SQL content mutations.
pub const SAVED_SQL_UPDATED_ACTION: &str = "saved_sql.updated";
/// Actions emitted for saved-SQL content mutations.
pub const SAVED_SQL_DELETED_ACTION: &str = "saved_sql.deleted";

/// Derives the deterministic audit `event_id` for one committed revision.
///
/// The same committed `(space, action, target, revision)` always maps to the
/// same event ID, so retries, crash recovery, and duplicate deliveries are
/// idempotent by construction.
pub fn mutation_audit_event_id(
    space_uid: &Uuid,
    action: &str,
    target_id: &str,
    revision_id: &str,
) -> Uuid {
    let material = format!("{space_uid}:{action}:{target_id}:{revision_id}");
    Uuid::new_v5(&MUTATION_AUDIT_EVENT_NAMESPACE, material.as_bytes())
}

/// Resolves audit attribution without inventing identities.
///
/// Authorized callers pass their principal IDs (first one attributes the
/// event). Operator-local callers have no principal, so the free-form author
/// string is used as subject. As a last resort the Space UID marks the event
/// as unattributed operator-local activity rather than dropping evidence.
pub fn audit_attribution(
    principal_ids: &[Uuid],
    author_fallback: &str,
    space_uid: &Uuid,
) -> (String, Option<String>) {
    if let Some(principal_id) = principal_ids.first() {
        let id = principal_id.to_string();
        return (id.clone(), Some(id));
    }
    if !author_fallback.trim().is_empty() {
        return (author_fallback.trim().to_string(), None);
    }
    (space_uid.to_string(), None)
}

/// Resolves reconciliation attribution from committed history only.
///
/// The committed revision actor (author or provenance string, as stored at
/// commit time) is the subject. Its textual shape does not establish an
/// authenticated principal identity. Only when history carries no actor does
/// the Space UID mark the event as unattributed.
pub fn committed_actor_attribution(
    committed_actor: Option<&str>,
    space_uid: &Uuid,
) -> (String, Option<String>) {
    let actor = committed_actor
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match actor {
        Some(value) => (value.to_string(), None),
        None => (space_uid.to_string(), None),
    }
}

#[allow(clippy::too_many_arguments)]
fn base_mutation_event(
    space_uid: &Uuid,
    action: &str,
    target_type: &str,
    target_id: &str,
    revision_id: &str,
    change_id: Option<&str>,
    subject: &str,
    actor: Option<&str>,
) -> Value {
    let event_id = mutation_audit_event_id(space_uid, action, target_id, revision_id);
    let mut metadata = serde_json::Map::from_iter([(
        "revision_id".to_string(),
        Value::String(revision_id.to_string()),
    )]);
    if let Some(change_id) = change_id.filter(|value| !value.trim().is_empty()) {
        metadata.insert(
            "change_id".to_string(),
            Value::String(change_id.to_string()),
        );
    }
    let mut event = serde_json::Map::from_iter([
        ("action".to_string(), Value::String(action.to_string())),
        (
            "subject_principal_id".to_string(),
            Value::String(subject.to_string()),
        ),
        (
            "space_uid".to_string(),
            Value::String(space_uid.to_string()),
        ),
        (
            "target_type".to_string(),
            Value::String(target_type.to_string()),
        ),
        (
            "target_id".to_string(),
            Value::String(target_id.to_string()),
        ),
        ("outcome".to_string(), Value::String("success".to_string())),
        ("event_id".to_string(), Value::String(event_id.to_string())),
        ("metadata".to_string(), Value::Object(metadata)),
    ]);
    if let Some(actor) = actor {
        event.insert(
            "actor_principal_id".to_string(),
            Value::String(actor.to_string()),
        );
    }
    Value::Object(event)
}

/// Builds the allow-listed audit event for one committed Entry revision.
///
/// Only identity fields are recorded; Entry body content is never accepted,
/// so it can never leak into the audit chain.
#[allow(clippy::too_many_arguments)]
pub fn entry_mutation_event(
    space_uid: &Uuid,
    action: &str,
    entry_id: &str,
    revision_id: &str,
    change_id: Option<&str>,
    subject: &str,
    actor: Option<&str>,
) -> Value {
    base_mutation_event(
        space_uid,
        action,
        "entry",
        entry_id,
        revision_id,
        change_id,
        subject,
        actor,
    )
}

/// Builds the allow-listed audit event for one committed saved-SQL revision.
///
/// SQL text and variable values are never accepted, so query content and
/// parameter secrets can never leak into the audit chain.
#[allow(clippy::too_many_arguments)]
pub fn saved_sql_mutation_event(
    space_uid: &Uuid,
    action: &str,
    sql_id: &str,
    revision_id: &str,
    subject: &str,
    actor: Option<&str>,
) -> Value {
    base_mutation_event(
        space_uid,
        action,
        "saved_sql",
        sql_id,
        revision_id,
        Some(revision_id),
        subject,
        actor,
    )
}

/// Idempotently delivers one content-mutation audit event.
///
/// Redelivering an event for the same committed revision returns the stored
/// event without duplicating the hash chain.
pub(crate) async fn deliver_mutation_audit_event(
    op: &Operator,
    space_id: &str,
    event: &Value,
) -> Result<Value> {
    audit::append_audit_event(op, space_id, event, None).await
}

#[derive(Debug, Clone)]
struct CommittedMutationAudit {
    event_id: String,
    space_uid: String,
    action: String,
    target_type: String,
    target_id: String,
    revision_id: String,
    change_id: Option<String>,
    missing_event: Value,
}

impl CommittedMutationAudit {
    fn new(
        space_uid: &Uuid,
        action: &str,
        target_type: &str,
        target_id: &str,
        revision_id: &str,
        change_id: Option<&str>,
        missing_event: Value,
    ) -> Self {
        let event_id = mutation_audit_event_id(space_uid, action, target_id, revision_id);
        Self {
            event_id: event_id.to_string(),
            space_uid: space_uid.to_string(),
            action: action.to_string(),
            target_type: target_type.to_string(),
            target_id: target_id.to_string(),
            revision_id: revision_id.to_string(),
            change_id: change_id
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            missing_event,
        }
    }

    fn validate_binding(&self, event: &Value) -> Result<()> {
        let metadata = event.get("metadata").and_then(Value::as_object);
        let event_change_id = metadata.and_then(|metadata| metadata.get("change_id"));
        let change_id_matches = match (event_change_id, self.change_id.as_deref()) {
            (None, None) => true,
            (Some(Value::String(actual)), Some(expected)) => actual == expected,
            _ => false,
        };
        let matches = event.get("event_id").and_then(Value::as_str) == Some(self.event_id.as_str())
            && event.get("space_uid").and_then(Value::as_str) == Some(self.space_uid.as_str())
            && event.get("action").and_then(Value::as_str) == Some(self.action.as_str())
            && event.get("target_type").and_then(Value::as_str) == Some(self.target_type.as_str())
            && event.get("target_id").and_then(Value::as_str) == Some(self.target_id.as_str())
            && metadata
                .and_then(|metadata| metadata.get("revision_id"))
                .and_then(Value::as_str)
                == Some(self.revision_id.as_str())
            && change_id_matches;
        if !matches {
            bail!(
                "audit event {} does not match committed {} revision {}",
                self.event_id,
                self.target_type,
                self.revision_id
            );
        }
        Ok(())
    }
}

/// Preserves verified historical events and appends only missing evidence.
/// The same resolver is used by target reconciliation and the Space sweep.
async fn resolve_committed_mutation_audits(
    op: &Operator,
    space_id: &str,
    space_uid: &str,
    expected: &[CommittedMutationAudit],
    checkpoint: Option<&audit::AuditCheckpointConfig>,
) -> Result<(Vec<Value>, usize)> {
    let event_ids = expected
        .iter()
        .map(|item| item.event_id.clone())
        .collect::<Vec<_>>();
    let canonical = audit::verified_events_by_id(op, space_id, &event_ids).await?;
    let mut resolved = BTreeMap::new();
    let mut missing = Vec::new();
    for item in expected {
        if let Some(event) = canonical.get(&item.event_id) {
            item.validate_binding(event)?;
            resolved.insert(item.event_id.clone(), event.clone());
        } else {
            item.validate_binding(&item.missing_event)?;
            missing.push(item);
        }
    }

    let missing_count = missing.len();
    if !missing.is_empty() {
        let missing_ids = missing
            .iter()
            .map(|item| item.event_id.clone())
            .collect::<Vec<_>>();
        let payloads = missing
            .iter()
            .map(|item| item.missing_event.clone())
            .collect::<Vec<_>>();
        let append_result = audit::append_reconciliation_audit_events(
            op, space_id, space_uid, &payloads, checkpoint,
        )
        .await;
        // Read the events back from the canonical chain after append. The
        // append API may consult event-id markers for retry safety; markers
        // do not prove that an event is present in `events.jsonl`.
        let canonical_after = audit::verified_events_by_id(op, space_id, &missing_ids).await?;
        let all_committed = missing
            .iter()
            .all(|item| canonical_after.contains_key(&item.event_id));
        if let Err(error) = append_result {
            if !all_committed {
                return Err(error);
            }
        }
        for item in missing {
            let Some(event) = canonical_after.get(&item.event_id) else {
                bail!(
                    "audit event {} was not committed to the canonical chain",
                    item.event_id
                );
            };
            item.validate_binding(event)?;
            resolved.insert(item.event_id.clone(), event.clone());
        }
    }

    let ordered = expected
        .iter()
        .map(|item| {
            resolved
                .get(&item.event_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("audit event {} was not resolved", item.event_id))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((ordered, missing_count))
}

fn entry_revision_audit_expectation(
    space_uid: &Uuid,
    entry_id: &str,
    action: &str,
    revision_id: &str,
    change_id: Option<&str>,
    committed_actor: Option<&str>,
) -> CommittedMutationAudit {
    let (subject, actor) = committed_actor_attribution(committed_actor, space_uid);
    let event = entry_mutation_event(
        space_uid,
        action,
        entry_id,
        revision_id,
        change_id,
        &subject,
        actor.as_deref(),
    );
    CommittedMutationAudit::new(
        space_uid,
        action,
        "entry",
        entry_id,
        revision_id,
        change_id,
        event,
    )
}

fn saved_sql_revision_audit_expectation(
    space_uid: &Uuid,
    sql_id: &str,
    action: &str,
    revision_id: &str,
    committed_actor: Option<&str>,
) -> CommittedMutationAudit {
    let (subject, actor) = committed_actor_attribution(committed_actor, space_uid);
    let event = saved_sql_mutation_event(
        space_uid,
        action,
        sql_id,
        revision_id,
        &subject,
        actor.as_deref(),
    );
    CommittedMutationAudit::new(
        space_uid,
        action,
        "saved_sql",
        sql_id,
        revision_id,
        Some(revision_id),
        event,
    )
}

fn latest_entry_revision(
    history: &Value,
) -> Option<(String, Option<String>, String, String, String)> {
    let revisions = history.get("revisions")?.as_array()?;
    let last = revisions.last()?.as_object()?;
    let revision_id = last.get("revision_id")?.as_str()?.to_string();
    let change_id = last
        .get("change_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let operation = last.get("operation")?.as_str()?.to_string();
    let author = last
        .get("author")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let updated_by = last
        .get("updated_by")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Some((revision_id, change_id, operation, author, updated_by))
}

impl UgoiteService {
    #[allow(clippy::too_many_arguments)]
    async fn deliver_entry_revision_audit(
        &self,
        space_id: &str,
        action: &str,
        entry_id: &str,
        revision_id: &str,
        change_id: Option<&str>,
        author: &str,
        updated_by: &str,
    ) -> Result<()> {
        let space_uid = self.space_uid(space_id).await?;
        // Attribution must converge with batch reconcile by construction:
        // the committed row actor is the authority (see
        // `committed_actor_attribution`), never the live caller identity.
        // Caller principals that differ from the stored row used to produce
        // same-ID events with different fingerprints, failing reopen with
        // "audit event id conflicts with canonical payload".
        let committed_actor = if updated_by.trim().is_empty() {
            author
        } else {
            updated_by
        };
        let (subject, actor) = committed_actor_attribution(Some(committed_actor), &space_uid);
        let event = entry_mutation_event(
            &space_uid,
            action,
            entry_id,
            revision_id,
            change_id,
            &subject,
            actor.as_deref(),
        );
        deliver_mutation_audit_event(self.operator(), space_id, &event).await?;
        Ok(())
    }

    /// Best-effort audit delivery for a committed Entry revision.
    ///
    /// The revision identity is always re-read from Entry history (the
    /// committed truth, tombstones included) rather than assumed: the entry
    /// layer mints change IDs the caller cannot predict. Failures never fail
    /// the already-committed mutation; the deterministic event ID keeps the
    /// intent recoverable via [`Self::reconcile_entry_audit`].
    pub(crate) async fn record_committed_entry_revision(
        &self,
        space_id: &str,
        entry_id: &str,
        action: &str,
    ) {
        let delivered = async {
            let history = crate::entry::get_entry_history(
                self.operator(),
                &self.workspace_path(space_id),
                entry_id,
            )
            .await?;
            let Some((revision_id, change_id, _, author, updated_by)) =
                latest_entry_revision(&history)
            else {
                return Ok::<(), anyhow::Error>(());
            };
            self.deliver_entry_revision_audit(
                space_id,
                action,
                entry_id,
                &revision_id,
                change_id.as_deref(),
                &author,
                &updated_by,
            )
            .await?;
            Ok::<(), anyhow::Error>(())
        }
        .await;
        let _ = delivered;
    }

    /// Best-effort audit delivery for a committed saved-SQL revision.
    ///
    /// Failures never fail the already-committed mutation; the deterministic
    /// event ID keeps the intent recoverable via
    /// [`Self::reconcile_saved_sql_audit`].
    pub(crate) async fn record_saved_sql_audit(
        &self,
        space_id: &str,
        action: &str,
        sql_id: &str,
        revision_id: &str,
        committed_author: &str,
    ) {
        let delivered = async {
            let space_uid = self.space_uid(space_id).await?;
            let (subject, actor) = committed_actor_attribution(Some(committed_author), &space_uid);
            let event = saved_sql_mutation_event(
                &space_uid,
                action,
                sql_id,
                revision_id,
                &subject,
                actor.as_deref(),
            );
            deliver_mutation_audit_event(self.operator(), space_id, &event).await?;
            Ok::<(), anyhow::Error>(())
        }
        .await;
        let _ = delivered;
    }

    /// Best-effort audit delivery for a committed Entry tombstone.
    ///
    /// Deletes share the revision re-read path: the tombstone revision only
    /// exists after commit. Failures never fail the already-committed
    /// delete; reconcile closes the gap.
    pub(crate) async fn record_committed_entry_delete(&self, space_id: &str, entry_id: &str) {
        self.record_committed_entry_revision(space_id, entry_id, ENTRY_DELETED_ACTION)
            .await;
    }

    /// Resolves audit evidence for `entry_id` from committed truth (Entry
    /// history, tombstones included), preserving verified existing events
    /// and delivering only missing evidence.
    ///
    /// Every committed revision gets its own deterministic event: the first
    /// revision is `entry.created`, a delete-operation revision is
    /// `entry.deleted`, all others are `entry.updated`. Reconciling only the
    /// latest revision would permanently drop evidence for earlier mutations
    /// whose delivery failed, so the whole chain converges here.
    ///
    /// Attribution comes only from committed revision provenance. Missing
    /// actor metadata falls back to the Space UID. The caller attribution
    /// arguments are retained for source compatibility but do not project
    /// identity into content history. Returns the last canonical event, or
    /// `None` when the Entry has no committed
    /// revisions, including when it never existed (consistent with saved-SQL
    /// reconcile). Closing a commit→delivery crash gap is a second call away
    /// however long after the crash the Space is reopened.
    pub async fn reconcile_entry_audit(
        &self,
        space_id: &str,
        entry_id: &str,
        _principal_ids: &[Uuid],
        _author_fallback: &str,
    ) -> Result<Option<Value>> {
        let history = match crate::entry::get_entry_history(
            self.operator(),
            &self.workspace_path(space_id),
            entry_id,
        )
        .await
        {
            Ok(history) => history,
            Err(error)
                if error
                    .downcast_ref::<ugoite_core::error::AppError>()
                    .is_some_and(|app| {
                        app.code() == ugoite_core::error::ErrorCode::EntryNotFound
                    }) =>
            {
                return Ok(None);
            }
            Err(error) => {
                return Err(error).with_context(|| format!("reconcile audit for Entry {entry_id}"));
            }
        };
        let revisions = history
            .get("revisions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if revisions.is_empty() {
            return Ok(None);
        }
        let space_uid = self.space_uid(space_id).await?;
        // History order is timestamp-based, so clock skew could mislabel the
        // create revision; entry_version is the authoritative creation order
        // when present, with position as the fallback.
        let created_version = revisions
            .iter()
            .filter_map(|revision| revision.get("entry_version").and_then(Value::as_u64))
            .min();
        let mut expected = Vec::with_capacity(revisions.len());
        for (index, revision) in revisions.iter().enumerate() {
            let Some(revision_id) = revision
                .get("revision_id")
                .and_then(Value::as_str)
                .map(str::to_string)
            else {
                continue;
            };
            let change_id = revision
                .get("change_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            let operation = revision
                .get("operation")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let is_first = match (
                revision.get("entry_version").and_then(Value::as_u64),
                created_version,
            ) {
                (Some(version), Some(created)) => version == created,
                _ => index == 0,
            };
            let action = if operation == "delete" {
                ENTRY_DELETED_ACTION
            } else if is_first {
                ENTRY_CREATED_ACTION
            } else {
                ENTRY_UPDATED_ACTION
            };
            let committed_actor = revision.get("actor").and_then(Value::as_str);
            expected.push(entry_revision_audit_expectation(
                &space_uid,
                entry_id,
                action,
                &revision_id,
                change_id.as_deref(),
                committed_actor,
            ));
        }
        let space_uid_text = space_uid.to_string();
        let (resolved, _) = resolve_committed_mutation_audits(
            self.operator(),
            space_id,
            &space_uid_text,
            &expected,
            None,
        )
        .await?;
        Ok(resolved.into_iter().last())
    }

    fn entry_revision_audit_events(
        entry_id: &str,
        mut revisions: Vec<crate::entry::RevisionRow>,
        space_uid: Uuid,
    ) -> Vec<CommittedMutationAudit> {
        revisions.sort_by(|left, right| {
            left.timestamp
                .partial_cmp(&right.timestamp)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.revision_id.cmp(&right.revision_id))
        });
        let created_version = revisions
            .iter()
            .map(|revision| revision.entry_version)
            .min();
        let mut events = Vec::with_capacity(revisions.len());
        for (index, revision) in revisions.iter().enumerate() {
            let is_first = match (Some(revision.entry_version), created_version) {
                (Some(version), Some(created)) => version == created,
                _ => index == 0,
            };
            let action = if revision.operation == "delete" {
                ENTRY_DELETED_ACTION
            } else if is_first {
                ENTRY_CREATED_ACTION
            } else {
                ENTRY_UPDATED_ACTION
            };
            let committed_actor = if revision.updated_by.trim().is_empty() {
                revision.author.as_str()
            } else {
                revision.updated_by.as_str()
            };
            events.push(entry_revision_audit_expectation(
                &space_uid,
                entry_id,
                action,
                &revision.revision_id,
                Some(&revision.change_id),
                Some(committed_actor),
            ));
        }
        events
    }

    fn saved_sql_revision_audit_events(
        sql_id: &str,
        mut revisions: Vec<crate::entry::RevisionRow>,
        space_uid: Uuid,
    ) -> Vec<CommittedMutationAudit> {
        revisions.sort_by(|left, right| {
            (left.entry_version, left.timestamp)
                .partial_cmp(&(right.entry_version, right.timestamp))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        revisions
            .iter()
            .map(|revision| {
                let action = if revision.operation == "delete" {
                    SAVED_SQL_DELETED_ACTION
                } else if revision.parent_revision_id.is_none() {
                    SAVED_SQL_CREATED_ACTION
                } else {
                    SAVED_SQL_UPDATED_ACTION
                };
                let committed_actor = if revision.updated_by.trim().is_empty() {
                    revision.author.as_str()
                } else {
                    revision.updated_by.as_str()
                };
                saved_sql_revision_audit_expectation(
                    &space_uid,
                    sql_id,
                    action,
                    &revision.revision_id,
                    Some(committed_actor),
                )
            })
            .collect()
    }

    /// Resolves audit evidence for `sql_id` from committed truth (saved-SQL
    /// revision rows, including tombstones), preserving verified existing
    /// events and delivering only missing evidence.
    ///
    /// Like Entries, every committed revision gets its own deterministic
    /// event (first revision without a parent is `saved_sql.created`, a
    /// deleted row is `saved_sql.deleted`, all others are
    /// `saved_sql.updated`), so an earlier update whose delivery failed is
    /// not dropped when a later revision reconciles.
    ///
    /// Attribution comes only from committed revision provenance. Missing
    /// actor metadata falls back to the Space UID; caller identity is never
    /// projected into content history. The caller attribution arguments are
    /// retained for source compatibility but do not affect the projection.
    pub async fn reconcile_saved_sql_audit(
        &self,
        space_id: &str,
        sql_id: &str,
        _principal_ids: &[Uuid],
        _author_fallback: &str,
    ) -> Result<Option<Value>> {
        let mut revisions: Vec<crate::entry::RevisionRow> =
            crate::entry::form_revision_rows_for_audit(
                self.operator(),
                &self.workspace_path(space_id),
                crate::saved_sql::SQL_FORM_NAME_FOR_AUDIT,
            )
            .await?
            .into_iter()
            .filter(|row| row.entry_id == sql_id)
            .collect();
        if revisions.is_empty() {
            return Ok(None);
        }
        revisions.sort_by(|a, b| {
            (a.entry_version, a.timestamp)
                .partial_cmp(&(b.entry_version, b.timestamp))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let space_uid = self.space_uid(space_id).await?;
        let mut expected = Vec::with_capacity(revisions.len());
        for revision in &revisions {
            let action = if revision.operation == "delete" {
                SAVED_SQL_DELETED_ACTION
            } else if revision.parent_revision_id.is_none() {
                SAVED_SQL_CREATED_ACTION
            } else {
                SAVED_SQL_UPDATED_ACTION
            };
            let committed_actor = if revision.updated_by.trim().is_empty() {
                revision.author.clone()
            } else {
                revision.updated_by.clone()
            };
            expected.push(saved_sql_revision_audit_expectation(
                &space_uid,
                sql_id,
                action,
                &revision.revision_id,
                Some(&committed_actor),
            ));
        }
        let space_uid_text = space_uid.to_string();
        let (resolved, _) = resolve_committed_mutation_audits(
            self.operator(),
            space_id,
            &space_uid_text,
            &expected,
            None,
        )
        .await?;
        Ok(resolved.into_iter().last())
    }

    /// Converges audit evidence for every committed Entry and saved-SQL
    /// target in one Space from committed history.
    ///
    /// Crash windows and delivery failures are per-mutation: a sweep must not
    /// stop at the latest revision of one target. Every committed Entry
    /// revision (tombstones included) and every committed saved-SQL row is
    /// converted to the same deterministic identities as the per-target
    /// paths. Verified existing events are preserved and only missing events
    /// are delivered as one batch. Attribution is used only for newly built
    /// evidence; existing event attribution remains historical. Failures
    /// propagate instead of hiding as success; existing events are never rewritten and
    /// Change/revision IDs never change. Returns the number of targets
    /// converged.
    pub async fn reconcile_space_audit(&self, space_id: &str) -> Result<usize> {
        let reconcile_started = Instant::now();
        let workspace = self.workspace_path(space_id);
        // `space_uid` verifies uniqueness against every discoverable Space.
        // Validate once per sweep, then reuse the immutable identity for each
        // target instead of re-listing every Space for every Entry.
        let space_uid = self.space_uid(space_id).await?;
        // Enumerate from revision rows (never the Current view) so
        // tombstoned entries are included: their delete evidence may be the
        // very gap being closed. Enumeration is read-only and unbounded by
        // row caps; a corrupt Form fails the sweep instead of being skipped.
        let mut entry_revisions = BTreeMap::new();
        let form_names = match crate::entry::list_form_names(self.operator(), &workspace).await {
            Ok(names) => names,
            // No committed Forms yet means no committed revisions to
            // converge. Typed missing-target only; corrupt catalogs and
            // storage failures propagate fail-closed.
            Err(error) if crate::audit::is_missing_audit_target(&error) => Vec::new(),
            Err(error) => return Err(error),
        };
        for form_name in form_names {
            // Saved-SQL rows are Entry storage rows too, but their evidence
            // lives under saved_sql.* actions: the SQL loop below owns them.
            // Emitting entry.* events for SQL rows would double-record one
            // committed revision under two action families.
            if form_name.eq_ignore_ascii_case(crate::saved_sql::SQL_FORM_NAME_FOR_AUDIT) {
                continue;
            }
            let revisions =
                crate::entry::form_revision_rows_for_audit(self.operator(), &workspace, &form_name)
                    .await
                    .with_context(|| format!("enumerate audit targets for Form {form_name}"))?;
            let mut form_entry_revisions = BTreeMap::new();
            for revision in revisions {
                form_entry_revisions
                    .entry(revision.entry_id.clone())
                    .or_insert_with(Vec::new)
                    .push(revision);
            }
            for (entry_id, revisions) in form_entry_revisions {
                // Match per-Entry history lookup, which resolves a duplicate
                // ID from the first Form in list order rather than combining
                // revisions from distinct Forms.
                entry_revisions.entry(entry_id).or_insert(revisions);
            }
        }
        let sql_ids = crate::saved_sql::list_sql_ids_for_audit(self.operator(), &workspace).await?;
        let entry_target_count = entry_revisions.len();
        let sql_target_count = sql_ids.len();
        let converged = entry_target_count + sql_target_count;
        let mut expected_audits = Vec::new();
        for (entry_id, revisions) in entry_revisions {
            expected_audits.extend(Self::entry_revision_audit_events(
                &entry_id, revisions, space_uid,
            ));
        }
        let sql_rows = if sql_ids.is_empty() {
            Vec::new()
        } else {
            crate::entry::form_revision_rows_for_audit(
                self.operator(),
                &workspace,
                crate::saved_sql::SQL_FORM_NAME_FOR_AUDIT,
            )
            .await?
        };
        let mut sql_revisions = BTreeMap::<String, Vec<crate::entry::RevisionRow>>::new();
        for revision in sql_rows {
            sql_revisions
                .entry(revision.entry_id.clone())
                .or_default()
                .push(revision);
        }
        for sql_id in sql_ids {
            if let Some(revisions) = sql_revisions.remove(&sql_id) {
                expected_audits.extend(Self::saved_sql_revision_audit_events(
                    &sql_id, revisions, space_uid,
                ));
            }
        }
        crate::audit::emit_startup_audit_measurement(
            "audit_target_enumeration",
            reconcile_started.elapsed(),
            serde_json::json!({
                "entries": entry_target_count,
                "saved_sql": sql_target_count
            }),
        );
        let target_count = converged;
        let append_started = Instant::now();
        let space_uid_text = space_uid.to_string();
        let (_, appended_events) = resolve_committed_mutation_audits(
            self.operator(),
            space_id,
            &space_uid_text,
            &expected_audits,
            self.audit_checkpoint_config(),
        )
        .await
        .with_context(|| format!("reconcile audit events for Space {space_id}"))?;
        crate::audit::emit_startup_audit_measurement(
            "audit_reconcile_append",
            append_started.elapsed(),
            serde_json::json!({
                "targets": target_count,
                "events": appended_events
            }),
        );
        Ok(converged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn test_uid() -> Uuid {
        Uuid::parse_str("0198a1b2-c3d4-7e5f-8901-23456789abcd").expect("fixed test UIDv7")
    }

    #[test]
    fn event_ids_are_deterministic_per_committed_revision() {
        let space_uid = test_uid();
        let first = mutation_audit_event_id(&space_uid, ENTRY_CREATED_ACTION, "entry-1", "rev-1");
        let retry = mutation_audit_event_id(&space_uid, ENTRY_CREATED_ACTION, "entry-1", "rev-1");
        assert_eq!(first, retry);
        let next_revision =
            mutation_audit_event_id(&space_uid, ENTRY_UPDATED_ACTION, "entry-1", "rev-2");
        assert_ne!(first, next_revision);
        let other_entry =
            mutation_audit_event_id(&space_uid, ENTRY_CREATED_ACTION, "entry-2", "rev-1");
        assert_ne!(first, other_entry);
    }

    #[test]
    fn entry_events_carry_no_content() {
        let space_uid = test_uid();
        let event = entry_mutation_event(
            &space_uid,
            ENTRY_CREATED_ACTION,
            "entry-1",
            "rev-1",
            Some("change-1"),
            "author",
            None,
        );
        let raw = serde_json::to_string(&event).expect("event serializes");
        for forbidden in ["markdown", "content", "body", "secret", "token"] {
            assert!(
                !raw.contains(forbidden),
                "event must not contain {forbidden}"
            );
        }
        assert_eq!(event["action"], json!(ENTRY_CREATED_ACTION));
        assert_eq!(event["target_type"], json!("entry"));
        assert_eq!(event["metadata"]["revision_id"], json!("rev-1"));
        assert_eq!(event["metadata"]["change_id"], json!("change-1"));
    }

    #[test]
    fn saved_sql_events_carry_no_query_text() {
        let space_uid = test_uid();
        let event = saved_sql_mutation_event(
            &space_uid,
            SAVED_SQL_CREATED_ACTION,
            "sql-1",
            "rev-1",
            "author",
            Some("author"),
        );
        let raw = serde_json::to_string(&event).expect("event serializes");
        for forbidden in ["SELECT", "sql_text", "variables", "secret", "token"] {
            assert!(
                !raw.contains(forbidden),
                "event must not contain {forbidden}"
            );
        }
        assert_eq!(event["target_type"], json!("saved_sql"));
    }

    #[test]
    fn attribution_prefers_principals_then_author_then_space() {
        let space_uid = test_uid();
        let principal = Uuid::now_v7();
        let (subject, actor) = audit_attribution(&[principal], "author", &space_uid);
        assert_eq!(subject, principal.to_string());
        assert_eq!(actor, Some(principal.to_string()));
        let (subject, actor) = audit_attribution(&[], "author", &space_uid);
        assert_eq!(subject, "author");
        assert_eq!(actor, None);
        let (subject, _) = audit_attribution(&[], "   ", &space_uid);
        assert_eq!(subject, space_uid.to_string());
    }

    fn entry_fields(body: &str) -> BTreeMap<String, Value> {
        BTreeMap::from([(String::from("Body"), Value::String(body.to_string()))])
    }

    async fn audit_test_space(slug_suffix: &str) -> anyhow::Result<(UgoiteService, String)> {
        let service = UgoiteService::new(format!("memory://mutation-audit-{slug_suffix}"))?;
        let space_uid = service
            .create_operator_space(&format!("audit-space-{slug_suffix}"))
            .await?;
        Ok((service, space_uid.to_string()))
    }

    async fn audit_total(service: &UgoiteService, space_id: &str) -> anyhow::Result<usize> {
        let listed = crate::audit::list_audit_events(
            service.operator(),
            space_id,
            crate::audit::AuditListOptions::default(),
        )
        .await?;
        Ok(listed.get("total").and_then(Value::as_u64).unwrap_or(0) as usize)
    }

    async fn audit_bytes(service: &UgoiteService, space_id: &str) -> anyhow::Result<Vec<u8>> {
        Ok(service
            .operator()
            .read(&format!("spaces/{space_id}/audit/events.jsonl"))
            .await?
            .to_vec())
    }

    async fn write_untracked_entry(
        service: &UgoiteService,
        space_id: &str,
        body: &str,
    ) -> anyhow::Result<()> {
        let integrity =
            crate::integrity::RealIntegrityProvider::from_space(service.operator(), space_id)
                .await?;
        crate::entry::create_structured_entry_with_scopes_and_change(
            service.operator(),
            &service.workspace_path(space_id),
            "entry-1",
            "Entry".into(),
            Vec::new(),
            entry_fields(body),
            BTreeMap::new(),
            "author",
            &integrity,
            None,
            None,
        )
        .await?;
        Ok(())
    }

    async fn write_authorized_entry(
        service: &UgoiteService,
        space_id: &str,
        principal: Uuid,
    ) -> anyhow::Result<()> {
        service
            .create_structured_entry_authorized_for_principals(
                space_id,
                "entry-1",
                "Entry".into(),
                Vec::new(),
                entry_fields("content"),
                BTreeMap::new(),
                &principal.to_string(),
                &[principal],
            )
            .await?;
        Ok(())
    }

    #[tokio::test]
    async fn committed_entry_mutations_leave_evidence() -> anyhow::Result<()> {
        let (service, space_id) = audit_test_space("evidence").await?;
        let (created, _) = service
            .create_structured_entry_with_receipt(
                &space_id,
                "entry-1",
                "Entry".into(),
                Vec::new(),
                entry_fields("content"),
                BTreeMap::new(),
                "author",
            )
            .await?;
        let revision_id = created
            .get("revision_id")
            .and_then(Value::as_str)
            .expect("revision id")
            .to_string();
        let updated = service
            .update_structured_entry(
                &space_id,
                "entry-1",
                Some("Entry".into()),
                entry_fields("updated"),
                BTreeMap::new(),
                None,
                "author",
            )
            .await?;
        let updated_revision_id = updated
            .get("revision_id")
            .and_then(Value::as_str)
            .expect("revision id")
            .to_string();
        assert_ne!(revision_id, updated_revision_id);
        service.delete_entry(&space_id, "entry-1", "author").await?;

        // list_audit_events verifies the hash chain on every read.
        let listed = crate::audit::list_audit_events(
            service.operator(),
            &space_id,
            crate::audit::AuditListOptions::default(),
        )
        .await?;
        assert_eq!(listed.get("total").and_then(Value::as_u64), Some(3));
        let actions: Vec<&str> = listed
            .get("items")
            .and_then(Value::as_array)
            .expect("items")
            .iter()
            .filter_map(|item| item.get("action").and_then(Value::as_str))
            .collect();
        for expected in [
            ENTRY_CREATED_ACTION,
            ENTRY_UPDATED_ACTION,
            ENTRY_DELETED_ACTION,
        ] {
            assert!(
                actions.contains(&expected),
                "missing {expected}: {actions:?}"
            );
        }
        // Allow-list: no Entry body anywhere in the evidence.
        let raw = serde_json::to_string(&listed).expect("serializes");
        assert!(
            !raw.contains("## Body"),
            "audit must not contain Entry content"
        );
        Ok(())
    }

    #[tokio::test]
    async fn redelivering_the_same_revision_never_duplicates() -> anyhow::Result<()> {
        let (service, space_id) = audit_test_space("dedupe").await?;
        let (created, _) = service
            .create_structured_entry_with_receipt(
                &space_id,
                "entry-1",
                "Entry".into(),
                Vec::new(),
                entry_fields("content"),
                BTreeMap::new(),
                "author",
            )
            .await?;
        let revision_id = created
            .get("revision_id")
            .and_then(Value::as_str)
            .expect("revision id")
            .to_string();
        assert_eq!(audit_total(&service, &space_id).await?, 1);
        // Rebuild the event from committed truth exactly as delivery and
        // reconcile do: converges, never duplicates. A differing payload
        // under the same event_id would fail closed.
        let history = crate::entry::get_entry_history(
            service.operator(),
            &service.workspace_path(&space_id),
            "entry-1",
        )
        .await?;
        let last = history
            .get("revisions")
            .and_then(Value::as_array)
            .and_then(|revisions| revisions.last())
            .expect("committed revision");
        let committed_change_id = last
            .get("change_id")
            .and_then(Value::as_str)
            .map(str::to_string);
        let space_uid = service.space_uid(&space_id).await?;
        let event = entry_mutation_event(
            &space_uid,
            ENTRY_CREATED_ACTION,
            "entry-1",
            &revision_id,
            committed_change_id.as_deref(),
            "author",
            None,
        );
        deliver_mutation_audit_event(service.operator(), &space_id, &event).await?;
        deliver_mutation_audit_event(service.operator(), &space_id, &event).await?;
        assert_eq!(audit_total(&service, &space_id).await?, 1);
        Ok(())
    }

    #[tokio::test]
    async fn reconcile_after_successful_delivery_stays_singular() -> anyhow::Result<()> {
        // Guards the live-delivery/reconcile payload contract: both derive
        // from committed truth, so reconcile after success converges instead
        // of failing closed on a payload conflict.
        let (service, space_id) = audit_test_space("reconcile-ok").await?;
        let _ = service
            .create_structured_entry_with_receipt(
                &space_id,
                "entry-1",
                "Entry".into(),
                Vec::new(),
                entry_fields("content"),
                BTreeMap::new(),
                "author",
            )
            .await?;
        assert_eq!(audit_total(&service, &space_id).await?, 1);
        let delivered = service
            .reconcile_entry_audit(&space_id, "entry-1", &[], "author")
            .await?
            .expect("reconcile converges after success");
        assert_eq!(delivered["action"], json!(ENTRY_CREATED_ACTION));
        assert_eq!(audit_total(&service, &space_id).await?, 1);
        // Unknown targets reconcile to no evidence, not an error, and the
        // read-only check must not bootstrap storage state as a side effect.
        assert!(service
            .reconcile_entry_audit(&space_id, "missing-entry", &[], "author")
            .await?
            .is_none());
        assert!(service
            .reconcile_saved_sql_audit(&space_id, "missing-sql", &[], "author")
            .await?
            .is_none());
        assert!(
            crate::form::read_form_definition(
                service.operator(),
                &service.workspace_path(&space_id),
                "SQL"
            )
            .await
            .is_err(),
            "reconciling a missing target must not create the SQL form"
        );
        Ok(())
    }

    #[tokio::test]
    async fn crash_gap_is_closed_by_reconcile_after_reopen() -> anyhow::Result<()> {
        let (service, space_id) = audit_test_space("crash").await?;
        // Simulate commit-without-delivery by writing through the low-level
        // entry layer, bypassing the audited service method.
        write_untracked_entry(&service, &space_id, "content").await?;
        assert_eq!(audit_total(&service, &space_id).await?, 0);

        // Reopen the Space (fresh service over the same storage) and
        // reconcile: the committed revision becomes audit evidence.
        let root_uri = service.root_uri().to_string();
        let service2 = UgoiteService::from_operator(service.operator().clone(), root_uri);
        let delivered = service2
            .reconcile_entry_audit(&space_id, "entry-1", &[], "author")
            .await?
            .expect("committed revision reconciles");
        assert_eq!(delivered["action"], json!(ENTRY_CREATED_ACTION));
        assert_eq!(audit_total(&service2, &space_id).await?, 1);
        // Reconcile is idempotent too.
        service2
            .reconcile_entry_audit(&space_id, "entry-1", &[], "author")
            .await?;
        assert_eq!(audit_total(&service2, &space_id).await?, 1);
        let events = crate::audit::list_audit_events(
            service2.operator(),
            &space_id,
            crate::audit::AuditListOptions::default(),
        )
        .await?;
        assert!(events["items"][0]["actor_principal_id"].is_null());
        let bytes_after_recovery = audit_bytes(&service2, &space_id).await?;
        let service3 = UgoiteService::from_operator(
            service2.operator().clone(),
            service2.root_uri().to_string(),
        );
        service3.open_space(&space_id).await?;
        assert_eq!(
            audit_bytes(&service3, &space_id).await?,
            bytes_after_recovery
        );
        Ok(())
    }

    #[tokio::test]
    async fn orphaned_event_marker_does_not_block_chain_recovery() -> anyhow::Result<()> {
        let (service, space_id) = audit_test_space("orphan-marker").await?;
        let uuid_author = Uuid::now_v7().to_string();
        service
            .create_structured_entry_with_receipt(
                &space_id,
                "entry-1",
                "Entry".into(),
                Vec::new(),
                entry_fields("content"),
                BTreeMap::new(),
                &uuid_author,
            )
            .await?;
        let history = crate::entry::get_entry_history(
            service.operator(),
            &service.workspace_path(&space_id),
            "entry-1",
        )
        .await?;
        let revision = &history["revisions"][0];
        let space_uid = service.space_uid(&space_id).await?;
        let expected = entry_revision_audit_expectation(
            &space_uid,
            "entry-1",
            ENTRY_CREATED_ACTION,
            revision["revision_id"].as_str().expect("revision id"),
            revision["change_id"].as_str(),
            Some(&uuid_author),
        );
        let marker_path = format!(
            "spaces/{space_id}/audit/event-ids/{}.json",
            expected.event_id
        );
        let mut orphan_marker: Value =
            serde_json::from_slice(&service.operator().read(&marker_path).await?.to_vec())?;
        orphan_marker["event"]["actor_principal_id"] = json!(uuid_author);
        service
            .operator()
            .write(&marker_path, serde_json::to_vec(&orphan_marker)?)
            .await?;

        let events_path = format!("spaces/{space_id}/audit/events.jsonl");
        service.operator().delete(&events_path).await?;
        assert!(
            crate::audit::verified_events_by_id(
                service.operator(),
                &space_id,
                std::slice::from_ref(&expected.event_id),
            )
            .await?
            .is_empty(),
            "an event-id marker is not canonical chain evidence"
        );

        let generic_append = crate::audit::append_audit_events(
            service.operator(),
            &space_id,
            std::slice::from_ref(&expected.missing_event),
        )
        .await;
        assert!(generic_append
            .expect_err("generic same-ID marker conflict stays strict")
            .to_string()
            .contains("audit event id conflicts with canonical payload"));
        assert!(
            !service.operator().exists(&events_path).await?,
            "generic conflict must not create a chain"
        );

        let reopened = UgoiteService::from_operator(
            service.operator().clone(),
            service.root_uri().to_string(),
        );
        reopened.open_space(&space_id).await?;
        assert_eq!(audit_total(&reopened, &space_id).await?, 1);
        let events = crate::audit::list_audit_events(
            reopened.operator(),
            &space_id,
            crate::audit::AuditListOptions::default(),
        )
        .await?;
        assert_eq!(
            events["items"][0]["subject_principal_id"],
            json!(uuid_author)
        );
        assert!(events["items"][0]["actor_principal_id"].is_null());
        let recovered_bytes = audit_bytes(&reopened, &space_id).await?;
        let repaired_marker: Value =
            serde_json::from_slice(&reopened.operator().read(&marker_path).await?.to_vec())?;
        assert!(repaired_marker["event"]["actor_principal_id"].is_null());

        reopened.open_space(&space_id).await?;
        assert_eq!(audit_bytes(&reopened, &space_id).await?, recovered_bytes);
        assert_eq!(audit_total(&reopened, &space_id).await?, 1);
        Ok(())
    }

    #[tokio::test]
    async fn uuid_shaped_committed_author_is_not_inferred_as_principal() -> anyhow::Result<()> {
        // The committed revision stores a UUID-shaped author string. It stays
        // portable subject provenance; its syntax alone is not an identity
        // assertion, in either live delivery or reconciliation.
        let service = UgoiteService::new("memory://mutation-audit-principal")?;
        let principal = Uuid::now_v7();
        let space_id = service
            .create_space_for_principal("audit-principal", principal, "Owner")
            .await?
            .to_string();
        write_authorized_entry(&service, &space_id, principal).await?;
        assert_eq!(audit_total(&service, &space_id).await?, 1);
        let delivered = service
            .reconcile_entry_audit(&space_id, "entry-1", &[principal], &principal.to_string())
            .await?
            .expect("principal reconcile converges");
        assert_eq!(
            delivered["subject_principal_id"],
            json!(principal.to_string())
        );
        assert!(delivered["actor_principal_id"].is_null());
        assert_eq!(audit_total(&service, &space_id).await?, 1);
        // The sweep path (no caller identity at all) converges too, purely
        // from committed metadata.
        assert_eq!(service.reconcile_space_audit(&space_id).await?, 1);
        assert_eq!(audit_total(&service, &space_id).await?, 1);
        Ok(())
    }

    #[tokio::test]
    async fn current_writer_delivery_and_reconciliation_preserve_audit_bytes() -> anyhow::Result<()>
    {
        let (service, space_id) = audit_test_space("writer-stable").await?;
        let uuid_shaped_author = Uuid::now_v7().to_string();
        service
            .create_structured_entry_with_receipt(
                &space_id,
                "entry-1",
                "Entry".into(),
                Vec::new(),
                entry_fields("content"),
                BTreeMap::new(),
                &uuid_shaped_author,
            )
            .await?;
        let sql_payload = crate::saved_sql::SqlPayload {
            name: Some("q".to_string()),
            kind: crate::saved_sql::SqlKind::UserQuery,
            metadata: None,
            sql: "SELECT 1".to_string(),
            variables: json!([]),
        };
        service
            .create_saved_sql(&space_id, Some("sql-1"), &sql_payload, &uuid_shaped_author)
            .await?;
        assert_eq!(audit_total(&service, &space_id).await?, 2);
        let bytes_after_delivery = audit_bytes(&service, &space_id).await?;

        let reopened = UgoiteService::from_operator(
            service.operator().clone(),
            service.root_uri().to_string(),
        );
        reopened.open_space(&space_id).await?;
        reopened
            .reconcile_entry_audit(&space_id, "entry-1", &[], "ignored")
            .await?;
        reopened
            .reconcile_saved_sql_audit(&space_id, "sql-1", &[], "ignored")
            .await?;
        assert_eq!(reopened.reconcile_space_audit(&space_id).await?, 2);
        assert_eq!(audit_total(&reopened, &space_id).await?, 2);
        assert_eq!(
            audit_bytes(&reopened, &space_id).await?,
            bytes_after_delivery
        );

        let events = crate::audit::list_audit_events(
            reopened.operator(),
            &space_id,
            crate::audit::AuditListOptions::default(),
        )
        .await?;
        for event in events["items"].as_array().expect("events") {
            assert_eq!(event["subject_principal_id"], json!(uuid_shaped_author));
            assert!(event["actor_principal_id"].is_null());
        }
        Ok(())
    }

    #[tokio::test]
    async fn reconciliation_rejects_events_not_bound_to_committed_revision() -> anyhow::Result<()> {
        for field in [
            "action",
            "target_type",
            "target_id",
            "revision_id",
            "change_id",
            "space_uid",
        ] {
            let (service, space_id) = audit_test_space(&format!("binding-{field}")).await?;
            write_untracked_entry(&service, &space_id, "content").await?;
            let history = crate::entry::get_entry_history(
                service.operator(),
                &service.workspace_path(&space_id),
                "entry-1",
            )
            .await?;
            let revision = &history["revisions"][0];
            let revision_id = revision["revision_id"].as_str().expect("revision id");
            let change_id = revision["change_id"].as_str().expect("Change id");
            let space_uid = service.space_uid(&space_id).await?;
            let expected = entry_revision_audit_expectation(
                &space_uid,
                "entry-1",
                ENTRY_CREATED_ACTION,
                revision_id,
                Some(change_id),
                Some("author"),
            );
            let mut conflicting = expected.missing_event;
            match field {
                "action" => conflicting["action"] = json!(ENTRY_UPDATED_ACTION),
                "target_type" => conflicting["target_type"] = json!("saved_sql"),
                "target_id" => conflicting["target_id"] = json!("other-entry"),
                "revision_id" => conflicting["metadata"]["revision_id"] = json!("other-revision"),
                "change_id" => conflicting["metadata"]["change_id"] = json!("other-change"),
                "space_uid" => conflicting["space_uid"] = json!(Uuid::now_v7().to_string()),
                _ => unreachable!(),
            }
            deliver_mutation_audit_event(service.operator(), &space_id, &conflicting).await?;
            let bytes_before_reconcile = audit_bytes(&service, &space_id).await?;
            let error = service
                .open_space(&space_id)
                .await
                .expect_err("mismatched committed identity must fail closed");
            assert!(
                format!("{error:#}").contains("does not match committed"),
                "unexpected error for {field}: {error:#}"
            );
            assert_eq!(
                audit_bytes(&service, &space_id).await?,
                bytes_before_reconcile,
                "reconciliation must not rewrite a conflicting event"
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn reconciliation_does_not_repair_a_tampered_audit_chain() -> anyhow::Result<()> {
        let (service, space_id) = audit_test_space("chain-tamper").await?;
        write_untracked_entry(&service, &space_id, "content").await?;
        let history = crate::entry::get_entry_history(
            service.operator(),
            &service.workspace_path(&space_id),
            "entry-1",
        )
        .await?;
        let revision = &history["revisions"][0];
        let space_uid = service.space_uid(&space_id).await?;
        let event = entry_revision_audit_expectation(
            &space_uid,
            "entry-1",
            ENTRY_CREATED_ACTION,
            revision["revision_id"].as_str().expect("revision id"),
            revision["change_id"].as_str(),
            Some("author"),
        )
        .missing_event;
        deliver_mutation_audit_event(service.operator(), &space_id, &event).await?;

        let mut tampered_event: Value =
            serde_json::from_slice(&audit_bytes(&service, &space_id).await?)?;
        tampered_event["actor_principal_id"] = json!("tampered");
        let mut tampered_bytes = serde_json::to_vec(&tampered_event)?;
        tampered_bytes.push(b'\n');
        service
            .operator()
            .write(
                &format!("spaces/{space_id}/audit/events.jsonl"),
                tampered_bytes.clone(),
            )
            .await?;

        let error = service
            .open_space(&space_id)
            .await
            .expect_err("tampered chain must fail closed");
        assert!(
            format!("{error:#}").contains("Audit chain integrity check failed"),
            "unexpected error: {error:#}"
        );
        assert_eq!(audit_bytes(&service, &space_id).await?, tampered_bytes);
        Ok(())
    }

    #[tokio::test]
    async fn space_sweep_restores_every_missing_revision_once() -> anyhow::Result<()> {
        let (service, space_id) = audit_test_space("sweep").await?;
        // Simulate two commit-without-delivery mutations through the
        // low-level structured entry layer, bypassing the audited service
        // methods.
        let integrity =
            crate::integrity::RealIntegrityProvider::from_space(service.operator(), &space_id)
                .await?;
        crate::entry::create_structured_entry_with_scopes_and_change(
            service.operator(),
            &service.workspace_path(&space_id),
            "entry-1",
            "Entry".into(),
            Vec::new(),
            entry_fields("content"),
            BTreeMap::new(),
            "author",
            &integrity,
            None,
            None,
        )
        .await?;
        let created_history = crate::entry::get_entry_history(
            service.operator(),
            &service.workspace_path(&space_id),
            "entry-1",
        )
        .await?;
        let created_revision_id = created_history["revisions"][0]["revision_id"]
            .as_str()
            .expect("created revision")
            .to_string();
        crate::entry::update_structured_entry_authorized_with_change(
            service.operator(),
            &service.workspace_path(&space_id),
            "entry-1",
            Some("Entry".into()),
            None,
            entry_fields("updated"),
            BTreeMap::new(),
            Some(&created_revision_id),
            "author",
            &integrity,
            None,
            None,
        )
        .await?;
        // A tombstone without delivery must converge too: enumeration reads
        // revision rows (never the Current view) so deleted entries are not
        // skipped.
        crate::entry::delete_entry(
            service.operator(),
            &service.workspace_path(&space_id),
            "entry-1",
            "author",
        )
        .await?;
        // Saved-SQL create+update without delivery: only the latest row is
        // listable, but every committed revision row still converges below.
        let sql_payload = crate::saved_sql::SqlPayload {
            name: Some("q".to_string()),
            kind: crate::saved_sql::SqlKind::UserQuery,
            metadata: None,
            sql: "SELECT 1".to_string(),
            variables: serde_json::json!([]),
        };
        let created_sql = crate::saved_sql::create_sql(
            service.operator(),
            &service.workspace_path(&space_id),
            "sql-1",
            &sql_payload,
            "author",
            &integrity,
        )
        .await?;
        let sql_revision_id = created_sql["revision_id"]
            .as_str()
            .expect("sql revision")
            .to_string();
        crate::saved_sql::update_sql(
            service.operator(),
            &service.workspace_path(&space_id),
            "sql-1",
            &sql_payload,
            &sql_revision_id,
            "author",
            &integrity,
        )
        .await?;
        assert_eq!(audit_total(&service, &space_id).await?, 0);

        // Reopen the Space and sweep: every committed revision converges.
        let root_uri = service.root_uri().to_string();
        let service2 = UgoiteService::from_operator(service.operator().clone(), root_uri);
        let converged = service2.reconcile_space_audit(&space_id).await?;
        assert_eq!(converged, 2);
        let listed = crate::audit::list_audit_events(
            service2.operator(),
            &space_id,
            crate::audit::AuditListOptions::default(),
        )
        .await?;
        assert_eq!(listed.get("total").and_then(Value::as_u64), Some(5));
        let items = listed
            .get("items")
            .and_then(Value::as_array)
            .expect("items");
        let actions: Vec<&str> = items
            .iter()
            .filter_map(|item| item.get("action").and_then(Value::as_str))
            .collect();
        assert!(actions.contains(&ENTRY_CREATED_ACTION));
        assert!(actions.contains(&ENTRY_UPDATED_ACTION));
        assert!(actions.contains(&ENTRY_DELETED_ACTION));
        assert!(actions.contains(&SAVED_SQL_CREATED_ACTION));
        assert!(actions.contains(&SAVED_SQL_UPDATED_ACTION));

        // Attribution matches committed history, not sweep-caller input.
        let history = crate::entry::get_entry_history(
            service2.operator(),
            &service2.workspace_path(&space_id),
            "entry-1",
        )
        .await?;
        let committed: std::collections::BTreeMap<String, (String, String, String)> = history
            ["revisions"]
            .as_array()
            .expect("revisions")
            .iter()
            .map(|revision| {
                (
                    revision["revision_id"].as_str().expect("rev").to_string(),
                    (
                        revision["actor"].as_str().expect("actor").to_string(),
                        revision["change_id"].as_str().expect("change").to_string(),
                        revision["operation"].as_str().expect("op").to_string(),
                    ),
                )
            })
            .collect();
        for item in items {
            // Entry evidence is attributed from Entry history; saved-SQL
            // evidence is checked against its own committed rows below.
            let action = item["action"].as_str().unwrap_or_default();
            if !action.starts_with("entry.") {
                continue;
            }
            let revision_id = item["metadata"]["revision_id"]
                .as_str()
                .expect("event revision");
            let (actor, change_id, _) = committed
                .get(revision_id)
                .expect("event names a committed revision");
            assert_eq!(item["target_id"], json!("entry-1"));
            assert_eq!(item["subject_principal_id"], json!(actor));
            assert_eq!(item["metadata"]["change_id"], json!(change_id));
        }

        // Saved-SQL evidence is attributed from its own committed rows.
        let sql_rows = crate::entry::form_revision_rows_for_audit(
            service2.operator(),
            &service2.workspace_path(&space_id),
            crate::saved_sql::SQL_FORM_NAME_FOR_AUDIT,
        )
        .await?;
        for item in items {
            let action = item["action"].as_str().unwrap_or_default();
            if !action.starts_with("saved_sql.") {
                continue;
            }
            let revision_id = item["metadata"]["revision_id"]
                .as_str()
                .expect("sql event revision");
            let row = sql_rows
                .iter()
                .find(|row| row.entry_id == "sql-1" && row.revision_id == revision_id)
                .expect("sql event names a committed revision");
            let committed_actor = if row.updated_by.trim().is_empty() {
                row.author.clone()
            } else {
                row.updated_by.clone()
            };
            assert_eq!(item["target_id"], json!("sql-1"));
            assert_eq!(item["subject_principal_id"], json!(committed_actor));
        }

        // Committed IDs are unchanged by reconciliation.
        let reopened = crate::entry::get_entry_history(
            service2.operator(),
            &service2.workspace_path(&space_id),
            "entry-1",
        )
        .await?;
        assert_eq!(reopened, history);

        // A second sweep converges without duplicating evidence.
        assert_eq!(service2.reconcile_space_audit(&space_id).await?, 2);
        assert_eq!(audit_total(&service2, &space_id).await?, 5);
        Ok(())
    }

    #[tokio::test]
    async fn committed_saved_sql_mutations_leave_evidence() -> anyhow::Result<()> {
        let (service, space_id) = audit_test_space("sql").await?;
        let payload = crate::saved_sql::SqlPayload {
            name: Some("q".to_string()),
            kind: crate::saved_sql::SqlKind::UserQuery,
            metadata: None,
            sql: "SELECT * FROM t WHERE secret = 's3cr3t'".to_string(),
            variables: serde_json::json!([]),
        };
        let created = service
            .create_saved_sql(&space_id, Some("sql-1"), &payload, "author")
            .await?;
        let revision_id = created
            .get("revision_id")
            .and_then(Value::as_str)
            .expect("revision id")
            .to_string();
        let update_payload = crate::saved_sql::SqlUpdatePayload {
            name: Some("q".to_string()),
            kind: crate::saved_sql::SqlKind::UserQuery,
            metadata: None,
            sql: "SELECT 1".to_string(),
            variables: serde_json::json!([]),
            parent_revision_id: revision_id,
        };
        service
            .update_saved_sql(
                &space_id,
                "sql-1",
                &update_payload.clone().into_sql_payload(),
                &update_payload.parent_revision_id,
                "author",
            )
            .await?;
        service
            .delete_saved_sql(&space_id, "sql-1", "author")
            .await?;

        let listed = crate::audit::list_audit_events(
            service.operator(),
            &space_id,
            crate::audit::AuditListOptions::default(),
        )
        .await?;
        assert_eq!(listed.get("total").and_then(Value::as_u64), Some(3));
        // Query content never enters the evidence.
        let raw = serde_json::to_string(&listed).expect("serializes");
        assert!(!raw.contains("s3cr3t"), "audit must not contain SQL text");
        Ok(())
    }

    #[tokio::test]
    async fn secret_metadata_is_rejected_fail_closed() -> anyhow::Result<()> {
        let (service, space_id) = audit_test_space("redact").await?;
        let space_uid = service.space_uid(&space_id).await?;
        let mut event = entry_mutation_event(
            &space_uid,
            ENTRY_CREATED_ACTION,
            "entry-1",
            "rev-1",
            None,
            "author",
            None,
        );
        event["metadata"]["token"] = json!("bearer-secret");
        let error = deliver_mutation_audit_event(service.operator(), &space_id, &event)
            .await
            .expect_err("secret material must be rejected");
        assert!(error.to_string().contains("secret"));
        assert_eq!(audit_total(&service, &space_id).await?, 0);
        Ok(())
    }

    #[tokio::test]
    async fn authorized_delete_tombstone_reconciles_with_change() -> anyhow::Result<()> {
        use ugoite_domain::change::{ChangeCommand, RunId};
        // Authorized delete with an explicit ChangeCommand: history stays
        // append-only (tombstone, never removal) and the delete evidence
        // carries the Change ID plus portable subject provenance.
        let service = UgoiteService::new("memory://mutation-audit-hard-delete")?;
        let principal = Uuid::now_v7();
        let space_id = service
            .create_space_for_principal("audit-hard-delete", principal, "Owner")
            .await?
            .to_string();
        write_authorized_entry(&service, &space_id, principal).await?;
        let delete_change = ChangeCommand {
            change_id: Uuid::now_v7().to_string(),
            run_id: Some(RunId::new("run-hard-delete")?),
            actor_principal_id: principal.to_string(),
            message: Some("authorized delete".to_string()),
            reverts_change_id: None,
            created_at_micros: chrono::Utc::now().timestamp_micros(),
        };
        let deleted = service
            .delete_entry_authorized_for_principals_with_change(
                &space_id,
                "entry-1",
                &principal.to_string(),
                &[principal],
                Some(delete_change.clone()),
            )
            .await?;
        assert_eq!(
            deleted.get("change_id").and_then(Value::as_str),
            Some(delete_change.change_id.as_str())
        );
        // Append-only: the tombstone revision is still reachable history.
        let history = crate::entry::get_entry_history(
            service.operator(),
            &service.workspace_path(&space_id),
            "entry-1",
        )
        .await?;
        let revisions = history
            .get("revisions")
            .and_then(Value::as_array)
            .expect("revisions");
        assert_eq!(revisions.len(), 2);
        assert_eq!(revisions[1]["operation"], json!("delete"));
        assert_eq!(
            revisions[1]["change_id"],
            json!(delete_change.change_id.as_str())
        );
        // Live delivery and reconcile converge without inferring a principal
        // from the UUID-shaped committed author.
        assert_eq!(audit_total(&service, &space_id).await?, 2);
        service
            .reconcile_entry_audit(&space_id, "entry-1", &[principal], &principal.to_string())
            .await?;
        assert_eq!(audit_total(&service, &space_id).await?, 2);
        let listed = crate::audit::list_audit_events(
            service.operator(),
            &space_id,
            crate::audit::AuditListOptions::default(),
        )
        .await?;
        let deleted_event = listed["items"]
            .as_array()
            .expect("items")
            .iter()
            .find(|item| item["action"] == json!(ENTRY_DELETED_ACTION))
            .expect("entry.deleted evidence");
        assert_eq!(
            deleted_event["metadata"]["change_id"],
            json!(delete_change.change_id.as_str())
        );
        assert_eq!(
            deleted_event["subject_principal_id"],
            json!(principal.to_string())
        );
        assert!(deleted_event["actor_principal_id"].is_null());
        Ok(())
    }

    #[tokio::test]
    async fn open_space_heals_crash_gap_without_explicit_reconcile() -> anyhow::Result<()> {
        let (service, space_id) = audit_test_space("open-heal").await?;
        // Simulate commit-without-delivery through the low-level entry
        // layer, bypassing the audited service method.
        write_untracked_entry(&service, &space_id, "content").await?;
        assert_eq!(audit_total(&service, &space_id).await?, 0);
        // Reopen the Space and run only the open hook: no explicit
        // reconcile call anywhere in this test.
        let root_uri = service.root_uri().to_string();
        let service2 = UgoiteService::from_operator(service.operator().clone(), root_uri);
        let opened = service2.open_space(&space_id).await?;
        assert_eq!(
            opened
                .get("audit_targets_converged")
                .and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(audit_total(&service2, &space_id).await?, 1);
        Ok(())
    }

    #[tokio::test]
    async fn audit_list_read_never_mutates_evidence() -> anyhow::Result<()> {
        let (service, space_id) = audit_test_space("light-read").await?;
        // Crash-gap space: committed revision, no evidence. Repeated light
        // reads report the gap without healing it.
        write_untracked_entry(&service, &space_id, "content").await?;
        for _ in 0..2 {
            let listed = service.list_space_audit(&space_id, 0, 100).await?;
            assert_eq!(listed.get("total").and_then(Value::as_u64), Some(0));
        }
        assert_eq!(audit_total(&service, &space_id).await?, 0);
        // After the open hook heals, repeated reads stay stable too.
        service.open_space(&space_id).await?;
        for _ in 0..2 {
            let listed = service.list_space_audit(&space_id, 0, 100).await?;
            assert_eq!(listed.get("total").and_then(Value::as_u64), Some(1));
        }
        assert_eq!(audit_total(&service, &space_id).await?, 1);
        Ok(())
    }
}
