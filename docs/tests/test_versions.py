"""Version roadmap consistency tests.

REQ-OPS-004: Version milestones must be YAML-defined with phase metadata.
"""

from __future__ import annotations

from pathlib import Path
from typing import Never

import yaml

REPO_ROOT = Path(__file__).resolve().parents[2]
VERSION_DIR = REPO_ROOT / "docs" / "version"
ISSUE_TEMPLATE_DIR = REPO_ROOT / ".github" / "ISSUE_TEMPLATE"


def _fail(message: str) -> Never:
    raise AssertionError(message)


def _load_yaml(path: Path) -> object:
    with path.open("r", encoding="utf-8") as handle:
        return yaml.safe_load(handle)


def _load_version(path: Path) -> dict[str, object]:
    loaded = _load_yaml(path)
    if not isinstance(loaded, dict):
        _fail(f"{path.relative_to(REPO_ROOT)} must be a YAML mapping")
    return loaded


def _milestones(version_doc: dict[str, object], label: str) -> list[dict[str, object]]:
    milestones = version_doc.get("milestones")
    if not isinstance(milestones, list) or not milestones:
        _fail(f"{label} must define non-empty milestones")
    normalized: list[dict[str, object]] = []
    for milestone in milestones:
        if not isinstance(milestone, dict):
            _fail(f"{label} milestones entries must be mappings")
        normalized.append(milestone)
    return normalized


def _phase_ids(milestone: dict[str, object], context: str) -> set[str]:
    phases = milestone.get("phases")
    if not isinstance(phases, list) or not phases:
        _fail(f"{context} must define phases")
    phase_ids = {
        str(phase.get("id") or "").strip()
        for phase in phases
        if isinstance(phase, dict)
    }
    if "" in phase_ids:
        _fail(f"{context} contains phase with missing id")
    return phase_ids


def test_docs_req_ops_004_version_files_exist() -> None:
    """REQ-OPS-004: v0.1/v0.2 milestone YAMLs must exist."""
    expected = {VERSION_DIR / "v0.1.yaml", VERSION_DIR / "v0.2.yaml"}
    missing = [path for path in expected if not path.exists()]
    if missing:
        formatted = ", ".join(str(path.relative_to(REPO_ROOT)) for path in missing)
        _fail(f"Missing version milestone files: {formatted}")


def test_docs_req_ops_004_milestones_are_unnumbered_and_phased() -> None:
    """REQ-OPS-004: Milestones must be unnumbered IDs with required phases."""
    required_phase_ids = {"design", "implementation", "testing"}
    for version_path in sorted(VERSION_DIR.glob("v*.yaml")):
        data = _load_version(version_path)
        for milestone in _milestones(data, version_path.name):
            milestone_id = str(milestone.get("id") or "").strip()
            if not milestone_id:
                _fail(f"{version_path.name} has milestone without id")
            if any(ch.isdigit() for ch in milestone_id):
                _fail(
                    f"{version_path.name} milestone id must be unnumbered: "
                    f"{milestone_id}",
                )
            if milestone_id == "release-preparation":
                _phase_ids(milestone, f"{version_path.name}:{milestone_id}")
                continue
            phase_ids = _phase_ids(milestone, f"{version_path.name}:{milestone_id}")
            if not required_phase_ids.issubset(phase_ids):
                _fail(
                    f"{version_path.name} milestone {milestone_id} missing "
                    "required phases",
                )


def test_docs_req_ops_004_release_preparation_is_explicit() -> None:
    """REQ-OPS-004: v0.1 must define explicit release-preparation phases."""
    data = _load_version(VERSION_DIR / "v0.1.yaml")
    milestones = _milestones(data, "v0.1.yaml")
    release = next(
        (m for m in milestones if m.get("id") == "release-preparation"),
        None,
    )
    if not isinstance(release, dict):
        _fail("v0.1.yaml must include release-preparation milestone")
    phase_ids = _phase_ids(release, "v0.1.yaml:release-preparation")
    required = {
        "container-image-deployment",
        "quickstart-documentation-finalization",
        "release-notes-drafting",
    }
    missing = sorted(required.difference(phase_ids))
    if missing:
        _fail("release-preparation missing phases: " + ", ".join(missing))


def test_docs_req_ops_004_v02_contains_target_milestones() -> None:
    """REQ-OPS-004: v0.2 must include user-controlled and AI-enabled milestones."""
    data = _load_version(VERSION_DIR / "v0.2.yaml")
    ids = {str(m.get("id") or "").strip() for m in _milestones(data, "v0.2.yaml")}
    required = {"user-controlled-view", "ai-enabled-and-ai-used"}
    missing = sorted(required.difference(ids))
    if missing:
        _fail("v0.2.yaml missing milestones: " + ", ".join(missing))


def test_docs_req_ops_004_issue_templates_support_phase() -> None:
    """REQ-OPS-004: Issue templates must include issue_type and phase fields."""
    for template_name in ("bug_report.yml", "feature_request.yml"):
        template = _load_yaml(ISSUE_TEMPLATE_DIR / template_name)
        if not isinstance(template, dict):
            _fail(f"{template_name} must be a YAML mapping")
        body = template.get("body")
        if not isinstance(body, list):
            _fail(f"{template_name} must define body list")
        ids = {
            str(item.get("id") or "")
            for item in body
            if isinstance(item, dict) and item.get("id")
        }
        if "issue_type" not in ids or "phase" not in ids:
            _fail(f"{template_name} must include issue_type and phase fields")
