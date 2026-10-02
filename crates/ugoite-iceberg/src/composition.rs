//! Space-owned storage identity for Composition records.
//!
//! This module owns the reserved Registry carrier and its Entry-backed
//! persistence boundary. YAML semantics remain defined by `ugoite-domain`.

use anyhow::{anyhow, Result};
use chrono::Utc;
use opendal::Operator;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_core::query::EntryScope;
use ugoite_domain::change::ChangeCommand;
use ugoite_domain::composition::{
    canonicalize_composition_yaml, CanonicalComposition, CompositionDocument,
};
use ugoite_domain::entry::{
    EntryMetadata, EntryOperation, EntryRevision, EntryRevisionDraft, FieldValue,
};
use ugoite_domain::form::FormDefinition;
use ugoite_domain::id::{validate_entry_id, validate_revision_id, EntryId, FieldId, RevisionId};
use uuid::Uuid;

pub const COMPOSITION_HISTORY_MAX_PAGE_SIZE: usize = 100;
pub const COMPOSITION_LIST_MAX_PAGE_SIZE: usize = 100;

#[cfg(test)]
#[path = "composition/authorized_raw_read_tests.rs"]
mod authorized_raw_read_tests;
#[cfg(test)]
#[path = "composition/history_tests.rs"]
mod history_tests;
#[cfg(test)]
#[path = "composition/list_tests.rs"]
mod list_tests;

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
    /// When omitted on update, the current Entry tags are preserved.
    pub tags: Option<Vec<String>>,
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

fn entry_uuid(entry_id: &str) -> EntryId {
    EntryId::from(
        uuid::Uuid::parse_str(entry_id).unwrap_or_else(|_| {
            uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, entry_id.as_bytes())
        }),
    )
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
    if revision.operation != EntryOperation::Upsert
        || !revision.extra_attributes.is_empty()
        || !revision.extension_metadata.is_empty()
        || revision.values.len() != form.fields.len()
        || revision.entry.external_id != revision.entry_id.to_string()
    {
        return Err(registry_conflict(
            "Composition revision has an invalid carrier shape",
        ));
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
pub(crate) async fn save_composition(
    operator: &Operator,
    workspace_path: &str,
    request: CompositionSaveRequest,
    entry_id: EntryId,
    canonical: CanonicalComposition,
    author: &str,
) -> Result<CompositionSaveResult> {
    if request.entry_id.is_some() != request.base_revision_id.is_some() {
        return Err(AppError::invalid_input(
            ErrorCode::InvalidInput,
            "Composition updates require an entry ID and exact base revision",
        )
        .into());
    }
    let form = ensure_composition_registry(operator, workspace_path).await?;
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
            let current_revision_id = current.revision_id.to_string();
            return Err(AppError::revision_conflict(
                &current_revision_id,
                &base_revision.to_string(),
                &current_revision_id,
            )
            .into());
        }
    }

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

    let change_id = Uuid::now_v7().to_string();
    let draft = EntryRevisionDraft {
        form_id: form.id,
        entry_id,
        revision_id: RevisionId::from(Uuid::now_v7()),
        change_id: change_id.clone(),
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
            tags: request.tags.unwrap_or_else(|| {
                current
                    .as_ref()
                    .map(|revision| revision.entry.tags.clone())
                    .unwrap_or_default()
            }),
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
    let document = validate_composition_revision(&revision, &form)?;
    let change = ChangeCommand {
        change_id,
        run_id: None,
        actor_principal_id: author.to_string(),
        message: Some(if current.is_some() {
            "Update Composition".to_string()
        } else {
            "Create Composition".to_string()
        }),
        reverts_change_id: None,
        created_at_micros: timestamp,
    };
    let publication =
        crate::publication_context_for_change(&change, "composition.save", &revision)?;
    crate::authorization::ensure_authorization_write_fence().await?;
    let workspace =
        crate::iceberg_store::native_mutation_workspace(operator, workspace_path).await?;
    let receipt = workspace
        .commit(publication)?
        .append_composition_revision_authorized(revision.clone())
        .await?;

    Ok(CompositionSaveResult {
        entry_id,
        revision_id: revision.revision_id,
        document,
        canonical_yaml: canonical.yaml,
        receipt,
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
        .find(|revision| revision.entry_id == entry_id && !revision.entry.deleted)
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
