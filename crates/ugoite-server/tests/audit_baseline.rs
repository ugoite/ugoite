//! Temporary characterization of audit F02, not evidence that it is fixed.
use anyhow::Result;
use ugoite_iceberg::{service::UgoiteService, space};
use ugoite_server::AppState;
use uuid::Uuid;

#[tokio::test]
async fn audit_baseline_startup_rejects_complete_unclaimed_space() -> Result<()> {
    let root = tempfile::tempdir()?;
    let root_uri = root.path().to_string_lossy().into_owned();
    // This is the service operation used by CLI core Space creation. No test
    // owner is injected: portable Knowledge has no Node account binding yet.
    let service = UgoiteService::new(root_uri.clone())?;
    let space_id = service
        .ensure_operator_space_with_name(&format!("audit-{}", Uuid::now_v7()), "Portable Audit")
        .await?
        .space_id()
        .to_string();
    space::validate_complete_bootstrap(service.operator(), &space_id).await?;
    assert_eq!(service.list_space_ids().await?, vec![space_id.clone()]);
    let authorization_path = root
        .path()
        .join("spaces")
        .join(&space_id)
        .join("security/principals.json");
    assert_eq!(
        std::fs::read(&authorization_path).unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
    let metadata_before = service.get_space(&space_id).await?;
    // Identity test state is isolated from the filesystem Space. This covers
    // the real pre-listener startup method, not a listening HTTP server or
    // Passkey setup ceremony; those remain required in the F02 fix.
    let state = AppState::new_for_tests(root_uri)?;
    let error = state
        .initialize_node()
        .await
        .expect_err("baseline: complete ownerless Space currently prevents Node startup");
    let diagnostic = format!("{error:#}");
    assert!(diagnostic.contains("read Space authorization state"));
    assert!(diagnostic.contains("principals.json"));
    assert!(diagnostic.contains("NotFound"));
    // Desired behavior: startup succeeds while the unclaimed Space remains
    // inaccessible until regular initial setup establishes its owner.
    assert_eq!(service.get_space(&space_id).await?, metadata_before);
    assert_eq!(
        std::fs::read(&authorization_path).unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
    Ok(())
}
