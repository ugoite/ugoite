"""Tests for the `build_sandbox.py` download logic (redirect handling)."""

import gzip
import http.client
import importlib.util
import json
import os
from pathlib import Path
from types import ModuleType

import pytest


def _load_module() -> ModuleType:
    """Load the script module from file (not importable).

    The function returns the loaded module object so tests can access
    the helpers inside the script. This performs runtime checks so a
    static type checker knows ``spec`` and ``loader`` are present.
    """
    here = Path(__file__).parent.parent
    script_path = here / "scripts" / "build_sandbox.py"
    spec = importlib.util.spec_from_file_location("build_sandbox", str(script_path))
    if spec is None:
        msg = "Failed to create module spec for build_sandbox.py"
        raise RuntimeError(msg)

    loader = spec.loader
    if loader is None or not hasattr(loader, "exec_module"):
        msg = "Spec loader is missing or cannot exec module"
        raise RuntimeError(msg)

    module = importlib.util.module_from_spec(spec)
    # mypy/ty should accept this after the runtime checks above
    loader.exec_module(module)  # type: ignore[arg-type]
    return module


_mod = _load_module()
download_and_extract_javy = _mod.download_and_extract_javy


class FakeResponse:
    """A minimal fake HTTP response for tests."""

    def __init__(
        self,
        status: int,
        headers: dict | None = None,
        body: bytes = b"",
    ) -> None:
        """Create a fake response."""
        self.status = status
        self._headers = {k.lower(): v for k, v in (headers or {}).items()}
        self._body = body

    def getheader(self, name: str, default: object | None = None) -> object | None:
        """Return header by name (case-insensitive)."""
        return self._headers.get(name.lower(), default)

    def read(self) -> bytes:
        """Return the response body as bytes."""
        return self._body


class FakeConnection:
    """A minimal fake HTTPSConnection replacement for tests."""

    def __init__(
        self,
        netloc: str,
        responses: list | None = None,
        _timeout: int | None = None,
    ) -> None:
        """Create a fake connection preloaded with responses."""
        # Store requested paths to allow assertions if needed
        self.netloc = netloc
        self.requested: list[tuple[str, str]] = []
        # A sequence of responses to return
        self._responses = list(responses or [])

    def request(self, method: str, path: str) -> None:
        """Record a request made against this fake connection."""
        self.requested.append((method, path))

    def getresponse(self) -> "FakeResponse":
        """Return the next configured fake response, or raise an error if none."""
        if not self._responses:
            msg = "No responses configured for FakeConnection"
            raise RuntimeError(msg)
        return self._responses.pop(0)


def compress_bytes(data: bytes) -> bytes:
    """Return gzip-compressed bytes for the provided data."""
    return gzip.compress(data)


def test_download_follows_redirects(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    """Ensure the download follows redirects and writes an executable file."""
    # Prepare a fake binary and its gzipped form
    binary = b"hello-javy-binary\n"
    gz = compress_bytes(binary)

    # Prepare fake responses: first a 302 redirect, then a 200 with gzipped body
    resp1 = FakeResponse(
        302,
        headers={"location": "https://stored.example.com/file.gz"},
    )
    resp2 = FakeResponse(200, headers={}, body=gz)

    # Prepare two connections: one for initial host and one for the redirected host
    conn_initial = FakeConnection("github.com", responses=[resp1])
    conn_redirect = FakeConnection("stored.example.com", responses=[resp2])

    # Monkeypatch HTTPSConnection to return the appropriate fake connection
    def fake_https_connection(
        netloc: str,
        timeout: int | None = None,
    ) -> FakeConnection:
        if netloc == "github.com":
            return conn_initial
        if netloc == "stored.example.com":
            return conn_redirect
        msg = f"Unexpected host: {netloc}"
        raise RuntimeError(msg)

    monkeypatch.setattr(http.client, "HTTPSConnection", fake_https_connection)

    # Destination path
    dest = tmp_path / "javy"

    # Run the function under test
    download_and_extract_javy(dest)

    # File should exist and contain the uncompressed binary
    assert dest.exists()
    assert dest.read_bytes() == binary

    # It should be executable
    assert os.access(dest, os.X_OK)


def test_download_raises_on_too_many_redirects(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    """If too many redirects are encountered, raise an error."""
    # Each response is a redirect back to the same host (infinite loop if not limited)
    responses = [
        FakeResponse(302, headers={"location": "/something"}) for _ in range(6)
    ]
    conn = FakeConnection("github.com", responses=responses)

    def fake_https_connection(
        netloc: str,
        timeout: int | None = None,
    ) -> FakeConnection:
        return conn

    monkeypatch.setattr(http.client, "HTTPSConnection", fake_https_connection)

    with pytest.raises(RuntimeError):
        # Use a tmp_path to avoid hard-coded /tmp usage
        download_and_extract_javy(tmp_path / "notreal")


def test_javy_version_is_latest() -> None:
    """Verify that JAVY_VERSION matches the latest release on GitHub."""
    # 1. Get local version
    local_version = getattr(_mod, "JAVY_VERSION", None)
    assert local_version is not None, "JAVY_VERSION not found in build_sandbox.py"

    # 2. Get remote version using http.client to avoid urlopen S310 warning
    host = "api.github.com"
    path = "/repos/bytecodealliance/javy/releases/latest"
    conn = http.client.HTTPSConnection(host, timeout=10)

    try:
        conn.request("GET", path, headers={"User-Agent": "ieapp-cli-test"})
        resp = conn.getresponse()
        if resp.status != http.client.OK:
            pytest.fail(f"GitHub API returned status {resp.status}")
        data = json.loads(resp.read().decode("utf-8"))
        remote_version = data.get("tag_name")
    except (http.client.HTTPException, OSError, json.JSONDecodeError) as exc:
        pytest.fail(f"Failed to fetch latest Javy version: {exc}")
    finally:
        close = getattr(conn, "close", None)
        if callable(close):
            close()

    # 3. Compare
    assert local_version == remote_version, (
        f"Local JAVY_VERSION ({local_version}) is outdated. "
        f"Latest release is {remote_version}."
    )
