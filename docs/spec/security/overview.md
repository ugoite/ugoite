# Security Overview

## Strategy

Current implementation uses a **Local-Only by Default** model; Milestone 4
targets an **Authenticated Access by Default** model.

### Milestone 4 Target Model

| Mode | Description |
|------|-------------|
| **Default (localhost)** | API binds to localhost by default, but still requires authenticated user sessions |
| **Remote** | When exposed beyond loopback, authentication remains mandatory |

### Current Implementation Note

- API binds to localhost by default.
- Remote access requires `UGOITE_ALLOW_REMOTE=true`.
- Protected API/MCP routes require authenticated identity.
- Phase 1 providers support bearer token and API key authentication.

## Network Isolation

### Localhost Binding
- API binds ONLY to `127.0.0.1` by default
- Prevents external network access without explicit configuration
- In the Milestone 4 target model, localhost binding does not bypass authentication

### Remote Access
- Blocked by default
- Set `UGOITE_ALLOW_REMOTE=true` to allow remote connections
- Required for dev containers or Codespaces
- Automatically configured in `mise run dev`

### CORS
- Restricted to specific frontend origin
- Configured via environment variable `FRONTEND_URL`

## Data Protection

### File Permissions
- Data directory uses `chmod 600`
- Prevents access by other users on shared systems

### HMAC Signing
- All data revisions signed with locally generated key
- Key stored in `global.json`
- Prevents tampering and detects corruption

### Input Sanitization
- All inputs validated via Pydantic models
- Path traversal prevention in file operations
- SQL injection not applicable (no SQL database)


## Authentication (Future)

Milestone 4 baseline enforces user authentication for every client channel (UI,
CLI via backend, MCP over HTTP, and future native clients). Authentication is
space-scoped and uses the following profile:

| Method | Use Case |
|--------|----------|
| Passkey (WebAuthn) | Primary user login, passwordless by default |
| OAuth2 (optional link) | External identity linking and optional auto-provisioning |
| One-time Invite Token | First-time bootstrap enrollment for admin-distributed invites |
| Service Account API Key | Non-interactive automation with scoped least-privilege actions |

### Identity Source

- Default identity store is metadata Form `User` per space.
- Metadata Form `UserGroup` is also provisioned per space.
- New OAuth2 users MAY be auto-created in metadata Form `User` when enabled.
- Service accounts are space-scoped identities with explicit action scopes and
  one-time reveal API keys.

### Recovery & Administration

- Space creator becomes initial admin.
- Admin can delegate admin role to other users.
- Admin UI MUST support forced credential reset and backup code issuance.
- Recovery operations MUST be audit logged.

## Threat Model

### In Scope
- Data integrity (tampering prevention)
- Access control (authenticated user identity + form-level authorization)
- Input validation

### Out of Scope (User Responsibility)
- Physical device security
- Storage backend security (S3 credentials)
- Backup and disaster recovery
- Network encryption (use reverse proxy for HTTPS)

## Incident Response

1. **Data Corruption**: Restore from revision history
2. **Key Compromise**: Rotate `hmac_key` in `global.json`
3. **Credential Recovery**: Admin performs forced reset and issues backup codes

## Audit Logging

Security-relevant events are stored per space as an append-only audit stream
in `spaces/{space_id}/audit/events.jsonl`.

### Event Schema

Each event includes:
- `id`
- `timestamp`
- `space_id`
- `action`
- `actor_user_id`
- `outcome` (`success` / `deny` / `error`)
- `target_type` / `target_id`
- `request_method` / `request_path` / `request_id`
- `metadata`
- `prev_hash` / `event_hash`

### Tamper-Evident Integrity

- Audit events form a hash chain.
- `event_hash` is computed from the canonical event payload and `prev_hash`.
- Retrieval verifies the full chain and rejects tampered records.

### Retention and Redaction

- Retention is bounded by `UGOITE_AUDIT_RETENTION_MAX_EVENTS` (default: `5000`).
- Oldest events are trimmed when the retention bound is exceeded.
- Stored request metadata excludes sensitive headers and raw credentials.

