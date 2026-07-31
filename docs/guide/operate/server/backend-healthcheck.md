---
title: "Server health check"
---

The unauthenticated health endpoint is:

```bash
curl --fail http://127.0.0.1:8000/health
```

HTTP `200` confirms that the process is accepting requests. It does not validate
every Space, credential, or storage backend.

For the source Compose file, resolve the random loopback port first:

```bash
docker compose port ugoite 8000
docker compose logs ugoite
```

The release Compose file binds `${UGOITE_PORT:-8000}` on loopback.
