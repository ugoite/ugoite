use std::collections::BTreeMap;
use ugoite_domain::entry::{
    EntryAsset, EntryIntegrity, EntryLink, EntryMetadata, EntryOperation, EntryRevision, FieldValue,
};
use ugoite_domain::form::{
    FieldType, FormChange, FormChangeSet, FormDefinition, FormField, FormVersion,
};
use ugoite_domain::id::{FieldId, FormId, SpaceId};
use ugoite_iceberg::{
    physical_form_name, publication_context, IcebergWorkspace, MigrationFormReport,
    MigrationManifest, MigrationReport, RevisionView,
};
use uuid::Uuid;

fn form() -> FormDefinition {
    FormDefinition {
        id: FormId::from(Uuid::from_u128(2)),
        version: FormVersion::new(1).unwrap(),
        name: "Task".into(),
        description: None,
        fields: vec![FormField {
            id: FieldId::new(100).unwrap(),
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

async fn create_form(workspace: &IcebergWorkspace, form: &FormDefinition) -> anyhow::Result<()> {
    workspace
        .commit(publication_context(
            Uuid::new_v4().to_string(),
            "test.form.create",
            form,
        )?)?
        .create_form(form)
        .await
}

async fn evolve_form(
    workspace: &IcebergWorkspace,
    changes: &FormChangeSet,
) -> anyhow::Result<FormDefinition> {
    workspace
        .commit(publication_context(
            Uuid::new_v4().to_string(),
            "test.form.evolve",
            changes,
        )?)?
        .evolve_form(changes)
        .await
}

async fn append_revisions(
    workspace: &IcebergWorkspace,
    form_id: FormId,
    revisions: Vec<EntryRevision>,
) -> anyhow::Result<ugoite_iceberg::CommitReceipt> {
    let command = publication_context(Uuid::new_v4().to_string(), "test.entry.append", &revisions)?;
    workspace
        .commit(command)?
        .append_revisions(form_id, revisions)
        .await
}

#[tokio::test]
async fn one_stable_form_id_maps_to_one_catalog_table() -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(1)),
        "memory://iceberg-native-workspace",
    )
    .await?;
    let form = form();
    create_form(&workspace, &form).await?;
    assert_eq!(workspace.list_forms().await?, vec![form.clone()]);
    assert_eq!(workspace.load_form(form.id).await?, form);
    let table = workspace
        .catalog_for_testing()
        .load_table(&iceberg::TableIdent::new(
            workspace.namespace_for_testing().clone(),
            physical_form_name(form.id),
        ))
        .await?;
    let arrow_schema = ugoite_iceberg::arrow_schema_for_form(&form)?;
    for field in &form.fields {
        let physical = arrow_schema.field_with_name(&field.name)?;
        assert_eq!(
            physical.metadata().get("PARQUET:field_id"),
            Some(&field.id.get().to_string())
        );
    }
    let title = table
        .metadata()
        .current_schema()
        .field_by_name("title")
        .expect("created Form field must exist in the Iceberg schema");
    assert_eq!(title.id, form.fields[0].id.get());
    assert!(
        !table
            .metadata()
            .properties()
            .contains_key("ugoite.form.field-id-map.v1"),
        "Iceberg field IDs replace the legacy Ugoite field-ID map"
    );
    assert_eq!(
        physical_form_name(form.id),
        "form_00000000000000000000000000000002"
    );
    assert!(table.metadata().location().ends_with(
        "/space_00000000000000000000000000000001/form_00000000000000000000000000000002"
    ));
    assert_eq!(
        workspace
            .catalog_for_testing()
            .list_tables(workspace.namespace_for_testing())
            .await?
            .len(),
        1
    );
    let sql = format!(
        "SELECT entry_id FROM ugoite.{}.{} LIMIT 1",
        workspace.namespace_for_testing().as_ref()[0],
        physical_form_name(form.id)
    );
    assert!(workspace
        .query(&sql)
        .await?
        .iter()
        .all(|batch| batch.num_rows() == 0));
    Ok(())
}

#[tokio::test]
async fn nested_fields_have_unique_iceberg_ids_across_form_columns() -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(40)),
        "memory://iceberg-nested-field-ids",
    )
    .await?;
    let mut form = form();
    form.fields[0].field_type = FieldType::List;
    form.fields.push(FormField {
        id: FieldId::new(101).unwrap(),
        name: "assignees".into(),
        field_type: FieldType::ObjectList,
        required: false,
        label: None,
        description: None,
        semantic_role: None,
        reference_form: None,
        validation: None,
        enum_values: Vec::new(),
        deprecated: false,
    });
    create_form(&workspace, &form).await?;
    let table = workspace
        .catalog_for_testing()
        .load_table(&iceberg::TableIdent::new(
            workspace.namespace_for_testing().clone(),
            physical_form_name(form.id),
        ))
        .await?;
    let title = table
        .metadata()
        .current_schema()
        .field_by_name("title")
        .unwrap();
    let assignees = table
        .metadata()
        .current_schema()
        .field_by_name("assignees")
        .unwrap();
    let iceberg::spec::Type::List(title_list) = title.field_type.as_ref() else {
        panic!("title must remain a list");
    };
    let iceberg::spec::Type::List(assignees_list) = assignees.field_type.as_ref() else {
        panic!("assignees must remain a list");
    };
    assert_ne!(title_list.element_field.id, assignees_list.element_field.id);
    Ok(())
}

