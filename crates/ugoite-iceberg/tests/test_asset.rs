mod common;
use common::setup_operator;
use ugoite_iceberg::asset;
use ugoite_iceberg::entry;
use ugoite_iceberg::form;
use ugoite_iceberg::index;
use ugoite_iceberg::integrity::FakeIntegrityProvider;
use ugoite_iceberg::space;

#[tokio::test]
async fn upload_returns_a_typed_reference_without_creating_an_entry() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "asset-space", "/tmp").await?;
    let ws_path = "spaces/asset-space";
    let reference = asset::save_asset(&op, ws_path, "image.png", b"bytes").await?;

    assert_eq!(reference.name, "image.png");
    assert_eq!(reference.size_bytes, 5);
    assert_eq!(reference.media_type, "application/octet-stream");
    assert!(
        op.exists(&format!("{ws_path}/assets/{}", reference.asset_id))
            .await?
    );
    assert!(ugoite_iceberg::entry::list_entries(&op, ws_path)
        .await?
        .is_empty());
    Ok(())
}

#[tokio::test]
async fn read_and_delete_use_the_exact_asset_id_key() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "asset-exact-key", "/tmp").await?;
    let ws_path = "spaces/asset-exact-key";
    let reference = asset::save_asset(&op, ws_path, "file.txt", b"data").await?;

    let content = asset::read_asset(&op, ws_path, &reference.asset_id).await?;
    assert_eq!(content.reference.asset_id, reference.asset_id);
    assert_eq!(content.bytes, b"data");

    asset::delete_asset(&op, ws_path, &reference.asset_id).await?;
    assert!(
        !op.exists(&format!("{ws_path}/assets/{}", reference.asset_id))
            .await?
    );
    Ok(())
}

#[tokio::test]
async fn upload_normalizes_the_display_name_without_leaking_storage_details() -> anyhow::Result<()>
{
    let op = setup_operator()?;
    space::create_space(&op, "asset-name", "/tmp").await?;
    let ws_path = "spaces/asset-name";
    let reference = asset::save_asset(
        &op,
        ws_path,
        "../../## uploaded_at\nspoofed.txt",
        b"payload",
    )
    .await?;

    assert_eq!(reference.name, "uploaded_at spoofed.txt");
    let encoded = serde_json::to_string(&reference)?;
    assert!(!encoded.contains("assets/"));
    assert!(!encoded.contains("ugoite://"));
    Ok(())
}

#[tokio::test]
async fn typed_form_asset_references_round_trip_and_guard_deletion() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "asset-form", "/tmp").await?;
    let ws_path = "spaces/asset-form";
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Media",
            "fields": {
                "Attachment": {"type": "asset_reference"},
                "Attachments": {
                    "type": "list",
                    "items": {"type": "asset_reference"}
                }
            }
        }),
    )
    .await?;
    let reference = asset::save_asset(&op, ws_path, "image.png", b"bytes").await?;
    let reference_json = serde_json::to_string(&reference)?;
    let content = format!(
        "---\nform: Media\nAttachment: {reference_json}\nAttachments: [{reference_json}]\n---\n# Photo"
    );
    entry::create_entry(
        &op,
        ws_path,
        "media-1",
        &content,
        "author",
        &FakeIntegrityProvider,
    )
    .await?;

    let entries = entry::list_entries(&op, ws_path).await?;
    assert_eq!(
        entries[0]["properties"]["Attachment"]["asset_id"],
        reference.asset_id
    );
    assert_eq!(
        entries[0]["properties"]["Attachments"][0]["asset_id"],
        reference.asset_id
    );
    let scalar_fields = ["Attachment".to_string()];
    let list_fields = ["Attachments".to_string()];
    assert!(
        index::current_asset_reference_exists(
            &op,
            ws_path,
            "Media",
            &scalar_fields,
            &list_fields,
            &reference.asset_id,
        )
        .await?
    );
    let error = asset::delete_asset(&op, ws_path, &reference.asset_id)
        .await
        .expect_err("current typed Form values must guard the asset");
    assert!(error.to_string().contains("referenced"));
    Ok(())
}
