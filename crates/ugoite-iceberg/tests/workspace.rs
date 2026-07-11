use std::collections::BTreeMap;
use ugoite_domain::entry::{EntryOperation, EntryRevision, FieldValue};
use ugoite_domain::form::{
    FieldType, FormChange, FormChangeSet, FormDefinition, FormField, FormVersion,
};
use ugoite_domain::id::{FieldId, FormId, SpaceId};
use ugoite_iceberg::{
    physical_form_name, IcebergWorkspace, MigrationFormReport, MigrationManifest, MigrationReport,
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

#[tokio::test]
async fn one_stable_form_id_maps_to_one_catalog_table() -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(1)),
        "memory://iceberg-native-workspace",
    )
    .await?;
    let form = form();
    workspace.create_form(&form).await?;
    assert_eq!(workspace.list_forms().await?, vec![form.clone()]);
    assert_eq!(workspace.load_form(form.id).await?, form);
    let table = workspace
        .catalog()
        .load_table(&iceberg::TableIdent::new(
            workspace.namespace().clone(),
            physical_form_name(form.id),
        ))
        .await?;
    assert_eq!(
        table
            .metadata()
            .current_schema()
            .field_by_name("title")
            .unwrap()
            .id,
        13
    );
    assert_eq!(
        table
            .metadata()
            .properties()
            .get("ugoite.form.field-id-map.v1"),
        Some(&r#"{"100":100}"#.to_string())
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
            .catalog()
            .list_tables(workspace.namespace())
            .await?
            .len(),
        1
    );
    let sql = format!(
        "SELECT entry_id FROM ugoite.{}.{} LIMIT 1",
        workspace.namespace().as_ref()[0],
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
    workspace.create_form(&form).await?;
    let table = workspace
        .catalog()
        .load_table(&iceberg::TableIdent::new(
            workspace.namespace().clone(),
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
async fn metadata_evolution_keeps_physical_identity() -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(3)),
        "memory://iceberg-form-evolution",
    )
    .await?;
    let form = form();
    workspace.create_form(&form).await?;
    let evolved = workspace
        .evolve_form(&FormChangeSet {
            form_id: form.id,
            expected_version: Some(form.version),
            changes: vec![FormChange::RenameForm {
                name: "Work item".into(),
            }],
        })
        .await?;
    assert_eq!(evolved.id, form.id);
    assert_eq!(evolved.name, "Work item");
    assert_eq!(
        workspace
            .catalog()
            .list_tables(workspace.namespace())
            .await?
            .len(),
        1
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
    workspace.create_form(&form).await?;
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
        values: BTreeMap::new(),
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    };
    let mut revision = revision;
    revision.values.insert(
        FieldId::new(100).unwrap(),
        FieldValue::String("title from revision".into()),
    );
    workspace
        .append_revisions(form.id, vec![revision.clone()])
        .await?;

    let conflicting = EntryRevision {
        revision_id: Uuid::from_u128(33).into(),
        ..revision
    };
    let error = workspace
        .append_revisions(form.id, vec![conflicting])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("conflict"));
    Ok(())
}

#[tokio::test]
async fn concurrent_workspace_writers_surface_equal_version_conflicts() -> anyhow::Result<()> {
    let first = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(50)),
        "memory://iceberg-concurrent-writers",
    )
    .await?;
    let second = IcebergWorkspace::new(
        first.catalog(),
        SpaceId::from(Uuid::from_u128(50)),
        "memory://iceberg-concurrent-writers",
        Default::default(),
    )
    .await?;
    let form = form();
    first.create_form(&form).await?;
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
        first.append_revisions(form.id, vec![left]),
        second.append_revisions(form.id, vec![right.clone()]),
    );
    left_result.or(right_result)?;
    let probe = EntryRevision {
        revision_id: Uuid::from_u128(54).into(),
        ..right
    };
    let conflict = first
        .append_revisions(form.id, vec![probe])
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
