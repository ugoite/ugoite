"""API router configuration."""

from fastapi import APIRouter, Depends

from app.api.endpoints import (
    asset,
    auth,
    audit,
    entry,
    members,
    preferences,
    search,
    service_accounts,
    space,
    sql,
    sql_sessions,
)
from app.api.endpoints import forms as form_endpoints
from app.core.auth import require_authenticated_identity

protected_router = APIRouter(dependencies=[Depends(require_authenticated_identity)])
protected_router.include_router(space.router)
protected_router.include_router(preferences.router)
protected_router.include_router(members.router)
protected_router.include_router(service_accounts.router)
protected_router.include_router(entry.router)
protected_router.include_router(form_endpoints.router)
protected_router.include_router(asset.router)
protected_router.include_router(audit.router)
protected_router.include_router(search.router)
protected_router.include_router(sql.router)
protected_router.include_router(sql_sessions.router)

router = APIRouter()
router.include_router(auth.router)
router.include_router(protected_router)
