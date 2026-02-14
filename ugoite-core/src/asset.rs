use anyhow::{anyhow, Result};
use chrono::Utc;
use futures::TryStreamExt;
use opendal::{EntryMode, Operator};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entry;
use crate::form;
use crate::integrity::RealIntegrityProvider;

const ASSET_FORM_NAME: &str = "Assets";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AssetInfo {
    pub id: String,
    pub name: String,
    pub path: String,
    pub link: String,
    pub uploaded_at: String,
}

fn asset_form_definition() -> serde_json::Value {
    serde_json::json!({
        "name": ASSET_FORM_NAME,
        "version": 1,
        "fields": {
            "name": {"type": "string", "required": true},
            "link": {"type": "string", "required": true},
            "uploaded_at": {"type": "timestamp", "required": true}
        },
        "allow_extra_attributes": "deny"
    })
}

async fn ensure_asset_form(op: &Operator, ws_path: &str) -> Result<()> {
    form::upsert_metadata_form(op, ws_path, &asset_form_definition()).await
}

fn space_id_from_ws_path(ws_path: &str) -> String {
    ws_path
        .trim_end_matches('/')
        .split('/')
        .next_back()
        .unwrap_or(ws_path)
        .to_string()
}

fn build_asset_entry_content(name: &str, link: &str, uploaded_at: &str) -> String {
    format!(
        "---\nform: {ASSET_FORM_NAME}\n---\n# {name}\n\n## name\n{name}\n\n## link\n{link}\n\n## uploaded_at\n{uploaded_at}\n"
    )
}

pub async fn save_asset(
    op: &Operator,
    ws_path: &str,
    filename: &str,
    content: &[u8],
) -> Result<AssetInfo> {
    ensure_asset_form(op, ws_path).await?;
    let asset_id = Uuid::new_v4().to_string();
    let safe_name = if filename.is_empty() {
        asset_id.clone()
    } else {
        filename.to_string()
    };
    let relative_path = format!("assets/{}_{}", asset_id, safe_name);
    let asset_path = format!("{}/{}", ws_path, relative_path);
    let link = format!("ugoite://asset/{asset_id}");
    let uploaded_at = Utc::now().to_rfc3339();
    op.write(&asset_path, content.to_vec()).await?;

    let space_id = space_id_from_ws_path(ws_path);
    let integrity = RealIntegrityProvider::from_space(op, &space_id).await?;
    let entry_content = build_asset_entry_content(&safe_name, &link, &uploaded_at);
    if let Err(error) =
        entry::create_entry(op, ws_path, &asset_id, &entry_content, "system", &integrity).await
    {
        if let Err(cleanup_error) = op.delete(&asset_path).await {
            eprintln!(
                "failed to cleanup asset file after metadata create failure (asset_id={}, path={}): {}",
                asset_id, asset_path, cleanup_error
            );
        }
        return Err(error);
    }

    Ok(AssetInfo {
        id: asset_id,
        name: safe_name,
        path: relative_path,
        link,
        uploaded_at,
    })
}

pub async fn list_assets(op: &Operator, ws_path: &str) -> Result<Vec<AssetInfo>> {
    ensure_asset_form(op, ws_path).await?;
    let mut metadata_by_id = std::collections::HashMap::new();
    if let Ok(form_def) = form::read_form_definition(op, ws_path, ASSET_FORM_NAME).await {
        if let Ok(rows) = entry::list_form_entry_rows(op, ws_path, ASSET_FORM_NAME, &form_def).await
        {
            for row in rows {
                if row.deleted {
                    continue;
                }
                let fields = row.fields.as_object();
                let link = fields
                    .and_then(|f| f.get("link"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let uploaded_at = fields
                    .and_then(|f| f.get("uploaded_at"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                metadata_by_id.insert(row.entry_id, (link, uploaded_at));
            }
        }
    }

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
                let (link, uploaded_at) = metadata_by_id
                    .get(id)
                    .cloned()
                    .unwrap_or((format!("ugoite://asset/{id}"), String::new()));
                assets.push(AssetInfo {
                    id: id.to_string(),
                    name: original.to_string(),
                    path: format!("assets/{}", name),
                    link,
                    uploaded_at,
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

    if let Err(error) = entry::delete_entry(op, ws_path, asset_id, false).await {
        eprintln!(
            "failed to cleanup asset metadata entry after file delete (asset_id={}, ws_path={}): {}",
            asset_id, ws_path, error
        );
    }

    Ok(())
}
