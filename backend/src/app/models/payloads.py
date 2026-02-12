"""Pydantic models for the application."""

from typing import Any, Literal

from pydantic import BaseModel, Field


class SpaceCreate(BaseModel):
    """Space creation payload."""

    name: str


class SampleSpaceCreate(BaseModel):
    """Sample data space creation payload."""

    space_id: str
    scenario: str = "renewable-ops"
    entry_count: int = Field(default=5000, ge=100, le=20000)
    seed: int | None = Field(default=None, ge=0)


class EntryCreate(BaseModel):
    """Entry creation payload."""

    id: str | None = None
    content: str


class EntryUpdate(BaseModel):
    """Entry update payload.

    Entry: frontmatter and canvas_position fields are accepted but not yet
    processed. Properties should be updated via markdown headers (e.g., ## Date).
    Full support for these fields is planned for future milestones.
    """

    markdown: str
    parent_revision_id: str
    frontmatter: dict[str, Any] | None = None
    canvas_position: dict[str, Any] | None = None
    assets: list[dict[str, Any]] | None = None


class EntryRestore(BaseModel):
    """Entry restore payload."""

    revision_id: str


class QueryRequest(BaseModel):
    """Query request payload."""

    filter: dict[str, Any]


class SqlSessionCreate(BaseModel):
    """SQL session creation payload."""

    sql: str


class SqlVariable(BaseModel):
    """SQL variable definition."""

    type: str
    name: str
    description: str


class SqlCreate(BaseModel):
    """Saved SQL creation payload."""

    id: str | None = None
    name: str
    sql: str
    variables: list[SqlVariable]


class SqlUpdate(BaseModel):
    """Saved SQL update payload."""

    name: str
    sql: str
    variables: list[SqlVariable]
    parent_revision_id: str | None = None


class SpacePatch(BaseModel):
    """Space patch payload for storage connectors/settings."""

    name: str | None = None
    storage_config: dict[str, Any] | None = None
    settings: dict[str, Any] | None = None


class SpaceConnectionRequest(BaseModel):
    """Space connection validation payload."""

    storage_config: dict[str, Any]


class FormCreate(BaseModel):
    """Form creation payload."""

    name: str
    version: int = 1
    template: str
    fields: dict[str, dict[str, Any]]
    allow_extra_attributes: Literal["deny", "allow_json", "allow_columns"] = "deny"
    defaults: dict[str, Any] | None = None
    strategies: dict[str, Any] | None = None
