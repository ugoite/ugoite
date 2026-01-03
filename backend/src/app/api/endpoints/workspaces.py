"""Workspace endpoints."""

import json
import logging
import re
import uuid
from pathlib import Path
from typing import Annotated, Any

import ieapp
from fastapi import APIRouter, File, HTTPException, Query, UploadFile, status
from ieapp.indexer import Indexer
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
from ieapp.utils import resolve_existing_path
from ieapp.workspace import (
    WorkspaceExistsError,
    create_workspace,
    get_workspace,
    list_workspaces,
)

from app.core.config import get_root_path
from app.models.schemas import (
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

# Pattern for valid IDs: alphanumeric, hyphens, underscores
_SAFE_ID_PATTERN = re.compile(r"^[a-zA-Z0-9_-]+$")


def _validate_path_id(identifier: str, name: str) -> None:
    """Validate identifier to prevent path traversal attacks.

    Args:
        identifier: The ID to validate.
        name: Name of the parameter (for error messages).

    Raises:
        HTTPException: If the identifier is invalid.

    """
    if not identifier or not _SAFE_ID_PATTERN.match(identifier):
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=f"Invalid {name}: must be alphanumeric, hyphens, or underscores",
        )


def _get_workspace_path(workspace_id: str) -> Path:
    """Get a safe workspace path after validation.

    Args:
        workspace_id: The workspace identifier (must be pre-validated).

    Returns:
        Path to the workspace directory.

    Raises:
        HTTPException: If path resolution fails due to traversal attempt.

    """
    root_path = get_root_path()
    try:
        return resolve_existing_path(root_path, "workspaces", workspace_id)
    except ValueError as e:
        # Input validation error -> 400 Bad Request
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=f"Invalid workspace_id: {workspace_id}",
        ) from e
    except (FileNotFoundError, NotADirectoryError) as e:
        # Missing workspace or path components -> 404 Not Found
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
        "path": str(resolve_existing_path(root_path, "workspaces", workspace_id)),
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
    ws_path = _get_workspace_path(workspace_id)

    meta_path = ws_path / "meta.json"
    settings_path = ws_path / "settings.json"

    if not meta_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Workspace not found",
        )

    meta = json.loads(meta_path.read_text())
    settings = json.loads(settings_path.read_text()) if settings_path.exists() else {}

    if payload.name:
        meta["name"] = payload.name

    if payload.storage_config:
        meta["storage_config"] = payload.storage_config

    if payload.settings:
        settings.update(payload.settings)

    meta_path.write_text(json.dumps(meta, indent=2))
    settings_path.write_text(json.dumps(settings, indent=2))

    return {**meta, "settings": settings}


@router.post("/workspaces/{workspace_id}/test-connection")
async def test_connection_endpoint(
    workspace_id: str,
    payload: TestConnectionRequest,
) -> dict[str, str]:
    """Validate the provided storage connector (stubbed for Milestone 6)."""
    _validate_path_id(workspace_id, "workspace_id")
    uri = payload.storage_config.get("uri", "")

    if uri.startswith(("file://", "/")):
        return {"status": "ok", "mode": "local"}

    if uri.startswith("s3://"):
        return {"status": "ok", "mode": "s3"}

    raise HTTPException(
        status_code=status.HTTP_400_BAD_REQUEST,
        detail="Unsupported storage connector",
    )


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

    if not ws_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Workspace not found",
        )

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

    if not ws_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Workspace not found",
        )

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

    if not ws_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Workspace not found",
        )

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

    if not ws_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Workspace not found",
        )

    try:
        update_note(
            ws_path,
            note_id,
            payload.markdown,
            payload.parent_revision_id,
        )
        if payload.attachments is not None:
            content_path = (
                _get_workspace_path(workspace_id) / "notes" / note_id / "content.json"
            )
            content_data = json.loads(content_path.read_text())
            content_data["attachments"] = payload.attachments
            content_path.write_text(json.dumps(content_data, indent=2))
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

    attachments_dir = ws_path / "attachments"
    attachments_dir.mkdir(exist_ok=True)

    attachment_id = uuid.uuid4().hex
    filename = file.filename or attachment_id
    relative_path = f"attachments/{attachment_id}_{filename}"
    destination = attachments_dir / f"{attachment_id}_{filename}"

    contents = await file.read()
    destination.write_bytes(contents)

    return {"id": attachment_id, "name": filename, "path": relative_path}


