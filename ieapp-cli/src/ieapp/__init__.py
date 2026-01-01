"""IEapp CLI package."""

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
from .utils import safe_resolve_path
from .workspace import (
    WorkspaceExistsError,
    create_workspace,
    get_workspace,
    list_workspaces,
)

# Alias query_index to query for convenience
query = query_index

__all__ = [
    "Indexer",
    "NoteExistsError",
    "RevisionMismatchError",
    "WorkspaceExistsError",
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
    "safe_resolve_path",
    "update_note",
    "validate_properties",
]
