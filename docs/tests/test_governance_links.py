"""Governance relationship consistency tests.

REQ-API-005: Requirements and specification mappings must be machine-validated.
"""

from __future__ import annotations

from dataclasses import dataclass
from fnmatch import fnmatch
from pathlib import Path
from typing import Any

import yaml

REPO_ROOT = Path(__file__).resolve().parents[2]
REQUIREMENTS_DIR = REPO_ROOT / "docs" / "spec" / "requirements"
POLICIES_FILE = REPO_ROOT / "docs" / "spec" / "policies" / "policies.yaml"
SPECS_FILE = REPO_ROOT / "docs" / "spec" / "specifications" / "specifications.yaml"
REQ_LINKS_FILE = (
    REPO_ROOT / "docs" / "spec" / "specifications" / "requirements-links.yaml"
)
SPEC_ROOT = REPO_ROOT / "docs" / "spec"

IdSetMap = dict[str, set[str]]


@dataclass(frozen=True)
class Policy:
    """Policy row parsed from the policy catalog."""

    policy_id: str
    related_requirements: tuple[str, ...]
    related_specifications: tuple[str, ...]


@dataclass(frozen=True)
class Specification:
    """Specification row parsed from the specification catalog."""

    spec_id: str
    sources: tuple[str, ...]
    related_policies: tuple[str, ...]
    related_requirements: tuple[str, ...]


@dataclass(frozen=True)
class RequirementLink:
    """Requirement selector row parsed from requirement link map."""

    requirement_selector: str
    related_policies: tuple[str, ...]
    related_specifications: tuple[str, ...]


@dataclass(frozen=True)
class GovernanceGraph:
    """Expanded relationships used by reciprocity checks."""

    requirement_ids: set[str]
    policy_ids: set[str]
    specification_ids: set[str]
    requirement_to_policies: IdSetMap
    requirement_to_specifications: IdSetMap
    policy_to_requirements: IdSetMap
    policy_to_specifications: IdSetMap
    specification_to_requirements: IdSetMap
    specification_to_policies: IdSetMap


