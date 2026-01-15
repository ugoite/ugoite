"""Workspace endpoints."""

import json
import logging
import uuid
from typing import Annotated, Any

import ieapp_core
from fastapi import APIRouter, File, HTTPException, Query, UploadFile, status

from app.core.config import get_root_path
from app.core.ids import validate_id
from app.core.storage import storage_config_from_root, workspace_uri
from app.models.classes import (
    ClassCreate,
    LinkCreate,
    NoteCreate,
    NoteRestore,
    NoteUpdate,
    QueryRequest,
    TestConnectionRequest,
    WorkspaceCreate,
    WorkspacePatch,
)

router = APIRouter()
logger = logging.getLogger(__name__)


def _storage_config() -> dict[str, str]:
    """Build storage config for the current workspace root."""
    return storage_config_from_root(get_root_path())


async def _ensure_workspace_exists(
    storage_config: dict[str, str],
    workspace_id: str,
) -> dict[str, Any]:
    """Ensure a workspace exists or raise 404."""
    try:
        return await ieapp_core.get_workspace(storage_config, workspace_id)
    except RuntimeError as exc:
        msg = str(exc)
        if "not found" in msg.lower():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Workspace not found: {workspace_id}",
            ) from exc
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=msg,
        ) from exc


def _format_class_validation_errors(errors: list[dict[str, Any]]) -> str:
    """Format class validation warnings into a single human-readable string.

    The validator returns a list of dicts describing issues. This helper
    consolidates them into a newline-separated message suitable for HTTP API
    responses.

    Args:
        errors: A list of dictionaries containing keys like "message" and
            "field" as returned by the property validator.

    Returns:
        A newline-separated string describing the validation issues.

    """
    parts: list[str] = []
    for err in errors:
        message = err.get("message")
        field = err.get("field")
        if isinstance(message, str) and message:
            parts.append(message)
        elif isinstance(field, str) and field:
            parts.append(f"Invalid field: {field}")
        else:
            parts.append("Class validation error")
    return "\n".join(parts)


async def _validate_note_markdown_against_class(
    storage_config: dict[str, str],
    workspace_id: str,
    markdown: str,
) -> None:
    """Validate extracted note properties against the workspace class."""
    properties = ieapp_core.extract_properties(markdown)
    note_class = properties.get("class")
    if not isinstance(note_class, str) or not note_class.strip():
        return

    try:
        class_def = await ieapp_core.get_class(storage_config, workspace_id, note_class)
    except RuntimeError as e:
        if "not found" not in str(e).lower():
            raise HTTPException(
                status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
                detail="Failed to load class definition",
            ) from e
        raise HTTPException(
            status_code=status.HTTP_422_UNPROCESSABLE_CONTENT,
            detail=f"Class not found: {note_class}",
        ) from e

    _casted, warnings = ieapp_core.validate_properties(
        json.dumps(properties),
        json.dumps(class_def),
    )
    if warnings:
        raise HTTPException(
            status_code=status.HTTP_422_UNPROCESSABLE_CONTENT,
            detail=_format_class_validation_errors(warnings),
        )


def _validate_path_id(identifier: str, name: str) -> None:
    """Validate identifier using shared ieapp-cli rules."""
    try:
        validate_id(identifier, name)
    except ValueError as exc:  # pragma: no cover - exercised via API tests
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=str(exc),
        ) from exc


def _workspace_uri(workspace_id: str) -> str:
    """Return workspace URI/path for API responses."""
    return workspace_uri(get_root_path(), workspace_id)


@router.get("/workspaces")
async def list_workspaces_endpoint() -> list[dict[str, Any]]:
    """List all workspaces."""
    try:
        storage_config = _storage_config()
        workspace_ids = await ieapp_core.list_workspaces(storage_config)
        results: list[dict[str, Any]] = []
        for ws_id in workspace_ids:
            try:
                results.append(await ieapp_core.get_workspace(storage_config, ws_id))
            except RuntimeError as exc:
                logger.warning("Failed to read workspace meta %s: %s", ws_id, exc)
    except Exception as e:
        logger.exception("Failed to list workspaces")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
    else:
        return results


@router.post("/workspaces", status_code=status.HTTP_201_CREATED)
async def create_workspace_endpoint(
    payload: WorkspaceCreate,
) -> dict[str, str]:
    """Create a new workspace."""
    workspace_id = payload.name  # Using name as ID for now per simple spec

    try:
        storage_config = _storage_config()
        await ieapp_core.create_workspace(storage_config, workspace_id)
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
        logger.exception("Failed to create workspace")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e

    return {
        "id": workspace_id,
        "name": payload.name,
        "path": _workspace_uri(workspace_id),
    }


