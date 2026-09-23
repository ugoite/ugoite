mod common;

use common::setup_operator;
use serde_json::json;
use std::collections::BTreeMap;
use ugoite_core::query::EntryScope;
use ugoite_iceberg::entry;
use ugoite_iceberg::form;
use ugoite_iceberg::iceberg_store;
use ugoite_iceberg::integrity::FakeIntegrityProvider;
use ugoite_iceberg::saved_sql::{
    self, SearchHistoryOperator, SqlGeneratedName, SqlKind, SqlMetadata, SqlPayload,
};
use ugoite_iceberg::space;

const FORM_RELATION: &str = "form_00000000000000000000000000000001";

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
        sql: format!("SELECT * FROM \"{FORM_RELATION}\" WHERE _ugoite_updated_at >= {{{{since}}}}"),
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
    let expected_sql =
        format!("SELECT * FROM \"{FORM_RELATION}\" WHERE _ugoite_updated_at >= $since");
    assert_eq!(
        fetched.get("sql").and_then(|v| v.as_str()),
        Some(expected_sql.as_str())
    );

    let entries = saved_sql::list_sql(&op, ws_path, EntryScope::AllCurrent).await?;
    assert!(entries
        .iter()
        .any(|item| item.get("id") == Some(&json!("sql-1"))));

    let update_payload = SqlPayload {
        name: Some("Recent Meetings".to_string()),
        kind: SqlKind::UserQuery,
        metadata: None,
        sql: format!(
            "SELECT * FROM \"{FORM_RELATION}\" WHERE _ugoite_updated_at >= $since ORDER BY _ugoite_updated_at DESC, _ugoite_id"
        ),
        variables: payload.variables.clone(),
    };

    let updated = saved_sql::update_sql(
        &op,
        ws_path,
        "sql-1",
        &update_payload,
        revision_id,
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

    saved_sql::delete_sql(&op, ws_path, "sql-1", "deleter").await?;
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
        sql: format!(
            "SELECT * FROM \"{FORM_RELATION}\" ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 50"
        ),
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
/// REQ-API-006 saved-sql-name-field: the display name is a normal optional
/// Form field; nameless historical records remain valid.
async fn saved_sql_name_is_a_normal_field() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "sql-name-field", "/tmp").await?;
    let ws_path = "spaces/sql-name-field";
    let integrity = FakeIntegrityProvider;

    let payload = SqlPayload {
        name: Some("Old Name".to_string()),
        kind: SqlKind::UserQuery,
        metadata: None,
        sql: format!("SELECT * FROM \"{FORM_RELATION}\" ORDER BY _ugoite_updated_at"),
        variables: json!([]),
    };
    let created =
        saved_sql::create_sql(&op, ws_path, "sql-named", &payload, "author", &integrity).await?;
    assert_eq!(
        created.get("name").and_then(|v| v.as_str()),
        Some("Old Name")
    );
    let revision_id = created
        .get("revision_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    let renamed = SqlPayload {
        name: Some("New Name".to_string()),
        ..payload
    };
    let updated = saved_sql::update_sql(
        &op,
        ws_path,
        "sql-named",
        &renamed,
        &revision_id,
        "author",
        &integrity,
    )
    .await?;
    assert_eq!(
        updated.get("name").and_then(|v| v.as_str()),
        Some("New Name")
    );

    let fetched = saved_sql::get_sql(&op, ws_path, "sql-named").await?;
    assert_eq!(
        fetched.get("name").and_then(|v| v.as_str()),
        Some("New Name")
    );
    let listed = saved_sql::list_sql(&op, ws_path, EntryScope::AllCurrent).await?;
    let listed_name = listed
        .iter()
        .find(|item| item.get("id") == Some(&json!("sql-named")))
        .and_then(|item| item.get("name"))
        .cloned()
        .unwrap_or(json!(null));
    assert_eq!(listed_name, json!("New Name"));

    // Nameless search-history records remain valid without a name field.
    let history = SqlPayload {
        name: None,
        kind: SqlKind::SearchHistory,
        metadata: Some(SqlMetadata {
            search_criteria: Some(ugoite_iceberg::saved_sql::SearchHistoryCriteria {
                form_name: "Meeting".to_string(),
                tags: vec![],
                updated_from: "".to_string(),
                updated_to: "".to_string(),
                field_conditions: vec![],
            }),
            generated_name: None,
        }),
        sql: format!("SELECT * FROM \"{FORM_RELATION}\" LIMIT 1"),
        variables: json!([]),
    };
    let saved_history =
        saved_sql::create_sql(&op, ws_path, "sql-history", &history, "author", &integrity).await?;
    assert!(saved_history["name"].is_null());
    let fetched_history = saved_sql::get_sql(&op, ws_path, "sql-history").await?;
    assert!(fetched_history["name"].is_null());

    Ok(())
}