#[tokio::test]
async fn native_form_types_are_preserved_in_iceberg_schema() -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(41)),
        "memory://iceberg-native-types",
    )
    .await?;
    let mut form = form();
    form.fields = [
        (100, "date", FieldType::Date),
        (101, "time", FieldType::Time),
        (102, "timestamp", FieldType::Timestamp),
        (103, "timestamp_tz", FieldType::TimestampTz),
        (104, "timestamp_ns", FieldType::TimestampNs),
        (105, "timestamp_tz_ns", FieldType::TimestampTzNs),
        (106, "uuid", FieldType::Uuid),
        (107, "binary", FieldType::Binary),
    ]
    .into_iter()
    .map(|(id, name, field_type)| FormField {
        id: FieldId::new(id).unwrap(),
        name: name.into(),
        field_type,
        required: false,
        label: None,
        description: None,
        semantic_role: None,
        reference_form: None,
        validation: None,
        enum_values: Vec::new(),
        deprecated: false,
    })
    .collect();
    create_form(&workspace, &form).await?;
    let table = workspace
        .catalog_for_testing()
        .load_table(&iceberg::TableIdent::new(
            workspace.namespace_for_testing().clone(),
            physical_form_name(form.id),
        ))
        .await?;
    let schema = table.metadata().current_schema();
    for (name, expected) in [
        ("date", iceberg::spec::PrimitiveType::Date),
        ("time", iceberg::spec::PrimitiveType::Time),
        ("timestamp", iceberg::spec::PrimitiveType::Timestamp),
        ("timestamp_tz", iceberg::spec::PrimitiveType::Timestamptz),
        ("timestamp_ns", iceberg::spec::PrimitiveType::TimestampNs),
        (
            "timestamp_tz_ns",
            iceberg::spec::PrimitiveType::TimestamptzNs,
        ),
        ("uuid", iceberg::spec::PrimitiveType::Uuid),
        ("binary", iceberg::spec::PrimitiveType::Binary),
    ] {
        assert_eq!(
            schema.field_by_name(name).unwrap().field_type.as_ref(),
            &iceberg::spec::Type::Primitive(expected)
        );
    }
    Ok(())
}

