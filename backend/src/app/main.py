"""Main application module."""

import contextlib
import logging
import os
from collections.abc import AsyncIterator

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from starlette.routing import Mount

from app.api.api import router as api_router
from app.core.middleware import security_middleware
from app.mcp import get_mcp_app

# Configure logging
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)


@contextlib.asynccontextmanager
async def lifespan(app: FastAPI) -> AsyncIterator[None]:  # noqa: ARG001
    """Manage application lifecycle including MCP server."""
    mcp = get_mcp_app()
    async with mcp.session_manager.run():
        yield


app = FastAPI(lifespan=lifespan)

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
    # MCP streamable HTTP needs this header exposed
    expose_headers=["Mcp-Session-Id"],
)

app.middleware("http")(security_middleware)


@app.get("/")
async def root() -> dict[str, str]:
    """Root endpoint."""
    return {"message": "Hello World!"}


app.include_router(api_router)

# Mount MCP server at /mcp
mcp_instance = get_mcp_app()
app.router.routes.append(
    Mount("/mcp", app=mcp_instance.streamable_http_app()),
)
