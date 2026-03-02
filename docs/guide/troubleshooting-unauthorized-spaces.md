# Troubleshooting Unauthorized Spaces Page

When the spaces page returns `Unauthorized`, validate these items in order:

1. Confirm backend is running and reachable.
   - `curl -sS http://localhost:8000/health`
2. Confirm frontend proxy target.
   - `BACKEND_URL` must point to the backend reachable by the frontend process.
3. Refresh local dev login/session.
   - Re-run `mise run dev`.
   - If needed, force refresh: `UGOITE_DEV_AUTH_FORCE_LOGIN=true mise run dev`.
4. Confirm frontend/backend started with the same generated token.
   - `scripts/dev-auth-env.sh` should export both `UGOITE_AUTH_BEARER_TOKEN` and `UGOITE_BOOTSTRAP_BEARER_TOKEN`.
5. Retry with clean browser session.
   - Clear stale cookies and local storage.

If these checks still fail, capture request/response headers and backend logs with tokens redacted.
