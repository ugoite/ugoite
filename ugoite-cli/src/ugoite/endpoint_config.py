"""CLI endpoint routing configuration and remote API helpers."""

from __future__ import annotations

import http.client
import json
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Literal
from urllib.parse import urlparse

EndpointMode = Literal["core", "backend", "api"]
JsonValue = None | bool | int | float | str | list["JsonValue"] | dict[str, "JsonValue"]

_CONFIG_DIRNAME = ".ugoite"
_CONFIG_FILENAME = "cli-endpoints.json"
_HTTP_ERROR_STATUS = 400


@dataclass
class EndpointConfig:
    """Persistent endpoint routing settings for CLI operations."""

    mode: EndpointMode = "core"
    backend_url: str = "http://localhost:8000"
    api_url: str = "http://localhost:3000/api"


def endpoint_config_path() -> Path:
    """Return the config file path under ~/.ugoite/."""
    return Path.home() / _CONFIG_DIRNAME / _CONFIG_FILENAME


def load_endpoint_config() -> EndpointConfig:
    """Load endpoint config from disk, returning defaults when missing/invalid."""
    path = endpoint_config_path()
    if not path.exists():
        return EndpointConfig()
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return EndpointConfig()

    if not isinstance(payload, dict):
        return EndpointConfig()

    mode = payload.get("mode", "core")
    if mode not in {"core", "backend", "api"}:
        mode = "core"

    backend_url = payload.get("backend_url", EndpointConfig.backend_url)
    api_url = payload.get("api_url", EndpointConfig.api_url)
    if not isinstance(backend_url, str) or not backend_url.strip():
        backend_url = EndpointConfig.backend_url
    if not isinstance(api_url, str) or not api_url.strip():
        api_url = EndpointConfig.api_url

    return EndpointConfig(
        mode=mode,
        backend_url=backend_url,
        api_url=api_url,
    )


def save_endpoint_config(config: EndpointConfig) -> Path:
    """Persist endpoint config to ~/.ugoite/cli-endpoints.json."""
    path = endpoint_config_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(asdict(config), indent=2), encoding="utf-8")
    return path


def resolve_base_url(config: EndpointConfig) -> str | None:
    """Resolve base URL for remote mode. Returns None for local core mode."""
    if config.mode == "backend":
        return config.backend_url.rstrip("/")
    if config.mode == "api":
        return config.api_url.rstrip("/")
    return None


def parse_space_id(space_path_or_id: str) -> str:
    """Extract space id from full path (.../spaces/<id>) or return raw id."""
    text = space_path_or_id.strip().rstrip("/")
    if not text:
        return text

    if "/spaces/" in text:
        return text.split("/spaces/")[-1].split("/")[0]

    return Path(text).name or text


def request_json(
    method: str,
    url: str,
    *,
    payload: dict[str, JsonValue] | None = None,
) -> JsonValue:
    """Execute an HTTP JSON request and return parsed JSON payload."""
    parsed = urlparse(url)
    if parsed.scheme not in {"http", "https"} or not parsed.netloc:
        msg = f"Invalid URL: {url}"
        raise RuntimeError(msg)

    body = None if payload is None else json.dumps(payload)
    headers = {"Accept": "application/json"}
    if body is not None:
        headers["Content-Type"] = "application/json"

    path = parsed.path or "/"
    if parsed.query:
        path = f"{path}?{parsed.query}"

    conn_cls: type[http.client.HTTPConnection] = (
        http.client.HTTPSConnection
        if parsed.scheme == "https"
        else http.client.HTTPConnection
    )
    try:
        conn = conn_cls(parsed.netloc, timeout=30)
        conn.request(method.upper(), path, body=body, headers=headers)
        response = conn.getresponse()
        raw_body = response.read().decode("utf-8")
        conn.close()
    except OSError as exc:
        msg = f"Connection failed: {exc}"
        raise RuntimeError(msg) from exc

    if response.status >= _HTTP_ERROR_STATUS:
        detail = raw_body
        try:
            payload_obj = json.loads(raw_body)
            if isinstance(payload_obj, dict):
                detail = str(payload_obj.get("detail", raw_body))
        except json.JSONDecodeError:
            pass
        msg = f"HTTP {response.status}: {detail}"
        raise RuntimeError(msg)

    if not raw_body:
        return None
    try:
        return json.loads(raw_body)
    except json.JSONDecodeError as exc:
        msg = "Response was not valid JSON"
        raise RuntimeError(msg) from exc
