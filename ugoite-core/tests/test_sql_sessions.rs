mod common;

use _ugoite_core::{entry, form, space, sql_session};
use common::setup_operator;

#[tokio::test]
/// REQ-API-008
async fn test_sql_sessions_req_api_008_end_to_end() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-sql-session", "/tmp").await?;
    let ws_path = "spaces/test-sql-session";

    struct MockIntegrity;
    impl _ugoite_core::integrity::IntegrityProvider for MockIntegrity {
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
        "fields": {"Body": {"type": "string"}}
    });
    form::upsert_form(&op, ws_path, &form_def).await?;

    let entry_one = "---\nform: Entry\n---\n# Alpha\n\n## Body\nalpha";
    entry::create_entry(&op, ws_path, "entry-1", entry_one, "author", &MockIntegrity).await?;
    let entry_two = "---\nform: Entry\n---\n# Beta\n\n## Body\nbeta";
    entry::create_entry(&op, ws_path, "entry-2", entry_two, "author", &MockIntegrity).await?;

    let session = sql_session::create_sql_session(
        &op,
        ws_path,
        "SELECT * FROM entries WHERE title = 'Alpha'",
    )
    .await?;
    assert_eq!(session["status"], "completed");
    let session_id = session["id"].as_str().unwrap();

    let count = sql_session::get_sql_session_count(&op, ws_path, session_id).await?;
    assert_eq!(count, 1);

    let rows = sql_session::get_sql_session_rows(&op, ws_path, session_id, 0, 10).await?;
    assert_eq!(rows["total_count"], 1);
    let rows_list = rows["rows"].as_array().unwrap();
    assert_eq!(rows_list.len(), 1);
    assert_eq!(rows_list[0]["id"], "entry-1");

    Ok(())
}
