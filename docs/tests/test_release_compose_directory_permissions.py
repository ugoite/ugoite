"""REQ-OPS-017 regression coverage for release quick-start directory permissions."""

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
CONTAINER_QUICKSTART = REPO_ROOT / "docs" / "guide" / "container-quickstart.md"
COMPOSE_TROUBLESHOOTING = (
    REPO_ROOT / "docs" / "guide" / "troubleshooting-compose-startup.md"
)


def test_docs_req_ops_017_release_quickstart_avoids_world_writable_spaces() -> None:
    """REQ-OPS-017: release quick-start prefers ownership-aware spaces permissions."""
    quickstart_text = CONTAINER_QUICKSTART.read_text(encoding="utf-8")
    troubleshooting_text = COMPOSE_TROUBLESHOOTING.read_text(encoding="utf-8")

    details = [
        message
        for condition, message in (
            (
                "sudo chown 10001:10001 ./spaces" not in quickstart_text,
                (
                    "container-quickstart.md must grant the published backend "
                    "uid/gid ownership"
                ),
            ),
            (
                "chmod 0750 ./spaces" not in quickstart_text,
                (
                    "container-quickstart.md must keep the published spaces "
                    "directory at 0750"
                ),
            ),
            (
                "chmod 0777 ./spaces" in quickstart_text,
                (
                    "container-quickstart.md must not keep 0777 in the primary "
                    "quick-start flow"
                ),
            ),
            (
                "Keep `chmod 0777` as a last-resort troubleshooting step only."
                not in quickstart_text,
                (
                    "container-quickstart.md must demote 0777 to last-resort "
                    "troubleshooting"
                ),
            ),
            (
                'sudo chown 10001:10001 "$SPACE_PATH"' not in troubleshooting_text,
                (
                    "troubleshooting-compose-startup.md must try ownership-aware "
                    "writes first"
                ),
            ),
            (
                'chmod 0750 "$SPACE_PATH"' not in troubleshooting_text,
                (
                    "troubleshooting-compose-startup.md must keep the "
                    "troubleshooting mode at 0750 first"
                ),
            ),
            (
                'setfacl -m u:10001:rwx "$SPACE_PATH"' not in troubleshooting_text,
                ("troubleshooting-compose-startup.md must mention the ACL fallback"),
            ),
            (
                'chmod 0777 "$SPACE_PATH"' not in troubleshooting_text,
                (
                    "troubleshooting-compose-startup.md must keep 0777 only as "
                    "an explicit last resort"
                ),
            ),
        )
        if condition
    ]

    if details:
        raise AssertionError("; ".join(details))
