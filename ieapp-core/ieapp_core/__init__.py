"""ieapp-core: Rust-based core logic and Python bindings."""

from ._ieapp_core import (
    create_link,
    create_note,
    create_workspace,
    delete_attachment,
    delete_link,
    delete_note,
    get_class,
    get_note,
    get_note_history,
    get_note_revision,
    get_workspace,
    list_attachments,
    list_classes,
    list_column_types,
    list_links,
    list_notes,
    list_workspaces,
    patch_workspace,
    query_index,
    restore_note,
    save_attachment,
    search_notes,
    test_storage_connection,
    update_note,
    upsert_class,
)

# Export the docstring from the native module
try:
    from . import _ieapp_core

    __doc__ = _ieapp_core.__doc__
except ImportError:
    # Failing to import the native module for its docstring is non-fatal.
    pass

__all__ = [
    "create_link",
    "create_note",
    "create_workspace",
    "delete_attachment",
    "delete_link",
    "delete_note",
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
    "patch_workspace",
    "query_index",
    "restore_note",
    "save_attachment",
    "search_notes",
    "test_storage_connection",
    "update_note",
    "upsert_class",
]
