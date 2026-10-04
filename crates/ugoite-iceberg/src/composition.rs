//! Space-owned storage identity for Composition records.
//!
//! This module owns the reserved Registry carrier and its Entry-backed
//! persistence boundary. YAML semantics remain defined by `ugoite-domain`.

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use opendal::Operator;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_core::query::EntryScope;
use ugoite_domain::change::ChangeDescriptor;
use ugoite_domain::composition::{
    canonicalize_composition, canonicalize_composition_yaml, CanonicalComposition,
    CompositionDocument,
};
use ugoite_domain::entry::{
    EntryMetadata, EntryOperation, EntryRevision, EntryRevisionDraft, FieldValue,
};
use ugoite_domain::form::FormDefinition;
use ugoite_domain::id::{validate_entry_id, validate_revision_id, EntryId, FieldId, RevisionId};
use uuid::Uuid;

pub const COMPOSITION_HISTORY_MAX_PAGE_SIZE: usize = 100;
pub const COMPOSITION_LIST_MAX_PAGE_SIZE: usize = 100;
const COMPOSITION_OPERATION_ID_MAX_BYTES: usize = 256;

#[cfg(test)]
#[path = "composition/authorized_raw_read_tests.rs"]
mod authorized_raw_read_tests;
#[cfg(test)]
#[path = "composition/history_tests.rs"]
mod history_tests;
#[cfg(test)]
#[path = "composition/list_tests.rs"]
mod list_tests;
#[cfg(test)]
#[path = "composition/restore_tests.rs"]
mod restore_tests;

/// An inspectable stored Composition revision. `revision.values` retains the
/// stable FieldId keyed carrier while `fields` provides its historical Form
/// field names. Values such as an unsupported format version or malformed
/// YAML remain available without invoking the strict Composition parser.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawCompositionRevision {
    pub revision: EntryRevision,
    pub fields: BTreeMap<String, Value>,
    pub unmapped_field_values: BTreeMap<FieldId, Value>,
}

impl RawCompositionRevision {
    /// Returns a version probe only when the raw carrier is a non-negative
    /// integer. It deliberately does not validate or interpret the document.
    pub fn format_version_probe(&self) -> Option<u64> {
        self.fields.get("format_version").and_then(Value::as_u64)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawCompositionHistoryPage {
    pub entry_id: EntryId,
    pub revisions: Vec<RawCompositionRevision>,
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub has_more: bool,
}

/// Bounded list projection for one current Composition Entry. The `spec`
/// carrier is intentionally omitted so Home/list surfaces do not materialize
/// or transport full YAML documents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawCompositionListItem {
    pub entry_id: String,
    pub revision_id: RevisionId,
    pub updated_at: f64,
    pub name: Option<Value>,
    pub kind: Option<Value>,
    pub format_version: Option<Value>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawCompositionListPage {
    pub items: Vec<RawCompositionListItem>,
    pub offset: usize,
    pub limit: usize,
    pub has_more: bool,
}

/// Create or update intent for one Composition Entry. Updates must carry the
/// exact current revision as `base_revision_id`; unconditional overwrite is
/// not part of this storage contract.
#[derive(Debug, Clone)]
pub struct CompositionSaveRequest {
    pub entry_id: Option<EntryId>,
    pub base_revision_id: Option<RevisionId>,
    pub document: CompositionDocument,
}

/// Result of one committed Composition publication.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompositionSaveResult {
    pub entry_id: EntryId,
    pub revision_id: RevisionId,
    pub document: CompositionDocument,
    pub canonical_yaml: String,
    pub receipt: crate::CommitReceipt,
}

/// Result of restoring one exact historical Composition revision as a new
/// append-only Entry revision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompositionRestoreResult {
    pub entry_id: EntryId,
    pub revision_id: RevisionId,
    pub restored_from_revision_id: RevisionId,
    pub document: CompositionDocument,
    pub canonical_yaml: String,
    pub receipt: crate::CommitReceipt,
}

fn entry_uuid(entry_id: &str) -> EntryId {
    EntryId::from(
        uuid::Uuid::parse_str(entry_id).unwrap_or_else(|_| {
            uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, entry_id.as_bytes())
        }),
    )
}

pub(crate) fn composition_entry_id_for_operation(space_id: &str, operation_id: &str) -> EntryId {
    let name = format!("ugoite:composition.save:v1:{space_id}:{operation_id}:entry");
    EntryId::from(Uuid::new_v5(&Uuid::NAMESPACE_URL, name.as_bytes()))
}

fn composition_revision_id_for_operation(space_id: &str, operation_id: &str) -> RevisionId {
    let name = format!("ugoite:composition.save:v1:{space_id}:{operation_id}:revision");
    RevisionId::from(Uuid::new_v5(&Uuid::NAMESPACE_URL, name.as_bytes()))
}

fn composition_command_id_for_operation(space_id: &str, operation_id: &str) -> String {
    let name = format!("ugoite:composition.save:v1:{space_id}:{operation_id}:command");
    Uuid::new_v5(&Uuid::NAMESPACE_URL, name.as_bytes()).to_string()
}

fn composition_restore_revision_id_for_operation(space_id: &str, operation_id: &str) -> RevisionId {
    let name = format!("ugoite:composition.restore:v1:{space_id}:{operation_id}:revision");
    RevisionId::from(Uuid::new_v5(&Uuid::NAMESPACE_URL, name.as_bytes()))
}

fn composition_restore_command_id_for_operation(space_id: &str, operation_id: &str) -> String {
    let name = format!("ugoite:composition.restore:v1:{space_id}:{operation_id}:command");
    Uuid::new_v5(&Uuid::NAMESPACE_URL, name.as_bytes()).to_string()
}

pub(crate) fn validate_operation_id(operation_id: &str) -> Result<()> {
    if operation_id.trim().is_empty() || operation_id.len() > COMPOSITION_OPERATION_ID_MAX_BYTES {
        return Err(AppError::invalid_input(
            ErrorCode::InvalidInput,
            "Composition operation identity must contain 1 to 256 bytes",
        )
        .into());
    }
    Ok(())
}

#[derive(Serialize)]
struct CompositionSaveIdentity<'a> {
    space_id: &'a str,
    entry_id: EntryId,
    is_create: bool,
    base_revision_id: Option<RevisionId>,
    canonical_yaml: &'a str,
    actor: &'a str,
}

