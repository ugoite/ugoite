//! Phase 6 service-boundary coverage for server and CLI adapters.

use anyhow::Result;
use ugoite_core::service::UgoiteService;

#[tokio::test]
async fn test_service_boundary_covers_primary_adapter_operations() -> Result<()> {
    let service = UgoiteService::new("memory://core-service-boundary")?;

    service.create_space("demo").await?;
    let spaces = service.list_space_ids().await?;
    assert_eq!(spaces, vec!["demo"]);

    service
        .upsert_form(
            "demo",
            &serde_json::json!({
                "name": "Note",
                "fields": {
                    "Body": {"type": "markdown"}
                }
            }),
        )
        .await?;
    assert_eq!(service.get_form("demo", "Note").await?["name"], "Note");

    let created = service
        .create_entry(
            "demo",
            "first",
            "---\nform: Note\n---\n# First\n\n## Body\nhello service",
            "test",
        )
        .await?;
    assert_eq!(created["id"], "first");

    let entries = service.list_entries("demo").await?;
    assert_eq!(entries.len(), 1);

    let search = service.search_entries("demo", "service").await?;
    assert_eq!(search.len(), 1);
    assert_eq!(search[0].id, "first");

    let asset = service.save_asset("demo", "hello.txt", b"hello").await?;
    let assets = service.list_assets("demo").await?;
    assert!(assets.iter().any(|item| item.id == asset.id));

    service.delete_asset("demo", &asset.id).await?;
    assert!(!service
        .list_assets("demo")
        .await?
        .iter()
        .any(|item| item.id == asset.id));

    Ok(())
}
