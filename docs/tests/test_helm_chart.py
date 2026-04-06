"""Helm chart topology tests.

REQ-OPS-028: Repository-owned Helm chart must mirror the published release
topology.
REQ-SEC-001: Exposed container packaging must not rely on repository-known auth secrets.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any, Never

import yaml

REPO_ROOT = Path(__file__).resolve().parents[2]
CHART_DIR = REPO_ROOT / "charts" / "ugoite"
CHART_METADATA_PATH = CHART_DIR / "Chart.yaml"
CHART_VALUES_PATH = CHART_DIR / "values.yaml"
CHART_README_PATH = CHART_DIR / "README.md"
CHART_HELPERS_PATH = CHART_DIR / "templates" / "_helpers.tpl"
CHART_SECRET_PATH = CHART_DIR / "templates" / "auth-secret.yaml"
CHART_BACKEND_DEPLOYMENT_PATH = CHART_DIR / "templates" / "backend-deployment.yaml"
CHART_BACKEND_PVC_PATH = CHART_DIR / "templates" / "backend-pvc.yaml"
CHART_BACKEND_SERVICE_PATH = CHART_DIR / "templates" / "backend-service.yaml"
CHART_FRONTEND_DEPLOYMENT_PATH = CHART_DIR / "templates" / "frontend-deployment.yaml"
CHART_FRONTEND_SERVICE_PATH = CHART_DIR / "templates" / "frontend-service.yaml"
CHART_NOTES_PATH = CHART_DIR / "templates" / "NOTES.txt"
BACKEND_DOCKERFILE_PATH = REPO_ROOT / "backend" / "Dockerfile"
HELM_GUIDE_PATH = REPO_ROOT / "docs" / "guide" / "helm-chart.md"
CONTAINER_GUIDE_PATH = REPO_ROOT / "docs" / "guide" / "container-quickstart.md"
STACK_SPEC_PATH = REPO_ROOT / "docs" / "spec" / "architecture" / "stack.md"
README_PATH = REPO_ROOT / "README.md"
RELEASE_COMPOSE_PATH = REPO_ROOT / "docker-compose.release.yaml"
HARDENED_RUNTIME_ID = 10001
DROP_ALL_CAPABILITIES = ["ALL"]
RUNTIME_DEFAULT_SECCOMP = "RuntimeDefault"

REQUIRED_CHART_PATHS = (
    CHART_METADATA_PATH,
    CHART_VALUES_PATH,
    CHART_README_PATH,
    CHART_HELPERS_PATH,
    CHART_SECRET_PATH,
    CHART_BACKEND_DEPLOYMENT_PATH,
    CHART_BACKEND_PVC_PATH,
    CHART_BACKEND_SERVICE_PATH,
    CHART_FRONTEND_DEPLOYMENT_PATH,
    CHART_FRONTEND_SERVICE_PATH,
    CHART_NOTES_PATH,
    HELM_GUIDE_PATH,
)
REQUIRED_HELPER_FRAGMENTS = {
    'define "ugoite.backendFullname"',
    'define "ugoite.frontendFullname"',
    'define "ugoite.authBearerSecrets"',
    'define "ugoite.frontendBackendUrl"',
    'printf "http://%s:%v"',
}
REQUIRED_BACKEND_TEMPLATE_FRAGMENTS = {
    ".Values.backend.image.repository",
    ".Values.image.tag",
    "name: UGOITE_ROOT",
    ".Values.backend.persistence.mountPath",
    "name: UGOITE_ALLOW_REMOTE",
    "name: UGOITE_DEV_AUTH_MODE",
    "name: UGOITE_DEV_USER_ID",
    "name: UGOITE_DEV_SIGNING_KID",
    "name: UGOITE_DEV_SIGNING_SECRET",
    "name: UGOITE_AUTH_BEARER_SECRETS",
    "name: UGOITE_AUTH_BEARER_ACTIVE_KIDS",
    "name: UGOITE_DEV_AUTH_PROXY_TOKEN",
    "persistentVolumeClaim:",
    "emptyDir: {}",
}
REQUIRED_SECRET_TEMPLATE_FRAGMENTS = {
    "kind: Secret",
    "stringData:",
    "UGOITE_DEV_SIGNING_SECRET",
    "UGOITE_AUTH_BEARER_SECRETS",
    "UGOITE_DEV_AUTH_PROXY_TOKEN",
    'include "ugoite.authBearerSecrets"',
}
REQUIRED_FRONTEND_TEMPLATE_FRAGMENTS = {
    ".Values.frontend.image.repository",
    ".Values.image.tag",
    "name: BACKEND_URL",
    'include "ugoite.frontendBackendUrl"',
    "name: UGOITE_DEV_AUTH_PROXY_TOKEN",
}
REQUIRED_PVC_TEMPLATE_FRAGMENTS = {
    "kind: PersistentVolumeClaim",
    ".Values.backend.persistence.accessModes",
    ".Values.backend.persistence.size",
    ".Values.backend.persistence.storageClassName",
}
REQUIRED_SERVICE_TEMPLATE_FRAGMENTS = {
    "kind: Service",
    "targetPort: http",
    "protocol: TCP",
}
REQUIRED_HELM_GUIDE_FRAGMENTS = {
    "helm upgrade --install ugoite ./charts/ugoite",
    "kubectl -n ugoite port-forward svc/ugoite-frontend 3000:3000",
    "kubectl -n ugoite port-forward svc/ugoite-backend 8000:8000",
    "http://127.0.0.1:3000/login",
    "Continue with Local Demo Login",
    "`/data`",
    "UGOITE_DEV_AUTH_PROXY_TOKEN",
    "backend.persistence.existingClaim",
    "frontend.backendUrl",
    "does not yet publish it to an OCI registry or install it from CI",
    "backend + frontend",
}
REQUIRED_README_FRAGMENTS = {
    "docs/guide/helm-chart.md",
    "charts/ugoite",
    "Kubernetes",
    "`/data`",
}
REQUIRED_CONTAINER_GUIDE_FRAGMENTS = {
    "[Helm Chart Guide](helm-chart.md)",
}
REQUIRED_CHART_README_FRAGMENTS = {
    "../../docs/guide/helm-chart.md",
    "docker-compose.release.yaml",
    "`UGOITE_ROOT=/data`",
}
REQUIRED_STACK_SPEC_FRAGMENTS = {
    "docker-compose.release.yaml",
    "charts/ugoite",
    "backend + frontend",
    "`/data`",
}
REQUIRED_BACKEND_DOCKERFILE_HARDENING_FRAGMENTS = {
    "ARG UGOITE_UID=10001",
    "ARG UGOITE_GID=10001",
    'groupadd --system --gid "${UGOITE_GID}" ugoite',
    'useradd --system --uid "${UGOITE_UID}" --gid "${UGOITE_GID}"',
    "mkdir -p /data",
    "chown -R ugoite:ugoite /app /data /home/ugoite",
    "ENV HOME=/home/ugoite",
    "USER ugoite:ugoite",
}
REQUIRED_HELM_HARDENING_GUIDE_FRAGMENTS = {
    "non-root runtime defaults for backend + frontend containers",
    "disabled privilege escalation",
    "dropped Linux capabilities",
    "`backend.podSecurityContext.fsGroup`",
    "`backend.securityContext`",
    "`frontend.securityContext`",
}
REQUIRED_STACK_HARDENING_FRAGMENTS = {
    "non-root/container-hardened",
    "root-only privileges",
}


def _fail(message: str) -> Never:
    raise AssertionError(message)


def _read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _load_yaml_mapping(path: Path) -> dict[str, Any]:
    loaded = yaml.safe_load(_read_text(path))
    if not isinstance(loaded, dict):
        _fail(f"{path.relative_to(REPO_ROOT)} must be a YAML mapping")
    return loaded


def _require_mapping(value: object, *, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        _fail(f"{label} must be a mapping")
    return value


def _missing_fragments(text: str, required_fragments: set[str]) -> list[str]:
    return sorted(fragment for fragment in required_fragments if fragment not in text)


def test_docs_req_ops_028_helm_chart_assets_exist() -> None:
    """REQ-OPS-028: Helm chart assets and install docs must exist."""
    missing_paths = [
        path.relative_to(REPO_ROOT).as_posix()
        for path in REQUIRED_CHART_PATHS
        if not path.exists()
    ]
    if missing_paths:
        _fail("Missing Helm chart assets: " + ", ".join(missing_paths))

    chart_metadata = _load_yaml_mapping(CHART_METADATA_PATH)
    if str(chart_metadata.get("apiVersion")) != "v2":
        _fail("charts/ugoite/Chart.yaml must declare apiVersion: v2")
    if str(chart_metadata.get("name")) != "ugoite":
        _fail("charts/ugoite/Chart.yaml must declare name: ugoite")
    if str(chart_metadata.get("type")) != "application":
        _fail("charts/ugoite/Chart.yaml must declare type: application")


def test_docs_req_ops_028_helm_chart_defaults_match_release_topology() -> None:
    """REQ-OPS-028: Helm chart defaults must mirror the release compose topology."""
    values = _load_yaml_mapping(CHART_VALUES_PATH)
    image = _require_mapping(values.get("image"), label="charts/ugoite values.image")
    backend = _require_mapping(
        values.get("backend"),
        label="charts/ugoite values.backend",
    )
    frontend = _require_mapping(
        values.get("frontend"),
        label="charts/ugoite values.frontend",
    )
    auth = _require_mapping(values.get("auth"), label="charts/ugoite values.auth")
    backend_image = _require_mapping(
        backend.get("image"),
        label="charts/ugoite values.backend.image",
    )
    frontend_image = _require_mapping(
        frontend.get("image"),
        label="charts/ugoite values.frontend.image",
    )
    backend_service = _require_mapping(
        backend.get("service"),
        label="charts/ugoite values.backend.service",
    )
    frontend_service = _require_mapping(
        frontend.get("service"),
        label="charts/ugoite values.frontend.service",
    )
    backend_persistence = _require_mapping(
        backend.get("persistence"),
        label="charts/ugoite values.backend.persistence",
    )

    expected_values = (
        (str(image.get("tag")), "stable", "charts/ugoite values.image.tag"),
        (
            str(backend_image.get("repository")),
            "ghcr.io/ugoite/ugoite/backend",
            "charts/ugoite values.backend.image.repository",
        ),
        (
            str(frontend_image.get("repository")),
            "ghcr.io/ugoite/ugoite/frontend",
            "charts/ugoite values.frontend.image.repository",
        ),
        (
            str(backend_service.get("port")),
            "8000",
            "charts/ugoite values.backend.service.port",
        ),
        (
            str(frontend_service.get("port")),
            "3000",
            "charts/ugoite values.frontend.service.port",
        ),
        (
            str(backend_persistence.get("mountPath")),
            "/data",
            "charts/ugoite values.backend.persistence.mountPath",
        ),
        (str(auth.get("mode")), "mock-oauth", "charts/ugoite values.auth.mode"),
        (
            str(auth.get("devUserId")),
            "dev-local-user",
            "charts/ugoite values.auth.devUserId",
        ),
        (
            str(auth.get("signingKid")),
            "release-compose-local-v1",
            "charts/ugoite values.auth.signingKid",
        ),
        (str(auth.get("signingSecret")), "", "charts/ugoite values.auth.signingSecret"),
        (str(auth.get("proxyToken")), "", "charts/ugoite values.auth.proxyToken"),
    )
    for actual, expected, label in expected_values:
        if actual != expected:
            _fail(f"{label} must be {expected!r}, got {actual!r}")

    bearer_active_kids = auth.get("bearerActiveKids")
    if bearer_active_kids != ["release-compose-local-v1"]:
        _fail(
            "charts/ugoite values.auth.bearerActiveKids must default to "
            "['release-compose-local-v1']",
        )

    compose_text = _read_text(RELEASE_COMPOSE_PATH)
    required_compose_fragments = {
        "ghcr.io/ugoite/ugoite/backend:${UGOITE_VERSION:?set UGOITE_VERSION}",
        "ghcr.io/ugoite/ugoite/frontend:${UGOITE_VERSION:?set UGOITE_VERSION}",
        "UGOITE_ROOT=/data",
        "UGOITE_ALLOW_REMOTE=true",
        "UGOITE_DEV_AUTH_MODE=mock-oauth",
        "UGOITE_DEV_USER_ID=${UGOITE_DEV_USER_ID:-dev-local-user}",
        "UGOITE_DEV_SIGNING_KID=release-compose-local-v1",
        "UGOITE_DEV_SIGNING_SECRET=release-compose-local-secret",
        "UGOITE_AUTH_BEARER_SECRETS=release-compose-local-v1:release-compose-local-secret",
        "UGOITE_AUTH_BEARER_ACTIVE_KIDS=release-compose-local-v1",
        "UGOITE_DEV_AUTH_PROXY_TOKEN=${UGOITE_DEV_AUTH_PROXY_TOKEN:-release-compose-auth-proxy}",
        "BACKEND_URL=http://backend:8000",
    }
    missing_compose_fragments = _missing_fragments(
        compose_text,
        required_compose_fragments,
    )
    if missing_compose_fragments:
        _fail(
            "docker-compose.release.yaml lost required parity fragments: "
            + ", ".join(missing_compose_fragments),
        )

    template_checks = (
        (
            CHART_HELPERS_PATH,
            REQUIRED_HELPER_FRAGMENTS,
            "charts/ugoite/templates/_helpers.tpl",
        ),
        (
            CHART_BACKEND_DEPLOYMENT_PATH,
            REQUIRED_BACKEND_TEMPLATE_FRAGMENTS,
            "charts/ugoite/templates/backend-deployment.yaml",
        ),
        (
            CHART_SECRET_PATH,
            REQUIRED_SECRET_TEMPLATE_FRAGMENTS,
            "charts/ugoite/templates/auth-secret.yaml",
        ),
        (
            CHART_FRONTEND_DEPLOYMENT_PATH,
            REQUIRED_FRONTEND_TEMPLATE_FRAGMENTS,
            "charts/ugoite/templates/frontend-deployment.yaml",
        ),
        (
            CHART_BACKEND_PVC_PATH,
            REQUIRED_PVC_TEMPLATE_FRAGMENTS,
            "charts/ugoite/templates/backend-pvc.yaml",
        ),
        (
            CHART_BACKEND_SERVICE_PATH,
            REQUIRED_SERVICE_TEMPLATE_FRAGMENTS
            | {"port: {{ .Values.backend.service.port }}"},
            "charts/ugoite/templates/backend-service.yaml",
        ),
        (
            CHART_FRONTEND_SERVICE_PATH,
            REQUIRED_SERVICE_TEMPLATE_FRAGMENTS
            | {"port: {{ .Values.frontend.service.port }}"},
            "charts/ugoite/templates/frontend-service.yaml",
        ),
    )
    for path, fragments, label in template_checks:
        missing = _missing_fragments(_read_text(path), fragments)
        if missing:
            _fail(f"{label} missing fragments: {', '.join(missing)}")


def test_docs_req_sec_001_helm_chart_requires_unique_auth_secrets() -> None:
    """REQ-SEC-001: Helm chart installs must require unique dev auth secrets."""
    secret_text = _read_text(CHART_SECRET_PATH)
    required_secret_fragments = {
        (
            'required "charts/ugoite values.auth.signingSecret '
            'must be set to a unique secret"'
        ),
        (
            'required "charts/ugoite values.auth.proxyToken '
            'must be set to a unique token"'
        ),
    }
    missing_secret_fragments = _missing_fragments(
        secret_text,
        required_secret_fragments,
    )
    if missing_secret_fragments:
        _fail(
            (
                "charts/ugoite/templates/auth-secret.yaml must require unique "
                "auth secrets: "
            )
            + ", ".join(missing_secret_fragments),
        )

    guide_text = _read_text(HELM_GUIDE_PATH)
    required_guide_fragments = {
        'HELM_AUTH_SIGNING_SECRET="$(openssl rand -hex 32)"',
        'HELM_AUTH_PROXY_TOKEN="$(openssl rand -hex 32)"',
        "signingSecret: ${HELM_AUTH_SIGNING_SECRET}",
        "proxyToken: ${HELM_AUTH_PROXY_TOKEN}",
        "empty (required unique value)",
    }
    missing_guide_fragments = _missing_fragments(
        guide_text,
        required_guide_fragments,
    )
    if missing_guide_fragments:
        _fail(
            "docs/guide/helm-chart.md must document unique auth secret setup: "
            + ", ".join(missing_guide_fragments),
        )


def test_docs_req_ops_028_helm_chart_docs_stay_wired() -> None:
    """REQ-OPS-028: Helm chart docs must stay discoverable and explicit."""
    doc_checks = (
        (
            README_PATH,
            REQUIRED_README_FRAGMENTS,
            "README.md",
        ),
        (
            HELM_GUIDE_PATH,
            REQUIRED_HELM_GUIDE_FRAGMENTS,
            "docs/guide/helm-chart.md",
        ),
        (
            CONTAINER_GUIDE_PATH,
            REQUIRED_CONTAINER_GUIDE_FRAGMENTS,
            "docs/guide/container-quickstart.md",
        ),
        (
            CHART_README_PATH,
            REQUIRED_CHART_README_FRAGMENTS,
            "charts/ugoite/README.md",
        ),
        (
            STACK_SPEC_PATH,
            REQUIRED_STACK_SPEC_FRAGMENTS,
            "docs/spec/architecture/stack.md",
        ),
        (
            CHART_NOTES_PATH,
            {
                "docker-compose.release.yaml",
                "http://127.0.0.1:3000/login",
                "Continue with Local Demo Login",
                "port-forward",
            },
            "charts/ugoite/templates/NOTES.txt",
        ),
    )
    for path, fragments, label in doc_checks:
        missing = _missing_fragments(_read_text(path), fragments)
        if missing:
            _fail(f"{label} missing fragments: {', '.join(missing)}")


def test_docs_req_ops_035_backend_image_runs_as_non_root() -> None:
    """REQ-OPS-035: The backend image must create and run as a non-root user."""
    missing = _missing_fragments(
        _read_text(BACKEND_DOCKERFILE_PATH),
        REQUIRED_BACKEND_DOCKERFILE_HARDENING_FRAGMENTS,
    )
    if missing:
        _fail(f"backend/Dockerfile missing hardening fragments: {', '.join(missing)}")


def test_docs_req_ops_035_helm_security_defaults_are_hardened() -> None:
    """REQ-OPS-035: Helm defaults must keep backend/frontend deployments hardened."""
    values = _load_yaml_mapping(CHART_VALUES_PATH)
    backend = _require_mapping(
        values.get("backend"),
        label="charts/ugoite values.backend",
    )
    frontend = _require_mapping(
        values.get("frontend"),
        label="charts/ugoite values.frontend",
    )
    backend_pod_security = _require_mapping(
        backend.get("podSecurityContext"),
        label="charts/ugoite values.backend.podSecurityContext",
    )
    backend_security = _require_mapping(
        backend.get("securityContext"),
        label="charts/ugoite values.backend.securityContext",
    )
    frontend_security = _require_mapping(
        frontend.get("securityContext"),
        label="charts/ugoite values.frontend.securityContext",
    )

    if backend_pod_security.get("fsGroup") != HARDENED_RUNTIME_ID:
        _fail(
            "charts/ugoite values.backend.podSecurityContext.fsGroup must be "
            f"{HARDENED_RUNTIME_ID}",
        )

    backend_expectations = (
        (
            backend_security.get("runAsNonRoot"),
            True,
            "backend.securityContext.runAsNonRoot",
        ),
        (
            backend_security.get("runAsUser"),
            HARDENED_RUNTIME_ID,
            "backend.securityContext.runAsUser",
        ),
        (
            backend_security.get("runAsGroup"),
            HARDENED_RUNTIME_ID,
            "backend.securityContext.runAsGroup",
        ),
        (
            backend_security.get("allowPrivilegeEscalation"),
            False,
            "backend.securityContext.allowPrivilegeEscalation",
        ),
        (
            frontend_security.get("runAsNonRoot"),
            True,
            "frontend.securityContext.runAsNonRoot",
        ),
        (
            frontend_security.get("allowPrivilegeEscalation"),
            False,
            "frontend.securityContext.allowPrivilegeEscalation",
        ),
    )
    for actual, expected, label in backend_expectations:
        if actual != expected:
            _fail(f"charts/ugoite values.{label} must be {expected!r}, got {actual!r}")

    backend_caps = _require_mapping(
        backend_security.get("capabilities"),
        label="charts/ugoite values.backend.securityContext.capabilities",
    )
    frontend_caps = _require_mapping(
        frontend_security.get("capabilities"),
        label="charts/ugoite values.frontend.securityContext.capabilities",
    )
    if backend_caps.get("drop") != DROP_ALL_CAPABILITIES:
        _fail(
            "charts/ugoite values.backend.securityContext.capabilities.drop "
            f"must be {DROP_ALL_CAPABILITIES!r}",
        )
    if frontend_caps.get("drop") != DROP_ALL_CAPABILITIES:
        _fail(
            "charts/ugoite values.frontend.securityContext.capabilities.drop "
            f"must be {DROP_ALL_CAPABILITIES!r}",
        )

    backend_seccomp = _require_mapping(
        backend_security.get("seccompProfile"),
        label="charts/ugoite values.backend.securityContext.seccompProfile",
    )
    frontend_seccomp = _require_mapping(
        frontend_security.get("seccompProfile"),
        label="charts/ugoite values.frontend.securityContext.seccompProfile",
    )
    if backend_seccomp.get("type") != RUNTIME_DEFAULT_SECCOMP:
        _fail(
            "charts/ugoite values.backend.securityContext.seccompProfile.type "
            f"must be {RUNTIME_DEFAULT_SECCOMP}",
        )
    if frontend_seccomp.get("type") != RUNTIME_DEFAULT_SECCOMP:
        _fail(
            "charts/ugoite values.frontend.securityContext.seccompProfile.type "
            f"must be {RUNTIME_DEFAULT_SECCOMP}",
        )

    template_checks = (
        (
            CHART_BACKEND_DEPLOYMENT_PATH,
            {
                ".Values.backend.podSecurityContext",
                ".Values.backend.securityContext",
                "securityContext:",
            },
            "charts/ugoite/templates/backend-deployment.yaml",
        ),
        (
            CHART_FRONTEND_DEPLOYMENT_PATH,
            {
                ".Values.frontend.securityContext",
                "securityContext:",
            },
            "charts/ugoite/templates/frontend-deployment.yaml",
        ),
    )
    for path, fragments, label in template_checks:
        missing = _missing_fragments(_read_text(path), fragments)
        if missing:
            _fail(f"{label} missing hardening fragments: {', '.join(missing)}")


def test_docs_req_ops_035_hardening_docs_stay_visible() -> None:
    """REQ-OPS-035: Hardening defaults must stay visible in deployment docs."""
    doc_checks = (
        (
            HELM_GUIDE_PATH,
            REQUIRED_HELM_HARDENING_GUIDE_FRAGMENTS,
            "docs/guide/helm-chart.md",
        ),
        (
            STACK_SPEC_PATH,
            REQUIRED_STACK_HARDENING_FRAGMENTS,
            "docs/spec/architecture/stack.md",
        ),
        (
            CHART_README_PATH,
            {"non-root runtime hardening defaults"},
            "charts/ugoite/README.md",
        ),
    )
    for path, fragments, label in doc_checks:
        missing = _missing_fragments(_read_text(path), fragments)
        if missing:
            _fail(f"{label} missing hardening fragments: {', '.join(missing)}")
