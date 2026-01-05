"""Workspace endpoints."""

import json
import logging
import uuid
from typing import Annotated, Any

import ieapp
from fastapi import APIRouter, File, HTTPException, Query, UploadFile, status
from ieapp import (
    AttachmentReferencedError,
    create_link,
    delete_attachment,
    delete_link,
    get_schema,
    list_attachments,
    list_links,
    list_schemas,
    save_attachment,
    search_notes,
    upsert_schema,
)
from ieapp.indexer import extract_properties, validate_properties
from ieapp.notes import (
    NoteExistsError,
    RevisionMismatchError,
    create_note,
    delete_note,
    get_note,
    get_note_history,
    get_note_revision,
    list_notes,
    restore_note,
    update_note,
)
from ieapp.utils import validate_id
from ieapp.workspace import (
    WorkspaceExistsError,
    create_workspace,
    get_workspace,
    list_workspaces,
    patch_workspace,
    test_storage_connection,
    workspace_path,
)

from app.core.config import get_root_path
from app.models.schemas import (
    LinkCreate,
    NoteCreate,
    NoteRestore,
    NoteUpdate,
    QueryRequest,
    SchemaCreate,
    TestConnectionRequest,
    WorkspaceCreate,
    WorkspacePatch,
)

router = APIRouter()
logger = logging.getLogger(__name__)


