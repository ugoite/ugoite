"""CLI entry point using Typer."""

import json
import uuid
from collections.abc import Callable
from functools import wraps
from pathlib import Path
from typing import Annotated, Any

import typer

from ieapp.attachments import (
    AttachmentReferencedError,
    delete_attachment,
    list_attachments,
    save_attachment,
)
from ieapp.classes import get_class, list_classes, list_column_types, migrate_class
from ieapp.indexer import Indexer, query_index
from ieapp.links import create_link, delete_link, list_links
from ieapp.logging_utils import setup_logging
from ieapp.notes import (
    create_note,
    delete_note,
    get_note,
    get_note_history,
    get_note_revision,
    list_notes,
    restore_note,
    update_note,
)
from ieapp.search import search_notes
from ieapp.workspace import (
    create_workspace,
    get_workspace,
    list_workspaces,
    patch_workspace,
    test_storage_connection,
)

app = typer.Typer(help="IEapp CLI - Knowledge base management")
note_app = typer.Typer(help="Note management commands")
index_app = typer.Typer(help="Indexer operations")
class_app = typer.Typer(help="Class management commands")
workspace_app = typer.Typer(help="Workspace management commands")
attachment_app = typer.Typer(help="Attachment management commands")
link_app = typer.Typer(help="Link management commands")
search_app = typer.Typer(help="Search commands")

app.add_typer(note_app, name="note")
app.add_typer(index_app, name="index")
app.add_typer(class_app, name="class")
app.add_typer(workspace_app, name="workspace")
app.add_typer(attachment_app, name="attachment")
app.add_typer(link_app, name="link")
app.add_typer(search_app, name="search")

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


def _parse_json_payload(value: str | None, label: str) -> dict[str, Any] | None:
    if value is None:
        return None
    try:
        payload = json.loads(value)
    except json.JSONDecodeError as exc:
        msg = f"Invalid JSON for {label}: {exc}"
        raise typer.BadParameter(msg) from exc
    if not isinstance(payload, dict):
        msg = f"{label} must be a JSON object"
        raise typer.BadParameter(msg)
    return payload


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


@workspace_app.command("list")
@handle_cli_errors
def cmd_workspace_list(
    root_path: Annotated[str, typer.Argument(help="Root path for workspaces")],
) -> None:
    """List all workspaces under the root path."""
    setup_logging()
    data = list_workspaces(root_path)
    typer.echo(json.dumps(data, indent=2))


@workspace_app.command("get")
@handle_cli_errors
def cmd_workspace_get(
    root_path: Annotated[str, typer.Argument(help="Root path for workspaces")],
    workspace_id: Annotated[str, typer.Argument(help="Workspace ID")],
) -> None:
    """Get workspace metadata."""
    setup_logging()
    data = get_workspace(root_path, workspace_id)
    typer.echo(json.dumps(data, indent=2))


@workspace_app.command("patch")
@handle_cli_errors
def cmd_workspace_patch(
    root_path: Annotated[str, typer.Argument(help="Root path for workspaces")],
    workspace_id: Annotated[str, typer.Argument(help="Workspace ID")],
    name: Annotated[str | None, typer.Option(help="New workspace name")] = None,
    storage_config: Annotated[
        str | None,
        typer.Option(help="JSON object for storage_config"),
    ] = None,
    settings: Annotated[
        str | None,
        typer.Option(help="JSON object for settings"),
    ] = None,
) -> None:
    """Patch workspace metadata/settings."""
    setup_logging()
    patch: dict[str, Any] = {}
    if name is not None:
        patch["name"] = name
    storage_payload = _parse_json_payload(storage_config, "storage_config")
    if storage_payload is not None:
        patch["storage_config"] = storage_payload
    settings_payload = _parse_json_payload(settings, "settings")
    if settings_payload is not None:
        patch["settings"] = settings_payload
    data = patch_workspace(root_path, workspace_id, patch=patch)
    typer.echo(json.dumps(data, indent=2))


@workspace_app.command("test-connection")
@handle_cli_errors
def cmd_workspace_test_connection(
    storage_config: Annotated[str, typer.Argument(help="Storage config JSON")],
) -> None:
    """Test a storage connector payload."""
    setup_logging()
    payload = _parse_json_payload(storage_config, "storage_config")
    if payload is None:
        msg = "storage_config is required"
        raise typer.BadParameter(msg)
    result = test_storage_connection(payload)
    typer.echo(json.dumps(result, indent=2))


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


