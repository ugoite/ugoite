use anyhow::{anyhow, Result};
use futures::TryStreamExt;
use opendal::{EntryMode, Operator};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::note;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AttachmentInfo {
    pub id: String,
    pub name: String,
    pub path: String,
}

pub async fn save_attachment(
    op: &Operator,
    ws_path: &str,
    filename: &str,
    content: &[u8],
) -> Result<AttachmentInfo> {
    let attachment_id = Uuid::new_v4().to_string();
    let safe_name = if filename.is_empty() {
        attachment_id.clone()
    } else {
        filename.to_string()
    };
    let relative_path = format!("attachments/{}_{}", attachment_id, safe_name);
    let attachment_path = format!("{}/{}", ws_path, relative_path);
    op.write(&attachment_path, content.to_vec()).await?;
    Ok(AttachmentInfo {
        id: attachment_id,
        name: safe_name,
        path: relative_path,
    })
}

pub async fn list_attachments(op: &Operator, ws_path: &str) -> Result<Vec<AttachmentInfo>> {
    let attachments_path = format!("{}/attachments/", ws_path);
    if !op.exists(&attachments_path).await? {
        return Ok(vec![]);
    }

    let mut lister = op.lister(&attachments_path).await?;
    let mut attachments = Vec::new();

    while let Some(entry) = lister.try_next().await? {
        let meta = entry.metadata();
        if meta.mode() == EntryMode::FILE {
            let name = entry.name().split('/').next_back().unwrap_or("");
            if name.is_empty() {
                continue;
            }
            if let Some((id, original)) = name.split_once('_') {
                attachments.push(AttachmentInfo {
                    id: id.to_string(),
                    name: original.to_string(),
                    path: format!("attachments/{}", name),
                });
            }
        }
    }

    Ok(attachments)
}

async fn is_attachment_referenced(
    op: &Operator,
    ws_path: &str,
    attachment_id: &str,
) -> Result<bool> {
    let rows = note::list_note_rows(op, ws_path).await?;
    for (_class_name, row) in rows {
        if row.deleted {
            continue;
        }
        if row
            .attachments
            .iter()
            .any(|att| att.get("id").and_then(|v| v.as_str()) == Some(attachment_id))
        {
            return Ok(true);
        }
    }

    Ok(false)
}

pub async fn delete_attachment(op: &Operator, ws_path: &str, attachment_id: &str) -> Result<()> {
    if is_attachment_referenced(op, ws_path, attachment_id).await? {
        return Err(anyhow!(
            "Attachment {} is referenced by a note",
            attachment_id
        ));
    }

    let attachments_path = format!("{}/attachments/", ws_path);
    if !op.exists(&attachments_path).await? {
        return Err(anyhow!("Attachment {} not found", attachment_id));
    }

    let mut deleted = false;
    let mut lister = op.lister(&attachments_path).await?;
    while let Some(entry) = lister.try_next().await? {
        let meta = entry.metadata();
        if meta.mode() != EntryMode::FILE {
            continue;
        }
        let name = entry.name().split('/').next_back().unwrap_or("");
        if name.starts_with(&format!("{}_", attachment_id)) {
            let entry_path = format!("{}/attachments/{}", ws_path, name);
            op.delete(&entry_path).await?;
            deleted = true;
        }
    }

    if !deleted {
        return Err(anyhow!("Attachment {} not found", attachment_id));
    }

    Ok(())
}
