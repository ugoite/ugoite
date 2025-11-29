"""IEapp CLI package."""

from .indexer import Indexer, query_index
from .workspace import WorkspaceExistsError, create_workspace

# Alias query_index to query for convenience
query = query_index

__all__ = ["Indexer", "WorkspaceExistsError", "create_workspace", "query"]
