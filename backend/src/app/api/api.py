"""API router configuration."""

from fastapi import APIRouter

from app.api.endpoints import workspaces

router = APIRouter()
router.include_router(workspaces.router)
