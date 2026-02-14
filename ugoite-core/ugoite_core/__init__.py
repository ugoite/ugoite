"""ugoite-core: Rust-based core logic and Python bindings."""

from contextlib import suppress
from typing import Any, cast

from . import _ugoite_core as _core
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
build_response_signature = _core_any.build_response_signature
create_entry = _core_any.create_entry
create_space = _core_any.create_space
create_sample_space = _core_any.create_sample_space
create_sample_space_job = _core_any.create_sample_space_job
get_sample_space_job = _core_any.get_sample_space_job
list_sample_scenarios = _core_any.list_sample_scenarios
delete_asset = _core_any.delete_asset
delete_entry = _core_any.delete_entry
extract_properties = _core_any.extract_properties
get_entry = _core_any.get_entry
get_entry_history = _core_any.get_entry_history
get_entry_revision = _core_any.get_entry_revision
get_form = _core_any.get_form
get_space = _core_any.get_space
list_assets = _core_any.list_assets
list_column_types = _core_any.list_column_types
list_entries = _core_any.list_entries
list_forms = _core_any.list_forms
list_spaces = _core_any.list_spaces
load_hmac_material = _core_any.load_hmac_material
load_response_hmac_material = _core_any.load_response_hmac_material
migrate_form = _core_any.migrate_form
patch_space = _core_any.patch_space
query_index = _core_any.query_index
reindex_all = _core_any.reindex_all
restore_entry = _core_any.restore_entry
save_asset = _core_any.save_asset
search_entries = _core_any.search_entries
test_storage_connection = _core_any.test_storage_connection
update_entry = _core_any.update_entry
update_entry_index = _core_any.update_entry_index
upsert_form = _core_any.upsert_form
validate_properties = _core_any.validate_properties
create_sql = _core_any.create_sql
delete_sql = _core_any.delete_sql
get_sql = _core_any.get_sql
list_sql = _core_any.list_sql
update_sql = _core_any.update_sql
create_sql_session = _core_any.create_sql_session
get_sql_session_status = _core_any.get_sql_session_status
get_sql_session_count = _core_any.get_sql_session_count
get_sql_session_rows = _core_any.get_sql_session_rows
get_sql_session_rows_all = _core_any.get_sql_session_rows_all

__all__ = [
    "SqlLintDiagnostic",
    "build_response_signature",
    "build_sql_schema",
    "create_entry",
    "create_sample_space",
    "create_sample_space_job",
    "create_space",
    "create_sql",
    "create_sql_session",
    "delete_asset",
    "delete_entry",
    "delete_sql",
    "extract_properties",
    "get_entry",
    "get_entry_history",
    "get_entry_revision",
    "get_form",
    "get_sample_space_job",
    "get_space",
    "get_sql",
    "get_sql_session_count",
    "get_sql_session_rows",
    "get_sql_session_rows_all",
    "get_sql_session_status",
    "lint_sql",
    "list_assets",
    "list_column_types",
    "list_entries",
    "list_forms",
    "list_sample_scenarios",
    "list_spaces",
    "list_sql",
    "load_hmac_material",
    "load_response_hmac_material",
    "load_sql_rules",
    "migrate_form",
    "patch_space",
    "query_index",
    "reindex_all",
    "restore_entry",
    "save_asset",
    "search_entries",
    "sql_completions",
    "test_storage_connection",
    "update_entry",
    "update_entry_index",
    "update_sql",
    "upsert_form",
    "validate_properties",
]
