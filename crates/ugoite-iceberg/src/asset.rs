use anyhow::Result;
use opendal::Operator;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::form;
use crate::index;
use ugoite_core::error::{AppError, ErrorCode};
pub use ugoite_domain::entry::AssetReference;
use ugoite_domain::id::validate_asset_id;
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AssetContent {
    pub reference: AssetReference,
    pub bytes: Vec<u8>,
}

fn asset_path(ws_path: &str, asset_id: &str) -> String {
    format!("{ws_path}/assets/{asset_id}")
}

fn normalize_asset_basename(segment: &str) -> Option<String> {
    let trimmed = segment.trim();
    if trimmed.is_empty() || matches!(trimmed, "." | "..") {
        return None;
    }
    let flattened = trimmed
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>();
    let single_line = flattened.split_whitespace().collect::<Vec<_>>().join(" ");
    let safe = single_line.trim_start_matches('#').trim_start();
    (!safe.is_empty()).then(|| safe.to_string())
}

fn normalize_asset_filename(filename: &str, fallback_name: &str) -> String {
    let basename = filename
        .rsplit(['/', '\\'])
        .find(|segment| !segment.is_empty())
        .unwrap_or("");
    normalize_asset_basename(basename).unwrap_or_else(|| fallback_name.to_string())
}

fn reference(asset_id: String, name: String, bytes: &[u8]) -> AssetReference {
    reference_with_media_type(asset_id, name, "application/octet-stream", bytes)
}

fn reference_with_media_type(
    asset_id: String,
    name: String,
    media_type: &str,
    bytes: &[u8],
) -> AssetReference {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    AssetReference {
        asset_id,
        name,
        media_type: media_type.to_string(),
        size_bytes: bytes.len() as u64,
        sha256: hex::encode(hasher.finalize()),
    }
}

/// Upload bytes and return a typed value ready to be placed in a Form field.
/// Uploading never creates an Entry or a system Form row.
pub async fn save_asset(
    op: &Operator,
    ws_path: &str,
    filename: &str,
    content: &[u8],
) -> Result<AssetReference> {
    save_asset_with_media_type(op, ws_path, filename, content, "application/octet-stream").await
}

pub async fn save_asset_with_media_type(
    op: &Operator,
    ws_path: &str,
    filename: &str,
    content: &[u8],
    media_type: &str,
) -> Result<AssetReference> {
    let asset_id = Uuid::now_v7().to_string();
    let safe_name = normalize_asset_filename(filename, &asset_id);
    op.write(&asset_path(ws_path, &asset_id), content.to_vec())
        .await?;
    Ok(reference_with_media_type(
        asset_id, safe_name, media_type, content,
    ))
}

pub async fn read_asset(op: &Operator, ws_path: &str, asset_id: &str) -> Result<AssetContent> {
    validate_asset_id(asset_id).map_err(|error| AppError::invalid_identifier(error.to_string()))?;
    let path = asset_path(ws_path, asset_id);
    let bytes = op.read(&path).await.map_err(|_| {
        AppError::not_found(
            ErrorCode::AssetNotFound,
            format!("Asset {asset_id} not found"),
        )
    })?;
    let bytes = bytes.to_vec();
    Ok(AssetContent {
        reference: reference(asset_id.to_string(), asset_id.to_string(), &bytes),
        bytes,
    })
}

async fn is_asset_referenced(op: &Operator, ws_path: &str, asset_id: &str) -> Result<bool> {
    for form_name in form::list_form_names(op, ws_path).await? {
        let form_def = form::read_form_definition(op, ws_path, &form_name).await?;
        let Some(fields) = form_def
            .get("fields")
            .and_then(serde_json::Value::as_object)
        else {
            continue;
        };
        let scalar_fields = fields
            .iter()
            .filter(|(_, field)| {
                field.get("type").and_then(serde_json::Value::as_str) == Some("asset_reference")
            })
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        let list_fields = fields
            .iter()
            .filter(|(_, field)| {
                field.get("type").and_then(serde_json::Value::as_str) == Some("list")
                    && field
                        .get("items")
                        .and_then(|items| items.get("type"))
                        .and_then(serde_json::Value::as_str)
                        == Some("asset_reference")
            })
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        if index::current_asset_reference_exists(
            op,
            ws_path,
            &form_name,
            &scalar_fields,
            &list_fields,
            asset_id,
        )
        .await?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub async fn delete_asset(op: &Operator, ws_path: &str, asset_id: &str) -> Result<()> {
    validate_asset_id(asset_id).map_err(|error| AppError::invalid_identifier(error.to_string()))?;
    if is_asset_referenced(op, ws_path, asset_id).await? {
        return Err(AppError::conflict(
            ErrorCode::AssetReferenced,
            format!("Asset {asset_id} is referenced by an entry"),
        )
        .into());
    }
    let path = asset_path(ws_path, asset_id);
    if !op.exists(&path).await? {
        return Err(AppError::not_found(
            ErrorCode::AssetNotFound,
            format!("Asset {asset_id} not found"),
        )
        .into());
    }
    op.delete(&path).await?;
    Ok(())
}
