# Security Overview

## Strategy

IEapp implements a **Local-Only by Default** security model:

| Mode | Description |
|------|-------------|
| **Default** | API binds to localhost only, no authentication required |
| **Remote** | When exposed beyond loopback, authentication MUST be enabled |

## Network Isolation

### Localhost Binding
- API binds ONLY to `127.0.0.1` by default
- Prevents external network access without explicit configuration

### Remote Access
- Blocked by default
- Set `IEAPP_ALLOW_REMOTE=true` to allow remote connections
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
- All inputs validated via Pydantic schemas
- Path traversal prevention in file operations
- SQL injection not applicable (no SQL database)


## Authentication (Future)

When authentication is required (Milestone 4):

| Method | Use Case |
|--------|----------|
| API Key | Service account access |
| Bearer Token | User sessions |
| OAuth Proxy | Enterprise SSO |

## Threat Model

### In Scope
- Data integrity (tampering prevention)
- Access control (localhost isolation)
- Input validation

### Out of Scope (User Responsibility)
- Physical device security
- Storage backend security (S3 credentials)
- Backup and disaster recovery
- Network encryption (use reverse proxy for HTTPS)

## Incident Response

1. **Data Corruption**: Restore from revision history
2. **Key Compromise**: Rotate `hmac_key` in `global.json`