@router.get("/workspaces/{workspace_id}/attachments")
async def list_attachments_endpoint(
    workspace_id: str,
) -> list[dict[str, str]]:
    """List all attachments in the workspace."""
    _validate_path_id(workspace_id, "workspace_id")
    ws_path = _get_workspace_path(workspace_id)

    attachments_dir = ws_path / "attachments"
    if not attachments_dir.exists():
        return []

    attachments = []
    for file_path in attachments_dir.iterdir():
        if file_path.is_file():
            # Parse filename: {attachment_id}_{original_filename}
            filename = file_path.name
            if "_" in filename:
                attachment_id, name = filename.split("_", 1)
                relative_path = f"attachments/{filename}"
                attachments.append(
                    {
                        "id": attachment_id,
                        "name": name,
                        "path": relative_path,
                    },
                )

    return attachments


@router.delete("/workspaces/{workspace_id}/attachments/{attachment_id}")
async def delete_attachment_endpoint(
    workspace_id: str,
    attachment_id: str,
) -> dict[str, str]:
    """Delete an attachment if it is not referenced by any note."""
    _validate_path_id(workspace_id, "workspace_id")
    _validate_path_id(attachment_id, "attachment_id")
    ws_path = _get_workspace_path(workspace_id)

    # Detect references in note content attachments arrays
    notes_dir = ws_path / "notes"
    if notes_dir.exists():
        for note_dir in notes_dir.iterdir():
            content_path = note_dir / "content.json"
            if not content_path.exists():
                continue
            try:
                content_data = json.loads(content_path.read_text())
            except json.JSONDecodeError:
                continue
            for attachment in content_data.get("attachments", []):
                if attachment.get("id") == attachment_id:
                    raise HTTPException(
                        status_code=status.HTTP_409_CONFLICT,
                        detail="Attachment is referenced by a note",
                    )

    attachments_dir = ws_path / "attachments"
    if not attachments_dir.exists():
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Not found")

    deleted = False
    for path in attachments_dir.glob(f"{attachment_id}_*"):
        path.unlink(missing_ok=True)
        deleted = True

    if not deleted:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Not found")

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

    if not ws_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Workspace not found",
        )

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

    if not ws_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Workspace not found",
        )

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

    if not ws_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Workspace not found",
        )

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

    if not ws_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Workspace not found",
        )

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
    link = {
        "id": link_id,
        "source": payload.source,
        "target": payload.target,
        "kind": payload.kind,
    }

    for note_id in (payload.source, payload.target):
        note_meta_path = ws_path / "notes" / note_id / "meta.json"
        if not note_meta_path.exists():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Note not found: {note_id}",
            )
        meta = json.loads(note_meta_path.read_text())
        links = meta.get("links", [])
        # Avoid duplicate links by id or target
        if not any(
            item.get("target") == link["target"]
            and item.get("source") == link["source"]
            for item in links
        ):
            links.append(
                link
                if note_id == payload.source
                else {
                    **link,
                    "source": payload.target,
                    "target": payload.source,
                },
            )
            meta["links"] = links
            note_meta_path.write_text(json.dumps(meta, indent=2))

    return link


@router.get("/workspaces/{workspace_id}/links")
async def list_links_endpoint(workspace_id: str) -> list[dict[str, Any]]:
    """List all unique links in the workspace."""
    _validate_path_id(workspace_id, "workspace_id")
    ws_path = _get_workspace_path(workspace_id)

    links_by_id: dict[str, dict[str, Any]] = {}
    notes_dir = ws_path / "notes"
    if notes_dir.exists():
        for meta_path in notes_dir.glob("*/meta.json"):
            try:
                meta = json.loads(meta_path.read_text())
            except json.JSONDecodeError:
                continue
            for link in meta.get("links", []):
                links_by_id.setdefault(link.get("id", uuid.uuid4().hex), link)

    return list(links_by_id.values())


