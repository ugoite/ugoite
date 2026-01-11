"""CLI entry point using Typer."""

import json
from collections.abc import Callable
from functools import wraps
from pathlib import Path
from typing import Annotated, Any

import typer

from ieapp.classes import list_column_types, migrate_class
from ieapp.indexer import Indexer, query_index
from ieapp.logging_utils import setup_logging
from ieapp.notes import create_note
from ieapp.workspace import create_workspace

app = typer.Typer(help="IEapp CLI - Knowledge base management")
note_app = typer.Typer(help="Note management commands")
index_app = typer.Typer(help="Indexer operations")
class_app = typer.Typer(help="Class management commands")

app.add_typer(note_app, name="note")
app.add_typer(index_app, name="index")
app.add_typer(class_app, name="class")

DEFAULT_NOTE_CONTENT = "# New Note\n"


def handle_cli_errors[R](func: Callable[..., R]) -> Callable[..., R]:
    """Handle common CLI errors.

    Wraps CLI commands to catch known exceptions and print user-friendly error messages.

    Args:
        func: The CLI command function to wrap.

    Returns:
        The wrapped function with error handling.

    """

    @wraps(func)
    def wrapper(*args: Any, **kwargs: Any) -> R:
        try:
            return func(*args, **kwargs)
        except Exception as e:
            typer.echo(f"Error: {e}", err=True)
            raise typer.Exit(code=1) from e

    return wrapper


@app.command("create-workspace")
@handle_cli_errors
def cmd_create_workspace(
    root_path: Annotated[str, typer.Argument(help="Root path for workspaces")],
    workspace_id: Annotated[str, typer.Argument(help="ID of the workspace to create")],
) -> None:
    """Create a new workspace."""
    setup_logging()
    create_workspace(root_path, workspace_id)
    typer.echo(
        f"Workspace '{workspace_id}' created successfully at '{root_path}'",
    )


@note_app.command("create")
@handle_cli_errors
def cmd_note_create(
    workspace_path: Annotated[
        str,
        typer.Argument(help="Full path to the workspace directory"),
    ],
    note_id: Annotated[str, typer.Argument(help="ID of the note to create")],
    content: Annotated[
        str,
        typer.Option(help="Content of the note"),
    ] = DEFAULT_NOTE_CONTENT,
    author: Annotated[str, typer.Option(help="Author of the note")] = "user",
) -> None:
    """Create a new note in a workspace."""
    setup_logging()
    create_note(workspace_path, note_id, content, author=author)
    typer.echo(f"Note '{note_id}' created successfully.")


@index_app.command("run")
@handle_cli_errors
def cmd_index_run(
    workspace_path: Annotated[
        str,
        typer.Argument(help="Full path to the workspace directory"),
    ],
) -> None:
    """Run the indexer to rebuild caches."""
    setup_logging()
    indexer = Indexer(workspace_path)
    indexer.run_once()
    typer.echo(f"Indexer completed for workspace '{workspace_path}'.")


@app.command("query")
@handle_cli_errors
def cmd_query(
    workspace_path: Annotated[
        str,
        typer.Argument(help="Full path to the workspace directory"),
    ],
    note_class: Annotated[
        str | None,
        typer.Option("--class", help="Filter by class"),
    ] = None,
    tag: Annotated[
        str | None,
        typer.Option(help="Filter by tag"),
    ] = None,
) -> None:
    """Query the index for notes."""
    setup_logging()
    filter_dict: dict[str, Any] | None = None
    if note_class or tag:
        filter_dict = {}
        if note_class:
            filter_dict["class"] = note_class
        if tag:
            filter_dict["tag"] = tag

    results = query_index(workspace_path, filter_dict)

    if not results:
        typer.echo("No notes found.")
    else:
        for note in results:
            typer.echo(f"- {note.get('id')}: {note.get('title')}")


def main() -> None:
    """Entry point for the IEapp CLI."""
    app()


if __name__ == "__main__":
    main()


@class_app.command("list-types")
@handle_cli_errors
def cmd_list_types() -> None:
    """List available column types."""
    types = list_column_types()
    for t in types:
        typer.echo(t)


@class_app.command("update")
@handle_cli_errors
def cmd_class_update(
    workspace_path: Annotated[
        str,
        typer.Argument(help="Full path to the workspace directory"),
    ],
    class_file: Annotated[str, typer.Argument(help="Path to class JSON file")],
    strategies: Annotated[
        str | None,
        typer.Option(help="JSON string of migration strategies"),
    ] = None,
) -> None:
    """Update class and migrate existing notes using strategies."""
    setup_logging()

    class_path = Path(class_file)
    try:
        with class_path.open() as f:
            class_data = json.load(f)
    except FileNotFoundError as e:
        err_msg = f"Class file not found: '{class_path}'"
        raise typer.BadParameter(err_msg) from e
    except OSError as e:
        err_msg = f"Could not read class file '{class_path}': {e}"
        raise typer.BadParameter(err_msg) from e
    except json.JSONDecodeError as e:
        err_msg = f"Invalid JSON in class file '{class_path}': {e}"
        raise typer.BadParameter(err_msg) from e

    strat_dict = None
    if strategies:
        try:
            strat_dict = json.loads(strategies)
        except json.JSONDecodeError as e:
            err_msg = f"Invalid JSON in strategies: {e}"
            raise typer.BadParameter(err_msg) from e

    count = migrate_class(workspace_path, class_data, strategies=strat_dict)
    note_word = "note" if count == 1 else "notes"
    typer.echo(f"Class updated. Migrated {count} {note_word}.")
