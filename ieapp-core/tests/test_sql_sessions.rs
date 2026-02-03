mod common;

use _ieapp_core::{class, note, sql_session, workspace};
use common::setup_operator;

#[tokio::test]
/// REQ-API-008
async fn test_sql_sessions_req_api_008_end_to_end() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-sql-session", "/tmp").await?;
    let ws_path = "workspaces/test-sql-session";

    struct MockIntegrity;
    impl _ieapp_core::integrity::IntegrityProvider for MockIntegrity {
        fn checksum(&self, data: &str) -> String {
            format!("chk-{}", data.len())
        }

        fn signature(&self, _data: &str) -> String {
            "mock-signature".to_string()
        }
    }

    let class_def = serde_json::json!({
        "name": "Note",
        "template": "# Note\n\n## Body\n",
        "fields": {"Body": {"type": "string"}}
    });
    class::upsert_class(&op, ws_path, &class_def).await?;

    let note_one = "---\nclass: Note\n---\n# Alpha\n\n## Body\nalpha";
    note::create_note(&op, ws_path, "note-1", note_one, "author", &MockIntegrity).await?;
    let note_two = "---\nclass: Note\n---\n# Beta\n\n## Body\nbeta";
    note::create_note(&op, ws_path, "note-2", note_two, "author", &MockIntegrity).await?;

    let session =
        sql_session::create_sql_session(&op, ws_path, "SELECT * FROM notes WHERE title = 'Alpha'")
            .await?;
    assert_eq!(session["status"], "completed");
    let session_id = session["id"].as_str().unwrap();

    let count = sql_session::get_sql_session_count(&op, ws_path, session_id).await?;
    assert_eq!(count, 1);

    let rows = sql_session::get_sql_session_rows(&op, ws_path, session_id, 0, 10).await?;
    assert_eq!(rows["total_count"], 1);
    let rows_list = rows["rows"].as_array().unwrap();
    assert_eq!(rows_list.len(), 1);
    assert_eq!(rows_list[0]["id"], "note-1");

    Ok(())
}
