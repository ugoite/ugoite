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
- Local development obtains browser/CLI bearer tokens through explicit
  `passkey-totp` or `mock-oauth` login endpoints after startup instead of
  injecting an authenticated token before the app starts.
- CLI endpoint configuration only allows cleartext `http://` for loopback
  development hosts (`localhost`, `127.0.0.1`, `[::1]`); remote credentialed
  endpoints MUST use `https://`.
- Space creation is further restricted to active admins of the reserved
  `admin-space`, and the creator of each non-admin space becomes that space's
  initial admin.

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
- Also required for the published two-container release Compose quick start,
  because the frontend container reaches the backend over the private Compose
  bridge network; host exposure still remains localhost-only there because the
  published ports bind to `127.0.0.1`
- `UGOITE_TRUST_PROXY_HEADERS=true` only trusts forwarded client headers from
  loopback proxy peers; direct remote clients cannot spoof `X-Forwarded-For`
  into looking like localhost
- The source `docker-compose.yaml` keeps both published ports on `127.0.0.1`
  and expects unique dev signing/proxy secrets before startup
- The Helm chart requires install-specific dev auth secrets instead of shipping
  repository-known signing or proxy defaults

### CORS
- Restricted to explicit frontend origins from `ALLOW_ORIGIN`
- Credentialed requests use explicit method and request-header allowlists

## Data Protection

### File Permissions
- Local space directories use `chmod 700`, and bootstrap metadata files use `chmod 600`
- Prevents access by other users on shared systems

### HMAC Signing
- All data revisions signed with locally generated key
- Space integrity key stored in `spaces/{space_id}/meta.json`
- Response-signing key stored in `spaces/{space_id}/hmac.json` and created on first response-signing use
- Prevents tampering and detects corruption

### Input Sanitization
- All inputs validated via Pydantic models
- Path traversal prevention in file operations
- Asset upload filenames are reduced to a single metadata-safe basename before
  storage writes so traversal segments, control characters, and Markdown heading
  prefixes cannot escape or spoof a space's `assets/` directory metadata
- SQL injection not applicable (no SQL database)

## Software Supply Chain Security

### SBOM Coverage

- CI workflow `sbom-ci.yml` generates CycloneDX SBOMs via Syft for:
  - Rust projects (`ugoite-core`, `ugoite-cli`)
  - Python projects (`backend`)
  - Bun/Node projects (`frontend`, `docsite`, `e2e`)
  - Built Docker images (`backend`, `frontend`)

### Signing and Attestation

- SBOM artifacts are signed with Cosign keyless signing (OIDC-backed).
- SBOM signatures are verified in CI before artifact publication.
- Build provenance attestations are generated for SBOM artifacts.

### Security Gates

- Grype scans generated SBOMs, with critical-severity CI gating on source SBOMs.
- SBOM and signature artifacts are uploaded as CI artifacts for auditability.

### Dependency Automation

- Dependabot updates are enabled across GitHub Actions, Bun, npm, Docker, uv-managed Python workspaces (`backend`, `ugoite-core`), and the Rust Cargo workspace root (`/`, covering `ugoite-minimum`, `ugoite-core`, and `ugoite-cli`).
- GitHub Dependency Graph and advisory alerts are expected to remain enabled for this repository.


## Authentication (Future)

Canonical machine-readable auth contract: `security/authentication-overview.yaml`.


Milestone 4 baseline enforces user authentication for every client channel (UI,
CLI via backend, MCP over HTTP, and future native clients). Authentication is
space-scoped and uses the following profile:

| Method | Use Case |
|--------|----------|
| Passkey (WebAuthn) | Primary user login, passwordless by default |
| OAuth2 (optional link) | External identity linking and optional auto-provisioning |
| One-time Invite Token | First-time bootstrap enrollment for admin-distributed invites |
| Service Account API Key | Non-interactive automation with scoped least-privilege actions |

- CLI server-backed endpoints MUST use `https://` for non-loopback hosts.
  Cleartext `http://` remains acceptable only for loopback local-development
  endpoints such as `http://localhost:8000`.

### Identity Source

- Default identity store is metadata Form `User` per space.
- Metadata Form `UserGroup` is also provisioned per space.
- New OAuth2 users MAY be auto-created in metadata Form `User` when enabled.
- Service accounts are space-scoped identities with explicit action scopes and
  one-time reveal API keys.

### Recovery & Administration

- Active `admin-space` admins can create spaces.
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
2. **Key Compromise**: Rotate the integrity key in `spaces/{space_id}/meta.json`
   and the response-signing key in `spaces/{space_id}/hmac.json` as needed
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