#[tokio::test]
async fn metadata_evolution_keeps_physical_identity() -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(3)),
        "memory://iceberg-form-evolution",
    )
    .await?;
    let form = form();
    create_form(&workspace, &form).await?;
    let evolved = evolve_form(
        &workspace,
        &FormChangeSet {
            form_id: form.id,
            expected_version: Some(form.version),
            changes: vec![FormChange::RenameForm {
                name: "Work item".into(),
            }],
        },
    )
    .await?;
    assert_eq!(evolved.id, form.id);
    assert_eq!(evolved.name, "Work item");
    assert_eq!(
        workspace
            .catalog_for_testing()
            .list_tables(workspace.namespace_for_testing())
            .await?
            .len(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn local_catalog_evolves_schema_bearing_changes() -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(4)),
        "memory://iceberg-schema-capability",
    )
    .await?;
    assert_eq!(
        workspace.schema_commit_capability(),
        ugoite_iceberg::SchemaCommitCapability::AtomicSchemaEvolution
    );
    let form = form();
    create_form(&workspace, &form).await?;
    let evolved = evolve_form(
        &workspace,
        &FormChangeSet {
            form_id: form.id,
            expected_version: Some(form.version),
            changes: vec![FormChange::AddField(FormField {
                id: FieldId::new(101).unwrap(),
                name: "due_at".into(),
                field_type: FieldType::Date,
                required: false,
                label: None,
                description: None,
                semantic_role: None,
                reference_form: None,
                validation: None,
                enum_values: Vec::new(),
                deprecated: false,
            })],
        },
    )
    .await?;
    assert!(evolved.fields.iter().any(|field| field.name == "due_at"));
    let table = workspace
        .catalog_for_testing()
        .load_table(&iceberg::TableIdent::new(
            workspace.namespace_for_testing().clone(),
            physical_form_name(form.id),
        ))
        .await?;
    assert_eq!(
        table
            .metadata()
            .current_schema()
            .field_by_name("due_at")
            .unwrap()
            .field_type
            .as_ref(),
        &iceberg::spec::Type::Primitive(iceberg::spec::PrimitiveType::Date),
    );
    Ok(())
}

#[tokio::test]
async fn append_enforces_revision_identity_and_entry_conflicts() -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(30)),
        "memory://iceberg-append-conflict",
    )
    .await?;
    let form = form();
    create_form(&workspace, &form).await?;
    let entry_id = Uuid::from_u128(31);
    let revision_id = Uuid::from_u128(32);
    let revision = EntryRevision {
        form_id: form.id,
        entry_id: entry_id.into(),
        revision_id: revision_id.into(),
        parent_revision_id: None,
        entry_version: 1,
        expected_version: None,
        operation: EntryOperation::Upsert,
        committed_at_micros: 1,
        author_id: "human:owner".into(),
        form_version: form.version,
        source_kind: "test".into(),
        source_id: None,
        entry: EntryMetadata::default(),
        values: BTreeMap::new(),
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    };
    let mut revision = revision;
    revision.values.insert(
        FieldId::new(100).unwrap(),
        FieldValue::String("title from revision".into()),
    );
    let receipt = append_revisions(&workspace, form.id, vec![revision.clone()]).await?;
    assert_eq!(receipt.committed_revision_ids, vec![revision.revision_id]);
    assert!(receipt.data_file_count > 0);
    assert_eq!(
        workspace
            .read_revision_view(form.id, RevisionView::LatestIncludingTombstones)
            .await?,
        vec![revision.clone()]
    );

    let conflicting = EntryRevision {
        revision_id: Uuid::from_u128(33).into(),
        ..revision
    };
    let error = append_revisions(&workspace, form.id, vec![conflicting])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("conflict"));
    Ok(())
}

