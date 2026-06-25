# REST API

The Rust server implementation in `crates/ugoite-server/src/lib.rs` and its generated `/openapi.json` document are authoritative. The checked-in [`openapi.yaml`](openapi.yaml) is verified against that output by `cargo run -p xtask -- openapi-check`.

When the server hosts static browser files (`UGOITE_STATIC_DIR` is set), application API routes are nested below `/api`; `/health` and `/openapi.json` also remain available at the root. When static hosting is disabled, the OpenAPI paths are served at the root.

## Authentication

Protected routes accept a bearer token/session cookie or configured API key. Authentication establishes an identity; Space membership and role checks authorize each operation. `POST /auth/login` is contracted but returns HTTP `403` in this release. Development login uses `POST /auth/mock-oauth` only when `UGOITE_DEV_AUTH_MODE=mock-oauth`.

## Endpoints

| Method | Path | Summary | Availability |
|---|---|---|---|
| `GET` | `/health` | Health check | implemented |
| `GET` | `/openapi.json` | OpenAPI snapshot | implemented |
| `GET` | `/auth/config` | Read local development auth configuration | implemented |
| `POST` | `/auth/login` | Passkey/TOTP login | contracted; returns 403 |
| `POST` | `/auth/mock-oauth` | Local mock OAuth login | implemented |
| `GET` | `/auth/session` | Read browser session | implemented |
| `DELETE` | `/auth/session` | Clear browser session | implemented |
| `GET` | `/preferences/me` | Read current user preferences | implemented |
| `PATCH` | `/preferences/me` | Patch current user preferences | implemented |
| `GET` | `/spaces` | List spaces visible to the authenticated identity | implemented |
| `POST` | `/spaces` | Create a space owned by the authenticated identity | implemented |
| `GET` | `/spaces/{space_id}` | Get a space | implemented |
| `PATCH` | `/spaces/{space_id}` | Patch space settings | implemented |
| `POST` | `/spaces/{space_id}/test-connection` | Test storage connection | implemented |
| `GET` | `/spaces/{space_id}/members` | List members | implemented |
| `POST` | `/spaces/{space_id}/members/invitations` | Invite a member | implemented |
| `POST` | `/spaces/{space_id}/members/accept` | Accept a member invitation | implemented |
| `POST` | `/spaces/{space_id}/members/{member_user_id}/role` | Update member role | implemented |
| `DELETE` | `/spaces/{space_id}/members/{member_user_id}` | Revoke a member | implemented |
| `POST` | `/spaces/{space_id}/sql-sessions` | Create SQL session | implemented |
| `GET` | `/spaces/{space_id}/sql-sessions/{session_id}` | Get SQL session status | implemented |
| `GET` | `/spaces/{space_id}/sql-sessions/{session_id}/count` | Get SQL session row count | implemented |
| `GET` | `/spaces/{space_id}/sql-sessions/{session_id}/rows` | Get SQL session rows | implemented |
| `GET` | `/spaces/{space_id}/entries` | List entries | implemented |
| `POST` | `/spaces/{space_id}/entries` | Create entry | implemented |
| `GET` | `/spaces/{space_id}/entries/options` | List entry options | implemented |
| `GET` | `/spaces/{space_id}/entries/{entry_id}` | Get entry | implemented |
| `PUT` | `/spaces/{space_id}/entries/{entry_id}` | Update entry | implemented |
| `DELETE` | `/spaces/{space_id}/entries/{entry_id}` | Delete entry | implemented |
| `GET` | `/spaces/{space_id}/entries/{entry_id}/history` | Get entry history | implemented |
| `GET` | `/spaces/{space_id}/entries/{entry_id}/history/{revision_id}` | Get entry revision | implemented |
| `POST` | `/spaces/{space_id}/entries/{entry_id}/restore` | Restore entry revision | implemented |
| `GET` | `/spaces/{space_id}/forms` | List forms | implemented |
| `POST` | `/spaces/{space_id}/forms` | Upsert form | implemented |
| `GET` | `/spaces/{space_id}/forms/types` | List form column types | implemented |
| `GET` | `/spaces/{space_id}/forms/{form_name}` | Get form | implemented |
| `GET` | `/spaces/{space_id}/search` | Search entries | implemented |
| `POST` | `/spaces/{space_id}/query` | Structured entry query | implemented |
| `GET` | `/spaces/{space_id}/sql` | List saved SQL | implemented |
| `POST` | `/spaces/{space_id}/sql` | Create saved SQL | implemented |
| `GET` | `/spaces/{space_id}/sql/{sql_id}` | Get saved SQL | implemented |
| `PUT` | `/spaces/{space_id}/sql/{sql_id}` | Update saved SQL | implemented |
| `DELETE` | `/spaces/{space_id}/sql/{sql_id}` | Delete saved SQL | implemented |
| `GET` | `/spaces/{space_id}/assets` | List assets | implemented |
| `POST` | `/spaces/{space_id}/assets` | Upload asset | implemented |
| `DELETE` | `/spaces/{space_id}/assets/{asset_id}` | Delete asset | implemented |
| `GET` | `/mcp/resources/{space_id}/entries/list` | MCP entry list resource | implemented |

## Current exclusions

- No service-account CRUD routes are shipped. Static/signed credentials are configured out of band.
- No audit-log listing route is shipped.
- Remote CLI asset upload is not supported even though REST/browser upload is implemented.
- MCP is documented separately and currently exposes one read-only resource route.

## Request limits and errors

The server applies a 20 MiB body limit, request IDs, tracing, and CORS middleware. Domain/service errors are mapped to JSON HTTP errors. Clients should branch on status and structured payload rather than matching human-readable text.

For exact schemas, parameters, and response bodies, use [`openapi.yaml`](openapi.yaml).
