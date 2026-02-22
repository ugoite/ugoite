"""Auth overview consistency tests.

REQ-SEC-003: Mandatory User Authentication.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import cast

from ugoite_core.auth import export_authentication_overview

DOC_ONLY_PROVIDER_FIELDS = {"active_kids_source", "revocation_source"}


def _as_map(value: object) -> dict[str, object]:
    assert isinstance(value, dict)
    return cast("dict[str, object]", value)


def _assert_provider_contract(
    expected_provider: dict[str, object],
    actual_provider: dict[str, object],
) -> None:
    expected_keys = set(expected_provider.keys()) - DOC_ONLY_PROVIDER_FIELDS
    actual_keys = set(actual_provider.keys())
    assert actual_keys == expected_keys

    for key in expected_keys:
        assert actual_provider[key] == expected_provider[key]

    for key in DOC_ONLY_PROVIDER_FIELDS:
        if key in expected_provider:
            assert key not in actual_provider


def test_auth_overview_matches_security_yaml_contract() -> None:
    """REQ-SEC-003: Rust auth overview matches docs security YAML contract."""
    docs_path = (
        Path(__file__).resolve().parents[2]
        / "docs"
        / "spec"
        / "security"
        / "authentication-overview.yaml"
    )
    expected = _as_map(json.loads(docs_path.read_text(encoding="utf-8")))
    actual = _as_map(export_authentication_overview())

    assert actual["version"] == expected["version"]
    assert actual["enforcement"] == expected["enforcement"]

    expected_identity = _as_map(expected["identity_model"])
    actual_identity = _as_map(actual["identity_model"])
    assert actual_identity["principal_types"] == expected_identity["principal_types"]
    assert actual_identity["fields"] == expected_identity["fields"]

    expected_providers = _as_map(expected["providers"])
    actual_providers = _as_map(actual["providers"])

    _assert_provider_contract(
        _as_map(expected_providers["bearer"]),
        _as_map(actual_providers["bearer"]),
    )
    _assert_provider_contract(
        _as_map(expected_providers["api_key"]),
        _as_map(actual_providers["api_key"]),
    )

    assert actual["channels"] == expected["channels"]
