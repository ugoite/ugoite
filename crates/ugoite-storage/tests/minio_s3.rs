use anyhow::{bail, Result};
use std::env;
use ugoite_storage::{
    operator_from_uri_with_endpoint, CasOutcome, CreateOutcome, OpendalPublicationStore,
    PublicationStore,
};
use uuid::Uuid;

/// Release-gated proof that the shared publication contract works against an
/// actual S3-compatible object store. Local development remains unchanged when
/// `UGOITE_MINIO_REQUIRED` is unset; CI supplies a disposable MinIO endpoint,
/// bucket, and credentials and opts into the required test explicitly.
#[tokio::test]
async fn minio_s3_proves_shared_conditional_storage_contract() -> Result<()> {
    let Some((endpoint, bucket)) = minio_test_config()? else {
        return Ok(());
    };
    let root = format!("ugoite/contract/{}", Uuid::now_v7());
    let uri = format!("s3://{bucket}/{root}");
    let operator = operator_from_uri_with_endpoint(&uri, Some(&endpoint))?;
    let store = OpendalPublicationStore::new(operator.clone());

    store.verify_contract().await?;
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
    let _ = operator.delete(key.as_str()).await;
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
