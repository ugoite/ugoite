mod common;

use common::setup_operator;
use serde_json::json;
use ugoite_iceberg::integrity::FakeIntegrityProvider;
use ugoite_iceberg::saved_sql::{self, SqlKind, SqlMetadata, SqlPayload};
use ugoite_iceberg::space;
use ugoite_iceberg::sql_session;

#[tokio::test]
/// REQ-API-006
async fn test_saved_sql_req_api_006_crud() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "sql-space", "/tmp").await?;
    let ws_path = "spaces/sql-space";
    let integrity = FakeIntegrityProvider;

    let payload = SqlPayload {
        name: Some("Recent Meetings".to_string()),
        kind: SqlKind::UserQuery,
        metadata: None,
        sql: "SELECT * FROM sql WHERE _ugoite_updated_at >= $since".to_string(),
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
        name: Some("Recent Meetings".to_string()),
        kind: SqlKind::UserQuery,
        metadata: None,
        sql:
            "SELECT * FROM sql WHERE _ugoite_updated_at >= $since ORDER BY _ugoite_updated_at DESC"
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
async fn advanced_search_sql_is_saved_and_materialized() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "advanced-search", "/tmp").await?;
    let ws_path = "spaces/advanced-search";
    let integrity = FakeIntegrityProvider;
    let payload = SqlPayload {
        name: None,
        kind: SqlKind::SearchHistory,
        metadata: Some(SqlMetadata {
            search_criteria: Some(ugoite_iceberg::saved_sql::SearchHistoryCriteria {
                form_name: "Meeting".to_string(),
                tags: vec!["project".to_string()],
                updated_from: "".to_string(),
                updated_to: "".to_string(),
                field_conditions: vec![],
            }),
            generated_name: None,
        }),
        sql: "SELECT _ugoite_id, _ugoite_title FROM sql ORDER BY _ugoite_updated_at DESC LIMIT 50"
            .to_string(),
        variables: json!([]),
    };

    let saved = saved_sql::create_sql(
        &op,
        ws_path,
        "advanced-search-1",
        &payload,
        "author",
        &integrity,
    )
    .await?;
    assert!(saved["name"].is_null());
    assert_eq!(saved["kind"], json!("search-history"));
    assert_eq!(saved["metadata"]["searchCriteria"]["formName"], "Meeting");
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
        name: Some("Missing placeholder".to_string()),
        kind: SqlKind::UserQuery,
        metadata: None,
        sql: "SELECT * FROM sql".to_string(),
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
        name: Some("Undefined placeholder".to_string()),
        kind: SqlKind::UserQuery,
        metadata: None,
        sql: "SELECT * FROM sql WHERE _ugoite_updated_at >= $since".to_string(),
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
        name: Some("Invalid SQL".to_string()),
        kind: SqlKind::UserQuery,
        metadata: None,
        sql: "SELECT * FROM missing".to_string(),
        variables: json!([]),
    };

    saved_sql::create_sql(
        &op,
        ws_path,
        "sql-invalid",
        &invalid_sql,
        "author",
        &integrity,
    )
    .await?;
    let readable_entries_by_form = std::collections::BTreeMap::new();
    let principal_ids = [uuid::Uuid::from_u128(1)];
    let authorization = sql_session::SqlSessionAuthorization {
        principal_ids: &principal_ids,
        policy_hash: "sha256:test-authorization-policy",
    };
    let create_authorization = sql_session::SqlSessionCreateAuthorization {
        authorization,
        readable_entries_by_form: &readable_entries_by_form,
    };
    assert!(
        sql_session::create_sql_session_authorized_for_principals_by_form(
            &op,
            ws_path,
            &invalid_sql.sql,
            create_authorization,
        )
        .await
        .is_err()
    );

    Ok(())
}