def _load_yaml(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        data = yaml.safe_load(handle)
    if not isinstance(data, dict):
        message = f"Expected YAML mapping in {path}"
        raise TypeError(message)
    return data


def _all_requirement_ids() -> set[str]:
    requirement_ids: set[str] = set()
    for path in sorted(REQUIREMENTS_DIR.glob("*.yaml")):
        data = _load_yaml(path)
        requirements = data.get("requirements")
        if not isinstance(requirements, list):
            continue
        for requirement in requirements:
            if not isinstance(requirement, dict):
                continue
            req_id = str(requirement.get("id") or "").strip()
            if req_id:
                requirement_ids.add(req_id)
    if not requirement_ids:
        message = "No requirement IDs found in docs/spec/requirements"
        raise AssertionError(message)
    return requirement_ids


def _expand_selector(selector: str, requirement_ids: set[str]) -> set[str]:
    return {req_id for req_id in requirement_ids if fnmatch(req_id, selector)}


def _init_map(keys: set[str]) -> IdSetMap:
    return {key: set() for key in keys}


def _assert_selector_matches(
    selector: str,
    requirement_ids: set[str],
    owner: str,
) -> set[str]:
    matched = _expand_selector(selector, requirement_ids)
    if matched:
        return matched
    message = f"{owner} selector {selector} matches no requirements"
    raise AssertionError(message)


def _assert_known_ids(
    owner: str,
    related: set[str],
    valid: set[str],
    kind: str,
) -> None:
    unknown = related - valid
    if unknown:
        message = f"{owner} references unknown {kind}: {sorted(unknown)}"
        raise AssertionError(message)


def _load_policies() -> tuple[Policy, ...]:
    data = _load_yaml(POLICIES_FILE)
    entries = data.get("policies")
    if not isinstance(entries, list) or not entries:
        message = f"Expected non-empty policies list in {POLICIES_FILE}"
        raise AssertionError(message)

    policies: list[Policy] = []
    for entry in entries:
        if not isinstance(entry, dict):
            message = f"Policy entries must be mappings in {POLICIES_FILE}"
            raise TypeError(message)
        policy_id = str(entry.get("id") or "").strip()
        related_requirements = entry.get("related_requirements")
        related_specifications = entry.get("related_specifications")
        if (
            not policy_id
            or not isinstance(related_requirements, list)
            or not isinstance(related_specifications, list)
        ):
            message = f"Invalid policy entry in {POLICIES_FILE}: {entry}"
            raise AssertionError(message)
        policies.append(
            Policy(
                policy_id=policy_id,
                related_requirements=tuple(str(item) for item in related_requirements),
                related_specifications=tuple(
                    str(item) for item in related_specifications
                ),
            ),
        )
    return tuple(policies)


def _load_specifications() -> tuple[Specification, ...]:
    data = _load_yaml(SPECS_FILE)
    entries = data.get("specifications")
    if not isinstance(entries, list) or not entries:
        message = f"Expected non-empty specifications list in {SPECS_FILE}"
        raise AssertionError(message)

    specifications: list[Specification] = []
    for entry in entries:
        if not isinstance(entry, dict):
            message = f"Specification entries must be mappings in {SPECS_FILE}"
            raise TypeError(message)
        spec_id = str(entry.get("id") or "").strip()
        sources = entry.get("sources")
        related_policies = entry.get("related_policies")
        related_requirements = entry.get("related_requirements")
        if (
            not spec_id
            or not isinstance(sources, list)
            or not isinstance(related_policies, list)
            or not isinstance(related_requirements, list)
        ):
            message = f"Invalid specification entry in {SPECS_FILE}: {entry}"
            raise AssertionError(message)
        specifications.append(
            Specification(
                spec_id=spec_id,
                sources=tuple(str(source) for source in sources),
                related_policies=tuple(str(item) for item in related_policies),
                related_requirements=tuple(str(item) for item in related_requirements),
            ),
        )
    return tuple(specifications)


def _load_requirement_links() -> tuple[RequirementLink, ...]:
    data = _load_yaml(REQ_LINKS_FILE)
    entries = data.get("requirement_links")
    if not isinstance(entries, list) or not entries:
        message = f"Expected non-empty requirement_links in {REQ_LINKS_FILE}"
        raise AssertionError(message)

    links: list[RequirementLink] = []
    for entry in entries:
        if not isinstance(entry, dict):
            message = f"Requirement link entries must be mappings in {REQ_LINKS_FILE}"
            raise TypeError(message)
        selector = str(entry.get("requirement_selector") or "").strip()
        related_policies = entry.get("related_policies")
        related_specifications = entry.get("related_specifications")
        if (
            not selector
            or not isinstance(related_policies, list)
            or not isinstance(related_specifications, list)
        ):
            message = f"Invalid requirement link entry in {REQ_LINKS_FILE}: {entry}"
            raise AssertionError(message)
        links.append(
            RequirementLink(
                requirement_selector=selector,
                related_policies=tuple(str(item) for item in related_policies),
                related_specifications=tuple(
                    str(item) for item in related_specifications
                ),
            ),
        )
    return tuple(links)


def _collect_policy_maps(
    policies: tuple[Policy, ...],
    requirement_ids: set[str],
    specification_ids: set[str],
) -> tuple[IdSetMap, IdSetMap]:
    policy_to_requirements = _init_map({policy.policy_id for policy in policies})
    policy_to_specifications = _init_map({policy.policy_id for policy in policies})
    for policy in policies:
        if not policy.related_requirements or not policy.related_specifications:
            message = (
                f"Policy {policy.policy_id} must reference requirements and "
                "specifications"
            )
            raise AssertionError(message)
        for selector in policy.related_requirements:
            matched = _assert_selector_matches(
                selector,
                requirement_ids,
                f"Policy {policy.policy_id}",
            )
            policy_to_requirements[policy.policy_id].update(matched)
        _assert_known_ids(
            f"Policy {policy.policy_id}",
            set(policy.related_specifications),
            specification_ids,
            "specifications",
        )
        policy_to_specifications[policy.policy_id].update(policy.related_specifications)
    return policy_to_requirements, policy_to_specifications


def _collect_specification_maps(
    specifications: tuple[Specification, ...],
    requirement_ids: set[str],
    policy_ids: set[str],
) -> tuple[IdSetMap, IdSetMap]:
    specification_to_requirements = _init_map(
        {spec.spec_id for spec in specifications},
    )
    specification_to_policies = _init_map({spec.spec_id for spec in specifications})
    for specification in specifications:
        if not specification.related_policies or not specification.related_requirements:
            message = (
                f"Specification {specification.spec_id} must reference policies "
                "and requirements"
            )
            raise AssertionError(message)
        for source in specification.sources:
            if not (SPEC_ROOT / source).exists():
                message = (
                    f"Specification {specification.spec_id} references missing "
                    f"source {source}"
                )
                raise AssertionError(message)
        _assert_known_ids(
            f"Specification {specification.spec_id}",
            set(specification.related_policies),
            policy_ids,
            "policies",
        )
        specification_to_policies[specification.spec_id].update(
            specification.related_policies,
        )
        for selector in specification.related_requirements:
            matched = _assert_selector_matches(
                selector,
                requirement_ids,
                f"Specification {specification.spec_id}",
            )
            specification_to_requirements[specification.spec_id].update(matched)
    return specification_to_requirements, specification_to_policies


def _collect_requirement_maps(
    requirement_links: tuple[RequirementLink, ...],
    requirement_ids: set[str],
    policy_ids: set[str],
    specification_ids: set[str],
) -> tuple[IdSetMap, IdSetMap]:
    requirement_to_policies = _init_map(requirement_ids)
    requirement_to_specifications = _init_map(requirement_ids)
    for link in requirement_links:
        matched = _assert_selector_matches(
            link.requirement_selector,
            requirement_ids,
            "Requirement",
        )
        _assert_known_ids(
            f"Requirement selector {link.requirement_selector}",
            set(link.related_policies),
            policy_ids,
            "policies",
        )
        _assert_known_ids(
            f"Requirement selector {link.requirement_selector}",
            set(link.related_specifications),
            specification_ids,
            "specifications",
        )
        for requirement_id in matched:
            requirement_to_policies[requirement_id].update(link.related_policies)
            requirement_to_specifications[requirement_id].update(
                link.related_specifications,
            )
    return requirement_to_policies, requirement_to_specifications


def _assert_requirements_are_linked(
    requirement_ids: set[str],
    requirement_to_policies: IdSetMap,
    requirement_to_specifications: IdSetMap,
) -> None:
    missing = [
        requirement_id
        for requirement_id in sorted(requirement_ids)
        if not requirement_to_policies[requirement_id]
        or not requirement_to_specifications[requirement_id]
    ]
    if missing:
        message = (
            "Requirements missing policy/spec links in requirements-links.yaml: "
            f"{missing}"
        )
        raise AssertionError(message)


def _assert_reciprocal(
    left_name: str,
    right_name: str,
    left_to_right: IdSetMap,
    right_to_left: IdSetMap,
) -> None:
    for left_id, right_ids in left_to_right.items():
        for right_id in right_ids:
            if left_id in right_to_left[right_id]:
                continue
            message = (
                f"{left_name} {left_id} links to {right_name} {right_id}, "
                f"but {right_name} does not link back"
            )
            raise AssertionError(message)


def _assert_no_orphans(name: str, left: IdSetMap, right: IdSetMap) -> None:
    orphans = [
        item_id for item_id in sorted(left) if not left[item_id] or not right[item_id]
    ]
    if orphans:
        message = f"Orphan {name} found: {orphans}"
        raise AssertionError(message)


def _build_graph() -> GovernanceGraph:
    requirement_ids = _all_requirement_ids()
    policies = _load_policies()
    specifications = _load_specifications()
    requirement_links = _load_requirement_links()

    policy_ids = {policy.policy_id for policy in policies}
    specification_ids = {spec.spec_id for spec in specifications}

    policy_to_requirements, policy_to_specifications = _collect_policy_maps(
        policies,
        requirement_ids,
        specification_ids,
    )
    specification_to_requirements, specification_to_policies = (
        _collect_specification_maps(
            specifications,
            requirement_ids,
            policy_ids,
        )
    )
    requirement_to_policies, requirement_to_specifications = _collect_requirement_maps(
        requirement_links,
        requirement_ids,
        policy_ids,
        specification_ids,
    )
    _assert_requirements_are_linked(
        requirement_ids,
        requirement_to_policies,
        requirement_to_specifications,
    )

    return GovernanceGraph(
        requirement_ids=requirement_ids,
        policy_ids=policy_ids,
        specification_ids=specification_ids,
        requirement_to_policies=requirement_to_policies,
        requirement_to_specifications=requirement_to_specifications,
        policy_to_requirements=policy_to_requirements,
        policy_to_specifications=policy_to_specifications,
        specification_to_requirements=specification_to_requirements,
        specification_to_policies=specification_to_policies,
    )


def test_governance_bidirectional_links() -> None:
    """REQ-API-005: Policy/Requirement/Specification links are bidirectional."""
    graph = _build_graph()

    _assert_reciprocal(
        "Requirement",
        "policy",
        graph.requirement_to_policies,
        graph.policy_to_requirements,
    )
    _assert_reciprocal(
        "Policy",
        "requirement",
        graph.policy_to_requirements,
        graph.requirement_to_policies,
    )
    _assert_reciprocal(
        "Requirement",
        "specification",
        graph.requirement_to_specifications,
        graph.specification_to_requirements,
    )
    _assert_reciprocal(
        "Specification",
        "requirement",
        graph.specification_to_requirements,
        graph.requirement_to_specifications,
    )
    _assert_reciprocal(
        "Policy",
        "specification",
        graph.policy_to_specifications,
        graph.specification_to_policies,
    )
    _assert_reciprocal(
        "Specification",
        "policy",
        graph.specification_to_policies,
        graph.policy_to_specifications,
    )
    _assert_no_orphans(
        "policies",
        graph.policy_to_requirements,
        graph.policy_to_specifications,
    )
    _assert_no_orphans(
        "specifications",
        graph.specification_to_requirements,
        graph.specification_to_policies,
    )
