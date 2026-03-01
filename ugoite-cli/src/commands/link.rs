use anyhow::Result;
use clap::Subcommand;

#[derive(Debug, clap::Args)]
pub struct LinkCmd {
    #[command(subcommand)]
    pub cmd: LinkSubCmd,
}

#[derive(Debug, Subcommand)]
pub enum LinkSubCmd {
    /// Create a link between entries
    Create {
        space_root: String,
        space_id: String,
        source_id: String,
        target_id: String,
        #[arg(long)]
        kind: Option<String>,
    },
    /// List links for an entry
    List {
        space_root: String,
        space_id: String,
        entry_id: String,
    },
    /// Delete a link
    Delete {
        space_root: String,
        space_id: String,
        link_id: String,
    },
}

pub async fn cmd_link_create(_space_root: &str, _space_id: &str, _source_id: &str, _target_id: &str, _kind: Option<&str>) -> Result<()> {
    anyhow::bail!("Link commands removed. Use row_reference fields instead.")
}

pub async fn cmd_link_list(_space_root: &str, _space_id: &str, _entry_id: &str) -> Result<()> {
    anyhow::bail!("Link commands removed. Use row_reference fields instead.")
}

pub async fn cmd_link_delete(_space_root: &str, _space_id: &str, _link_id: &str) -> Result<()> {
    anyhow::bail!("Link commands removed. Use row_reference fields instead.")
}

pub async fn run(cmd: LinkCmd) -> Result<()> {
    match cmd.cmd {
        LinkSubCmd::Create { space_root, space_id, source_id, target_id, kind } => {
            cmd_link_create(&space_root, &space_id, &source_id, &target_id, kind.as_deref()).await
        }
        LinkSubCmd::List { space_root, space_id, entry_id } => {
            cmd_link_list(&space_root, &space_id, &entry_id).await
        }
        LinkSubCmd::Delete { space_root, space_id, link_id } => {
            cmd_link_delete(&space_root, &space_id, &link_id).await
        }
    }
}
