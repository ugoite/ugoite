#![recursion_limit = "256"]

use anyhow::Result;
use clap::{Parser, Subcommand};
use ugoite_cli::commands;
use ugoite_cli::output::{project_error, project_mutation_error, render_error, stderr_style};

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
    long_about = "Ugoite CLI - Knowledge base management\n\nQuick start:\n  mkdir knowledge\n  cd knowledge\n  ugoite config init\n  ugoite space create demo\n  ugoite entry list\n  ugoite entry list --text planning\n\nSwitch contexts or select one for a single invocation:\n  ugoite context use work\n  ugoite --context research entry list --text catalyst\n\nManage remote connections with `ugoite config connection` and pair a named credential with `ugoite auth login --connection NAME --credential NAME`.",
    styles = QUIET_ACCENT_STYLES
)]
struct Cli {
    /// Explicit canonical config file (single-file override, no merging).
    #[arg(long, value_name = "PATH", global = true)]
    config: Option<std::path::PathBuf>,
    /// Explicit context name for this invocation (does not change current_context).
    #[arg(long, value_name = "NAME", global = true)]
    context: Option<String>,
    #[command(subcommand)]
    command: Commands,
}

impl Cli {
    fn is_mutation_command(&self) -> bool {
        use ugoite_cli::commands::{
            asset::AssetSubCmd, change::ChangeSubCmd, entry::EntrySubCmd, form::FormSubCmd,
            pin::PinSubCmd, run::RunSubCmd, sql::SavedSqlSubCmd,
        };

        match &self.command {
            Commands::Entry(command) => matches!(
                &command.sub,
                EntrySubCmd::Create { .. }
                    | EntrySubCmd::Update { .. }
                    | EntrySubCmd::Delete { .. }
                    | EntrySubCmd::Restore { .. }
            ),
            Commands::Form(command) => matches!(&command.sub, FormSubCmd::Save { .. }),
            Commands::Asset(command) => matches!(
                &command.sub,
                AssetSubCmd::Upload { .. } | AssetSubCmd::Delete { .. }
            ),
            Commands::Change(command) => {
                matches!(&command.sub, ChangeSubCmd::Revert { dry_run: false, .. })
            }
            Commands::Run(command) => {
                matches!(&command.sub, RunSubCmd::Undo { dry_run: false, .. })
            }
            Commands::Pin(command) => matches!(
                &command.sub,
                PinSubCmd::Create { .. } | PinSubCmd::Delete { .. }
            ),
            Commands::Sql(command) => matches!(
                &command.sub,
                ugoite_cli::commands::sql::SqlSubCmd::Saved(
                    SavedSqlSubCmd::Create { .. }
                        | SavedSqlSubCmd::Update { .. }
                        | SavedSqlSubCmd::Delete { .. }
                )
            ),
            Commands::Composition(_) => false,
            Commands::Auth(_)
            | Commands::Config(_)
            | Commands::Context(_)
            | Commands::Space(_)
            | Commands::Index(_) => false,
            // Konase can fail during a read-only model/tool phase as well as
            // during a write. Its executor emits its own unconfirmed-write
            // diagnostic, so blanket classification here would be false.
            Commands::Konase(_) => false,
        }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Authentication helpers
    Auth(commands::auth::AuthCmd),
    /// Canonical CLI configuration and named connections
    Config(commands::config::ConfigCmd),
    /// Named CLI execution contexts (connection + Space UID + credential)
    Context(commands::context::ContextCmd),
    /// Space management commands.
    ///
    /// Space-bound commands use the selected context by default (see `ugoite config current`).
    Space(commands::space::SpaceCmd),
    /// Entry management commands
    Entry(commands::entry::EntryCmd),
    /// Form management commands.
    ///
    /// Space-bound commands use the selected context by default (see `ugoite config current`).
    Form(commands::form::FormCmd),
    /// Asset management commands
    Asset(commands::asset::AssetCmd),
    /// Space Change history and revert commands.
    ///
    /// Space-bound commands use the selected context by default (see `ugoite config current`).
    Change(commands::change::ChangeCmd),
    /// Knowledge snapshot (pin) commands.
    ///
    /// Space-bound commands use the selected context by default (see `ugoite config current`).
    Pin(commands::pin::PinCmd),
    /// Run undo commands.
    ///
    /// Space-bound commands use the selected context by default (see `ugoite config current`).
    Run(commands::run::RunCmd),
    /// SQL syntax linting and completion commands
    Sql(commands::sql::SqlCmd),
    /// Composition document commands
    Composition(commands::composition::CompositionCmd),
    /// Indexer operations
    Index(commands::index::IndexCmd),
    /// Start the Konase assistant
    Konase(commands::konase::KonaseCmd),
}

fn main() {
    let cli = Cli::parse();
    let mutation_command = cli.is_mutation_command();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(async {
        // Derived refreshes are best-effort and process-local. A one-shot core
        // CLI command ends after the authoritative commit; `ugoite index run`
        // is the explicit repair path for derived freshness.
        run(cli).await
    });
    if let Err(error) = result {
        let projected = if mutation_command {
            project_mutation_error(&error)
        } else {
            project_error(&error)
        };
        if ugoite_cli::output::is_machine_stderr() {
            eprintln!("{}", projected.envelope());
        } else {
            eprintln!("{}", render_error(&projected, &stderr_style()));
        }
        std::process::exit(projected.exit_code());
    }
}

async fn run(cli: Cli) -> Result<()> {
    let explicit_config = cli.config.as_deref();
    let explicit_context = cli.context.as_deref();
    if let Some(path) = explicit_config {
        std::env::set_var("UGOITE_CLI_ACTIVE_CONFIG", path);
    }
    if let Some(context) = explicit_context {
        std::env::set_var("UGOITE_CLI_ACTIVE_CONTEXT", context);
    }
    match cli.command {
        Commands::Auth(cmd) => commands::auth::run(cmd, explicit_config, explicit_context).await,
        Commands::Config(cmd) => {
            commands::config::run(cmd, explicit_config, explicit_context).await
        }
        Commands::Context(cmd) => {
            commands::context::run(cmd, explicit_config, explicit_context).await
        }
        Commands::Space(cmd) => commands::space::run(cmd, explicit_config, explicit_context).await,
        Commands::Entry(cmd) => commands::entry::run(cmd, explicit_config, explicit_context).await,
        Commands::Form(cmd) => commands::form::run(cmd, explicit_config, explicit_context).await,
        Commands::Asset(cmd) => commands::asset::run(cmd, explicit_config, explicit_context).await,
        Commands::Change(cmd) => {
            commands::change::run(cmd, explicit_config, explicit_context).await
        }
        Commands::Pin(cmd) => commands::pin::run(cmd, explicit_config, explicit_context).await,
        Commands::Run(cmd) => commands::run::run(cmd, explicit_config, explicit_context).await,
        Commands::Sql(cmd) => commands::sql::run(cmd, explicit_config, explicit_context).await,
        Commands::Composition(cmd) => {
            commands::composition::run(cmd, explicit_config, explicit_context).await
        }
        Commands::Index(cmd) => commands::index::run(cmd, explicit_config, explicit_context).await,
        Commands::Konase(cmd) => {
            commands::konase::run(cmd, explicit_config, explicit_context).await
        }
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

    #[test]
    fn recovery_dry_runs_are_not_classified_as_mutations() {
        let change_preview =
            Cli::try_parse_from(["ugoite", "change", "revert", "change-id", "--dry-run"]).unwrap();
        assert!(!change_preview.is_mutation_command());

        let run_preview =
            Cli::try_parse_from(["ugoite", "run", "undo", "run-id", "--dry-run"]).unwrap();
        assert!(!run_preview.is_mutation_command());

        let change_write =
            Cli::try_parse_from(["ugoite", "change", "revert", "change-id"]).unwrap();
        assert!(change_write.is_mutation_command());

        let run_write = Cli::try_parse_from(["ugoite", "run", "undo", "run-id"]).unwrap();
        assert!(run_write.is_mutation_command());
    }

    #[test]
    fn composition_lint_accepts_a_file_without_a_space_context() {
        let cli = Cli::try_parse_from([
            "ugoite",
            "composition",
            "lint",
            "monthly-expense.ugcomp.yaml",
        ])
        .expect("Composition lint command");

        assert!(!cli.is_mutation_command());
        assert!(matches!(
            cli.command,
            Commands::Composition(commands::composition::CompositionCmd {
                sub: commands::composition::CompositionSubCmd::Lint { .. }
            })
        ));
    }

    #[test]
    fn composition_list_accepts_bounded_page_options_without_becoming_a_write() {
        let defaults = Cli::try_parse_from(["ugoite", "composition", "list"])
            .expect("Composition list command");
        assert!(!defaults.is_mutation_command());
        assert!(matches!(
            defaults.command,
            Commands::Composition(commands::composition::CompositionCmd {
                sub: commands::composition::CompositionSubCmd::List {
                    limit: None,
                    offset: None,
                }
            })
        ));

        let paged = Cli::try_parse_from([
            "ugoite",
            "composition",
            "list",
            "--limit",
            "25",
            "--offset",
            "50",
        ])
        .expect("Composition list paging options");
        assert!(matches!(
            paged.command,
            Commands::Composition(commands::composition::CompositionCmd {
                sub: commands::composition::CompositionSubCmd::List {
                    limit: Some(25),
                    offset: Some(50),
                }
            })
        ));
    }

    #[test]
    fn composition_inspect_accepts_exact_revision_and_raw_output_without_becoming_a_write() {
        let cli = Cli::try_parse_from([
            "ugoite",
            "--context",
            "research",
            "composition",
            "inspect",
            "composition-1",
            "--revision",
            "revision-2",
            "--raw",
        ])
        .expect("Composition inspect command");

        assert!(!cli.is_mutation_command());
        assert!(matches!(
            cli.command,
            Commands::Composition(commands::composition::CompositionCmd {
                sub: commands::composition::CompositionSubCmd::Inspect {
                    composition_id,
                    revision: Some(revision),
                    raw: true,
                }
            }) if composition_id == "composition-1" && revision == "revision-2"
        ));
    }
}
