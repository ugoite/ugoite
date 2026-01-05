"""Attachment helpers must work across fsspec implementations."""

import hashlib
import hmac
import json
from typing import Any

import fsspec
import pytest

from ieapp.attachments import (
    AttachmentReferencedError,
    delete_attachment,
    list_attachments,
    save_attachment,
)
from ieapp.notes import create_note, update_note
from ieapp.workspace import create_workspace


@pytest.fixture
def fake_integrity_provider() -> Any:  # type: ignore[override]
    """Create a simple integrity provider for tests."""

    class _FakeIntegrityProvider:
        secret = b"attachment-test"

        def checksum(self, content: str) -> str:
            return hashlib.sha256(content.encode("utf-8")).hexdigest()

        def signature(self, content: str) -> str:
            return hmac.new(
                self.secret,
                content.encode("utf-8"),
                digestmod="sha256",
            ).hexdigest()

    return _FakeIntegrityProvider()


def test_attachment_lifecycle_in_memory_fs(fake_integrity_provider: Any) -> None:
    """Save/list/delete should work on memory filesystem and honor references."""
    fs = fsspec.filesystem("memory")

    root = "/ieapp"
    ws_id = "ws-attachments"
    create_workspace(root, ws_id, fs=fs)
    ws_path = f"{root}/workspaces/{ws_id}"

    attachment = save_attachment(ws_path, b"hello", "voice.m4a", fs=fs)

    assert attachment["id"]
    assert fs.exists(f"{ws_path}/attachments/{attachment['id']}_voice.m4a")

    listed = list_attachments(ws_path, fs=fs)
    assert any(item["id"] == attachment["id"] for item in listed)

    create_note(
        ws_path,
        "note-a",
        "# A",
        integrity_provider=fake_integrity_provider,
        fs=fs,
    )

    with fs.open(f"{ws_path}/notes/note-a/content.json", "r") as handle:
        rev = json.load(handle)["revision_id"]

    update_note(
        ws_path,
        "note-a",
        "# A\n\nlinked",
        parent_revision_id=rev,
        attachments=[attachment],
        integrity_provider=fake_integrity_provider,
        fs=fs,
    )

    with pytest.raises(AttachmentReferencedError):
        delete_attachment(ws_path, attachment["id"], fs=fs)

    with fs.open(f"{ws_path}/notes/note-a/content.json", "r") as handle:
        rev_after = json.load(handle)["revision_id"]

    update_note(
        ws_path,
        "note-a",
        "# A\n\ncleared",
        parent_revision_id=rev_after,
        attachments=[],
        integrity_provider=fake_integrity_provider,
        fs=fs,
    )

    delete_attachment(ws_path, attachment["id"], fs=fs)

    remaining = list_attachments(ws_path, fs=fs)
    assert all(item["id"] != attachment["id"] for item in remaining)
