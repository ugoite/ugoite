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
SPECIFICATIONS_PATH = SPEC_ROOT / "specifications.yaml"
REQUIREMENTS_DIR = SPEC_ROOT / "requirements"


def _fail(message: str) -> Never:
    raise AssertionError(message)


def _load_yaml(path: Path) -> object:
    with path.open("r", encoding="utf-8") as handle:
        return yaml.safe_load(handle)


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


def _require_non_empty_str(req: dict[str, Any], key: str, context: str) -> str:
    value = str(req.get(key) or "").strip()
    if not value:
        _fail(f"{context} missing {key}")
    return value


def _require_non_empty_list(req: dict[str, Any], key: str, context: str) -> list[str]:
    value = req.get(key)
    if not isinstance(value, list) or not value:
        _fail(f"{context} missing {key}")
    normalized = [str(v).strip() for v in value]
    if any(not v for v in normalized):
        _fail(f"{context} has empty value in {key}")
    return normalized


def _normalize_requirement_metadata(
    req_file: Path,
    req: dict[str, Any],
) -> tuple[str, dict[str, Any]]:
    context = f"{req_file.name} requirement"
    set_id = _require_non_empty_str(req, "set_id", context)
    source_file = _require_non_empty_str(req, "source_file", context)
    scope = _require_non_empty_str(req, "scope", context)
    linked_policies = _require_non_empty_list(req, "linked_policies", context)
    linked_specs = _require_non_empty_list(req, "linked_specifications", context)

    relative = req_file.relative_to(SPEC_ROOT).as_posix()
    if source_file != relative:
        _fail(
            f"{set_id} source_file mismatch: expected {relative}, got {source_file}",
        )

    normalized = {
        "id": set_id,
        "source_file": source_file,
        "scope": scope,
        "linked_policies": linked_policies,
        "linked_specifications": linked_specs,
    }
    return set_id, normalized


def _accumulate_requirement_metadata(
    set_map: dict[str, dict[str, Any]],
    req_file: Path,
    requirements: list[dict[str, Any]],
) -> None:
    for req in requirements:
        if not isinstance(req, dict):
            _fail(f"{req_file.name} requirements entries must be mappings")

        set_id, normalized = _normalize_requirement_metadata(req_file, req)
        existing = set_map.get(set_id)
        if existing is None:
            set_map[set_id] = normalized
            continue
        if existing != normalized:
            _fail(f"{set_id} metadata must be identical across requirements")


def _load_policies() -> dict[str, dict[str, Any]]:
    data = _load_yaml(POLICIES_PATH)
    if not isinstance(data, dict):
        _fail("policies.yaml must be a mapping")
    policies = data.get("policies")
    if not isinstance(policies, list):
        _fail("policies list is required")
    if not policies:
        _fail("policies list must be non-empty")
    for item in policies:
        if not isinstance(item, dict):
            _fail("policies entries must be mappings")
    return _to_id_map(policies, "policy")


def _load_specifications() -> dict[str, dict[str, Any]]:
    data = _load_yaml(SPECIFICATIONS_PATH)
    if not isinstance(data, list):
        _fail("specifications.yaml must be a list")
    if not data:
        _fail("specifications.yaml must be non-empty")
    for item in data:
        if not isinstance(item, dict):
            _fail("specifications entries must be mappings")
    return _to_id_map(data, "specification")


def _load_requirement_sets() -> dict[str, dict[str, Any]]:
    set_map: dict[str, dict[str, Any]] = {}
    files = sorted(REQUIREMENTS_DIR.glob("*.yaml"))
    if not files:
        _fail("requirements directory must contain YAML files")

    for req_file in files:
        loaded = _load_yaml(req_file)
        if not isinstance(loaded, dict):
            _fail(f"{req_file.name} must be a mapping")
        requirements = loaded.get("requirements")
        if not isinstance(requirements, list):
            _fail(f"{req_file.name} must define requirements list")
        if not requirements:
            _fail(f"{req_file.name} requirements list must be non-empty")

        typed_requirements: list[dict[str, Any]] = []
        for req in requirements:
            if not isinstance(req, dict):
                _fail(f"{req_file.name} requirements entries must be mappings")
            typed_requirements.append(req)

        _accumulate_requirement_metadata(set_map, req_file, typed_requirements)

    if not set_map:
        _fail("No requirement-set metadata found in requirements/*.yaml")
    return set_map


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
    _assert_exists(PHILOSOPHY_PATH)
    _assert_exists(POLICIES_PATH)
    _assert_exists(SPECIFICATIONS_PATH)
    _assert_exists(REQUIREMENTS_DIR)


def test_req_ops_003_ids_and_links_are_structurally_valid() -> None:
    """REQ-OPS-003: Governance YAMLs must define valid IDs and link lists."""
    philosophy_loaded = _load_yaml(PHILOSOPHY_PATH)
    if not isinstance(philosophy_loaded, dict):
        _fail("philosophy/foundation.yaml must be a mapping")
    philosophies = philosophy_loaded.get("philosophies")
    if not isinstance(philosophies, list) or not philosophies:
        _fail("philosophies list is required")
    _to_id_map(philosophies, "philosophy")

    policy_map = _load_policies()
    requirement_map = _load_requirement_sets()
    specification_map = _load_specifications()

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
    policy_map = _load_policies()
    requirement_map = _load_requirement_sets()
    specification_map = _load_specifications()

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
    _assert_bidirectional(
        source_map=specification_map,
        source_key="linked_policies",
        target_map=policy_map,
        target_key="linked_specifications",
    )
    _assert_bidirectional(
        source_map=specification_map,
        source_key="linked_requirements",
        target_map=requirement_map,
        target_key="linked_specifications",
    )
    _assert_bidirectional(
        source_map=requirement_map,
        source_key="linked_policies",
        target_map=policy_map,
        target_key="linked_requirements",
    )
