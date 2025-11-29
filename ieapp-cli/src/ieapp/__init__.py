"""IEapp CLI package."""

from .indexer import (
    Indexer,
    aggregate_stats,
    extract_properties,
    query_index,
    validate_properties,
)
from .workspace import WorkspaceExistsError, create_workspace

# Alias query_index to query for convenience
query = query_index

__all__ = [
    "Indexer",
    "WorkspaceExistsError",
    "aggregate_stats",
    "create_workspace",
    "extract_properties",
    "query",
    "validate_properties",
]
