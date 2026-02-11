"""CLI entry point using Typer."""

import json
import uuid
from collections.abc import Callable
from functools import wraps
from pathlib import Path
from typing import Annotated, Any

import typer
from ieapp_core import build_sql_schema, lint_sql, sql_completions

from ieapp import saved_sql
from ieapp.assets import (
    AssetReferencedError,
    delete_asset,
    list_assets,
    save_asset,
)
from ieapp.entries import (
    create_entry,
    delete_entry,
    get_entry,
    get_entry_history,
    get_entry_revision,
    list_entries,
    restore_entry,
    update_entry,
)
from ieapp.forms import get_form, list_column_types, list_forms, migrate_form
from ieapp.indexer import Indexer, create_sql_session, get_sql_session_rows, query_index
from ieapp.links import create_link, delete_link, list_links
from ieapp.logging_utils import setup_logging
from ieapp.search import search_entries
from ieapp.space import (
    SampleSpaceOptions,
    create_sample_space,
    create_space,
    get_space,
    list_spaces,
    patch_space,
    test_storage_connection,
)

app = typer.Typer(help="IEapp CLI - Knowledge base management")
entry_app = typer.Typer(help="Entry management commands")
index_app = typer.Typer(help="Indexer operations")
form_app = typer.Typer(help="Form management commands")
space_app = typer.Typer(help="Space management commands")
asset_app = typer.Typer(help="Asset management commands")
link_app = typer.Typer(help="Link management commands")
search_app = typer.Typer(help="Search commands")
sql_app = typer.Typer(help="SQL linting and completion commands")

app.add_typer(entry_app, name="entry")
app.add_typer(index_app, name="index")
app.add_typer(form_app, name="form")
app.add_typer(space_app, name="space")
app.add_typer(asset_app, name="asset")
app.add_typer(link_app, name="link")
app.add_typer(search_app, name="search")
app.add_typer(sql_app, name="sql")

DEFAULT_NOTE_CONTENT = "# New Entry\n"


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


def _parse_json_list(value: str | None, label: str) -> list[dict[str, Any]] | None:
    if value is None:
        return None
    try:
        payload = json.loads(value)
    except json.JSONDecodeError as exc:
        msg = f"Invalid JSON for {label}: {exc}"
        raise typer.BadParameter(msg) from exc
    if not isinstance(payload, list):
        msg = f"{label} must be a JSON array"
        raise typer.BadParameter(msg)
    for item in payload:
        if not isinstance(item, dict):
            msg = f"{label} must contain JSON objects"
            raise typer.BadParameter(msg)
    return payload


@app.command("create-space")
@handle_cli_errors
def cmd_create_space(
    root_path: Annotated[str, typer.Argument(help="Root path for spaces")],
    space_id: Annotated[str, typer.Argument(help="ID of the space to create")],
) -> None:
    """Create a new space."""
    setup_logging()
    create_space(root_path, space_id)
    typer.echo(
        f"Space '{space_id}' created successfully at '{root_path}'",
    )


@space_app.command("list")
@handle_cli_errors
def cmd_space_list(
    root_path: Annotated[str, typer.Argument(help="Root path for spaces")],
) -> None:
    """List all spaces under the root path."""
    setup_logging()
    data = list_spaces(root_path)
    typer.echo(json.dumps(data, indent=2))


@space_app.command("get")
@handle_cli_errors
def cmd_space_get(
    root_path: Annotated[str, typer.Argument(help="Root path for spaces")],
    space_id: Annotated[str, typer.Argument(help="Space ID")],
) -> None:
    """Get space metadata."""
    setup_logging()
    data = get_space(root_path, space_id)
    typer.echo(json.dumps(data, indent=2))