#[tokio::test]
async fn revision_views_keep_tombstones_and_restore_current_entries() -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(60)),
        "memory://iceberg-revision-views",
    )
    .await?;
    let form = form();
    create_form(&workspace, &form).await?;

    let first = EntryRevision {
        form_id: form.id,
        entry_id: Uuid::from_u128(61).into(),
        revision_id: Uuid::from_u128(62).into(),
        parent_revision_id: None,
        entry_version: 1,
        expected_version: None,
        operation: EntryOperation::Upsert,
        committed_at_micros: 1,
        author_id: "human:owner".into(),
        form_version: form.version,
        source_kind: "test".into(),
        source_id: None,
        entry: EntryMetadata::default(),
        values: BTreeMap::new(),
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    };
    let first_receipt = append_revisions(&workspace, form.id, vec![first.clone()]).await?;

    let deleted = EntryRevision {
        revision_id: Uuid::from_u128(63).into(),
        parent_revision_id: Some(first.revision_id),
        entry_version: 2,
        expected_version: Some(1),
        operation: EntryOperation::Delete,
        committed_at_micros: 2,
        entry: EntryMetadata {
            deleted: true,
            deleted_at_micros: Some(2),
            ..EntryMetadata::default()
        },
        ..first.clone()
    };
    let deleted_receipt = append_revisions(&workspace, form.id, vec![deleted.clone()]).await?;
    assert_eq!(
        workspace
            .read_revision_view(form.id, RevisionView::LatestIncludingTombstones)
            .await?,
        vec![deleted.clone()]
    );
    assert!(workspace
        .read_revision_view(form.id, RevisionView::Current)
        .await?
        .is_empty());
    assert_eq!(
        workspace
            .read_latest_revisions_for_entry(form.id, first.entry_id)
            .await?,
        vec![deleted.clone()]
    );
    assert_eq!(
        workspace
            .read_revision_view_at_snapshot(
                form.id,
                RevisionView::LatestIncludingTombstones,
                first_receipt.snapshot_id,
            )
            .await?,
        vec![first.clone()]
    );
    assert!(
        workspace
            .read_revision_view_at_snapshot(
                form.id,
                RevisionView::Current,
                deleted_receipt.snapshot_id,
            )
            .await?
            .is_empty()
    );

    let restored = EntryRevision {
        revision_id: Uuid::from_u128(64).into(),
        parent_revision_id: Some(deleted.revision_id),
        entry_version: 3,
        expected_version: Some(2),
        operation: EntryOperation::Restore,
        committed_at_micros: 3,
        entry: EntryMetadata {
            restored_from: Some(deleted.revision_id),
            ..EntryMetadata::default()
        },
        ..first.clone()
    };
    append_revisions(&workspace, form.id, vec![restored.clone()]).await?;
    assert_eq!(
        workspace
            .read_revision_view(form.id, RevisionView::Current)
            .await?,
        vec![restored.clone()]
    );
    assert_eq!(
        workspace.read_revisions(form.id).await?,
        vec![first, deleted, restored]
    );
    Ok(())
}

#[tokio::test]
async fn coordinator_replays_only_the_same_canonical_command() -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(34)),
        "memory://iceberg-command-idempotency",
    )
    .await?;
    let form = form();
    create_form(&workspace, &form).await?;
    let revision = EntryRevision {
        form_id: form.id,
        entry_id: Uuid::from_u128(35).into(),
        revision_id: Uuid::from_u128(36).into(),
        parent_revision_id: None,
        entry_version: 1,
        expected_version: None,
        operation: EntryOperation::Upsert,
        committed_at_micros: 1,
        author_id: "human:owner".into(),
        form_version: form.version,
        source_kind: "test".into(),
        source_id: None,
        entry: EntryMetadata::default(),
        values: BTreeMap::from([(
            FieldId::new(100).unwrap(),
            FieldValue::String("exactly once".into()),
        )]),
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    };
    let command = publication_context("append-35", "test.entry.append", &vec![revision.clone()])?;
    let first = workspace
        .commit(command.clone())?
        .append_revisions(form.id, vec![revision.clone()])
        .await?;
    let replay = workspace
        .commit(command)?
        .append_revisions(form.id, vec![revision.clone()])
        .await?;
    assert_eq!(replay.command_id, "append-35");
    assert_eq!(replay.catalog_generation, first.catalog_generation);
    assert_eq!(replay.snapshot_id, first.snapshot_id);
    assert_eq!(
        workspace.read_revisions(form.id).await?,
        vec![revision.clone()]
    );

    let mut changed = revision;
    changed.revision_id = Uuid::from_u128(37).into();
    let reuse = workspace
        .commit(publication_context(
            "append-35",
            "test.entry.append",
            &vec![changed.clone()],
        )?)?
        .append_revisions(form.id, vec![changed])
        .await
        .unwrap_err();
    assert!(reuse.to_string().contains("reused"));
    Ok(())
}

