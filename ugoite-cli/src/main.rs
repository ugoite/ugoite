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
    #[command(hide = true)]
    Link(commands::link::LinkCmd),
    /// Create a new space (deprecated: use `space create` instead)
    /// Create a new space
    #[command(
        hide = true,
        long_about = "Create a new space.\n\nExamples:\n  # Core mode (workspace root)\n  ugoite create-space my-space --root /root\n\n  # Backend mode (requires: ugoite config set --mode backend ...)\n  ugoite create-space my-space"
    )]
    CreateSpace {
        #[arg(
            long = "root",
            value_name = "LOCAL_ROOT",
            help = "Workspace root that contains the spaces/ directory in core mode."
        )]
        root_path: Option<String>,
        #[arg(
            value_name = "SPACE_ID",
            help = "New space ID (alphanumeric + hyphens, e.g. 'my-project')"
        )]
        space_id: String,
    },
    /// Query the index using SQL
    ///
    /// Examples:
    ///   # List all entries in a space (core mode)
    ///   ugoite query /root/spaces/my-space --sql "SELECT id, title FROM entries LIMIT 10"
    ///
    ///   # Filter by form type
    ///   ugoite query /root/spaces/my-space --sql "SELECT id, title FROM entries WHERE form='note'"
    ///
    ///   # Paginate results
    ///   ugoite query my-space --sql "SELECT id FROM entries" --limit 20 --offset 40
    #[command(
        long_about = "Query the index using SQL.\n\nThe SQL dialect is SQLite. The queryable table is `entries` with columns: id, title, body, form, tags, created_at, updated_at.\n\nExamples:\n  # Core mode (full path)\n  ugoite query /root/spaces/my-space --sql \"SELECT id, title FROM entries LIMIT 10\"\n\n  # Backend/API mode (space ID only)\n  ugoite query my-space --sql \"SELECT id, title FROM entries WHERE form='note'\"\n\n  # Paginate results\n  ugoite query my-space --sql \"SELECT id FROM entries\" --limit 20 --offset 40"
    )]
    Query {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
        #[arg(
            long,
            help = "SQL query to run against the indexed entries (SQLite dialect). Table: entries. Columns: id, title, body, form, tags, created_at, updated_at.\n\nExample: \"SELECT id, title FROM entries LIMIT 10\""
        )]
        sql: String,
        #[arg(long, default_value_t = 100, help = "Maximum number of rows to return")]
        limit: u64,
        #[arg(long, default_value_t = 0, help = "Row offset for pagination")]
        offset: u64,
        #[arg(long, help = "Filter entries by form type (e.g. 'note', 'task')")]
        form: Option<String>,
        #[arg(long, help = "Filter entries by tag")]
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
        } => {
            commands::space::create_space_cmd(root_path.as_deref(), &space_id, "create-space").await
        }
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
