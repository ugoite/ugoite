"""Main application module."""

import logging
import os
import uuid
from collections.abc import Awaitable, Callable
from pathlib import Path
from typing import Any

import ieapp
from fastapi import FastAPI, HTTPException, Request, Response, status
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import JSONResponse
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
from ieapp.workspace import (
    WorkspaceExistsError,
    create_workspace,
    get_workspace,
    list_workspaces,
)
from pydantic import BaseModel
from starlette.concurrency import iterate_in_threadpool

from security import build_response_signature, is_local_host, resolve_client_host

# Configure logging
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

app = FastAPI()

# Allow CORS for frontend development
app.add_middleware(
    CORSMiddleware,  # type: ignore[arg-type]
    # ALLOW_ORIGIN (comma-separated) or fallback to localhost:3000 in development
    allow_origins=(os.environ.get("ALLOW_ORIGIN") or "http://localhost:3000").split(
        ",",
    ),
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)


@app.middleware("http")
async def security_middleware(
    request: Request,
    call_next: Callable[[Request], Awaitable[Response]],
) -> Response:
    """Enforce security policies."""
    root_path = get_root_path()
    # 1. Localhost Binding Check (unless disabled via env var)
    allow_remote = os.environ.get("IEAPP_ALLOW_REMOTE", "false").lower() == "true"
    client_host = resolve_client_host(
        request.headers,
        request.client.host if request.client else None,
    )

    if not allow_remote and not is_local_host(client_host):
        logger.warning("Blocking remote request from %s", client_host)
        response = JSONResponse(
            status_code=status.HTTP_403_FORBIDDEN,
            content={
                "detail": (
                    "Remote access is disabled. Set IEAPP_ALLOW_REMOTE=true only on"
                    " trusted networks."
                ),
            },
        )
        body = bytes(response.body or b"")
        return _apply_security_headers(response, body, root_path)

    response = await call_next(request)
    body = await _capture_response_body(response)
    return _apply_security_headers(response, body, root_path)


async def _capture_response_body(response: Response) -> bytes:
    """Consume the response iterator so it can be signed and replayed."""
    body = b""
    body_iterator = getattr(response, "body_iterator", None)

    if body_iterator is None:
        return bytes(response.body or b"")

    async for chunk in body_iterator:
        body += chunk

    response.body_iterator = iterate_in_threadpool(iter([body]))  # type: ignore[attr-defined]
    return body


def _apply_security_headers(
    response: Response,
    body: bytes,
    root_path: Path,
) -> Response:
    """Attach security-related headers including the HMAC signature."""
    key_id, signature = build_response_signature(body, root_path)
    response.headers["X-Content-Type-Options"] = "nosniff"
    response.headers["X-IEApp-Key-Id"] = key_id
    response.headers["X-IEApp-Signature"] = signature
    response.headers["Content-Length"] = str(len(body))
    return response


class WorkspaceCreate(BaseModel):
    """Workspace creation payload."""

    name: str


class NoteCreate(BaseModel):
    """Note creation payload."""

    id: str | None = None
    content: str


class NoteUpdate(BaseModel):
    """Note update payload."""

    markdown: str
    parent_revision_id: str
    frontmatter: dict[str, Any] | None = None
    canvas_position: dict[str, Any] | None = None


class NoteRestore(BaseModel):
    """Note restore payload."""

    revision_id: str


class QueryRequest(BaseModel):
    """Query request payload."""

    filter: dict[str, Any]


def get_root_path() -> Path:
    """Get the root path for workspaces."""
    return Path(os.environ.get("IEAPP_ROOT", str(Path.cwd())))


@app.get("/")
async def root() -> dict[str, str]:
    """Root endpoint."""
    return {"message": "Hello World!"}


# ==============================================================================
# Workspace Endpoints
# ==============================================================================


@app.get("/workspaces")
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


@app.post("/workspaces", status_code=status.HTTP_201_CREATED)
async def create_workspace_endpoint(payload: WorkspaceCreate) -> dict[str, str]:
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
        "path": str(root_path / "workspaces" / workspace_id),
    }


@app.post("/workspaces/{workspace_id}/notes", status_code=status.HTTP_201_CREATED)
async def create_note_endpoint(
    workspace_id: str,
    payload: NoteCreate,
) -> dict[str, str]:
    """Create a new note."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Workspace not found",
        )

    note_id = payload.id or str(uuid.uuid4())

    try:
        create_note(ws_path, note_id, payload.content)
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

    return {"id": note_id}


# ==============================================================================
# Notes Endpoints (continued)
# ==============================================================================


@app.get("/workspaces/{workspace_id}")
async def get_workspace_endpoint(workspace_id: str) -> dict[str, Any]:
    """Get workspace metadata."""
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


@app.get("/workspaces/{workspace_id}/notes")
async def list_notes_endpoint(workspace_id: str) -> list[dict[str, Any]]:
    """List all notes in a workspace."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

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


@app.get("/workspaces/{workspace_id}/notes/{note_id}")
async def get_note_endpoint(workspace_id: str, note_id: str) -> dict[str, Any]:
    """Get a note by ID."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

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


@app.put("/workspaces/{workspace_id}/notes/{note_id}")
async def update_note_endpoint(
    workspace_id: str,
    note_id: str,
    payload: NoteUpdate,
) -> dict[str, Any]:
    """Update an existing note.

    Requires parent_revision_id for optimistic concurrency control.
    Returns 409 Conflict if the revision has changed.
    """
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Workspace not found",
        )

    try:
        update_note(ws_path, note_id, payload.markdown, payload.parent_revision_id)
        # Return the updated note
        return get_note(ws_path, note_id)
    except FileNotFoundError as e:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=str(e),
        ) from e
    except RevisionMismatchError as e:
        # Return 409 with the current server version for client merge
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


@app.delete("/workspaces/{workspace_id}/notes/{note_id}")
async def delete_note_endpoint(workspace_id: str, note_id: str) -> dict[str, str]:
    """Tombstone (soft delete) a note."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

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


# ==============================================================================
# History Endpoints
# ==============================================================================


@app.get("/workspaces/{workspace_id}/notes/{note_id}/history")
async def get_note_history_endpoint(
    workspace_id: str,
    note_id: str,
) -> dict[str, Any]:
    """Get the revision history for a note."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

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


@app.get("/workspaces/{workspace_id}/notes/{note_id}/history/{revision_id}")
async def get_note_revision_endpoint(
    workspace_id: str,
    note_id: str,
    revision_id: str,
) -> dict[str, Any]:
    """Get a specific revision of a note."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

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


@app.post("/workspaces/{workspace_id}/notes/{note_id}/restore")
async def restore_note_endpoint(
    workspace_id: str,
    note_id: str,
    payload: NoteRestore,
) -> dict[str, Any]:
    """Restore a note to a previous revision."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

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


# ==============================================================================
# Query Endpoint
# ==============================================================================


@app.post("/workspaces/{workspace_id}/query")
async def query_endpoint(
    workspace_id: str,
    payload: QueryRequest,
) -> list[dict[str, Any]]:
    """Query the workspace index."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

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
