use crate::config::{
    base_url, effective_format, load_config, operator_for_path, print_json, print_json_table,
    resolve_space_reference, space_ws_path, Format,
};
use crate::http;
use anyhow::Result;
use clap::{Args, Subcommand};

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
        long_about = "Create an entry in a space.\n\nThe entry ID is a slug (alphanumeric + hyphens). Content is a Markdown string.\n\nExamples:\n  # Core mode - create a note\n  ugoite entry create /root/spaces/my-space my-note --content $'---\\nform: Note\\n---\\n# My Note\\n\\n## Body\\n\\nHello world.'\n\n  # Backend mode - minimal entry\n  ugoite entry create my-space task-01 --content '# Task 01'\n\n  # With custom author\n  ugoite entry create my-space my-note --content '# Note' --author alice"
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
            default_value = "# New Entry\n",
            allow_hyphen_values = true,
            help = "Entry content as a Markdown string (supports frontmatter for form/tags)"
        )]
        content: String,
        #[arg(
            long,
            default_value = "cli",
            help = "Author name to record in the revision history"
        )]
        author: String,
    },
    /// Update an entry
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
            help = "Updated entry content as a Markdown string (must keep the same form frontmatter)"
        )]
        markdown: String,
        #[arg(
            long,
            help = "Expected current revision ID to enforce optimistic concurrency checks"
        )]
        parent_revision_id: Option<String>,
        #[arg(
            long,
            help = "JSON array of asset objects to persist with the updated entry revision"
        )]
        assets: Option<String>,
        #[arg(
            long,
            default_value = "cli",
            help = "Author name to record in the revision history"
        )]
        author: String,
    },
    /// Delete an entry
    Delete {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
        entry_id: String,
        #[arg(long)]
        hard_delete: bool,
    },
    /// Get entry history
    History {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
        entry_id: String,
    },
    /// Get a specific revision
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
    Restore {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
        entry_id: String,
        revision_id: String,
        #[arg(long, default_value = "cli")]
        author: String,
    },
}