@note_app.command("list")
@handle_cli_errors
def cmd_note_list(
    workspace_path: Annotated[str, typer.Argument(help="Full path to workspace")],
) -> None:
    """List notes in a workspace."""
    setup_logging()
    notes = list_notes(workspace_path)
    typer.echo(json.dumps(notes, indent=2))


@note_app.command("get")
@handle_cli_errors
def cmd_note_get(
    workspace_path: Annotated[str, typer.Argument(help="Full path to workspace")],
    note_id: Annotated[str, typer.Argument(help="Note ID")],
) -> None:
    """Get a single note by ID."""
    setup_logging()
    note = get_note(workspace_path, note_id)
    typer.echo(json.dumps(note, indent=2))


@note_app.command("update")
@handle_cli_errors
def cmd_note_update(
    workspace_path: Annotated[str, typer.Argument(help="Full path to workspace")],
    note_id: Annotated[str, typer.Argument(help="Note ID")],
    markdown: Annotated[str, typer.Option(help="Updated markdown content")],
    parent_revision_id: Annotated[str, typer.Option(help="Parent revision ID")],
    attachments: Annotated[
        str | None,
        typer.Option(help="JSON array of attachment metadata"),
    ] = None,
    author: Annotated[str, typer.Option(help="Author")] = "user",
) -> None:
    """Update a note with optimistic concurrency."""
    setup_logging()
    attachment_payload = None
    if attachments is not None:
        try:
            attachment_payload = json.loads(attachments)
        except json.JSONDecodeError as exc:
            msg = f"Invalid JSON for attachments: {exc}"
            raise typer.BadParameter(msg) from exc
        if not isinstance(attachment_payload, list):
            msg = "attachments must be a JSON array"
            raise typer.BadParameter(msg)
    update_note(
        workspace_path,
        note_id,
        markdown,
        parent_revision_id,
        attachments=attachment_payload,
        author=author,
    )
    typer.echo(f"Note '{note_id}' updated successfully.")


@note_app.command("delete")
@handle_cli_errors
def cmd_note_delete(
    workspace_path: Annotated[str, typer.Argument(help="Full path to workspace")],
    note_id: Annotated[str, typer.Argument(help="Note ID")],
    hard_delete: Annotated[bool | None, typer.Option(help="Permanently delete")] = None,
) -> None:
    """Delete (tombstone) a note."""
    setup_logging()
    delete_note(workspace_path, note_id, hard_delete=hard_delete is True)
    typer.echo(f"Note '{note_id}' deleted successfully.")


@note_app.command("history")
@handle_cli_errors
def cmd_note_history(
    workspace_path: Annotated[str, typer.Argument(help="Full path to workspace")],
    note_id: Annotated[str, typer.Argument(help="Note ID")],
) -> None:
    """Get note revision history."""
    setup_logging()
    history = get_note_history(workspace_path, note_id)
    typer.echo(json.dumps(history, indent=2))


@note_app.command("revision")
@handle_cli_errors
def cmd_note_revision(
    workspace_path: Annotated[str, typer.Argument(help="Full path to workspace")],
    note_id: Annotated[str, typer.Argument(help="Note ID")],
    revision_id: Annotated[str, typer.Argument(help="Revision ID")],
) -> None:
    """Get a specific note revision."""
    setup_logging()
    revision = get_note_revision(workspace_path, note_id, revision_id)
    typer.echo(json.dumps(revision, indent=2))


@note_app.command("restore")
@handle_cli_errors
def cmd_note_restore(
    workspace_path: Annotated[str, typer.Argument(help="Full path to workspace")],
    note_id: Annotated[str, typer.Argument(help="Note ID")],
    revision_id: Annotated[str, typer.Argument(help="Revision ID")],
    author: Annotated[str, typer.Option(help="Author")] = "user",
) -> None:
    """Restore a note to a previous revision."""
    setup_logging()
    data = restore_note(workspace_path, note_id, revision_id, author=author)
    typer.echo(json.dumps(data, indent=2))


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
    sql: Annotated[
        str | None,
        typer.Option("--sql", help="IEapp SQL query"),
    ] = None,
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
    if sql:
        filter_dict = {"$sql": sql}
    elif note_class or tag:
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


@class_app.command("list")
@handle_cli_errors
def cmd_class_list(
    workspace_path: Annotated[str, typer.Argument(help="Full path to workspace")],
) -> None:
    """List classes in a workspace."""
    setup_logging()
    classes = list_classes(workspace_path)
    typer.echo(json.dumps(classes, indent=2))


