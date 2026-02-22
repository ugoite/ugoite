"""Auth overview consistency tests.

REQ-SEC-003: Mandatory User Authentication.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import cast

from ugoite_core.auth import export_authentication_overview


def _as_map(value: object) -> dict[str, object]:
    assert isinstance(value, dict)
    return cast("dict[str, object]", value)


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

    actual_providers = _as_map(actual["providers"])
    actual_bearer = _as_map(actual_providers["bearer"])
    actual_api_key = _as_map(actual_providers["api_key"])

    assert actual_bearer["supports_static_tokens"] is True
    assert actual_bearer["supports_signed_tokens"] is True
    assert actual_api_key["supports_static_api_keys"] is True
    assert actual_api_key["supports_space_service_account_keys"] is True

    assert actual["channels"] == expected["channels"]
