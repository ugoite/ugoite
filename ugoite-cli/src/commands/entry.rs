use crate::config::{
    base_url, load_config, operator_for_path, parse_space_path, print_json, space_ws_path,
};
use crate::http;
use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Args)]
pub struct EntryCmd {
    #[command(subcommand)]
    pub sub: EntrySubCmd,
}

#[derive(Subcommand)]
pub enum EntrySubCmd {
    /// List entries in a space
    List { space_path: String },
    /// Get an entry
    Get {
        space_path: String,
        entry_id: String,
    },
    /// Create an entry
    Create {
        space_path: String,
        entry_id: String,
        #[arg(long, default_value = "# New Entry\n")]
        content: String,
        #[arg(long, default_value = "cli")]
        author: String,
    },
    /// Update an entry
    Update {
        space_path: String,
        entry_id: String,
        #[arg(long)]
        markdown: String,
        #[arg(long)]
        parent_revision_id: Option<String>,
        #[arg(long)]
        assets: Option<String>,
        #[arg(long, default_value = "cli")]
        author: String,
    },
    /// Delete an entry
    Delete {
        space_path: String,
        entry_id: String,
        #[arg(long)]
        hard_delete: bool,
    },
    /// Get entry history
    History {
        space_path: String,
        entry_id: String,
    },
    /// Get a specific revision
    Revision {
        space_path: String,
        entry_id: String,
        revision_id: String,
    },
    /// Restore an entry to a revision
    Restore {
        space_path: String,
        entry_id: String,
        revision_id: String,
        #[arg(long, default_value = "cli")]
        author: String,
    },
}

pub async fn run(cmd: EntryCmd) -> Result<()> {
    let config = load_config();
    match cmd.sub {
        EntrySubCmd::List { space_path } => {
            let (root, space_id) = parse_space_path(&space_path);
            if let Some(base) = base_url(&config) {
                let result = http::http_get(&format!("{base}/spaces/{space_id}/entries"))?;
                print_json(&result);
                return Ok(());
            }
            let op = operator_for_path(&root)?;
            let ws = space_ws_path(&root, &space_id);
            let entries = ugoite_core::entry::list_entries(&op, &ws).await?;
            print_json(&entries);
        }
        EntrySubCmd::Get {
            space_path,
            entry_id,
        } => {
            let (root, space_id) = parse_space_path(&space_path);
            if let Some(base) = base_url(&config) {
                let result =
                    http::http_get(&format!("{base}/spaces/{space_id}/entries/{entry_id}"))?;
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
            let (root, space_id) = parse_space_path(&space_path);
            if let Some(base) = base_url(&config) {
                let result = http::http_post(
                    &format!("{base}/spaces/{space_id}/entries/{entry_id}"),
                    &serde_json::json!({"content": content, "author": author}),
                )?;
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
            let (root, space_id) = parse_space_path(&space_path);
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
                )?;
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
            let (root, space_id) = parse_space_path(&space_path);
            if let Some(base) = base_url(&config) {
                let url = if hard_delete {
                    format!("{base}/spaces/{space_id}/entries/{entry_id}?hard_delete=true")
                } else {
                    format!("{base}/spaces/{space_id}/entries/{entry_id}")
                };
                let result = http::http_delete(&url)?;
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
            let (root, space_id) = parse_space_path(&space_path);
            if let Some(base) = base_url(&config) {
                let result = http::http_get(&format!(
                    "{base}/spaces/{space_id}/entries/{entry_id}/history"
                ))?;
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
            let (root, space_id) = parse_space_path(&space_path);
            if let Some(base) = base_url(&config) {
                let result = http::http_get(&format!(
                    "{base}/spaces/{space_id}/entries/{entry_id}/revisions/{revision_id}"
                ))?;
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
            let (root, space_id) = parse_space_path(&space_path);
            if let Some(base) = base_url(&config) {
                let result = http::http_post(
                    &format!("{base}/spaces/{space_id}/entries/{entry_id}/restore/{revision_id}"),
                    &serde_json::json!({"author": author}),
                )?;
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