#[derive(Serialize)]
struct CompositionRestoreIdentity<'a> {
    space_id: &'a str,
    entry_id: EntryId,
    source_revision_id: RevisionId,
    base_revision_id: RevisionId,
    actor: &'a str,
}

fn registry_form_at_publication(forms: Vec<FormDefinition>) -> Result<Option<FormDefinition>> {
    let expected = composition_registry_definition()?;
    let existing = forms.into_iter().find(|form| {
        form.name
            .eq_ignore_ascii_case(COMPOSITION_REGISTRY_FORM_NAME)
    });
    if let Some(existing) = &existing {
        validate_registry_definition(existing, &expected)?;
    }
    Ok(existing)
}

fn raw_revision(
    revision: EntryRevision,
    form_history: &[FormDefinition],
) -> Result<RawCompositionRevision> {
    let form = form_history
        .iter()
        .find(|form| form.version == revision.form_version)
        .ok_or_else(|| anyhow!("Composition revision Form history is incomplete"))?;
    let fields_by_id = form
        .fields
        .iter()
        .map(|field| (field.id, field.name.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut fields = BTreeMap::new();
    let mut unmapped_field_values = BTreeMap::new();
    for (field_id, value) in &revision.values {
        let value = serde_json::to_value(value)?;
        if let Some(name) = fields_by_id.get(field_id) {
            fields.insert((*name).to_string(), value);
        } else {
            unmapped_field_values.insert(*field_id, value);
        }
    }
    Ok(RawCompositionRevision {
        revision,
        fields,
        unmapped_field_values,
    })
}

fn target_scope(entry_id: EntryId) -> EntryScope {
    EntryScope::Only(BTreeSet::from([entry_id]))
}

fn validate_raw_read(entry_id: &str) -> Result<EntryId> {
    validate_entry_id(entry_id).map_err(|error| AppError::invalid_identifier(error.to_string()))?;
    Ok(entry_uuid(entry_id))
}

fn registry_field_value<'a>(
    form: &FormDefinition,
    revision: &'a EntryRevision,
    name: &str,
) -> Result<&'a FieldValue> {
    let field = form
        .fields
        .iter()
        .find(|field| field.name == name)
        .ok_or_else(|| registry_conflict("registry is missing a required carrier field"))?;
    revision
        .values
        .get(&field.id)
        .ok_or_else(|| registry_conflict("Composition revision is missing a carrier value"))
}

/// Validate every bit of the reserved Entry carrier before a production
/// Composition append. This keeps the crate-private write path unable to
/// publish malformed or noncanonical carrier values.
pub(crate) fn validate_composition_revision(
    revision: &EntryRevision,
    form: &FormDefinition,
) -> Result<CompositionDocument> {
    validate_composition_registry_form(form)?;
    if !matches!(
        revision.operation,
        EntryOperation::Upsert | EntryOperation::Restore
    ) || !revision.extra_attributes.is_empty()
        || !revision.extension_metadata.is_empty()
        || revision.values.len() != form.fields.len()
        || revision.entry.external_id != revision.entry_id.to_string()
    {
        return Err(registry_conflict(
            "Composition revision has an invalid carrier shape",
        ));
    }
    match revision.operation {
        EntryOperation::Upsert
            if revision.entry.restored_from.is_some() || revision.source_id.is_some() =>
        {
            return Err(registry_conflict(
                "Composition upsert has restore-only provenance",
            ));
        }
        EntryOperation::Restore => {
            let Some(restored_from) = revision.entry.restored_from else {
                return Err(registry_conflict(
                    "Composition restore is missing its source revision",
                ));
            };
            let restored_from = restored_from.to_string();
            if revision.source_id.as_deref() != Some(restored_from.as_str()) {
                return Err(registry_conflict(
                    "Composition restore source does not match its Entry provenance",
                ));
            }
        }
        _ => {}
    }

    let name = match registry_field_value(form, revision, "name")? {
        FieldValue::String(value) => value,
        _ => {
            return Err(registry_conflict(
                "Composition name carrier has the wrong type",
            ))
        }
    };
    let kind = match registry_field_value(form, revision, "kind")? {
        FieldValue::String(value) => value,
        _ => {
            return Err(registry_conflict(
                "Composition kind carrier has the wrong type",
            ))
        }
    };
    let format_version = match registry_field_value(form, revision, "format_version")? {
        FieldValue::Integer(value) => *value,
        _ => {
            return Err(registry_conflict(
                "Composition format-version carrier has the wrong type",
            ))
        }
    };
    let spec = match registry_field_value(form, revision, "spec")? {
        FieldValue::String(value) => value,
        _ => {
            return Err(registry_conflict(
                "Composition spec carrier has the wrong type",
            ))
        }
    };
    let canonical = canonicalize_composition_yaml(spec)
        .map_err(|diagnostic| registry_conflict(diagnostic.as_str()))?;
    let expected_kind = serde_json::to_value(canonical.document.kind)?;
    if canonical.yaml != *spec
        || canonical.document.name != *name
        || expected_kind.as_str() != Some(kind.as_str())
        || i64::from(canonical.document.format_version) != format_version
        || canonical.document.tags != revision.entry.tags
    {
        return Err(registry_conflict(
            "Composition carrier fields do not match canonical spec",
        ));
    }
    Ok(canonical.document)
}

