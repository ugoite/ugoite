"""Storage documentation consistency tests.

REQ-STO-001: Storage docs must declare OpenDAL as the current runtime layer.
REQ-STO-007: Backend/core storage boundary docs must match the shared runtime.
"""

from __future__ import annotations

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
FRONTEND_BACKEND_INTERFACE_PATH = (
    REPO_ROOT / "docs" / "spec" / "architecture" / "frontend-backend-interface.md"
)
ARCHITECTURE_DECISIONS_PATH = (
    REPO_ROOT / "docs" / "spec" / "architecture" / "decisions.md"
)
TESTING_STRATEGY_PATH = REPO_ROOT / "docs" / "spec" / "testing" / "strategy.md"
STORAGE_REQUIREMENTS_PATH = (
    REPO_ROOT / "docs" / "spec" / "requirements" / "storage.yaml"
)


def _normalize_whitespace(text: str) -> str:
    return " ".join(text.split())


def test_docs_req_sto_001_storage_runtime_declares_opendal_current() -> None:
    """REQ-STO-001: Storage docs treat OpenDAL as the current runtime layer."""
    interface_text = _normalize_whitespace(
        FRONTEND_BACKEND_INTERFACE_PATH.read_text(encoding="utf-8"),
    )
    decisions_text = _normalize_whitespace(
        ARCHITECTURE_DECISIONS_PATH.read_text(encoding="utf-8"),
    )
    requirement_text = _normalize_whitespace(
        STORAGE_REQUIREMENTS_PATH.read_text(encoding="utf-8"),
    )

    details: list[str] = []
    if "transitioning to OpenDAL" in interface_text:
        details.append(
            "frontend-backend-interface.md must not describe the runtime as "
            "transitioning to OpenDAL",
        )
    if "OpenDAL-backed adapter" not in interface_text:
        details.append(
            "frontend-backend-interface.md must describe the current runtime "
            "adapter as OpenDAL-backed",
        )
    if "historical context only" not in interface_text:
        details.append(
            "frontend-backend-interface.md must describe fsspec as historical "
            "context only",
        )
    if (
        "fsspec" not in decisions_text
        or "no longer part of the active runtime storage architecture"
        not in decisions_text
    ):
        details.append(
            "decisions.md must describe fsspec as no longer part of the active "
            "runtime storage architecture",
        )
    if "current runtime adapter is OpenDAL-backed" not in requirement_text:
        details.append(
            "requirements/storage.yaml must state that the current runtime "
            "adapter is OpenDAL-backed",
        )
    if details:
        raise AssertionError("; ".join(details))


def test_docs_req_sto_007_backend_storage_boundary_docs_match_runtime() -> None:
    """REQ-STO-007: Shared-runtime backend docs stay aligned."""
    interface_text = _normalize_whitespace(
        FRONTEND_BACKEND_INTERFACE_PATH.read_text(encoding="utf-8"),
    )
    testing_text = _normalize_whitespace(
        TESTING_STRATEGY_PATH.read_text(encoding="utf-8"),
    )
    requirement_text = _normalize_whitespace(
        STORAGE_REQUIREMENTS_PATH.read_text(encoding="utf-8"),
    )

    details: list[str] = []
    if "must not perform direct filesystem operations" not in interface_text:
        details.append(
            "frontend-backend-interface.md must keep the backend "
            "no-direct-filesystem boundary",
        )
    if (
        "OpenDAL `fs://` (local filesystem) and `memory://` backends"
        not in interface_text
    ):
        details.append(
            "frontend-backend-interface.md must document fs:// and memory:// "
            "backend coverage",
        )
    if "Shared storage abstraction via the `memory://` backend" not in testing_text:
        details.append(
            "testing/strategy.md must describe backend tests through the "
            "shared memory backend",
        )
    if (
        "All runtime filesystem interactions MUST remain inside the shared Rust "
        "storage layer" not in requirement_text
    ):
        details.append(
            "requirements/storage.yaml must keep REQ-STO-007 scoped to the "
            "shared Rust storage layer",
        )
    if details:
        raise AssertionError("; ".join(details))
