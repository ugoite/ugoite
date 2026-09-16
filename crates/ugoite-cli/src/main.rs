#![recursion_limit = "256"]

use anyhow::Result;
use clap::{Parser, Subcommand};
use ugoite_cli::commands;
use ugoite_cli::output::{project_error, render_error, stderr_style};

const QUIET_ACCENT_STYLES: clap::builder::Styles = clap::builder::Styles::styled()
    .header(anstyle::Style::new().bold())
    .usage(anstyle::Style::new().bold())
    .literal(anstyle::AnsiColor::Cyan.on_default())
    .placeholder(anstyle::Style::new().dimmed())
    .error(anstyle::AnsiColor::Red.on_default().bold())
    .valid(anstyle::AnsiColor::Green.on_default())
    .invalid(anstyle::AnsiColor::Yellow.on_default())
    .context(anstyle::Style::new().dimmed())
    .context_value(anstyle::Style::new());

#[derive(Parser)]
#[command(
    name = "ugoite",
    about = "Ugoite CLI - Knowledge base management",
    version = env!("CARGO_PKG_VERSION"),
    long_about = "Ugoite CLI - Knowledge base management\n\nQuick start (local-first / core mode):\n  # Inspect the spaces in your current workspace\n  ugoite space list .\n\n  # Create your first space with an explicit local spaces path\n  ugoite space create /path/to/workspace/spaces/demo\n\nQuick start (backend / API mode):\n  # Point the CLI at your backend\n  ugoite config set --mode backend --backend-url http://localhost:8000\n\n  # Authenticate, then list spaces from the backend\n  ugoite auth login\n  ugoite space list",
    styles = QUIET_ACCENT_STYLES
)]
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
    /// Space management commands.
    ///
    /// Run `ugoite config current` to check whether you are in core, backend, or api mode before choosing positional arguments.
    /// Use `/root/spaces/<slug>` for `SPACE_UID_OR_PATH` arguments in core mode.
    /// Use a bare immutable `SPACE_UID` in backend/api mode.
    /// For `ugoite space list`, pass `ROOT_PATH` in core mode and omit it in backend/api mode.
    Space(commands::space::SpaceCmd),
    /// Entry management commands
    Entry(commands::entry::EntryCmd),
    /// Form management commands.
    ///
    /// Run `ugoite config current` to check whether you should pass `/root/spaces/<slug>` in core mode or a bare `SPACE_UID` in backend/api mode.
    Form(commands::form::FormCmd),
    /// Asset management commands
    Asset(commands::asset::AssetCmd),
    /// Search commands.
    ///
    /// Run `ugoite config current` to check whether you should pass `/root/spaces/<slug>` in core mode or a bare `SPACE_UID` in backend/api mode.
    Search(commands::search::SearchCmd),
    /// Space Change history and revert commands.
    ///
    /// Run `ugoite config current` to check whether you should pass `/root/spaces/<slug>` in core mode or a bare `SPACE_UID` in backend/api mode.
    Change(commands::change::ChangeCmd),
    /// Knowledge snapshot (pin) commands.
    ///
    /// Run `ugoite config current` to check whether you should pass `/root/spaces/<slug>` in core mode or a bare `SPACE_UID` in backend/api mode.
    Pin(commands::pin::PinCmd),
    /// Run undo commands.
    ///
    /// Run `ugoite config current` to check whether you should pass `/root/spaces/<slug>` in core mode or a bare `SPACE_UID` in backend/api mode.
    Run(commands::run::RunCmd),
    /// SQL syntax linting and completion commands
    Sql(commands::sql::SqlCmd),
    /// Indexer operations
    Index(commands::index::IndexCmd),
    /// Start the Konase assistant
    Konase(commands::konase::KonaseCmd),
    /// Create a new space (deprecated: use `space create` instead)
    /// Create a new space
    #[command(
        hide = true,
        long_about = "Create a new space.\n\nThe positional value is a local Space slug in core mode or the new human-readable Space slug in backend/api mode. A server-generated Space UID is returned after creation.\n\nExamples:\n  # Core mode (workspace root)\n  ugoite create-space my-space --root /root\n\n  # Backend mode (requires: ugoite config set --mode backend ...)\n  ugoite create-space team-notes"
    )]
    CreateSpace {
        #[arg(
            long = "root",
            value_name = "LOCAL_ROOT",
            help = "Workspace root that contains the spaces/ directory in core mode."
        )]
        root_path: Option<String>,
        #[arg(
            value_name = "SPACE_SLUG",
            help = "New Space slug (alphanumeric + hyphens, e.g. 'my-project')"
        )]
        space_id: String,
        #[arg(
            long,
            value_name = "DISPLAY_NAME",
            help = "Display name for the new Space; defaults to the requested slug."
        )]
        name: Option<String>,
    },
    /// Query the index using SQL
    ///
    /// Examples:
    ///   # List all entries in a space (core mode)
    ///   ugoite query /root/spaces/my-space --sql "SELECT _ugoite_id, field_100 FROM \"form_<FormId>\" LIMIT 10"
    ///
    ///   # Filter by form type
    ///   ugoite query /root/spaces/my-space --sql "SELECT _ugoite_id FROM \"form_<FormId>\" WHERE field_100 = 'Daily note'"
    ///
    #[command(
        long_about = "Query a Space with DataFusion SQL.\n\nThe backend returns a stable Form relation (form_<FormId>) and stable field columns (field_<FieldId>) alongside _ugoite_* metadata columns. Only authorized Form relations are resolvable.\n\nExamples:\n  # Core mode (full local Space path)\n  ugoite query /root/spaces/my-space --sql \"SELECT _ugoite_id, field_100 FROM \\\"form_<FormId>\\\" LIMIT 10\"\n\n  # Backend/API mode (immutable Space UID)\n  ugoite query 019f1234-5678-7abc-8def-0123456789ab --sql \"SELECT _ugoite_id FROM \\\"form_<FormId>\\\" WHERE field_100 = 'Daily note'\""
    )]
    Query {
        #[arg(
            value_name = "SPACE_UID_OR_PATH",
            help = "Immutable Space UID in backend/api mode, or a local Space path in core mode."
        )]
        space_path: String,
        #[arg(
            long,
            help = "Read-only DataFusion SQL over authorized Iceberg Form relations. Use the backend-provided form_<FormId> relation and field_<FieldId> columns, plus _ugoite_* metadata columns.\n\nExample: \"SELECT _ugoite_id, field_100 FROM \\\"form_<FormId>\\\" LIMIT 10\""
        )]
        sql: String,
    },
}

