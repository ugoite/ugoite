use crate::composition;
use crate::entry;
use crate::integrity::FakeIntegrityProvider;
use crate::service::UgoiteService;
use crate::{iceberg_store, publication_context_for_change};
use serde_json::json;
use std::collections::BTreeMap;
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_domain::change::ChangeCommand;
use ugoite_domain::composition::parse_composition_yaml;
use ugoite_domain::entry::{EntryMetadata, EntryOperation, EntryRevision, FieldValue};
use ugoite_domain::id::{EntryId, FieldId, RevisionId};

const COMPOSITION_YAML: &str =
    include_str!("../../../ugoite-domain/tests/fixtures/composition/monthly-expense.ugcomp.yaml");

fn assert_registry_conflict(error: &anyhow::Error) {
    assert!(error.to_string().contains("composition_registry_conflict"));
    assert!(error.chain().any(|cause| {
        cause
            .downcast_ref::<AppError>()
            .is_some_and(|app_error| app_error.code() == ErrorCode::CompositionRegistryConflict)
    }));
}

#[tokio::test]
async fn generic_entry_mutations_cannot_write_composition_registry() -> anyhow::Result<()> {
    let service = UgoiteService::new(format!(
        "memory://composition-generic-write-{}",
        uuid::Uuid::now_v7()
    ))?;
    let owner_id = uuid::Uuid::now_v7();
    let space_id = service
        .create_space_for_principal("composition-generic-write", owner_id, "Owner")
        .await?
        .to_string();
    let workspace_path = service.workspace_path(&space_id);
    let registry =
        composition::ensure_composition_registry(service.operator(), &workspace_path).await?;
    let entry_id = uuid::Uuid::now_v7();
    let revision_id = uuid::Uuid::now_v7();
    let author = owner_id.to_string();
    let committed_at_micros = 1_800_000_000_000_000;
    let change_id = uuid::Uuid::now_v7().to_string();
    let revision = EntryRevision {
        form_id: registry.id,
        entry_id: EntryId::from(entry_id),
        revision_id: RevisionId::from(revision_id),
        change_id: change_id.clone(),
        parent_revision_id: None,
        entry_version: 1,
        expected_version: None,
        operation: EntryOperation::Upsert,
        committed_at_micros,
        author_id: author.clone(),
        form_version: registry.version,
        source_kind: "test".into(),
        source_id: None,
        entry: EntryMetadata {
            external_id: entry_id.to_string(),
            created_at_micros: committed_at_micros,
            updated_at_micros: committed_at_micros,
            updated_by: author.clone(),
            ..EntryMetadata::default()
        },
        values: BTreeMap::from([
            (
                FieldId::new(100)?,
                FieldValue::String("Monthly expenses".into()),
            ),
            (FieldId::new(101)?, FieldValue::String("dashboard".into())),
            (FieldId::new(102)?, FieldValue::Integer(1)),
            (
                FieldId::new(103)?,
                FieldValue::String(COMPOSITION_YAML.into()),
            ),
        ]),
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    };
    let change = ChangeCommand {
        change_id,
        run_id: None,
        actor_principal_id: author.clone(),
        message: Some("seed a valid Composition revision for guard coverage".into()),
        reverts_change_id: None,
        created_at_micros: committed_at_micros,
    };
    assert!(ugoite_domain::composition::parse_composition_yaml(COMPOSITION_YAML).is_ok());
    let workspace = iceberg_store::native_workspace(service.operator(), &workspace_path).await?;
    workspace
        .commit(publication_context_for_change(
            &change,
            "test.composition.seed",
            &revision,
        )?)?
        .append_composition_revisions_authorized(registry.id, vec![revision.clone()], None)
        .await?;

    let generic_append_error = workspace
        .commit(publication_context_for_change(
            &change,
            "test.composition.seed",
            &revision,
        )?)?
        .append_revisions(registry.id, vec![revision])
        .await
        .unwrap_err();
    assert_registry_conflict(&generic_append_error);

    let fields = BTreeMap::from([
        ("name".to_string(), json!("Monthly expenses")),
        ("kind".to_string(), json!("dashboard")),
        ("format_version".to_string(), json!(1)),
        ("spec".to_string(), json!(COMPOSITION_YAML)),
    ]);
    let extra_attributes = BTreeMap::new();
    let new_entry_id = uuid::Uuid::now_v7().to_string();

    let error = service
        .create_structured_entry_with_optional_id_and_receipt(
            &space_id,
            Some(&new_entry_id),
            composition::COMPOSITION_REGISTRY_FORM_NAME.into(),
            Vec::new(),
            fields.clone(),
            extra_attributes.clone(),
            &author,
        )
        .await
        .unwrap_err();
    assert_registry_conflict(&error);

    let principals = [owner_id];
    let error = service
        .create_structured_entry_authorized_for_principals_with_optional_id(
            &space_id,
            Some(&uuid::Uuid::now_v7().to_string()),
            composition::COMPOSITION_REGISTRY_FORM_NAME.into(),
            Vec::new(),
            fields.clone(),
            extra_attributes.clone(),
            &author,
            &principals,
        )
        .await
        .unwrap_err();
    assert_registry_conflict(&error);

    let error = entry::create_draft_entries_with_scopes_and_change(
        service.operator(),
        &workspace_path,
        vec![entry::EntryDraftRequest {
            entry_id: uuid::Uuid::now_v7().to_string(),
            draft: ugoite_core::entry::StructuredEntryDraft {
                form_name: Some(composition::COMPOSITION_REGISTRY_FORM_NAME.into()),
                tags: Vec::new(),
                fields: fields.clone(),
                extra_attributes: extra_attributes.clone(),
            },
        }],
        &author,
        &FakeIntegrityProvider,
        None,
        None,
    )
    .await
    .unwrap_err();
    assert_registry_conflict(&error);

    let error = entry::append_revision_batch_for_form(
        service.operator(),
        &workspace_path,
        composition::COMPOSITION_REGISTRY_FORM_NAME,
        &[],
    )
    .await
    .unwrap_err();
    assert_registry_conflict(&error);

    let error = service
        .update_structured_entry(
            &space_id,
            &entry_id.to_string(),
            None,
            fields.clone(),
            extra_attributes.clone(),
            None,
            &author,
        )
        .await
        .unwrap_err();
    assert_registry_conflict(&error);

    let error = service
        .update_structured_entry_authorized_for_principals(
            &space_id,
            &entry_id.to_string(),
            None,
            None,
            fields.clone(),
            extra_attributes.clone(),
            None,
            &author,
            &principals,
        )
        .await
        .unwrap_err();
    assert_registry_conflict(&error);

    let error = service
        .delete_entry_with_receipt(&space_id, &entry_id.to_string(), &author)
        .await
        .unwrap_err();
    assert_registry_conflict(&error);

    let error = service
        .delete_entry_authorized_for_principals_with_change_receipt(
            &space_id,
            &entry_id.to_string(),
            &author,
            &principals,
            None,
        )
        .await
        .unwrap_err();
    assert_registry_conflict(&error);

    let error = service
        .restore_entry(
            &space_id,
            &entry_id.to_string(),
            &revision_id.to_string(),
            &author,
        )
        .await
        .unwrap_err();
    assert_registry_conflict(&error);

    let error = service
        .restore_entry_authorized_for_principals(
            &space_id,
            &entry_id.to_string(),
            &revision_id.to_string(),
            &author,
            &principals,
        )
        .await
        .unwrap_err();
    assert_registry_conflict(&error);

    service
        .create_pin(
            &space_id,
            "before-generic-restore",
            &author,
            &uuid::Uuid::now_v7().to_string(),
        )
        .await?;
    let error = service
        .restore_entry_from_pin_authorized_for_principals(
            &space_id,
            &entry_id.to_string(),
            &revision_id.to_string(),
            "before-generic-restore",
            &author,
            &principals,
        )
        .await
        .unwrap_err();
    assert_registry_conflict(&error);

    let current = service
        .get_entry_authorized_for_principals(&space_id, &entry_id.to_string(), &principals)
        .await?;
    assert_eq!(current["revision_id"], revision_id.to_string());
    let history = service
        .entry_history(&space_id, &entry_id.to_string())
        .await?;
    assert_eq!(history["revisions"].as_array().map(Vec::len), Some(1));

    service
        .upsert_form(
            &space_id,
            &json!({"name": "Ordinary", "fields": {"label": {"type": "string"}}}),
        )
        .await?;
    let ordinary = service
        .create_structured_entry_with_receipt(
            &space_id,
            &uuid::Uuid::now_v7().to_string(),
            "Ordinary".into(),
            Vec::new(),
            BTreeMap::from([("label".to_string(), json!("still writable"))]),
            BTreeMap::new(),
            &author,
        )
        .await?;
    assert_eq!(ordinary.0["form"], "Ordinary");

    Ok(())
}

