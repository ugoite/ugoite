"""Entry endpoints."""

import json
import logging
import uuid
from typing import Any

import ugoite_core
from fastapi import APIRouter, HTTPException, status

from app.api.endpoints.space import (
    _ensure_space_exists,
    _storage_config,
    _validate_entry_markdown_against_form,
    _validate_path_id,
)
from app.models.payloads import EntryCreate, EntryRestore, EntryUpdate

router = APIRouter()
logger = logging.getLogger(__name__)


@router.post(
    "/spaces/{space_id}/entries",
    status_code=status.HTTP_201_CREATED,
)
async def create_entry_endpoint(
    space_id: str,
    payload: EntryCreate,
) -> dict[str, Any]:
    """Create a new entry."""
    _validate_path_id(space_id, "space_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    entry_id = payload.id or str(uuid.uuid4())

    try:
        await ugoite_core.create_entry(
            storage_config,
            space_id,
            entry_id,
            payload.content,
        )
        entry_data = await ugoite_core.get_entry(storage_config, space_id, entry_id)
    except RuntimeError as e:
        if "already exists" in str(e).lower():
            raise HTTPException(
                status_code=status.HTTP_409_CONFLICT,
                detail=str(e),
            ) from e
        if "form" in str(e).lower() or "unknown" in str(e).lower():
            raise HTTPException(
                status_code=status.HTTP_422_UNPROCESSABLE_ENTITY,
                detail=str(e),
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
    except Exception as e:
        logger.exception("Failed to create entry")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e

    return {"id": entry_id, "revision_id": entry_data.get("revision_id", "")}


@router.get("/spaces/{space_id}/entries")
async def list_entries_endpoint(space_id: str) -> list[dict[str, Any]]:
    """List all entries in a space."""
    _validate_path_id(space_id, "space_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        return await ugoite_core.list_entries(storage_config, space_id)
    except Exception as e:
        logger.exception("Failed to list entries")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/spaces/{space_id}/entries/{entry_id}")
async def get_entry_endpoint(space_id: str, entry_id: str) -> dict[str, Any]:
    """Get an entry by ID."""
    _validate_path_id(space_id, "space_id")
    _validate_path_id(entry_id, "entry_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        return await ugoite_core.get_entry(storage_config, space_id, entry_id)
    except RuntimeError as e:
        if "not found" in str(e).lower():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=str(e),
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
    except Exception as e:
        logger.exception("Failed to get entry")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.put("/spaces/{space_id}/entries/{entry_id}")
async def update_entry_endpoint(
    space_id: str,
    entry_id: str,
    payload: EntryUpdate,
) -> dict[str, Any]:
    """Update an existing entry.

    Requires parent_revision_id for optimistic concurrency control.
    Returns 409 Conflict if the revision has changed.
    """
    _validate_path_id(space_id, "space_id")
    _validate_path_id(entry_id, "entry_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        await _validate_entry_markdown_against_form(
            storage_config,
            space_id,
            payload.markdown,
        )
        assets_json = json.dumps(payload.assets) if payload.assets is not None else None
        updated_entry = await ugoite_core.update_entry(
            storage_config,
            space_id,
            entry_id,
            payload.markdown,
            payload.parent_revision_id,
            assets_json=assets_json,
        )
        return {
            "id": entry_id,
            "revision_id": updated_entry.get("revision_id", ""),
        }
    except RuntimeError as e:
        msg = str(e)
        if "conflict" in msg.lower():
            try:
                current_entry = await ugoite_core.get_entry(
                    storage_config,
                    space_id,
                    entry_id,
                )
                raise HTTPException(
                    status_code=status.HTTP_409_CONFLICT,
                    detail={
                        "message": msg,
                        "current_revision": current_entry,
                    },
                ) from e
            except RuntimeError:
                raise HTTPException(
                    status_code=status.HTTP_409_CONFLICT,
                    detail=msg,
                ) from e
        if "not found" in msg.lower():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=msg,
            ) from e
        if "form" in msg.lower() or "unknown" in msg.lower():
            raise HTTPException(
                status_code=status.HTTP_422_UNPROCESSABLE_ENTITY,
                detail=msg,
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=msg,
        ) from e
    except HTTPException:
        raise
    except Exception as e:
        logger.exception("Failed to update entry")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.delete("/spaces/{space_id}/entries/{entry_id}")
async def delete_entry_endpoint(
    space_id: str,
    entry_id: str,
) -> dict[str, str]:
    """Tombstone (soft delete) an entry."""
    _validate_path_id(space_id, "space_id")
    _validate_path_id(entry_id, "entry_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        await ugoite_core.delete_entry(storage_config, space_id, entry_id)
    except RuntimeError as e:
        if "not found" in str(e).lower():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=str(e),
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
    except Exception as e:
        logger.exception("Failed to delete entry")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
    else:
        return {"id": entry_id, "status": "deleted"}


@router.get("/spaces/{space_id}/entries/{entry_id}/history")
async def get_entry_history_endpoint(
    space_id: str,
    entry_id: str,
) -> dict[str, Any]:
    """Get the revision history for an entry."""
    _validate_path_id(space_id, "space_id")
    _validate_path_id(entry_id, "entry_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        return await ugoite_core.get_entry_history(storage_config, space_id, entry_id)
    except RuntimeError as e:
        if "not found" in str(e).lower():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=str(e),
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
    except Exception as e:
        logger.exception("Failed to get entry history")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/spaces/{space_id}/entries/{entry_id}/history/{revision_id}")
async def get_entry_revision_endpoint(
    space_id: str,
    entry_id: str,
    revision_id: str,
) -> dict[str, Any]:
    """Get a specific revision of an entry."""
    _validate_path_id(space_id, "space_id")
    _validate_path_id(entry_id, "entry_id")
    _validate_path_id(revision_id, "revision_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        return await ugoite_core.get_entry_revision(
            storage_config,
            space_id,
            entry_id,
            revision_id,
        )
    except RuntimeError as e:
        if "not found" in str(e).lower():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=str(e),
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
    except Exception as e:
        logger.exception("Failed to get entry revision")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.post("/spaces/{space_id}/entries/{entry_id}/restore")
async def restore_entry_endpoint(
    space_id: str,
    entry_id: str,
    payload: EntryRestore,
) -> dict[str, Any]:
    """Restore an entry to a previous revision."""
    _validate_path_id(space_id, "space_id")
    _validate_path_id(entry_id, "entry_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        entry_data = await ugoite_core.restore_entry(
            storage_config,
            space_id,
            entry_id,
            payload.revision_id,
        )
    except RuntimeError as e:
        if "not found" in str(e).lower():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=str(e),
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
    except Exception as e:
        logger.exception("Failed to restore entry")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e

    return entry_data
