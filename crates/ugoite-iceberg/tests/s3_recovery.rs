use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::env;
use ugoite_iceberg::{authorization::Authorizer, service::UgoiteService};
use uuid::Uuid;

/// Optional end-to-end proof through the same application service used by the
/// server. Set UGOITE_S3_TEST_REQUIRED=1 to make missing configuration fail.
#[tokio::test]
async fn s3_backed_space_survives_service_reopen_and_revert() -> Result<()> {
    let Some((endpoint, bucket)) = s3_test_config()? else {
        return Ok(());
    };
    let prefix = format!("ugoite/l12/recovery/{}", Uuid::now_v7());
    let root_uri = format!("s3://{bucket}/{prefix}");
    let owner = Uuid::now_v7();
    let space_slug = format!("recovery-{}", Uuid::now_v7().simple());

    let service = open_verified_service(&root_uri, &endpoint, &space_slug).await?;
    let space_id = service
        .create_space_for_principal(&space_slug, owner, "MinIO recovery test")
        .await?
        .to_string();
    service
        .upsert_form(
            &space_id,
            &json!({
                "name": "RecoveryNote",
                "fields": {"Body": {"type": "markdown", "required": true}}
            }),
        )
        .await?;

    let mut fields = BTreeMap::new();
    fields.insert("Body".to_string(), json!("before restart"));
    service
        .create_structured_entry_authorized_for_principals(
            &space_id,
            "recovery-entry",
            "RecoveryNote".to_string(),
            Vec::new(),
            fields,
            BTreeMap::new(),
            "MinIO recovery test",
            &[owner],
        )
        .await?;

    let asset = service
        .save_asset(&space_id, "recovery.txt", b"persisted asset")
        .await?;
    let mut updated_fields = BTreeMap::new();
    updated_fields.insert("Body".to_string(), json!("after update"));
    let updated = service
        .update_structured_entry_authorized_for_principals(
            &space_id,
            "recovery-entry",
            None,
            None,
            updated_fields,
            BTreeMap::new(),
            None,
            "MinIO recovery test",
            &[owner],
        )
        .await?;
    let update_change_id = updated
        .get("change_id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("updated Entry omitted its Change ID"))?
        .to_string();
    let reverted = service
        .revert_change(
            &space_id,
            &update_change_id,
            &owner.to_string(),
            None,
            Some("Recover the pre-update value"),
        )
        .await?;
    let revert_change_id = reverted
        .get("change_id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("revert omitted its Change ID"))?
        .to_string();
    drop(service);

    // A newly constructed service/operator must recover the same Space and its
    // append-only Change history from the remote store.
    let reopened = open_verified_service(&root_uri, &endpoint, &space_slug).await?;
    let recovery = reopened.open_space(&space_id).await?;
    assert_eq!(recovery["space_id"], space_id);
    assert!(reopened.list_space_ids().await?.contains(&space_slug));
    assert_eq!(
        reopened.get_form(&space_id, "RecoveryNote").await?["name"],
        "RecoveryNote"
    );
    assert_eq!(
        reopened.get_entry(&space_id, "recovery-entry").await?["fields"]["Body"],
        "before restart"
    );
    assert_eq!(
        reopened.read_asset(&space_id, &asset.asset_id).await?.bytes,
        b"persisted asset"
    );

    let changes = reopened.list_changes(&space_id).await?;
    let changes = changes
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Change history was not an array"))?;
    assert!(changes.iter().any(|change| {
        change.get("change_id").and_then(Value::as_str) == Some(update_change_id.as_str())
    }));
    assert!(changes.iter().any(|change| {
        change.get("change_id").and_then(Value::as_str) == Some(revert_change_id.as_str())
            && change
                .pointer("/change/reverts_change_id")
                .and_then(Value::as_str)
                == Some(update_change_id.as_str())
    }));
    Ok(())
}

async fn open_verified_service(
    root_uri: &str,
    endpoint: &str,
    space_slug: &str,
) -> Result<UgoiteService> {
    let service = UgoiteService::new_with_endpoint(root_uri, Some(endpoint))?;
    let authorizer = Authorizer::new(service.operator().clone());
    authorizer.ensure_authoritative_mutation_contract()?;
    authorizer.verify_authoritative_storage(space_slug).await?;
    Ok(service)
}

fn s3_test_config() -> Result<Option<(String, String)>> {
    let required = env::var_os("UGOITE_S3_TEST_REQUIRED").is_some();
    let endpoint = env::var("UGOITE_S3_TEST_ENDPOINT").ok();
    let bucket = env::var("UGOITE_S3_TEST_BUCKET").ok();
    match (endpoint, bucket) {
        (None, None) if required => bail!("S3 endpoint and bucket are required for this test"),
        (None, None) => Ok(None),
        (Some(endpoint), Some(bucket))
            if !endpoint.trim().is_empty() && !bucket.trim().is_empty() =>
        {
            Ok(Some((
                endpoint.trim().to_string(),
                bucket.trim().to_string(),
            )))
        }
        (Some(_), Some(_)) => bail!("S3 test endpoint and bucket must not be empty"),
        _ => bail!("S3 test endpoint and bucket must be configured together"),
    }
}
