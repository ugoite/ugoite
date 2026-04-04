"""REQ-OPS-005: Repository artifact hygiene guard behavior tests."""

from __future__ import annotations

import shutil
import subprocess
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[2]
ARTIFACT_HYGIENE_SCRIPT = REPO_ROOT / "scripts" / "check-root-artifact-hygiene.sh"
PLACEHOLDER_SCRIPT = REPO_ROOT / "scripts" / "check-placeholder-artifacts.sh"
ROOT_GITIGNORE_PATH = REPO_ROOT / ".gitignore"


def _init_temp_repo(tmp_path: Path) -> Path:
    repo_path = tmp_path / "repo"
    repo_path.mkdir()
    subprocess.run(["git", "init", "-q"], cwd=repo_path, check=True)
    scripts_dir = repo_path / "scripts"
    scripts_dir.mkdir()
    shutil.copy2(ARTIFACT_HYGIENE_SCRIPT, scripts_dir / ARTIFACT_HYGIENE_SCRIPT.name)
    shutil.copy2(PLACEHOLDER_SCRIPT, scripts_dir / PLACEHOLDER_SCRIPT.name)
    return repo_path


def _run_hygiene(repo_path: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["bash", "scripts/check-root-artifact-hygiene.sh"],
        cwd=repo_path,
        text=True,
        capture_output=True,
        check=False,
    )


def test_req_ops_005_root_artifact_hygiene_accepts_clean_repo(tmp_path: Path) -> None:
    """REQ-OPS-005: Artifact hygiene must pass for clean tracked repositories."""
    repo_path = _init_temp_repo(tmp_path)
    (repo_path / "README.md").write_text("clean repo\n", encoding="utf-8")
    subprocess.run(["git", "add", "README.md"], cwd=repo_path, check=True)

    completed = _run_hygiene(repo_path)

    assert completed.returncode == 0, completed.stderr
    assert "Repository artifact hygiene check passed." in completed.stdout


def test_req_ops_005_root_artifact_hygiene_rejects_tracked_ignored_paths(
    tmp_path: Path,
) -> None:
    """REQ-OPS-005: Artifact hygiene must fail for tracked ignored files."""
    repo_path = _init_temp_repo(tmp_path)
    (repo_path / ".gitignore").write_text("ignored.txt\n", encoding="utf-8")
    (repo_path / "ignored.txt").write_text("tracked but ignored\n", encoding="utf-8")
    subprocess.run(["git", "add", ".gitignore"], cwd=repo_path, check=True)
    subprocess.run(
        ["git", "add", "-f", "ignored.txt"],
        cwd=repo_path,
        check=True,
    )

    completed = _run_hygiene(repo_path)

    output = completed.stdout + completed.stderr
    assert completed.returncode != 0
    assert "Tracked files must not also match ignore rules:" in output
    assert "ignored.txt" in output


@pytest.mark.parametrize(
    ("tracked_path", "forbidden_segment"),
    [
        ("node_modules/pkg/file.txt", "node_modules"),
        ("target/pkg/file.txt", "target"),
    ],
    ids=["node_modules", "target"],
)
def test_req_ops_005_root_artifact_hygiene_rejects_generated_dirs(
    tmp_path: Path,
    tracked_path: str,
    forbidden_segment: str,
) -> None:
    """REQ-OPS-005: Artifact hygiene must fail for tracked generated directories."""
    repo_path = _init_temp_repo(tmp_path)
    path = repo_path / tracked_path
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("generated\n", encoding="utf-8")
    subprocess.run(["git", "add", tracked_path], cwd=repo_path, check=True)

    completed = _run_hygiene(repo_path)

    output = completed.stdout + completed.stderr
    assert completed.returncode != 0
    assert "generated dependency/build directories" in output
    assert tracked_path in output
    assert forbidden_segment in output


def test_req_ops_005_root_artifact_hygiene_rejects_oversized_tracked_files(
    tmp_path: Path,
) -> None:
    """REQ-OPS-005: Artifact hygiene must fail for oversized tracked artifacts."""
    repo_path = _init_temp_repo(tmp_path)
    (repo_path / "large.bin").write_bytes(b"x" * (1024 * 1024 + 8))
    subprocess.run(["git", "add", "large.bin"], cwd=repo_path, check=True)

    completed = _run_hygiene(repo_path)

    output = completed.stdout + completed.stderr
    assert completed.returncode != 0
    assert "Tracked files larger than 1 MiB" in output
    assert "large.bin" in output


def test_req_ops_005_root_gitignore_covers_validation_artifacts() -> None:
    """REQ-OPS-005: Root ignore rules must cover validation-generated artifacts."""
    gitignore_text = ROOT_GITIGNORE_PATH.read_text(encoding="utf-8")
    required_pattern_sets = {
        "root uv lockfile": {"/uv.lock", "uv.lock"},
        "Python bytecode caches": {"__pycache__/", "scripts/__pycache__/"},
        "docsite coverage output": {"docsite/coverage/", "/docsite/coverage/"},
    }

    missing = [
        label
        for label, patterns in required_pattern_sets.items()
        if not any(pattern in gitignore_text for pattern in patterns)
    ]
    if missing:
        details = ", ".join(missing)
        message = (
            "root .gitignore must ignore validation-generated artifacts for clean "
            f"worktrees: missing {details}"
        )
        raise AssertionError(message)
