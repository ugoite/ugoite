use std::collections::BTreeMap;
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
