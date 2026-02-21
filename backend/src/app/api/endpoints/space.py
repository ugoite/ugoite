"""Space endpoints."""

import json
import logging
from typing import Any

import ugoite_core
from fastapi import APIRouter, HTTPException, Request, status

from app.core.authorization import (
    raise_authorization_http_error,
    request_identity,
)
from app.core.config import get_root_path
from app.core.ids import validate_id
from app.core.storage import space_uri, storage_config_from_root
from app.models.payloads import (
    SpaceConnectionRequest,
    SpaceCreate,
    SpacePatch,
)

router = APIRouter()
logger = logging.getLogger(__name__)


def _storage_config() -> dict[str, str]:
    """Build storage config for the current space root."""
    return storage_config_from_root(get_root_path())


async def _ensure_space_exists(
    storage_config: dict[str, str],
    space_id: str,
) -> dict[str, Any]:
    """Ensure a space exists or raise 404."""
    try:
        return await ugoite_core.get_space(storage_config, space_id)
    except RuntimeError as exc:
        msg = str(exc)
        if "not found" in msg.lower():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Space not found: {space_id}",
            ) from exc
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=msg,
        ) from exc


def _format_form_validation_errors(errors: list[dict[str, Any]]) -> str:
    """Format form validation warnings into a single human-readable string.

    The validator returns a list of dicts describing issues. This helper
    consolidates them into a newline-separated message suitable for HTTP API
    responses.

    Args:
        errors: A list of dictionaries containing keys like "message" and
            "field" as returned by the property validator.

    Returns:
        A newline-separated string describing the validation issues.

    """
    parts: list[str] = []
    for err in errors:
        message = err.get("message")
        field = err.get("field")
        if isinstance(message, str) and message:
            parts.append(message)
        elif isinstance(field, str) and field:
            parts.append(f"Invalid field: {field}")
        else:
            parts.append("Form validation error")
    return "\n".join(parts)


async def _validate_entry_markdown_against_form(
    storage_config: dict[str, str],
    space_id: str,
    markdown: str,
) -> None:
    """Validate extracted entry properties against the space form."""
    properties = ugoite_core.extract_properties(markdown)
    entry_form = properties.get("form")
    if not isinstance(entry_form, str) or not entry_form.strip():
        return

    try:
        form_def = await ugoite_core.get_form(storage_config, space_id, entry_form)
    except RuntimeError as e:
        if "not found" not in str(e).lower():
            raise HTTPException(
                status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
                detail="Failed to load form definition",
            ) from e
        raise HTTPException(
            status_code=status.HTTP_422_UNPROCESSABLE_CONTENT,
            detail=f"Form not found: {entry_form}",
        ) from e

    _casted, warnings = ugoite_core.validate_properties(
        json.dumps(properties),
        json.dumps(form_def),
    )
    if warnings:
        raise HTTPException(
            status_code=status.HTTP_422_UNPROCESSABLE_CONTENT,
            detail=_format_form_validation_errors(warnings),
        )


def _validate_path_id(identifier: str, name: str) -> None:
    """Validate identifier using shared ugoite-cli rules."""
    try:
        validate_id(identifier, name)
    except ValueError as exc:  # pragma: no cover - exercised via API tests
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=str(exc),
        ) from exc


def _space_uri(space_id: str) -> str:
    """Return space URI/path for API responses."""
    return space_uri(get_root_path(), space_id)


def _sanitize_space_meta(space_meta: dict[str, Any]) -> dict[str, Any]:
    """Redact sensitive membership secrets before API response serialization."""
    sanitized = dict(space_meta)
    settings_obj = sanitized.get("settings")
    if not isinstance(settings_obj, dict):
        return sanitized

    settings = dict(settings_obj)
    if "invitations" in settings:
        settings["invitations"] = {}
    sanitized["settings"] = settings
    return sanitized


@router.get("/spaces")
async def list_spaces_endpoint(request: Request) -> list[dict[str, Any]]:
    """List spaces visible to the authenticated principal."""
    identity = request_identity(request)
    storage_config = _storage_config()
    try:
        space_ids = await ugoite_core.list_spaces(storage_config)
    except RuntimeError as exc:
        logger.warning("Failed to list spaces, returning empty list: %s", exc)
        return []
    except Exception as exc:
        logger.exception("Failed to list spaces")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(exc),
        ) from exc

    results: list[dict[str, Any]] = []
    for space_id in space_ids:
        try:
            await ugoite_core.require_space_action(
                storage_config,
                space_id,
                identity,
                "space_list",
            )
            space_meta = await ugoite_core.get_space(storage_config, space_id)
            results.append(_sanitize_space_meta(space_meta))
        except ugoite_core.AuthorizationError:
            continue
        except Exception:
            logger.exception("Failed to read space meta %s", space_id)

    return results