#[tokio::test]
async fn one_explicit_form_batch_publishes_one_snapshot_and_receipt() -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(35)),
        "memory://iceberg-batched-append",
    )
    .await?;
    let form = form();
    create_form(&workspace, &form).await?;
    let revisions = [36_u128, 37]
        .into_iter()
        .map(|id| {
            let mut values = BTreeMap::new();
            values.insert(
                FieldId::new(100).unwrap(),
                FieldValue::String(format!("revision {id}")),
            );
            EntryRevision {
                form_id: form.id,
                entry_id: Uuid::from_u128(id).into(),
                revision_id: Uuid::from_u128(id + 100).into(),
                parent_revision_id: None,
                entry_version: 1,
                expected_version: None,
                operation: EntryOperation::Upsert,
                committed_at_micros: 1,
                author_id: "human:owner".into(),
                form_version: form.version,
                source_kind: "test".into(),
                source_id: None,
                entry: EntryMetadata::default(),
                values,
                extra_attributes: BTreeMap::new(),
                extension_metadata: BTreeMap::new(),
            }
        })
        .collect::<Vec<_>>();

    let receipt = append_revisions(&workspace, form.id, revisions.clone()).await?;
    assert_eq!(
        receipt.committed_revision_ids,
        revisions
            .iter()
            .map(|revision| revision.revision_id)
            .collect::<Vec<_>>()
    );
    assert!(receipt.data_file_count > 0);

    let table = workspace
        .catalog_for_testing()
        .load_table(&iceberg::TableIdent::new(
            workspace.namespace_for_testing().clone(),
            physical_form_name(form.id),
        ))
        .await?;
    assert_eq!(table.metadata().snapshots().len(), 1);
    assert_eq!(
        table.metadata().current_snapshot().unwrap().snapshot_id(),
        receipt.snapshot_id
    );
    Ok(())
}

