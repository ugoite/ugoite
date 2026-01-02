"""Main application module."""

import logging
import os
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pathlib import Path

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from ieapp.workspace import WorkspaceExistsError, create_workspace

from app.api.api import router as api_router
from app.core.config import get_root_path
from app.core.middleware import security_middleware
from app.mcp.server import mcp

# Configure logging
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

app = FastAPI()

# Mount MCP Server (SSE)
app.mount("/mcp", mcp.sse_app())

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

app.middleware("http")(security_middleware)


@app.on_event("startup")
async def ensure_default_workspace() -> None:
    """Create the default workspace if it doesn't exist."""
    root_path: Path = get_root_path()
    try:
        create_workspace(root_path, "default")
        logger.info("Created default workspace at startup")
    except WorkspaceExistsError:
        logger.info("Default workspace already exists")
    except (
        OSError,
        RuntimeError,
        ValueError,
    ) as exc:  # pragma: no cover - best effort guard
        logger.warning("Failed to ensure default workspace: %s", exc)


@app.get("/")
async def root() -> dict[str, str]:
    """Root endpoint."""
    return {"message": "Hello World!"}


@app.get("/health")
async def health() -> dict[str, str]:
    """Health check endpoint for CI/CD and monitoring."""
    return {"status": "healthy"}


app.include_router(api_router)