/// Append one canonical Composition as one Entry revision. Authorization is
/// held by the service boundary; the coordinator rechecks Entry revision
/// parentage against the latest Catalog Head before publication.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn save_composition(
    operator: &Operator,
    space_id: &str,
    workspace_path: &str,
    request: CompositionSaveRequest,
    entry_id: EntryId,
    canonical: CanonicalComposition,
    author: &str,
    operation_id: &str,
) -> Result<CompositionSaveResult> {
    validate_operation_id(operation_id)?;
    if request.entry_id.is_some() != request.base_revision_id.is_some() {
        return Err(AppError::invalid_input(
            ErrorCode::InvalidInput,
            "Composition updates require an entry ID and exact base revision",
        )
        .into());
    }
    let command_id = composition_command_id_for_operation(space_id, operation_id);
    let revision_id = composition_revision_id_for_operation(space_id, operation_id);
    let identity = CompositionSaveIdentity {
        space_id,
        entry_id,
        is_create: request.entry_id.is_none(),
        base_revision_id: request.base_revision_id,
        canonical_yaml: &canonical.yaml,
        actor: author,
    };
    let publication =
        crate::publication_context(command_id.clone(), "composition.save", &identity)?;
    let now = Utc::now().timestamp_micros();
    let publication = publication
        .with_change_descriptor(ChangeDescriptor {
            run_id: None,
            actor_principal_id: author.to_string(),
            message: Some(if request.entry_id.is_some() {
                "Update Composition".to_string()
            } else {
                "Create Composition".to_string()
            }),
            reverts_change_id: None,
            created_at_micros: now,
        })
        .map_err(|error| AppError::invalid_input(ErrorCode::InvalidInput, error.to_string()))?;

    // Resolve the durable outcome before checking the base revision. A retry
    // of a committed update naturally carries the now-stale original base.
    if let Some(result) = resolve_published_composition_save(
        operator,
        workspace_path,
        entry_id,
        revision_id,
        request.base_revision_id,
        &command_id,
        &canonical.yaml,
        author,
        &publication,
    )
    .await?
    {
        return Ok(result);
    }

    let current_raw = if request.base_revision_id.is_some() {
        read_composition_raw(operator, workspace_path, &entry_id.to_string())
            .await?
            .ok_or_else(|| AppError::not_found(ErrorCode::EntryNotFound, "Composition not found"))?
            .into()
    } else {
        None
    };
    let current = current_raw.map(|raw: RawCompositionRevision| raw.revision);
    if let (Some(base_revision), Some(current)) = (request.base_revision_id, current.as_ref()) {
        if base_revision != current.revision_id {
            // The first outcome check can race the original publication. Give
            // an identical retry another chance to resolve before returning a
            // stale-base conflict.
            if let Some(result) = resolve_published_composition_save(
                operator,
                workspace_path,
                entry_id,
                revision_id,
                request.base_revision_id,
                &command_id,
                &canonical.yaml,
                author,
                &publication,
            )
            .await?
            {
                return Ok(result);
            }
            let current_revision_id = current.revision_id.to_string();
            return Err(AppError::revision_conflict(
                &current_revision_id,
                &base_revision.to_string(),
                &current_revision_id,
            )
            .into());
        }
    }
    // Do not create the Registry for a failed update of a missing Entry. A
    // current Composition update has already proved that its carrier exists;
    // creates still ensure the Registry only after the canonical input reaches
    // this storage boundary.
    let form = ensure_composition_registry(operator, workspace_path).await?;

    let timestamp = Utc::now().timestamp_micros().max(
        current
            .as_ref()
            .map(|revision| revision.entry.updated_at_micros.saturating_add(1))
            .unwrap_or_default(),
    );
    let mut values = BTreeMap::new();
    for (name, value) in [
        ("name", FieldValue::String(canonical.document.name.clone())),
        (
            "kind",
            FieldValue::String(
                serde_json::to_value(canonical.document.kind)?
                    .as_str()
                    .expect("Composition kind serializes as a string")
                    .to_string(),
            ),
        ),
        (
            "format_version",
            FieldValue::Integer(i64::from(canonical.document.format_version)),
        ),
        ("spec", FieldValue::String(canonical.yaml.clone())),
    ] {
        let field_id = form
            .fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| field.id)
            .ok_or_else(|| registry_conflict("registry is missing a required carrier field"))?;
        values.insert(field_id, value);
    }

    let draft = EntryRevisionDraft {
        form_id: form.id,
        entry_id,
        revision_id,
        change_id: command_id.clone(),
        operation: EntryOperation::Upsert,
        committed_at_micros: timestamp,
        author_id: current
            .as_ref()
            .map(|revision| revision.author_id.clone())
            .unwrap_or_else(|| author.to_string()),
        form_version: form.version,
        source_kind: "api".to_string(),
        source_id: None,
        entry: EntryMetadata {
            external_id: entry_id.to_string(),
            tags: canonical.document.tags.clone(),
            created_at_micros: current
                .as_ref()
                .map(|revision| revision.entry.created_at_micros)
                .unwrap_or(timestamp),
            updated_at_micros: timestamp,
            updated_by: author.to_string(),
            integrity: current
                .as_ref()
                .map(|revision| revision.entry.integrity.clone())
                .unwrap_or_default(),
            ..EntryMetadata::default()
        },
        values,
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    };
    let revision = draft
        .build(&form, current.as_ref())
        .map_err(|error| AppError::invalid_input(ErrorCode::InvalidInput, error.to_string()))?;
    validate_composition_revision(&revision, &form)?;
    #[cfg(debug_assertions)]
    crate::wait_at_test_validation_gate(std::slice::from_ref(&revision)).await;
    crate::authorization::ensure_authorization_write_fence().await?;
    let workspace =
        crate::iceberg_store::native_mutation_workspace(operator, workspace_path).await?;
    let mut receipt = match workspace
        .commit(publication.clone())?
        .append_composition_revision_authorized(revision.clone())
        .await
    {
        Ok(receipt) => receipt,
        Err(error) => {
            // Another retry with the same operation identity may have
            // committed after our last preflight but before this append lost
            // the Head race. Recover its durable result before reporting the
            // stale-base error from our losing writer.
            if let Some(result) = resolve_published_composition_save(
                operator,
                workspace_path,
                entry_id,
                revision_id,
                request.base_revision_id,
                &command_id,
                &canonical.yaml,
                author,
                &publication,
            )
            .await?
            {
                return Ok(result);
            }
            return Err(error);
        }
    };

    if receipt.data_file_count == 0 {
        // Generic append recovery reports files created by this attempt. For
        // Composition saves, return the stable receipt of the durable
        // publication so a recovered save matches its later idempotent replay.
        receipt = resolve_published_composition_save(
            operator,
            workspace_path,
            entry_id,
            revision_id,
            request.base_revision_id,
            &command_id,
            &canonical.yaml,
            author,
            &publication,
        )
        .await?
        .ok_or_else(|| registry_conflict("published Composition outcome is missing"))?
        .receipt;
    }

    let committed = read_composition_raw_revision(
        operator,
        workspace_path,
        &entry_id.to_string(),
        &revision_id.to_string(),
    )
    .await?
    .ok_or_else(|| registry_conflict("published Composition revision is missing"))?;
    let committed_document = validate_saved_composition_revision(
        &committed,
        entry_id,
        revision_id,
        request.base_revision_id,
        &command_id,
        &canonical.yaml,
        author,
    )?;
    receipt.committed_revision_ids = vec![committed.revision.revision_id];
    receipt.committed_at_micros = committed.revision.committed_at_micros;

    Ok(CompositionSaveResult {
        entry_id,
        revision_id: committed.revision.revision_id,
        document: committed_document,
        canonical_yaml: canonical.yaml,
        receipt,
    })
}