#[tokio::test]
async fn form_rename_and_optional_addition_read_old_and_new_files_by_stable_id(
) -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(60)),
        "memory://iceberg-stable-ids-across-files",
    )
    .await?;
    let form = form();
    create_form(&workspace, &form).await?;
    let entry_id = Uuid::from_u128(61).into();
    let first = EntryRevision {
        form_id: form.id,
        entry_id,
        revision_id: Uuid::from_u128(62).into(),
        parent_revision_id: None,
        entry_version: 1,
        expected_version: None,
        operation: EntryOperation::Upsert,
        committed_at_micros: 1,
        author_id: "human:owner".into(),
        form_version: form.version,
        source_kind: "test".into(),
        source_id: None,
        entry: EntryMetadata {
            title: "before rename".into(),
            ..Default::default()
        },
        values: BTreeMap::from([(FieldId::new(100).unwrap(), FieldValue::String("old".into()))]),
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    };
    append_revisions(&workspace, form.id, vec![first.clone()]).await?;

    let evolved = evolve_form(
        &workspace,
        &FormChangeSet {
            form_id: form.id,
            expected_version: Some(form.version),
            changes: vec![
                FormChange::RenameField {
                    field_id: FieldId::new(100).unwrap(),
                    name: "summary".into(),
                },
                FormChange::AddField(FormField {
                    id: FieldId::new(101).unwrap(),
                    name: "status".into(),
                    field_type: FieldType::String,
                    required: false,
                    label: None,
                    description: None,
                    semantic_role: None,
                    reference_form: None,
                    validation: None,
                    enum_values: Vec::new(),
                    deprecated: false,
                }),
            ],
        },
    )
    .await?;
    let table = workspace
        .catalog_for_testing()
        .load_table(&iceberg::TableIdent::new(
            workspace.namespace_for_testing().clone(),
            physical_form_name(form.id),
        ))
        .await?;
    assert_eq!(
        table
            .metadata()
            .current_schema()
            .field_by_id(100)
            .map(|field| field.name.as_str()),
        Some("summary")
    );

    let second = EntryRevision {
        form_id: evolved.id,
        entry_id,
        revision_id: Uuid::from_u128(63).into(),
        parent_revision_id: Some(first.revision_id),
        entry_version: 2,
        expected_version: Some(1),
        operation: EntryOperation::Upsert,
        committed_at_micros: 2,
        author_id: "human:owner".into(),
        form_version: evolved.version,
        source_kind: "test".into(),
        source_id: None,
        entry: EntryMetadata {
            title: "after rename".into(),
            ..Default::default()
        },
        values: BTreeMap::from([
            (FieldId::new(100).unwrap(), FieldValue::String("new".into())),
            (
                FieldId::new(101).unwrap(),
                FieldValue::String("active".into()),
            ),
        ]),
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    };
    append_revisions(&workspace, form.id, vec![second.clone()]).await?;

    let revisions = workspace.read_revisions(form.id).await?;
    assert_eq!(revisions.len(), 2);
    assert_eq!(
        revisions[0].values.get(&FieldId::new(100).unwrap()),
        Some(&FieldValue::String("old".into()))
    );
    assert!(!revisions[0]
        .values
        .contains_key(&FieldId::new(101).unwrap()));
    assert_eq!(revisions[1], second);
    Ok(())
}

#[tokio::test]
async fn typed_forms_and_fixed_entry_metadata_round_trip_without_json_payloads(
) -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(70)),
        "memory://iceberg-typed-entry-round-trip",
    )
    .await?;
    let mut form = form();
    form.fields.extend([
        FormField {
            id: FieldId::new(101).unwrap(),
            name: "labels".into(),
            field_type: FieldType::List,
            required: false,
            label: None,
            description: None,
            semantic_role: None,
            reference_form: None,
            validation: None,
            enum_values: Vec::new(),
            deprecated: false,
        },
        FormField {
            id: FieldId::new(102).unwrap(),
            name: "references".into(),
            field_type: FieldType::ObjectList,
            required: false,
            label: None,
            description: None,
            semantic_role: None,
            reference_form: None,
            validation: None,
            enum_values: Vec::new(),
            deprecated: false,
        },
        FormField {
            id: FieldId::new(103).unwrap(),
            name: "related_entry".into(),
            field_type: FieldType::RowReference,
            required: false,
            label: None,
            description: None,
            semantic_role: None,
            reference_form: None,
            validation: None,
            enum_values: Vec::new(),
            deprecated: false,
        },
    ]);
    create_form(&workspace, &form).await?;
    let revision = EntryRevision {
        form_id: form.id,
        entry_id: Uuid::from_u128(71).into(),
        revision_id: Uuid::from_u128(72).into(),
        parent_revision_id: None,
        entry_version: 1,
        expected_version: None,
        operation: EntryOperation::Upsert,
        committed_at_micros: 1,
        author_id: "human:owner".into(),
        form_version: form.version,
        source_kind: "test".into(),
        source_id: Some("import-1".into()),
        entry: EntryMetadata {
            external_id: "task-71".into(),
            title: "typed metadata".into(),
            tags: vec!["important".into(), "today".into()],
            links: vec![EntryLink {
                id: "link-1".into(),
                target: "https://example.com".into(),
                kind: "reference".into(),
            }],
            created_at_micros: 10,
            updated_at_micros: 11,
            assets: vec![EntryAsset {
                id: "asset-1".into(),
                name: "image.png".into(),
                path: "assets/image.png".into(),
            }],
            integrity: EntryIntegrity {
                checksum: "sha256:abc".into(),
                signature: "sig".into(),
            },
            deleted: false,
            deleted_at_micros: None,
            restored_from: None,
        },
        values: BTreeMap::from([
            (
                FieldId::new(100).unwrap(),
                FieldValue::String("typed value".into()),
            ),
            (
                FieldId::new(101).unwrap(),
                FieldValue::List(vec![FieldValue::String("rust".into())]),
            ),
            (
                FieldId::new(102).unwrap(),
                FieldValue::List(vec![FieldValue::Object(BTreeMap::from([
                    ("type".into(), FieldValue::String("issue".into())),
                    ("name".into(), FieldValue::String("1816".into())),
                    (
                        "description".into(),
                        FieldValue::String("typed reference".into()),
                    ),
                ]))]),
            ),
            (
                FieldId::new(103).unwrap(),
                FieldValue::String("00000000-0000-0000-0000-000000000001".into()),
            ),
        ]),
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    };
    append_revisions(&workspace, form.id, vec![revision.clone()]).await?;
    assert_eq!(workspace.read_revisions(form.id).await?, vec![revision]);
    Ok(())
}

