# Local Storage Growth and Cleanup Strategy

Long-running local environments can accumulate spaces and temporary artifacts.

## Recommended routine

- Check storage usage weekly.
- Archive important spaces before deletion.
- Remove stale E2E storage roots and temporary local review files.

## Quick checks

```bash
du -sh backend/spaces || true
du -sh /tmp/ugoite-e2e || true
```

## Cleanup examples

```bash
rm -rf /tmp/ugoite-e2e
find backend/spaces -maxdepth 1 -type d -name "acl-*" -mtime +14 -print
```

Review matched paths before deleting in shared environments.
