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
                'setfacl -m u:10001:rwx,d:u:10001:rwx ./spaces' not in quickstart_text,
                (
                    "container-quickstart.md must prefer ACL-based access for the "
                    "published backend user"
                ),
            ),
            (
                'sudo chown "$(id -u)":10001 ./spaces' not in quickstart_text,
                (
                    "container-quickstart.md must keep the host user as the "
                    "directory owner when ACLs are unavailable"
                ),
            ),
            (
                "chmod 0770 ./spaces" not in quickstart_text,
                (
                    "container-quickstart.md must keep the fallback group "
                    "writeable without opening the directory to everyone"
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
                "host user in control of `./spaces`" not in quickstart_text,
                (
                    "container-quickstart.md must explain that the safer path "
                    "preserves local-first host access"
                ),
            ),
            (
                'setfacl -m u:10001:rwx,d:u:10001:rwx "$SPACE_PATH"'
                not in troubleshooting_text,
                (
                    "troubleshooting-compose-startup.md must try ACL-based "
                    "writes first"
                ),
            ),
            (
                'sudo chown "$(id -u)":10001 "$SPACE_PATH"' not in troubleshooting_text,
                (
                    "troubleshooting-compose-startup.md must keep the host user "
                    "as owner when ACLs are unavailable"
                ),
            ),
            (
                'chmod 0770 "$SPACE_PATH"' not in troubleshooting_text,
                (
                    "troubleshooting-compose-startup.md must keep the fallback "
                    "group-writeable before reaching for 0777"
                ),
            ),
            (
                "keep your current user as the owner" not in troubleshooting_text,
                (
                    "troubleshooting-compose-startup.md must explain why the "
                    "fallback preserves host ownership"
                ),
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