@class_app.command("get")
@handle_cli_errors
def cmd_class_get(
    workspace_path: Annotated[str, typer.Argument(help="Full path to workspace")],
    class_name: Annotated[str, typer.Argument(help="Class name")],
) -> None:
    """Get a class definition by name."""
    setup_logging()
    class_def = get_class(workspace_path, class_name)
    typer.echo(json.dumps(class_def, indent=2))


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


@attachment_app.command("upload")
@handle_cli_errors
def cmd_attachment_upload(
    workspace_path: Annotated[str, typer.Argument(help="Full path to workspace")],
    file_path: Annotated[str, typer.Argument(help="Path to the file to upload")],
    filename: Annotated[str | None, typer.Option(help="Override filename")] = None,
) -> None:
    """Upload an attachment to the workspace."""
    setup_logging()
    path = Path(file_path)
    try:
        data = path.read_bytes()
    except FileNotFoundError as exc:
        err_msg = f"Attachment file not found: '{path}'"
        raise typer.BadParameter(err_msg) from exc
    except IsADirectoryError as exc:
        err_msg = f"Attachment path is a directory, not a file: '{path}'"
        raise typer.BadParameter(err_msg) from exc
    except PermissionError as exc:
        err_msg = f"Permission denied when reading attachment file '{path}'"
        raise typer.BadParameter(err_msg) from exc
    except OSError as exc:
        err_msg = f"Could not read attachment file '{path}': {exc}"
        raise typer.BadParameter(err_msg) from exc
    name = filename or path.name
    attachment = save_attachment(workspace_path, data, name)
    typer.echo(json.dumps(attachment, indent=2))


@attachment_app.command("list")
@handle_cli_errors
def cmd_attachment_list(
    workspace_path: Annotated[str, typer.Argument(help="Full path to workspace")],
) -> None:
    """List attachments in a workspace."""
    setup_logging()
    attachments = list_attachments(workspace_path)
    typer.echo(json.dumps(attachments, indent=2))


@attachment_app.command("delete")
@handle_cli_errors
def cmd_attachment_delete(
    workspace_path: Annotated[str, typer.Argument(help="Full path to workspace")],
    attachment_id: Annotated[str, typer.Argument(help="Attachment ID")],
) -> None:
    """Delete an attachment by ID."""
    setup_logging()
    try:
        delete_attachment(workspace_path, attachment_id)
    except AttachmentReferencedError as exc:
        raise typer.Exit(code=1) from exc
    typer.echo(f"Attachment '{attachment_id}' deleted successfully.")


@link_app.command("create")
@handle_cli_errors
def cmd_link_create(
    workspace_path: Annotated[str, typer.Argument(help="Full path to workspace")],
    source: Annotated[str, typer.Argument(help="Source note ID")],
    target: Annotated[str, typer.Argument(help="Target note ID")],
    kind: Annotated[str, typer.Option(help="Link kind")] = "related",
) -> None:
    """Create a link between two notes."""
    setup_logging()
    link_id = uuid.uuid4().hex
    link = create_link(
        workspace_path,
        source=source,
        target=target,
        kind=kind,
        link_id=link_id,
    )
    typer.echo(json.dumps(link, indent=2))


@link_app.command("list")
@handle_cli_errors
def cmd_link_list(
    workspace_path: Annotated[str, typer.Argument(help="Full path to workspace")],
) -> None:
    """List links in a workspace."""
    setup_logging()
    links = list_links(workspace_path)
    typer.echo(json.dumps(links, indent=2))


@link_app.command("delete")
@handle_cli_errors
def cmd_link_delete(
    workspace_path: Annotated[str, typer.Argument(help="Full path to workspace")],
    link_id: Annotated[str, typer.Argument(help="Link ID")],
) -> None:
    """Delete a link by ID."""
    setup_logging()
    delete_link(workspace_path, link_id)
    typer.echo(f"Link '{link_id}' deleted successfully.")


@search_app.command("keyword")
@handle_cli_errors
def cmd_search_keyword(
    workspace_path: Annotated[str, typer.Argument(help="Full path to workspace")],
    query: Annotated[str, typer.Argument(help="Search query")],
) -> None:
    """Keyword search notes."""
    setup_logging()
    results = search_notes(workspace_path, query)
    typer.echo(json.dumps(results, indent=2))