@space_app.command("sample-data")
@handle_cli_errors
def cmd_space_sample_data(
    root_path: Annotated[str, typer.Argument(help="Root path for spaces")],
    space_id: Annotated[str, typer.Argument(help="Space ID")],
    scenario: Annotated[
        str,
        typer.Option(help="Sample data scenario"),
    ] = "renewable-ops",
    entry_count: Annotated[
        int,
        typer.Option(help="Approximate total number of entries"),
    ] = 5000,
    seed: Annotated[
        int | None,
        typer.Option(help="Seed for deterministic generation"),
    ] = None,
) -> None:
    """Create a sample-data space with generated content."""
    setup_logging()
    options = SampleSpaceOptions(
        scenario=scenario,
        entry_count=entry_count,
        seed=seed,
    )
    summary = create_sample_space(root_path, space_id, options=options)
    typer.echo(json.dumps(summary, indent=2))


@space_app.command("patch")
@handle_cli_errors
def cmd_space_patch(
    root_path: Annotated[str, typer.Argument(help="Root path for spaces")],
    space_id: Annotated[str, typer.Argument(help="Space ID")],
    name: Annotated[str | None, typer.Option(help="New space name")] = None,
    storage_config: Annotated[
        str | None,
        typer.Option(help="JSON object for storage_config"),
    ] = None,
    settings: Annotated[
        str | None,
        typer.Option(help="JSON object for settings"),
    ] = None,
) -> None:
    """Patch space metadata/settings."""
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
    data = patch_space(root_path, space_id, patch=patch)
    typer.echo(json.dumps(data, indent=2))


