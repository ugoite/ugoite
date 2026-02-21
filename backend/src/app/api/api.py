"""API router configuration."""

from fastapi import APIRouter, Depends

from app.api.endpoints import (
    asset,
    audit,
    entry,
    members,
    search,
    space,
    sql,
    sql_sessions,
)
from app.api.endpoints import forms as form_endpoints
from app.core.auth import require_authenticated_identity

router = APIRouter(dependencies=[Depends(require_authenticated_identity)])
router.include_router(space.router)
router.include_router(members.router)
router.include_router(entry.router)
router.include_router(form_endpoints.router)
router.include_router(asset.router)
router.include_router(audit.router)
router.include_router(search.router)
router.include_router(sql.router)
router.include_router(sql_sessions.router)
