mod common;
use _ugoite_core::asset;
use _ugoite_core::space;
use common::setup_operator;
#[cfg(unix)]
use opendal::services::Fs;
#[cfg(unix)]
use opendal::Operator;
#[cfg(unix)]
use tempfile::tempdir;

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
    let op = Operator::new(builder)?.finish();

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

    Ok(())
}
