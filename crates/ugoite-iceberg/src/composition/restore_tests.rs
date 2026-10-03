use super::*;
use crate::service::UgoiteService;
use crate::{iceberg_store, publication_context_for_change};
use serde_json::json;
use ugoite_domain::change::ChangeCommand;

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
