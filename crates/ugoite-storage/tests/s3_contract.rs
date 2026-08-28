use anyhow::{bail, Result};
use std::env;
use ugoite_storage::{operator_from_uri_with_endpoint, OpendalPublicationStore};
use uuid::Uuid;

/// Verify the publication contract against an explicitly configured S3
/// deployment backend. The test is intentionally a no-op unless both test
/// configuration variables are supplied.
#[tokio::test]
async fn s3_backend_satisfies_publication_contract() -> Result<()> {
    let Some((endpoint, bucket)) = s3_test_config()? else {
        return Ok(());
    };
    let uri = format!("s3://{bucket}/ugoite/contract/{}", Uuid::now_v7());
    let operator = operator_from_uri_with_endpoint(&uri, Some(&endpoint))?;
    OpendalPublicationStore::new(operator)
        .verify_contract()
        .await
        .map_err(|error| anyhow::anyhow!(error))?;
    Ok(())
}

fn s3_test_config() -> Result<Option<(String, String)>> {
    match (
        env::var("UGOITE_S3_TEST_ENDPOINT").ok(),
        env::var("UGOITE_S3_TEST_BUCKET").ok(),
    ) {
        (None, None) => Ok(None),
        (Some(endpoint), Some(bucket)) => {
            let endpoint = endpoint.trim();
            if endpoint.is_empty() {
                bail!("UGOITE_S3_TEST_ENDPOINT must not be empty");
            }
            let bucket = bucket.trim();
            if bucket.is_empty() {
                bail!("UGOITE_S3_TEST_BUCKET must not be empty");
            }
            Ok(Some((endpoint.to_owned(), bucket.to_owned())))
        }
        (Some(_), None) => {
            bail!("UGOITE_S3_TEST_BUCKET is required with UGOITE_S3_TEST_ENDPOINT")
        }
        (None, Some(_)) => {
            bail!("UGOITE_S3_TEST_ENDPOINT is required with UGOITE_S3_TEST_BUCKET")
        }
    }
}