fn main() {
    let cli = Cli::parse();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(async {
        // Derived refreshes are best-effort and process-local. A one-shot core
        // CLI command ends after the authoritative commit; `ugoite index run`
        // is the explicit repair path for derived freshness.
        run(cli).await
    });
    if let Err(error) = result {
        let projected = project_error(&error);
        if ugoite_cli::output::is_machine_stderr() {
            eprintln!("{}", projected.envelope());
        } else {
            eprintln!("{}", render_error(&projected, &stderr_style()));
        }
        std::process::exit(projected.exit_code());
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
        Commands::Change(cmd) => commands::change::run(cmd).await,
        Commands::Pin(cmd) => commands::pin::run(cmd).await,
        Commands::Run(cmd) => commands::run::run(cmd).await,
        Commands::Sql(cmd) => commands::sql::run(cmd).await,
        Commands::Index(cmd) => commands::index::run(cmd).await,
        Commands::Konase(cmd) => commands::konase::run(cmd).await,
        Commands::CreateSpace {
            root_path,
            space_id,
            name,
        } => {
            commands::space::create_space_cmd_with_name(
                root_path.as_deref(),
                &space_id,
                name.as_deref(),
                "create-space",
            )
            .await
        }
        Commands::Query { space_path, sql } => commands::index::query_cmd(&space_path, &sql).await,
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn help_styles_match_quiet_accent_roles() {
        let styles = Cli::command().get_styles().clone();

        assert_eq!(styles.get_header(), &anstyle::Style::new().bold());
        assert_eq!(styles.get_usage(), &anstyle::Style::new().bold());
        assert_eq!(styles.get_literal(), &anstyle::AnsiColor::Cyan.on_default());
        assert_eq!(styles.get_placeholder(), &anstyle::Style::new().dimmed());
        assert_eq!(
            styles.get_error(),
            &anstyle::AnsiColor::Red.on_default().bold()
        );
        assert_eq!(
            styles.get_invalid(),
            &anstyle::AnsiColor::Yellow.on_default()
        );
        assert_eq!(styles.get_context(), &anstyle::Style::new().dimmed());
        assert_eq!(styles.get_context_value(), &anstyle::Style::new());
    }
}
