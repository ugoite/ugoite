use crate::cli_config::{resolve_command_target, SpaceTarget};
use crate::config::{effective_format, print_json, print_json_table, Format};
use crate::http;
use anyhow::Result;
use clap::{Args, Subcommand};
use std::io::Write as IoWrite;
use std::io::{IsTerminal, Read};
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_iceberg::service::UgoiteService;

/// Byte budget for inline `asset read` text previews. Storage allows much
/// larger objects; inline display is a convenience that must stay out of the
/// way of terminals and pipes. Full bytes always use `asset download`.
const INLINE_TEXT_PREVIEW_BYTES: usize = 32 * 1024;

#[derive(Args)]
pub struct AssetCmd {
    /// Output format (default: table when TTY, json when piped)
    #[arg(short = 'o', long, value_enum, global = true)]
    pub format: Option<Format>,
    #[command(subcommand)]
    pub sub: AssetSubCmd,
}

#[derive(Subcommand)]
pub enum AssetSubCmd {
    /// Upload an asset
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Upload {
        #[arg(value_name = "FILE")]
        file: String,
        #[arg(long)]
        filename: Option<String>,
    },
    /// Delete an asset
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Delete {
        #[arg(value_name = "ASSET_ID")]
        asset_id: String,
        #[arg(long)]
        human_approval: Option<String>,
    },
    /// List Form-owned asset references visible in a space
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    List,
    /// Read an asset referenced by an entry field
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Read {
        #[arg(value_name = "ASSET_ID")]
        asset_id: String,
        #[arg(long, help = "Containing entry that references the asset")]
        entry: String,
        #[arg(long, help = "Entry field that references the asset")]
        field: String,
    },
    /// Download an asset referenced by an entry field
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Download {
        #[arg(value_name = "ASSET_ID")]
        asset_id: String,
        #[arg(long, help = "Containing entry that references the asset")]
        entry: String,
        #[arg(long, help = "Entry field that references the asset")]
        field: String,
        #[arg(long, help = "Output path, or - for stdout (refused on a TTY)")]
        out: String,
    },
}

