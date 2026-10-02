//! Space-owned storage identity for Composition records.
//!
//! This module only establishes the reserved Registry Form. Composition YAML
//! parsing and record mutation are handled by higher layers after their
//! contracts are defined.

use anyhow::{anyhow, Result};
use opendal::Operator;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_core::query::EntryScope;
use ugoite_domain::entry::EntryRevision;
use ugoite_domain::form::FormDefinition;
use ugoite_domain::id::{validate_entry_id, validate_revision_id, EntryId, FieldId, RevisionId};

pub const COMPOSITION_HISTORY_MAX_PAGE_SIZE: usize = 100;

#[cfg(test)]
#[path = "composition/authorized_raw_read_tests.rs"]
mod authorized_raw_read_tests;

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
    let revision = workspace
        .read_revision_view_at_publication_with_scope(
            &publication,
            form.id,
            target_scope(entry_id),
            crate::RevisionView::All,
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
    let mut revisions = workspace
        .read_revision_view_at_publication_with_scope(
            &publication,
            form.id,
            target_scope(entry_id),
            crate::RevisionView::All,
        )
        .await?
        .into_iter()
        .filter(|revision| revision.entry_id == entry_id)
        .map(|revision| raw_revision(revision, &form_history))
        .collect::<Result<Vec<_>>>()?;
    if revisions.is_empty() {
        return Ok(None);
    }
    revisions.sort_by_key(|revision| {
        (
            revision.revision.committed_at_micros,
            revision.revision.revision_id,
        )
    });
    let total = revisions.len();
    let has_more = offset.saturating_add(limit) < total;
    let revisions = revisions.into_iter().skip(offset).take(limit).collect();
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

#[cfg(test)]
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
