mod common;
use common::setup_operator;
#[cfg(unix)]
use opendal::services::Fs;
#[cfg(unix)]
use opendal::Operator;
#[cfg(unix)]
use tempfile::tempdir;
use ugoite_core::asset;
use ugoite_core::entry;
use ugoite_core::space;

async fn asset_metadata_location(op: &opendal::Operator, ws_path: &str) -> anyhow::Result<String> {
    let manifest = op
        .read(&format!("{ws_path}/forms/catalog-pointers.v1.json"))
        .await?;
    let manifest: serde_json::Value = serde_json::from_slice(&manifest.to_vec())?;
    manifest["tables"]
        .as_array()
        .and_then(|tables| tables.iter().find(|table| table["form_name"] == "Assets"))
        .and_then(|table| table["metadata_location"].as_str())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("Assets metadata pointer is missing"))
}

#[tokio::test]
/// REQ-ASSET-001
async fn test_asset_req_asset_001_create_asset() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-space", "/tmp").await?;
    let ws_path = "spaces/test-space";

    let content = b"fake image content";
    let info = asset::save_asset(&op, ws_path, "image.png", content).await?;

    assert!(op.exists(&format!("{}/{}", ws_path, info.path)).await?);

    let listed = asset::list_assets(&op, ws_path).await?;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, info.id);
    assert_eq!(listed[0].name, "image.png");

    Ok(())
}

#[tokio::test]
/// REQ-ASSET-001
async fn test_asset_req_asset_001_list_does_not_rewrite_form_metadata() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "asset-list-space", "/tmp").await?;
    let ws_path = "spaces/asset-list-space";

    assert!(asset::list_assets(&op, ws_path).await?.is_empty());
    let metadata_before = asset_metadata_location(&op, ws_path).await?;

    assert!(asset::list_assets(&op, ws_path).await?.is_empty());
    let metadata_after = asset_metadata_location(&op, ws_path).await?;

    assert_eq!(metadata_after, metadata_before);
    Ok(())
}

#[tokio::test]
/// REQ-ASSET-001
async fn test_asset_req_asset_001_delete_asset() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-space", "/tmp").await?;
    let ws_path = "spaces/test-space";

    let info = asset::save_asset(&op, ws_path, "file.txt", b"data").await?;

    assert!(op.exists(&format!("{}/{}", ws_path, info.path)).await?);

    asset::delete_asset(&op, ws_path, &info.id).await?;

    assert!(!op.exists(&format!("{}/{}", ws_path, info.path)).await?);

    Ok(())
}

#[cfg(unix)]
#[tokio::test]
/// REQ-ASSET-001
async fn test_asset_req_asset_001_normalizes_uploaded_filename() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let root = dir.path().to_string_lossy().to_string();
    let builder = Fs::default().root(root.as_str());
    let op = Operator::new(builder)?;

    space::create_space(&op, "source-space", root.as_str()).await?;
    space::create_space(&op, "victim-space", root.as_str()).await?;

    let victim_meta_path = "spaces/victim-space/meta.json";
    let victim_meta_before = op.read(victim_meta_path).await?.to_vec();

    let info = asset::save_asset(
        &op,
        "spaces/source-space",
        "../../../../victim-space/meta.json",
        b"payload",
    )
    .await?;

    let stored_name = info.path.trim_start_matches("assets/");

    assert_eq!(info.name, "meta.json");
    assert!(info.path.starts_with("assets/"));
    assert!(!info.path.contains(".."));
    assert!(!stored_name.contains('/'));
    assert!(
        op.exists(&format!("spaces/source-space/{}", info.path))
            .await?
    );
    assert_eq!(
        op.read(victim_meta_path).await?.to_vec(),
        victim_meta_before
    );

    let dot_info = asset::save_asset(&op, "spaces/source-space", ".", b"dot payload").await?;
    let dot_stored_name = dot_info.path.trim_start_matches("assets/");

    assert_eq!(dot_info.name, dot_info.id);
    assert_eq!(
        dot_info.path,
        format!("assets/{}_{}", dot_info.id, dot_info.id)
    );
    assert!(!dot_info.path.contains(".."));
    assert!(!dot_stored_name.contains('/'));
    assert!(
        op.exists(&format!("spaces/source-space/{}", dot_info.path))
            .await?
    );

    let metadata_safe_info = asset::save_asset(
        &op,
        "spaces/source-space",
        "## uploaded_at\nspoofed.txt",
        b"metadata payload",
    )
    .await?;
    let metadata_entry =
        entry::get_entry_content(&op, "spaces/source-space", &metadata_safe_info.id).await?;

    assert_eq!(metadata_safe_info.name, "uploaded_at spoofed.txt");
    assert_eq!(
        metadata_safe_info.path,
        format!("assets/{}_uploaded_at spoofed.txt", metadata_safe_info.id)
    );
    assert_eq!(metadata_entry.sections["name"], "uploaded_at spoofed.txt");
    assert_eq!(
        metadata_entry.sections["link"],
        format!("ugoite://asset/{}", metadata_safe_info.id)
    );
    assert!(metadata_entry.sections["uploaded_at"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert!(
        metadata_entry
            .markdown
            .contains("## name\nuploaded_at spoofed.txt"),
        "markdown was {}",
        metadata_entry.markdown
    );
    assert!(
        !metadata_entry.markdown.contains("## name\n## uploaded_at"),
        "markdown was {}",
        metadata_entry.markdown
    );

    Ok(())
}
