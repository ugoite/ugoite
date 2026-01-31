"""ieapp-core: Rust-based core logic and Python bindings."""

from ._ieapp_core import (
    build_response_signature,
    create_link,
    create_note,
    create_workspace,
    delete_attachment,
    delete_link,
    delete_note,
    extract_properties,
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
    load_hmac_material,
    migrate_class,
    patch_workspace,
    query_index,
    reindex_all,
    restore_note,
    save_attachment,
    search_notes,
    test_storage_connection,
    update_note,
    update_note_index,
    upsert_class,
    validate_properties,
)
from .sql_rules import (
    SqlLintDiagnostic,
    build_sql_schema,
    lint_sql,
    load_sql_rules,
    sql_completions,
)

# Export the docstring from the native module
try:
    from . import _ieapp_core

    __doc__ = _ieapp_core.__doc__
except ImportError:
    # Failing to import the native module for its docstring is non-fatal.
    pass

__all__ = [
    "SqlLintDiagnostic",
    "build_response_signature",
    "build_sql_schema",
    "create_link",
    "create_note",
    "create_workspace",
    "delete_attachment",
    "delete_link",
    "delete_note",
    "extract_properties",
    "get_class",
    "get_note",
    "get_note_history",
    "get_note_revision",
    "get_workspace",
    "lint_sql",
    "list_attachments",
    "list_classes",
    "list_column_types",
    "list_links",
    "list_notes",
    "list_workspaces",
    "load_hmac_material",
    "load_sql_rules",
    "migrate_class",
    "patch_workspace",
    "query_index",
    "reindex_all",
    "restore_note",
    "save_attachment",
    "search_notes",
    "sql_completions",
    "test_storage_connection",
    "update_note",
    "update_note_index",
    "upsert_class",
    "validate_properties",
]
