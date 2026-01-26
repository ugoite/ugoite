"""Integration tests for Python bindings."""

import json
import pathlib

import pytest

import ieapp_core


@pytest.mark.asyncio
async def test_bindings_async() -> None:
    """Test storage connection and basic async bindings using memory backend."""
    config = {"uri": "memory://"}

    # Test Storage Connection
    assert (await ieapp_core.test_storage_connection(config))["status"] == "ok"

    # List (initially empty or not exists?)
    # With memory backend, root is empty.
    workspaces = await ieapp_core.list_workspaces(config)
    assert isinstance(workspaces, list)
    # Note: mock memory might be fresh each time if create_operator_from_uri
    # creates NEW memory backend each call?
    # OpenDAL Memory service: "Each builder will create a new memory backend."
    # So state is LOST between calls if I don't persist the operator or
    # use a shared memory region?
    # Ah! OpenDAL Memory is transient.
    # If `ieapp_core` re-creates Operator on every call (which it does in
    # `get_operator`), then state is lost!

    # This is a problem for stateless bindings using Memory backend.
    # But for "fs://", keys persist on disk.
    # So I should test with "fs://".


@pytest.mark.asyncio
async def test_bindings_file_backend(tmp_path: pathlib.Path) -> None:
    """Test workspace and note operations using a file-based backend."""
    root = tmp_path / "storage"
    root.mkdir()
    uri = f"fs://{root}"
    config = {"uri": uri}

    # List
    workspaces = await ieapp_core.list_workspaces(config)
    assert workspaces == []

    # Create
    await ieapp_core.create_workspace(config, "ws-1")

    # List
    workspaces = await ieapp_core.list_workspaces(config)
    assert "ws-1" in workspaces

    # List Classes (Empty)
    classes = await ieapp_core.list_classes(config, "ws-1")
    assert classes == []

    # Upsert Class
    class_def = json.dumps(
        {
            "name": "Note",
            "template": "# Note\n\n## Body\n",
            "fields": {"Body": {"type": "markdown"}},
        },
    )
    await ieapp_core.upsert_class(config, "ws-1", class_def)

    classes = await ieapp_core.list_classes(config, "ws-1")
    assert len(classes) == 1
    assert classes[0]["name"] == "Note"

    # Create Note
    # Expects "Author" to be passed optional? Signature says Option<String>.
    note = await ieapp_core.create_note(
        config,
        "ws-1",
        "note-1",
        """---
class: Note
---
# Content

## Body
Body text""",
        author="tester",
    )
    assert note["id"] == "note-1"
    assert note["created_at"]
