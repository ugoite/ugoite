"""REQ-OPS-017 regression coverage for release compose auth defaults."""

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
RELEASE_COMPOSE = REPO_ROOT / "docker-compose.release.yaml"
CONTAINER_QUICKSTART = REPO_ROOT / "docs" / "guide" / "container-quickstart.md"


def test_docs_req_ops_017_release_compose_requires_install_specific_auth_values() -> (
    None
):
    """REQ-OPS-017: release compose requires install-specific auth values."""
    release_compose_text = RELEASE_COMPOSE.read_text(encoding="utf-8")
    quickstart_text = CONTAINER_QUICKSTART.read_text(encoding="utf-8")
    required_release_fragments = (
        "UGOITE_DEV_AUTH_MODE=${UGOITE_DEV_AUTH_MODE:-passkey-totp}",
        (
            "UGOITE_DEV_SIGNING_SECRET="
            "${UGOITE_DEV_SIGNING_SECRET:?set UGOITE_DEV_SIGNING_SECRET}"
        ),
        (
            "UGOITE_AUTH_BEARER_SECRETS="
            "${UGOITE_AUTH_BEARER_SECRETS:?set UGOITE_AUTH_BEARER_SECRETS}"
        ),
        (
            "UGOITE_DEV_AUTH_PROXY_TOKEN="
            "${UGOITE_DEV_AUTH_PROXY_TOKEN:?set UGOITE_DEV_AUTH_PROXY_TOKEN}"
        ),
    )
    details = [
        f"docker-compose.release.yaml missing fragment: {required_fragment}"
        for required_fragment in required_release_fragments
        if required_fragment not in release_compose_text
    ]

    forbidden_fragments = (
        "release-compose-local-secret",
        "release-compose-auth-proxy",
    )
    details.extend(
        [
            "docker-compose.release.yaml still contains predictable auth "
            f"default: {forbidden_fragment}"
            for forbidden_fragment in forbidden_fragments
            if forbidden_fragment in release_compose_text
        ],
    )
    details.extend(
        [
            "container-quickstart.md still contains predictable auth "
            f"default: {forbidden_fragment}"
            for forbidden_fragment in forbidden_fragments
            if forbidden_fragment in quickstart_text
        ],
    )

    required_quickstart_fragments = (
        "signing_secret = secrets.token_urlsafe(32)",
        "proxy_token = secrets.token_urlsafe(32)",
        'print(f"UGOITE_DEV_SIGNING_SECRET={signing_secret}")',
        'print(f"UGOITE_DEV_AUTH_PROXY_TOKEN={proxy_token}")',
        "requires operator-supplied auth values",
        "| `UGOITE_DEV_SIGNING_SECRET` | required unique value |",
        "| `UGOITE_DEV_AUTH_PROXY_TOKEN` | required unique value |",
    )
    details.extend(
        [
            f"container-quickstart.md missing fragment: {required_quickstart_fragment}"
            for required_quickstart_fragment in required_quickstart_fragments
            if required_quickstart_fragment not in quickstart_text
        ],
    )

    if details:
        raise AssertionError("; ".join(details))
