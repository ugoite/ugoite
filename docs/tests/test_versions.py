"""Version roadmap consistency tests.

REQ-OPS-004: Version milestones must be YAML-defined with phase metadata.
REQ-OPS-026: Release changelog sources must stay channel-scoped.
"""

from __future__ import annotations

from pathlib import Path
from typing import Never

import yaml

REPO_ROOT = Path(__file__).resolve().parents[2]
VERSION_DIR = REPO_ROOT / "docs" / "version"
RELEASE_CHANGELOG_DIR = VERSION_DIR / "changelog"
ISSUE_TEMPLATE_DIR = REPO_ROOT / ".github" / "ISSUE_TEMPLATE"
README_PATH = REPO_ROOT / "README.md"
REQUIRED_README_VERSION_DOC_FRAGMENTS = {
    "[Versions Overview](docs/spec/versions/index.md)",
    "[v0.1 release stream](docs/spec/versions/v0.1.md)",
    "[v0.2 roadmap](docs/spec/versions/v0.2.md)",
}
FORBIDDEN_README_ROADMAP_FRAGMENTS = {
    "docs/tasks/roadmap.md",
    "- **Milestone 2** (Completed): Codebase unification, Rust core library",
    "- **Milestone 3**: Full AI integration, vector search",
    (
        "- **Milestone 4** (Phase 1/2 completed): User management, authentication "
        "hardening and follow-up tasks"
    ),
    "- **Milestone 5**: Native desktop app (Tauri)",
}


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


def test_docs_req_ops_004_milestone_yaml_files_exist() -> None:
    """REQ-OPS-004: Source files referenced in version milestones must exist."""
    for version_path in sorted(VERSION_DIR.glob("v*.yaml")):
        data = _load_version(version_path)
        for milestone in _milestones(data, version_path.name):
            source = milestone.get("source")
            if source is None:
                continue
            if isinstance(source, str):
                sources: list[object] = [source]
            elif isinstance(source, list):
                sources = list(source)
            else:
                _fail(
                    f"{version_path.name} milestone {milestone.get('id')!r} "
                    "source must be a string or a list of strings",
                )
                continue
            for src in sources:
                if not isinstance(src, str) or not src.strip():
                    _fail(
                        f"{version_path.name} milestone {milestone.get('id')!r} "
                        "source entries must be non-empty strings",
                    )
                    continue
                src_path = REPO_ROOT / src
                if not src_path.exists():
                    _fail(
                        f"{version_path.name} milestone {milestone.get('id')!r} "
                        f"source file not found: {src}",
                    )


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


def test_docs_req_ops_004_readme_roadmap_points_to_canonical_version_docs() -> None:
    """REQ-OPS-004: README roadmap pointers must use canonical version docs."""
    readme_text = README_PATH.read_text(encoding="utf-8")

    missing = sorted(
        fragment
        for fragment in REQUIRED_README_VERSION_DOC_FRAGMENTS
        if fragment not in readme_text
    )
    if missing:
        _fail(
            "README.md missing canonical version roadmap references: "
            + ", ".join(missing),
        )

    forbidden = sorted(
        fragment
        for fragment in FORBIDDEN_README_ROADMAP_FRAGMENTS
        if fragment in readme_text
    )
    if forbidden:
        _fail(
            "README.md still includes stale roadmap fragments: " + ", ".join(forbidden),
        )


def test_docs_req_ops_026_release_channel_changelog_sources_exist() -> None:
    """REQ-OPS-026: Stable/beta/alpha changelog sources and docs must exist."""
    for channel in ("stable", "beta", "alpha"):
        changelog_path = RELEASE_CHANGELOG_DIR / f"{channel}.yaml"
        data = _load_version(changelog_path)
        if str(data.get("channel") or "").strip() != channel:
            _fail(
                f"{changelog_path.relative_to(REPO_ROOT)} must declare "
                f"channel={channel}",
            )

        doc_path = str(data.get("doc_path") or "").strip()
        if not doc_path:
            _fail(f"{changelog_path.relative_to(REPO_ROOT)} must define doc_path")
        full_doc_path = REPO_ROOT / doc_path
        if not full_doc_path.exists():
            _fail(
                f"{changelog_path.relative_to(REPO_ROOT)} doc_path not found: "
                f"{doc_path}",
            )

        release_notes = data.get("release_notes")
        if not isinstance(release_notes, dict):
            _fail(f"{changelog_path.relative_to(REPO_ROOT)} must define release_notes")

        intro = str(release_notes.get("intro") or "").strip()
        if not intro:
            _fail(
                f"{changelog_path.relative_to(REPO_ROOT)} "
                "release_notes.intro is required",
            )

        for key in ("expectations", "added", "changed", "planned"):
            values = release_notes.get(key)
            if not isinstance(values, list) or not values:
                _fail(
                    f"{changelog_path.relative_to(REPO_ROOT)} release_notes.{key} "
                    "must be a non-empty list",
                )
            if any(not isinstance(value, str) or not value.strip() for value in values):
                _fail(
                    f"{changelog_path.relative_to(REPO_ROOT)} release_notes.{key} "
                    "must only contain non-empty strings",
                )


def test_docs_req_ops_026_release_preparation_tracks_channel_drafting() -> None:
    """REQ-OPS-026: Release preparation must track stable/beta/alpha drafting tasks."""
    data = _load_version(VERSION_DIR / "v0.1" / "release-preparation.yaml")
    phases = data.get("phases")
    if not isinstance(phases, list):
        _fail("docs/version/v0.1/release-preparation.yaml must define phases")

    release_notes_phase = next(
        (
            phase
            for phase in phases
            if isinstance(phase, dict) and phase.get("id") == "release-notes-drafting"
        ),
        None,
    )
    if not isinstance(release_notes_phase, dict):
        _fail("release-preparation must define a release-notes-drafting phase")

    tasks = release_notes_phase.get("tasks")
    if not isinstance(tasks, list):
        _fail("release-notes-drafting must define tasks")

    descriptions = {
        str(task.get("description") or "").strip()
        for task in tasks
        if isinstance(task, dict)
    }
    required = {
        "Prepare stable release notes and operator guidance",
        "Prepare beta release notes and prerelease validation guidance",
        "Prepare alpha release notes and experimental guidance",
    }
    missing = sorted(required.difference(descriptions))
    if missing:
        _fail("release-notes-drafting missing tasks: " + ", ".join(missing))
