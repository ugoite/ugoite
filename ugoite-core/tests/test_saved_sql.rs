mod common;

use _ugoite_core::integrity::FakeIntegrityProvider;
use _ugoite_core::saved_sql::{self, SqlPayload};
use _ugoite_core::space;
use common::setup_operator;
use serde_json::json;

#[tokio::test]
/// REQ-API-006
async fn test_saved_sql_req_api_006_crud() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "sql-space", "/tmp").await?;
    let ws_path = "spaces/sql-space";
    let integrity = FakeIntegrityProvider;

    let payload = SqlPayload {
        name: "Recent Meetings".to_string(),
        sql: "SELECT * FROM entries WHERE updated_at >= {{since}}".to_string(),
        variables: json!([
            {
                "type": "date",
                "name": "since",
                "description": "Lower bound",
            }
        ]),
    };

    let entry =
        saved_sql::create_sql(&op, ws_path, "sql-1", &payload, "author", &integrity).await?;
    let revision_id = entry
        .get("revision_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(!revision_id.is_empty());

    let fetched = saved_sql::get_sql(&op, ws_path, "sql-1").await?;
    assert_eq!(
        fetched.get("name").and_then(|v| v.as_str()),
        Some("Recent Meetings")
    );

    let entries = saved_sql::list_sql(&op, ws_path).await?;
    assert!(entries
        .iter()
        .any(|item| item.get("id") == Some(&json!("sql-1"))));

    let update_payload = SqlPayload {
        name: "Recent Meetings".to_string(),
        sql: "SELECT * FROM entries WHERE updated_at >= {{since}} ORDER BY updated_at DESC"
            .to_string(),
        variables: payload.variables.clone(),
    };

    let updated = saved_sql::update_sql(
        &op,
        ws_path,
        "sql-1",
        &update_payload,
        Some(revision_id),
        "author",
        &integrity,
    )
    .await?;
    let new_revision_id = updated
        .get("revision_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(!new_revision_id.is_empty());
    assert_ne!(revision_id, new_revision_id);

    saved_sql::delete_sql(&op, ws_path, "sql-1").await?;
    assert!(saved_sql::get_sql(&op, ws_path, "sql-1").await.is_err());

    Ok(())
}

#[tokio::test]
/// REQ-API-007
async fn test_saved_sql_req_api_007_validation_errors() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "sql-validate", "/tmp").await?;
    let ws_path = "spaces/sql-validate";
    let integrity = FakeIntegrityProvider;

    let missing_placeholder = SqlPayload {
        name: "Missing placeholder".to_string(),
        sql: "SELECT * FROM entries".to_string(),
        variables: json!([
            {
                "type": "date",
                "name": "since",
                "description": "Lower bound",
            }
        ]),
    };

    let missing_err = saved_sql::create_sql(
        &op,
        ws_path,
        "sql-missing",
        &missing_placeholder,
        "author",
        &integrity,
    )
    .await
    .unwrap_err();
    assert!(missing_err.to_string().contains("UGOITE_SQL_VALIDATION"));

    let undefined_placeholder = SqlPayload {
        name: "Undefined placeholder".to_string(),
        sql: "SELECT * FROM entries WHERE updated_at >= {{since}}".to_string(),
        variables: json!([]),
    };

    let undefined_err = saved_sql::create_sql(
        &op,
        ws_path,
        "sql-undefined",
        &undefined_placeholder,
        "author",
        &integrity,
    )
    .await
    .unwrap_err();
    assert!(undefined_err.to_string().contains("UGOITE_SQL_VALIDATION"));

    let invalid_sql = SqlPayload {
        name: "Invalid SQL".to_string(),
        sql: "FROM entries".to_string(),
        variables: json!([]),
    };

    let invalid_err = saved_sql::create_sql(
        &op,
        ws_path,
        "sql-invalid",
        &invalid_sql,
        "author",
        &integrity,
    )
    .await
    .unwrap_err();
    assert!(invalid_err.to_string().contains("UGOITE_SQL_VALIDATION"));

    Ok(())
}