@space_app.command("test-connection")
@handle_cli_errors
def cmd_space_test_connection(
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


@entry_app.command("create")
@handle_cli_errors
def cmd_entry_create(
    space_path: Annotated[
        str,
        typer.Argument(help="Full path to the space directory"),
    ],
    entry_id: Annotated[str, typer.Argument(help="ID of the entry to create")],
    content: Annotated[
        str,
        typer.Option(help="Content of the entry"),
    ] = DEFAULT_NOTE_CONTENT,
    author: Annotated[str, typer.Option(help="Author of the entry")] = "user",
) -> None:
    """Create a new entry in a space."""
    setup_logging()
    create_entry(space_path, entry_id, content, author=author)
    typer.echo(f"Entry '{entry_id}' created successfully.")


@entry_app.command("list")
@handle_cli_errors
def cmd_entry_list(
    space_path: Annotated[str, typer.Argument(help="Full path to space")],
) -> None:
    """List entries in a space."""
    setup_logging()
    entries = list_entries(space_path)
    typer.echo(json.dumps(entries, indent=2))


@entry_app.command("get")
@handle_cli_errors
def cmd_entry_get(
    space_path: Annotated[str, typer.Argument(help="Full path to space")],
    entry_id: Annotated[str, typer.Argument(help="Entry ID")],
) -> None:
    """Get a single entry by ID."""
    setup_logging()
    entry = get_entry(space_path, entry_id)
    typer.echo(json.dumps(entry, indent=2))


@entry_app.command("update")
@handle_cli_errors
def cmd_entry_update(
    space_path: Annotated[str, typer.Argument(help="Full path to space")],
    entry_id: Annotated[str, typer.Argument(help="Entry ID")],
    markdown: Annotated[str, typer.Option(help="Updated markdown content")],
    parent_revision_id: Annotated[str, typer.Option(help="Parent revision ID")],
    assets: Annotated[
        str | None,
        typer.Option(help="JSON array of asset metadata"),
    ] = None,
    author: Annotated[str, typer.Option(help="Author")] = "user",
) -> None:
    """Update a entry with optimistic concurrency."""
    setup_logging()
    asset_payload = None
    if assets is not None:
        try:
            asset_payload = json.loads(assets)
        except json.JSONDecodeError as exc:
            msg = f"Invalid JSON for assets: {exc}"
            raise typer.BadParameter(msg) from exc
        if not isinstance(asset_payload, list):
            msg = "assets must be a JSON array"
            raise typer.BadParameter(msg)
    update_entry(
        space_path,
        entry_id,
        markdown,
        parent_revision_id,
        assets=asset_payload,
        author=author,
    )
    typer.echo(f"Entry '{entry_id}' updated successfully.")


@entry_app.command("delete")
@handle_cli_errors
def cmd_entry_delete(
    space_path: Annotated[str, typer.Argument(help="Full path to space")],
    entry_id: Annotated[str, typer.Argument(help="Entry ID")],
    hard_delete: Annotated[bool | None, typer.Option(help="Permanently delete")] = None,
) -> None:
    """Delete (tombstone) a entry."""
    setup_logging()
    delete_entry(space_path, entry_id, hard_delete=hard_delete is True)
    typer.echo(f"Entry '{entry_id}' deleted successfully.")


@entry_app.command("history")
@handle_cli_errors
def cmd_entry_history(
    space_path: Annotated[str, typer.Argument(help="Full path to space")],
    entry_id: Annotated[str, typer.Argument(help="Entry ID")],
) -> None:
    """Get entry revision history."""
    setup_logging()
    history = get_entry_history(space_path, entry_id)
    typer.echo(json.dumps(history, indent=2))


@entry_app.command("revision")
@handle_cli_errors
def cmd_entry_revision(
    space_path: Annotated[str, typer.Argument(help="Full path to space")],
    entry_id: Annotated[str, typer.Argument(help="Entry ID")],
    revision_id: Annotated[str, typer.Argument(help="Revision ID")],
) -> None:
    """Get a specific entry revision."""
    setup_logging()
    revision = get_entry_revision(space_path, entry_id, revision_id)
    typer.echo(json.dumps(revision, indent=2))


@entry_app.command("restore")
@handle_cli_errors
def cmd_entry_restore(
    space_path: Annotated[str, typer.Argument(help="Full path to space")],
    entry_id: Annotated[str, typer.Argument(help="Entry ID")],
    revision_id: Annotated[str, typer.Argument(help="Revision ID")],
    author: Annotated[str, typer.Option(help="Author")] = "user",
) -> None:
    """Restore a entry to a previous revision."""
    setup_logging()
    data = restore_entry(space_path, entry_id, revision_id, author=author)
    typer.echo(json.dumps(data, indent=2))


@index_app.command("run")
@handle_cli_errors
def cmd_index_run(
    space_path: Annotated[
        str,
        typer.Argument(help="Full path to the space directory"),
    ],
) -> None:
    """Run the indexer to rebuild caches."""
    setup_logging()
    indexer = Indexer(space_path)
    indexer.run_once()
    typer.echo(f"Indexer completed for space '{space_path}'.")


@app.command("query")
@handle_cli_errors
def cmd_query(
    space_path: Annotated[
        str,
        typer.Argument(help="Full path to the space directory"),
    ],
    sql: Annotated[
        str | None,
        typer.Option("--sql", help="IEapp SQL query"),
    ] = None,
    limit: Annotated[
        int,
        typer.Option("--limit", help="Maximum rows to return"),
    ] = 50,
    offset: Annotated[
        int,
        typer.Option("--offset", help="Row offset for pagination"),
    ] = 0,
    entry_form: Annotated[
        str | None,
        typer.Option("--form", help="Filter by form"),
    ] = None,
    tag: Annotated[
        str | None,
        typer.Option(help="Filter by tag"),
    ] = None,
) -> None:
    """Query the index for entries."""
    setup_logging()
    filter_dict: dict[str, Any] | None = None
    if sql:
        session = create_sql_session(space_path, sql)
        if session.get("status") == "failed":
            error = session.get("error") or "SQL query failed"
            typer.echo(error)
            raise typer.Exit(code=1)

        rows_payload = get_sql_session_rows(
            space_path,
            session.get("id", ""),
            offset=offset,
            limit=limit,
        )
        results = rows_payload.get("rows", [])
        total = rows_payload.get("total_count", len(results))
        if not results:
            typer.echo("No entries found.")
        else:
            typer.echo(f"Total results: {total}")
            for entry in results:
                typer.echo(f"- {entry.get('id')}: {entry.get('title')}")
        return
    if entry_form or tag:
        filter_dict = {}
        if entry_form:
            filter_dict["form"] = entry_form
        if tag:
            filter_dict["tag"] = tag

    results = query_index(space_path, filter_dict)

    if not results:
        typer.echo("No entries found.")
    else:
        for entry in results:
            typer.echo(f"- {entry.get('id')}: {entry.get('title')}")


@sql_app.command("lint")
@handle_cli_errors
def cmd_sql_lint(
    sql: Annotated[str, typer.Argument(help="IEapp SQL query")],
    json_output: Annotated[
        bool | None,
        typer.Option("--json", help="Output diagnostics as JSON"),
    ] = None,
) -> None:
    """Lint an IEapp SQL query using shared rules."""
    diagnostics = lint_sql(sql)
    if json_output:
        payload = [diag.__dict__ for diag in diagnostics]
        typer.echo(json.dumps(payload, indent=2))
        return

    if not diagnostics:
        typer.echo("No lint issues.")
        return

    has_error = False
    for diag in diagnostics:
        if diag.severity.lower() == "error":
            has_error = True
        typer.echo(f"{diag.severity.upper()}: {diag.message}")

    if has_error:
        raise typer.Exit(code=1)


@sql_app.command("schema")
@handle_cli_errors
def cmd_sql_schema(
    space_path: Annotated[
        str | None,
        typer.Option("--space", help="Full path to space for form fields"),
    ] = None,
) -> None:
    """Output the SQL completion schema as JSON."""
    forms = list_forms(space_path) if space_path else []
    schema = build_sql_schema(forms)
    typer.echo(json.dumps(schema, indent=2))


@sql_app.command("complete")
@handle_cli_errors
def cmd_sql_complete(
    sql: Annotated[str, typer.Argument(help="IEapp SQL query prefix")],
    space_path: Annotated[
        str | None,
        typer.Option("--space", help="Full path to space for form fields"),
    ] = None,
) -> None:
    """Provide SQL completion suggestions."""
    forms = list_forms(space_path) if space_path else []
    suggestions = sql_completions(sql, forms)
    typer.echo(json.dumps(suggestions, indent=2))


@sql_app.command("saved-list")
@handle_cli_errors
def cmd_sql_saved_list(
    space_path: Annotated[
        str,
        typer.Argument(help="Full path to the space directory"),
    ],
) -> None:
    """List saved SQL entries in a space."""
    setup_logging()
    entries = saved_sql.list_sql(space_path)
    typer.echo(json.dumps(entries, indent=2))


@sql_app.command("saved-get")
@handle_cli_errors
def cmd_sql_saved_get(
    space_path: Annotated[
        str,
        typer.Argument(help="Full path to the space directory"),
    ],
    sql_id: Annotated[str, typer.Argument(help="Saved SQL ID")],
) -> None:
    """Get a saved SQL entry by ID."""
    setup_logging()
    entry = saved_sql.get_sql(space_path, sql_id)
    typer.echo(json.dumps(entry, indent=2))


@sql_app.command("saved-create")
@handle_cli_errors
def cmd_sql_saved_create(
    space_path: Annotated[
        str,
        typer.Argument(help="Full path to the space directory"),
    ],
    name: Annotated[str, typer.Argument(help="Saved SQL name")],
    sql: Annotated[str, typer.Argument(help="SQL text")],
    variables: Annotated[
        str,
        typer.Option(help="JSON array of SQL variables"),
    ] = "[]",
    sql_id: Annotated[
        str | None,
        typer.Option(help="Optional saved SQL ID"),
    ] = None,
    author: Annotated[
        str,
        typer.Option(help="Author name"),
    ] = "user",
) -> None:
    """Create a saved SQL entry."""
    setup_logging()
    variable_list = _parse_json_list(variables, "variables") or []
    payload = {
        "name": name,
        "sql": sql,
        "variables": variable_list,
    }
    entry = saved_sql.create_sql(
        space_path,
        payload,
        sql_id=sql_id,
        author=author,
    )
    typer.echo(json.dumps(entry, indent=2))


@sql_app.command("saved-update")
@handle_cli_errors
def cmd_sql_saved_update(
    space_path: Annotated[
        str,
        typer.Argument(help="Full path to the space directory"),
    ],
    sql_id: Annotated[str, typer.Argument(help="Saved SQL ID")],
    name: Annotated[str, typer.Argument(help="Saved SQL name")],
    sql: Annotated[str, typer.Argument(help="SQL text")],
    variables: Annotated[
        str,
        typer.Option(help="JSON array of SQL variables"),
    ] = "[]",
    parent_revision_id: Annotated[
        str | None,
        typer.Option(help="Parent revision ID"),
    ] = None,
    author: Annotated[
        str,
        typer.Option(help="Author name"),
    ] = "user",
) -> None:
    """Update a saved SQL entry."""
    setup_logging()
    variable_list = _parse_json_list(variables, "variables") or []
    payload = {
        "name": name,
        "sql": sql,
        "variables": variable_list,
        "parent_revision_id": parent_revision_id,
    }
    entry = saved_sql.update_sql(space_path, sql_id, payload, author=author)
    typer.echo(json.dumps(entry, indent=2))


@sql_app.command("saved-delete")
@handle_cli_errors
def cmd_sql_saved_delete(
    space_path: Annotated[
        str,
        typer.Argument(help="Full path to the space directory"),
    ],
    sql_id: Annotated[str, typer.Argument(help="Saved SQL ID")],
) -> None:
    """Delete a saved SQL entry."""
    setup_logging()
    saved_sql.delete_sql(space_path, sql_id)
    typer.echo(f"Saved SQL '{sql_id}' deleted successfully.")


def main() -> None:
    """Entry point for the IEapp CLI."""
    app()


if __name__ == "__main__":
    main()


@form_app.command("list-types")
@handle_cli_errors
def cmd_list_types() -> None:
    """List available column types."""
    types = list_column_types()
    for t in types:
        typer.echo(t)


@form_app.command("list")
@handle_cli_errors
def cmd_form_list(
    space_path: Annotated[str, typer.Argument(help="Full path to space")],
) -> None:
    """List forms in a space."""
    setup_logging()
    forms = list_forms(space_path)
    typer.echo(json.dumps(forms, indent=2))


@form_app.command("get")
@handle_cli_errors
def cmd_form_get(
    space_path: Annotated[str, typer.Argument(help="Full path to space")],
    form_name: Annotated[str, typer.Argument(help="Form name")],
) -> None:
    """Get a form definition by name."""
    setup_logging()
    form_def = get_form(space_path, form_name)
    typer.echo(json.dumps(form_def, indent=2))


@form_app.command("update")
@handle_cli_errors
def cmd_form_update(
    space_path: Annotated[
        str,
        typer.Argument(help="Full path to the space directory"),
    ],
    form_file: Annotated[str, typer.Argument(help="Path to form JSON file")],
    strategies: Annotated[
        str | None,
        typer.Option(help="JSON string of migration strategies"),
    ] = None,
) -> None:
    """Update form and migrate existing entries using strategies."""
    setup_logging()

    form_path = Path(form_file)
    try:
        with form_path.open() as f:
            form_data = json.load(f)
    except FileNotFoundError as e:
        err_msg = f"Form file not found: '{form_path}'"
        raise typer.BadParameter(err_msg) from e
    except OSError as e:
        err_msg = f"Could not read form file '{form_path}': {e}"
        raise typer.BadParameter(err_msg) from e
    except json.JSONDecodeError as e:
        err_msg = f"Invalid JSON in form file '{form_path}': {e}"
        raise typer.BadParameter(err_msg) from e

    strat_dict = None
    if strategies:
        try:
            strat_dict = json.loads(strategies)
        except json.JSONDecodeError as e:
            err_msg = f"Invalid JSON in strategies: {e}"
            raise typer.BadParameter(err_msg) from e

    count = migrate_form(space_path, form_data, strategies=strat_dict)
    entry_word = "entry" if count == 1 else "entries"
    typer.echo(f"Form updated. Migrated {count} {entry_word}.")


@asset_app.command("upload")
@handle_cli_errors
def cmd_asset_upload(
    space_path: Annotated[str, typer.Argument(help="Full path to space")],
    file_path: Annotated[str, typer.Argument(help="Path to the file to upload")],
    filename: Annotated[str | None, typer.Option(help="Override filename")] = None,
) -> None:
    """Upload an asset to the space."""
    setup_logging()
    path = Path(file_path)
    try:
        data = path.read_bytes()
    except FileNotFoundError as exc:
        err_msg = f"Asset file not found: '{path}'"
        raise typer.BadParameter(err_msg) from exc
    except IsADirectoryError as exc:
        err_msg = f"Asset path is a directory, not a file: '{path}'"
        raise typer.BadParameter(err_msg) from exc
    except PermissionError as exc:
        err_msg = f"Permission denied when reading asset file '{path}'"
        raise typer.BadParameter(err_msg) from exc
    except OSError as exc:
        err_msg = f"Could not read asset file '{path}': {exc}"
        raise typer.BadParameter(err_msg) from exc
    name = filename or path.name
    asset = save_asset(space_path, data, name)
    typer.echo(json.dumps(asset, indent=2))


@asset_app.command("list")
@handle_cli_errors
def cmd_asset_list(
    space_path: Annotated[str, typer.Argument(help="Full path to space")],
) -> None:
    """List assets in a space."""
    setup_logging()
    assets = list_assets(space_path)
    typer.echo(json.dumps(assets, indent=2))


@asset_app.command("delete")
@handle_cli_errors
def cmd_asset_delete(
    space_path: Annotated[str, typer.Argument(help="Full path to space")],
    asset_id: Annotated[str, typer.Argument(help="Asset ID")],
) -> None:
    """Delete an asset by ID."""
    setup_logging()
    try:
        delete_asset(space_path, asset_id)
    except AssetReferencedError as exc:
        raise typer.Exit(code=1) from exc
    typer.echo(f"Asset '{asset_id}' deleted successfully.")


@link_app.command("create")
@handle_cli_errors
def cmd_link_create(
    space_path: Annotated[str, typer.Argument(help="Full path to space")],
    source: Annotated[str, typer.Argument(help="Source entry ID")],
    target: Annotated[str, typer.Argument(help="Target entry ID")],
    kind: Annotated[str, typer.Option(help="Link kind")] = "related",
) -> None:
    """Create a link between two entries."""
    setup_logging()
    link_id = uuid.uuid4().hex
    link = create_link(
        space_path,
        source=source,
        target=target,
        kind=kind,
        link_id=link_id,
    )
    typer.echo(json.dumps(link, indent=2))


@link_app.command("list")
@handle_cli_errors
def cmd_link_list(
    space_path: Annotated[str, typer.Argument(help="Full path to space")],
) -> None:
    """List links in a space."""
    setup_logging()
    links = list_links(space_path)
    typer.echo(json.dumps(links, indent=2))


@link_app.command("delete")
@handle_cli_errors
def cmd_link_delete(
    space_path: Annotated[str, typer.Argument(help="Full path to space")],
    link_id: Annotated[str, typer.Argument(help="Link ID")],
) -> None:
    """Delete a link by ID."""
    setup_logging()
    delete_link(space_path, link_id)
    typer.echo(f"Link '{link_id}' deleted successfully.")


@search_app.command("keyword")
@handle_cli_errors
def cmd_search_keyword(
    space_path: Annotated[str, typer.Argument(help="Full path to space")],
    query: Annotated[str, typer.Argument(help="Search query")],
) -> None:
    """Keyword search entries."""
    setup_logging()
    results = search_entries(space_path, query)
    typer.echo(json.dumps(results, indent=2))