#[tokio::test]
async fn revert_change_cannot_write_composition_registry() -> anyhow::Result<()> {
    let service = UgoiteService::new(format!(
        "memory://composition-revert-write-{}",
        uuid::Uuid::now_v7()
    ))?;
    let owner_id = uuid::Uuid::now_v7();
    let space_id = service
        .create_space_for_principal("composition-revert-write", owner_id, "Owner")
        .await?
        .to_string();
    let author = owner_id.to_string();
    let document = parse_composition_yaml(COMPOSITION_YAML)
        .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;
    let saved = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document,
            },
            &author,
            &[owner_id],
        )
        .await?;

    let error = service
        .revert_change(
            &space_id,
            &saved.receipt.command_id,
            &author,
            None,
            Some("attempt to revert a Composition publication"),
        )
        .await
        .unwrap_err();
    assert_registry_conflict(&error);

    let entry_id = saved.entry_id.to_string();
    let current = service
        .get_composition_raw_local(&space_id, &entry_id)
        .await?;
    assert_eq!(current.revision.revision_id, saved.revision_id);
    assert_eq!(current.revision.change_id, saved.receipt.command_id);

    let history = service
        .composition_history_local_page(&space_id, &entry_id, 10, 0)
        .await?;
    assert_eq!(history.total, 1);
    assert_eq!(history.revisions.len(), 1);
    assert!(!history.has_more);
    assert_eq!(history.revisions[0].revision.revision_id, saved.revision_id);
    Ok(())
}
