"""Integration tests for Python bindings."""

import hashlib
import json
import pathlib

import pytest

import ugoite_core


@pytest.mark.asyncio
async def test_bindings_async() -> None:
    """Test storage connection and basic async bindings using memory backend."""
    config = {"uri": "memory://"}

    # Test Storage Connection
    assert (await ugoite_core.test_storage_connection(config))["status"] == "ok"

    # List (initially empty or not exists?)
    # With memory backend, root is empty.
    spaces = await ugoite_core.list_spaces(config)
    assert isinstance(spaces, list)
    # Heads-up: mock memory might be fresh each time if create_operator_from_uri
    # creates NEW memory backend each call?
    # OpenDAL Memory service: "Each builder will create a new memory backend."
    # So state is LOST between calls if I don't persist the operator or
    # use a shared memory region?
    # Ah! OpenDAL Memory is transient.
    # If `ugoite_core` re-creates Operator on every call (which it does in
    # `get_operator`), then state is lost!

    # This is a problem for stateless bindings using Memory backend.
    # But for "fs://", keys persist on disk.
    # So I should test with "fs://".


@pytest.mark.asyncio
async def test_bindings_file_backend(tmp_path: pathlib.Path) -> None:
    """Test space and entry operations using a file-based backend."""
    root = tmp_path / "storage"
    root.mkdir()
    uri = f"fs://{root}"
    config = {"uri": uri}

    # List
    spaces = await ugoite_core.list_spaces(config)
    assert spaces == []

    # Create
    await ugoite_core.create_space(config, "sp-1")

    # List
    spaces = await ugoite_core.list_spaces(config)
    assert "sp-1" in spaces

    # List Forms (Empty)
    forms = await ugoite_core.list_forms(config, "sp-1")
    assert forms == []

    # Upsert Form
    form_def = json.dumps(
        {
            "name": "Entry",
            "template": "# Entry\n\n## Body\n",
            "fields": {"Body": {"type": "markdown"}},
        },
    )
    await ugoite_core.upsert_form(config, "sp-1", form_def)

    forms = await ugoite_core.list_forms(config, "sp-1")
    assert len(forms) == 1
    assert forms[0]["name"] == "Entry"

    # Create Entry
    # Expects "Author" to be passed optional? Signature says Option<String>.
    entry = await ugoite_core.create_entry(
        config,
        "sp-1",
        "entry-1",
        """---
form: Entry
---
# Content

## Body
Body text""",
        author="tester",
    )
    assert entry["id"] == "entry-1"
    assert entry["created_at"]


@pytest.mark.asyncio
async def test_user_preferences_bindings(tmp_path: pathlib.Path) -> None:
    """REQ-STO-011: Verify user preference bindings use portable hashed storage paths."""
    root = tmp_path / "storage"
    root.mkdir()
    config = {"uri": f"fs://{root}"}
    user_id = "portable/user@example.com"

    preferences = await ugoite_core.get_user_preferences(config, user_id)
    assert preferences == {
        "selected_space_id": None,
        "locale": None,
        "ui_theme": None,
        "color_mode": None,
        "primary_color": None,
    }

    updated = await ugoite_core.patch_user_preferences(
        config,
        user_id,
        json.dumps(
            {
                "selected_space_id": "space-1",
                "locale": "ja",
                "ui_theme": "classic",
            },
        ),
    )
    assert updated["selected_space_id"] == "space-1"
    assert updated["locale"] == "ja"
    assert updated["ui_theme"] == "classic"

    hashed_user = hashlib.sha256(user_id.encode("utf-8")).hexdigest()
    assert (root / "users" / hashed_user / "preferences.json").exists()