#[allow(clippy::too_many_arguments)]
async fn resolve_published_composition_save(
    operator: &Operator,
    workspace_path: &str,
    entry_id: EntryId,
    revision_id: RevisionId,
    base_revision_id: Option<RevisionId>,
    command_id: &str,
    canonical_yaml: &str,
    author: &str,
    publication: &crate::PublicationContext,
) -> Result<Option<CompositionSaveResult>> {
    let workspace =
        crate::iceberg_store::native_mutation_workspace(operator, workspace_path).await?;
    let coordinator = workspace.commit(publication.clone())?;
    let outcome = match coordinator.publication_outcome().await {
        Ok(outcome) => outcome,
        Err(error) if publication_content_conflict(&error) => {
            return Err(AppError::conflict(
                ErrorCode::IdempotencyConflict,
                "Composition operation identity was reused with different save content",
            )
            .into());
        }
        Err(error) => return Err(error),
    };
    let Some(outcome) = outcome else {
        return Ok(None);
    };
    let raw = read_composition_raw_revision(
        operator,
        workspace_path,
        &entry_id.to_string(),
        &revision_id.to_string(),
    )
    .await?
    .ok_or_else(|| registry_conflict("published Composition revision is missing"))?;
    let document = validate_saved_composition_revision(
        &raw,
        entry_id,
        revision_id,
        base_revision_id,
        command_id,
        canonical_yaml,
        author,
    )?;
    let snapshot_id = outcome
        .snapshot_id
        .context("Composition publication did not create an Iceberg snapshot")?;
    Ok(Some(CompositionSaveResult {
        entry_id,
        revision_id: raw.revision.revision_id,
        document,
        canonical_yaml: canonical_yaml.to_string(),
        receipt: crate::CommitReceipt {
            command_id: outcome.command_id,
            catalog_generation: outcome.catalog_generation,
            snapshot_id,
            committed_revision_ids: vec![raw.revision.revision_id],
            committed_at_micros: raw.revision.committed_at_micros,
            data_file_count: outcome.data_file_count,
        },
    }))
}

fn validate_saved_composition_revision(
    raw: &RawCompositionRevision,
    entry_id: EntryId,
    revision_id: RevisionId,
    base_revision_id: Option<RevisionId>,
    command_id: &str,
    canonical_yaml: &str,
    author: &str,
) -> Result<CompositionDocument> {
    let spec = raw
        .fields
        .get("spec")
        .and_then(Value::as_str)
        .ok_or_else(|| registry_conflict("published Composition spec is missing"))?;
    let canonical = canonicalize_composition_yaml(spec)
        .map_err(|diagnostic| registry_conflict(diagnostic.as_str()))?;
    if raw.revision.entry_id != entry_id
        || raw.revision.revision_id != revision_id
        || raw.revision.change_id != command_id
        || raw.revision.parent_revision_id != base_revision_id
        || raw.revision.operation != EntryOperation::Upsert
        || raw.revision.entry.updated_by != author
        || raw.revision.entry.tags != canonical.document.tags
        || canonical.yaml != canonical_yaml
        || raw.fields.get("name").and_then(Value::as_str) != Some(canonical.document.name.as_str())
        || raw.fields.get("format_version").and_then(Value::as_i64)
            != Some(i64::from(canonical.document.format_version))
    {
        return Err(registry_conflict(
            "published Composition revision does not match its command identity",
        ));
    }
    let expected_kind = serde_json::to_value(canonical.document.kind)?;
    if raw.fields.get("kind").and_then(Value::as_str) != expected_kind.as_str() {
        return Err(registry_conflict(
            "published Composition kind does not match its canonical spec",
        ));
    }
    Ok(canonical.document)
}