@router.get("/workspaces/{workspace_id}")
async def get_workspace_endpoint(workspace_id: str) -> dict[str, Any]:
    """Get workspace metadata."""
    _validate_path_id(workspace_id, "workspace_id")
    try:
        storage_config = _storage_config()
        return await ieapp_core.get_workspace(storage_config, workspace_id)
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
        logger.exception("Failed to get workspace")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.patch("/workspaces/{workspace_id}")
async def patch_workspace_endpoint(
    workspace_id: str,
    payload: WorkspacePatch,
) -> dict[str, Any]:
    """Update workspace metadata/settings including storage connector."""
    _validate_path_id(workspace_id, "workspace_id")

    # Build patch dict from payload
    patch_data = {}
    if payload.name is not None:
        patch_data["name"] = payload.name
    if payload.storage_config is not None:
        patch_data["storage_config"] = payload.storage_config
    if payload.settings is not None:
        patch_data["settings"] = payload.settings

    try:
        storage_config = _storage_config()
        return await ieapp_core.patch_workspace(
            storage_config,
            workspace_id,
            json.dumps(patch_data),
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
        logger.exception("Failed to patch workspace")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.post("/workspaces/{workspace_id}/test-connection")
async def test_connection_endpoint(
    workspace_id: str,
    payload: TestConnectionRequest,
) -> dict[str, Any]:
    """Validate the provided storage connector (stubbed for Milestone 6)."""
    _validate_path_id(workspace_id, "workspace_id")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    try:
        return await ieapp_core.test_storage_connection(payload.storage_config)
    except ValueError as e:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=str(e),
        ) from e


@router.post(
    "/workspaces/{workspace_id}/notes",
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


@router.get("/workspaces/{workspace_id}/notes")
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


@router.get("/workspaces/{workspace_id}/notes/{note_id}")
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


@router.put("/workspaces/{workspace_id}/notes/{note_id}")
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


@router.post(
    "/workspaces/{workspace_id}/attachments",
    status_code=status.HTTP_201_CREATED,
)
async def upload_attachment_endpoint(
    workspace_id: str,
    file: Annotated[UploadFile, File(...)],
) -> dict[str, Any]:
    """Upload a binary attachment into the workspace attachments directory."""
    _validate_path_id(workspace_id, "workspace_id")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    contents = await file.read()

    try:
        return await ieapp_core.save_attachment(
            storage_config,
            workspace_id,
            file.filename or "",
            contents,
        )
    except Exception as e:
        logger.exception("Failed to save attachment")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/workspaces/{workspace_id}/attachments")
async def list_attachments_endpoint(
    workspace_id: str,
) -> list[dict[str, Any]]:
    """List all attachments in the workspace."""
    _validate_path_id(workspace_id, "workspace_id")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    try:
        return await ieapp_core.list_attachments(storage_config, workspace_id)
    except Exception as e:
        logger.exception("Failed to list attachments")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.delete("/workspaces/{workspace_id}/attachments/{attachment_id}")
async def delete_attachment_endpoint(
    workspace_id: str,
    attachment_id: str,
) -> dict[str, str]:
    """Delete an attachment if it is not referenced by any note."""
    _validate_path_id(workspace_id, "workspace_id")
    _validate_path_id(attachment_id, "attachment_id")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    try:
        await ieapp_core.delete_attachment(storage_config, workspace_id, attachment_id)
    except RuntimeError as e:
        msg = str(e)
        if "referenced" in msg.lower():
            raise HTTPException(
                status_code=status.HTTP_409_CONFLICT,
                detail=msg,
            ) from e
        if "not found" in msg.lower():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail="Not found",
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=msg,
        ) from e
    except Exception as e:
        logger.exception("Failed to delete attachment")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
    else:
        return {"status": "deleted", "id": attachment_id}


@router.delete("/workspaces/{workspace_id}/notes/{note_id}")
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


@router.get("/workspaces/{workspace_id}/notes/{note_id}/history")
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


@router.get("/workspaces/{workspace_id}/notes/{note_id}/history/{revision_id}")
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


@router.post("/workspaces/{workspace_id}/notes/{note_id}/restore")
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


