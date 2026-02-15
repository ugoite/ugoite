"""Governance taxonomy validation tests.

REQ-OPS-003: Governance taxonomy links must be complete and bidirectional.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any, Never

import yaml

REPO_ROOT = Path(__file__).resolve().parents[2]
SPEC_ROOT = REPO_ROOT / "docs" / "spec"
PHILOSOPHY_PATH = SPEC_ROOT / "philosophy" / "foundation.yaml"
POLICIES_PATH = SPEC_ROOT / "policies" / "policies.yaml"
REQUIREMENTS_DEFINED_PATH = SPEC_ROOT / "requirements-defined" / "requirements.yaml"
SPECIFICATIONS_DEFINED_PATH = (
    SPEC_ROOT / "specifications-defined" / "specifications.yaml"
)


def _fail(message: str) -> Never:
    raise AssertionError(message)


def _load_yaml(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        data = yaml.safe_load(handle)
    if not isinstance(data, dict):
        message = f"Expected YAML mapping in {path}"
        raise TypeError(message)
    return data


def _assert_exists(path: Path) -> None:
    if not path.exists():
        _fail(f"Missing file: {path.relative_to(REPO_ROOT)}")


def _to_id_map(items: list[dict[str, Any]], label: str) -> dict[str, dict[str, Any]]:
    mapped: dict[str, dict[str, Any]] = {}
    for item in items:
        item_id = str(item.get("id") or "").strip()
        if not item_id:
            _fail(f"{label} item missing id")
        if item_id in mapped:
            _fail(f"Duplicate {label} id found: {item_id}")
        mapped[item_id] = item
    return mapped


def _assert_string_list(item: dict[str, Any], key: str, item_label: str) -> list[str]:
    value = item.get(key)
    if not isinstance(value, list):
        _fail(f"{item_label} must define list: {key}")
    if not value:
        _fail(f"{item_label} must define non-empty list: {key}")
    normalized = [str(v).strip() for v in value]
    if any(not v for v in normalized):
        _fail(f"{item_label} contains empty value in {key}")
    return normalized


def _load_catalog_items(path: Path, key: str, label: str) -> list[dict[str, Any]]:
    raw = _load_yaml(path).get(key)
    if not isinstance(raw, list):
        _fail(f"{label} list is required")
    if not raw:
        _fail(f"{label} list must be non-empty")
    for item in raw:
        if not isinstance(item, dict):
            _fail(f"{label} entries must be mappings")
    return raw


def _load_catalogs() -> tuple[
    dict[str, dict[str, Any]],
    dict[str, dict[str, Any]],
    dict[str, dict[str, Any]],
]:
    policy_items = _load_catalog_items(POLICIES_PATH, "policies", "policies")
    req_items = _load_catalog_items(
        REQUIREMENTS_DEFINED_PATH,
        "requirement_sets",
        "requirement_sets",
    )
    spec_items = _load_catalog_items(
        SPECIFICATIONS_DEFINED_PATH,
        "specifications",
        "specifications",
    )
    return (
        _to_id_map(policy_items, "policy"),
        _to_id_map(req_items, "requirement_set"),
        _to_id_map(spec_items, "specification"),
    )


def _assert_known_refs(
    source_map: dict[str, dict[str, Any]],
    source_key: str,
    target_map: dict[str, dict[str, Any]],
    target_label: str,
) -> None:
    for source_id, item in source_map.items():
        refs = _assert_string_list(item, source_key, source_id)
        for target_id in refs:
            if target_id not in target_map:
                _fail(
                    f"{source_id} references unknown {target_label}: {target_id}",
                )


def _assert_bidirectional(
    source_map: dict[str, dict[str, Any]],
    source_key: str,
    target_map: dict[str, dict[str, Any]],
    target_key: str,
) -> None:
    for source_id, source_item in source_map.items():
        target_ids = _assert_string_list(source_item, source_key, source_id)
        for target_id in target_ids:
            reverse_ids = _assert_string_list(
                target_map[target_id],
                target_key,
                target_id,
            )
            if source_id not in reverse_ids:
                _fail(f"Missing reverse link for {source_id} and {target_id}")


def test_req_ops_003_governance_files_exist() -> None:
    """REQ-OPS-003: Governance taxonomy YAML files must exist."""
    expected_files = [
        PHILOSOPHY_PATH,
        POLICIES_PATH,
        REQUIREMENTS_DEFINED_PATH,
        SPECIFICATIONS_DEFINED_PATH,
    ]
    for path in expected_files:
        _assert_exists(path)


def test_req_ops_003_ids_and_links_are_structurally_valid() -> None:
    """REQ-OPS-003: Governance YAMLs must define valid IDs and link lists."""
    philosophies = _load_catalog_items(
        PHILOSOPHY_PATH,
        "philosophies",
        "philosophies",
    )
    _to_id_map(philosophies, "philosophy")

    policy_map, requirement_map, specification_map = _load_catalogs()

    for req_id, req in requirement_map.items():
        source_file = str(req.get("source_file") or "").strip()
        if not source_file:
            _fail(f"{req_id} must define source_file")
        _assert_exists(SPEC_ROOT / source_file)

    for spec_id, spec in specification_map.items():
        source_file = str(spec.get("source_file") or "").strip()
        if not source_file:
            _fail(f"{spec_id} must define source_file")
        _assert_exists(SPEC_ROOT / source_file)

    _assert_known_refs(
        policy_map,
        "linked_requirements",
        requirement_map,
        "requirement",
    )
    _assert_known_refs(
        policy_map,
        "linked_specifications",
        specification_map,
        "specification",
    )
    _assert_known_refs(requirement_map, "linked_policies", policy_map, "policy")
    _assert_known_refs(
        requirement_map,
        "linked_specifications",
        specification_map,
        "specification",
    )
    _assert_known_refs(specification_map, "linked_policies", policy_map, "policy")
    _assert_known_refs(
        specification_map,
        "linked_requirements",
        requirement_map,
        "requirement",
    )


def test_req_ops_003_bidirectional_links_hold() -> None:
    """REQ-OPS-003: Policy/requirement/specification links must be bidirectional."""
    policy_map, requirement_map, specification_map = _load_catalogs()

    _assert_bidirectional(
        source_map=policy_map,
        source_key="linked_requirements",
        target_map=requirement_map,
        target_key="linked_policies",
    )
    _assert_bidirectional(
        source_map=policy_map,
        source_key="linked_specifications",
        target_map=specification_map,
        target_key="linked_policies",
    )
    _assert_bidirectional(
        source_map=requirement_map,
        source_key="linked_specifications",
        target_map=specification_map,
        target_key="linked_requirements",
    )

    for req_id, req in requirement_map.items():
        _assert_string_list(req, "linked_policies", f"requirement_set {req_id}")
        _assert_string_list(req, "linked_specifications", f"requirement_set {req_id}")

    for spec_id, spec in specification_map.items():
        _assert_string_list(spec, "linked_policies", f"specification {spec_id}")
        _assert_string_list(spec, "linked_requirements", f"specification {spec_id}")
