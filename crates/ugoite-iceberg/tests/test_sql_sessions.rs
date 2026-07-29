mod common;

use common::setup_operator;
use std::collections::{BTreeMap, HashSet};
use ugoite_iceberg::{entry, form, saved_sql, space, sql_session};
use uuid::Uuid;

fn authorized_entries(form: &str, entry_ids: &[&str]) -> (Uuid, BTreeMap<String, HashSet<String>>) {
    (
        Uuid::from_u128(1),
        [(
            form.to_ascii_lowercase(),
            entry_ids
                .iter()
                .map(|entry_id| (*entry_id).to_string())
                .collect(),
        )]
        .into_iter()
        .collect(),
    )
}

const AUTHORIZATION_POLICY_HASH: &str = "sha256:test-authorization-policy";

#[tokio::test]
/// REQ-API-008
async fn test_sql_sessions_req_api_008_end_to_end() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-sql-session", "/tmp").await?;
    let ws_path = "spaces/test-sql-session";

    struct MockIntegrity;
    impl ugoite_iceberg::integrity::IntegrityProvider for MockIntegrity {
        fn checksum(&self, data: &str) -> String {
            format!("chk-{}", data.len())
        }

        fn signature(&self, _data: &str) -> String {
            "mock-signature".to_string()
        }
    }

    let form_def = serde_json::json!({
        "name": "Entry",
        "template": "# Entry\n\n## Body\n",
        "fields": {"Body": {"type": "markdown"}}
    });
    form::upsert_form(&op, ws_path, &form_def).await?;

    let entry_one = "---\nform: Entry\n---\n# Alpha\n\n## Body\nalpha";
    entry::create_entry(&op, ws_path, "entry-1", entry_one, "author", &MockIntegrity).await?;
    let entry_two = "---\nform: Entry\n---\n# Beta\n\n## Body\nbeta";
    entry::create_entry(&op, ws_path, "entry-2", entry_two, "author", &MockIntegrity).await?;

    let sql_payload = saved_sql::SqlPayload {
        name: "Alpha Query".to_string(),
        sql: "SELECT * FROM entry WHERE _ugoite_title = $title ORDER BY _ugoite_id".to_string(),
        variables: serde_json::json!([{
            "name": "title",
            "type": "string",
            "description": "Entry title",
        }]),
    };
    saved_sql::create_sql(
        &op,
        ws_path,
        "sql-alpha",
        &sql_payload,
        "author",
        &MockIntegrity,
    )
    .await?;

    let (principal_id, readable_entries_by_form) =
        authorized_entries("Entry", &["entry-1", "entry-2"]);
    let principal_ids = [principal_id];
    let authorization = sql_session::SqlSessionAuthorization {
        principal_ids: &principal_ids,
        policy_hash: AUTHORIZATION_POLICY_HASH,
        readable_entries_by_form: &readable_entries_by_form,
    };
    let parameters = [("title".to_string(), serde_json::json!("Alpha"))]
        .into_iter()
        .collect();
    let parameter_types = [("title".to_string(), "string".to_string())]
        .into_iter()
        .collect();
    let session =
        sql_session::create_sql_session_authorized_for_principals_by_form_with_parameters(
            &op,
            ws_path,
            &sql_payload.sql,
            parameters,
            parameter_types,
            authorization,
        )
        .await?;
    assert_eq!(session["status"], "ready");
    assert_eq!(session["parameters"], serde_json::json!({"title": "Alpha"}));
    assert_eq!(
        session["parameter_types"],
        serde_json::json!({"title": "string"})
    );
    let session_id = session["id"].as_str().unwrap();

    entry::create_entry(
        &op,
        ws_path,
        "entry-3",
        "---\nform: Entry\n---\n# After checkpoint\n\n## Body\nnew",
        "author",
        &MockIntegrity,
    )
    .await?;

    let count = sql_session::get_sql_session_count_authorized_by_form(
        &op,
        ws_path,
        session_id,
        authorization,
    )
    .await?;
    assert_eq!(count, 1);

    let rows = sql_session::get_sql_session_rows_authorized_by_form(
        &op,
        ws_path,
        session_id,
        authorization,
        0,
        10,
    )
    .await?;
    assert_eq!(rows["total_count"], 1);
    let rows_list = rows["rows"].as_array().unwrap();
    assert_eq!(rows_list.len(), 1);
    assert_eq!(rows_list[0]["_ugoite_id"], "entry-1");

    Ok(())
}

