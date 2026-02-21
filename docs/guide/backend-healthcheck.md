# Backend Healthcheck

Use the backend health endpoint for quick readiness checks.

## Endpoint

- `GET /health`
- Success response: `200` with JSON body `{ "status": "ok" }`

## Local verification

```bash
curl -sS http://localhost:8000/health
```

Expected output:

```json
{"status":"ok"}
```
