use std::collections::BTreeMap;
use ugoite_domain::entry::{EntryMetadata, EntryOperation, EntryRevision, FieldValue};
use ugoite_domain::form::{FieldType, FormDefinition, FormField, FormVersion};
use ugoite_domain::id::{FieldId, FormId, SpaceId};
use ugoite_iceberg::{
    publication_context, CheckpointIntegrityError, CheckpointUnavailable, IcebergWorkspace,
    RevisionView,
};
use uuid::Uuid;

fn form() -> FormDefinition {
    FormDefinition {
        id: FormId::from(Uuid::from_u128(2)),
        version: FormVersion::new(1).expect("test Form version"),
        name: "Task".into(),
        description: None,
        fields: vec![FormField {
            id: FieldId::new(100).expect("test field ID"),
            name: "title".into(),
            field_type: FieldType::String,
            required: false,
            label: None,
            description: None,
            semantic_role: None,
            reference_form: None,
            validation: None,
            enum_values: Vec::new(),
            deprecated: false,
        }],
        allow_extra_attributes: false,
        extension_metadata: BTreeMap::new(),
    }
}

fn revision(form: &FormDefinition, version: u64, title: &str) -> EntryRevision {
    let mut values = BTreeMap::new();
    values.insert(
        FieldId::new(100).expect("test field ID"),
        FieldValue::String(title.into()),
    );
    EntryRevision {
        form_id: form.id,
        entry_id: Uuid::from_u128(11).into(),
        revision_id: Uuid::from_u128(20 + u128::from(version)).into(),
        parent_revision_id: (version > 1)
            .then(|| Uuid::from_u128(20 + u128::from(version - 1)).into()),
        entry_version: version,
        expected_version: (version > 1).then_some(version - 1),
        operation: EntryOperation::Upsert,
        committed_at_micros: i64::try_from(version).expect("test version fits i64"),
        author_id: "human:owner".into(),
        form_version: form.version,
        source_kind: "test".into(),
        source_id: None,
        entry: EntryMetadata::default(),
        values,
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    }
}

async fn create_form(workspace: &IcebergWorkspace, form: &FormDefinition) -> anyhow::Result<()> {
    workspace
        .commit(publication_context(
            "checkpoint-form",
            "test.form.create",
            form,
        )?)?
        .create_form(form)
        .await
}

async fn append(
    workspace: &IcebergWorkspace,
    form: &FormDefinition,
    revision: EntryRevision,
) -> anyhow::Result<()> {
    workspace
        .commit(publication_context(
            format!("checkpoint-revision-{}", revision.entry_version),
            "test.entry.append",
            &revision,
        )?)?
        .append_revisions(form.id, vec![revision])
        .await?;
    Ok(())
}

#[tokio::test]
async fn checkpoint_pins_one_head_and_uses_static_iceberg_coordinates() -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(1)),
        "memory://checkpoint-pinned-head",
    )
    .await?;
    let form = form();
    create_form(&workspace, &form).await?;
    let first = revision(&form, 1, "before checkpoint");
    append(&workspace, &form, first.clone()).await?;

    let checkpoint = workspace.capture_checkpoint().await?;
    let repeated_capture = workspace.capture_checkpoint().await?;
    assert_eq!(
        checkpoint.coordinate_checksum, repeated_capture.coordinate_checksum,
        "capturing the same Head twice must retain one coordinate identity"
    );
    assert_eq!(checkpoint.tables.len(), 1);
    assert_eq!(checkpoint.tables[0].form_id, form.id);
    assert!(checkpoint.tables[0].snapshot_id.is_some());
    assert!(checkpoint.validate_coordinate_checksum());

    workspace
        .save_checkpoint("before-update", &checkpoint)
        .await?;
    let stored = workspace.load_checkpoint("before-update").await?;
    assert_eq!(stored.name.as_deref(), Some("before-update"));
    assert_eq!(stored.coordinate_checksum, checkpoint.coordinate_checksum);

    let second = revision(&form, 2, "after checkpoint");
    append(&workspace, &form, second.clone()).await?;

    assert_eq!(
        workspace
            .read_revision_view_at_checkpoint(
                &stored,
                form.id,
                RevisionView::LatestIncludingTombstones,
            )
            .await?,
        vec![first]
    );
    assert_eq!(
        workspace
            .read_revision_view(form.id, RevisionView::LatestIncludingTombstones)
            .await?,
        vec![second]
    );
    Ok(())
}

#[tokio::test]
async fn named_checkpoints_are_immutable() -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(4)),
        "memory://checkpoint-immutable-name",
    )
    .await?;
    let form = form();
    create_form(&workspace, &form).await?;
    append(&workspace, &form, revision(&form, 1, "first")).await?;
    let first = workspace.capture_checkpoint().await?;
    workspace.save_checkpoint("fixed-name", &first).await?;

    workspace
        .save_checkpoint("fixed-name", &first)
        .await
        .expect_err("reusing a checkpoint name must fail");

    append(&workspace, &form, revision(&form, 2, "second")).await?;
    let second = workspace.capture_checkpoint().await?;
    workspace
        .save_checkpoint("fixed-name", &second)
        .await
        .expect_err("a different checkpoint must not replace a named checkpoint");

    let stored = workspace.load_checkpoint("fixed-name").await?;
    assert_eq!(stored.coordinate_checksum, first.coordinate_checksum);
    Ok(())
}

#[tokio::test]
async fn checkpoint_reports_missing_and_tampered_coordinates_explicitly() -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(3)),
        "memory://checkpoint-errors",
    )
    .await?;
    let missing = workspace
        .load_checkpoint("not-saved")
        .await
        .expect_err("missing durable checkpoint must not fall back to Head");
    assert!(missing.downcast_ref::<CheckpointUnavailable>().is_some());

    let form = form();
    create_form(&workspace, &form).await?;
    append(&workspace, &form, revision(&form, 1, "immutable")).await?;
    let mut checkpoint = workspace.capture_checkpoint().await?;
    checkpoint.coordinate_checksum = "tampered".into();
    let error = workspace
        .read_revision_view_at_checkpoint(&checkpoint, form.id, RevisionView::Current)
        .await
        .expect_err("tampered coordinate checksum must fail before query planning");
    assert!(error.downcast_ref::<CheckpointIntegrityError>().is_some());

    let mut rewritten = workspace.capture_checkpoint().await?;
    rewritten.catalog_head_checksum = "attacker-recomputed-checksum".into();
    rewritten.coordinate_checksum = rewritten.computed_coordinate_checksum();
    let error = workspace
        .read_revision_view_at_checkpoint(&rewritten, form.id, RevisionView::Current)
        .await
        .expect_err("a self-consistent checkpoint must still match immutable publication evidence");
    assert!(error.downcast_ref::<CheckpointIntegrityError>().is_some());

    let mut duplicate_form = workspace.capture_checkpoint().await?;
    duplicate_form.tables.push(duplicate_form.tables[0].clone());
    duplicate_form.coordinate_checksum = duplicate_form.computed_coordinate_checksum();
    let error = workspace
        .read_revision_view_at_checkpoint(&duplicate_form, form.id, RevisionView::Current)
        .await
        .expect_err("ambiguous Form coordinates must fail closed");
    assert!(error.downcast_ref::<CheckpointIntegrityError>().is_some());
    Ok(())
}
