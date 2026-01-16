# Error Handling & Resilience

This document defines cross-cutting error-handling principles for IEapp.

## Principles

- Prefer **clear, stable HTTP status codes** over ambiguous 200 responses.
- Preserve the **root cause** in logs but return **safe messages** to clients.
- Treat **409 Conflict** as a first-class, expected outcome (optimistic concurrency).
- Do not leak filesystem paths or secrets in API errors.

## Common Scenarios

### Validation Errors

- Use **400** for invalid path ids / malformed requests.
- Use **422** for semantic validation failures (e.g., Class validation warnings).

### Concurrency Conflicts

- Use **409** with enough context for the client to recover:
  - current revision id
  - or current note snapshot

### Not Found

- Use **404** when workspace/note/class does not exist.

### Server Errors

- Use **5xx** for unexpected failures.
- Log exceptions with request context.

## Client Behavior

See [frontend–backend contracts](../architecture/frontend-backend-interface.md#error-handling-standards).

