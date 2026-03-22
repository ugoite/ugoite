mod common;
use _ugoite_core::form;
use _ugoite_core::space;
use common::setup_operator;

#[tokio::test]
/// REQ-FORM-008
async fn test_form_req_form_008_reject_user_form_name() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-user-meta-form", "/tmp").await?;
    let ws_path = "spaces/test-user-meta-form";

    let form_def = serde_json::json!({
        "name": "User",
        "fields": {
            "DisplayName": {"type": "string"}
        }
    });

    let result = form::upsert_form(&op, ws_path, &form_def).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("reserved"));

    Ok(())
}

#[tokio::test]
/// REQ-FORM-008
async fn test_form_req_form_008_reject_usergroup_form_name() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-usergroup-meta-form", "/tmp").await?;
    let ws_path = "spaces/test-usergroup-meta-form";

    let form_def = serde_json::json!({
        "name": "UserGroup",
        "fields": {
            "Name": {"type": "string"}
        }
    });

    let result = form::upsert_form(&op, ws_path, &form_def).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("reserved"));

    Ok(())
}
