# Helm Chart Guide

Use this guide when you want the shipped Ugoite browser stack on Kubernetes
instead of Docker Compose. The in-repo chart under `charts/ugoite/` mirrors
`docker-compose.release.yaml`: it keeps the published backend + frontend image
pair, preserves the backend storage contract at `/data`, wires the frontend to
the backend through an in-cluster service URL, and does not add extra always-on
services beyond the current topology.

For the published local browser quick start without cloning the repository, keep
using [Container Quick Start](container-quickstart.md). For source-based
container development, use [Docker Compose Guide](docker-compose.md).

## Prerequisites

- A Kubernetes cluster
- Helm 3.x
- A default StorageClass or a pre-created PersistentVolumeClaim
- A local checkout of this repository

## Quick start

Clone the repository, prepare a small values file, and install the chart from
the workspace:

```bash
git clone https://github.com/ugoite/ugoite.git
cd ugoite
HELM_AUTH_SIGNING_SECRET="$(openssl rand -hex 32)"
HELM_AUTH_PROXY_TOKEN="$(openssl rand -hex 32)"
cat > values.local.yaml <<EOF
image:
  tag: stable
auth:
  devUserId: dev-local-user
  signingSecret: ${HELM_AUTH_SIGNING_SECRET}
  proxyToken: ${HELM_AUTH_PROXY_TOKEN}
backend:
  persistence:
    size: 10Gi
EOF
helm upgrade --install ugoite ./charts/ugoite \
  --namespace ugoite \
  --create-namespace \
  -f values.local.yaml
```

By default the chart uses:

- `ghcr.io/ugoite/ugoite/backend:${image.tag}`
- `ghcr.io/ugoite/ugoite/frontend:${image.tag}`
- a backend volume mounted at `/data`
- local demo login (`mock-oauth`) defaults that match the release Compose quick
  start
- install-specific auth secrets that you supply through `values.local.yaml`
- a computed `BACKEND_URL` equivalent to `http://backend:8000`, but scoped to
  the generated Kubernetes service name for the current release

## Access the services locally

The chart defaults both Services to `ClusterIP`, so use `kubectl port-forward`
when working from your laptop:

```bash
kubectl -n ugoite port-forward svc/ugoite-frontend 3000:3000
kubectl -n ugoite port-forward svc/ugoite-backend 8000:8000
```

The example commands above assume the Helm release name is `ugoite`. Then open:

- Frontend UI login: http://127.0.0.1:3000/login
- Backend API: http://127.0.0.1:8000

Click **Continue with Local Demo Login** to reach `/spaces`.

## Key values

| Value | Default | Purpose |
| --- | --- | --- |
| `image.tag` | `stable` | Shared published image tag for backend and frontend. |
| `backend.image.repository` | `ghcr.io/ugoite/ugoite/backend` | Backend image repository. |
| `frontend.image.repository` | `ghcr.io/ugoite/ugoite/frontend` | Frontend image repository. |
| `backend.service.port` | `8000` | Backend Service port exposed inside the cluster. |
| `frontend.service.port` | `3000` | Frontend Service port exposed inside the cluster. |
| `backend.persistence.mountPath` | `/data` | Mount path that backs `UGOITE_ROOT`. |
| `backend.persistence.size` | `10Gi` | Requested PVC size for the backend storage volume. |
| `backend.persistence.storageClassName` | empty | Optional storage class override for the backend PVC. |
| `backend.persistence.existingClaim` | empty | Reuse an existing PVC instead of creating a new one. |
| `auth.devUserId` | `dev-local-user` | Local demo login user id for the shipped login flow. |
| `auth.proxyToken` | empty (required unique value) | Shared token for frontend proxy and backend dev auth wiring (`UGOITE_DEV_AUTH_PROXY_TOKEN`). |
| `auth.signingKid` | `release-compose-local-v1` | Signing key id for the default bearer-token setup. |
| `auth.signingSecret` | empty (required unique value) | Signing secret used to mint the dev bearer-token secret for this install. |
| `auth.bearerSecrets` | computed from signing values | Override `UGOITE_AUTH_BEARER_SECRETS` directly when needed. |
| `auth.bearerActiveKids` | `["release-compose-local-v1"]` | Active bearer-token key ids exposed to the backend. |
| `frontend.backendUrl` | computed | Override the frontend `BACKEND_URL` instead of using the generated backend Service URL. |
| `backend.extraEnv` / `frontend.extraEnv` | empty | Additional environment variables appended to each container. |

## Notes

- Publication or automatic cluster deployment is intentionally out of scope for this chart today. The repository keeps the chart in-tree, but it does not yet publish it to an OCI registry or install it from CI.
- The chart deliberately stays limited to the existing backend + frontend
  topology from `docker-compose.release.yaml`.
- Generate a fresh `auth.signingSecret` and `auth.proxyToken` for every install
  so exposed clusters do not rely on repository-known development secrets.
- If your cluster already has a durable claim, set
  `backend.persistence.existingClaim` instead of creating a new one.
- If you need a fixed backend host instead of the computed chart-equivalent
  service URL, set `frontend.backendUrl`.
