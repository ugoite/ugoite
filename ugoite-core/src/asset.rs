use anyhow::{anyhow, Result};
use futures::TryStreamExt;
use opendal::{EntryMode, Operator};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entry;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AssetInfo {
    pub id: String,
    pub name: String,
    pub path: String,
}

pub async fn save_asset(
    op: &Operator,
    ws_path: &str,
    filename: &str,
    content: &[u8],
) -> Result<AssetInfo> {
    let asset_id = Uuid::new_v4().to_string();
    let safe_name = if filename.is_empty() {
        asset_id.clone()
    } else {
        filename.to_string()
    };
    let relative_path = format!("assets/{}_{}", asset_id, safe_name);
    let asset_path = format!("{}/{}", ws_path, relative_path);
    op.write(&asset_path, content.to_vec()).await?;
    Ok(AssetInfo {
        id: asset_id,
        name: safe_name,
        path: relative_path,
    })
}

pub async fn list_assets(op: &Operator, ws_path: &str) -> Result<Vec<AssetInfo>> {
    let assets_path = format!("{}/assets/", ws_path);
    if !op.exists(&assets_path).await? {
        return Ok(vec![]);
    }

    let mut lister = op.lister(&assets_path).await?;
    let mut assets = Vec::new();

    while let Some(entry) = lister.try_next().await? {
        let meta = entry.metadata();
        if meta.mode() == EntryMode::FILE {
            let name = entry.name().split('/').next_back().unwrap_or("");
            if name.is_empty() {
                continue;
            }
            if let Some((id, original)) = name.split_once('_') {
                assets.push(AssetInfo {
                    id: id.to_string(),
                    name: original.to_string(),
                    path: format!("assets/{}", name),
                });
            }
        }
    }

    Ok(assets)
}

async fn is_asset_referenced(op: &Operator, ws_path: &str, asset_id: &str) -> Result<bool> {
    let rows = entry::list_entry_rows(op, ws_path).await?;
    for (_form_name, row) in rows {
        if row.deleted {
            continue;
        }
        if row
            .assets
            .iter()
            .any(|att| att.get("id").and_then(|v| v.as_str()) == Some(asset_id))
        {
            return Ok(true);
        }
    }

    Ok(false)
}

pub async fn delete_asset(op: &Operator, ws_path: &str, asset_id: &str) -> Result<()> {
    if is_asset_referenced(op, ws_path, asset_id).await? {
        return Err(anyhow!("Asset {} is referenced by a entry", asset_id));
    }

    let assets_path = format!("{}/assets/", ws_path);
    if !op.exists(&assets_path).await? {
        return Err(anyhow!("Asset {} not found", asset_id));
    }

    let mut deleted = false;
    let mut lister = op.lister(&assets_path).await?;
    while let Some(entry) = lister.try_next().await? {
        let meta = entry.metadata();
        if meta.mode() != EntryMode::FILE {
            continue;
        }
        let name = entry.name().split('/').next_back().unwrap_or("");
        if name.starts_with(&format!("{}_", asset_id)) {
            let entry_path = format!("{}/assets/{}", ws_path, name);
            op.delete(&entry_path).await?;
            deleted = true;
        }
    }

    if !deleted {
        return Err(anyhow!("Asset {} not found", asset_id));
    }

    Ok(())
}
