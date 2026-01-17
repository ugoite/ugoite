"""Note endpoints."""

import json
import logging
import uuid
from typing import Any

import ieapp_core
from fastapi import APIRouter, HTTPException, status

from app.api.endpoints.workspace import (
    _ensure_workspace_exists,
    _storage_config,
    _validate_note_markdown_against_class,
    _validate_path_id,
)
from app.models.classes import NoteCreate, NoteRestore, NoteUpdate

router = APIRouter()
logger = logging.getLogger(__name__)


@router.post(
    "/api/workspaces/{workspace_id}/notes",
    status_code=status.HTTP_201_CREATED,
)
async def create_note_endpoint(
    workspace_id: str,
    payload: NoteCreate,
) -> dict[str, Any]:
    """Create a new note."""
    _validate_path_id(workspace_id, "workspace_id")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    note_id = payload.id or str(uuid.uuid4())

    try:
        await ieapp_core.create_note(
            storage_config,
            workspace_id,
            note_id,
            payload.content,
        )
        note_data = await ieapp_core.get_note(storage_config, workspace_id, note_id)
    except RuntimeError as e:
        if "already exists" in str(e).lower():
            raise HTTPException(
                status_code=status.HTTP_409_CONFLICT,
                detail=str(e),
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
    except Exception as e:
        logger.exception("Failed to create note")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e

    return {"id": note_id, "revision_id": note_data.get("revision_id", "")}


@router.get("/api/workspaces/{workspace_id}/notes")
async def list_notes_endpoint(workspace_id: str) -> list[dict[str, Any]]:
    """List all notes in a workspace."""
    _validate_path_id(workspace_id, "workspace_id")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    try:
        return await ieapp_core.list_notes(storage_config, workspace_id)
    except Exception as e:
        logger.exception("Failed to list notes")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/api/workspaces/{workspace_id}/notes/{note_id}")
async def get_note_endpoint(workspace_id: str, note_id: str) -> dict[str, Any]:
    """Get a note by ID."""
    _validate_path_id(workspace_id, "workspace_id")
    _validate_path_id(note_id, "note_id")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    try:
        return await ieapp_core.get_note(storage_config, workspace_id, note_id)
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
        logger.exception("Failed to get note")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.put("/api/workspaces/{workspace_id}/notes/{note_id}")
async def update_note_endpoint(
    workspace_id: str,
    note_id: str,
    payload: NoteUpdate,
) -> dict[str, Any]:
    """Update an existing note.

    Requires parent_revision_id for optimistic concurrency control.
    Returns 409 Conflict if the revision has changed.
    """
    _validate_path_id(workspace_id, "workspace_id")
    _validate_path_id(note_id, "note_id")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    try:
        await _validate_note_markdown_against_class(
            storage_config,
            workspace_id,
            payload.markdown,
        )
        attachments_json = (
            json.dumps(payload.attachments) if payload.attachments is not None else None
        )
        updated_note = await ieapp_core.update_note(
            storage_config,
            workspace_id,
            note_id,
            payload.markdown,
            payload.parent_revision_id,
            attachments_json=attachments_json,
        )
        return {
            "id": note_id,
            "revision_id": updated_note.get("revision_id", ""),
        }
    except RuntimeError as e:
        msg = str(e)
        if "conflict" in msg.lower():
            try:
                current_note = await ieapp_core.get_note(
                    storage_config,
                    workspace_id,
                    note_id,
                )
                raise HTTPException(
                    status_code=status.HTTP_409_CONFLICT,
                    detail={
                        "message": msg,
                        "current_revision": current_note,
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
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=msg,
        ) from e
    except HTTPException:
        raise
    except Exception as e:
        logger.exception("Failed to update note")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.delete("/api/workspaces/{workspace_id}/notes/{note_id}")
async def delete_note_endpoint(
    workspace_id: str,
    note_id: str,
) -> dict[str, str]:
    """Tombstone (soft delete) a note."""
    _validate_path_id(workspace_id, "workspace_id")
    _validate_path_id(note_id, "note_id")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    try:
        await ieapp_core.delete_note(storage_config, workspace_id, note_id)
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
        logger.exception("Failed to delete note")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
    else:
        return {"id": note_id, "status": "deleted"}


@router.get("/api/workspaces/{workspace_id}/notes/{note_id}/history")
async def get_note_history_endpoint(
    workspace_id: str,
    note_id: str,
) -> dict[str, Any]:
    """Get the revision history for a note."""
    _validate_path_id(workspace_id, "workspace_id")
    _validate_path_id(note_id, "note_id")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    try:
        return await ieapp_core.get_note_history(storage_config, workspace_id, note_id)
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
        logger.exception("Failed to get note history")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/api/workspaces/{workspace_id}/notes/{note_id}/history/{revision_id}")
async def get_note_revision_endpoint(
    workspace_id: str,
    note_id: str,
    revision_id: str,
) -> dict[str, Any]:
    """Get a specific revision of a note."""
    _validate_path_id(workspace_id, "workspace_id")
    _validate_path_id(note_id, "note_id")
    _validate_path_id(revision_id, "revision_id")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    try:
        return await ieapp_core.get_note_revision(
            storage_config,
            workspace_id,
            note_id,
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
        logger.exception("Failed to get note revision")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.post("/api/workspaces/{workspace_id}/notes/{note_id}/restore")
async def restore_note_endpoint(
    workspace_id: str,
    note_id: str,
    payload: NoteRestore,
) -> dict[str, Any]:
    """Restore a note to a previous revision."""
    _validate_path_id(workspace_id, "workspace_id")
    _validate_path_id(note_id, "note_id")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    try:
        note_data = await ieapp_core.restore_note(
            storage_config,
            workspace_id,
            note_id,
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
        logger.exception("Failed to restore note")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e

    return note_data