#[tokio::test]
async fn concurrent_workspace_writers_surface_equal_version_conflicts() -> anyhow::Result<()> {
    let first = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(50)),
        "memory://iceberg-concurrent-writers",
    )
    .await?;
    let second = first.clone_for_testing();
    let form = form();
    create_form(&first, &form).await?;
    let entry_id = Uuid::from_u128(51).into();
    let mut left = EntryRevision {
        form_id: form.id,
        entry_id,
        revision_id: Uuid::from_u128(52).into(),
        parent_revision_id: None,
        entry_version: 1,
        expected_version: None,
        operation: EntryOperation::Upsert,
        committed_at_micros: 1,
        author_id: "left".into(),
        form_version: form.version,
        source_kind: "test".into(),
        source_id: None,
        entry: EntryMetadata::default(),
        values: BTreeMap::new(),
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    };
    left.values.insert(
        FieldId::new(100).unwrap(),
        FieldValue::String("left".into()),
    );
    let mut right = left.clone();
    right.revision_id = Uuid::from_u128(53).into();
    right.author_id = "right".into();
    right.values.insert(
        FieldId::new(100).unwrap(),
        FieldValue::String("right".into()),
    );
    let (left_result, right_result) = tokio::join!(
        append_revisions(&first, form.id, vec![left]),
        append_revisions(&second, form.id, vec![right.clone()]),
    );
    assert!(
        left_result.is_ok() ^ right_result.is_ok(),
        "same-base mutations for one Entry must have exactly one winner",
    );
    let probe = EntryRevision {
        revision_id: Uuid::from_u128(54).into(),
        ..right
    };
    let conflict = append_revisions(&first, form.id, vec![probe])
        .await
        .unwrap_err();
    assert!(conflict.to_string().contains("conflict"));
    Ok(())
}

#[test]
fn migration_report_rejects_count_or_mapping_drift() {
    let report = MigrationReport::verify(
        MigrationManifest {
            format_version: 1,
            space_id: SpaceId::from(Uuid::from_u128(4)),
            source_backup: "s3://backup/space".into(),
            forms: vec![MigrationFormReport {
                form_id: FormId::from(Uuid::from_u128(5)),
                form_name: "Task".into(),
                source_entry_count: 2,
                source_revision_count: 3,
                migrated_revision_count: 2,
                tombstone_count: 1,
                field_mapping_complete: false,
            }],
        },
        true,
    );
    assert!(!report.logical_data_matches);
    assert_eq!(report.errors.len(), 2);
}
