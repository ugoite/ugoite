"""API router configuration."""

from fastapi import APIRouter

from app.api.endpoints import workspace

router = APIRouter()
router.include_router(workspace.router)
