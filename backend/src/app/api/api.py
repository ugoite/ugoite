"""API router configuration."""

from fastapi import APIRouter

from app.api.endpoints import (
    asset,
    entry,
    link,
    search,
    space,
    sql,
    sql_sessions,
)
from app.api.endpoints import forms as form_endpoints

router = APIRouter()
router.include_router(space.router)
router.include_router(entry.router)
router.include_router(form_endpoints.router)
router.include_router(asset.router)
router.include_router(link.router)
router.include_router(search.router)
router.include_router(sql.router)
router.include_router(sql_sessions.router)