#[tokio::test]
/// REQ-API-006 saved-sql-name-field: reading a pre-name SQL Form evolves only
/// the Form schema; the old row remains readable and new rows use `name` as a
/// normal field without a table rewrite.
async fn saved_sql_evolves_legacy_form_without_rewriting_entries() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "sql-name-field-legacy", "/tmp").await?;
    let ws_path = "spaces/sql-name-field-legacy";
    let integrity = FakeIntegrityProvider;

    // The public Form API rejects reserved metadata names. Seed the historical
    // SQL Form through the storage boundary so this fixture represents a
    // pre-name system Form rather than a user-created Form.
    iceberg_store::ensure_form_tables(
        &op,
        ws_path,
        &json!({
            "id": "00000000-0000-0000-0000-000000000001",
            "name": "SQL",
            "version": 1,
            "fields": {
                "sql": {"id": 100, "type": "sql", "required": true},
                "variables": {"id": 101, "type": "object_list", "required": false}
            },
            "allow_extra_attributes": "allow_json"
        }),
    )
    .await?;
    let legacy_form = form::get_form(&op, ws_path, "SQL").await?;
    assert!(legacy_form["fields"].get("name").is_none());

    let mut legacy_fields = BTreeMap::new();
    legacy_fields.insert("sql".to_string(), json!("SELECT 1"));
    legacy_fields.insert("variables".to_string(), json!([]));
    let mut legacy_attributes = BTreeMap::new();
    legacy_attributes.insert("kind".to_string(), json!("user-query"));
    legacy_attributes.insert("metadata".to_string(), serde_json::Value::Null);
    entry::create_structured_entry_with_scopes_and_change(
        &op,
        ws_path,
        "legacy-sql",
        "SQL".to_string(),
        Vec::new(),
        legacy_fields,
        legacy_attributes,
        "author",
        &integrity,
        None,
        None,
    )
    .await?;
    let before_evolution = entry::get_entry(&op, ws_path, "legacy-sql").await?;
    let legacy_revision = before_evolution["revision_id"].clone();

    let legacy = saved_sql::get_sql(&op, ws_path, "legacy-sql").await?;
    assert!(legacy["name"].is_null());
    assert_eq!(legacy["revision_id"], legacy_revision);

    let evolved_form = form::get_form(&op, ws_path, "SQL").await?;
    assert!(
        evolved_form["fields"].get("name").is_some(),
        "evolved SQL Form: {evolved_form}"
    );
    assert_eq!(evolved_form["version"], json!(2));

    let new_payload = SqlPayload {
        name: Some("Current SQL".to_string()),
        kind: SqlKind::UserQuery,
        metadata: None,
        sql: "SELECT 2".to_string(),
        variables: json!([]),
    };
    saved_sql::create_sql(
        &op,
        ws_path,
        "current-sql",
        &new_payload,
        "author",
        &integrity,
    )
    .await?;
    let current_entry = entry::get_entry(&op, ws_path, "current-sql").await?;
    assert_eq!(current_entry["fields"]["name"], json!("Current SQL"));

    Ok(())
}

