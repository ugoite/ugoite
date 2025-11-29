"""CLI entry point using Typer."""

from typing import Annotated

import typer

from ieapp.indexer import Indexer, query_index
from ieapp.logging_utils import setup_logging
from ieapp.notes import NoteExistsError, create_note
from ieapp.workspace import WorkspaceExistsError, create_workspace

app = typer.Typer(help="IEapp CLI - Knowledge base management")
note_app = typer.Typer(help="Note management commands")
index_app = typer.Typer(help="Indexer operations")

app.add_typer(note_app, name="note")
app.add_typer(index_app, name="index")

DEFAULT_NOTE_CONTENT = "# New Note\n"


@app.command("create-workspace")
def cmd_create_workspace(
    root_path: Annotated[str, typer.Argument(help="Root path for workspaces")],
    workspace_id: Annotated[str, typer.Argument(help="ID of the workspace to create")],
) -> None:
    """Create a new workspace."""
    setup_logging()
    try:
        create_workspace(root_path, workspace_id)
        typer.echo(
            f"Workspace '{workspace_id}' created successfully at '{root_path}'",
        )
    except WorkspaceExistsError as e:
        typer.echo(f"Error: {e}", err=True)
        raise typer.Exit(code=1) from e
    except Exception as e:
        typer.echo(f"Error: {e}", err=True)
        raise typer.Exit(code=1) from e


@note_app.command("create")
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
    try:
        create_note(workspace_path, note_id, content, author=author)
        typer.echo(f"Note '{note_id}' created successfully.")
    except NoteExistsError as e:
        typer.echo(f"Error: {e}", err=True)
        raise typer.Exit(code=1) from e
    except Exception as e:
        typer.echo(f"Error: {e}", err=True)
        raise typer.Exit(code=1) from e


@index_app.command("run")
def cmd_index_run(
    workspace_path: Annotated[
        str,
        typer.Argument(help="Full path to the workspace directory"),
    ],
) -> None:
    """Run the indexer to rebuild caches."""
    setup_logging()
    try:
        indexer = Indexer(workspace_path)
        indexer.run_once()
        typer.echo(f"Indexer completed for workspace '{workspace_path}'.")
    except Exception as e:
        typer.echo(f"Error: {e}", err=True)
        raise typer.Exit(code=1) from e


@app.command("query")
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
    try:
        filter_dict: dict[str, str] = {}
        if note_class:
            filter_dict["class"] = note_class
        if tag:
            filter_dict["tag"] = tag

        results = query_index(workspace_path, filter_dict or None)

        if not results:
            typer.echo("No notes found.")
        else:
            for note in results:
                typer.echo(f"- {note.get('id')}: {note.get('title')}")
    except Exception as e:
        typer.echo(f"Error: {e}", err=True)
        raise typer.Exit(code=1) from e


def main() -> None:
    """Entry point for the IEapp CLI."""
    app()


if __name__ == "__main__":
    main()
