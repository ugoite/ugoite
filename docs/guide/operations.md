# Operations and Troubleshooting

Use this guide as the hub for the operational docs that keep an existing Ugoite
stack healthy, diagnosable, and safe to evolve.

## Run and verify the stack

- [Backend Healthcheck](backend-healthcheck.md) for the quickest readiness check
  when the browser or CLI looks stuck.
- [Environment Variable Matrix](env-matrix.md) when you need to confirm which
  env vars exist, where they apply, and which values are safe defaults.

## Deploy and harden

- [Helm Chart Guide](helm-chart.md) for Kubernetes-oriented deployment and
  release topology.
- [Log Redaction](log-redaction.md) when you need to confirm sensitive values do
  not leak into logs or diagnostics.

## Maintain storage safely

- [Storage Cleanup](storage-cleanup.md) for reclaiming generated state without
  guessing which paths are safe to remove.
- [Storage Migration](storage-migration.md) when you need to move or reshape a
  space without treating storage as disposable.

## Troubleshoot access issues

- [Troubleshooting Unauthorized Spaces](troubleshooting-unauthorized-spaces.md)
  when auth succeeds but a space still looks inaccessible or empty.

If you are still choosing an entry path instead of operating an existing stack,
return to the README Start Here section or the docsite Getting Started page.
