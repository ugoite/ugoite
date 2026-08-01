use iceberg::{NamespaceIdent, TableIdent};
use std::collections::BTreeMap;
use ugoite_domain::entry::{EntryMetadata, EntryOperation, EntryRevision, FieldValue};
use ugoite_domain::form::{sql_relation_name, FieldType, FormDefinition, FormField, FormVersion};
use ugoite_domain::id::{FieldId, FormId, SpaceId};
use ugoite_iceberg::{
    publication_context, CheckpointIntegrityError, CheckpointUnavailable, IcebergWorkspace,
    RevisionView,
};
use ugoite_storage::operator_from_uri;
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
            list_item: None,
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

async fn checkpoint_with_one_revision(
    warehouse: &str,
    space: u128,
) -> anyhow::Result<(
    IcebergWorkspace,
    FormDefinition,
    ugoite_iceberg::SpaceCheckpoint,
)> {
    let workspace =
        IcebergWorkspace::memory_for_tests(SpaceId::from(Uuid::from_u128(space)), warehouse)
            .await?;
    let form = form();
    create_form(&workspace, &form).await?;
    append(&workspace, &form, revision(&form, 1, "immutable")).await?;
    let checkpoint = workspace.capture_checkpoint().await?;
    Ok((workspace, form, checkpoint))
}

fn object_path(location: &str) -> &str {
    location
        .strip_prefix("memory:///")
        .or_else(|| location.strip_prefix("memory:/"))
        .unwrap_or(location)
}

async fn assert_checkpoint_unavailable_after_delete(
    warehouse: &str,
    space: u128,
    target: impl FnOnce(
        &IcebergWorkspace,
        &FormDefinition,
        &ugoite_iceberg::SpaceCheckpoint,
    ) -> anyhow::Result<String>,
) -> anyhow::Result<()> {
    let (workspace, form, checkpoint) = checkpoint_with_one_revision(warehouse, space).await?;
    let path = target(&workspace, &form, &checkpoint)?;
    operator_from_uri(warehouse)?
        .delete(object_path(&path))
        .await?;
    let error = workspace
        .read_revision_view_at_checkpoint(&checkpoint, form.id, RevisionView::Current)
        .await
        .expect_err("a missing immutable checkpoint target must be explicit");
    assert!(error.downcast_ref::<CheckpointUnavailable>().is_some());
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
    assert_eq!(
        workspace
            .form_at_checkpoint(&checkpoint, &sql_relation_name(form.id))
            .await?
            .id,
        form.id
    );

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

#[tokio::test]
async fn checkpoint_rejects_tampered_snapshot_and_schema_before_persistence_or_load(
) -> anyhow::Result<()> {
    let warehouse = "memory://checkpoint-coordinate-validation";
    let (workspace, _, checkpoint) = checkpoint_with_one_revision(warehouse, 29).await?;

    for (name, mutate) in [
        (
            "snapshot",
            (|checkpoint: &mut ugoite_iceberg::SpaceCheckpoint| {
                checkpoint.tables[0].snapshot_id =
                    checkpoint.tables[0].snapshot_id.map(|id| id + 1);
            }) as fn(&mut ugoite_iceberg::SpaceCheckpoint),
        ),
        (
            "schema",
            (|checkpoint: &mut ugoite_iceberg::SpaceCheckpoint| {
                checkpoint.tables[0].schema_id += 1;
            }) as fn(&mut ugoite_iceberg::SpaceCheckpoint),
        ),
    ] {
        let mut tampered = checkpoint.clone();
        mutate(&mut tampered);
        tampered.coordinate_checksum = tampered.computed_coordinate_checksum();
        let error = workspace
            .save_checkpoint(&format!("tampered-{name}"), &tampered)
            .await
            .expect_err("tampered table coordinates must not become durable");
        assert!(error.downcast_ref::<CheckpointIntegrityError>().is_some());

        let load_name = format!("raw-tampered-{name}");
        tampered.name = Some(load_name.clone());
        let space_root = format!("test/space_{}", checkpoint.space_id.as_uuid().simple());
        operator_from_uri(warehouse)?
            .write(
                &format!("{space_root}/_ugoite/checkpoints/{load_name}.json"),
                serde_json::to_vec(&tampered)?,
            )
            .await?;
        let error = workspace
            .load_checkpoint(&load_name)
            .await
            .expect_err("tampered durable coordinates must fail while loading");
        assert!(error.downcast_ref::<CheckpointIntegrityError>().is_some());
    }
    Ok(())
}

#[tokio::test]
async fn checkpoint_missing_immutable_targets_are_unavailable() -> anyhow::Result<()> {
    assert_checkpoint_unavailable_after_delete(
        "memory://checkpoint-missing-publication",
        30,
        |_, _, checkpoint| Ok(checkpoint.publication_location.clone()),
    )
    .await?;
    assert_checkpoint_unavailable_after_delete(
        "memory://checkpoint-missing-metadata",
        31,
        |_, _, checkpoint| Ok(checkpoint.tables[0].metadata_location.clone()),
    )
    .await?;

    let warehouse = "memory://checkpoint-missing-named";
    let (workspace, _, checkpoint) = checkpoint_with_one_revision(warehouse, 32).await?;
    workspace.save_checkpoint("removed", &checkpoint).await?;
    let space_root = format!("test/space_{}", checkpoint.space_id.as_uuid().simple());
    operator_from_uri(warehouse)?
        .delete(&format!("{space_root}/_ugoite/checkpoints/removed.json"))
        .await?;
    let error = workspace
        .load_checkpoint("removed")
        .await
        .expect_err("a deleted named checkpoint must be unavailable");
    assert!(error.downcast_ref::<CheckpointUnavailable>().is_some());
    Ok(())
}

async fn manifest_and_data_locations(
    workspace: &IcebergWorkspace,
    checkpoint: &ugoite_iceberg::SpaceCheckpoint,
) -> anyhow::Result<(String, String, String)> {
    let coordinate = &checkpoint.tables[0];
    let identifier = TableIdent::new(
        NamespaceIdent::from_vec(coordinate.namespace.clone())?,
        coordinate.table.clone(),
    );
    let table = workspace
        .catalog_for_testing()
        .load_table(&identifier)
        .await?;
    let snapshot = table
        .metadata()
        .current_snapshot()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("test table has no snapshot"))?;
    let manifest_list = snapshot.manifest_list().to_owned();
    let manifests = table.manifest_list_reader(&snapshot).load().await?;
    let manifest = manifests
        .entries()
        .first()
        .ok_or_else(|| anyhow::anyhow!("test snapshot has no manifest"))?;
    let manifest_path = manifest.manifest_path.clone();
    let loaded_manifest = manifest.load_manifest(table.file_io()).await?;
    let data = loaded_manifest
        .entries()
        .first()
        .ok_or_else(|| anyhow::anyhow!("test manifest has no data file"))?
        .data_file()
        .file_path()
        .to_owned();
    Ok((manifest_list, manifest_path, data))
}

