//! Space-owned storage identity for Composition records.
//!
//! This module only establishes the reserved Registry Form. Composition YAML
//! parsing and record mutation are handled by higher layers after their
//! contracts are defined.

use anyhow::Result;
use opendal::Operator;
use serde_json::json;
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_domain::form::FormDefinition;

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

/// Ensure the reserved Registry Form exists and has the exact Composition
/// identity. A same-name Form is never upgraded or adopted automatically.
pub async fn ensure_composition_registry(
    operator: &Operator,
    workspace_path: &str,
) -> Result<FormDefinition> {
    let expected = composition_registry_definition()?;
    let workspace = crate::iceberg_store::native_workspace(operator, workspace_path).await?;
    let existing = workspace.list_forms().await?.into_iter().find(|form| {
        form.name
            .eq_ignore_ascii_case(COMPOSITION_REGISTRY_FORM_NAME)
    });

    if let Some(existing) = existing {
        validate_registry_definition(&existing, &expected)?;
        return Ok(existing);
    }

    let mut created = expected.clone();
    created.id = ugoite_domain::id::FormId::from(uuid::Uuid::now_v7());
    match crate::form::create_system_form(operator, workspace_path, &created).await {
        Ok(()) => Ok(created),
        Err(error) if is_registry_conflict(&error) => {
            // Another opener may have created the canonical registry after the
            // first read. Re-read and accept it only if its marker and schema
            // are exact; a name collision remains fail-closed.
            let workspace =
                crate::iceberg_store::native_workspace(operator, workspace_path).await?;
            let existing = workspace.list_forms().await?.into_iter().find(|form| {
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
        Err(error) => Err(error),
    }
}

fn is_registry_conflict(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<AppError>()
            .is_some_and(|app_error| app_error.code() == ErrorCode::CompositionRegistryConflict)
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
