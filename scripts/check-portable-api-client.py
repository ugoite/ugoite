#!/usr/bin/env python3
"""Fast structural validation for Ugoite's portable API client boundary."""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def fail(message: str) -> None:
    raise AssertionError(message)


def quoted_items(block: str) -> list[str]:
    return re.findall(r'"([a-z][a-z0-9_.]+)"', block)


def rust_manifest_operations(source: str) -> list[str]:
    match = re.search(
        r"pub const SUPPORTED_OPERATIONS: &\[&str\] = &\[(.*?)\];",
        source,
        re.S,
    )
    if not match:
        fail("Rust SUPPORTED_OPERATIONS manifest was not found")
    return quoted_items(match.group(1))


def typescript_manifest_operations(source: str) -> list[str]:
    match = re.search(
        r"export const UGOITE_API_OPERATIONS = \[(.*?)\] as const;",
        source,
        re.S,
    )
    if not match:
        fail("TypeScript UGOITE_API_OPERATIONS manifest was not found")
    return quoted_items(match.group(1))


def frontend_operation_uses() -> dict[str, list[str]]:
    pattern = re.compile(
        r"protocolFetch(?:<.*?>)?\s*\(\s*[\"']([^\"']+)",
        re.S,
    )
    uses: dict[str, list[str]] = {}
    for path in sorted((ROOT / "frontend/src/lib").glob("*-api.ts")):
        for operation in pattern.findall(path.read_text()):
            uses.setdefault(operation, []).append(str(path.relative_to(ROOT)))
    return uses


def cli_operation_uses() -> dict[str, list[str]]:
    pattern = re.compile(
        r"http::execute\s*\(\s*[^,]+,\s*[\"']([^\"']+)",
        re.S,
    )
    uses: dict[str, list[str]] = {}
    for path in sorted((ROOT / "crates/ugoite-cli/src/commands").glob("*.rs")):
        for operation in pattern.findall(path.read_text()):
            uses.setdefault(operation, []).append(str(path.relative_to(ROOT)))
    return uses


def dependency_keys_from_manifest(path: Path) -> set[str]:
    dependencies: set[str] = set()
    in_dependencies = False
    for raw_line in path.read_text().splitlines():
        line = raw_line.strip()
        if line.startswith("["):
            in_dependencies = line == "[dependencies]"
            continue
        if not in_dependencies or not line or line.startswith("#") or "=" not in line:
            continue
        key = line.split("=", 1)[0].strip()
        if key:
            dependencies.add(key)
    return dependencies


def check_cargo_manifests() -> None:
    for path in ROOT.glob("**/Cargo.toml"):
        path.read_text()
    (ROOT / "Cargo.lock").read_text()

    api_dependencies = dependency_keys_from_manifest(
        ROOT / "crates/ugoite-api-client/Cargo.toml"
    )
    forbidden_api = {
        "axum",
        "opendal",
        "reqwest",
        "tokio",
        "ugoite-core",
        "ugoite-domain",
        "ugoite-storage",
        "wasm-bindgen",
        "web-sys",
    }
    unexpected = sorted(api_dependencies & forbidden_api)
    if unexpected:
        fail(f"ugoite-api-client has forbidden dependencies: {unexpected}")

    wasm_dependencies = dependency_keys_from_manifest(
        ROOT / "crates/ugoite-wasm/Cargo.toml"
    )
    forbidden_wasm = {
        "opendal",
        "reqwest",
        "tokio",
        "ugoite-core",
        "ugoite-storage",
    }
    unexpected = sorted(wasm_dependencies & forbidden_wasm)
    if unexpected:
        fail(f"ugoite-wasm has forbidden dependencies: {unexpected}")


def check_source_boundaries() -> None:
    for path in sorted((ROOT / "frontend/src/lib").glob("*-api.ts")):
        content = path.read_text()
        if "apiFetch" in content:
            fail(f"{path.relative_to(ROOT)} imports or calls apiFetch directly")

    for path in sorted((ROOT / "crates/ugoite-cli/src/commands").glob("*.rs")):
        content = path.read_text()
        forbidden = [
            "http::http_get",
            "http::http_post",
            "http::http_put",
            "http::http_patch",
            "http::http_delete",
            'format!("{base}/',
        ]
        for token in forbidden:
            if token in content:
                fail(
                    f"{path.relative_to(ROOT)} bypasses the portable operation boundary via {token}"
                )


def main() -> int:
    rust_source = (ROOT / "crates/ugoite-api-client/src/lib.rs").read_text()
    ts_source = (ROOT / "frontend/src/lib/ugoite-client/protocol.ts").read_text()
    rust_operations = rust_manifest_operations(rust_source)
    ts_operations = typescript_manifest_operations(ts_source)

    if len(rust_operations) != len(set(rust_operations)):
        fail("Rust operation manifest contains duplicates")
    if len(ts_operations) != len(set(ts_operations)):
        fail("TypeScript operation manifest contains duplicates")
    if rust_operations != ts_operations:
        rust_only = sorted(set(rust_operations) - set(ts_operations))
        ts_only = sorted(set(ts_operations) - set(rust_operations))
        fail(
            "Rust/TypeScript operation manifests differ: "
            f"rust_only={rust_only}, ts_only={ts_only}"
        )

    supported = set(rust_operations)
    for label, uses in [
        ("frontend", frontend_operation_uses()),
        ("CLI", cli_operation_uses()),
    ]:
        unknown = sorted(set(uses) - supported)
        if unknown:
            fail(f"{label} uses unknown portable operations: {unknown}")

    prepare_section = rust_source.split("pub fn prepare_request", 1)[1].split(
        "pub fn decode_response", 1
    )[0]
    decode_section = rust_source.split("fn operation_spec", 1)[1].split(
        "fn required_string", 1
    )[0]
    for operation in rust_operations:
        token = f'"{operation}" =>'
        if token not in prepare_section:
            fail(f"prepare_request is missing {operation}")
        if token not in decode_section:
            fail(f"operation_spec is missing {operation}")

    check_cargo_manifests()
    check_source_boundaries()

    print(
        "portable API client structural validation passed: "
        f"{len(rust_operations)} operations, "
        f"{len(frontend_operation_uses())} used by frontend, "
        f"{len(cli_operation_uses())} used by CLI"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except AssertionError as error:
        print(f"portable API client structural validation failed: {error}", file=sys.stderr)
        raise SystemExit(1)
