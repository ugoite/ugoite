"""Main application module."""

import logging
import os

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

from app.api.api import router as api_router
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


@app.get("/")
async def root() -> dict[str, str]:
    """Root endpoint."""
    return {"message": "Hello World!"}


app.include_router(api_router)
