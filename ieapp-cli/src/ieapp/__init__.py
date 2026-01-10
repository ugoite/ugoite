"""IEapp CLI package."""

from .attachments import (
    AttachmentReferencedError,
    delete_attachment,
    list_attachments,
    save_attachment,
)
from .classes import (
    get_class,
    list_classes,
    list_column_types,
    migrate_class,
    upsert_class,
)
from .hmac_manager import (
    build_response_signature,
    ensure_global_json,
    load_hmac_material,
)
from .indexer import (
    Indexer,
    aggregate_stats,
    extract_properties,
    query_index,
    validate_properties,
)
from .links import create_link, delete_link, list_links
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
from .search import search_notes
from .utils import resolve_existing_path
from .workspace import (
    WorkspaceExistsError,
    create_workspace,
    get_workspace,
    list_workspaces,
    patch_workspace,
    test_storage_connection,
    workspace_path,
)

# Alias query_index to query for convenience
query = query_index

__all__ = [
    "AttachmentReferencedError",
    "Indexer",
    "NoteExistsError",
    "RevisionMismatchError",
    "WorkspaceExistsError",
    "aggregate_stats",
    "build_response_signature",
    "create_link",
    "create_note",
    "create_workspace",
    "delete_attachment",
    "delete_link",
    "delete_note",
    "ensure_global_json",
    "extract_properties",
    "get_class",
    "get_note",
    "get_note_history",
    "get_note_revision",
    "get_workspace",
    "list_attachments",
    "list_classes",
    "list_column_types",
    "list_links",
    "list_notes",
    "list_workspaces",
    "load_hmac_material",
    "migrate_class",
    "patch_workspace",
    "query",  # Alias for query_index
    "query_index",
    "resolve_existing_path",
    "restore_note",
    "save_attachment",
    "search_notes",
    "test_storage_connection",
    "update_note",
    "upsert_class",
    "validate_properties",
    "workspace_path",
]