pub async fn run(
    cmd: AssetCmd,
    explicit_config: Option<&std::path::Path>,
    context_override: Option<&str>,
) -> Result<()> {
    let fmt = effective_format(cmd.format);
    match cmd.sub {
        AssetSubCmd::Upload { file, filename } => {
            let file_path = file;
            let target = resolve_command_target(explicit_config, context_override, "asset upload")?;
            let file_size = std::fs::metadata(&file_path)?.len();
            if file_size > ugoite_iceberg::asset::MAX_ASSET_BYTES as u64 {
                anyhow::bail!(
                    "asset exceeds the {}-byte size limit",
                    ugoite_iceberg::asset::MAX_ASSET_BYTES
                );
            }
            let mut file = std::fs::File::open(&file_path)?;
            let mut data = Vec::with_capacity(file_size as usize);
            Read::by_ref(&mut file)
                .take(ugoite_iceberg::asset::MAX_ASSET_BYTES as u64 + 1)
                .read_to_end(&mut data)?;
            if data.len() > ugoite_iceberg::asset::MAX_ASSET_BYTES {
                anyhow::bail!(
                    "asset exceeds the {}-byte size limit",
                    ugoite_iceberg::asset::MAX_ASSET_BYTES
                );
            }
            let name = filename.unwrap_or_else(|| {
                std::path::Path::new(&file_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("asset")
                    .to_string()
            });
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let result = http::execute_multipart_for_target(
                    &target,
                    "asset.upload",
                    serde_json::json!({"space_id": space_uid}),
                    name,
                    data,
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation asset.upload does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            let asset = service.save_asset(space_id, &name, &data).await?;
            print_json(&asset);
        }
        AssetSubCmd::Delete {
            asset_id,
            human_approval,
        } => {
            let target = resolve_command_target(explicit_config, context_override, "asset delete")?;
            let human_approval =
                human_approval.or_else(|| std::env::var("UGOITE_HUMAN_APPROVAL").ok());
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let result = http::execute_for_target(
                    &target,
                    "asset.delete",
                    serde_json::json!({"space_id": space_uid, "asset_id": asset_id, "human_approval": human_approval}),
                    None,
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            if human_approval.is_some() {
                anyhow::bail!("--human-approval is only supported in backend/api mode");
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation asset.delete does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            service.delete_asset(space_id, &asset_id).await?;
            print_json(&serde_json::json!({"deleted": true}));
        }
        AssetSubCmd::List => {
            let target = resolve_command_target(explicit_config, context_override, "asset list")?;
            let items = match &target {
                SpaceTarget::Remote { space_uid, .. } => {
                    let result = http::execute_for_target(
                        &target,
                        "asset.list",
                        serde_json::json!({"space_id": space_uid}),
                        None,
                    )
                    .await?;
                    result.as_array().cloned().unwrap_or_default()
                }
                SpaceTarget::Core { root, space_id } => {
                    let service = UgoiteService::new_without_background_refresh(root)?;
                    service.list_assets(space_id).await?
                }
            };
            if fmt != Format::Json {
                print_json_table(
                    &items,
                    &[
                        ("ASSET_ID", "asset_id"),
                        ("NAME", "name"),
                        ("MEDIA_TYPE", "media_type"),
                        ("SIZE", "size_bytes"),
                        ("ENTRY", "entry_id"),
                        ("FIELD", "field"),
                    ],
                );
                return Ok(());
            }
            print_json(&items);
        }
        AssetSubCmd::Read {
            asset_id,
            entry,
            field,
        } => {
            let target = resolve_command_target(explicit_config, context_override, "asset read")?;
            let context = resolve_asset_context(&target, &asset_id, &entry, &field).await?;
            match inline_text(&context.bytes, reference_media_type(&context.reference)) {
                Some(preview) => {
                    if fmt == Format::Json {
                        print_json(&read_payload(&context, Some(preview.text)));
                    } else {
                        print_read_table(&context, Some(&preview));
                    }
                }
                None => {
                    if fmt == Format::Json {
                        print_json(&read_payload(&context, None));
                    } else {
                        print_read_table(&context, None);
                    }
                }
            }
        }
        AssetSubCmd::Download {
            asset_id,
            entry,
            field,
            out,
        } => {
            let target =
                resolve_command_target(explicit_config, context_override, "asset download")?;
            let use_stdout = out.trim() == "-";
            if use_stdout && std::io::stdout().is_terminal() {
                anyhow::bail!("refusing to write binary asset bytes to a terminal; use --out PATH");
            }
            let context = resolve_asset_context(&target, &asset_id, &entry, &field).await?;
            if use_stdout {
                IoWrite::write_all(&mut std::io::stdout().lock(), &context.bytes)
                    .map_err(|error| anyhow::anyhow!("write asset bytes to stdout: {error}"))?;
                return Ok(());
            }
            std::fs::write(&out, &context.bytes)
                .map_err(|error| anyhow::anyhow!("write asset to {out}: {error}"))?;
            print_json(&serde_json::json!({
                "downloaded": true,
                "asset_id": context.reference.get("asset_id"),
                "entry_id": context.reference.get("entry_id"),
                "field": context.reference.get("field"),
                "size_bytes": context.bytes.len(),
                "out": out,
            }));
        }
    }
    Ok(())
}

/// Entry-field context resolved to the shared read authority plus bytes.
///
/// The reference must be visible in `asset list` for the exact
/// (entry, field, asset) triple; anything else fails as not found without
/// falling back to another entry, field, or asset. Core mode additionally
/// enforces the reference edge through the shared service boundary before
/// touching bytes; backend mode relies on the server-side authorization
/// that `asset.read` requires.
struct AssetContext {
    reference: serde_json::Value,
    bytes: Vec<u8>,
}

async fn resolve_asset_context(
    target: &SpaceTarget,
    asset_id: &str,
    entry_id: &str,
    field: &str,
) -> Result<AssetContext> {
    let items = match target {
        SpaceTarget::Remote { space_uid, .. } => http::execute_for_target(
            target,
            "asset.list",
            serde_json::json!({"space_id": space_uid}),
            None,
        )
        .await?
        .as_array()
        .cloned()
        .unwrap_or_default(),
        SpaceTarget::Core { root, space_id } => {
            let service = UgoiteService::new_without_background_refresh(root)?;
            service.list_assets(space_id).await?
        }
    };
    let reference = items
        .iter()
        .find(|item| {
            item.get("asset_id").and_then(serde_json::Value::as_str) == Some(asset_id)
                && item.get("entry_id").and_then(serde_json::Value::as_str) == Some(entry_id)
                && item.get("field").and_then(serde_json::Value::as_str) == Some(field)
        })
        .cloned()
        .ok_or_else(|| {
            AppError::not_found(
                ErrorCode::AssetNotFound,
                format!("Asset {asset_id} not found in Entry {entry_id} field {field}"),
            )
        })?;
    let form = reference
        .get("form")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let bytes = match target {
        SpaceTarget::Remote { space_uid, .. } => {
            http::execute_bytes_for_target(
                target,
                "asset.read",
                serde_json::json!({
                    "space_id": space_uid,
                    "asset_id": asset_id,
                    "form": form,
                    "entry_id": entry_id,
                }),
            )
            .await?
        }
        SpaceTarget::Core { root, space_id } => {
            let service = UgoiteService::new_without_background_refresh(root)?;
            service
                .ensure_asset_reference_is_readable(space_id, &form, entry_id, asset_id)
                .await?;
            service.read_asset(space_id, asset_id).await?.bytes
        }
    };
    Ok(AssetContext { reference, bytes })
}

fn reference_media_type(reference: &serde_json::Value) -> &str {
    reference
        .get("media_type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
}

fn reference_name(reference: &serde_json::Value) -> &str {
    reference
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
}

struct TextPreview {
    text: String,
    truncated_bytes: usize,
}

/// Safely displayable text only: the Form-owned media type decides, never
/// content sniffing, and undecodable or non-text bytes stay download-only.
fn inline_text(bytes: &[u8], media_type: &str) -> Option<TextPreview> {
    let displayable = media_type.starts_with("text/") || media_type == "application/json";
    if !displayable {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    if bytes.len() <= INLINE_TEXT_PREVIEW_BYTES {
        return Some(TextPreview {
            text: text.to_string(),
            truncated_bytes: 0,
        });
    }
    let mut end = 0;
    for (index, ch) in text.char_indices() {
        if index >= INLINE_TEXT_PREVIEW_BYTES {
            break;
        }
        end = index + ch.len_utf8();
    }
    Some(TextPreview {
        text: text[..end].to_string(),
        truncated_bytes: bytes.len() - end,
    })
}

fn read_payload(context: &AssetContext, content_text: Option<String>) -> serde_json::Value {
    serde_json::json!({
        "asset_id": context.reference.get("asset_id"),
        "name": context.reference.get("name"),
        "media_type": context.reference.get("media_type"),
        "size_bytes": context.reference.get("size_bytes"),
        "sha256": context.reference.get("sha256"),
        "form": context.reference.get("form"),
        "entry_id": context.reference.get("entry_id"),
        "field": context.reference.get("field"),
        "content_text": content_text,
    })
}

fn print_read_table(context: &AssetContext, preview: Option<&TextPreview>) {
    println!(
        "asset_id: {}",
        context
            .reference
            .get("asset_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
    );
    println!("name: {}", reference_name(&context.reference));
    println!("media_type: {}", reference_media_type(&context.reference));
    println!(
        "size_bytes: {}",
        context
            .reference
            .get("size_bytes")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default()
    );
    println!(
        "entry: {} field: {}",
        context
            .reference
            .get("entry_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default(),
        context
            .reference
            .get("field")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
    );
    match preview {
        Some(preview) => {
            println!("content:");
            println!("{}", preview.text);
            if preview.truncated_bytes > 0 {
                println!(
                    "[truncated {} bytes; use `asset download` for full content]",
                    preview.truncated_bytes
                );
            }
        }
        None => println!("content: (binary content not shown; use `asset download`)"),
    }
}

#[cfg(test)]
mod tests {
    use super::{inline_text, INLINE_TEXT_PREVIEW_BYTES};

    #[test]
    fn inline_preview_is_gated_by_declared_media_type() {
        let preview = inline_text(b"hello", "text/plain").expect("text previews");
        assert_eq!(preview.text, "hello");
        assert_eq!(preview.truncated_bytes, 0);
        let json = inline_text(br#"{"a":1}"#, "application/json").expect("json previews");
        assert_eq!(json.text, r#"{"a":1}"#);
        assert!(inline_text(b"hello", "application/octet-stream").is_none());
        assert!(inline_text(b"hello", "image/png").is_none());
        assert!(inline_text(b"\xff\xfe binary", "text/plain").is_none());
    }

    #[test]
    fn inline_preview_truncates_on_char_boundaries() {
        let big = "é".repeat(INLINE_TEXT_PREVIEW_BYTES);
        let preview = inline_text(big.as_bytes(), "text/plain").expect("truncated preview");
        assert!(preview.text.len() <= INLINE_TEXT_PREVIEW_BYTES);
        assert!(preview.text.ends_with('é'));
        assert!(preview.truncated_bytes > 0);
    }
}
