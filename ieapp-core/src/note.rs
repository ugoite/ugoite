use crate::integrity::IntegrityProvider;
use anyhow::{anyhow, Result};
use chrono::Utc;
use opendal::Operator;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
struct NoteContent {
    markdown: String,
    frontmatter: serde_json::Value,
    sections: serde_json::Value,
    revision_id: String,
    parent_revision_id: Option<String>,
    author: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct NoteMeta {
    id: String,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct HistoryIndex {
    versions: Vec<String>,
}

pub async fn create_note<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    note_id: &str,
    content: &str,
    integrity: &I,
) -> Result<()> {
    let note_path = format!("{}/notes/{}", ws_path, note_id);

    // Check if note already exists
    if op.exists(&format!("{}/", note_path)).await?
        || op.exists(&format!("{}/meta.json", note_path)).await?
    {
        return Err(anyhow!("Note already exists: {}", note_id));
    }

    op.create_dir(&format!("{}/", note_path)).await?;
    op.create_dir(&format!("{}/history/", note_path)).await?;

    let now = Utc::now().to_rfc3339();
    let revision_id = integrity.checksum(content); // Use checksum as revision ID for now

    // 1. Create content.json
    let note_content = NoteContent {
        markdown: content.to_string(),
        frontmatter: serde_json::json!({}), // Parsing logic needed if we want to extract frontmatter
        sections: serde_json::json!({}),
        revision_id: revision_id.clone(),
        parent_revision_id: None,
        author: "user".to_string(), // Placeholder
    };

    let content_json = serde_json::to_vec_pretty(&note_content)?;
    op.write(&format!("{}/content.json", note_path), content_json)
        .await?;

    // 2. Create meta.json
    let meta = NoteMeta {
        id: note_id.to_string(),
        created_at: now.clone(),
        updated_at: now,
    };
    let meta_json = serde_json::to_vec_pretty(&meta)?;
    op.write(&format!("{}/meta.json", note_path), meta_json)
        .await?;

    // 3. Create history/index.json
    let history = HistoryIndex {
        versions: vec![revision_id],
    };
    let history_json = serde_json::to_vec_pretty(&history)?;
    op.write(&format!("{}/history/index.json", note_path), history_json)
        .await?;

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
