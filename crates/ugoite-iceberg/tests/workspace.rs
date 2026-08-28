use opendal::services::Memory;
use opendal::{EntryMode, Operator};
use std::collections::BTreeMap;
use std::time::Duration;
use ugoite_core::error::{AppError, ErrorCode, ErrorKind};
use ugoite_core::query::{
    AuthorizedQueryForm, AuthorizedQueryPolicy, EntryScope, QueryLimits, QuerySystemColumn,
};
use ugoite_domain::change::ChangeCommand;
use ugoite_domain::entry::{
    EntryIntegrity, EntryMetadata, EntryOperation, EntryRevision, FieldValue,
};
use ugoite_domain::form::{
    sql_column_name, sql_relation_name, FieldType, FormChange, FormChangeSet, FormDefinition,
    FormField, FormVersion, ListItemDefinition,
};
use ugoite_domain::id::{FieldId, FormId, SpaceId};
use ugoite_iceberg::{
    physical_form_name, publication_context, publication_context_for_change, IcebergWorkspace,
    RevisionView, WriteConfig,
};
use ugoite_storage::{SpaceCatalogStore, SpaceUri};
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
            list_item: None,
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
    let change_id = revisions
        .first()
        .map(|revision| revision.change_id.clone())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let revisions = revisions
        .into_iter()
        .map(|mut revision| {
            revision.change_id = change_id.clone();
            revision
        })
        .collect::<Vec<_>>();
    let actor = revisions
        .first()
        .map(|revision| revision.entry.updated_by.clone())
        .unwrap_or_else(|| "test:owner".to_string());
    let command = ChangeCommand {
        change_id,
        run_id: None,
        actor_principal_id: actor,
        message: Some("test change".into()),
        reverts_change_id: None,
        created_at_micros: revisions
            .iter()
            .map(|revision| revision.committed_at_micros)
            .max()
            .unwrap_or_default(),
    };
    let command_context =
        publication_context_for_change(&command, "test.entry.append", &revisions)?;
    workspace
        .commit(command_context)?
        .append_revisions(form_id, revisions)
        .await
}