/// Restore one exact historical Composition revision as a new Entry
/// revision. The operation identity and exact base bind retries to one durable
/// publication, and both the source and new carrier use the Composition
/// validator used by ordinary saves.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn restore_composition(
    operator: &Operator,
    space_id: &str,
    workspace_path: &str,
    entry_id: EntryId,
    source_revision_id: RevisionId,
    base_revision_id: RevisionId,
    author: &str,
    operation_id: &str,
) -> Result<CompositionRestoreResult> {
    validate_operation_id(operation_id)?;

    let command_id = composition_restore_command_id_for_operation(space_id, operation_id);
    let revision_id = composition_restore_revision_id_for_operation(space_id, operation_id);
    let identity = CompositionRestoreIdentity {
        space_id,
        entry_id,
        source_revision_id,
        base_revision_id,
        actor: author,
    };
    let publication_context =
        crate::publication_context(command_id.clone(), "composition.restore", &identity)?
            .with_change_descriptor(ChangeDescriptor {
                run_id: None,
                actor_principal_id: author.to_string(),
                message: Some("Restore Composition".to_string()),
                reverts_change_id: None,
                created_at_micros: Utc::now().timestamp_micros(),
            })
            .map_err(|error| AppError::invalid_input(ErrorCode::InvalidInput, error.to_string()))?;

    // Resolve a durable outcome before comparing the current revision with
    // the supplied base. A committed retry naturally carries a stale base.
    if let Some(result) = resolve_published_composition_restore(
        operator,
        workspace_path,
        entry_id,
        source_revision_id,
        base_revision_id,
        revision_id,
        &command_id,
        author,
        &publication_context,
    )
    .await?
    {
        return Ok(result);
    }

    let workspace =
        crate::iceberg_store::native_workspace_read_only(operator, workspace_path).await?;
    let current_publication = workspace.current_publication().await?;
    let Some(form) =
        registry_form_at_publication(workspace.forms_at_publication(&current_publication).await?)?
    else {
        return Err(AppError::not_found(
            ErrorCode::EntryNotFound,
            format!("Composition not found: {entry_id}"),
        )
        .into());
    };
    let checkpoint = workspace.resolve_publication(&current_publication).await?;
    let current = workspace
        .read_revision_view_at_publication_with_scope(
            &current_publication,
            form.id,
            target_scope(entry_id),
            crate::RevisionView::LatestIncludingTombstones,
        )
        .await?
        .into_iter()
        .find(|revision| revision.entry_id == entry_id)
        .ok_or_else(|| {
            AppError::not_found(
                ErrorCode::EntryNotFound,
                format!("Composition not found: {entry_id}"),
            )
        })?;
    let Some(source) = workspace
        .read_revision_ids_at_checkpoint_with_scope(
            &checkpoint,
            form.id,
            target_scope(entry_id),
            &[source_revision_id],
        )
        .await?
        .into_iter()
        .find(|revision| {
            revision.entry_id == entry_id && revision.revision_id == source_revision_id
        })
    else {
        return Err(AppError::not_found(
            ErrorCode::EntryNotFound,
            format!("Composition not found: {entry_id}"),
        )
        .into());
    };

    validate_composition_revision(&source, &form)?;
    if current.revision_id != base_revision_id {
        // A same-key writer can publish between outcome resolution and this
        // read. Check once more before reporting its now-stale base.
        if let Some(result) = resolve_published_composition_restore(
            operator,
            workspace_path,
            entry_id,
            source_revision_id,
            base_revision_id,
            revision_id,
            &command_id,
            author,
            &publication_context,
        )
        .await?
        {
            return Ok(result);
        }
        let current_revision_id = current.revision_id.to_string();
        return Err(AppError::revision_conflict(
            &current_revision_id,
            &base_revision_id.to_string(),
            &current_revision_id,
        )
        .into());
    }
    let timestamp = Utc::now()
        .timestamp_micros()
        .max(current.entry.updated_at_micros.saturating_add(1));
    let mut entry = source.entry.clone();
    entry.external_id = current.entry.external_id.clone();
    entry.created_at_micros = current.entry.created_at_micros;
    entry.updated_at_micros = timestamp;
    entry.updated_by = author.to_string();
    entry.integrity = current.entry.integrity.clone();
    entry.deleted = false;
    entry.deleted_at_micros = None;
    entry.deleted_by = None;
    entry.restored_from = Some(source_revision_id);
    let revision = EntryRevisionDraft {
        form_id: form.id,
        entry_id,
        revision_id,
        change_id: command_id.clone(),
        operation: EntryOperation::Restore,
        committed_at_micros: timestamp,
        author_id: current.author_id.clone(),
        form_version: form.version,
        source_kind: "api".to_string(),
        source_id: Some(source_revision_id.to_string()),
        entry,
        values: source.values,
        extra_attributes: source.extra_attributes,
        extension_metadata: BTreeMap::new(),
    }
    .build(&form, Some(&current))
    .map_err(|error| AppError::invalid_input(ErrorCode::InvalidInput, error.to_string()))?;
    validate_composition_revision(&revision, &form)?;
    crate::authorization::ensure_authorization_write_fence().await?;
    let workspace =
        crate::iceberg_store::native_mutation_workspace(operator, workspace_path).await?;
    match workspace
        .commit(publication_context.clone())?
        .append_composition_revision_authorized(revision)
        .await
    {
        Ok(_) => resolve_published_composition_restore(
            operator,
            workspace_path,
            entry_id,
            source_revision_id,
            base_revision_id,
            revision_id,
            &command_id,
            author,
            &publication_context,
        )
        .await?
        .ok_or_else(|| registry_conflict("published Composition restore outcome is missing")),
        Err(error) => {
            if let Some(result) = resolve_published_composition_restore(
                operator,
                workspace_path,
                entry_id,
                source_revision_id,
                base_revision_id,
                revision_id,
                &command_id,
                author,
                &publication_context,
            )
            .await?
            {
                return Ok(result);
            }
            Err(error)
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn resolve_published_composition_restore(
    operator: &Operator,
    workspace_path: &str,
    entry_id: EntryId,
    source_revision_id: RevisionId,
    base_revision_id: RevisionId,
    revision_id: RevisionId,
    command_id: &str,
    author: &str,
    publication: &crate::PublicationContext,
) -> Result<Option<CompositionRestoreResult>> {
    let workspace =
        crate::iceberg_store::native_mutation_workspace(operator, workspace_path).await?;
    let outcome = match workspace
        .commit(publication.clone())?
        .publication_outcome()
        .await
    {
        Ok(outcome) => outcome,
        Err(error) if publication_content_conflict(&error) => {
            return Err(AppError::conflict(
                ErrorCode::IdempotencyConflict,
                "Composition operation identity was reused with different restore content",
            )
            .into());
        }
        Err(error) => return Err(error),
    };
    let Some(outcome) = outcome else {
        return Ok(None);
    };
    let raw = read_composition_raw_revision(
        operator,
        workspace_path,
        &entry_id.to_string(),
        &revision_id.to_string(),
    )
    .await?
    .ok_or_else(|| registry_conflict("published Composition restore revision is missing"))?;
    let document = validate_restored_composition_revision(
        &raw,
        entry_id,
        source_revision_id,
        base_revision_id,
        revision_id,
        command_id,
        author,
    )?;
    let snapshot_id = outcome
        .snapshot_id
        .context("Composition restore publication did not create an Iceberg snapshot")?;
    Ok(Some(CompositionRestoreResult {
        entry_id,
        revision_id: raw.revision.revision_id,
        restored_from_revision_id: source_revision_id,
        canonical_yaml: canonicalize_composition(&document)
            .map_err(|diagnostic| registry_conflict(diagnostic.as_str()))?
            .yaml,
        document,
        receipt: crate::CommitReceipt {
            command_id: outcome.command_id,
            catalog_generation: outcome.catalog_generation,
            snapshot_id,
            committed_revision_ids: vec![raw.revision.revision_id],
            committed_at_micros: raw.revision.committed_at_micros,
            data_file_count: outcome.data_file_count,
        },
    }))
}

fn validate_restored_composition_revision(
    raw: &RawCompositionRevision,
    entry_id: EntryId,
    source_revision_id: RevisionId,
    base_revision_id: RevisionId,
    revision_id: RevisionId,
    command_id: &str,
    author: &str,
) -> Result<CompositionDocument> {
    let spec = raw
        .fields
        .get("spec")
        .and_then(Value::as_str)
        .ok_or_else(|| registry_conflict("published Composition restore spec is missing"))?;
    let canonical = canonicalize_composition_yaml(spec)
        .map_err(|diagnostic| registry_conflict(diagnostic.as_str()))?;
    let restored_from = source_revision_id.to_string();
    if raw.revision.entry_id != entry_id
        || raw.revision.revision_id != revision_id
        || raw.revision.change_id != command_id
        || raw.revision.parent_revision_id != Some(base_revision_id)
        || raw.revision.operation != EntryOperation::Restore
        || raw.revision.entry.updated_by != author
        || raw.revision.entry.deleted
        || raw.revision.entry.restored_from != Some(source_revision_id)
        || raw.revision.source_id.as_deref() != Some(restored_from.as_str())
        || raw.revision.entry.tags != canonical.document.tags
        || raw.fields.len() != 4
        || !raw.unmapped_field_values.is_empty()
        || !raw.revision.extra_attributes.is_empty()
        || !raw.revision.extension_metadata.is_empty()
        || canonical.yaml != spec
        || raw.fields.get("name").and_then(Value::as_str) != Some(canonical.document.name.as_str())
        || raw.fields.get("format_version").and_then(Value::as_i64)
            != Some(i64::from(canonical.document.format_version))
    {
        return Err(registry_conflict(
            "published Composition restore does not match its command identity",
        ));
    }
    let expected_kind = serde_json::to_value(canonical.document.kind)?;
    if raw.fields.get("kind").and_then(Value::as_str) != expected_kind.as_str() {
        return Err(registry_conflict(
            "published Composition restore kind does not match its canonical spec",
        ));
    }
    Ok(canonical.document)
}

fn publication_content_conflict(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<crate::space_catalog::PublicationContentConflict>()
            .is_some()
    })
}

pub(crate) fn validate_history_page(limit: usize) -> Result<()> {
    if !(1..=COMPOSITION_HISTORY_MAX_PAGE_SIZE).contains(&limit) {
        return Err(AppError::invalid_input(
            ErrorCode::InvalidInput,
            format!("Composition history limit must be between 1 and {COMPOSITION_HISTORY_MAX_PAGE_SIZE}"),
        )
        .into());
    }
    Ok(())
}

pub(crate) fn validate_list_page(limit: usize, offset: usize) -> Result<()> {
    if !(1..=COMPOSITION_LIST_MAX_PAGE_SIZE).contains(&limit) {
        return Err(AppError::invalid_input(
            ErrorCode::InvalidInput,
            format!(
                "Composition list limit must be between 1 and {COMPOSITION_LIST_MAX_PAGE_SIZE}"
            ),
        )
        .into());
    }
    if offset.checked_add(limit + 1).is_none() {
        return Err(AppError::invalid_input(
            ErrorCode::InvalidInput,
            "Composition list offset is out of range",
        )
        .into());
    }
    Ok(())
}

/// Reads one deterministic, current Composition page without creating or
/// repairing its Registry Form. The caller supplies current ACL scopes.
pub(crate) async fn read_composition_page(
    operator: &Operator,
    workspace_path: &str,
    entry_scope: EntryScope,
    limit: usize,
    offset: usize,
) -> Result<RawCompositionListPage> {
    validate_list_page(limit, offset)?;
    let workspace =
        crate::iceberg_store::native_workspace_read_only(operator, workspace_path).await?;
    let publication = workspace.current_publication().await?;
    let checkpoint = workspace.resolve_publication(&publication).await?;
    let forms = workspace.forms_at_publication(&publication).await?;
    let Some(form) = registry_form_at_publication(forms.clone())? else {
        return Ok(RawCompositionListPage {
            items: Vec::new(),
            offset,
            limit,
            has_more: false,
        });
    };
    let field_id = |name: &str| {
        form.fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| field.id)
            .ok_or_else(|| registry_conflict("registry is missing a required carrier field"))
    };
    let projection = [
        field_id("name")?,
        field_id("kind")?,
        field_id("format_version")?,
    ];
    let form_id = form.id;
    let scopes = BTreeMap::from([(form.name.to_ascii_lowercase(), entry_scope)]);
    let (rows, has_more) = crate::index::query_form_projected_page_authorized(
        operator,
        workspace_path,
        crate::index::FormProjectionPage {
            checkpoint,
            forms,
            form_id,
            relation_scopes: &scopes,
            field_ids: &projection,
            limit,
            offset,
        },
    )
    .await?;
    let items = rows
        .into_iter()
        .map(|row| RawCompositionListItem {
            entry_id: row.entry_id,
            revision_id: row.revision_id,
            updated_at: row.updated_at,
            name: row.fields.get("name").cloned(),
            kind: row.fields.get("kind").cloned(),
            format_version: row.fields.get("format_version").cloned(),
            tags: row.tags,
        })
        .collect();
    Ok(RawCompositionListPage {
        items,
        offset,
        limit,
        has_more,
    })
}

