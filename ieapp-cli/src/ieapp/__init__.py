"""IEapp CLI package."""

import os
from typing import Any

from .indexer import (
    Indexer,
    aggregate_stats,
    extract_properties,
    query_index,
    validate_properties,
)
from .notes import (
    NoteExistsError,
    RevisionMismatchError,
    create_note,
    delete_note,
    get_note,
    get_note_history,
    get_note_revision,
    list_notes,
    restore_note,
    update_note,
)
from .workspace import (
    WorkspaceExistsError,
    create_workspace,
    get_workspace,
    list_workspaces,
)


class WorkspacePathError(ValueError):
    """Raised when workspace path is not provided and not set in environment."""


def query(
    workspace_path: str | None = None,
    **kwargs: object,
) -> list[dict[str, Any]]:
    """Query the index using keyword arguments as filters.

    If workspace_path is not provided, it falls back to IEAPP_WORKSPACE_ROOT env var.
    """
    if workspace_path is None:
        workspace_path = os.environ.get("IEAPP_WORKSPACE_ROOT")

    if not workspace_path:
        msg = "workspace_path must be provided or IEAPP_WORKSPACE_ROOT set"
        raise WorkspacePathError(msg)

    return query_index(workspace_path, filter_dict=dict(kwargs))


__all__ = [
    "Indexer",
    "NoteExistsError",
    "RevisionMismatchError",
    "WorkspaceExistsError",
    "WorkspacePathError",
    "aggregate_stats",
    "create_note",
    "create_workspace",
    "delete_note",
    "extract_properties",
    "get_note",
    "get_note_history",
    "get_note_revision",
    "get_workspace",
    "list_notes",
    "list_workspaces",
    "query",  # Alias for query_index
    "query_index",
    "restore_note",
    "update_note",
    "validate_properties",
]