#[tokio::test]
async fn one_stable_form_id_maps_to_one_catalog_table() -> anyhow::Result<()> {
    let space_id = SpaceId::from(Uuid::now_v7());
    let workspace =
        IcebergWorkspace::memory_for_tests(space_id, "memory://iceberg-native-workspace").await?;
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
    let table_location = SpaceUri::parse(table.metadata().location())?;
    assert_eq!(table_location.space_uid(), space_id.as_uuid());
    assert_eq!(
        table_location.key().as_str(),
        "forms/form_00000000000000000000000000000002"
    );
    let metadata_location = SpaceUri::parse(table.metadata_location_result()?)?;
    assert_eq!(metadata_location.space_uid(), space_id.as_uuid());
    assert!(metadata_location
        .key()
        .as_str()
        .starts_with("forms/form_00000000000000000000000000000002/metadata/"));
    let persisted_metadata = table
        .file_io()
        .new_input(table.metadata_location_result()?)?
        .read()
        .await?;
    let persisted_metadata = String::from_utf8(persisted_metadata.to_vec())?;
    assert!(persisted_metadata.contains("ugoite://"));
    assert!(!persisted_metadata.contains("memory://"));
    assert!(!persisted_metadata.contains("file://"));
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
async fn pins_are_head_owned_publication_references() -> anyhow::Result<()> {
    let space_id = SpaceId::from(Uuid::now_v7());
    let workspace =
        IcebergWorkspace::memory_for_tests(space_id, "memory://iceberg-pin-head").await?;
    let form = form();
    create_form(&workspace, &form).await?;

    let pin = workspace
        .create_pin(
            "before-import",
            "principal:owner",
            42,
            "pin-create-before-import",
        )
        .await?;
    assert_eq!(pin.created_by_principal_id, "principal:owner");
    assert_eq!(pin.coordinate.generation, 0);
    assert!(pin
        .coordinate
        .publication_uri
        .to_string()
        .starts_with("ugoite://"));

    let replayed_pin = workspace
        .create_pin(
            "before-import",
            "principal:owner",
            99,
            "pin-create-before-import",
        )
        .await?;
    assert_eq!(replayed_pin, pin);

    let first_create = workspace.clone();
    let second_create = workspace.clone();
    let (first_create, second_create) = tokio::join!(
        first_create.create_pin("concurrent", "principal:owner", 42, "pin-create-concurrent",),
        second_create.create_pin("concurrent", "principal:owner", 42, "pin-create-concurrent",)
    );
    assert!(first_create.is_ok());
    assert!(second_create.is_ok());

    let pins = workspace.list_pins().await?;
    assert_eq!(pins.get("before-import"), Some(&pin));
    assert!(pins.contains_key("concurrent"));
    workspace
        .delete_pin("before-import", "pin-delete-before-import")
        .await?;
    workspace
        .delete_pin("before-import", "pin-delete-before-import")
        .await?;
    let first_delete = workspace.clone();
    let second_delete = workspace.clone();
    let (first_delete, second_delete) = tokio::join!(
        first_delete.delete_pin("concurrent", "pin-delete-concurrent"),
        second_delete.delete_pin("concurrent", "pin-delete-concurrent")
    );
    assert!(first_delete.is_ok());
    assert!(second_delete.is_ok());
    assert!(workspace.list_pins().await?.is_empty());

    let revision = EntryRevision {
        form_id: form.id,
        entry_id: Uuid::from_u128(3_001).into(),
        revision_id: Uuid::from_u128(3_002).into(),
        change_id: "change-history-1".into(),
        parent_revision_id: None,
        entry_version: 1,
        expected_version: None,
        operation: EntryOperation::Upsert,
        committed_at_micros: 43,
        author_id: "principal:owner".into(),
        form_version: form.version,
        source_kind: "test".into(),
        source_id: None,
        entry: EntryMetadata {
            external_id: "entry-history-1".into(),
            updated_by: "principal:owner".into(),
            ..EntryMetadata::default()
        },
        values: BTreeMap::from([(
            FieldId::new(100).unwrap(),
            FieldValue::String("history".into()),
        )]),
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    };
    let command = ChangeCommand {
        change_id: "change-history-1".into(),
        run_id: None,
        actor_principal_id: "principal:owner".into(),
        message: Some("test change".into()),
        reverts_change_id: None,
        created_at_micros: 43,
    };
    let command_context = publication_context_for_change(&command, "test.entry.append", &revision)?;
    let receipt = workspace
        .commit(command_context)?
        .append_revisions(form.id, vec![revision])
        .await?;
    let changes = workspace.list_changes().await?;
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].change_id, receipt.command_id);
    assert_eq!(changes[0].change.actor_principal_id, "principal:owner");
    assert_eq!(changes[0].change.message.as_deref(), Some("test change"));
    assert_eq!(changes[0].generation, receipt.catalog_generation);
    Ok(())
}

