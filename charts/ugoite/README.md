# ugoite Helm Chart

This chart packages the published Ugoite backend + frontend deployment topology
for Kubernetes. It mirrors `docker-compose.release.yaml`, keeps
`UGOITE_ROOT=/data`, and computes the in-cluster frontend-to-backend service URL
by default.

Use [`../../docs/guide/helm-chart.md`](../../docs/guide/helm-chart.md) for the
documented install flow, value overrides, and local `kubectl port-forward`
commands.
