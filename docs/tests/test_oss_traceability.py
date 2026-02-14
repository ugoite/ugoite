"""OSS governance traceability tests.

REQ-OSS-001: Policy and requirement links must be bidirectional.
REQ-OSS-002: Policy and specification links must be bidirectional.
REQ-OSS-003: Requirement and specification links must be bidirectional.
REQ-OSS-004: Governance items must not be orphaned.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

import yaml

REPO_ROOT = Path(__file__).resolve().parents[2]
PHILOSOPHY_FILE = REPO_ROOT / "docs" / "spec" / "philosophy" / "oss.yaml"
POLICY_FILE = REPO_ROOT / "docs" / "spec" / "policies" / "oss.yaml"
REQUIREMENT_FILE = REPO_ROOT / "docs" / "spec" / "requirements" / "oss.yaml"
SPECIFICATION_FILE = REPO_ROOT / "docs" / "spec" / "specifications" / "oss.yaml"


def _load_yaml(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        data = yaml.safe_load(handle)
    if not isinstance(data, dict):
        message = f"Expected YAML mapping in {path}"
        raise TypeError(message)
    return data


def _as_id_map(entries: list[dict[str, Any]], kind: str) -> dict[str, dict[str, Any]]:
    mapped: dict[str, dict[str, Any]] = {}
    for entry in entries:
        entry_id = str(entry.get("id") or "").strip()
        if not entry_id:
            message = f"{kind} entries must include id"
            raise AssertionError(message)
        if entry_id in mapped:
            message = f"Duplicate {kind} id: {entry_id}"
            raise AssertionError(message)
        mapped[entry_id] = entry
    return mapped


def _load_oss_documents() -> tuple[
    dict[str, dict[str, Any]],
    dict[str, dict[str, Any]],
    dict[str, dict[str, Any]],
    dict[str, dict[str, Any]],
]:
    philosophy_data = _load_yaml(PHILOSOPHY_FILE)
    policy_data = _load_yaml(POLICY_FILE)
    requirement_data = _load_yaml(REQUIREMENT_FILE)
    specification_data = _load_yaml(SPECIFICATION_FILE)

    philosophies = philosophy_data.get("principles")
    policies = policy_data.get("policies")
    requirements = requirement_data.get("requirements")
    specifications = specification_data.get("specifications")

    if not isinstance(philosophies, list):
        message = f"Expected principles list in {PHILOSOPHY_FILE}"
        raise TypeError(message)
    if not isinstance(policies, list):
        message = f"Expected policies list in {POLICY_FILE}"
        raise TypeError(message)
    if not isinstance(requirements, list):
        message = f"Expected requirements list in {REQUIREMENT_FILE}"
        raise TypeError(message)
    if not isinstance(specifications, list):
        message = f"Expected specifications list in {SPECIFICATION_FILE}"
        raise TypeError(message)

    return (
        _as_id_map(philosophies, "principle"),
        _as_id_map(policies, "policy"),
        _as_id_map(requirements, "requirement"),
        _as_id_map(specifications, "specification"),
    )


def _related_ids(entry: dict[str, Any], key: str, entry_id: str) -> set[str]:
    values = entry.get(key)
    if not isinstance(values, list):
        message = f"{entry_id} must define {key} as list"
        raise TypeError(message)
    as_ids = {str(item).strip() for item in values if str(item).strip()}
    if not as_ids:
        message = f"{entry_id} must include at least one {key} reference"
        raise AssertionError(message)
    return as_ids


def _require_existing(
    entries: dict[str, dict[str, Any]],
    referenced_id: str,
    source_id: str,
    target_kind: str,
) -> dict[str, Any]:
    target = entries.get(referenced_id)
    if target is None:
        message = f"{source_id} references unknown {target_kind} {referenced_id}"
        raise AssertionError(message)
    return target


def _assert_reverse_link(
    entry: dict[str, Any],
    key: str,
    entry_id: str,
    expected_reference: str,
) -> None:
    if expected_reference not in _related_ids(entry, key, entry_id):
        message = f"Missing reverse link: {entry_id}.{key} -> {expected_reference}"
        raise AssertionError(message)


def test_oss_req_001_policy_requirement_links_bidirectional() -> None:
    """REQ-OSS-001: policy and requirement links are bidirectional."""
    _philosophies, policies, requirements, _specifications = _load_oss_documents()

    for policy_id, policy in policies.items():
        policy_requirements = _related_ids(policy, "related_requirements", policy_id)
        for requirement_id in policy_requirements:
            requirement = _require_existing(
                requirements,
                requirement_id,
                policy_id,
                "requirement",
            )
            _assert_reverse_link(
                requirement,
                "related_policies",
                requirement_id,
                policy_id,
            )

    for requirement_id, requirement in requirements.items():
        requirement_policies = _related_ids(
            requirement,
            "related_policies",
            requirement_id,
        )
        for policy_id in requirement_policies:
            policy = _require_existing(
                policies,
                policy_id,
                requirement_id,
                "policy",
            )
            _assert_reverse_link(
                policy,
                "related_requirements",
                policy_id,
                requirement_id,
            )


def test_oss_req_002_policy_spec_links_bidirectional() -> None:
    """REQ-OSS-002: policy and specification links are bidirectional."""
    _philosophies, policies, _requirements, specifications = _load_oss_documents()

    for policy_id, policy in policies.items():
        policy_specs = _related_ids(policy, "related_specifications", policy_id)
        for specification_id in policy_specs:
            specification = _require_existing(
                specifications,
                specification_id,
                policy_id,
                "specification",
            )
            _assert_reverse_link(
                specification,
                "related_policies",
                specification_id,
                policy_id,
            )

    for specification_id, specification in specifications.items():
        specification_policies = _related_ids(
            specification,
            "related_policies",
            specification_id,
        )
        for policy_id in specification_policies:
            policy = _require_existing(
                policies,
                policy_id,
                specification_id,
                "policy",
            )
            _assert_reverse_link(
                policy,
                "related_specifications",
                policy_id,
                specification_id,
            )


def test_oss_req_003_requirement_spec_links_bidirectional() -> None:
    """REQ-OSS-003: requirement and specification links are bidirectional."""
    _philosophies, _policies, requirements, specifications = _load_oss_documents()

    for requirement_id, requirement in requirements.items():
        requirement_specs = _related_ids(
            requirement,
            "related_specifications",
            requirement_id,
        )
        for specification_id in requirement_specs:
            specification = _require_existing(
                specifications,
                specification_id,
                requirement_id,
                "specification",
            )
            _assert_reverse_link(
                specification,
                "related_requirements",
                specification_id,
                requirement_id,
            )

    for specification_id, specification in specifications.items():
        specification_requirements = _related_ids(
            specification,
            "related_requirements",
            specification_id,
        )
        for requirement_id in specification_requirements:
            requirement = _require_existing(
                requirements,
                requirement_id,
                specification_id,
                "requirement",
            )
            _assert_reverse_link(
                requirement,
                "related_specifications",
                requirement_id,
                specification_id,
            )


def test_oss_req_004_no_orphan_governance_items() -> None:
    """REQ-OSS-004: policies, requirements, and specifications are not orphaned."""
    _philosophies, policies, requirements, specifications = _load_oss_documents()

    for policy_id, policy in policies.items():
        _related_ids(policy, "related_requirements", policy_id)
        _related_ids(policy, "related_specifications", policy_id)

    for requirement_id, requirement in requirements.items():
        _related_ids(requirement, "related_policies", requirement_id)
        _related_ids(requirement, "related_specifications", requirement_id)

    for specification_id, specification in specifications.items():
        _related_ids(specification, "related_policies", specification_id)
        _related_ids(specification, "related_requirements", specification_id)
