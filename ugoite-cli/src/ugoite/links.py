"""Link helpers removed in favor of row_reference fields."""

from __future__ import annotations

from typing import Any


def _links_removed() -> RuntimeError:
    return RuntimeError("Link commands removed. Use row_reference fields instead.")


def create_link(
    _space_path: str,
    **_kwargs: object,
) -> dict[str, object]:
    """Raise link removal error."""
    raise _links_removed()


def list_links(
    _space_path: str,
    *,
    _fs: object | None = None,
) -> list[dict[str, Any]]:
    """Raise link removal error."""
    raise _links_removed()


def delete_link(
    _space_path: str,
    _link_id: str,
    *,
    _fs: object | None = None,
) -> None:
    """Raise link removal error."""
    raise _links_removed()
