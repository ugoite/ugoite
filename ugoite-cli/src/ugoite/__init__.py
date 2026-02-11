"""Ugoite CLI package."""

from .assets import (
    AssetReferencedError,
    delete_asset,
    list_assets,
    save_asset,
)
from .entries import (
    EntryExistsError,
    RevisionMismatchError,
    create_entry,
    delete_entry,
    get_entry,
    get_entry_history,
    get_entry_revision,
    list_entries,
    restore_entry,
    update_entry,
)
from .forms import (
    get_form,
    list_column_types,
    list_forms,
    migrate_form,
    upsert_form,
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
from .search import search_entries
from .space import (
    SpaceExistsError,
    create_space,
    get_space,
    list_spaces,
    patch_space,
    space_path,
    test_storage_connection,
)
from .utils import resolve_existing_path

__all__ = [
    "AssetReferencedError",
    "EntryExistsError",
    "Indexer",
    "RevisionMismatchError",
    "SpaceExistsError",
    "aggregate_stats",
    "build_response_signature",
    "create_entry",
    "create_link",
    "create_space",
    "delete_asset",
    "delete_entry",
    "delete_link",
    "ensure_global_json",
    "extract_properties",
    "get_entry",
    "get_entry_history",
    "get_entry_revision",
    "get_form",
    "get_space",
    "list_assets",
    "list_column_types",
    "list_entries",
    "list_forms",
    "list_links",
    "list_spaces",
    "load_hmac_material",
    "migrate_form",
    "patch_space",
    "query_index",
    "resolve_existing_path",
    "restore_entry",
    "save_asset",
    "search_entries",
    "space_path",
    "test_storage_connection",
    "update_entry",
    "upsert_form",
    "validate_properties",
]
