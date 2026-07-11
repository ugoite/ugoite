use arrow_array::{
    builder::FixedSizeBinaryBuilder, ArrayRef, Int32Array, Int64Array, RecordBatch, StringArray,
    TimestampMicrosecondArray,
};
use std::collections::BTreeMap;
use std::sync::Arc;
use ugoite_domain::entry::{EntryOperation, EntryRevision};
use ugoite_domain::form::{
    FieldType, FormChange, FormChangeSet, FormDefinition, FormField, FormVersion,
};
use ugoite_domain::id::{FieldId, FormId, SpaceId};
use ugoite_iceberg::{
    physical_form_name, IcebergWorkspace, MigrationFormReport, MigrationManifest, MigrationReport,
};
use uuid::Uuid;

fn revision_batch(
    form: &FormDefinition,
    entry_id: Uuid,
    revision_id: Uuid,
    parent_revision_id: Option<Uuid>,
    entry_version: i64,
) -> RecordBatch {
    let table_schema = ugoite_iceberg::arrow_schema_for_form(form).unwrap();
    let mut entry_ids = FixedSizeBinaryBuilder::with_capacity(1, 16);
    entry_ids.append_value(entry_id.as_bytes()).unwrap();
    let mut revision_ids = FixedSizeBinaryBuilder::with_capacity(1, 16);
    revision_ids.append_value(revision_id.as_bytes()).unwrap();
    let mut parents = FixedSizeBinaryBuilder::with_capacity(1, 16);
    match parent_revision_id {
        Some(value) => parents.append_value(value.as_bytes()).unwrap(),
        None => parents.append_null(),
    }
    RecordBatch::try_new(
        Arc::new(table_schema),
        vec![
            Arc::new(entry_ids.finish()) as ArrayRef,
            Arc::new(revision_ids.finish()),
            Arc::new(parents.finish()),
            Arc::new(Int64Array::from(vec![entry_version])),
            Arc::new(StringArray::from(vec!["upsert"])),
            Arc::new(TimestampMicrosecondArray::from(vec![1_i64]).with_timezone("+00:00")),
            Arc::new(StringArray::from(vec!["human:owner"])),
            Arc::new(Int32Array::from(vec![1_i32])),
            Arc::new(StringArray::from(vec!["test"])),
            Arc::new(StringArray::from(vec![None::<&str>])),
            Arc::new(StringArray::from(vec![None::<&str>])),
            Arc::new(StringArray::from(vec![None::<&str>])),
            Arc::new(StringArray::from(vec!["title"])),
        ],
    )
    .unwrap()
}

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
        Some(&r#"{"100":13}"#.to_string())
    );
    assert_eq!(
        physical_form_name(form.id),
        "form_00000000000000000000000000000002"
    );
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
    let identity_error = workspace
        .append_record_batches(
            form.id,
            vec![revision_batch(
                &form,
                Uuid::from_u128(99),
                revision_id,
                None,
                1,
            )],
            std::slice::from_ref(&revision),
        )
        .await
        .unwrap_err();
    assert!(identity_error.to_string().contains("metadata"));
    workspace
        .append_record_batches(
            form.id,
            vec![revision_batch(&form, entry_id, revision_id, None, 1)],
            std::slice::from_ref(&revision),
        )
        .await?;

    let conflicting = EntryRevision {
        revision_id: Uuid::from_u128(33).into(),
        ..revision
    };
    let error = workspace
        .append_record_batches(
            form.id,
            vec![revision_batch(
                &form,
                entry_id,
                Uuid::from_u128(33),
                None,
                1,
            )],
            std::slice::from_ref(&conflicting),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("conflict"));
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
