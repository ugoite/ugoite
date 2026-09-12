use crate::config::{load_config, resolve_space_reference, validated_base_url};
use crate::http;
use crate::output::{
    effective_format, emit_success, print_json_table, read_compat_input, Format, MutationReceipt,
    UsageError,
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
    #[command(
        long_about = "List entries in a space.\n\nExamples:\n  # Core mode (local filesystem)\n  ugoite entry list /root/spaces/my-space\n\n  # Backend mode (requires config set --mode backend first)\n  ugoite entry list my-space"
    )]
    List {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
    },
    /// Get an entry by ID
    #[command(
        long_about = "Get an entry by ID.\n\nExamples:\n  # Core mode\n  ugoite entry get /root/spaces/my-space my-entry-id\n\n  # Backend mode\n  ugoite entry get my-space my-entry-id"
    )]
    Get {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
        #[arg(
            value_name = "ENTRY_ID",
            help = "Entry slug/ID (e.g. 'my-note', 'task-01')"
        )]
        entry_id: String,
    },
    /// Create an entry
    #[command(
        long_about = "Create an entry in a space.\n\nThe entry ID is a slug (alphanumeric + hyphens). Content is a Markdown string. Frontmatter is optional and only needed when you want form-backed metadata.\n\nExamples:\n  # Core mode - minimal note\n  ugoite entry create /root/spaces/my-space my-note --content '# My Note'\n\n  # Core mode - read content from a file\n  ugoite entry create /root/spaces/my-space my-note --file ./note.md\n\n  # Core mode - read content from explicit stdin\n  cat ./note.md | ugoite entry create /root/spaces/my-space my-note --file -\n\n  # Core mode - note with form frontmatter\n  ugoite entry create /root/spaces/my-space my-note --content $'---\\nform: Note\\n---\\n# My Note\\n\\n## Body\\n\\nHello world.'\n\n  # Backend mode - minimal entry\n  ugoite entry create my-space task-01 --content '# Task 01'\n\n  # Core mode with custom author\n  ugoite entry create /root/spaces/my-space my-note --content '# Note' --author alice\n\nStructured authoring is recommended; raw Markdown is the 0.1.x compatibility surface.\n\nExamples:\n  # Core mode - structured fields without Markdown\n  ugoite entry create /root/spaces/my-space task-01 --form Task --title 'Ship 0.1.x' --field status=open --field priority=3\n\n  # Core mode - complex values from a JSON object file (or --fields-file - for stdin)\n  ugoite entry create /root/spaces/my-space task-01 --form Task --fields-file fields.json"
    )]
    Create {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
        #[arg(
            value_name = "ENTRY_ID",
            help = "Entry slug/ID (e.g. 'my-note', 'task-01')"
        )]
        entry_id: String,
        #[arg(
            long,
            allow_hyphen_values = true,
            help = "Entry content as a Markdown string (supports frontmatter for form/tags)"
        )]
        content: Option<String>,
        #[arg(
            long,
            value_name = "PATH",
            help = "Read Markdown content from PATH, or from explicit stdin with --file - (cannot combine with --content)"
        )]
        file: Option<String>,
        #[arg(
            long,
            value_name = "FORM",
            help = "Form name for structured authoring (requires no --content/--file)"
        )]
        form: Option<String>,
        #[arg(
            long,
            allow_hyphen_values = true,
            help = "Entry title for structured authoring"
        )]
        title: Option<String>,
        #[arg(
            long = "field",
            value_name = "KEY=VALUE",
            help = "Structured field as KEY=VALUE (repeatable; VALUE stays a string and the shared Rust boundary coerces it)"
        )]
        fields: Vec<String>,
        #[arg(
            long = "fields-file",
            value_name = "PATH",
            help = "Read structured fields as a JSON object from PATH, or from explicit stdin with --fields-file - (repeatable; duplicate keys with --field are an error)"
        )]
        fields_files: Vec<String>,
        #[arg(
            long,
            help = "Author name to record in the revision history (core mode only)"
        )]
        author: Option<String>,
    },
    /// Update an entry
    #[command(
        long_about = "Update an entry in a space.\n\nExamples:\n  # Core mode\n  ugoite entry update /root/spaces/my-space my-note --markdown '# Updated'\n\n  # Core mode - read content from a file\n  ugoite entry update /root/spaces/my-space my-note --file ./note.md\n\n  # Core mode with optimistic concurrency\n  ugoite entry update /root/spaces/my-space my-note --markdown '# Updated' --parent-revision-id rev-1\n\n  # Backend mode\n  ugoite entry update my-space my-note --markdown '# Updated'"
    )]
    Update {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
        #[arg(
            value_name = "ENTRY_ID",
            help = "Entry slug/ID (e.g. 'my-note', 'task-01')"
        )]
        entry_id: String,
        #[arg(
            long,
            allow_hyphen_values = true,
            help = "Updated entry content as a Markdown string (must keep the same form frontmatter)"
        )]
        markdown: Option<String>,
        #[arg(
            long,
            value_name = "PATH",
            help = "Read Markdown content from PATH, or from explicit stdin with --file - (cannot combine with --markdown)"
        )]
        file: Option<String>,
        #[arg(
            long,
            help = "Expected current revision ID to enforce optimistic concurrency checks"
        )]
        parent_revision_id: Option<String>,
        #[arg(
            long,
            default_value = "cli",
            help = "Author name to record in the revision history (core mode only)"
        )]
        author: String,
    },
    /// Delete an entry
    #[command(
        long_about = "Delete an entry from a space.\n\nExamples:\n  # Core mode\n  ugoite entry delete /root/spaces/my-space my-note\n\n  # Backend mode (dangerous: requires a human approval token)\n  ugoite entry delete my-space my-note --human-approval <token>"
    )]
    Delete {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
        entry_id: String,
        #[arg(long)]
        hard_delete: bool,
        /// Single-use approval token issued by a recently reauthenticated human.
        #[arg(long)]
        human_approval: Option<String>,
        #[arg(
            long,
            default_value = "cli",
            help = "Actor name to record for the delete (core mode only)"
        )]
        author: String,
    },
    /// Get entry history
    #[command(
        long_about = "Get the revision history of an entry.\n\nExamples:\n  # Core mode\n  ugoite entry history /root/spaces/my-space my-note\n\n  # Backend mode\n  ugoite entry history my-space my-note"
    )]
    History {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
        entry_id: String,
    },
    /// Get a specific revision
    #[command(
        long_about = "Get a specific revision of an entry.\n\nExamples:\n  # Core mode\n  ugoite entry revision /root/spaces/my-space my-note rev-1\n\n  # Backend mode\n  ugoite entry revision my-space my-note rev-1"
    )]
    Revision {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
        entry_id: String,
        revision_id: String,
    },
    /// Restore an entry to a revision
    #[command(
        long_about = "Restore an entry to a previous revision.\n\nExamples:\n  # Core mode\n  ugoite entry restore /root/spaces/my-space my-note rev-1\n\n  # Backend mode\n  ugoite entry restore my-space my-note rev-1"
    )]
    Restore {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
        entry_id: String,
        revision_id: String,
        #[arg(
            long,
            default_value = "cli",
            help = "Author name to record in the revision history (core mode only)"
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
        return Err(UsageError(format!(
            "duplicate field {} from --field/--fields-file; specify each field once",
            names.join(", ")
        ))
        .into());
    }
    Ok(merged)
}

