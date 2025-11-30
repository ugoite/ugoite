"""Pydantic models for the application."""

from typing import Any

from pydantic import BaseModel


class WorkspaceCreate(BaseModel):
    """Workspace creation payload."""

    name: str


class NoteCreate(BaseModel):
    """Note creation payload."""

    id: str | None = None
    content: str


class NoteUpdate(BaseModel):
    """Note update payload.

    Note: frontmatter and canvas_position fields are accepted but not yet
    processed. Properties should be updated via markdown headers (e.g., ## Date).
    Full support for these fields is planned for future milestones.
    """

    markdown: str
    parent_revision_id: str
    frontmatter: dict[str, Any] | None = None
    canvas_position: dict[str, Any] | None = None


class NoteRestore(BaseModel):
    """Note restore payload."""

    revision_id: str


class QueryRequest(BaseModel):
    """Query request payload."""

    filter: dict[str, Any]
