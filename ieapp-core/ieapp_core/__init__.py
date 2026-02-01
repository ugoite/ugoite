"""ieapp-core: Rust-based core logic and Python bindings."""

from contextlib import suppress
from typing import Any, cast

from . import _ieapp_core as _core
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
with suppress(ImportError):
    __doc__ = _core.__doc__

_core_any = cast("Any", _core)
create_sql = _core_any.create_sql
delete_sql = _core_any.delete_sql
get_sql = _core_any.get_sql
list_sql = _core_any.list_sql
update_sql = _core_any.update_sql

__all__ = [
    "SqlLintDiagnostic",
    "build_response_signature",
    "build_sql_schema",
    "create_link",
    "create_note",
    "create_sql",
    "create_workspace",
    "delete_attachment",
    "delete_link",
    "delete_note",
    "delete_sql",
    "extract_properties",
    "get_class",
    "get_note",
    "get_note_history",
    "get_note_revision",
    "get_sql",
    "get_workspace",
    "lint_sql",
    "list_attachments",
    "list_classes",
    "list_column_types",
    "list_links",
    "list_notes",
    "list_sql",
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
    "update_sql",
    "upsert_class",
    "validate_properties",
]