#[allow(clippy::too_many_arguments)]
async fn create_structured_entry(
    config: &crate::config::EndpointConfig,
    fmt: &Format,
    space_path: String,
    entry_id: String,
    form: Option<String>,
    title: Option<String>,
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
    let (root, space_id) = resolve_space_reference(config, &space_path, "entry create")?;
    if let Some(base) = validated_base_url(config)? {
        if author.is_some() {
            return Err(UsageError(
                "entry create --author is only supported in core mode; backend/api derive author from the authenticated identity"
                    .to_string(),
            )
            .into());
        }
        let mut body = serde_json::json!({
            "id": entry_id,
            "form": form_name,
            "fields": merged,
        });
        if let Some(title) = title.as_deref() {
            body["title"] = serde_json::json!(title);
        }
        let result = http::execute(
            &base,
            "entry.create",
            serde_json::json!({"space_id": space_id}),
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
        emit_success(&result, fmt, Some(receipt.human()));
        return Ok(());
    }
    let author = author.unwrap_or_else(|| "cli".to_string());
    let service = UgoiteService::new_without_background_refresh(&root)?;
    let (mut meta, commit_receipt) = service
        .create_structured_entry_with_receipt(
            &space_id,
            &entry_id,
            title,
            form_name,
            Vec::new(),
            merged,
            std::collections::BTreeMap::new(),
            &author,
        )
        .await?;
    meta["change_id"] = serde_json::json!(commit_receipt.command_id);
    let receipt = entry_receipt(
        entry_id,
        meta.get("revision_id")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        meta.get("change_id")
            .and_then(|value| value.as_str())
            .map(str::to_string),
    );
    emit_success(&meta, fmt, Some(receipt.human()));
    Ok(())
}

pub async fn run(cmd: EntryCmd) -> Result<()> {
    let config = load_config();
    let fmt = effective_format(cmd.format);
    match cmd.sub {
        EntrySubCmd::List { space_path } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "entry list")?;
            if let Some(base) = validated_base_url(&config)? {
                let result = http::execute(
                    &base,
                    "entry.list",
                    serde_json::json!({"space_id": space_id}),
                    None,
                )
                .await?;
                if fmt != Format::Json {
                    if let Some(arr) = result.as_array() {
                        print_json_table(arr, &[("ID", "id"), ("TITLE", "title")]);
                        return Ok(());
                    }
                }
                emit_success(&result, &fmt, None);
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let entries = service.list_entries(&space_id).await?;
            if fmt != Format::Json {
                let rows: Vec<serde_json::Value> = entries
                    .iter()
                    .map(|e| {
                        serde_json::json!({
                            "id": e.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                            "title": e.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                        })
                    })
                    .collect();
                print_json_table(&rows, &[("ID", "id"), ("TITLE", "title")]);
            } else {
                emit_success(&entries, &fmt, None);
            }
        }
        EntrySubCmd::Get {
            space_path,
            entry_id,
        } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "entry get")?;
            if let Some(base) = validated_base_url(&config)? {
                let result = http::execute(
                    &base,
                    "entry.get",
                    serde_json::json!({"space_id": space_id, "entry_id": entry_id}),
                    None,
                )
                .await?;
                emit_success(&result, &fmt, None);
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let entry = service.get_entry(&space_id, &entry_id).await?;
            emit_success(&entry, &fmt, None);
        }
        EntrySubCmd::Create {
            space_path,
            entry_id,
            content,
            file,
            form,
            title,
            fields,
            fields_files,
            author,
        } => {
            let has_structured =
                form.is_some() || title.is_some() || !fields.is_empty() || !fields_files.is_empty();
            let has_markdown = content.is_some() || file.is_some();
            if has_structured && has_markdown {
                return Err(UsageError(
                    "structured options (--form/--title/--field/--fields-file) and --content/--file cannot be combined; specify exactly one input style"
                        .to_string(),
                )
                .into());
            }
            if has_structured {
                return create_structured_entry(
                    &config,
                    &fmt,
                    space_path,
                    entry_id,
                    form,
                    title,
                    fields,
                    fields_files,
                    author,
                )
                .await;
            }
            // Shell-safe compatibility ingress: inline and file are mutually
            // exclusive; neither provided falls back to the default note.
            let content = match (content, file) {
                (Some(_), Some(_)) => {
                    return Err(UsageError(
                        "--content and --file cannot be combined; specify exactly one".to_string(),
                    )
                    .into());
                }
                (Some(text), None) => text,
                (None, Some(path)) => read_compat_input(None, "--content", Some(path))?,
                (None, None) => "# New Entry\n".to_string(),
            };
            let (root, space_id) = resolve_space_reference(&config, &space_path, "entry create")?;
            if let Some(base) = validated_base_url(&config)? {
                if author.is_some() {
                    return Err(UsageError(
                        "entry create --author is only supported in core mode; backend/api derive author from the authenticated identity"
                            .to_string(),
                    )
                    .into());
                }
                let result = http::execute(
                    &base,
                    "entry.create",
                    serde_json::json!({"space_id": space_id}),
                    Some(serde_json::json!({"id": entry_id, "markdown": content})),
                )
                .await?;
                // 0.1.x machine contract: keep the existing output shape.
                // The receipt is TTY display only; switching the machine
                // default to the receipt is a v0.2 interface decision.
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
                emit_success(&result, &fmt, Some(receipt.human()));
                return Ok(());
            }
            let author = author.unwrap_or_else(|| "cli".to_string());
            // A mutation schedules the process-local coalesced refresh but
            // never drains it; the authoritative commit is the CLI latency
            // boundary and `ugoite index run` is the explicit repair command.
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let (mut meta, commit_receipt) = service
                .create_entry_with_receipt(&space_id, &entry_id, &content, &author)
                .await?;
            meta["change_id"] = serde_json::json!(commit_receipt.command_id);
            let receipt = entry_receipt(
                entry_id,
                meta.get("revision_id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
                meta.get("change_id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
            );
            emit_success(&meta, &fmt, Some(receipt.human()));
        }
        EntrySubCmd::Update {
            space_path,
            entry_id,
            markdown,
            file,
            parent_revision_id,
            author,
        } => {
            let markdown = read_compat_input(markdown, "--markdown", file)?;
            let (root, space_id) = resolve_space_reference(&config, &space_path, "entry update")?;
            if let Some(base) = validated_base_url(&config)? {
                if author != "cli" {
                    return Err(UsageError(
                        "entry update --author is only supported in core mode; backend/api derive author from the authenticated identity"
                            .to_string(),
                    )
                    .into());
                }
                let mut body = serde_json::json!({"markdown": markdown});
                if let Some(p) = &parent_revision_id {
                    body["parent_revision_id"] = serde_json::json!(p);
                }
                let result = http::execute(
                    &base,
                    "entry.update",
                    serde_json::json!({"space_id": space_id, "entry_id": entry_id}),
                    Some(body),
                )
                .await?;
                // 0.1.x machine contract: keep the existing output shape (see create).
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
                emit_success(&result, &fmt, Some(receipt.human()));
                return Ok(());
            }
            // Do not wait for Derived refreshes in a one-shot mutation.
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let result = service
                .update_entry(
                    &space_id,
                    &entry_id,
                    &markdown,
                    parent_revision_id.as_deref(),
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
            emit_success(&result, &fmt, Some(receipt.human()));
        }
        EntrySubCmd::Delete {
            space_path,
            entry_id,
            hard_delete,
            human_approval,
            author,
        } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "entry delete")?;
            let human_approval =
                human_approval.or_else(|| std::env::var("UGOITE_HUMAN_APPROVAL").ok());
            if let Some(base) = validated_base_url(&config)? {
                if author != "cli" {
                    return Err(UsageError(
                        "entry delete --author is only supported in core mode; backend/api derive actor from the authenticated identity"
                            .to_string(),
                    )
                    .into());
                }
                let result = http::execute(
                    &base,
                    "entry.delete",
                    serde_json::json!({
                        "space_id": space_id,
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
                emit_success(&result, &fmt, Some(receipt.human()));
                return Ok(());
            }
            if human_approval.is_some() {
                return Err(UsageError(
                    "--human-approval is only supported in backend/api mode".to_string(),
                )
                .into());
            }
            // Do not wait for Derived refreshes in a one-shot mutation.
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let result = service
                .delete_entry_with_receipt(&space_id, &entry_id, hard_delete, &author)
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
            emit_success(&result, &fmt, Some(receipt.human()));
        }
        EntrySubCmd::History {
            space_path,
            entry_id,
        } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "entry history")?;
            if let Some(base) = validated_base_url(&config)? {
                let result = http::execute(
                    &base,
                    "entry.history",
                    serde_json::json!({"space_id": space_id, "entry_id": entry_id}),
                    None,
                )
                .await?;
                emit_success(&result, &fmt, None);
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let history = service.entry_history(&space_id, &entry_id).await?;
            emit_success(&history, &fmt, None);
        }
        EntrySubCmd::Revision {
            space_path,
            entry_id,
            revision_id,
        } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "entry revision")?;
            if let Some(base) = validated_base_url(&config)? {
                let result = http::execute(
                    &base,
                    "entry.revision",
                    serde_json::json!({
                        "space_id": space_id,
                        "entry_id": entry_id,
                        "revision_id": revision_id,
                    }),
                    None,
                )
                .await?;
                emit_success(&result, &fmt, None);
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let rev = service
                .entry_revision(&space_id, &entry_id, &revision_id)
                .await?;
            emit_success(&rev, &fmt, None);
        }
        EntrySubCmd::Restore {
            space_path,
            entry_id,
            revision_id,
            author,
        } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "entry restore")?;
            if let Some(base) = validated_base_url(&config)? {
                if author != "cli" {
                    return Err(UsageError(
                        "entry restore --author is only supported in core mode; backend/api derive author from the authenticated identity"
                            .to_string(),
                    )
                    .into());
                }
                let result = http::execute(
                    &base,
                    "entry.restore",
                    serde_json::json!({"space_id": space_id, "entry_id": entry_id}),
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
                emit_success(&result, &fmt, Some(receipt.human()));
                return Ok(());
            }
            // Do not wait for Derived refreshes in a one-shot mutation.
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let result = service
                .restore_entry(&space_id, &entry_id, &revision_id, &author)
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
            emit_success(&result, &fmt, Some(receipt.human()));
        }
    }
    Ok(())
}