@router.post(
    "/workspaces/{workspace_id}/links",
    status_code=status.HTTP_201_CREATED,
)
async def create_link_endpoint(
    workspace_id: str,
    payload: LinkCreate,
) -> dict[str, Any]:
    """Create a bi-directional link between two notes."""
    _validate_path_id(workspace_id, "workspace_id")
    _validate_path_id(payload.source, "source")
    _validate_path_id(payload.target, "target")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    link_id = uuid.uuid4().hex

    try:
        return await ieapp_core.create_link(
            storage_config,
            workspace_id,
            payload.source,
            payload.target,
            payload.kind,
            link_id,
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
        logger.exception("Failed to create link")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/workspaces/{workspace_id}/links")
async def list_links_endpoint(workspace_id: str) -> list[dict[str, Any]]:
    """List all unique links in the workspace."""
    _validate_path_id(workspace_id, "workspace_id")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    try:
        return await ieapp_core.list_links(storage_config, workspace_id)
    except Exception as e:
        logger.exception("Failed to list links")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.delete("/workspaces/{workspace_id}/links/{link_id}")
async def delete_link_endpoint(
    workspace_id: str,
    link_id: str,
) -> dict[str, str]:
    """Delete a link and remove it from both notes."""
    _validate_path_id(workspace_id, "workspace_id")
    _validate_path_id(link_id, "link_id")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    try:
        await ieapp_core.delete_link(storage_config, workspace_id, link_id)
    except RuntimeError as e:
        if "not found" in str(e).lower():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail="Link not found",
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
    except Exception as e:
        logger.exception("Failed to delete link")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
    else:
        return {"status": "deleted", "id": link_id}


@router.post("/workspaces/{workspace_id}/query")
async def query_endpoint(
    workspace_id: str,
    payload: QueryRequest,
) -> list[dict[str, Any]]:
    """Query the workspace index."""
    _validate_path_id(workspace_id, "workspace_id")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    try:
        return await ieapp_core.query_index(
            storage_config,
            workspace_id,
            json.dumps(payload.filter),
        )
    except Exception as e:
        logger.exception("Query failed")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/workspaces/{workspace_id}/search")
async def search_endpoint(
    workspace_id: str,
    q: Annotated[str, Query(..., min_length=1)],
) -> list[dict[str, Any]]:
    """Hybrid keyword search using inverted index with on-demand indexing."""
    _validate_path_id(workspace_id, "workspace_id")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    try:
        return await ieapp_core.search_notes(storage_config, workspace_id, q)
    except Exception as e:
        logger.exception("Search failed")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/workspaces/{workspace_id}/classes")
async def list_classes_endpoint(workspace_id: str) -> list[dict[str, Any]]:
    """List all classes in the workspace."""
    _validate_path_id(workspace_id, "workspace_id")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    try:
        return await ieapp_core.list_classes(storage_config, workspace_id)
    except Exception as e:
        logger.exception("Failed to list classes")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/workspaces/{workspace_id}/classes/types")
async def list_class_types_endpoint(workspace_id: str) -> list[str]:
    """Get list of available column types."""
    _validate_path_id(workspace_id, "workspace_id")
    try:
        # Verify workspace exists even though types are static
        storage_config = _storage_config()
        await _ensure_workspace_exists(storage_config, workspace_id)
        return await ieapp_core.list_column_types()
    except Exception as e:
        if isinstance(e, HTTPException):
            raise
        logger.exception("Failed to list class types")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/workspaces/{workspace_id}/classes/{class_name}")
async def get_class_endpoint(workspace_id: str, class_name: str) -> dict[str, Any]:
    """Get a specific class definition."""
    _validate_path_id(workspace_id, "workspace_id")
    _validate_path_id(class_name, "class_name")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    try:
        return await ieapp_core.get_class(storage_config, workspace_id, class_name)
    except RuntimeError as e:
        if "not found" in str(e).lower():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Class not found: {class_name}",
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.post("/workspaces/{workspace_id}/classes", status_code=status.HTTP_201_CREATED)
async def create_class_endpoint(
    workspace_id: str,
    payload: ClassCreate,
) -> dict[str, Any]:
    """Create or update a class definition."""
    _validate_path_id(workspace_id, "workspace_id")
    _validate_path_id(payload.name, "class_name")
    storage_config = _storage_config()
    await _ensure_workspace_exists(storage_config, workspace_id)

    try:
        # Separate strategies from persistent class definition
        class_data = payload.model_dump()
        strategies = class_data.pop("strategies", None)
        class_json = json.dumps(class_data)

        await ieapp_core.upsert_class(storage_config, workspace_id, class_json)

        if strategies:
            strategies_json = json.dumps(strategies)
            await ieapp_core.migrate_class(
                storage_config,
                workspace_id,
                class_json,
                strategies_json,
            )

        return await ieapp_core.get_class(storage_config, workspace_id, payload.name)
    except Exception as e:
        logger.exception("Failed to upsert class")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