#[tokio::test]
/// REQ-API-007
async fn test_saved_sql_req_api_007_validation_errors() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "sql-validate", "/tmp").await?;
    let ws_path = "spaces/sql-validate";
    let integrity = FakeIntegrityProvider;

    let invalid_operator = serde_json::from_value::<SqlPayload>(json!({
        "name": null,
        "kind": "search-history",
        "metadata": {
            "searchCriteria": {
                "formName": "Meeting",
                "tags": [],
                "updatedFrom": "",
                "updatedTo": "",
                "fieldConditions": [{
                    "field": "Status",
                    "operator": "starts-with",
                    "value": "Active"
                }]
            }
        },
        "sql": "SELECT 1",
        "variables": []
    }));
    assert!(invalid_operator.is_err());

    let unknown_create_field = serde_json::from_value::<SqlPayload>(json!({
        "name": "Query",
        "kind": "user-query",
        "sql": "SELECT 1",
        "variables": [],
        "id": "client-selected-id"
    }));
    assert!(unknown_create_field.is_err());

    let blank_name = SqlPayload {
        name: Some("  ".to_string()),
        kind: SqlKind::UserQuery,
        metadata: None,
        sql: "SELECT 1".to_string(),
        variables: json!([]),
    };
    let blank_name_err = saved_sql::create_sql(
        &op,
        ws_path,
        "sql-blank-name",
        &blank_name,
        "author",
        &integrity,
    )
    .await
    .unwrap_err();
    assert!(blank_name_err.to_string().contains("non-blank"));

    let named_with_generated_name = SqlPayload {
        name: Some("Named query".to_string()),
        kind: SqlKind::UserQuery,
        metadata: Some(SqlMetadata {
            search_criteria: None,
            generated_name: Some(SqlGeneratedName::Untitled),
        }),
        sql: "SELECT 1".to_string(),
        variables: json!([]),
    };
    let named_with_generated_name_err = saved_sql::create_sql(
        &op,
        ws_path,
        "sql-named-generated",
        &named_with_generated_name,
        "author",
        &integrity,
    )
    .await
    .unwrap_err();
    assert!(named_with_generated_name_err
        .to_string()
        .contains("named user-query"));

    let empty_metadata = SqlPayload {
        name: Some("Named query".to_string()),
        kind: SqlKind::UserQuery,
        metadata: Some(SqlMetadata {
            search_criteria: None,
            generated_name: None,
        }),
        sql: "SELECT 1".to_string(),
        variables: json!([]),
    };
    let empty_metadata_err = saved_sql::create_sql(
        &op,
        ws_path,
        "sql-empty-metadata",
        &empty_metadata,
        "author",
        &integrity,
    )
    .await
    .unwrap_err();
    assert!(empty_metadata_err
        .to_string()
        .contains("metadata must be omitted"));

    let search_history_with_generated_name = SqlPayload {
        name: None,
        kind: SqlKind::SearchHistory,
        metadata: Some(SqlMetadata {
            search_criteria: Some(ugoite_iceberg::saved_sql::SearchHistoryCriteria {
                form_name: "Meeting".to_string(),
                tags: vec![],
                updated_from: "".to_string(),
                updated_to: "".to_string(),
                field_conditions: vec![],
            }),
            generated_name: Some(SqlGeneratedName::Untitled),
        }),
        sql: "SELECT 1".to_string(),
        variables: json!([]),
    };
    let search_history_with_generated_name_err = saved_sql::create_sql(
        &op,
        ws_path,
        "sql-history-generated",
        &search_history_with_generated_name,
        "author",
        &integrity,
    )
    .await
    .unwrap_err();
    assert!(search_history_with_generated_name_err
        .to_string()
        .contains("only search_criteria"));

    assert_eq!(
        serde_json::to_value(SearchHistoryOperator::Equals)?,
        json!("equals")
    );

    let missing_placeholder = SqlPayload {
        name: Some("Missing placeholder".to_string()),
        kind: SqlKind::UserQuery,
        metadata: None,
        sql: format!("SELECT * FROM \"{FORM_RELATION}\""),
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

    let empty_sql = SqlPayload {
        name: Some("Empty SQL".to_string()),
        kind: SqlKind::UserQuery,
        metadata: None,
        sql: "  ".to_string(),
        variables: json!([]),
    };
    let empty_sql_err =
        saved_sql::create_sql(&op, ws_path, "sql-empty", &empty_sql, "author", &integrity)
            .await
            .unwrap_err();
    assert!(empty_sql_err
        .to_string()
        .contains("SQL must contain a statement"));

    let undefined_placeholder = SqlPayload {
        name: Some("Undefined placeholder".to_string()),
        kind: SqlKind::UserQuery,
        metadata: None,
        sql: format!("SELECT * FROM \"{FORM_RELATION}\" WHERE _ugoite_updated_at >= $since"),
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
    Ok(())
}