#[tokio::test]
/// REQ-API-008
async fn test_sql_sessions_req_api_008_scopes_rows_before_limit() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-sql-session-acl", "/tmp").await?;
    let ws_path = "spaces/test-sql-session-acl";

    struct MockIntegrity;
    impl ugoite_iceberg::integrity::IntegrityProvider for MockIntegrity {
        fn checksum(&self, data: &str) -> String {
            format!("chk-{}", data.len())
        }

        fn signature(&self, _data: &str) -> String {
            "mock-signature".to_string()
        }
    }

    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "PublicTask",
            "template": "# PublicTask\n\n## Summary\n",
            "fields": {"Summary": {"type": "string"}},
        }),
    )
    .await?;
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "RestrictedTask",
            "template": "# RestrictedTask\n\n## Summary\n",
            "fields": {"Summary": {"type": "string"}},
        }),
    )
    .await?;

    entry::create_entry(
        &op,
        ws_path,
        "public-a",
        "---\nform: PublicTask\n---\n# Public A\n\n## Summary\nPublic A\n",
        "author",
        &MockIntegrity,
    )
    .await?;
    entry::create_entry(
        &op,
        ws_path,
        "public-b",
        "---\nform: PublicTask\n---\n# Public B\n\n## Summary\nPublic B\n",
        "author",
        &MockIntegrity,
    )
    .await?;
    entry::create_entry(
        &op,
        ws_path,
        "restricted-z",
        "---\nform: RestrictedTask\n---\n# Restricted Z\n\n## Summary\nRestricted Z\n",
        "author",
        &MockIntegrity,
    )
    .await?;

    let (principal_id, readable_entries_by_form) =
        authorized_entries("PublicTask", &["public-a", "public-b"]);
    let principal_ids = [principal_id];
    let authorization = sql_session::SqlSessionAuthorization {
        principal_ids: &principal_ids,
        policy_hash: AUTHORIZATION_POLICY_HASH,
        readable_entries_by_form: &readable_entries_by_form,
    };
    let session = sql_session::create_sql_session_authorized_for_principals_by_form(
        &op,
        ws_path,
        "SELECT * FROM publictask ORDER BY _ugoite_id DESC LIMIT 2",
        authorization,
    )
    .await?;
    let session_id = session["id"].as_str().unwrap();

    let count = sql_session::get_sql_session_count_authorized_by_form(
        &op,
        ws_path,
        session_id,
        authorization,
    )
    .await?;
    assert_eq!(count, 2);

    let rows = sql_session::get_sql_session_rows_authorized_by_form(
        &op,
        ws_path,
        session_id,
        authorization,
        0,
        10,
    )
    .await?;
    assert_eq!(rows["total_count"], 2);
    let rows_list = rows["rows"].as_array().unwrap();
    assert_eq!(rows_list.len(), 2);
    assert_eq!(rows_list[0]["_ugoite_id"], "public-b");
    assert_eq!(rows_list[1]["_ugoite_id"], "public-a");

    Ok(())
}

#[tokio::test]
async fn sql_sessions_reject_unsafe_pagination_and_authorization_changes() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-sql-session-boundaries", "/tmp").await?;
    let ws_path = "spaces/test-sql-session-boundaries";

    struct MockIntegrity;
    impl ugoite_iceberg::integrity::IntegrityProvider for MockIntegrity {
        fn checksum(&self, data: &str) -> String {
            format!("chk-{}", data.len())
        }

        fn signature(&self, _data: &str) -> String {
            "mock-signature".to_string()
        }
    }

    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Task",
            "template": "# Task\n\n## Summary\n",
            "fields": {"Summary": {"type": "string"}},
        }),
    )
    .await?;
    entry::create_entry(
        &op,
        ws_path,
        "task-1",
        "---\nform: Task\n---\n# Task one\n\n## Summary\nOne\n",
        "author",
        &MockIntegrity,
    )
    .await?;
    let (principal_id, readable_entries_by_form) = authorized_entries("Task", &["task-1"]);
    let principal_ids = [principal_id];
    let authorization = sql_session::SqlSessionAuthorization {
        principal_ids: &principal_ids,
        policy_hash: AUTHORIZATION_POLICY_HASH,
        readable_entries_by_form: &readable_entries_by_form,
    };

    for sql in [
        "SELECT * FROM task",
        "SELECT * FROM task ORDER BY _ugoite_updated_at",
        "SELECT DISTINCT _ugoite_id FROM task ORDER BY _ugoite_id",
        "SELECT * FROM task ORDER BY _ugoite_id LIMIT 1 OFFSET 1000000",
    ] {
        assert!(
            sql_session::create_sql_session_authorized_for_principals_by_form(
                &op,
                ws_path,
                sql,
                authorization,
            )
            .await
            .is_err()
        );
    }

    let session = sql_session::create_sql_session_authorized_for_principals_by_form(
        &op,
        ws_path,
        "SELECT * FROM task ORDER BY _ugoite_id",
        authorization,
    )
    .await?;
    let session_id = session["id"].as_str().expect("session id");
    assert!(sql_session::get_sql_session_rows_authorized_by_form(
        &op,
        ws_path,
        session_id,
        authorization,
        1_000,
        1,
    )
    .await
    .is_err());
    assert!(sql_session::get_sql_session_rows_authorized_by_form(
        &op,
        ws_path,
        session_id,
        sql_session::SqlSessionAuthorization {
            policy_hash: "sha256:changed-policy",
            ..authorization
        },
        0,
        1,
    )
    .await
    .is_err());

    Ok(())
}
