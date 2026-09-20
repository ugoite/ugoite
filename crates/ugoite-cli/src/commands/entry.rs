use crate::cli_config::{resolve_command_target, SpaceTarget};
use crate::http;
use crate::output::{
    effective_format, emit_success, print_json_table, render_receipt, stdout_style, Format,
    MutationReceipt, UsageError,
};
use anyhow::Result;
use clap::{Args, Subcommand};
use ugoite_iceberg::service::UgoiteService;

#[derive(Args)]
pub struct EntryCmd {
    /// Output format (default: table when TTY, json when piped)
    #[arg(short = 'o', long, value_enum, global = true)]
    pub format: Option<Format>,
    #[command(subcommand)]
    pub sub: EntrySubCmd,
}

#[derive(Subcommand)]
pub enum EntrySubCmd {
    /// List entries in a space
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    List,
    /// Get an entry by ID
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Get {
        #[arg(value_name = "ENTRY_ID")]
        entry_id: String,
    },
    /// Create an entry
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Create {
        #[arg(value_name = "ENTRY_ID")]
        entry_id: String,
        #[arg(long, value_name = "FORM", help = "Form name for structured authoring")]
        form: Option<String>,
        #[arg(
            long = "field",
            value_name = "KEY=VALUE",
            help = "Structured field as KEY=VALUE (repeatable; VALUE stays a string and the shared Rust boundary coerces it)"
        )]
        fields: Vec<String>,
        #[arg(
            long = "fields-file",
            value_name = "PATH",
            help = "Read structured fields as a JSON object from PATH, or from explicit stdin with --fields-file - (duplicate inputs are errors; duplicate keys within one JSON object use the parser's last value)"
        )]
        fields_files: Vec<String>,
        #[arg(
            long,
            help = "Author name to record in the revision history (local only)"
        )]
        author: Option<String>,
    },
    /// Update an entry
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Update {
        #[arg(value_name = "ENTRY_ID")]
        entry_id: String,
        #[arg(
            long,
            value_name = "FORM",
            help = "Form name for structured authoring (must match the stored form; changes are rejected)"
        )]
        form: Option<String>,
        #[arg(
            long = "field",
            value_name = "KEY=VALUE",
            help = "Structured field as KEY=VALUE (repeatable; VALUE stays a string and the shared Rust boundary coerces it)"
        )]
        fields: Vec<String>,
        #[arg(
            long = "fields-file",
            value_name = "PATH",
            help = "Read structured fields as a JSON object from PATH, or from explicit stdin with --fields-file - (duplicate inputs are errors; duplicate keys within one JSON object use the parser's last value)"
        )]
        fields_files: Vec<String>,
        #[arg(
            long,
            help = "Expected current revision ID to enforce optimistic concurrency checks; if omitted, the CLI reads it immediately before updating"
        )]
        parent_revision_id: Option<String>,
        #[arg(
            long,
            default_value = "cli",
            help = "Author name to record in the revision history (local only)"
        )]
        author: String,
    },
    /// Delete an entry
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Delete {
        #[arg(value_name = "ENTRY_ID")]
        entry_id: String,
        #[arg(long)]
        hard_delete: bool,
        /// Single-use approval token issued by a recently reauthenticated human.
        #[arg(long)]
        human_approval: Option<String>,
        #[arg(
            long,
            default_value = "cli",
            help = "Actor name to record for the delete (local only)"
        )]
        author: String,
    },
    /// Get entry history
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    History {
        #[arg(value_name = "ENTRY_ID")]
        entry_id: String,
    },
    /// Get a specific revision
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Revision {
        #[arg(value_name = "ENTRY_ID")]
        entry_id: String,
        #[arg(value_name = "REVISION_ID")]
        revision_id: String,
    },
    /// Restore an entry to a revision
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Restore {
        #[arg(value_name = "ENTRY_ID")]
        entry_id: String,
        #[arg(value_name = "REVISION_ID")]
        revision_id: String,
        #[arg(
            long,
            default_value = "cli",
            help = "Author name to record in the revision history (local only)"
        )]
        author: String,
    },
}

