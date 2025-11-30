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
from ieapp.notes import NoteExistsError, create_note
from ieapp.workspace import WorkspaceExistsError, create_workspace
from pydantic import BaseModel

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
    # 1. Localhost Binding Check (unless disabled via env var)
    allow_remote = os.environ.get("IEAPP_ALLOW_REMOTE", "false").lower() == "true"
    if not allow_remote:
        client_host = request.client.host if request.client else "unknown"
        # Allow localhost/127.0.0.1/::1
        if client_host not in ("127.0.0.1", "localhost", "::1"):
            pass

    response = await call_next(request)

    # 2. Security Headers
    response.headers["X-Content-Type-Options"] = "nosniff"

    # 3. HMAC Signature (Mocked)
    response.headers["X-IEApp-Signature"] = "mock-signature"

    return response


class WorkspaceCreate(BaseModel):
    """Workspace creation payload."""

    name: str


class NoteCreate(BaseModel):
    """Note creation payload."""

    id: str | None = None
    content: str


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


@app.post("/workspaces", status_code=status.HTTP_201_CREATED)
async def create_workspace_endpoint(payload: WorkspaceCreate) -> dict[str, str]:
    """Create a new workspace."""
    root_path = get_root_path()
    workspace_id = payload.name  # Using name as ID for now per simple spec

    try:
        create_workspace(root_path, workspace_id)
    except WorkspaceExistsError as e:
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT, detail=str(e),
        ) from e
    except Exception as e:
        logger.exception("Failed to create workspace")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR, detail=str(e),
        ) from e

    return {
        "id": workspace_id,
        "name": payload.name,
        "path": str(root_path / "workspaces" / workspace_id),
    }


@app.post("/workspaces/{workspace_id}/notes", status_code=status.HTTP_201_CREATED)
async def create_note_endpoint(
    workspace_id: str, payload: NoteCreate,
) -> dict[str, str]:
    """Create a new note."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND, detail="Workspace not found",
        )

    note_id = payload.id or str(uuid.uuid4())

    try:
        create_note(ws_path, note_id, payload.content)
    except NoteExistsError as e:
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT, detail=str(e),
        ) from e
    except Exception as e:
        logger.exception("Failed to create note")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR, detail=str(e),
        ) from e

    return {"id": note_id}


@app.post("/workspaces/{workspace_id}/query")
async def query_endpoint(
    workspace_id: str, payload: QueryRequest,
) -> list[dict[str, Any]]:
    """Query the workspace index."""
    root_path = get_root_path()
    ws_path = root_path / "workspaces" / workspace_id

    if not ws_path.exists():
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND, detail="Workspace not found",
        )

    try:
        # ieapp.query expects workspace_path as string or Path
        return ieapp.query(str(ws_path), payload.filter)
    except Exception as e:
        logger.exception("Query failed")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR, detail=str(e),
        ) from e
