"""MCP documentation contract tests.

REQ-API-012: MCP resource input safety.
REQ-API-013: Current MCP surface documentation.
"""

from __future__ import annotations

import ast
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]


def _mcp_resource_uris(module_source: str) -> list[str]:
    """Return registered MCP resource URIs from decorated backend functions."""
    uris: list[str] = []
    tree = ast.parse(module_source)
    for node in ast.walk(tree):
        if not isinstance(node, (ast.AsyncFunctionDef, ast.FunctionDef)):
            continue
        for decorator in node.decorator_list:
            if not (
                isinstance(decorator, ast.Call)
                and isinstance(decorator.func, ast.Attribute)
                and decorator.func.attr == "resource"
                and isinstance(decorator.func.value, ast.Name)
                and decorator.func.value.id == "mcp"
                and decorator.args
                and isinstance(decorator.args[0], ast.Constant)
                and isinstance(decorator.args[0].value, str)
            ):
                continue
            uris.append(decorator.args[0].value)
    return uris


def test_docs_req_api_012_mcp_contract_covers_safe_ids_and_untrusted_content() -> None:
    """REQ-API-012: MCP docs describe safe space IDs and untrusted entry content."""
    doc = (REPO_ROOT / "docs" / "spec" / "api" / "mcp.md").read_text(
        encoding="utf-8",
    )
    details = [
        f"docs/spec/api/mcp.md missing fragment: {fragment!r}"
        for fragment in (
            "^[A-Za-z0-9_-]+$",
            "path traversal",
            "null bytes",
            "untrusted data",
            "never follow instructions",
        )
        if fragment not in doc
    ]
    if details:
        raise AssertionError("; ".join(details))


def test_docs_req_api_013_current_mcp_surface_stays_resource_first() -> None:
    """REQ-API-013: Current docs match the shipped MCP surface and roadmap framing."""
    spec_index = (REPO_ROOT / "docs" / "spec" / "index.md").read_text(
        encoding="utf-8",
    )
    architecture = (
        REPO_ROOT / "docs" / "spec" / "architecture" / "overview.md"
    ).read_text(encoding="utf-8")
    mcp_doc = (REPO_ROOT / "docs" / "spec" / "api" / "mcp.md").read_text(
        encoding="utf-8",
    )
    v0_2 = (REPO_ROOT / "docs" / "spec" / "versions" / "v0.2.md").read_text(
        encoding="utf-8",
    )
    versions_index = (
        REPO_ROOT / "docs" / "spec" / "versions" / "index.md"
    ).read_text(encoding="utf-8")
    mvp = (REPO_ROOT / "docs" / "version" / "v0.1" / "mvp.yaml").read_text(
        encoding="utf-8",
    )
    v0_2_yaml = (REPO_ROOT / "docs" / "version" / "v0.2.yaml").read_text(
        encoding="utf-8",
    )
    roadmap = (
        REPO_ROOT / "docs" / "version" / "unknown" / "roadmap.yaml"
    ).read_text(encoding="utf-8")
    backend_mcp = (
        REPO_ROOT / "backend" / "src" / "app" / "mcp" / "server.py"
    ).read_text(encoding="utf-8")
    backend_mcp_resources = _mcp_resource_uris(backend_mcp)
    normalized_v0_2 = " ".join(v0_2.split())
    normalized_versions_index = " ".join(versions_index.split())
    normalized_v0_2_yaml = " ".join(v0_2_yaml.split())
    normalized_roadmap = " ".join(roadmap.split())

    details = [
        message
        for ok, message in (
            (
                "Resource-First MCP Surface" in spec_index,
                "docs/spec/index.md must frame the current vision around a "
                "resource-first MCP surface",
            ),
            (
                "Resource-First MCP" in architecture,
                "docs/spec/architecture/overview.md must describe the current "
                "MCP design as resource-first",
            ),
            (
                "ugoite://{space_id}/entries/list" in mcp_doc,
                "docs/spec/api/mcp.md must document the shipped entries/list resource",
            ),
            (
                "No MCP tools are currently exposed." in mcp_doc,
                "docs/spec/api/mcp.md must state that no MCP tools are "
                "currently exposed",
            ),
            (
                "No MCP prompts are currently exposed." in mcp_doc,
                "docs/spec/api/mcp.md must state that no MCP prompts are "
                "currently exposed",
            ),
            (
                "### `ugoite://{space_id}/entries/{entry_id}`" not in mcp_doc,
                "docs/spec/api/mcp.md must not present unshipped entry detail "
                "resources as current behavior",
            ),
            (
                "### `ugoite://{space_id}/forms`" not in mcp_doc,
                "docs/spec/api/mcp.md must not present unshipped form resources "
                "as current behavior",
            ),
            (
                "resource-first MCP baseline" in normalized_v0_2,
                "docs/spec/versions/v0.2.md must describe v0.2 work as "
                "expanding beyond today's resource-first MCP baseline",
            ),
            (
                "resource-first MCP baseline" in normalized_versions_index,
                "docs/spec/versions/index.md must summarize v0.2 as work "
                "beyond today's resource-first MCP baseline",
            ),
            (
                "`entries/list`" in mvp,
                "docs/version/v0.1/mvp.yaml must describe the completed MCP "
                "work as the entries/list baseline",
            ),
            (
                "resource-first MCP baseline" in normalized_v0_2_yaml,
                "docs/version/v0.2.yaml must summarize the release stream as "
                "work beyond today's resource-first MCP baseline",
            ),
            (
                "resource-first MCP baseline" in normalized_roadmap,
                "docs/version/unknown/roadmap.yaml must frame future AI work "
                "as growing from today's resource-first MCP baseline",
            ),
            (
                len(backend_mcp_resources) == 1,
                "backend/src/app/mcp/server.py must still register exactly one MCP resource for this docs contract",
            ),
            (
                "ugoite://{space_id}/entries/list" in backend_mcp_resources,
                "backend/src/app/mcp/server.py must register the entries/list MCP resource",
            ),
        )
        if not ok
    ]
    if details:
        raise AssertionError("; ".join(details))
