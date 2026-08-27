use anyhow::Result;
use futures::TryStreamExt;
use opendal::Operator;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

use ugoite_core::error::{AppError, ErrorCode};
use ugoite_core::query::{
    AuthorizedQueryForm, AuthorizedQueryPolicy, EntryScope, QueryLimits, QuerySystemColumn,
};
pub use ugoite_domain::entry::AssetReference;
use ugoite_domain::form::{sql_column_name, sql_relation_name};
use ugoite_domain::id::validate_asset_id;

/// Maximum size of one operator-owned Asset object. The same boundary is used
/// by core upload, REST upload, direct reads, and the derived parser so a
/// locally-created object cannot bypass later resource limits.
pub const MAX_ASSET_BYTES: usize = ugoite_domain::entry::MAX_ASSET_REFERENCE_SIZE_BYTES as usize;
/// Multipart framing and headers are not part of the Asset object limit. The
/// server body limit includes them, while upload_asset enforces the exact
/// per-file MAX_ASSET_BYTES boundary after parsing.
pub const MAX_ASSET_MULTIPART_OVERHEAD_BYTES: usize = 128 * 1024;
const ASSET_READ_CHUNK_BYTES: usize = 256 * 1024;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AssetContent {
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub(crate) enum AssetDeleteConflict {
    Visible,
    Hidden,
}

impl std::fmt::Display for AssetDeleteConflict {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Visible => formatter.write_str("Asset is referenced by an authorized entry"),
            Self::Hidden => formatter.write_str("Asset cannot be deleted while it is in use"),
        }
    }
}

impl std::error::Error for AssetDeleteConflict {}

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
    crate::authorization::Authorizer::new(op.clone()).ensure_authoritative_mutation_contract()?;
    if content.len() > MAX_ASSET_BYTES {
        anyhow::bail!("asset exceeds the {MAX_ASSET_BYTES}-byte size limit");
    }
    let asset_id = Uuid::now_v7().to_string();
    let safe_name = normalize_asset_filename(filename, &asset_id);
    let reference = reference_with_media_type(asset_id, safe_name, media_type, content);
    reference
        .validate()
        .map_err(|error| AppError::invalid_input(ErrorCode::InvalidInput, error.to_string()))?;
    // The blob is immutable and intentionally separate from Catalog
    // publication. A failed later publication leaves this object orphaned;
    // it never becomes authoritative by itself.
    op.write(&asset_path(ws_path, &reference.asset_id), content.to_vec())
        .await?;
    Ok(reference)
}

pub async fn read_asset(op: &Operator, ws_path: &str, asset_id: &str) -> Result<AssetContent> {
    validate_asset_id(asset_id).map_err(|error| AppError::invalid_identifier(error.to_string()))?;
    let workspace = crate::iceberg_store::native_workspace(op, ws_path).await?;
    if workspace.asset_is_deleted(asset_id).await? {
        return Err(AppError::not_found(
            ErrorCode::AssetNotFound,
            format!("Asset {asset_id} not found"),
        )
        .into());
    }
    let path = asset_path(ws_path, asset_id);
    let metadata = op.stat(&path).await.map_err(|error| {
        if error.kind() == opendal::ErrorKind::NotFound {
            AppError::not_found(
                ErrorCode::AssetNotFound,
                format!("Asset {asset_id} not found"),
            )
            .into()
        } else {
            anyhow::Error::from(error)
        }
    })?;
    if metadata.content_length() > MAX_ASSET_BYTES as u64 {
        anyhow::bail!("asset exceeds the {MAX_ASSET_BYTES}-byte size limit");
    }
    let etag = metadata.etag().filter(|etag| !etag.is_empty());
    if crate::is_shared_backend(op) && etag.is_none() {
        anyhow::bail!("exact Asset read requires an ETag: {path}");
    }
    let mut reader = op.reader_with(&path);
    if let Some(etag) = metadata.etag().filter(|etag| !etag.is_empty()) {
        reader = reader.if_match(etag);
    }
    let reader = reader
        .chunk(ASSET_READ_CHUNK_BYTES)
        .await
        .map_err(|error| {
            if error.kind() == opendal::ErrorKind::NotFound {
                AppError::not_found(
                    ErrorCode::AssetNotFound,
                    format!("Asset {asset_id} not found"),
                )
                .into()
            } else {
                anyhow::Error::from(error)
            }
        })?;
    let mut stream = reader.into_stream(0..).await.map_err(|error| {
        if error.kind() == opendal::ErrorKind::NotFound {
            AppError::not_found(
                ErrorCode::AssetNotFound,
                format!("Asset {asset_id} not found"),
            )
            .into()
        } else {
            anyhow::Error::from(error)
        }
    })?;
    let mut bytes = Vec::with_capacity(metadata.content_length() as usize);
    while let Some(buffer) = stream.try_next().await? {
        bytes.extend(buffer.into_iter().flatten());
        if bytes.len() > MAX_ASSET_BYTES {
            anyhow::bail!("asset exceeds the {MAX_ASSET_BYTES}-byte size limit");
        }
    }
    // The exact object key carries no logical name or media type. Those
    // values belong to the Form-owned reference and must not be fabricated.
    Ok(AssetContent { bytes })
}

pub(crate) async fn asset_exists(op: &Operator, ws_path: &str, asset_id: &str) -> Result<bool> {
    validate_asset_id(asset_id)
        .map_err(|error| AppError::invalid_input(ErrorCode::InvalidInput, error.to_string()))?;
    Ok(op.exists(&asset_path(ws_path, asset_id)).await?)
}

