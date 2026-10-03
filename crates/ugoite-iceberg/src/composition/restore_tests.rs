use super::*;
use crate::service::UgoiteService;
use crate::{iceberg_store, publication_context_for_change};
use serde_json::json;
use ugoite_domain::change::ChangeCommand;
use ugoite_domain::composition::parse_composition_yaml;

#[tokio::test]
async fn restore_rejects_an_unknown_format_revision_without_publishing() -> anyhow::Result<()> {
    let service = UgoiteService::new(format!(
        "memory://composition-restore-unknown-{}",
        Uuid::now_v7()
    ))?;
    let owner = Uuid::now_v7();
    let space_id = service
        .create_space_for_principal("composition-restore-unknown", owner, "Owner")
        .await?
        .to_string();
    let workspace_path = service.workspace_path(&space_id);
    let form = ensure_composition_registry(service.operator(), &workspace_path).await?;
    let entry_id = EntryId::from(Uuid::now_v7());
    let revision_id = RevisionId::from(Uuid::now_v7());
    let change_id = Uuid::now_v7().to_string();
    let author = owner.to_string();
    let timestamp = Utc::now().timestamp_micros();
    let field_id = |name: &str| {
        form.fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| field.id)
            .ok_or_else(|| anyhow::anyhow!("missing Composition field {name}"))
    };
    let revision = EntryRevision {
        form_id: form.id,
        entry_id,
        revision_id,
        parent_revision_id: None,
        entry_version: 1,
        change_id: change_id.clone(),
        expected_version: None,
        operation: EntryOperation::Upsert,
        committed_at_micros: timestamp,
        author_id: author.clone(),
        form_version: form.version,
        source_kind: "test".into(),
        source_id: None,
        entry: EntryMetadata {
            external_id: entry_id.to_string(),
            created_at_micros: timestamp,
            updated_at_micros: timestamp,
            updated_by: author.clone(),
            ..EntryMetadata::default()
        },
        values: BTreeMap::from([
            (field_id("name")?, FieldValue::String("Unknown".into())),
            (field_id("kind")?, FieldValue::String("dashboard".into())),
            (field_id("format_version")?, FieldValue::Integer(99)),
            (
                field_id("spec")?,
                FieldValue::String("not: [valid YAML".into()),
            ),
        ]),
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    };
    let change = ChangeCommand {
        change_id,
        run_id: None,
        actor_principal_id: author,
        message: Some("seed unknown Composition version".into()),
        reverts_change_id: None,
        created_at_micros: timestamp,
    };
    iceberg_store::native_workspace(service.operator(), &workspace_path)
        .await?
        .commit(publication_context_for_change(
            &change,
            "test.composition.restore-unknown-source",
            &revision,
        )?)?
        .append_composition_revisions_authorized(form.id, vec![revision], None)
        .await?;

    let error = service
        .restore_composition_local(
            &space_id,
            &entry_id.to_string(),
            &revision_id.to_string(),
            &owner.to_string(),
        )
        .await
        .expect_err("unknown versions remain inspectable but cannot be restored");
    assert!(error.to_string().contains("composition_registry_conflict"));
    let history = service
        .composition_history_local_page(
            &space_id,
            &entry_id.to_string(),
            COMPOSITION_HISTORY_MAX_PAGE_SIZE,
            0,
        )
        .await?;
    assert_eq!(history.total, 1);
    assert_eq!(history.revisions[0].revision.revision_id, revision_id);
    assert_eq!(
        history.revisions[0].fields.get("format_version"),
        Some(&json!(99))
    );
    Ok(())
}

#[tokio::test]
async fn composition_revision_admission_is_bound_to_the_validated_batch() -> anyhow::Result<()> {
    let service = UgoiteService::new(format!(
        "memory://composition-admission-bound-{}",
        Uuid::now_v7()
    ))?;
    let owner = Uuid::now_v7();
    let space_id = service
        .create_space_for_principal("composition-admission-bound", owner, "Owner")
        .await?
        .to_string();
    let document = ugoite_domain::composition::parse_composition_yaml(include_str!(
        "../../../ugoite-domain/tests/fixtures/composition/monthly-expense.ugcomp.yaml"
    ))
    .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;
    let saved = service
        .save_composition_authorized_for_principals(
            &space_id,
            CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document,
            },
            &owner.to_string(),
            &[owner],
        )
        .await?;
    let raw = service
        .get_composition_raw_local(&space_id, &saved.entry_id.to_string())
        .await?;
    let registry =
        ensure_composition_registry(service.operator(), &service.workspace_path(&space_id)).await?;
    let admitted = [raw.revision.clone()];
    let admission = crate::RevisionBatchAdmission::composition(&registry, &admitted)?;
    admission.recheck(registry.id, &admitted, &registry)?;

    let wrong_form = ugoite_domain::id::FormId::from(Uuid::now_v7());
    let error = admission
        .recheck(wrong_form, &admitted, &registry)
        .expect_err("a Composition admission cannot be reused for another Form");
    assert!(error.to_string().contains("composition_registry_conflict"));

    let mut changed = raw.revision;
    changed.committed_at_micros += 1;
    changed.entry.updated_at_micros += 1;
    changed.source_kind = "other-valid-provenance".into();
    let error = admission
        .recheck(registry.id, &[changed], &registry)
        .expect_err("a batch cannot be changed after Composition validation");
    assert!(error.to_string().contains("composition_registry_conflict"));
    Ok(())
}

