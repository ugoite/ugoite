"""IEapp CLI package."""

from .indexer import Indexer, query_index
from .workspace import WorkspaceExistsError, create_workspace

# Alias query_index to query for convenience
query = query_index

__all__ = ["WorkspaceExistsError", "create_workspace", "Indexer", "query"]
