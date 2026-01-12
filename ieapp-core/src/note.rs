use crate::integrity::IntegrityProvider;
use anyhow::Result;
use opendal::Operator;

pub async fn create_note<I: IntegrityProvider>(
    _op: &Operator,
    _ws_path: &str,
    _note_id: &str,
    _content: &str,
    _integrity: &I,
) -> Result<()> {
    // TODO: Implement create_note
    Ok(())
}

pub async fn list_notes(_op: &Operator, _ws_path: &str) -> Result<Vec<String>> {
    // TODO: Implement list_notes
    Ok(vec![])
}

pub async fn get_note(_op: &Operator, _ws_path: &str, _note_id: &str) -> Result<String> {
    // TODO: Implement get_note
    Ok("".to_string())
}

pub async fn update_note<I: IntegrityProvider>(
    _op: &Operator,
    _ws_path: &str,
    _note_id: &str,
    _content: &str,
    _old_revision: Option<&str>,
    _integrity: &I,
) -> Result<()> {
    // TODO: Implement update_note
    Ok(())
}

pub async fn delete_note(_op: &Operator, _ws_path: &str, _note_id: &str) -> Result<()> {
    Ok(())
}
