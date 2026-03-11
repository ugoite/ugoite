"""Pydantic models for the application."""

from typing import Annotated, Any, Literal

from pydantic import BaseModel, Field, StringConstraints

Identifier = Annotated[
    str,
    StringConstraints(
        min_length=1,
        max_length=128,
    ),
]
BoundedIdentifier = Annotated[str, StringConstraints(max_length=128)]
ShortText = Annotated[str, StringConstraints(min_length=1, max_length=256)]


class SpaceCreate(BaseModel):
    """Space creation payload."""

    name: Identifier


class EntryCreate(BaseModel):
    """Entry creation payload."""

    id: BoundedIdentifier | None = None
    content: Annotated[str, StringConstraints(min_length=1, max_length=200_000)]


class EntryUpdate(BaseModel):
    """Entry update payload.

    Entry: frontmatter and canvas_position fields are accepted but not yet
    processed. Properties should be updated via markdown headers (e.g., ## Date).
    Full support for these fields is planned for future milestones.
    """

    markdown: Annotated[str, StringConstraints(min_length=1, max_length=200_000)]
    parent_revision_id: ShortText
    frontmatter: dict[str, Any] | None = None
    canvas_position: dict[str, Any] | None = None
    assets: list[dict[str, Any]] | None = None


class EntryRestore(BaseModel):
    """Entry restore payload."""

    revision_id: ShortText


class QueryRequest(BaseModel):
    """Query request payload."""

    filter: dict[str, Any]


class SqlSessionCreate(BaseModel):
    """SQL session creation payload."""

    sql: str


class SqlVariable(BaseModel):
    """SQL variable definition."""

    type: ShortText
    name: ShortText
    description: Annotated[str, StringConstraints(min_length=1, max_length=2_000)]


class SqlCreate(BaseModel):
    """Saved SQL creation payload."""

    id: BoundedIdentifier | None = None
    name: ShortText
    sql: Annotated[str, StringConstraints(min_length=1, max_length=100_000)]
    variables: list[SqlVariable] = Field(default_factory=list)


class SqlUpdate(BaseModel):
    """Saved SQL update payload."""

    name: ShortText
    sql: Annotated[str, StringConstraints(min_length=1, max_length=100_000)]
    variables: list[SqlVariable] = Field(default_factory=list)
    parent_revision_id: ShortText | None = None


class SpacePatch(BaseModel):
    """Space patch payload for storage connectors/settings."""

    name: Identifier | None = None
    storage_config: dict[str, Any] | None = None
    settings: dict[str, Any] | None = None


class UserPreferencesFields(BaseModel):
    """Shared portable user preference fields."""

    selected_space_id: str | None = None
    locale: Literal["en", "ja"] | None = None
    ui_theme: Literal["materialize", "classic", "pop"] | None = None
    color_mode: Literal["light", "dark"] | None = None
    primary_color: Literal["violet", "blue", "emerald", "amber"] | None = None


class UserPreferences(UserPreferencesFields):
    """Portable user preference payload."""


class UserPreferencesPatch(UserPreferencesFields):
    """Portable user preference patch payload."""


class SpaceConnectionRequest(BaseModel):
    """Space connection validation payload."""

    storage_config: dict[str, Any]


class SpaceMemberInvite(BaseModel):
    """Space member invitation payload."""

    user_id: ShortText
    role: Literal["admin", "editor", "viewer"]
    email: str | None = None
    expires_in_seconds: int | None = None


class SpaceMemberAccept(BaseModel):
    """Invitation acceptance payload."""

    token: ShortText


class SpaceMemberRoleUpdate(BaseModel):
    """Space member role update payload."""

    role: Literal["admin", "editor", "viewer"]


class ServiceAccountCreate(BaseModel):
    """Service account creation payload."""

    display_name: ShortText
    scopes: list[str]


class ServiceAccountKeyCreate(BaseModel):
    """Service account key creation payload."""

    key_name: ShortText


class ServiceAccountKeyRotate(BaseModel):
    """Service account key rotation payload."""

    key_name: ShortText | None = None


class FormCreate(BaseModel):
    """Form creation payload."""

    class FormPrincipal(BaseModel):
        """Principal for form ACL metadata."""

        kind: Literal["user", "user_group"]
        id: ShortText

    name: Identifier
    version: int = 1
    template: str
    fields: dict[str, dict[str, Any]]
    allow_extra_attributes: Literal["deny", "allow_json", "allow_columns"] = "deny"
    read_principals: list[FormPrincipal] | None = None
    write_principals: list[FormPrincipal] | None = None
    defaults: dict[str, Any] | None = None
    strategies: dict[str, Any] | None = None