#[tokio::test]
async fn revert_change_appends_a_selective_inverse_without_rewinding_head() -> anyhow::Result<()> {
    let space_id = SpaceId::from(Uuid::now_v7());
    let workspace =
        IcebergWorkspace::memory_for_tests(space_id, "memory://iceberg-revert-change").await?;
    let form = form();
    create_form(&workspace, &form).await?;
    let entry_id = Uuid::from_u128(3_101).into();
    let title = FieldId::new(100).unwrap();
    let initial = EntryRevision {
        form_id: form.id,
        entry_id,
        revision_id: Uuid::from_u128(3_102).into(),
        change_id: "change-create".into(),
        parent_revision_id: None,
        entry_version: 1,
        expected_version: None,
        operation: EntryOperation::Upsert,
        committed_at_micros: 1,
        author_id: "principal:owner".into(),
        form_version: form.version,
        source_kind: "test".into(),
        source_id: None,
        entry: EntryMetadata {
            external_id: "entry-revert-1".into(),
            updated_by: "principal:owner".into(),
            ..EntryMetadata::default()
        },
        values: BTreeMap::from([(title, FieldValue::String("before".into()))]),
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    };
    append_revisions(&workspace, form.id, vec![initial.clone()]).await?;
    let changed = EntryRevision {
        revision_id: Uuid::from_u128(3_103).into(),
        change_id: "change-target".into(),
        parent_revision_id: Some(initial.revision_id),
        entry_version: 2,
        expected_version: Some(1),
        committed_at_micros: 2,
        entry: EntryMetadata {
            external_id: "entry-revert-1".into(),
            updated_by: "principal:owner".into(),
            ..EntryMetadata::default()
        },
        values: BTreeMap::from([(title, FieldValue::String("after".into()))]),
        ..initial
    };
    append_revisions(&workspace, form.id, vec![changed]).await?;

    let command = ChangeCommand {
        change_id: "change-undo".into(),
        run_id: Some(ugoite_domain::change::RunId::new("run-1")?),
        actor_principal_id: "principal:owner".into(),
        message: Some("undo target".into()),
        reverts_change_id: Some("change-target".into()),
        created_at_micros: 3,
    };
    let receipt = workspace.revert_change("change-target", &command).await?;
    assert_eq!(receipt.command_id, "change-undo");
    let current = workspace
        .read_revision_view(form.id, RevisionView::Current)
        .await?;
    assert_eq!(current.len(), 1);
    assert_eq!(
        current[0].values.get(&title),
        Some(&FieldValue::String("before".into()))
    );
    let changes = workspace.list_changes().await?;
    assert!(changes
        .iter()
        .any(|change| change.change_id == "change-undo"
            && change.change.reverts_change_id.as_deref() == Some("change-target")
            && change.change.run_id.as_ref().map(|run| run.as_str()) == Some("run-1")));
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
        list_item: None,
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
        list_item: None,
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
async fn sql_relation_and_saved_query_survive_form_rename() -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(305)),
        "memory://iceberg-stable-sql-relation",
    )
    .await?;
    let form = form();
    create_form(&workspace, &form).await?;
    let relation = sql_relation_name(form.id);
    let saved_sql = format!("SELECT * FROM \"{relation}\" ORDER BY _ugoite_id");
    let before = workspace.capture_checkpoint().await?;

    evolve_form(
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
    let after = workspace.capture_checkpoint().await?;
    assert_eq!(
        workspace.form_at_checkpoint(&before, &relation).await?.name,
        "Task"
    );
    assert_eq!(
        workspace.form_at_checkpoint(&after, &relation).await?.name,
        "Work item"
    );

    let context = workspace
        .authorized_query_context(AuthorizedQueryPolicy {
            forms: [(
                form.id,
                AuthorizedQueryForm {
                    relation: relation.clone(),
                    entry_scope: EntryScope::AllCurrent,
                    columns: [sql_column_name(form.fields[0].id)].into_iter().collect(),
                    system_columns: [QuerySystemColumn::ExternalId].into_iter().collect(),
                },
            )]
            .into_iter()
            .collect(),
            checkpoint: Some(after),
            limits: QueryLimits {
                max_memory_bytes: 8 * 1024 * 1024,
                max_rows: 10,
                timeout: Duration::from_secs(5),
                max_concurrency: 1,
                allowed_functions: Default::default(),
            },
        })
        .await?;
    context.execute(&saved_sql).await?;
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
                list_item: None,
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
async fn existing_form_field_type_changes_are_typed_and_leave_form_unchanged() -> anyhow::Result<()>
{
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(5)),
        "memory://iceberg-unsupported-form-type-change",
    )
    .await?;
    let mut form = form();
    form.fields[0].field_type = FieldType::Timestamp;
    form.fields.push(FormField {
        id: FieldId::new(101).unwrap(),
        name: "count".into(),
        field_type: FieldType::Integer,
        required: false,
        label: None,
        description: None,
        semantic_role: None,
        reference_form: None,
        list_item: None,
        validation: None,
        enum_values: Vec::new(),
        deprecated: false,
    });
    create_form(&workspace, &form).await?;
    let checkpoint_before = workspace.capture_checkpoint().await?;

    for (field_id, target_type, expected_message) in [
        (
            FieldId::new(100).unwrap(),
            FieldType::Date,
            "Changing the type of existing Form field 'title' from 'timestamp' to 'date' is not supported; create a new field instead",
        ),
        (
            FieldId::new(101).unwrap(),
            FieldType::Long,
            "Changing the type of existing Form field 'count' from 'integer' to 'long' is not supported; create a new field instead",
        ),
    ] {
        let error = evolve_form(
            &workspace,
            &FormChangeSet {
                form_id: form.id,
                expected_version: Some(form.version),
                changes: vec![FormChange::ChangeFieldType {
                    field_id,
                    field_type: target_type,
                }],
            },
        )
        .await
        .expect_err("existing Form field type changes must be rejected");
        let app_error = error
            .downcast_ref::<AppError>()
            .expect("type-change rejection must remain a typed application error");
        assert_eq!(app_error.kind(), ErrorKind::InvalidInput);
        assert_eq!(app_error.code(), ErrorCode::FormFieldTypeChangeNotSupported);
        assert_eq!(app_error.message(), expected_message);
        assert_eq!(workspace.load_form(form.id).await?, form);
        assert_eq!(workspace.capture_checkpoint().await?, checkpoint_before);
    }
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
        change_id: "change-32".into(),
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
            updated_by: "human:owner".into(),
            ..EntryMetadata::default()
        },
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
        ..revision.clone()
    };
    let error = append_revisions(&workspace, form.id, vec![conflicting])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("reused"));

    let changed_author = EntryRevision {
        revision_id: Uuid::from_u128(38).into(),
        change_id: "change-38".into(),
        parent_revision_id: Some(revision.revision_id),
        entry_version: 2,
        expected_version: Some(1),
        author_id: "human:other".into(),
        ..revision
    };
    let author_error = append_revisions(&workspace, form.id, vec![changed_author])
        .await
        .unwrap_err();
    assert!(author_error
        .to_string()
        .contains("entry author cannot change"));
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
    let history_error = workspace
        .read_revision_view_with_scope(form.id, EntryScope::AllCurrent, RevisionView::All)
        .await
        .expect_err("scoped reads must never ignore their Entry scope for history");
    assert!(history_error
        .to_string()
        .contains("scoped revision views do not expose full history"));

    let first = EntryRevision {
        form_id: form.id,
        entry_id: Uuid::from_u128(61).into(),
        revision_id: Uuid::from_u128(62).into(),
        change_id: "change-62".into(),
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
            updated_by: "human:owner".into(),
            ..EntryMetadata::default()
        },
        values: BTreeMap::new(),
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    };
    let first_receipt = append_revisions(&workspace, form.id, vec![first.clone()]).await?;

    let deleted = EntryRevision {
        change_id: "change-delete".into(),
        revision_id: Uuid::from_u128(63).into(),
        parent_revision_id: Some(first.revision_id),
        entry_version: 2,
        expected_version: Some(1),
        operation: EntryOperation::Delete,
        committed_at_micros: 2,
        entry: EntryMetadata {
            deleted: true,
            deleted_at_micros: Some(2),
            updated_by: "human:owner".into(),
            deleted_by: Some("human:owner".into()),
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
        change_id: "change-restore".into(),
        revision_id: Uuid::from_u128(64).into(),
        parent_revision_id: Some(deleted.revision_id),
        entry_version: 3,
        expected_version: Some(2),
        operation: EntryOperation::Restore,
        committed_at_micros: 3,
        entry: EntryMetadata {
            restored_from: Some(deleted.revision_id),
            updated_by: "human:owner".into(),
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
        change_id: "change-36".into(),
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
            updated_by: "human:owner".into(),
            ..EntryMetadata::default()
        },
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

async fn count_files_under(operator: &Operator, prefix: &str) -> anyhow::Result<usize> {
    Ok(operator
        .list_with(prefix)
        .recursive(true)
        .await?
        .into_iter()
        .filter(|entry| entry.metadata().mode() == EntryMode::FILE)
        .count())
}

#[tokio::test]
async fn append_recovery_adopts_existing_publication_without_rewriting_iceberg(
) -> anyhow::Result<()> {
    let operator = Operator::new(Memory::default())?;
    let store = SpaceCatalogStore::new(operator.clone(), "spaces/append-publication-recovery")?
        .single_process();
    let workspace = IcebergWorkspace::open_space(
        store.clone(),
        SpaceId::from(Uuid::from_u128(18_527)),
        WriteConfig::default(),
    )
    .await?;
    let form = form();
    create_form(&workspace, &form).await?;
    let revision = EntryRevision {
        form_id: form.id,
        entry_id: Uuid::from_u128(18_528).into(),
        revision_id: Uuid::from_u128(18_529).into(),
        change_id: "change-18529".into(),
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
            updated_by: "human:owner".into(),
            ..EntryMetadata::default()
        },
        values: BTreeMap::from([(
            FieldId::new(100).unwrap(),
            FieldValue::String("recovered append".into()),
        )]),
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    };
    let command = publication_context(
        "append-publication-recovery",
        "test.entry.append",
        &vec![revision.clone()],
    )?;
    let base_head = store.read_exact_head().await?.expect("base Head");
    let base_head_json: serde_json::Value = serde_json::from_slice(&base_head.bytes)?;
    let intended = store.publication_path(
        base_head_json["generation"].as_u64().expect("generation") + 1,
        command.command_id(),
    );
    let forms_prefix = "spaces/append-publication-recovery/forms";

    // Stop the real Iceberg append after it has created Parquet, manifest,
    // metadata, and the immutable publication, but before Head CAS.
    let gate = ugoite_iceberg::TestPublicationGate::new();
    ugoite_iceberg::install_test_publication_gate(gate.clone());
    let append_workspace = workspace.clone();
    let append_command = command.clone();
    let append_revision = revision.clone();
    let append = tokio::spawn(async move {
        append_workspace
            .commit(append_command)
            .expect("publication context")
            .append_revisions(form.id, vec![append_revision])
            .await
    });
    gate.wait_until_entered().await;
    let publication_before = store.read_publication(&intended).await?;
    let files_before = count_files_under(&operator, forms_prefix).await?;
    assert_eq!(
        store.read_exact_head().await?.expect("Head").bytes,
        base_head.bytes,
        "the append must still be invisible before Head CAS"
    );
    append.abort();
    let aborted = append.await;
    assert!(aborted.is_err(), "the original writer must be discarded");
    ugoite_iceberg::clear_test_publication_gate();
    gate.release();

    // A fresh coordinator adopts the durable publication. It must not invoke
    // append_revisions' Iceberg writer a second time, so no new physical file
    // or alternate publication can appear.
    let recovered = workspace
        .commit(command)
        .expect("publication context")
        .append_revisions(form.id, vec![revision.clone()])
        .await?;
    assert_eq!(recovered.data_file_count, 0);
    assert_eq!(
        store.read_publication(&intended).await?,
        publication_before,
        "recovery must adopt, not regenerate, the immutable publication"
    );
    assert_eq!(
        count_files_under(&operator, forms_prefix).await?,
        files_before,
        "recovery must not write another Parquet/manifest/metadata object"
    );
    assert_eq!(workspace.read_revisions(form.id).await?, vec![revision]);
    let publication_json: serde_json::Value = serde_json::from_slice(&publication_before)?;
    let head_json: serde_json::Value = serde_json::from_slice(
        &store
            .read_exact_head()
            .await?
            .expect("Head after recovery")
            .bytes,
    )?;
    assert_eq!(head_json, publication_json["next_head"]);
    assert!(
        !operator
            .exists(&format!(
                "spaces/append-publication-recovery/_ugoite/catalog/command-receipts/{}.json",
                recovered.command_id
            ))
            .await?
    );
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
                change_id: format!("change-{id}"),
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
                    updated_by: "human:owner".into(),
                    ..EntryMetadata::default()
                },
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
        change_id: "change-62".into(),
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
            updated_by: "human:owner".into(),
            ..Default::default()
        },
        values: BTreeMap::from([(FieldId::new(100).unwrap(), FieldValue::String("old".into()))]),
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    };
    append_revisions(&workspace, form.id, vec![first.clone()]).await?;
    let before_evolution = workspace
        .catalog_for_testing()
        .load_table(&iceberg::TableIdent::new(
            workspace.namespace_for_testing().clone(),
            physical_form_name(form.id),
        ))
        .await?;
    let before_snapshot = before_evolution
        .metadata()
        .current_snapshot()
        .expect("append creates a current snapshot");
    let before_snapshot_identity = (
        before_snapshot.snapshot_id(),
        before_snapshot.sequence_number(),
        before_snapshot.parent_snapshot_id(),
        before_snapshot.manifest_list().to_string(),
    );
    let before_snapshot_schema_id = before_snapshot.schema_id();

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
                    list_item: None,
                    validation: None,
                    enum_values: Vec::new(),
                    deprecated: false,
                }),
            ],
        },
    )
    .await?;
    assert_eq!(
        workspace.form_history(form.id).await?,
        vec![form.clone(), evolved.clone()]
    );
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
    let after_snapshot = table
        .metadata()
        .current_snapshot()
        .expect("current snapshot remains available after schema evolution");
    assert_eq!(
        (
            after_snapshot.snapshot_id(),
            after_snapshot.sequence_number(),
            after_snapshot.parent_snapshot_id(),
            after_snapshot.manifest_list().to_string(),
        ),
        before_snapshot_identity
    );
    assert_eq!(after_snapshot.schema_id(), before_snapshot_schema_id);

    let metadata_location = table.metadata_location_result()?.to_string();
    let metadata_before_read = table
        .file_io()
        .new_input(&metadata_location)?
        .read()
        .await?;
    let current = workspace
        .read_revision_view_with_scope(form.id, EntryScope::AllCurrent, RevisionView::Current)
        .await?;
    assert_eq!(current.len(), 1);
    assert_eq!(
        current[0].values.get(&FieldId::new(100).unwrap()),
        Some(&FieldValue::String("old".into()))
    );
    assert_eq!(current[0].form_version, form.version);
    assert!(!current[0].values.contains_key(&FieldId::new(101).unwrap()));

    let explicit_snapshot = workspace
        .read_revision_view_at_snapshot(
            form.id,
            RevisionView::Current,
            after_snapshot.snapshot_id(),
        )
        .await?;
    assert_eq!(explicit_snapshot.len(), 1);
    assert_eq!(
        explicit_snapshot[0].values.get(&FieldId::new(100).unwrap()),
        Some(&FieldValue::String("old".into()))
    );
    assert!(!explicit_snapshot[0]
        .values
        .contains_key(&FieldId::new(101).unwrap()));

    let checkpoint = workspace.capture_checkpoint().await?;
    let checkpoint_current = workspace
        .read_revision_view_at_checkpoint(&checkpoint, form.id, RevisionView::Current)
        .await?;
    assert_eq!(checkpoint_current, explicit_snapshot);
    let metadata_after_read = table
        .file_io()
        .new_input(&metadata_location)?
        .read()
        .await?;
    assert_eq!(
        metadata_after_read, metadata_before_read,
        "a read must not rewrite Iceberg metadata"
    );

    let second = EntryRevision {
        form_id: evolved.id,
        entry_id,
        revision_id: Uuid::from_u128(63).into(),
        change_id: "change-63".into(),
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
            updated_by: "human:owner".into(),
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
            list_item: None,
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
            list_item: None,
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
            reference_form: Some(form.id),
            list_item: None,
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
        change_id: "change-72".into(),
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
            created_at_micros: 10,
            updated_at_micros: 11,
            updated_by: "human:owner".into(),
            integrity: EntryIntegrity {
                checksum: "sha256:abc".into(),
                signature: "sig".into(),
            },
            deleted: false,
            deleted_at_micros: None,
            deleted_by: None,
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
                FieldValue::String("task-71".into()),
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
async fn every_supported_typed_list_item_round_trips_with_nulls() -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(73)),
        "memory://iceberg-all-typed-lists",
    )
    .await?;
    let mut form = form();
    let list_types = [
        (101, "booleans", FieldType::Boolean),
        (102, "integers", FieldType::Integer),
        (103, "longs", FieldType::Long),
        (104, "floats", FieldType::Float),
        (105, "doubles", FieldType::Double),
        (106, "dates", FieldType::Date),
        (107, "times", FieldType::Time),
        (108, "timestamps", FieldType::Timestamp),
        (109, "timestamp_tzs", FieldType::TimestampTz),
        (110, "timestamp_nss", FieldType::TimestampNs),
        (111, "timestamp_tz_nss", FieldType::TimestampTzNs),
        (112, "uuids", FieldType::Uuid),
        (113, "binaries", FieldType::Binary),
    ];
    form.fields
        .extend(list_types.iter().map(|(id, name, item_type)| FormField {
            id: FieldId::new(*id).unwrap(),
            name: (*name).into(),
            field_type: FieldType::List,
            required: false,
            label: None,
            description: None,
            semantic_role: None,
            reference_form: None,
            list_item: Some(ListItemDefinition {
                field_type: item_type.clone(),
                reference_form: None,
            }),
            validation: None,
            enum_values: Vec::new(),
            deprecated: false,
        }));
    create_form(&workspace, &form).await?;
    let revision = EntryRevision {
        form_id: form.id,
        entry_id: Uuid::from_u128(74).into(),
        revision_id: Uuid::from_u128(75).into(),
        change_id: "change-75".into(),
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
            external_id: "typed-lists".into(),
            updated_by: "human:owner".into(),
            ..Default::default()
        },
        values: BTreeMap::from([
            (FieldId::new(100).unwrap(), FieldValue::String("all".into())),
            (
                FieldId::new(101).unwrap(),
                FieldValue::List(vec![FieldValue::Boolean(true), FieldValue::Null]),
            ),
            (
                FieldId::new(102).unwrap(),
                FieldValue::List(vec![FieldValue::Integer(7), FieldValue::Null]),
            ),
            (
                FieldId::new(103).unwrap(),
                FieldValue::List(vec![FieldValue::Integer(7), FieldValue::Null]),
            ),
            (
                FieldId::new(104).unwrap(),
                FieldValue::List(vec![FieldValue::Number(1.25), FieldValue::Null]),
            ),
            (
                FieldId::new(105).unwrap(),
                FieldValue::List(vec![FieldValue::Number(2.5), FieldValue::Null]),
            ),
            (
                FieldId::new(106).unwrap(),
                FieldValue::List(vec![
                    FieldValue::String("2025-01-02".into()),
                    FieldValue::Null,
                ]),
            ),
            (
                FieldId::new(107).unwrap(),
                FieldValue::List(vec![
                    FieldValue::String("12:34:56.123456".into()),
                    FieldValue::Null,
                ]),
            ),
            (
                FieldId::new(108).unwrap(),
                FieldValue::List(vec![
                    FieldValue::String("2025-01-02T03:04:05.123456".into()),
                    FieldValue::Null,
                ]),
            ),
            (
                FieldId::new(109).unwrap(),
                FieldValue::List(vec![
                    FieldValue::String("2025-01-02T03:04:05.123456+00:00".into()),
                    FieldValue::Null,
                ]),
            ),
            (
                FieldId::new(110).unwrap(),
                FieldValue::List(vec![
                    FieldValue::String("2025-01-02T03:04:05.123456789".into()),
                    FieldValue::Null,
                ]),
            ),
            (
                FieldId::new(111).unwrap(),
                FieldValue::List(vec![
                    FieldValue::String("2025-01-02T03:04:05.123456789Z".into()),
                    FieldValue::Null,
                ]),
            ),
            (
                FieldId::new(112).unwrap(),
                FieldValue::List(vec![
                    FieldValue::String("00000000-0000-0000-0000-000000000001".into()),
                    FieldValue::Null,
                ]),
            ),
            (
                FieldId::new(113).unwrap(),
                FieldValue::List(vec![
                    FieldValue::String("base64:AQI=".into()),
                    FieldValue::Null,
                ]),
            ),
        ]),
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    };
    append_revisions(&workspace, form.id, vec![revision.clone()]).await?;
    let restored = workspace.read_revisions(form.id).await?;
    assert_eq!(restored, vec![revision]);
    Ok(())
}