/// Reads the current Composition revision without creating or repairing its
/// Registry Form. Deleted records are absent from the current read surface.
pub(crate) async fn read_composition_raw(
    operator: &Operator,
    workspace_path: &str,
    entry_id: &str,
) -> Result<Option<RawCompositionRevision>> {
    Ok(
        read_composition_raw_latest_including_tombstones(operator, workspace_path, entry_id)
            .await?
            .filter(|revision| !revision.revision.entry.deleted),
    )
}

/// Reads the latest Composition revision while retaining a tombstone. This is
/// used by restore wrappers to bind an operation to the exact current base;
/// callers must still enforce the appropriate authorization boundary.
pub(crate) async fn read_composition_raw_latest_including_tombstones(
    operator: &Operator,
    workspace_path: &str,
    entry_id: &str,
) -> Result<Option<RawCompositionRevision>> {
    let entry_id = validate_raw_read(entry_id)?;
    let workspace =
        crate::iceberg_store::native_workspace_read_only(operator, workspace_path).await?;
    let publication = workspace.current_publication().await?;
    let Some(form) =
        registry_form_at_publication(workspace.forms_at_publication(&publication).await?)?
    else {
        return Ok(None);
    };
    let history = workspace
        .form_history_at_publication(&publication, form.id)
        .await?;
    let mut revisions = workspace
        .read_revision_view_at_publication_with_scope(
            &publication,
            form.id,
            target_scope(entry_id),
            crate::RevisionView::LatestIncludingTombstones,
        )
        .await?;
    let Some(revision) = revisions
        .drain(..)
        .find(|revision| revision.entry_id == entry_id)
    else {
        return Ok(None);
    };
    Ok(Some(raw_revision(revision, &history)?))
}

