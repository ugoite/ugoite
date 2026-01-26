use anyhow::{anyhow, Result};
use opendal::Operator;
use serde::{Deserialize, Serialize};

use crate::note::{find_note_class, read_note_row, write_note_row};

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
    let source_class = find_note_class(op, ws_path, source)
        .await?
        .ok_or_else(|| anyhow!("Source note not found: {}", source))?;
    let target_class = find_note_class(op, ws_path, target)
        .await?
        .ok_or_else(|| anyhow!("Target note not found: {}", target))?;

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
    update_note_links(op, ws_path, &source_class, source, link_record.clone()).await?;

    // Update target
    update_note_links(op, ws_path, &target_class, target, reciprocal_record).await?;

    Ok(link_record)
}

async fn update_note_links(
    op: &Operator,
    ws_path: &str,
    class_name: &str,
    note_id: &str,
    link: Link,
) -> Result<()> {
    let mut row = read_note_row(op, ws_path, class_name, note_id).await?;
    row.links.retain(|l| l.id != link.id);
    row.links.push(link);
    row.updated_at = crate::note::now_ts();
    write_note_row(op, ws_path, class_name, note_id, &row).await?;
    Ok(())
}

/// Return deduplicated links in a workspace.
pub async fn list_links(op: &Operator, ws_path: &str) -> Result<Vec<Link>> {
    let mut links = std::collections::HashMap::new();
    let rows = crate::note::list_note_rows(op, ws_path).await?;
    for (_class_name, row) in rows {
        if row.deleted {
            continue;
        }
        for link in row.links {
            links.insert(link.id.clone(), link);
        }
    }

    Ok(links.into_values().collect())
}

/// Delete a link and remove it from all notes in the workspace.
pub async fn delete_link(op: &Operator, ws_path: &str, link_id: &str) -> Result<()> {
    let mut found = false;
    let rows = crate::note::list_note_rows(op, ws_path).await?;
    for (class_name, mut row) in rows {
        let initial_len = row.links.len();
        row.links.retain(|l| l.id != link_id);
        if row.links.len() != initial_len {
            found = true;
            row.updated_at = crate::note::now_ts();
            write_note_row(op, ws_path, &class_name, &row.note_id, &row).await?;
        }
    }

    if !found {
        return Err(anyhow!("Link not found: {}", link_id));
    }

    Ok(())
}