pub async fn current_asset_reference_exists(
    op: &Operator,
    ws_path: &str,
    asset_id: &str,
    relation_scopes: &BTreeMap<String, EntryScope>,
) -> Result<bool> {
    let workspace = crate::iceberg_store::native_workspace(op, ws_path).await?;
    current_asset_reference_exists_in_workspace(&workspace, asset_id, relation_scopes).await
}

/// Evaluates Asset references against one exact workspace/catalog view and one
/// closed DataFusion context. The caller supplies the already-derived
/// relation scopes; absent Forms are not registered as empty discoverable
/// relations.
pub async fn current_asset_reference_exists_in_workspace(
    workspace: &crate::IcebergWorkspace,
    asset_id: &str,
    relation_scopes: &BTreeMap<String, EntryScope>,
) -> Result<bool> {
    let forms = workspace.list_forms().await?;
    let mut policy_forms = BTreeMap::new();
    let mut queries = Vec::new();
    let mut list_queries = Vec::new();
    for form_def in forms {
        let Some(entry_scope) = relation_scopes
            .get(&form_def.name.to_ascii_lowercase())
            .cloned()
        else {
            continue;
        };
        let scalar_fields = form_def
            .fields
            .iter()
            .filter(|field| field.field_type == ugoite_domain::form::FieldType::AssetReference)
            .map(|field| sql_column_name(field.id))
            .collect::<Vec<_>>();
        let list_fields = form_def
            .fields
            .iter()
            .filter(|field| {
                field.field_type == ugoite_domain::form::FieldType::List
                    && field.list_item.as_ref().is_some_and(|item| {
                        item.field_type == ugoite_domain::form::FieldType::AssetReference
                    })
            })
            .map(|field| sql_column_name(field.id))
            .collect::<Vec<_>>();
        let relation_name = sql_relation_name(form_def.id);
        let relation = format!("\"{}\"", relation_name.replace('"', "\"\""));
        let literal = format!("'{}'", asset_id.replace('\\', "\\\\").replace('\'', "''"));
        let scalar = scalar_fields.iter().map(|field| {
            format!(
                "\"{}\".asset_id = {literal}",
                field.to_ascii_lowercase().replace('"', "\"\"")
            )
        });
        let predicates = scalar.collect::<Vec<_>>();
        if !predicates.is_empty() {
            queries.push(format!(
                "SELECT 1 FROM {} WHERE ({})",
                relation,
                predicates.join(" OR ")
            ));
        }
        for field in list_fields {
            list_queries.push((relation_name.clone(), field));
        }
        policy_forms.insert(
            form_def.id,
            AuthorizedQueryForm {
                relation: relation_name,
                entry_scope,
                columns: form_def
                    .fields
                    .iter()
                    .map(|field| sql_column_name(field.id))
                    .collect(),
                system_columns: BTreeSet::from([QuerySystemColumn::ExternalId]),
            },
        );
    }
    if queries.is_empty() && list_queries.is_empty() {
        return Ok(false);
    }
    let context = workspace
        .authorized_query_context(AuthorizedQueryPolicy {
            forms: policy_forms,
            checkpoint: None,
            limits: QueryLimits {
                max_memory_bytes: 64 * 1024 * 1024,
                max_rows: 1,
                timeout: std::time::Duration::from_secs(30),
                max_concurrency: 1,
                allowed_functions: BTreeSet::new(),
            },
        })
        .await?;
    if !queries.is_empty() {
        let sql = format!("{} LIMIT 1", queries.join(" UNION ALL "));
        if context
            .execute(&sql)
            .await?
            .iter()
            .any(|batch| batch.num_rows() > 0)
        {
            return Ok(true);
        }
    }
    for (relation, field) in list_queries {
        if context
            .contains_struct_list_value(&relation, &field, "asset_id", asset_id)
            .await?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub async fn delete_asset(
    op: &Operator,
    ws_path: &str,
    asset_id: &str,
    relation_scopes: &BTreeMap<String, EntryScope>,
) -> Result<()> {
    crate::authorization::Authorizer::new(op.clone()).ensure_authoritative_mutation_contract()?;
    validate_asset_id(asset_id).map_err(|error| AppError::invalid_identifier(error.to_string()))?;
    let path = asset_path(ws_path, asset_id);
    if !op.exists(&path).await? {
        return Err(AppError::not_found(
            ErrorCode::AssetNotFound,
            format!("Asset {asset_id} not found"),
        )
        .into());
    }
    let workspace = crate::iceberg_store::native_workspace(op, ws_path).await?;
    let publication = crate::publication_context(
        format!("asset-delete:{asset_id}"),
        "asset.delete",
        &serde_json::json!({"asset_id": asset_id}),
    )?;
    let deletion = workspace
        .commit(publication)?
        .delete_asset(asset_id, relation_scopes)
        .await;
    if let Err(error) = deletion {
        if error
            .downcast_ref::<AssetDeleteConflict>()
            .is_some_and(|conflict| matches!(conflict, AssetDeleteConflict::Visible))
        {
            return Err(AppError::conflict(
                ErrorCode::AssetReferenced,
                "Asset is referenced by an authorized entry",
            )
            .into());
        }
        if error
            .downcast_ref::<AssetDeleteConflict>()
            .is_some_and(|conflict| matches!(conflict, AssetDeleteConflict::Hidden))
        {
            return Err(AppError::forbidden("Asset deletion is not permitted").into());
        }
        return Err(error);
    }
    op.delete(&path).await?;
    Ok(())
}