/// Reads an exact Composition revision from the append-only Entry history.
/// There is no fallback to the current revision when the requested ID is
/// missing, and tombstones remain inspectable through this path.
pub(crate) async fn read_composition_raw_revision(
    operator: &Operator,
    workspace_path: &str,
    entry_id: &str,
    revision_id: &str,
) -> Result<Option<RawCompositionRevision>> {
    let entry_id = validate_raw_read(entry_id)?;
    validate_revision_id(revision_id)
        .map_err(|error| AppError::invalid_identifier(error.to_string()))?;
    let revision_id = uuid::Uuid::parse_str(revision_id)
        .map(RevisionId::from)
        .map_err(|error| AppError::invalid_identifier(error.to_string()))?;
    let workspace =
        crate::iceberg_store::native_workspace_read_only(operator, workspace_path).await?;
    let publication = workspace.current_publication().await?;
    let Some(form) =
        registry_form_at_publication(workspace.forms_at_publication(&publication).await?)?
    else {
        return Ok(None);
    };
    let history = workspace
        .form_history_at_publication(&publication, form.id)
        .await?;
    let checkpoint = workspace.resolve_publication(&publication).await?;
    let revision = workspace
        .read_revision_ids_at_checkpoint_with_scope(
            &checkpoint,
            form.id,
            target_scope(entry_id),
            &[revision_id],
        )
        .await?
        .into_iter()
        .find(|revision| revision.entry_id == entry_id && revision.revision_id == revision_id);
    revision
        .map(|revision| raw_revision(revision, &history))
        .transpose()
}

/// Reads a bounded page of the append-only history for one Composition.
/// Authorization is supplied by the service boundary; the provider read is
/// always narrowed to this Entry before revisions are decoded.
pub(crate) async fn read_composition_history_page(
    operator: &Operator,
    workspace_path: &str,
    entry_id: &str,
    limit: usize,
    offset: usize,
) -> Result<Option<RawCompositionHistoryPage>> {
    let entry_id = validate_raw_read(entry_id)?;
    validate_history_page(limit)?;
    let workspace =
        crate::iceberg_store::native_workspace_read_only(operator, workspace_path).await?;
    let publication = workspace.current_publication().await?;
    let Some(form) =
        registry_form_at_publication(workspace.forms_at_publication(&publication).await?)?
    else {
        return Ok(None);
    };
    let form_history = workspace
        .form_history_at_publication(&publication, form.id)
        .await?;
    let checkpoint = workspace.resolve_publication(&publication).await?;
    let (mut revisions, total) = workspace
        .read_revision_page_at_checkpoint_with_scope(
            &checkpoint,
            form.id,
            target_scope(entry_id),
            offset,
            limit,
        )
        .await?;
    if total == 0 {
        return Ok(None);
    }
    let has_more = revisions.len() > limit;
    revisions.truncate(limit);
    let revisions = revisions
        .into_iter()
        .map(|revision| raw_revision(revision, &form_history))
        .collect::<Result<Vec<_>>>()?;
    let total = usize::try_from(total)
        .map_err(|_| anyhow!("Composition history total exceeds the supported range"))?;
    Ok(Some(RawCompositionHistoryPage {
        entry_id,
        revisions,
        total,
        offset,
        limit,
        has_more,
    }))
}

pub const COMPOSITION_REGISTRY_FORM_NAME: &str = "_ugoite_compositions";
const COMPOSITION_REGISTRY_MARKER_KEY: &str = "ugoite.registry";
const COMPOSITION_REGISTRY_MARKER: &str = "composition.v1";

fn registry_conflict(reason: &str) -> anyhow::Error {
    AppError::conflict(
        ErrorCode::CompositionRegistryConflict,
        format!("composition_registry_conflict: {reason}"),
    )
    .into()
}

/// Generic Entry mutation paths must not publish directly into the reserved
/// Composition Registry. Composition-aware operations validate canonical
/// YAML before appending the corresponding Entry revision.
pub(crate) fn ensure_generic_entry_write_allowed(form_name: &str) -> Result<()> {
    if form_name.eq_ignore_ascii_case(COMPOSITION_REGISTRY_FORM_NAME) {
        return Err(registry_conflict(
            "generic Entry mutations cannot write the Composition registry",
        ));
    }
    Ok(())
}

/// Build the expected registry definition. Field IDs and types are stable
/// storage identities; Form IDs are assigned once when the Space is created.
pub fn composition_registry_definition() -> Result<FormDefinition> {
    let mut definition = crate::form::to_domain_form(&json!({
        "id": uuid::Uuid::now_v7().to_string(),
        "name": COMPOSITION_REGISTRY_FORM_NAME,
        "version": 1,
        "fields": {
            "name": {"id": 100, "type": "string", "required": true},
            "kind": {"id": 101, "type": "string", "required": true},
            "format_version": {"id": 102, "type": "integer", "required": true},
            "spec": {"id": 103, "type": "string", "required": true}
        },
        "allow_extra_attributes": "deny"
    }))?;
    definition.extension_metadata.insert(
        COMPOSITION_REGISTRY_MARKER_KEY.to_string(),
        json!(COMPOSITION_REGISTRY_MARKER),
    );
    Ok(definition)
}

fn validate_registry_definition(
    existing: &FormDefinition,
    expected: &FormDefinition,
) -> Result<()> {
    let same_schema = existing.name == expected.name
        && existing.version == expected.version
        && existing.description == expected.description
        && existing.allow_extra_attributes == expected.allow_extra_attributes
        && existing.extension_metadata == expected.extension_metadata
        && existing.fields.len() == expected.fields.len()
        && expected.fields.iter().all(|expected_field| {
            existing
                .fields
                .iter()
                .any(|existing_field| existing_field == expected_field)
        });
    if !same_schema {
        return Err(registry_conflict(
            "the reserved Form marker or schema does not match the Composition registry",
        ));
    }
    Ok(())
}

pub(crate) fn validate_composition_registry_form(existing: &FormDefinition) -> Result<()> {
    let expected = composition_registry_definition()?;
    validate_registry_definition(existing, &expected)
}