#[tokio::test]
async fn checkpoint_missing_manifest_list_or_data_file_is_unavailable() -> anyhow::Result<()> {
    let warehouse = "memory://checkpoint-missing-manifest-list";
    let (workspace, form, checkpoint) = checkpoint_with_one_revision(warehouse, 33).await?;
    let (manifest_list, _, _) = manifest_and_data_locations(&workspace, &checkpoint).await?;
    operator_from_uri(warehouse)?
        .delete(object_path(&manifest_list))
        .await?;
    let error = workspace
        .read_revision_view_at_checkpoint(&checkpoint, form.id, RevisionView::Current)
        .await
        .expect_err("a deleted manifest list must be unavailable");
    assert!(error.downcast_ref::<CheckpointUnavailable>().is_some());

    let warehouse = "memory://checkpoint-missing-manifest";
    let (workspace, form, checkpoint) = checkpoint_with_one_revision(warehouse, 34).await?;
    let (_, manifest, _) = manifest_and_data_locations(&workspace, &checkpoint).await?;
    operator_from_uri(warehouse)?
        .delete(object_path(&manifest))
        .await?;
    let error = workspace
        .read_revision_view_at_checkpoint(&checkpoint, form.id, RevisionView::Current)
        .await
        .expect_err("a deleted manifest must be unavailable");
    assert!(error.downcast_ref::<CheckpointUnavailable>().is_some());

    let warehouse = "memory://checkpoint-missing-data";
    let (workspace, form, checkpoint) = checkpoint_with_one_revision(warehouse, 35).await?;
    let (_, _, data) = manifest_and_data_locations(&workspace, &checkpoint).await?;
    operator_from_uri(warehouse)?
        .delete(object_path(&data))
        .await?;
    let error = workspace
        .read_revision_view_at_checkpoint(&checkpoint, form.id, RevisionView::Current)
        .await
        .expect_err("a deleted data file must be unavailable");
    assert!(error.downcast_ref::<CheckpointUnavailable>().is_some());
    Ok(())
}
