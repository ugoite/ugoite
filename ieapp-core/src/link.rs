use anyhow::{anyhow, Result};
use opendal::Operator;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Link {
    pub id: String,
    pub source: String,
    pub target: String,
    pub kind: String,
}

/// Create a bi-directional link between two notes and persist metadata.
pub async fn create_link(
    op: &Operator,
    ws_path: &str,
    source: &str,
    target: &str,
    kind: &str,
    link_id: &str,
) -> Result<Link> {
    let source_meta_path = format!("{}/notes/{}/meta.json", ws_path, source);
    let target_meta_path = format!("{}/notes/{}/meta.json", ws_path, target);

    if !op.exists(&source_meta_path).await? {
        return Err(anyhow!("Source note not found: {}", source));
    }
    if !op.exists(&target_meta_path).await? {
        return Err(anyhow!("Target note not found: {}", target));
    }

    let link_record = Link {
        id: link_id.to_string(),
        source: source.to_string(),
        target: target.to_string(),
        kind: kind.to_string(),
    };

    let reciprocal_record = Link {
        id: link_id.to_string(),
        source: target.to_string(), // Reciprocal source is target
        target: source.to_string(), // Reciprocal target is source
        kind: kind.to_string(),
    };

    // Update source
    update_note_links(op, &source_meta_path, link_record.clone()).await?;

    // Update target
    update_note_links(op, &target_meta_path, reciprocal_record).await?;

    Ok(link_record)
}

async fn update_note_links(op: &Operator, meta_path: &str, link: Link) -> Result<()> {
    let bytes = op.read(meta_path).await?;
    let mut meta: crate::note::NoteMeta = serde_json::from_slice(&bytes.to_vec())?;

    // Remove existing link with same ID if any (update/insert)
    meta.links.retain(|l| l.id != link.id);
    meta.links.push(link);

    let json = serde_json::to_vec_pretty(&meta)?;
    op.write(meta_path, json).await?;
    Ok(())
}

/// Return deduplicated links in a workspace.
pub async fn list_links(op: &Operator, ws_path: &str) -> Result<Vec<Link>> {
    let notes_dir = format!("{}/notes/", ws_path);
    if !op.exists(&notes_dir).await? {
        return Ok(vec![]);
    }

    let mut links = std::collections::HashMap::new();
    let ds = op.list(&notes_dir).await?;

    for entry in ds {
        let path = entry.path();

        if path.ends_with('/') {
            let meta_path = format!("{}meta.json", path);
            if op.exists(&meta_path).await? {
                let bytes = op.read(&meta_path).await?;
                if let Ok(meta) = serde_json::from_slice::<crate::note::NoteMeta>(&bytes.to_vec()) {
                    for link in meta.links {
                        links.insert(link.id.clone(), link);
                    }
                }
            }
        }
    }

    Ok(links.into_values().collect())
}

/// Delete a link and remove it from all notes in the workspace.
pub async fn delete_link(op: &Operator, ws_path: &str, link_id: &str) -> Result<()> {
    let notes_dir = format!("{}/notes/", ws_path);
    if !op.exists(&notes_dir).await? {
        return Err(anyhow!("Link not found: {}", link_id));
    }

    let mut found = false;
    let ds = op.list(&notes_dir).await?;

    for entry in ds {
        let path = entry.path();

        if path.ends_with('/') {
            let meta_path = format!("{}meta.json", path);
            if op.exists(&meta_path).await? {
                let bytes = op.read(&meta_path).await?;
                if let Ok(mut meta) =
                    serde_json::from_slice::<crate::note::NoteMeta>(&bytes.to_vec())
                {
                    let initial_len = meta.links.len();
                    meta.links.retain(|l| l.id != link_id);

                    if meta.links.len() != initial_len {
                        found = true;
                        let json = serde_json::to_vec_pretty(&meta)?;
                        op.write(&meta_path, json).await?;
                    }
                }
            }
        }
    }

    if !found {
        return Err(anyhow!("Link not found: {}", link_id));
    }

    Ok(())
}
