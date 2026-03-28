use anyhow::Result;
use clap::{Parser, Subcommand};
use ugoite_cli::commands;

#[derive(Parser)]
#[command(name = "ugoite", about = "Ugoite CLI - Knowledge base management")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Authentication helpers
    Auth(commands::auth::AuthCmd),
    /// CLI endpoint routing settings
    Config(commands::config::ConfigCmd),
    /// Space management commands
    Space(commands::space::SpaceCmd),
    /// Entry management commands
    Entry(commands::entry::EntryCmd),
    /// Form management commands
    Form(commands::form::FormCmd),
    /// Asset management commands
    Asset(commands::asset::AssetCmd),
    /// Search commands
    Search(commands::search::SearchCmd),
    /// SQL linting and completion commands
    Sql(commands::sql::SqlCmd),
    /// Indexer operations
    Index(commands::index::IndexCmd),
    /// Link management commands (deprecated: use row_reference fields)
    Link(commands::link::LinkCmd),
    /// Create a new space
    #[command(long_about = "Create a new space.\n\nExamples:\n  # Core mode (local filesystem)\n  ugoite create-space my-space --root /root/spaces\n\n  # Using UGOITE_ROOT env var\n  ugoite create-space my-space\n\n  # Backend mode (requires: ugoite config set --mode backend ...)\n  ugoite create-space my-space")]
    CreateSpace {
        #[arg(long = "root", value_name = "LOCAL_ROOT", help = "Local filesystem root for spaces (core mode). Falls back to UGOITE_ROOT env var.")]
        root_path: Option<String>,
        #[arg(value_name = "SPACE_ID", help = "New space ID (alphanumeric + hyphens, e.g. 'my-project')")]
        space_id: String,
    },
    /// Query the index
    Query {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
        #[arg(long)]
        sql: String,
        #[arg(long, default_value_t = 100)]
        limit: u64,
        #[arg(long, default_value_t = 0)]
        offset: u64,
        #[arg(long)]
        form: Option<String>,
        #[arg(long)]
        tag: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(run(cli));
    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Auth(cmd) => commands::auth::run(cmd).await,
        Commands::Config(cmd) => commands::config::run(cmd).await,
        Commands::Space(cmd) => commands::space::run(cmd).await,
        Commands::Entry(cmd) => commands::entry::run(cmd).await,
        Commands::Form(cmd) => commands::form::run(cmd).await,
        Commands::Asset(cmd) => commands::asset::run(cmd).await,
        Commands::Search(cmd) => commands::search::run(cmd).await,
        Commands::Sql(cmd) => commands::sql::run(cmd).await,
        Commands::Index(cmd) => commands::index::run(cmd).await,
        Commands::Link(cmd) => commands::link::run(cmd).await,
        Commands::CreateSpace {
            root_path,
            space_id,
        } => commands::space::create_space_cmd(root_path.as_deref(), &space_id).await,
        Commands::Query {
            space_path,
            sql,
            limit: _,
            offset: _,
            form: _,
            tag: _,
        } => commands::index::query_cmd(&space_path, &sql).await,
    }
}
