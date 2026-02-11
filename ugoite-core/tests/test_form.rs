mod common;
use _ugoite_core::form;
use _ugoite_core::space;
use common::setup_operator;

#[tokio::test]
/// REQ-FORM-002
async fn test_form_req_form_002_upsert_and_list_forms() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-space", "/tmp").await?;
    let ws_path = "spaces/test-space";

    let form_def = r#"{
        "name": "meeting",
        "description": "Meeting entries",
        "fields": [
            {"name": "date", "type": "date"},
            {"name": "summary", "type": "markdown"}
        ]
    }"#;

    let form_value: serde_json::Value = serde_json::from_str(form_def)?;
    form::upsert_form(&op, ws_path, &form_value).await?;

    let forms = form::list_forms(&op, ws_path).await?;
    assert!(forms
        .iter()
        .any(|c| c.get("name").and_then(|v| v.as_str()) == Some("meeting")));

    Ok(())
}

#[tokio::test]
/// REQ-FORM-001
async fn test_form_req_form_001_list_column_types() -> anyhow::Result<()> {
    let types = form::list_column_types().await?;
    assert!(types.contains(&"string".to_string()));
    assert!(types.contains(&"markdown".to_string()));
    assert!(types.contains(&"number".to_string()));
    assert!(types.contains(&"double".to_string()));
    assert!(types.contains(&"float".to_string()));
    assert!(types.contains(&"integer".to_string()));
    assert!(types.contains(&"long".to_string()));
    assert!(types.contains(&"boolean".to_string()));
    assert!(types.contains(&"date".to_string()));
    assert!(types.contains(&"time".to_string()));
    assert!(types.contains(&"timestamp".to_string()));
    assert!(types.contains(&"timestamp_tz".to_string()));
    assert!(types.contains(&"timestamp_ns".to_string()));
    assert!(types.contains(&"timestamp_tz_ns".to_string()));
    assert!(types.contains(&"uuid".to_string()));
    assert!(types.contains(&"row_reference".to_string()));
    assert!(types.contains(&"binary".to_string()));
    assert!(types.contains(&"list".to_string()));
    Ok(())
}

#[tokio::test]
/// REQ-FORM-005
async fn test_form_req_form_005_reject_reserved_metadata_columns() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-meta-cols", "/tmp").await?;
    let ws_path = "spaces/test-meta-cols";

    let form_def = serde_json::json!({
        "name": "BadForm",
        "fields": {
            "title": {"type": "string"}
        }
    });

    let result = form::upsert_form(&op, ws_path, &form_def).await;
    assert!(result.is_err());
    let message = result.unwrap_err().to_string();
    assert!(message.contains("reserved"));

    Ok(())
}

#[tokio::test]
/// REQ-FORM-006
async fn test_form_req_form_006_reject_reserved_metadata_form() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-meta-form", "/tmp").await?;
    let ws_path = "spaces/test-meta-form";

    let form_def = serde_json::json!({
        "name": "SQL",
        "fields": {
            "sql": {"type": "string"}
        }
    });

    let result = form::upsert_form(&op, ws_path, &form_def).await;
    assert!(result.is_err());
    let message = result.unwrap_err().to_string();
    assert!(message.contains("reserved"));

    Ok(())
}

#[tokio::test]
/// REQ-FORM-007
async fn test_form_req_form_007_row_reference_requires_target() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-row-ref", "/tmp").await?;
    let ws_path = "spaces/test-row-ref";

    let base_form = serde_json::json!({
        "name": "Project",
        "fields": {
            "Name": {"type": "string"}
        }
    });
    form::upsert_form(&op, ws_path, &base_form).await?;

    let invalid_form = serde_json::json!({
        "name": "Task",
        "fields": {
            "Project": {"type": "row_reference"}
        }
    });
    let result = form::upsert_form(&op, ws_path, &invalid_form).await;
    assert!(result.is_err());
    let message = result.unwrap_err().to_string();
    assert!(message.contains("target_form"));

    let valid_form = serde_json::json!({
        "name": "Task",
        "fields": {
            "Project": {"type": "row_reference", "target_form": "Project"}
        }
    });
    form::upsert_form(&op, ws_path, &valid_form).await?;

    Ok(())
}
