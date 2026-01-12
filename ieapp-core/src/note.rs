use crate::integrity::IntegrityProvider;
use anyhow::{anyhow, Result};
use chrono::Utc;
use opendal::Operator;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct NoteContent {
    pub markdown: String,
    pub frontmatter: serde_json::Value,
    pub sections: serde_json::Value,
    pub revision_id: String,
    pub parent_revision_id: Option<String>,
    pub author: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NoteMeta {
    pub id: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub links: Vec<crate::link::Link>,
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
    author: &str,
    integrity: &I,
) -> Result<NoteMeta> {
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
        author: author.to_string(),
    };

    let content_json = serde_json::to_vec_pretty(&note_content)?;
    op.write(&format!("{}/content.json", note_path), content_json)
        .await?;

    // 2. Create meta.json
    let meta = NoteMeta {
        id: note_id.to_string(),
        created_at: now.clone(),
        updated_at: now.clone(),
        links: vec![],
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

    Ok(meta)
}

pub async fn list_notes(_op: &Operator, _ws_path: &str) -> Result<Vec<String>> {
    // TODO: Implement list_notes
    Ok(vec![])
}

pub async fn get_note(_op: &Operator, _ws_path: &str, _note_id: &str) -> Result<String> {
    // TODO: Implement get_note
    Ok("".to_string())
}

pub async fn get_note_content(op: &Operator, ws_path: &str, note_id: &str) -> Result<NoteContent> {
    let path = format!("{}/notes/{}/content.json", ws_path, note_id);
    if !op.exists(&path).await? {
        return Err(anyhow!("Note content not found: {}", note_id));
    }
    let bytes = op.read(&path).await?;
    let content: NoteContent = serde_json::from_slice(&bytes.to_vec())?;
    Ok(content)
}

pub async fn update_note<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    note_id: &str,
    content: &str,
    parent_revision_id: Option<&str>,
    author: &str,
    integrity: &I,
) -> Result<NoteMeta> {
    let note_path = format!("{}/notes/{}", ws_path, note_id);

    // Check existence
    if !op.exists(&format!("{}/meta.json", note_path)).await? {
        return Err(anyhow!("Note not found: {}", note_id));
    }

    // Read current content to check parent revision
    let current_content_bytes = op.read(&format!("{}/content.json", note_path)).await?;
    let current_content: NoteContent = serde_json::from_slice(&current_content_bytes.to_vec())?;

    // Optimistic concurrency check
    if let Some(expected_parent) = parent_revision_id {
        if current_content.revision_id != expected_parent {
            return Err(anyhow!(
                "Revision conflict: expected {}, got {}",
                expected_parent,
                current_content.revision_id
            ));
        }
    }

    // New revision
    let revision_id = integrity.checksum(content);

    // Update content.json
    let new_note_content = NoteContent {
        markdown: content.to_string(),
        frontmatter: serde_json::json!({}),
        sections: serde_json::json!({}),
        revision_id: revision_id.clone(),
        parent_revision_id: Some(current_content.revision_id.clone()),
        author: author.to_string(),
    };

    op.write(
        &format!("{}/content.json", note_path),
        serde_json::to_vec_pretty(&new_note_content)?,
    )
    .await?;

    // Update meta.json (updated_at)
    let meta_bytes = op.read(&format!("{}/meta.json", note_path)).await?;
    let mut meta: NoteMeta = serde_json::from_slice(&meta_bytes.to_vec())?;
    meta.updated_at = Utc::now().to_rfc3339();
    op.write(
        &format!("{}/meta.json", note_path),
        serde_json::to_vec_pretty(&meta)?,
    )
    .await?;

    // Update history/index.json
    let history_path = format!("{}/history/index.json", note_path);
    let mut history: HistoryIndex = if op.exists(&history_path).await? {
        let h_bytes = op.read(&history_path).await?;
        serde_json::from_slice(&h_bytes.to_vec())?
    } else {
        HistoryIndex { versions: vec![] }
    };
    history.versions.push(revision_id);
    op.write(&history_path, serde_json::to_vec_pretty(&history)?)
        .await?;

    Ok(meta)
}

pub async fn delete_note(_op: &Operator, _ws_path: &str, _note_id: &str) -> Result<()> {
    Ok(())
}
