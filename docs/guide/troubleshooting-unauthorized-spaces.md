# Troubleshooting Unauthorized Spaces Page

When the spaces page returns `Unauthorized`, validate these items in order:

1. Confirm backend is running and reachable.
   - `curl -sS http://localhost:8000/health`
2. Confirm frontend proxy target.
   - `BACKEND_URL` must point to the backend reachable by the frontend process.
3. Confirm auth token wiring.
   - Frontend server should receive `UGOITE_FRONTEND_BEARER_TOKEN`.
4. Confirm token exists in backend auth source.
   - Include the token in `UGOITE_AUTH_BEARER_TOKENS_JSON` or bootstrap settings.
5. Retry with clean browser session.
   - Clear stale cookies and local storage.

If these checks still fail, capture request/response headers and backend logs with tokens redacted.
