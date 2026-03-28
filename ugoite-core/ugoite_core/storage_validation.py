"""Validation helpers for user-supplied storage connector configs."""

from __future__ import annotations

from ipaddress import ip_address
from typing import Any
from urllib.parse import urlparse

ALLOWED_STORAGE_URI_SCHEMES = frozenset({"memory", "file", "fs", "s3"})
BLOCKED_ENDPOINT_HOSTS = frozenset({"localhost"})


def validate_test_storage_config(storage_config: dict[str, Any]) -> None:
    """Reject unsupported or unsafe storage test configurations."""
    uri = storage_config.get("uri")
    if not isinstance(uri, str) or not uri.strip():
        message = "Missing 'uri' in storage config"
        raise ValueError(message)
    _validate_storage_uri(uri)

    endpoint = storage_config.get("endpoint")
    if endpoint is None:
        return
    if not isinstance(endpoint, str) or not endpoint.strip():
        message = "Storage endpoint must be a non-empty string"
        raise ValueError(message)
    _validate_storage_endpoint(endpoint)


def _validate_storage_uri(uri: str) -> None:
    if uri.startswith(("/", ".")):
        return

    scheme = urlparse(uri).scheme.lower()
    if scheme in ALLOWED_STORAGE_URI_SCHEMES:
        return
    message = f"Unsupported storage connector scheme: {scheme or 'local'}"
    raise ValueError(message)


def _validate_storage_endpoint(endpoint: str) -> None:
    parsed = urlparse(endpoint)
    if parsed.scheme.lower() not in {"http", "https"}:
        message = "Storage endpoint must use http or https"
        raise ValueError(message)
    if parsed.hostname is None:
        message = "Storage endpoint must include a host"
        raise ValueError(message)

    host = parsed.hostname.lower()
    if host in BLOCKED_ENDPOINT_HOSTS:
        message = f"Storage endpoint host is not allowed: {host}"
        raise ValueError(message)

    try:
        address = ip_address(host)
    except ValueError:
        return

    if address.is_loopback:
        message = f"Storage endpoint host is not allowed: {host}"
        raise ValueError(message)
    if address.is_link_local:
        message = f"Storage endpoint host is not allowed: {host}"
        raise ValueError(message)