pub async fn run(cmd: EntryCmd) -> Result<()> {
    let config = load_config();
    let fmt = effective_format(cmd.format);
    match cmd.sub {
        EntrySubCmd::List { space_path } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "entry list")?;
            if let Some(base) = base_url(&config) {
                let result = http::http_get(&format!("{base}/spaces/{space_id}/entries")).await?;
                if fmt != Format::Json {
                    if let Some(arr) = result.as_array() {
                        print_json_table(arr, &[("ID", "id"), ("TITLE", "title")]);
                        return Ok(());
                    }
                }
                print_json(&result);
                return Ok(());
            }
            let op = operator_for_path(&root)?;
            let ws = space_ws_path(&root, &space_id);
            let entries = ugoite_core::entry::list_entries(&op, &ws).await?;
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
            if let Some(base) = base_url(&config) {
                let result =
                    http::http_get(&format!("{base}/spaces/{space_id}/entries/{entry_id}")).await?;
                print_json(&result);
                return Ok(());
            }
            let op = operator_for_path(&root)?;
            let ws = space_ws_path(&root, &space_id);
            let entry = ugoite_core::entry::get_entry(&op, &ws, &entry_id).await?;
            print_json(&entry);
        }
        EntrySubCmd::Create {
            space_path,
            entry_id,
            content,
            author,
        } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "entry create")?;
            if let Some(base) = base_url(&config) {
                let result = http::http_post(
                    &format!("{base}/spaces/{space_id}/entries/{entry_id}"),
                    &serde_json::json!({"content": content, "author": author}),
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let op = operator_for_path(&root)?;
            let ws = space_ws_path(&root, &space_id);
            let integrity =
                ugoite_core::integrity::RealIntegrityProvider::from_space(&op, &space_id).await?;
            let meta = ugoite_core::entry::create_entry(
                &op, &ws, &entry_id, &content, &author, &integrity,
            )
            .await?;
            print_json(&meta);
        }
        EntrySubCmd::Update {
            space_path,
            entry_id,
            markdown,
            parent_revision_id,
            assets,
            author,
        } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "entry update")?;
            if let Some(base) = base_url(&config) {
                let mut body = serde_json::json!({"markdown": markdown, "author": author});
                if let Some(p) = &parent_revision_id {
                    body["parent_revision_id"] = serde_json::json!(p);
                }
                if let Some(a) = &assets {
                    let v: serde_json::Value = serde_json::from_str(a)?;
                    body["assets"] = v;
                }
                let result = http::http_put(
                    &format!("{base}/spaces/{space_id}/entries/{entry_id}"),
                    &body,
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let op = operator_for_path(&root)?;
            let ws = space_ws_path(&root, &space_id);
            let integrity =
                ugoite_core::integrity::RealIntegrityProvider::from_space(&op, &space_id).await?;
            let assets_vec: Option<Vec<serde_json::Value>> = if let Some(a) = assets {
                Some(serde_json::from_str(&a)?)
            } else {
                None
            };
            let result = ugoite_core::entry::update_entry(
                &op,
                &ws,
                &entry_id,
                &markdown,
                parent_revision_id.as_deref(),
                &author,
                assets_vec,
                &integrity,
            )
            .await?;
            print_json(&result);
        }
        EntrySubCmd::Delete {
            space_path,
            entry_id,
            hard_delete,
        } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "entry delete")?;
            if let Some(base) = base_url(&config) {
                let url = if hard_delete {
                    format!("{base}/spaces/{space_id}/entries/{entry_id}?hard_delete=true")
                } else {
                    format!("{base}/spaces/{space_id}/entries/{entry_id}")
                };
                let result = http::http_delete(&url).await?;
                print_json(&result);
                return Ok(());
            }
            let op = operator_for_path(&root)?;
            let ws = space_ws_path(&root, &space_id);
            ugoite_core::entry::delete_entry(&op, &ws, &entry_id, hard_delete).await?;
            print_json(&serde_json::json!({"deleted": true}));
        }
        EntrySubCmd::History {
            space_path,
            entry_id,
        } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "entry history")?;
            if let Some(base) = base_url(&config) {
                let result = http::http_get(&format!(
                    "{base}/spaces/{space_id}/entries/{entry_id}/history"
                ))
                .await?;
                print_json(&result);
                return Ok(());
            }
            let op = operator_for_path(&root)?;
            let ws = space_ws_path(&root, &space_id);
            let history = ugoite_core::entry::get_entry_history(&op, &ws, &entry_id).await?;
            print_json(&history);
        }
        EntrySubCmd::Revision {
            space_path,
            entry_id,
            revision_id,
        } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "entry revision")?;
            if let Some(base) = base_url(&config) {
                let result = http::http_get(&format!(
                    "{base}/spaces/{space_id}/entries/{entry_id}/revisions/{revision_id}"
                ))
                .await?;
                print_json(&result);
                return Ok(());
            }
            let op = operator_for_path(&root)?;
            let ws = space_ws_path(&root, &space_id);
            let rev =
                ugoite_core::entry::get_entry_revision(&op, &ws, &entry_id, &revision_id).await?;
            print_json(&rev);
        }
        EntrySubCmd::Restore {
            space_path,
            entry_id,
            revision_id,
            author,
        } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "entry restore")?;
            if let Some(base) = base_url(&config) {
                let result = http::http_post(
                    &format!("{base}/spaces/{space_id}/entries/{entry_id}/restore/{revision_id}"),
                    &serde_json::json!({"author": author}),
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let op = operator_for_path(&root)?;
            let ws = space_ws_path(&root, &space_id);
            let integrity =
                ugoite_core::integrity::RealIntegrityProvider::from_space(&op, &space_id).await?;
            let result = ugoite_core::entry::restore_entry(
                &op,
                &ws,
                &entry_id,
                &revision_id,
                &author,
                &integrity,
            )
            .await?;
            print_json(&result);
        }
    }
    Ok(())
}
