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
    opendal::install_default();
    let uri = format!("s3://{bucket}/ugoite/contract/{}", Uuid::now_v7());
    let operator = operator_from_uri_with_endpoint(&uri, Some(&endpoint))?;
    OpendalPublicationStore::new(operator)
        .verify_contract()
        .await
        .map_err(|error| anyhow::anyhow!(error))?;
    Ok(())
}

fn s3_test_config() -> Result<Option<(String, String)>> {
    let required = s3_test_required()?;
    match (
        env::var("UGOITE_S3_TEST_ENDPOINT").ok(),
        env::var("UGOITE_S3_TEST_BUCKET").ok(),
    ) {
        (None, None) if required => {
            bail!("UGOITE_S3_TEST_ENDPOINT and UGOITE_S3_TEST_BUCKET are required")
        }
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

/// Explicit opt-in parser for `UGOITE_S3_TEST_REQUIRED` (issue #3262).
/// Unset or empty means the S3 backend stays optional; `1`/`true`/`yes`/`on`
/// require it, `0`/`false`/`no`/`off` skip it, and anything else fails with
/// a diagnostic instead of silently requiring (or skipping) the backend.
fn parse_s3_test_required(raw: Option<&str>) -> Result<bool> {
    match raw
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        None | Some("") => Ok(false),
        Some("1" | "true" | "yes" | "on") => Ok(true),
        Some("0" | "false" | "no" | "off") => Ok(false),
        Some(other) => bail!(
            "UGOITE_S3_TEST_REQUIRED has an unsupported value {other:?}; expected one of \
             1/true/yes/on to require S3 or 0/false/no/off (or unset) to skip it"
        ),
    }
}

fn s3_test_required() -> Result<bool> {
    parse_s3_test_required(env::var("UGOITE_S3_TEST_REQUIRED").ok().as_deref())
}

#[test]
fn s3_test_required_flag_parses_explicitly() {
    assert!(!parse_s3_test_required(None).unwrap());
    assert!(!parse_s3_test_required(Some("")).unwrap());
    for value in ["0", "false", "FALSE", "no", "off", " 0 "] {
        assert!(!parse_s3_test_required(Some(value)).unwrap(), "{value}");
    }
    for value in ["1", "true", "TRUE", "yes", "on", " 1 "] {
        assert!(parse_s3_test_required(Some(value)).unwrap(), "{value}");
    }
    for value in ["2", "required", "yes please"] {
        let error = parse_s3_test_required(Some(value)).unwrap_err();
        assert!(
            error.to_string().contains("UGOITE_S3_TEST_REQUIRED"),
            "{error:?}"
        );
    }
}
