"""Main application module."""

import logging
import os
from collections.abc import AsyncGenerator
from contextlib import asynccontextmanager
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pathlib import Path

import ugoite_core
from fastapi import FastAPI, HTTPException, Request, status
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import JSONResponse

from app.api.api import router as api_router
from app.core.config import get_root_path
from app.core.middleware import security_middleware
from app.core.storage import storage_config_from_root
from app.mcp.server import mcp

# Configure logging
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

ALLOWED_CORS_METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"]
ALLOWED_CORS_HEADERS = [
    "Authorization",
    "Content-Type",
    "X-API-Key",
    "X-Request-Id",
    "X-Ugoite-Dev-Auth-Proxy-Token",
]


def _cors_allowed_origins() -> list[str]:
    raw_origins = os.environ.get("ALLOW_ORIGIN") or "http://localhost:3000"
    return [origin.strip() for origin in raw_origins.split(",") if origin.strip()]


def _validate_cors_configuration(allowed_origins: list[str]) -> None:
    if (
        os.environ.get("UGOITE_ALLOW_REMOTE", "false").lower() == "true"
        and "*" in allowed_origins
    ):
        message = (
            "ALLOW_ORIGIN must not include '*' when UGOITE_ALLOW_REMOTE=true and "
            "credentialed CORS is enabled."
        )
        raise RuntimeError(message)


@asynccontextmanager
async def lifespan(_app: FastAPI) -> AsyncGenerator[None]:
    """Application lifespan context manager for startup/shutdown events."""
    # Startup
    _validate_cors_configuration(_cors_allowed_origins())
    root_path: Path | str = get_root_path()
    storage_config = storage_config_from_root(root_path)
    dev_auth_mode = os.environ.get("UGOITE_DEV_AUTH_MODE", "").strip()
    dev_user_id = os.environ.get("UGOITE_DEV_USER_ID", "").strip()
    if dev_auth_mode in {"manual-totp", "mock-oauth"} and dev_user_id:
        try:
            await ugoite_core.ensure_admin_space(storage_config, dev_user_id)
            logger.info("Created or updated admin-space bootstrap for %s", dev_user_id)
        except (OSError, ValueError, RuntimeError) as exc:  # pragma: no cover
            logger.warning("Failed to ensure admin-space bootstrap: %s", exc)

    should_bootstrap_default_space = (
        os.environ.get("UGOITE_BOOTSTRAP_DEFAULT_SPACE", "false").lower() == "true"
    )
    if should_bootstrap_default_space:
        try:
            await ugoite_core.create_space(storage_config, "default")
            logger.info("Created default space at startup")
        except RuntimeError as exc:  # pragma: no cover - best effort guard
            if "already exists" in str(exc).lower():
                logger.info("Default space already exists")
            else:
                logger.warning("Failed to ensure default space: %s", exc)
        except (OSError, ValueError) as exc:  # pragma: no cover - best effort guard
            logger.warning("Failed to ensure default space: %s", exc)

    yield
    # Shutdown (if needed)


app = FastAPI(lifespan=lifespan)

# Mount MCP Server (SSE)
app.mount("/mcp", mcp.sse_app())

# Allow CORS for frontend development
app.add_middleware(
    CORSMiddleware,  # ty: ignore[invalid-argument-type]
    # ALLOW_ORIGIN (comma-separated) or fallback to localhost:3000 in development
    allow_origins=_cors_allowed_origins(),
    allow_credentials=True,
    allow_methods=ALLOWED_CORS_METHODS,
    allow_headers=ALLOWED_CORS_HEADERS,
)

app.middleware("http")(security_middleware)


@app.exception_handler(HTTPException)
async def handle_http_exception(_request: Request, exc: HTTPException) -> JSONResponse:
    """Sanitize server-side HTTP error details before returning to clients."""
    detail = exc.detail
    if (
        exc.status_code >= status.HTTP_500_INTERNAL_SERVER_ERROR
        and isinstance(detail, str)
        and not detail.startswith("Failed to ")
    ):
        detail = {
            "code": "internal_error",
            "message": "Internal server error",
        }
    return JSONResponse(
        status_code=exc.status_code,
        content={"detail": detail},
        headers=exc.headers,
    )


@app.get("/")
async def root() -> dict[str, str]:
    """Root endpoint."""
    return {"message": "Hello World!"}


@app.get("/health")
async def health() -> dict[str, str]:
    """Healthcheck endpoint."""
    return {"status": "ok"}


app.include_router(api_router)
