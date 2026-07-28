use std::collections::BTreeMap;

use ugoite_domain::entry::{EntryMetadata, EntryOperation, EntryRevision, FieldValue};
use ugoite_domain::form::{FieldType, FormDefinition, FormField, FormVersion};
use ugoite_domain::id::{FieldId, FormId, SpaceId};
use ugoite_iceberg::{health::HealthStatus, publication_context, IcebergWorkspace};
use ugoite_storage::operator_from_uri;
use uuid::Uuid;

fn form() -> FormDefinition {
    FormDefinition {
        id: FormId::from(Uuid::from_u128(200)),
        version: FormVersion::new(1).expect("test Form version"),
        name: "Health".into(),
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

fn revision(form: &FormDefinition) -> EntryRevision {
    EntryRevision {
        form_id: form.id,
        entry_id: Uuid::from_u128(201).into(),
        revision_id: Uuid::from_u128(202).into(),
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
        values: [(
            FieldId::new(100).expect("test field ID"),
            FieldValue::String("safe".into()),
        )]
        .into_iter()
        .collect(),
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    }
}

#[tokio::test]
async fn health_uses_only_reachable_metadata_and_redacts_locations() -> anyhow::Result<()> {
    let warehouse = "memory://health-evidence";
    let space_id = SpaceId::from(Uuid::from_u128(203));
    let workspace = IcebergWorkspace::memory_for_tests(space_id, warehouse).await?;
    let form = form();
    workspace
        .commit(publication_context(
            "health-form",
            "test.form.create",
            &form,
        )?)?
        .create_form(&form)
        .await?;
    workspace
        .commit(publication_context(
            "health-entry",
            "test.entry.append",
            &revision(&form),
        )?)?
        .append_revisions(form.id, vec![revision(&form)])
        .await?;
    let checkpoint = workspace.capture_checkpoint().await?;
    workspace.save_checkpoint("healthy", &checkpoint).await?;

    let head_path = format!(
        "test/space_{}/_ugoite/catalog/head.json",
        space_id.as_uuid().simple()
    );
    let operator = operator_from_uri(warehouse)?;
    let before = operator.read(&head_path).await?.to_vec();
    let report = workspace
        .health_report(&["healthy".into(), "missing".into()])
        .await?;
    let after = operator.read(&head_path).await?.to_vec();

    assert_eq!(before, after, "health must not mutate Catalog Head");
    assert_eq!(report.status, HealthStatus::Degraded);
    assert!(report.catalog_head.readable);
    assert!(report.catalog_head.checksum.is_some());
    assert_eq!(report.tables.len(), 1);
    assert_eq!(report.tables[0].status, HealthStatus::Healthy);
    assert_eq!(
        report.tables[0].form_id.as_deref(),
        Some(form.id.to_string().as_str())
    );
    assert!(report.tables[0].snapshot_count.unwrap_or_default() >= 1);
    assert!(report.tables[0].manifest_count.unwrap_or_default() >= 1);
    assert!(report.tables[0].total_data_file_count.unwrap_or_default() >= 1);
    assert!(report.tables[0].total_record_count.unwrap_or_default() >= 1);
    assert!(report.tables[0].file_size_distribution.is_some());
    assert!(report.tables[0].metadata_location_redacted);
    assert_eq!(report.checkpoints[0].status, HealthStatus::Healthy);
    assert_eq!(report.checkpoints[1].status, HealthStatus::Degraded);
    let json = serde_json::to_string(&report)?;
    let value: serde_json::Value = serde_json::from_str(&json)?;
    assert!(value.pointer("/tables/0/metadata_location").is_none());
    assert!(!json.contains("memory://"));
    assert!(report.unreachable_failed_attempts.is_empty());
    assert!(!report.backend.shared_write_contract);
    assert_eq!(
        serde_json::to_value(&report.backend)?["probe_status"],
        "active_probe_unavailable"
    );
    Ok(())
}

#[tokio::test]
async fn health_reports_an_unreadable_catalog_head_without_storage_inference() -> anyhow::Result<()>
{
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(204)),
        "memory://health-missing-head",
    )
    .await?;

    let report = workspace
        .health_report(&["known-checkpoint".into()])
        .await?;

    assert_eq!(report.status, HealthStatus::Degraded);
    assert!(!report.catalog_head.readable);
    assert!(report.catalog_head.checksum.is_none());
    assert_eq!(
        report.catalog_head.issue.as_ref().map(|issue| issue.code),
        Some("catalog_head_missing")
    );
    assert!(report.tables.is_empty());
    assert_eq!(report.checkpoints[0].status, HealthStatus::Degraded);
    Ok(())
}

#[tokio::test]
async fn health_classifies_a_corrupt_exact_head_without_disclosing_location() -> anyhow::Result<()>
{
    let warehouse = "memory://health-corrupt-head";
    let space_id = SpaceId::from(Uuid::from_u128(205));
    let workspace = IcebergWorkspace::memory_for_tests(space_id, warehouse).await?;
    let definition = form();
    workspace
        .commit(publication_context(
            "health-form",
            "test.form.create",
            &definition,
        )?)?
        .create_form(&definition)
        .await?;

    let head_path = format!(
        "test/space_{}/_ugoite/catalog/head.json",
        space_id.as_uuid().simple()
    );
    operator_from_uri(warehouse)?
        .write(&head_path, b"not-json".to_vec())
        .await?;

    let report = workspace.health_report(&[]).await?;
    assert_eq!(report.status, HealthStatus::Degraded);
    assert_eq!(
        report.catalog_head.issue.as_ref().map(|issue| issue.code),
        Some("catalog_head_decode_failure")
    );
    assert!(!serde_json::to_string(&report)?.contains("memory://"));
    Ok(())
}