#[tokio::test]
async fn restore_after_tombstone_appends_and_clears_deletion_metadata() -> anyhow::Result<()> {
    let service = UgoiteService::new(format!(
        "memory://composition-restore-tombstone-{}",
        Uuid::now_v7()
    ))?;
    let owner = Uuid::now_v7();
    let author = owner.to_string();
    let space_id = service
        .create_space_for_principal("composition-restore-tombstone", owner, "Owner")
        .await?
        .to_string();
    let document = parse_composition_yaml(include_str!(
        "../../../ugoite-domain/tests/fixtures/composition/monthly-expense.ugcomp.yaml"
    ))
    .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;
    let saved = service
        .save_composition_local(
            &space_id,
            CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document,
            },
            &author,
        )
        .await?;
    let workspace_path = service.workspace_path(&space_id);
    let registry = ensure_composition_registry(service.operator(), &workspace_path).await?;
    let current = service
        .get_composition_raw_local(&space_id, &saved.entry_id.to_string())
        .await?
        .revision;

    let committed_at_micros = Utc::now()
        .timestamp_micros()
        .max(current.entry.updated_at_micros.saturating_add(1));
    let mut tombstone_entry = current.entry.clone();
    tombstone_entry.updated_at_micros = committed_at_micros;
    tombstone_entry.updated_by = author.clone();
    tombstone_entry.deleted = true;
    tombstone_entry.deleted_at_micros = Some(committed_at_micros);
    tombstone_entry.deleted_by = Some(author.clone());
    let tombstone_change_id = Uuid::now_v7().to_string();
    let tombstone = EntryRevisionDraft {
        form_id: registry.id,
        entry_id: saved.entry_id,
        revision_id: RevisionId::from(Uuid::now_v7()),
        change_id: tombstone_change_id.clone(),
        operation: EntryOperation::Delete,
        committed_at_micros,
        author_id: author.clone(),
        form_version: registry.version,
        source_kind: "test".to_string(),
        source_id: None,
        entry: tombstone_entry,
        values: BTreeMap::new(),
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    }
    .build(&registry, Some(&current))?;
    let change = ChangeCommand {
        change_id: tombstone_change_id,
        run_id: None,
        actor_principal_id: author.clone(),
        message: Some("seed a Composition tombstone".to_string()),
        reverts_change_id: None,
        created_at_micros: committed_at_micros,
    };
    // Generic writes are intentionally barred from the reserved Registry;
    // seed the historical tombstone through the private test fixture path.
    iceberg_store::native_workspace(service.operator(), &workspace_path)
        .await?
        .commit(publication_context_for_change(
            &change,
            "test.composition.tombstone",
            &tombstone,
        )?)?
        .append_composition_revisions_authorized(registry.id, vec![tombstone.clone()], None)
        .await?;

    let tombstone_history = service
        .composition_history_local_page(
            &space_id,
            &saved.entry_id.to_string(),
            COMPOSITION_HISTORY_MAX_PAGE_SIZE,
            0,
        )
        .await?;
    assert_eq!(tombstone_history.total, 2);
    assert!(tombstone_history.revisions.iter().any(|revision| {
        revision.revision.revision_id == tombstone.revision_id
            && revision.revision.operation == EntryOperation::Delete
            && revision.revision.entry.deleted
    }));
    let no_current = service
        .get_composition_raw_local(&space_id, &saved.entry_id.to_string())
        .await
        .expect_err("a current tombstone should not appear as a readable Composition");
    assert_eq!(
        no_current.downcast_ref::<AppError>().unwrap().code(),
        ErrorCode::EntryNotFound
    );

    let restored = service
        .restore_composition_local(
            &space_id,
            &saved.entry_id.to_string(),
            &saved.revision_id.to_string(),
            &author,
        )
        .await?;
    assert_eq!(restored.restored_from_revision_id, saved.revision_id);
    assert_eq!(
        restored.receipt.committed_revision_ids,
        [restored.revision_id]
    );

    let latest = service
        .get_composition_raw_local(&space_id, &saved.entry_id.to_string())
        .await?;
    assert_eq!(latest.revision.revision_id, restored.revision_id);
    assert_eq!(
        latest.revision.parent_revision_id,
        Some(tombstone.revision_id)
    );
    assert_eq!(latest.revision.operation, EntryOperation::Restore);
    assert_eq!(latest.revision.entry.restored_from, Some(saved.revision_id));
    assert_eq!(latest.revision.change_id, restored.receipt.command_id);
    assert!(!latest.revision.entry.deleted);
    assert_eq!(latest.revision.entry.deleted_at_micros, None);
    assert_eq!(latest.revision.entry.deleted_by, None);

    let history = service
        .composition_history_local_page(
            &space_id,
            &saved.entry_id.to_string(),
            COMPOSITION_HISTORY_MAX_PAGE_SIZE,
            0,
        )
        .await?;
    assert_eq!(history.total, 3);
    assert!(history.revisions.iter().any(|revision| {
        revision.revision.revision_id == tombstone.revision_id
            && revision.revision.operation == EntryOperation::Delete
    }));
    assert!(history
        .revisions
        .iter()
        .any(|revision| revision.revision.revision_id == saved.revision_id));
    assert_eq!(
        history
            .revisions
            .last()
            .map(|revision| revision.revision.revision_id),
        Some(restored.revision_id)
    );
    Ok(())
}