@router.post("/spaces", status_code=status.HTTP_201_CREATED)
async def create_space_endpoint(
    request: Request,
    payload: SpaceCreate,
) -> dict[str, str]:
    """Create a new space."""
    identity = request_identity(request)
    _validate_path_id(payload.name, "space_id")
    space_id = payload.name  # Using name as ID for now per simple spec

    try:
        storage_config = _storage_config()
        await ugoite_core.create_space(storage_config, space_id)
        await ugoite_core.patch_space(
            storage_config,
            space_id,
            json.dumps(
                {
                    "owner_user_id": identity.user_id,
                    "admin_user_ids": [identity.user_id],
                    "member_roles": {},
                    "settings": {
                        "owner_user_id": identity.user_id,
                        "admin_user_ids": [identity.user_id],
                        "member_roles": {},
                        "members": {
                            identity.user_id: {
                                "user_id": identity.user_id,
                                "role": "owner",
                                "state": "active",
                                "activated_at": "bootstrap",
                                "invited_by": identity.user_id,
                                "invited_at": "bootstrap",
                                "revoked_at": None,
                            },
                        },
                        "invitations": {},
                        "membership_version": 1,
                    },
                },
            ),
        )
    except RuntimeError as e:
        if "already exists" in str(e).lower():
            raise HTTPException(
                status_code=status.HTTP_409_CONFLICT,
                detail=str(e),
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
    except Exception as e:
        logger.exception("Failed to create space")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e

    return {
        "id": space_id,
        "name": payload.name,
        "path": _space_uri(space_id),
    }


@router.get("/spaces/{space_id}")
async def get_space_endpoint(space_id: str, request: Request) -> dict[str, Any]:
    """Get space metadata."""
    identity = request_identity(request)
    _validate_path_id(space_id, "space_id")
    try:
        storage_config = _storage_config()
        await ugoite_core.require_space_action(
            storage_config,
            space_id,
            identity,
            "space_read",
        )
        space_meta = await ugoite_core.get_space(storage_config, space_id)
        return _sanitize_space_meta(space_meta)
    except ugoite_core.AuthorizationError as exc:
        raise_authorization_http_error(exc, space_id=space_id)
    except RuntimeError as e:
        if "not found" in str(e).lower():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=str(e),
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
    except Exception as e:
        logger.exception("Failed to get space")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.patch("/spaces/{space_id}")
async def patch_space_endpoint(
    space_id: str,
    payload: SpacePatch,
    request: Request,
) -> dict[str, Any]:
    """Update space metadata/settings including storage connector."""
    identity = request_identity(request)
    _validate_path_id(space_id, "space_id")

    # Build patch dict from payload
    patch_data = {}
    if payload.name is not None:
        patch_data["name"] = payload.name
    if payload.storage_config is not None:
        patch_data["storage_config"] = payload.storage_config
    if payload.settings is not None:
        patch_data["settings"] = payload.settings

    try:
        storage_config = _storage_config()
        await ugoite_core.require_space_action(
            storage_config,
            space_id,
            identity,
            "space_admin",
        )
        updated_space = await ugoite_core.patch_space(
            storage_config,
            space_id,
            json.dumps(patch_data),
        )
        return _sanitize_space_meta(updated_space)
    except ugoite_core.AuthorizationError as exc:
        raise_authorization_http_error(exc, space_id=space_id)
    except RuntimeError as e:
        if "not found" in str(e).lower():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=str(e),
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
    except Exception as e:
        logger.exception("Failed to patch space")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.post("/spaces/{space_id}/test-connection")
async def test_connection_endpoint(
    space_id: str,
    payload: SpaceConnectionRequest,
    request: Request,
) -> dict[str, Any]:
    """Validate the provided storage connector (stubbed for Milestone 6)."""
    identity = request_identity(request)
    _validate_path_id(space_id, "space_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        await ugoite_core.require_space_action(
            storage_config,
            space_id,
            identity,
            "space_admin",
        )
        return await ugoite_core.test_storage_connection(payload.storage_config)
    except ugoite_core.AuthorizationError as exc:
        raise_authorization_http_error(exc, space_id=space_id)
    except ValueError as e:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=str(e),
        ) from e
