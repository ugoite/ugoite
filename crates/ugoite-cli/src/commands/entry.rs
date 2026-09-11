use crate::config::{load_config, resolve_space_reference, validated_base_url};
use crate::http;
use crate::output::{
    effective_format, emit_success, print_json, print_json_table, read_compat_input, Format,
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
        long_about = "Create an entry in a space.\n\nThe entry ID is a slug (alphanumeric + hyphens). Content is a Markdown string. Frontmatter is optional and only needed when you want form-backed metadata.\n\nExamples:\n  # Core mode - minimal note\n  ugoite entry create /root/spaces/my-space my-note --content '# My Note'\n\n  # Core mode - read content from a file\n  ugoite entry create /root/spaces/my-space my-note --file ./note.md\n\n  # Core mode - read content from explicit stdin\n  cat ./note.md | ugoite entry create /root/spaces/my-space my-note --file -\n\n  # Core mode - note with form frontmatter\n  ugoite entry create /root/spaces/my-space my-note --content $'---\\nform: Note\\n---\\n# My Note\\n\\n## Body\\n\\nHello world.'\n\n  # Backend mode - minimal entry\n  ugoite entry create my-space task-01 --content '# Task 01'\n\n  # Core mode with custom author\n  ugoite entry create /root/spaces/my-space my-note --content '# Note' --author alice"
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

fn entry_receipt(id: String, revision_id: Option<String>) -> MutationReceipt {
    // Change/run IDs are None here: the CLI never fabricates them. Durable
    // Knowledge Change ID exposure from the commit boundary is follow-up.
    MutationReceipt::entry(id, revision_id, None)
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
                print_json(&result);
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
                print_json(&entries);
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
                print_json(&result);
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let entry = service.get_entry(&space_id, &entry_id).await?;
            print_json(&entry);
        }
        EntrySubCmd::Create {
            space_path,
            entry_id,
            content,
            file,
            author,
        } => {
            // Shell-safe compatibility ingress: inline and file are mutually
            // exclusive; neither falls back to the historical default note.
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
                let revision = result
                    .get("revision_id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string);
                let receipt = entry_receipt(entry_id, revision);
                emit_success(&receipt.value(), &fmt, Some(receipt.human()));
                return Ok(());
            }
            let author = author.unwrap_or_else(|| "cli".to_string());
            // A mutation schedules the process-local coalesced refresh but
            // never drains it; the authoritative commit is the CLI latency
            // boundary and `ugoite index run` is the explicit repair command.
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let meta = service
                .create_entry(&space_id, &entry_id, &content, &author)
                .await?;
            let revision = meta
                .get("revision_id")
                .and_then(|value| value.as_str())
                .map(str::to_string);
            let receipt = entry_receipt(entry_id, revision);
            emit_success(&receipt.value(), &fmt, Some(receipt.human()));
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
                let revision = result
                    .get("revision_id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string);
                let receipt = entry_receipt(entry_id, revision);
                emit_success(&receipt.value(), &fmt, Some(receipt.human()));
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
            let revision = result
                .get("revision_id")
                .and_then(|value| value.as_str())
                .map(str::to_string);
            let receipt = entry_receipt(entry_id, revision);
            emit_success(&receipt.value(), &fmt, Some(receipt.human()));
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
                print_json(&result);
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
            service
                .delete_entry(&space_id, &entry_id, hard_delete, &author)
                .await?;
            print_json(&serde_json::json!({"deleted": true}));
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
                print_json(&result);
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let history = service.entry_history(&space_id, &entry_id).await?;
            print_json(&history);
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
                print_json(&result);
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let rev = service
                .entry_revision(&space_id, &entry_id, &revision_id)
                .await?;
            print_json(&rev);
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
                print_json(&result);
                return Ok(());
            }
            // Do not wait for Derived refreshes in a one-shot mutation.
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let result = service
                .restore_entry(&space_id, &entry_id, &revision_id, &author)
                .await?;
            print_json(&result);
        }
    }
    Ok(())
}