fn entry_receipt(
    id: String,
    revision_id: Option<String>,
    change_id: Option<String>,
) -> MutationReceipt {
    MutationReceipt::entry(id, revision_id, change_id)
}

fn current_entry_revision_id(entry: &serde_json::Value) -> Result<String> {
    entry
        .get("revision_id")
        .and_then(serde_json::Value::as_str)
        .filter(|revision_id| !revision_id.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("current entry response is missing revision_id"))
}

fn entry_object_map(
    entry: &serde_json::Value,
    key: &str,
) -> Result<std::collections::BTreeMap<String, serde_json::Value>> {
    let value = entry
        .get(key)
        .ok_or_else(|| anyhow::anyhow!("current entry response is missing {key}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("current entry response field {key} must be an object"))?;
    Ok(object.clone().into_iter().collect())
}

/// Parse one `--field KEY=VALUE` argument. Values stay strings; the shared
/// Rust boundary owns all coercion, so the CLI never interprets types here.
fn parse_field_arg(arg: &str) -> Result<(String, serde_json::Value), UsageError> {
    let (key, value) = arg.split_once('=').ok_or_else(|| {
        UsageError(format!(
            "--field must be KEY=VALUE, got {arg:?}; complex values belong in --fields-file JSON"
        ))
    })?;
    if key.is_empty() {
        return Err(UsageError(format!(
            "--field must be KEY=VALUE with a non-empty key, got {arg:?}"
        )));
    }
    Ok((
        key.to_string(),
        serde_json::Value::String(value.to_string()),
    ))
}

/// Read one `--fields-file` JSON object (or `-` for explicit stdin). Values
/// stay typed JSON; the shared Rust boundary owns coercion and validation.
fn read_fields_file(
    path: &str,
) -> Result<std::collections::BTreeMap<String, serde_json::Value>, anyhow::Error> {
    let text = if path == "-" {
        use std::io::Read;
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .map_err(|error| anyhow::anyhow!("read stdin: {error}"))?;
        text
    } else {
        std::fs::read_to_string(path)
            .map_err(|error| UsageError(format!("read --fields-file {path}: {error}")))?
    };
    let parsed: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| UsageError(format!("parse --fields-file {path}: {error}")))?;
    match parsed {
        serde_json::Value::Object(map) => Ok(map.into_iter().collect()),
        _ => Err(UsageError(format!(
            "--fields-file {path} must contain a JSON object mapping field names to values"
        ))
        .into()),
    }
}

/// Merge `--field` and `--fields-file` inputs. Duplicate keys are a
/// deterministic usage error so concurrent authors never silently win.
fn merge_structured_fields(
    fields: Vec<String>,
    fields_files: Vec<String>,
) -> Result<std::collections::BTreeMap<String, serde_json::Value>, anyhow::Error> {
    let mut merged = std::collections::BTreeMap::new();
    let mut duplicates = std::collections::BTreeSet::new();
    for path in &fields_files {
        for (key, value) in read_fields_file(path)? {
            if merged.insert(key.clone(), value).is_some() {
                duplicates.insert(key);
            }
        }
    }
    for arg in &fields {
        let (key, value) = parse_field_arg(arg).map_err(anyhow::Error::from)?;
        if merged.insert(key.clone(), value).is_some() {
            duplicates.insert(key);
        }
    }
    if !duplicates.is_empty() {
        let names: Vec<String> = duplicates.into_iter().collect();
        let noun = if names.len() == 1 { "field" } else { "fields" };
        return Err(UsageError(format!(
            "duplicate {noun} {} from --field/--fields-file; specify each field once",
            names.join(", ")
        ))
        .into());
    }
    Ok(merged)
}

#[allow(clippy::too_many_arguments)]
async fn create_structured_entry(
    target: &SpaceTarget,
    fmt: &Format,
    entry_id: String,
    form: Option<String>,
    fields: Vec<String>,
    fields_files: Vec<String>,
    author: Option<String>,
) -> Result<()> {
    let Some(form_name) = form else {
        return Err(
            UsageError("--form is required for structured entry create".to_string()).into(),
        );
    };
    let merged = merge_structured_fields(fields, fields_files)?;
    if let SpaceTarget::Remote { space_uid, .. } = target {
        if author.is_some() {
            return Err(UsageError(
                "entry create --author is only supported in core mode; backend/api derive author from the authenticated identity"
                    .to_string(),
            )
            .into());
        }
        let body = serde_json::json!({
            "id": entry_id,
            "form": form_name,
            "fields": merged,
        });
        let result = http::execute_for_target(
            target,
            "entry.create",
            serde_json::json!({"space_id": space_uid}),
            Some(body),
        )
        .await?;
        let receipt = entry_receipt(
            entry_id,
            result
                .get("revision_id")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            result
                .get("change_id")
                .and_then(|value| value.as_str())
                .map(str::to_string),
        );
        emit_success(
            &result,
            fmt,
            Some(render_receipt(&receipt, &stdout_style())),
        );
        return Ok(());
    }
    let SpaceTarget::Core { root, space_id } = target else {
        anyhow::bail!("operation entry.create does not use the remote transport")
    };
    let author = author.unwrap_or_else(|| "cli".to_string());
    let service = UgoiteService::new_without_background_refresh(root)?;
    let (meta, _commit_receipt) = service
        .create_structured_entry_with_receipt(
            space_id,
            &entry_id,
            form_name,
            Vec::new(),
            merged,
            std::collections::BTreeMap::new(),
            &author,
        )
        .await?;
    let receipt = entry_receipt(
        entry_id,
        meta.get("revision_id")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        meta.get("change_id")
            .and_then(|value| value.as_str())
            .map(str::to_string),
    );
    emit_success(&meta, fmt, Some(render_receipt(&receipt, &stdout_style())));
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn update_structured_entry(
    target: &SpaceTarget,
    fmt: &Format,
    entry_id: String,
    form: Option<String>,
    fields: Vec<String>,
    fields_files: Vec<String>,
    parent_revision_id: Option<String>,
    author: String,
) -> Result<()> {
    if fields.is_empty() && fields_files.is_empty() {
        return Err(UsageError(
            "structured update requires --field or --fields-file carrying the complete post-update field map"
                .to_string(),
        )
        .into());
    }
    let fields = merge_structured_fields(fields, fields_files)?;
    if let SpaceTarget::Remote { space_uid, .. } = target {
        if author != "cli" {
            return Err(UsageError(
                "entry update --author is only supported in core mode; backend/api derive author from the authenticated identity"
                    .to_string(),
            )
            .into());
        }
        let current = http::execute_for_target(
            target,
            "entry.get",
            serde_json::json!({"space_id": space_uid, "entry_id": entry_id}),
            None,
        )
        .await?;
        let mut extra_attributes = entry_object_map(&current, "extra_attributes")?;
        // Explicit structured inputs are the complete post-update field map:
        // they replace preserved extra_attributes on key overlap. The shared
        // Rust boundary rejects overlap instead of preferring a side, so the
        // caller resolves it here, explicitly, before sending.
        for key in fields.keys() {
            extra_attributes.remove(key);
        }
        let parent_revision_id = match parent_revision_id {
            Some(parent_revision_id) => parent_revision_id,
            None => current_entry_revision_id(&current)?,
        };
        let mut body = serde_json::json!({
            "fields": fields,
            "extra_attributes": extra_attributes,
        });
        if let Some(form) = form.as_deref() {
            body["form"] = serde_json::json!(form);
        }
        body["parent_revision_id"] = serde_json::json!(parent_revision_id);
        let result = http::execute_for_target(
            target,
            "entry.update",
            serde_json::json!({"space_id": space_uid, "entry_id": entry_id}),
            Some(body),
        )
        .await?;
        let receipt = entry_receipt(
            entry_id,
            result
                .get("revision_id")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            result
                .get("change_id")
                .and_then(|value| value.as_str())
                .map(str::to_string),
        );
        emit_success(
            &result,
            fmt,
            Some(render_receipt(&receipt, &stdout_style())),
        );
        return Ok(());
    }
    let SpaceTarget::Core { root, space_id } = target else {
        anyhow::bail!("operation entry.update does not use the remote transport")
    };
    let service = UgoiteService::new_without_background_refresh(root)?;
    let current = service.get_entry(space_id, &entry_id).await?;
    let mut extra_attributes = entry_object_map(&current, "extra_attributes")?;
    // Explicit structured inputs are the complete post-update field map:
    // they replace preserved extra_attributes on key overlap. The shared
    // Rust boundary rejects overlap instead of preferring a side, so the
    // caller resolves it here, explicitly, before sending.
    for key in fields.keys() {
        extra_attributes.remove(key);
    }
    let parent_revision_id = match parent_revision_id {
        Some(parent_revision_id) => parent_revision_id,
        None => current_entry_revision_id(&current)?,
    };
    let result = service
        .update_structured_entry(
            space_id,
            &entry_id,
            form,
            fields,
            extra_attributes,
            Some(&parent_revision_id),
            &author,
        )
        .await?;
    let receipt = entry_receipt(
        entry_id,
        result
            .get("revision_id")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        result
            .get("change_id")
            .and_then(|value| value.as_str())
            .map(str::to_string),
    );
    emit_success(
        &result,
        fmt,
        Some(render_receipt(&receipt, &stdout_style())),
    );
    Ok(())
}

pub async fn run(
    cmd: EntryCmd,
    explicit_config: Option<&std::path::Path>,
    context_override: Option<&str>,
) -> Result<()> {
    let fmt = effective_format(cmd.format);
    match cmd.sub {
        EntrySubCmd::List => {
            let target = resolve_command_target(explicit_config, context_override, "entry list")?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let result = http::execute_for_target(
                    &target,
                    "entry.list",
                    serde_json::json!({"space_id": space_uid}),
                    None,
                )
                .await?;
                if fmt != Format::Json {
                    if let Some(arr) = result.as_array() {
                        print_json_table(arr, &[("ID", "id")]);
                        return Ok(());
                    }
                }
                emit_success(&result, &fmt, None);
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation entry.list does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            let entries = service.list_entries(space_id).await?;
            if fmt != Format::Json {
                let rows: Vec<serde_json::Value> = entries
                    .iter()
                    .map(|e| {
                        serde_json::json!({
                            "id": e.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                        })
                    })
                    .collect();
                print_json_table(&rows, &[("ID", "id")]);
            } else {
                emit_success(&entries, &fmt, None);
            }
        }
        EntrySubCmd::Get { entry_id } => {
            let target = resolve_command_target(explicit_config, context_override, "entry get")?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let result = http::execute_for_target(
                    &target,
                    "entry.get",
                    serde_json::json!({"space_id": space_uid, "entry_id": entry_id}),
                    None,
                )
                .await?;
                emit_success(&result, &fmt, None);
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation entry.get does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            let entry = service.get_entry(space_id, &entry_id).await?;
            emit_success(&entry, &fmt, None);
        }
        EntrySubCmd::Create {
            entry_id,
            form,
            fields,
            fields_files,
            author,
        } => {
            let target = resolve_command_target(explicit_config, context_override, "entry create")?;
            create_structured_entry(&target, &fmt, entry_id, form, fields, fields_files, author)
                .await?;
        }
        EntrySubCmd::Update {
            entry_id,
            form,
            fields,
            fields_files,
            parent_revision_id,
            author,
        } => {
            let target = resolve_command_target(explicit_config, context_override, "entry update")?;
            update_structured_entry(
                &target,
                &fmt,
                entry_id,
                form,
                fields,
                fields_files,
                parent_revision_id,
                author,
            )
            .await?;
        }
        EntrySubCmd::Delete {
            entry_id,
            hard_delete,
            human_approval,
            author,
        } => {
            let target = resolve_command_target(explicit_config, context_override, "entry delete")?;
            let human_approval =
                human_approval.or_else(|| std::env::var("UGOITE_HUMAN_APPROVAL").ok());
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                if author != "cli" {
                    return Err(UsageError(
                        "entry delete --author is only supported in core mode; backend/api derive actor from the authenticated identity"
                            .to_string(),
                    )
                    .into());
                }
                let result = http::execute_for_target(
                    &target,
                    "entry.delete",
                    serde_json::json!({
                        "space_id": space_uid,
                        "entry_id": entry_id,
                        "hard_delete": hard_delete,
                        "human_approval": human_approval,
                    }),
                    None,
                )
                .await?;
                let receipt = entry_receipt(
                    entry_id,
                    result
                        .get("revision_id")
                        .and_then(|value| value.as_str())
                        .map(str::to_string),
                    result
                        .get("change_id")
                        .and_then(|value| value.as_str())
                        .map(str::to_string),
                );
                emit_success(
                    &result,
                    &fmt,
                    Some(render_receipt(&receipt, &stdout_style())),
                );
                return Ok(());
            }
            if human_approval.is_some() {
                return Err(UsageError(
                    "--human-approval is only supported in backend/api mode".to_string(),
                )
                .into());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation entry.delete does not use the remote transport")
            };
            // Do not wait for Derived refreshes in a one-shot mutation.
            let service = UgoiteService::new_without_background_refresh(root)?;
            let result = service
                .delete_entry_with_receipt(space_id, &entry_id, hard_delete, &author)
                .await?;
            let receipt = entry_receipt(
                entry_id,
                result
                    .get("revision_id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
                result
                    .get("change_id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
            );
            emit_success(
                &result,
                &fmt,
                Some(render_receipt(&receipt, &stdout_style())),
            );
        }
        EntrySubCmd::History { entry_id } => {
            let target =
                resolve_command_target(explicit_config, context_override, "entry history")?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let result = http::execute_for_target(
                    &target,
                    "entry.history",
                    serde_json::json!({"space_id": space_uid, "entry_id": entry_id}),
                    None,
                )
                .await?;
                emit_success(&result, &fmt, None);
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation entry.history does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            let history = service.entry_history(space_id, &entry_id).await?;
            emit_success(&history, &fmt, None);
        }
        EntrySubCmd::Revision {
            entry_id,
            revision_id,
        } => {
            let target =
                resolve_command_target(explicit_config, context_override, "entry revision")?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let result = http::execute_for_target(
                    &target,
                    "entry.revision",
                    serde_json::json!({
                        "space_id": space_uid,
                        "entry_id": entry_id,
                        "revision_id": revision_id,
                    }),
                    None,
                )
                .await?;
                emit_success(&result, &fmt, None);
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation entry.revision does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            let rev = service
                .entry_revision(space_id, &entry_id, &revision_id)
                .await?;
            emit_success(&rev, &fmt, None);
        }
        EntrySubCmd::Restore {
            entry_id,
            revision_id,
            author,
        } => {
            let target =
                resolve_command_target(explicit_config, context_override, "entry restore")?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                if author != "cli" {
                    return Err(UsageError(
                        "entry restore --author is only supported in core mode; backend/api derive author from the authenticated identity"
                            .to_string(),
                    )
                    .into());
                }
                let result = http::execute_for_target(
                    &target,
                    "entry.restore",
                    serde_json::json!({"space_id": space_uid, "entry_id": entry_id}),
                    Some(serde_json::json!({"revision_id": revision_id})),
                )
                .await?;
                let receipt = entry_receipt(
                    entry_id,
                    result
                        .get("revision_id")
                        .and_then(|value| value.as_str())
                        .map(str::to_string),
                    result
                        .get("change_id")
                        .and_then(|value| value.as_str())
                        .map(str::to_string),
                );
                emit_success(
                    &result,
                    &fmt,
                    Some(render_receipt(&receipt, &stdout_style())),
                );
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation entry.restore does not use the remote transport")
            };
            // Do not wait for Derived refreshes in a one-shot mutation.
            let service = UgoiteService::new_without_background_refresh(root)?;
            let result = service
                .restore_entry(space_id, &entry_id, &revision_id, &author)
                .await?;
            let receipt = entry_receipt(
                entry_id,
                result
                    .get("revision_id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
                result
                    .get("change_id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
            );
            emit_success(
                &result,
                &fmt,
                Some(render_receipt(&receipt, &stdout_style())),
            );
        }
    }
    Ok(())
}
