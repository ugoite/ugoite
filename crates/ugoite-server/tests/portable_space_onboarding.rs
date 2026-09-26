use anyhow::Result;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use std::path::Path;
use tower::ServiceExt;
use ugoite_iceberg::{service::SpaceOnboardingState, service::UgoiteService};
use ugoite_server::{app, AppState};
use uuid::Uuid;

fn copy_tree(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else {
            std::fs::copy(source_path, destination_path)?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn prefix_only_portable_space_allows_startup_but_not_anonymous_api_access() -> Result<()> {
    let source_root = tempfile::tempdir()?;
    let source_service = UgoiteService::new(source_root.path().to_string_lossy())?;
    let slug = format!("portable-{}", Uuid::now_v7());
    let space_id = source_service
        .ensure_operator_space_with_name(&slug, "Portable Space")
        .await?
        .space_id()
        .to_string();
    let history_before = source_service.list_changes(&space_id).await?;
    let source_prefix = source_root.path().join("spaces").join(&space_id);
    let destination_root = tempfile::tempdir()?;
    let destination_prefix = destination_root.path().join("spaces").join(&space_id);
    copy_tree(&source_prefix, &destination_prefix)?;

    // Only the Space prefix was transferred. Neither slug-claim records nor
    // Node control state/secrets from the source are copied.
    let state = AppState::new_for_tests(destination_root.path().to_string_lossy())?;
    state.initialize_node().await?;
    let destination_service = UgoiteService::new(destination_root.path().to_string_lossy())?;
    assert_eq!(
        destination_service
            .classify_space_for_node_onboarding(&space_id)
            .await?,
        SpaceOnboardingState::PortableUnclaimed {
            uid: Uuid::parse_str(&space_id)?,
            slug,
        }
    );
    assert_eq!(
        destination_service.list_changes(&space_id).await?,
        history_before,
        "prefix-only transfer must preserve the existing Change history"
    );
    assert!(!destination_prefix.join("security/principals.json").exists());

    let response = app(state.clone())
        .oneshot(
            Request::builder()
                .uri(format!("/spaces/{space_id}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::LOCKED);
    let response_body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(response_body["code"], "NODE_UNINITIALIZED");
    let write_response = app(state)
        .oneshot(
            Request::post(format!("/spaces/{space_id}/forms"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "name": "AnonymousWrite",
                        "version": 1,
                        "fields": {"Subject": {"type": "string", "required": true}}
                    })
                    .to_string(),
                ))?,
        )
        .await?;
    assert_eq!(write_response.status(), StatusCode::LOCKED);
    let write_body: serde_json::Value =
        serde_json::from_slice(&to_bytes(write_response.into_body(), usize::MAX).await?)?;
    assert_eq!(write_body["code"], "NODE_UNINITIALIZED");
    Ok(())
}

#[tokio::test]
async fn malformed_authorization_state_is_not_classified_as_unclaimed() -> Result<()> {
    let root = tempfile::tempdir()?;
    let root_uri = root.path().to_string_lossy().into_owned();
    let service = UgoiteService::new(root_uri.clone())?;
    let space_id = service
        .ensure_operator_space_with_name(
            &format!("corrupt-{}", Uuid::now_v7()),
            "Corrupt ACL fixture",
        )
        .await?
        .space_id()
        .to_string();
    std::fs::write(
        root.path()
            .join("spaces")
            .join(&space_id)
            .join("security/principals.json"),
        b"{not-json",
    )?;
    let error = service
        .classify_space_for_node_onboarding(&space_id)
        .await
        .expect_err("malformed authorization state must fail closed");
    assert!(format!("{error:#}").contains("decode Space authorization state"));
    Ok(())
}
