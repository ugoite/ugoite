use anyhow::{anyhow, Result};
use opendal::Operator;
use serde::{Deserialize, Serialize};

use crate::entry::{find_entry_form, read_entry_row, write_entry_row};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Link {
    pub id: String,
    pub source: String,
    pub target: String,
    pub kind: String,
}

/// Create a bi-directional link between two entries and persist metadata.
pub async fn create_link(
    op: &Operator,
    ws_path: &str,
    source: &str,
    target: &str,
    kind: &str,
    link_id: &str,
) -> Result<Link> {
    let source_form = find_entry_form(op, ws_path, source)
        .await?
        .ok_or_else(|| anyhow!("Source entry not found: {}", source))?;
    let target_form = find_entry_form(op, ws_path, target)
        .await?
        .ok_or_else(|| anyhow!("Target entry not found: {}", target))?;

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
    update_entry_links(op, ws_path, &source_form, source, link_record.clone()).await?;

    // Update target
    update_entry_links(op, ws_path, &target_form, target, reciprocal_record).await?;

    Ok(link_record)
}

async fn update_entry_links(
    op: &Operator,
    ws_path: &str,
    form_name: &str,
    entry_id: &str,
    link: Link,
) -> Result<()> {
    let mut row = read_entry_row(op, ws_path, form_name, entry_id).await?;
    row.links.retain(|l| l.id != link.id);
    row.links.push(link);
    row.updated_at = crate::entry::now_ts();
    write_entry_row(op, ws_path, form_name, entry_id, &row).await?;
    Ok(())
}

/// Return deduplicated links in a space.
pub async fn list_links(op: &Operator, ws_path: &str) -> Result<Vec<Link>> {
    let mut links = std::collections::HashMap::new();
    let rows = crate::entry::list_entry_rows(op, ws_path).await?;
    for (_form_name, row) in rows {
        if row.deleted {
            continue;
        }
        for link in row.links {
            links.insert(link.id.clone(), link);
        }
    }

    Ok(links.into_values().collect())
}

/// Delete a link and remove it from all entries in the space.
pub async fn delete_link(op: &Operator, ws_path: &str, link_id: &str) -> Result<()> {
    let mut found = false;
    let rows = crate::entry::list_entry_rows(op, ws_path).await?;
    for (form_name, mut row) in rows {
        let initial_len = row.links.len();
        row.links.retain(|l| l.id != link_id);
        if row.links.len() != initial_len {
            found = true;
            row.updated_at = crate::entry::now_ts();
            write_entry_row(op, ws_path, &form_name, &row.entry_id, &row).await?;
        }
    }

    if !found {
        return Err(anyhow!("Link not found: {}", link_id));
    }

    Ok(())
}
