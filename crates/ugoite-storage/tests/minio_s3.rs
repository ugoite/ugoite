use anyhow::{bail, Result};
use std::env;
use ugoite_storage::{
    catalog_write_mode, operator_from_uri_with_endpoint, verify_storage_contract, CasOutcome,
    CatalogWriteMode, CreateOutcome, OpendalPublicationStore, PublicationStore,
    StorageContractStatus,
};
use uuid::Uuid;

/// Release-gated proof that the supported non-local path is based on the
/// OpenDAL behavior contract rather than the provider name. The test is a
/// no-op for ordinary local development; CI supplies a disposable MinIO
/// endpoint and credentials.
#[tokio::test]
async fn minio_s3_proves_shared_conditional_storage_contract() -> Result<()> {
    let Some(endpoint) = env::var_os("UGOITE_MINIO_ENDPOINT") else {
        return Ok(());
    };
    let endpoint = endpoint.to_string_lossy();
    let bucket = env::var("UGOITE_MINIO_BUCKET").unwrap_or_else(|_| "ugoite-ci".to_string());
    let root = format!("ugoite/contract/{}", Uuid::now_v7());
    let uri = format!("s3://{bucket}/{root}");
    let operator = operator_from_uri_with_endpoint(&uri, Some(endpoint.as_ref()))?;

    let contract_status = verify_storage_contract(&operator).await;
    assert_eq!(
        contract_status,
        StorageContractStatus::Verified,
        "MinIO storage contract verification failed: {contract_status:?}"
    );
    assert_eq!(
        catalog_write_mode(&operator),
        CatalogWriteMode::SharedVerified
    );

    let store = OpendalPublicationStore::new(operator.clone());
    let key = ugoite_storage::SpaceKey::parse(&format!("contract/{}.json", Uuid::now_v7()))?;
    assert_eq!(
        store.create(&key, b"first".to_vec()).await?,
        CreateOutcome::Created
    );
    let exact = store.load(&key).await?.expect("contract object exists");
    let left = store.compare_and_swap(&key, &exact.revision, b"left".to_vec());
    let right = store.compare_and_swap(&key, &exact.revision, b"right".to_vec());
    let (left, right) = tokio::join!(left, right);
    let winners = [left?, right?]
        .into_iter()
        .filter(|outcome| *outcome == CasOutcome::Replaced)
        .count();
    if winners != 1 {
        bail!("MinIO CAS race produced {winners} winners, expected exactly one");
    }

    let final_object = store.load(&key).await?.expect("CAS winner exists");
    assert!(final_object.bytes == b"left" || final_object.bytes == b"right");
    operator.delete(key.as_str()).await?;
    Ok(())
}
