# Log Redaction Guidance

Never expose authentication material in logs, screenshots, or bug reports.

## Always redact

- Bearer tokens
- API keys
- HMAC secrets
- Authorization headers
- Cookie or session secrets

## Safe reporting checklist

- Replace secrets with `[REDACTED]`.
- Keep only token prefix when needed for correlation.
- Remove credential-bearing query strings.
- Review shell history before sharing command output.
- Sanitize CI logs before publishing artifacts.
