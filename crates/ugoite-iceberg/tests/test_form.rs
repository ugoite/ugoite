mod common;
use common::setup_operator;
use ugoite_core::error::{AppError, ErrorCode};
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
async fn form_upsert_receipt_tracks_create_evolve_noop_and_concurrent_writers() -> anyhow::Result<()>
{
    let op = setup_operator()?;
    space::create_space(&op, "form-receipt-outcome", "/tmp").await?;
    let ws_path = "spaces/form-receipt-outcome";
    let initial = serde_json::json!({
        "name": "ReceiptForm",
        "fields": {"Subject": {"type": "string", "required": true}}
    });

    let workspace = ugoite_iceberg::iceberg_store::native_workspace(&op, ws_path).await?;
    let changes_before_create = workspace.list_changes().await?;
    let created = form::upsert_form_result(&op, ws_path, &initial).await?;
    assert!(created.applied);
    assert_eq!(created.form_version.get(), 1);
    let persisted = form::get_form(&op, ws_path, "ReceiptForm").await?;
    let created_form_id = created.form_id.to_string();
    assert_eq!(persisted["id"].as_str(), Some(created_form_id.as_str()));
    assert_eq!(
        persisted["version"].as_u64(),
        Some(u64::from(created.form_version.get()))
    );
    let changes_after_create = workspace.list_changes().await?;
    assert_eq!(changes_after_create.len(), changes_before_create.len() + 1);
    let created_change = changes_after_create
        .iter()
        .find(|change| Some(change.change_id.as_str()) == created.change_id.as_deref())
        .expect("the receipt identifies the created Form Change");
    assert_eq!(
        created.change_id.as_deref(),
        Some(created_change.change_id.as_str())
    );

    let noop = form::upsert_form_result(&op, ws_path, &initial).await?;
    assert!(!noop.applied);
    assert_eq!(noop.form_id, created.form_id);
    assert_eq!(noop.form_version, created.form_version);
    assert_eq!(noop.change_id, None);
    assert_eq!(
        workspace.list_changes().await?.len(),
        changes_after_create.len()
    );

    let evolved = serde_json::json!({
        "name": "ReceiptForm",
        "fields": {
            "Subject": {"type": "string", "required": true},
            "Body": {"type": "markdown"}
        }
    });
    let evolution = form::upsert_form_result(&op, ws_path, &evolved).await?;
    assert!(evolution.applied);
    assert_eq!(evolution.form_id, created.form_id);
    assert_eq!(evolution.form_version.get(), 2);
    let changes_after_evolution = workspace.list_changes().await?;
    assert_eq!(
        changes_after_evolution.len(),
        changes_after_create.len() + 1
    );
    assert_eq!(
        evolution.change_id.as_deref(),
        Some(changes_after_evolution.last().unwrap().change_id.as_str())
    );

    // Start two distinct same-version metadata edits together. Whether the
    // storage serializes them or one must re-evaluate after a publication
    // collision, each receipt must identify its own committed Change. Both
    // payloads retain the full current field map, as required by Form upsert.
    let with_alpha = serde_json::json!({
        "name": "ReceiptForm",
        "fields": {
            "Subject": {"type": "string", "required": true, "label": "Alpha"},
            "Body": {"type": "markdown"}
        }
    });
    let with_beta = serde_json::json!({
        "name": "ReceiptForm",
        "fields": {
            "Subject": {"type": "string", "required": true, "label": "Beta"},
            "Body": {"type": "markdown"}
        }
    });
    // Both writers have read Form version 2 and prepared different changes
    // before either is allowed to create its deterministic publication.
    let gate = form::TestFormEvolutionGate::new(created.form_id);
    form::install_test_form_evolution_gate(gate);
    let (alpha, beta) = tokio::join!(
        form::upsert_form_result(&op, ws_path, &with_alpha),
        form::upsert_form_result(&op, ws_path, &with_beta),
    );
    form::clear_test_form_evolution_gate();
    let alpha = alpha?;
    let beta = beta?;
    assert!(alpha.applied && beta.applied);
    assert_eq!(alpha.form_id, created.form_id);
    assert_eq!(beta.form_id, created.form_id);
    let alpha_change = alpha.change_id.expect("Alpha Change ID");
    let beta_change = beta.change_id.expect("Beta Change ID");
    assert_ne!(alpha_change, beta_change);
    let all_changes = workspace.list_changes().await?;
    assert!(all_changes
        .iter()
        .any(|change| change.change_id == alpha_change));
    assert!(all_changes
        .iter()
        .any(|change| change.change_id == beta_change));
    let final_form = form::get_form(&op, ws_path, "ReceiptForm").await?;
    assert!(
        final_form["fields"]["Subject"]["label"] == "Alpha"
            || final_form["fields"]["Subject"]["label"] == "Beta"
    );
    let latest_change_id = all_changes
        .last()
        .expect("the two evolutions were published")
        .change_id
        .as_str();
    let final_label = final_form["fields"]["Subject"]["label"]
        .as_str()
        .expect("final Subject label");
    assert!(
        (latest_change_id == alpha_change && final_label == "Alpha")
            || (latest_change_id == beta_change && final_label == "Beta"),
        "the receipt for the last publication must belong to the writer whose value is in the final Form"
    );
    assert!(final_form["fields"]["Body"].is_object());
    assert_eq!(final_form["version"], 4);
    Ok(())
}

#[tokio::test]
async fn sql_columns_remain_stable_when_pre_v1_renames_are_rejected() -> anyhow::Result<()> {
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

    let rename_error = form::upsert_form(
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
    .await
    .expect_err("pre-v1 Form renames must be rejected");
    let app_error = rename_error
        .downcast_ref::<AppError>()
        .expect("Form rename rejection must remain typed");
    assert_eq!(app_error.code(), ErrorCode::FormFieldRemovalNotSupported);
    let after = form::get_form(&op, ws_path, "CaseFields").await?;
    assert_eq!(after["fields"]["Status"]["sql_column"], status_column);
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
            "id": {"type": "string"}
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
