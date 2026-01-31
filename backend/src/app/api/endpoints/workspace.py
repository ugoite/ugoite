"""Workspace endpoints."""

import json
import logging
from typing import Any

import ieapp_core
from fastapi import APIRouter, HTTPException, status

from app.core.config import get_root_path
from app.core.ids import validate_id
from app.core.storage import storage_config_from_root, workspace_uri
from app.models.classes import TestConnectionRequest, WorkspaceCreate, WorkspacePatch

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
    storage_config = _storage_config()
    try:
        workspace_ids = await ieapp_core.list_workspaces(storage_config)
    except RuntimeError as exc:
        logger.warning("Failed to list workspaces, returning empty list: %s", exc)
        return []
    except Exception as exc:
        logger.exception("Failed to list workspaces")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(exc),
        ) from exc

    results: list[dict[str, Any]] = []
    for ws_id in workspace_ids:
        try:
            results.append(await ieapp_core.get_workspace(storage_config, ws_id))
        except Exception:
            logger.exception("Failed to read workspace meta %s", ws_id)

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