/// Ensure the reserved Registry Form exists and has the exact Composition
/// identity. A same-name Form is never upgraded or adopted automatically.
pub async fn ensure_composition_registry(
    operator: &Operator,
    workspace_path: &str,
) -> Result<FormDefinition> {
    let mut expected = composition_registry_definition()?;
    let space_id = crate::iceberg_store::stable_space_id(operator, workspace_path).await?;
    expected.id = ugoite_domain::id::FormId::from(uuid::Uuid::new_v5(
        &space_id.as_uuid(),
        COMPOSITION_REGISTRY_FORM_NAME.as_bytes(),
    ));
    let workspace = crate::iceberg_store::native_workspace(operator, workspace_path).await?;
    let existing = workspace.list_forms().await?.into_iter().find(|form| {
        form.name
            .eq_ignore_ascii_case(COMPOSITION_REGISTRY_FORM_NAME)
    });

    if let Some(existing) = existing {
        validate_registry_definition(&existing, &expected)?;
        return Ok(existing);
    }

    match crate::form::create_system_form(operator, workspace_path, &expected).await {
        Ok(()) => Ok(expected),
        Err(error) => {
            // Every concurrent opener derives the same ID from immutable
            // Space identity. After any creation error, re-read current Head
            // and accept only the exact marker/schema; never mint a second
            // same-name registry with a different ID.
            let workspace =
                match crate::iceberg_store::native_workspace(operator, workspace_path).await {
                    Ok(workspace) => workspace,
                    Err(inspect_error) => {
                        return Err(error.context(format!(
                    "registry creation failed and follow-up inspection failed: {inspect_error:#}"
                )))
                    }
                };
            let forms = match workspace.list_forms().await {
                Ok(forms) => forms,
                Err(inspect_error) => {
                    return Err(error.context(format!(
                    "registry creation failed and follow-up inspection failed: {inspect_error:#}"
                )))
                }
            };
            let existing = forms.into_iter().find(|form| {
                form.name
                    .eq_ignore_ascii_case(COMPOSITION_REGISTRY_FORM_NAME)
            });
            if let Some(existing) = existing {
                validate_registry_definition(&existing, &expected)?;
                Ok(existing)
            } else {
                Err(error)
            }
        }
    }
}

/// Fixture-only carrier for one raw Composition revision.
///
/// The typed parser must reject some of these carriers (future
/// `format_version`, malformed documents); that is the point of the fixture.
/// The batch still travels through the reserved Registry admission — Registry
/// identity plus the revision-batch fingerprint — so this is not a generic
/// write bypass: generic Entry writes still cannot target the Registry, and
/// production save/restore paths never call this function.
#[cfg(feature = "test-support")]
#[derive(Debug, Clone)]
pub struct RawCompositionSeed {
    pub name: String,
    pub kind: String,
    pub tags: Vec<String>,
    pub format_version: i64,
    /// Exact raw YAML bytes preserved verbatim in the `spec` carrier.
    pub spec: String,
}

/// Identity of a fixture-seeded raw Composition revision.
#[cfg(feature = "test-support")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawCompositionSeedIds {
    pub entry_id: EntryId,
    pub revision_id: RevisionId,
}

/// Persist one raw Composition revision through the reserved Registry
/// admission without typed document validation. Available only with the
/// `test-support` feature for CLI/E2E recovery journeys.
#[cfg(feature = "test-support")]
pub async fn seed_raw_composition_revision(
    operator: &Operator,
    workspace_path: &str,
    seed: RawCompositionSeed,
    author: &str,
) -> Result<RawCompositionSeedIds> {
    use ugoite_domain::change::ChangeCommand;

    let form = ensure_composition_registry(operator, workspace_path).await?;
    let entry_id = EntryId::from(Uuid::now_v7());
    let revision_id = RevisionId::from(Uuid::now_v7());
    let change_id = Uuid::now_v7().to_string();
    let now = Utc::now().timestamp_micros();
    let mut values = BTreeMap::new();
    for (name, value) in [
        ("name", FieldValue::String(seed.name)),
        ("kind", FieldValue::String(seed.kind)),
        ("format_version", FieldValue::Integer(seed.format_version)),
        ("spec", FieldValue::String(seed.spec)),
    ] {
        let field_id = form
            .fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| field.id)
            .ok_or_else(|| registry_conflict("registry is missing a required carrier field"))?;
        values.insert(field_id, value);
    }
    let draft = EntryRevisionDraft {
        form_id: form.id,
        entry_id,
        revision_id,
        change_id: change_id.clone(),
        operation: EntryOperation::Upsert,
        committed_at_micros: now,
        author_id: author.to_string(),
        form_version: form.version,
        source_kind: "test".to_string(),
        source_id: None,
        entry: EntryMetadata {
            external_id: entry_id.to_string(),
            tags: seed.tags,
            created_at_micros: now,
            updated_at_micros: now,
            updated_by: author.to_string(),
            ..EntryMetadata::default()
        },
        values,
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    };
    let revision = draft
        .build(&form, None)
        .map_err(|error| AppError::invalid_input(ErrorCode::InvalidInput, error.to_string()))?;
    let change = ChangeCommand {
        change_id,
        run_id: None,
        actor_principal_id: author.to_string(),
        message: Some("seed a raw Composition revision fixture".to_string()),
        reverts_change_id: None,
        created_at_micros: now,
    };
    crate::authorization::ensure_authorization_write_fence().await?;
    let workspace =
        crate::iceberg_store::native_mutation_workspace(operator, workspace_path).await?;
    workspace
        .commit(crate::publication_context_for_change(
            &change,
            "test.composition.raw-seed",
            &revision,
        )?)?
        .append_composition_revisions_authorized(form.id, vec![revision], None)
        .await?;
    Ok(RawCompositionSeedIds {
        entry_id,
        revision_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn registry_definition_has_expected_fields_and_marker() -> anyhow::Result<()> {
        let definition = composition_registry_definition()?;
        assert_eq!(definition.name, COMPOSITION_REGISTRY_FORM_NAME);
        assert!(!definition.allow_extra_attributes);
        assert_eq!(
            definition
                .extension_metadata
                .get(COMPOSITION_REGISTRY_MARKER_KEY),
            Some(&json!(COMPOSITION_REGISTRY_MARKER))
        );
        let fields = definition
            .fields
            .iter()
            .map(|field| (field.name.as_str(), field.field_type.as_str()))
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(
            fields,
            [
                ("format_version", "integer"),
                ("kind", "string"),
                ("name", "string"),
                ("spec", "string"),
            ]
            .into_iter()
            .collect()
        );
        Ok(())
    }

    #[test]
    fn registry_identity_rejects_marker_and_schema_mismatch() -> anyhow::Result<()> {
        let expected = composition_registry_definition()?;
        let mut wrong_marker = expected.clone();
        wrong_marker
            .extension_metadata
            .insert(COMPOSITION_REGISTRY_MARKER_KEY.to_string(), json!("other"));
        assert!(validate_registry_definition(&wrong_marker, &expected).is_err());

        let mut wrong_schema = expected.clone();
        wrong_schema.fields.pop();
        assert!(validate_registry_definition(&wrong_schema, &expected).is_err());

        let mut wrong_name = expected.clone();
        wrong_name.name = "_ugoite_compositions_old".to_string();
        assert!(validate_registry_definition(&wrong_name, &expected).is_err());
        Ok(())
    }
}
