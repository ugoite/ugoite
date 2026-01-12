use anyhow::Result;
use futures::TryStreamExt;
use opendal::{EntryMode, Operator};

pub async fn save_attachment(
    op: &Operator,
    ws_path: &str,
    filename: &str,
    content: &[u8],
) -> Result<()> {
    // Ensure filename is safe or sanitized?
    let attachment_path = format!("{}/attachments/{}", ws_path, filename);
    op.write(&attachment_path, content.to_vec()).await?;
    Ok(())
}

pub async fn list_attachments(op: &Operator, ws_path: &str) -> Result<Vec<String>> {
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
            if !name.is_empty() {
                attachments.push(name.to_string());
            }
        }
    }

    Ok(attachments)
}

pub async fn delete_attachment(op: &Operator, ws_path: &str, filename: &str) -> Result<()> {
    let attachment_path = format!("{}/attachments/{}", ws_path, filename);
    op.delete(&attachment_path).await?;
    Ok(())
}