def _format_schema_validation_errors(errors: list[dict[str, Any]]) -> str:
    """Format schema validation warnings into a single human-readable string.

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
            parts.append("Schema validation error")
    return "\n".join(parts)


def _validate_note_markdown_against_schema(ws_path: str, markdown: str) -> None:
    """Validate extracted note properties against the workspace schema."""
    properties = extract_properties(markdown)
    note_class = properties.get("class")
    if not isinstance(note_class, str) or not note_class.strip():
        return

    try:
        schema = get_schema(ws_path, note_class)
    except FileNotFoundError as e:
        raise HTTPException(
            status_code=status.HTTP_422_UNPROCESSABLE_CONTENT,
            detail=f"Schema not found: {note_class}",
        ) from e
    except json.JSONDecodeError as e:
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail="Invalid schema file",
        ) from e

    _casted, warnings = validate_properties(properties, schema)
    if warnings:
        raise HTTPException(
            status_code=status.HTTP_422_UNPROCESSABLE_CONTENT,
            detail=_format_schema_validation_errors(warnings),
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


def _get_workspace_path(workspace_id: str) -> str:
    """Get a safe workspace path string after validation."""
    root_path = get_root_path()
    try:
        return workspace_path(root_path, workspace_id, must_exist=True)
    except ValueError as e:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=f"Invalid workspace_id: {workspace_id}",
        ) from e
    except FileNotFoundError as e:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Workspace not found: {workspace_id}",
        ) from e


@router.get("/workspaces")
async def list_workspaces_endpoint() -> list[dict[str, Any]]:
    """List all workspaces."""
    root_path = get_root_path()

    try:
        return list_workspaces(root_path)
    except Exception as e:
        logger.exception("Failed to list workspaces")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.post("/workspaces", status_code=status.HTTP_201_CREATED)
async def create_workspace_endpoint(
    payload: WorkspaceCreate,
) -> dict[str, str]:
    """Create a new workspace."""
    root_path = get_root_path()
    workspace_id = payload.name  # Using name as ID for now per simple spec

    try:
        create_workspace(root_path, workspace_id)
    except WorkspaceExistsError as e:
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
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
        "path": workspace_path(root_path, workspace_id, must_exist=True),
    }


@router.get("/workspaces/{workspace_id}")
async def get_workspace_endpoint(workspace_id: str) -> dict[str, Any]:
    """Get workspace metadata."""
    _validate_path_id(workspace_id, "workspace_id")
    root_path = get_root_path()

    try:
        return get_workspace(root_path, workspace_id)
    except FileNotFoundError as e:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
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
    root_path = get_root_path()

    # Build patch dict from payload
    patch_data = {}
    if payload.name is not None:
        patch_data["name"] = payload.name
    if payload.storage_config is not None:
        patch_data["storage_config"] = payload.storage_config
    if payload.settings is not None:
        patch_data["settings"] = payload.settings

    try:
        return patch_workspace(
            root_path,
            workspace_id,
            patch=patch_data,
        )
    except FileNotFoundError as e:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
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
) -> dict[str, str]:
    """Validate the provided storage connector (stubbed for Milestone 6)."""
    _validate_path_id(workspace_id, "workspace_id")
    _get_workspace_path(workspace_id)

    try:
        return test_storage_connection(payload.storage_config)
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
) -> dict[str, str]:
    """Create a new note."""
    _validate_path_id(workspace_id, "workspace_id")
    ws_path = _get_workspace_path(workspace_id)

    note_id = payload.id or str(uuid.uuid4())

    try:
        create_note(ws_path, note_id, payload.content)
        # Get the created note to retrieve revision_id
        note_data = get_note(ws_path, note_id)
    except NoteExistsError as e:
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
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
    ws_path = _get_workspace_path(workspace_id)

    try:
        return list_notes(ws_path)
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
    ws_path = _get_workspace_path(workspace_id)

    try:
        return get_note(ws_path, note_id)
    except FileNotFoundError as e:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
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
    ws_path = _get_workspace_path(workspace_id)

    try:
        _validate_note_markdown_against_schema(ws_path, payload.markdown)
        update_note(
            ws_path,
            note_id,
            payload.markdown,
            payload.parent_revision_id,
            attachments=payload.attachments,
        )
        # Return the updated note with id and revision_id
        updated_note = get_note(ws_path, note_id)
        return {
            "id": note_id,
            "revision_id": updated_note.get("revision_id", ""),
        }
    except FileNotFoundError as e:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=str(e),
        ) from e
    except RevisionMismatchError as e:
        # Return 409 with the current server version for client merge.
        # FastAPI supports dict as detail value, which serializes to JSON.
        # This allows clients to perform OCC merge with the current_revision.
        try:
            current_note = get_note(ws_path, note_id)
            raise HTTPException(
                status_code=status.HTTP_409_CONFLICT,
                detail={
                    "message": str(e),
                    "current_revision": current_note,
                },
            ) from e
        except FileNotFoundError:
            raise HTTPException(
                status_code=status.HTTP_409_CONFLICT,
                detail=str(e),
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
) -> dict[str, str]:
    """Upload a binary attachment into the workspace attachments directory."""
    _validate_path_id(workspace_id, "workspace_id")
    ws_path = _get_workspace_path(workspace_id)

    contents = await file.read()

    try:
        return save_attachment(ws_path, contents, file.filename or "")
    except Exception as e:
        logger.exception("Failed to save attachment")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/workspaces/{workspace_id}/attachments")
async def list_attachments_endpoint(
    workspace_id: str,
) -> list[dict[str, str]]:
    """List all attachments in the workspace."""
    _validate_path_id(workspace_id, "workspace_id")
    ws_path = _get_workspace_path(workspace_id)

    try:
        return list_attachments(ws_path)
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
    ws_path = _get_workspace_path(workspace_id)

    try:
        delete_attachment(ws_path, attachment_id)
    except AttachmentReferencedError as e:
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail=str(e),
        ) from e
    except FileNotFoundError as e:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Not found",
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
    ws_path = _get_workspace_path(workspace_id)

    try:
        delete_note(ws_path, note_id)
    except FileNotFoundError as e:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
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
    ws_path = _get_workspace_path(workspace_id)

    try:
        return get_note_history(ws_path, note_id)
    except FileNotFoundError as e:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
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
    ws_path = _get_workspace_path(workspace_id)

    try:
        return get_note_revision(ws_path, note_id, revision_id)
    except FileNotFoundError as e:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
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
    ws_path = _get_workspace_path(workspace_id)

    try:
        return restore_note(ws_path, note_id, payload.revision_id)
    except FileNotFoundError as e:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=str(e),
        ) from e
    except Exception as e:
        logger.exception("Failed to restore note")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


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
    ws_path = _get_workspace_path(workspace_id)

    link_id = uuid.uuid4().hex

    try:
        return create_link(
            ws_path,
            source=payload.source,
            target=payload.target,
            kind=payload.kind,
            link_id=link_id,
        )
    except FileNotFoundError as e:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
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
    ws_path = _get_workspace_path(workspace_id)

    try:
        return list_links(ws_path)
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
    ws_path = _get_workspace_path(workspace_id)

    try:
        delete_link(ws_path, link_id)
    except FileNotFoundError as e:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Link not found",
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
    ws_path = _get_workspace_path(workspace_id)

    try:
        # ieapp.query expects workspace_path as string or Path
        return ieapp.query(str(ws_path), payload.filter)
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
    ws_path = _get_workspace_path(workspace_id)

    try:
        return search_notes(ws_path, q)
    except Exception as e:
        logger.exception("Search failed")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/workspaces/{workspace_id}/schemas")
async def list_schemas_endpoint(workspace_id: str) -> list[dict[str, Any]]:
    """List all schemas in the workspace."""
    _validate_path_id(workspace_id, "workspace_id")
    ws_path = _get_workspace_path(workspace_id)

    try:
        return list_schemas(ws_path)
    except Exception as e:
        logger.exception("Failed to list schemas")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/workspaces/{workspace_id}/schemas/{class_name}")
async def get_schema_endpoint(workspace_id: str, class_name: str) -> dict[str, Any]:
    """Get a specific schema definition."""
    _validate_path_id(workspace_id, "workspace_id")
    _validate_path_id(class_name, "class_name")
    ws_path = _get_workspace_path(workspace_id)

    try:
        return get_schema(ws_path, class_name)
    except FileNotFoundError as e:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Schema not found: {class_name}",
        ) from e
    except json.JSONDecodeError as e:
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail="Invalid schema file",
        ) from e


@router.post("/workspaces/{workspace_id}/schemas", status_code=status.HTTP_201_CREATED)
async def create_schema_endpoint(
    workspace_id: str,
    payload: SchemaCreate,
) -> dict[str, Any]:
    """Create or update a schema definition."""
    _validate_path_id(workspace_id, "workspace_id")
    _validate_path_id(payload.name, "schema_name")
    ws_path = _get_workspace_path(workspace_id)

    try:
        return upsert_schema(ws_path, payload.model_dump())
    except Exception as e:
        logger.exception("Failed to upsert schema")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
