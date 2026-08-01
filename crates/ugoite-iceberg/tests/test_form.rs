mod common;
use common::setup_operator;
use ugoite_iceberg::form;
use ugoite_iceberg::space;

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
    let meeting = forms
        .iter()
        .find(|c| c.get("name").and_then(|v| v.as_str()) == Some("meeting"))
        .expect("meeting Form");
    assert!(meeting["sql_relation"].as_str().is_some());
    assert_eq!(meeting["fields"]["date"]["sql_column"], "field_100");
    assert_eq!(meeting["fields"]["summary"]["sql_column"], "field_101");

    Ok(())
}

#[tokio::test]
async fn sql_columns_follow_field_ids_across_case_collisions_and_renames() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "stable-sql-columns", "/tmp").await?;
    let ws_path = "spaces/stable-sql-columns";

    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "CaseFields",
            "fields": {
                "Status": {"type": "string"},
                "status": {"type": "string"}
            }
        }),
    )
    .await?;
    let before = form::get_form(&op, ws_path, "CaseFields").await?;
    let status_column = before["fields"]["Status"]["sql_column"]
        .as_str()
        .expect("Status SQL column")
        .to_string();
    let lowercase_status_column = before["fields"]["status"]["sql_column"]
        .as_str()
        .expect("status SQL column")
        .to_string();
    assert_ne!(status_column, lowercase_status_column);

    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "CaseFields",
            "fields": {
                "RenamedStatus": {"id": 100, "type": "string"},
                "status": {"id": 101, "type": "string"}
            }
        }),
    )
    .await?;
    let after = form::get_form(&op, ws_path, "CaseFields").await?;
    assert_eq!(
        after["fields"]["RenamedStatus"]["sql_column"],
        status_column
    );
    assert_eq!(
        after["fields"]["status"]["sql_column"],
        lowercase_status_column
    );
    Ok(())
}

#[tokio::test]
async fn idempotent_form_upsert_accepts_explicit_default_requiredness() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "idempotent-form", "/tmp").await?;
    let ws_path = "spaces/idempotent-form";

    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Entry",
            "fields": {"Body": {"type": "markdown"}},
        }),
    )
    .await?;
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Entry",
            "version": 1,
            "template": "# Entry\\n\\n## Body\\n",
            "fields": {"Body": {"type": "markdown", "required": false}},
        }),
    )
    .await?;

    Ok(())
}

#[tokio::test]
async fn existing_form_accepts_a_time_column() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "time-form", "/tmp").await?;
    let ws_path = "spaces/time-form";

    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Entry",
            "fields": {"Body": {"type": "markdown"}},
        }),
    )
    .await?;
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Entry",
            "fields": {
                "Body": {"type": "markdown"},
                "time": {"type": "time"},
            },
        }),
    )
    .await?;

    let entry = form::get_form(&op, ws_path, "Entry").await?;
    assert_eq!(entry["fields"]["time"]["type"], "time");
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
    assert!(types.contains(&"asset_reference".to_string()));
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

    let stored = form::get_form(&op, ws_path, "Task").await?;
    let project = form::get_form(&op, ws_path, "Project").await?;
    let target_form = stored["fields"]["Project"]["target_form"]
        .as_str()
        .expect("stable target Form ID");
    assert_eq!(
        target_form,
        project["id"].as_str().expect("Project Form ID")
    );

    Ok(())
}

#[tokio::test]
async fn self_reference_reupsert_preserves_the_persisted_form_id() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "self-reference-form", "/tmp").await?;
    let ws_path = "spaces/self-reference-form";
    let definition = serde_json::json!({
        "name": "Task",
        "fields": {
            "Parent": {"type": "row_reference", "target_form": "Task"}
        }
    });
    form::upsert_form(&op, ws_path, &definition).await?;
    let first = form::get_form(&op, ws_path, "Task").await?;
    let stable_id = first["id"].as_str().unwrap().to_string();

    form::upsert_form(&op, ws_path, &definition).await?;
    let second = form::get_form(&op, ws_path, "Task").await?;
    assert_eq!(second["id"].as_str(), Some(stable_id.as_str()));
    assert_eq!(
        second["fields"]["Parent"]["target_form"].as_str(),
        Some(stable_id.as_str())
    );
    Ok(())
}

#[tokio::test]
async fn unknown_uuid_reference_target_is_rejected() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "unknown-reference-form", "/tmp").await?;
    let ws_path = "spaces/unknown-reference-form";
    let result = form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Task",
            "fields": {
                "Parent": {
                    "type": "row_reference",
                    "target_form": "00000000-0000-0000-0000-000000000099"
                }
            }
        }),
    )
    .await;
    assert!(result.unwrap_err().to_string().contains("not found"));
    Ok(())
}