@router.delete("/workspaces/{workspace_id}/links/{link_id}")
async def delete_link_endpoint(
    workspace_id: str,
    link_id: str,
) -> dict[str, str]:
    """Delete a link and remove it from both notes."""
    _validate_path_id(workspace_id, "workspace_id")
    _validate_path_id(link_id, "link_id")
    ws_path = _get_workspace_path(workspace_id)

    notes_dir = ws_path / "notes"
    found = False
    if notes_dir.exists():
        for meta_path in notes_dir.glob("*/meta.json"):
            try:
                meta = json.loads(meta_path.read_text())
            except json.JSONDecodeError:
                continue
            original_len = len(meta.get("links", []))
            meta["links"] = [
                item for item in meta.get("links", []) if item.get("id") != link_id
            ]
            if len(meta.get("links", [])) != original_len:
                found = True
                meta_path.write_text(json.dumps(meta, indent=2))

    if not found:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Link not found",
        )

    return {"status": "deleted", "id": link_id}


@router.post("/workspaces/{workspace_id}/query")
async def query_endpoint(
    workspace_id: str,
    payload: QueryRequest,
) -> list[dict[str, Any]]:
    """Query the workspace index."""
    _validate_path_id(workspace_id, "workspace_id")
    ws_path = _get_workspace_path(workspace_id)

    if not ws_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Workspace not found",
        )

    try:
        # ieapp.query expects workspace_path as string or Path
        return ieapp.query(str(ws_path), payload.filter)
    except Exception as e:
        logger.exception("Query failed")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


def _load_notes_map(index_path: Path) -> dict[str, Any]:
    try:
        index_data = json.loads(index_path.read_text())
        return index_data.get("notes", {}) if isinstance(index_data, dict) else {}
    except (FileNotFoundError, json.JSONDecodeError):
        return {}


def _search_inverted(inverted_path: Path, token: str) -> set[str]:
    try:
        inverted = json.loads(inverted_path.read_text())
    except (FileNotFoundError, json.JSONDecodeError):
        return set()
    matches: set[str] = set()
    for term, note_ids in inverted.items():
        if token in term:
            matches.update(note_ids)
    return matches


def _search_index_records(notes_map: dict[str, Any], token: str) -> set[str]:
    matches: set[str] = set()
    for note_id, record in notes_map.items():
        haystack = json.dumps(record).lower()
        if token in haystack:
            matches.add(note_id)
    return matches


def _search_content_files(ws_path: Path, token: str) -> set[str]:
    matches: set[str] = set()
    notes_dir = ws_path / "notes"
    if not notes_dir.exists():
        return matches
    for content_path in notes_dir.glob("*/content.json"):
        try:
            content_json = json.loads(content_path.read_text())
        except json.JSONDecodeError:
            continue
        if token in json.dumps(content_json).lower():
            matches.add(content_path.parent.name)
    return matches


@router.get("/workspaces/{workspace_id}/search")
async def search_endpoint(
    workspace_id: str,
    q: Annotated[str, Query(..., min_length=1)],
) -> list[dict[str, Any]]:
    """Hybrid keyword search using inverted index with on-demand indexing."""
    _validate_path_id(workspace_id, "workspace_id")
    ws_path = _get_workspace_path(workspace_id)

    # Refresh index for deterministic tests
    Indexer(str(ws_path)).run_once()

    token = q.lower()
    inverted_path = ws_path / "index" / "inverted_index.json"
    index_path = ws_path / "index" / "index.json"

    notes_map = _load_notes_map(index_path)

    matches = _search_inverted(inverted_path, token)
    if not matches:
        matches = _search_index_records(notes_map, token)
    if not matches:
        matches = _search_content_files(ws_path, token)

    results = []
    for note_id in matches:
        if note_id in notes_map:
            note = notes_map[note_id]
            note.setdefault("id", note_id)
            results.append(note)
        else:
            results.append({"id": note_id})

    return results
