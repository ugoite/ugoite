"""Form endpoints."""

import json
import logging
from typing import Any

import ugoite_core
from fastapi import APIRouter, HTTPException, Request, status

from app.api.endpoints.space import (
    _ensure_space_exists,
    _storage_config,
    _validate_path_id,
)
from app.core.authorization import (
    raise_authorization_http_error,
    request_identity,
)
from app.models.payloads import FormCreate

router = APIRouter()
logger = logging.getLogger(__name__)


async def _persist_form_acl_settings(
    storage_config: dict[str, str],
    space_id: str,
    form_name: str,
    read_principals: list[dict[str, Any]] | None,
    write_principals: list[dict[str, Any]] | None,
) -> None:
    if read_principals is None and write_principals is None:
        return

    existing_form_acls: dict[str, Any] = {}
    try:
        space_meta = await ugoite_core.patch_space(
            storage_config,
            space_id,
            json.dumps({}),
        )
        settings = space_meta.get("settings")
        if isinstance(settings, dict):
            form_acls = settings.get("form_acls")
            if isinstance(form_acls, dict):
                existing_form_acls = {
                    key: value
                    for key, value in form_acls.items()
                    if isinstance(key, str) and isinstance(value, dict)
                }
    except RuntimeError:
        existing_form_acls = {}

    next_form_acls = dict(existing_form_acls)
    acl_entry = dict(next_form_acls.get(form_name, {}))
    if read_principals is not None:
        acl_entry["read_principals"] = read_principals
    if write_principals is not None:
        acl_entry["write_principals"] = write_principals
    next_form_acls[form_name] = acl_entry

    await ugoite_core.patch_space(
        storage_config,
        space_id,
        json.dumps({"settings": {"form_acls": next_form_acls}}),
    )


@router.get("/spaces/{space_id}/forms")
async def list_forms_endpoint(space_id: str, request: Request) -> list[dict[str, Any]]:
    """List all forms in the space."""
    identity = request_identity(request)
    _validate_path_id(space_id, "space_id")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        await ugoite_core.require_space_action(
            storage_config,
            space_id,
            identity,
            "form_read",
        )
        forms = await ugoite_core.list_forms(storage_config, space_id)
        visible_forms: list[dict[str, Any]] = []
        for form in forms:
            form_name = form.get("name")
            if not isinstance(form_name, str) or not form_name:
                continue
            try:
                await ugoite_core.require_form_read(
                    storage_config,
                    space_id,
                    identity,
                    form_name,
                )
            except ugoite_core.AuthorizationError:
                continue
            visible_forms.append(form)
    except ugoite_core.AuthorizationError as exc:
        raise_authorization_http_error(exc, space_id=space_id)
    except Exception as e:
        logger.exception("Failed to list forms")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
    else:
        return visible_forms


@router.get("/spaces/{space_id}/forms/types")
async def list_form_types_endpoint(space_id: str, request: Request) -> list[str]:
    """Get list of available column types."""
    identity = request_identity(request)
    _validate_path_id(space_id, "space_id")
    try:
        # Verify space exists even though types are static
        storage_config = _storage_config()
        await _ensure_space_exists(storage_config, space_id)
        await ugoite_core.require_space_action(
            storage_config,
            space_id,
            identity,
            "space_read",
        )
        return await ugoite_core.list_column_types()
    except ugoite_core.AuthorizationError as exc:
        raise_authorization_http_error(exc, space_id=space_id)
    except Exception as e:
        if isinstance(e, HTTPException):
            raise
        logger.exception("Failed to list form types")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.get("/spaces/{space_id}/forms/{form_name}")
async def get_form_endpoint(
    space_id: str,
    form_name: str,
    request: Request,
) -> dict[str, Any]:
    """Get a specific form definition."""
    identity = request_identity(request)
    _validate_path_id(space_id, "space_id")
    _validate_path_id(form_name, "form_name")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        await ugoite_core.require_form_read(
            storage_config,
            space_id,
            identity,
            form_name,
        )
        return await ugoite_core.get_form(storage_config, space_id, form_name)
    except ugoite_core.AuthorizationError as exc:
        raise_authorization_http_error(exc, space_id=space_id)
    except RuntimeError as e:
        if "not found" in str(e).lower():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Form not found: {form_name}",
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e


@router.post("/spaces/{space_id}/forms", status_code=status.HTTP_201_CREATED)
async def create_form_endpoint(
    space_id: str,
    payload: FormCreate,
    request: Request,
) -> dict[str, Any]:
    """Create or update a form definition."""
    identity = request_identity(request)
    _validate_path_id(space_id, "space_id")
    _validate_path_id(payload.name, "form_name")
    storage_config = _storage_config()
    await _ensure_space_exists(storage_config, space_id)

    try:
        await ugoite_core.require_space_action(
            storage_config,
            space_id,
            identity,
            "form_write",
        )
        # Separate strategies from persistent form definition
        form_data = payload.model_dump()
        strategies = form_data.pop("strategies", None)
        read_principals = form_data.pop("read_principals", None)
        write_principals = form_data.pop("write_principals", None)

        if read_principals is not None or write_principals is not None:
            await ugoite_core.require_space_action(
                storage_config,
                space_id,
                identity,
                "space_admin",
            )

        form_json = json.dumps(form_data)

        await ugoite_core.upsert_form(storage_config, space_id, form_json)
        await _persist_form_acl_settings(
            storage_config,
            space_id,
            payload.name,
            read_principals,
            write_principals,
        )

        if strategies:
            strategies_json = json.dumps(strategies)
            await ugoite_core.migrate_form(
                storage_config,
                space_id,
                form_json,
                strategies_json,
            )

        return await ugoite_core.get_form(storage_config, space_id, payload.name)
    except ugoite_core.AuthorizationError as exc:
        raise_authorization_http_error(exc, space_id=space_id)
    except RuntimeError as e:
        msg = str(e)
        lowered = msg.lower()
        if (
            "reserved" in lowered
            or "row_reference" in lowered
            or "target_form" in lowered
        ):
            raise HTTPException(
                status_code=status.HTTP_422_UNPROCESSABLE_CONTENT,
                detail=msg,
            ) from e
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=msg,
        ) from e
    except Exception as e:
        logger.exception("Failed to upsert form")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e),
        ) from e
