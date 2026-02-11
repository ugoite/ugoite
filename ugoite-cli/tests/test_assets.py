"""Asset helpers must work across fsspec implementations.

REQ-ASSET-001: Asset creation and lifecycle.

These tests exercise saving, listing and deleting assets and
reference handling across multiple filesystem implementations.
"""

import hashlib
import hmac
import json
from typing import Any

import fsspec
import pytest

from ugoite.assets import (
    AssetReferencedError,
    delete_asset,
    list_assets,
    save_asset,
)
from ugoite.entries import create_entry, update_entry
from ugoite.space import create_space
from ugoite.utils import fs_join


@pytest.fixture
def fake_integrity_provider() -> Any:
    """Create a simple integrity provider for tests."""

    class _FakeIntegrityProvider:
        secret = b"asset-test"

        def checksum(self, content: str) -> str:
            return hashlib.sha256(content.encode("utf-8")).hexdigest()

        def signature(self, content: str) -> str:
            return hmac.new(
                self.secret,
                content.encode("utf-8"),
                digestmod="sha256",
            ).hexdigest()

    return _FakeIntegrityProvider()


def test_asset_lifecycle(
    fs_impl: tuple[fsspec.AbstractFileSystem, str],
    fake_integrity_provider: Any,
) -> None:
    """Save/list/delete should work on any filesystem and honor references."""
    fs, root = fs_impl
    ws_id = "ws-assets"
    create_space(root, ws_id, fs=fs)
    ws_path = fs_join(root, "spaces", ws_id)

    asset = save_asset(ws_path, b"hello", "voice.m4a", fs=fs)

    assert asset["id"]
    # Check file existence
    # Filename format is {id}_{original_name}
    expected_path = fs_join(ws_path, "assets", f"{asset['id']}_voice.m4a")
    assert fs.exists(expected_path)

    listed = list_assets(ws_path, fs=fs)
    assert any(item["id"] == asset["id"] for item in listed)

    create_entry(
        ws_path,
        "entry-a",
        "# A",
        integrity_provider=fake_integrity_provider,
        fs=fs,
    )

    entry_content_path = fs_join(ws_path, "entries", "entry-a", "content.json")
    with fs.open(entry_content_path, "r") as handle:
        rev = json.load(handle)["revision_id"]

    update_entry(
        ws_path,
        "entry-a",
        "# A\n\nlinked",
        parent_revision_id=rev,
        assets=[asset],
        integrity_provider=fake_integrity_provider,
        fs=fs,
    )

    with pytest.raises(AssetReferencedError):
        delete_asset(ws_path, asset["id"], fs=fs)

    with fs.open(entry_content_path, "r") as handle:
        rev_after = json.load(handle)["revision_id"]

    update_entry(
        ws_path,
        "entry-a",
        "# A\n\ncleared",
        parent_revision_id=rev_after,
        assets=[],
        integrity_provider=fake_integrity_provider,
        fs=fs,
    )

    delete_asset(ws_path, asset["id"], fs=fs)

    remaining = list_assets(ws_path, fs=fs)
    assert all(item["id"] != asset["id"] for item in remaining)
