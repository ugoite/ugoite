use anyhow::{bail, Result};
use iceberg::spec::Schema;
use iceberg::TableCreation;
use std::env;
use ugoite_domain::id::SpaceId;
use ugoite_iceberg::{IcebergWorkspace, WriteConfig};
use ugoite_storage::{operator_from_uri_with_endpoint, SpaceCatalogStore};
use uuid::Uuid;

/// Exercise an actual Iceberg metadata round trip against the configured
/// operator used by the Catalog Head and its logical FileIO bridge. Local
/// development skips this test unless `UGOITE_MINIO_REQUIRED` is set; the CI
/// release gate supplies the complete configuration explicitly.
#[tokio::test]
async fn minio_space_catalog_uses_the_configured_operator() -> Result<()> {
    let Some((endpoint, bucket)) = minio_test_config()? else {
        return Ok(());
    };
    let root = format!("s3://{bucket}/ugoite-iceberg-e2e-{}", Uuid::now_v7());
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

fn minio_test_config() -> Result<Option<(String, String)>> {
    let required = env::var_os("UGOITE_MINIO_REQUIRED").is_some();
    minio_test_config_from(
        required,
        env::var("UGOITE_MINIO_ENDPOINT").ok().as_deref(),
        env::var("UGOITE_MINIO_BUCKET").ok().as_deref(),
    )
}

fn minio_test_config_from(
    required: bool,
    endpoint: Option<&str>,
    bucket: Option<&str>,
) -> Result<Option<(String, String)>> {
    let endpoint = match endpoint {
        Some(endpoint) if !endpoint.trim().is_empty() => endpoint.trim().to_string(),
        Some(_) => bail!("UGOITE_MINIO_ENDPOINT must not be empty"),
        None if required => {
            bail!("UGOITE_MINIO_ENDPOINT is required when UGOITE_MINIO_REQUIRED is set")
        }
        None => return Ok(None),
    };
    let bucket = match bucket {
        Some(bucket) if !bucket.trim().is_empty() => bucket.trim().to_string(),
        Some(_) => bail!("UGOITE_MINIO_BUCKET must not be empty"),
        None if required => {
            bail!("UGOITE_MINIO_BUCKET is required when UGOITE_MINIO_REQUIRED is set")
        }
        None => "ugoite-ci".to_string(),
    };
    Ok(Some((endpoint, bucket)))
}

#[test]
fn minio_configuration_is_optional_without_the_release_opt_in() {
    assert_eq!(minio_test_config_from(false, None, None).unwrap(), None);
}

#[test]
fn minio_configuration_is_complete_when_release_opted_in() {
    assert!(minio_test_config_from(true, None, Some("ugoite-ci")).is_err());
    assert!(minio_test_config_from(true, Some("http://127.0.0.1:9000"), None).is_err());
    assert_eq!(
        minio_test_config_from(true, Some(" http://127.0.0.1:9000 "), Some(" ugoite-ci ")).unwrap(),
        Some(("http://127.0.0.1:9000".to_string(), "ugoite-ci".to_string()))
    );
}
