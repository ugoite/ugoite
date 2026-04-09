"""REQ-OPS-018 regression coverage for verify-first CLI install guidance."""

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
CLI_GUIDE = REPO_ROOT / "docs" / "guide" / "cli.md"


def test_docs_req_ops_018_cli_guide_leads_with_verify_first_release_install() -> None:
    """REQ-OPS-018: CLI guide leads with a verify-first released install path."""
    guide_text = CLI_GUIDE.read_text(encoding="utf-8")

    verify_first_heading = "### Verify-first archive install"
    raw_bootstrap_command = (
        "curl -fsSL https://raw.githubusercontent.com/ugoite/ugoite/main/"
        "scripts/install-ugoite-cli.sh | bash"
    )

    details = [
        message
        for condition, message in (
            (
                verify_first_heading not in guide_text,
                "cli.md is missing the verify-first archive install heading",
            ),
            (
                'curl -fsSLO "${BASE_URL}/ugoite-v${VERSION}-${TARGET}.tar.gz"'
                not in guide_text,
                "cli.md is missing the exact-release archive download command",
            ),
            (
                'curl -fsSLO "${BASE_URL}/ugoite-v${VERSION}-${TARGET}.tar.gz.sha256"'
                not in guide_text,
                "cli.md is missing the exact-release checksum download command",
            ),
            (
                'sha256sum -c "ugoite-v${VERSION}-${TARGET}.tar.gz.sha256"'
                not in guide_text,
                "cli.md is missing the Linux checksum verification command",
            ),
            (
                'shasum -a 256 -c "ugoite-v${VERSION}-${TARGET}.tar.gz.sha256"'
                not in guide_text,
                "cli.md is missing the macOS checksum verification command",
            ),
            (
                raw_bootstrap_command not in guide_text,
                "cli.md must still document the direct shell bootstrap shortcut",
            ),
            (
                "trust-the-script shortcut rather than the default recommendation"
                not in guide_text,
                "cli.md must mark the raw bootstrap path as a secondary shortcut",
            ),
            (
                verify_first_heading in guide_text
                and raw_bootstrap_command in guide_text
                and guide_text.index(verify_first_heading)
                > guide_text.index(raw_bootstrap_command),
                "cli.md must place the verify-first archive flow before raw piping",
            ),
        )
        if condition
    ]

    if details:
        raise AssertionError("; ".join(details))
