"""Validation helpers for user-supplied storage connector configs."""

from __future__ import annotations

import socket
from ipaddress import IPv4Address, IPv6Address, ip_address
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
    uri = uri.strip()
    _validate_storage_uri(uri)

    endpoint = storage_config.get("endpoint")
    if endpoint is None:
        return
    if not isinstance(endpoint, str) or not endpoint.strip():
        message = "Storage endpoint must be a non-empty string"
        raise ValueError(message)
    endpoint = endpoint.strip()
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

    if _is_blocked_host(host):
        message = f"Storage endpoint host is not allowed: {host}"
        raise ValueError(message)


def _is_blocked_host(host: str) -> bool:
    address = _parse_ip_address(host)
    if address is not None:
        return address.is_loopback or address.is_link_local

    for resolved_address in _resolve_host_addresses(host):
        if resolved_address.is_loopback or resolved_address.is_link_local:
            return True
    return False


def _parse_ip_address(host: str) -> IPv4Address | IPv6Address | None:
    normalized_host = host.split("%", 1)[0]
    try:
        return ip_address(normalized_host)
    except ValueError:
        return None


def _resolve_host_addresses(host: str) -> set[IPv4Address | IPv6Address]:
    try:
        addr_info = socket.getaddrinfo(host, None, type=socket.SOCK_STREAM)
    except socket.gaierror:
        return set()

    resolved = set()
    for _family, _socktype, _proto, _canonname, sockaddr in addr_info:
        address = _parse_ip_address(sockaddr[0])
        if address is not None:
            resolved.add(address)
    return resolved
