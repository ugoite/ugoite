use anyhow::Result;
use iceberg::spec::Schema;
use iceberg::TableCreation;
use std::env;
use ugoite_domain::id::SpaceId;
use ugoite_iceberg::{IcebergWorkspace, WriteConfig};
use ugoite_storage::{operator_from_uri_with_endpoint, SpaceCatalogStore};
use uuid::Uuid;

/// Exercise an actual Iceberg metadata round trip against the configured
/// operator used by the Catalog Head and its logical FileIO bridge.
#[tokio::test]
async fn minio_space_catalog_uses_the_configured_operator() -> Result<()> {
    let Some(endpoint) = env::var_os("UGOITE_MINIO_ENDPOINT") else {
        return Ok(());
    };
    let bucket = env::var("UGOITE_MINIO_BUCKET").unwrap_or_else(|_| "ugoite-ci".to_string());
    let root = format!("s3://{bucket}/ugoite-iceberg-e2e-{}", Uuid::now_v7());
    let endpoint = endpoint.to_string_lossy().into_owned();
    let operator = operator_from_uri_with_endpoint(&root, Some(&endpoint))?;
    let store = SpaceCatalogStore::new(operator.clone(), "spaces/minio")?
        .verify_shared_writes()
        .await?;
    let space_id = SpaceId::from(Uuid::now_v7());
    let workspace = IcebergWorkspace::open_space(store, space_id, WriteConfig::default()).await?;
    let catalog = workspace.catalog_for_testing();
    let namespace = workspace.namespace_for_testing().clone();

    let table = catalog
        .create_table(
            &namespace,
            TableCreation::builder()
                .name("form_00000000000000000000000000000001".to_string())
                .location(format!("ugoite://{}/forms/form", space_id.as_uuid()))
                .schema(Schema::builder().with_fields(vec![]).build()?)
                .build(),
        )
        .await?;
    assert!(catalog.table_exists(table.identifier()).await?);

    // Cleanup is maintenance only; authoritative correctness is established
    // by the Head and metadata writes above.
    let _ = operator.delete_with("").recursive(true).await;
    Ok(())
}
