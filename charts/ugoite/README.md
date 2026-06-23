# Ugoite Helm chart

Deploys the single Ugoite runtime image with persistent `/data` storage.

```bash
helm upgrade --install ugoite charts/ugoite   --set auth.bootstrapToken="$(openssl rand -hex 32)"   --set auth.signingSecret="$(openssl rand -hex 32)"
```

The templates require unique bootstrap and signing secrets. The pod runs as non-root UID/GID `10001`, drops capabilities, and mounts one PVC by default. `values.yaml` defaults to development `mock-oauth`; change authentication configuration before exposing a shared deployment.

See [`docs/guide/helm-chart.md`](../../docs/guide/helm-chart.md) for the values that map to current server environment variables.
