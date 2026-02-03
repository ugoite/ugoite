"""API router configuration."""

from fastapi import APIRouter

from app.api.endpoints import (
    attachment,
    link,
    note,
    search,
    sql,
    sql_sessions,
    workspace,
)
from app.api.endpoints import classes as class_endpoints

router = APIRouter()
router.include_router(workspace.router)
router.include_router(note.router)
router.include_router(class_endpoints.router)
router.include_router(attachment.router)
router.include_router(link.router)
router.include_router(search.router)
router.include_router(sql.router)
router.include_router(sql_sessions.router)
