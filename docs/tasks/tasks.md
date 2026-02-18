# Milestone 4: User Management

**Status**: 📋 Planned  
**Goal**: Introduce secure multi-user operation with authentication, authorization, and collaboration while preserving the local-first default model.

This milestone implements the future authentication and collaboration model already referenced in specs, especially:
- `docs/spec/security/overview.md` (Local-only by default, remote requires auth)
- `docs/spec/api/rest.md` and `docs/spec/api/mcp.md` (authentication behavior)
- `docs/spec/stories/experimental.yaml` (STORY-010 Multi-User Collaboration)

---

## Constraints (MUST)

- **Local-first remains default**: localhost mode must continue to work with minimal friction.
- **Remote requires authentication**: when `UGOITE_ALLOW_REMOTE=true`, protected APIs must require auth.
- **Core/backend boundary**: business logic and file I/O stay in `ugoite-core`; backend remains API orchestration.
- **No security-by-obscurity**: access checks must be explicit, testable, and requirement-linked.
- **Requirements traceability**: all new tests must map to `REQ-*` in `docs/spec/requirements/*.yaml`.

---

## Phase 0: Spec & Requirement Baseline

**Objective**: Formalize User Management requirements and align milestone scope across docs/spec and docs/tasks.

### Key Tasks
- [ ] Add/extend requirements in `docs/spec/requirements/security.yaml` for authn/authz/API keys/audit.
- [ ] Add collaboration requirements for member invitation and role management.
- [ ] Cross-link requirements to security/api/stories specs.
- [ ] Add requirement-to-test mapping placeholders for backend/core/cli/frontend/e2e.

### Acceptance Criteria
- [ ] User Management requirements exist as `REQ-*` entries and are reviewable.
- [ ] Each requirement has planned test coverage locations.
- [ ] Milestone 4 scope is consistent across roadmap/spec/tasks.

---

## Phase 1: Identity & Auth Foundation

**Objective**: Establish identity primitives and authentication mechanisms for local and remote modes.

### Key Tasks
- [ ] Define identity model (`user_id`, principal type, display metadata, disabled state).
- [ ] Implement auth provider interface (API key, bearer token; OAuth proxy adapter point).
- [ ] Add secure token/key storage and rotation strategy.
- [ ] Implement auth middleware and dependency injection for REST/MCP requests.
- [ ] Keep localhost-no-auth behavior for non-remote mode and document exact policy.
- [ ] Add negative-path handling (expired token, invalid signature, revoked key).

### Acceptance Criteria
- [ ] Remote mode denies unauthenticated requests to protected endpoints.
- [ ] Auth providers can be configured without changing API contracts.
- [ ] Identity is available in request context for downstream authorization/audit.

---

## Phase 2: Authorization (RBAC + Resource Scope)

**Objective**: Enforce role-based authorization at space and entry/resource levels.

### Key Tasks
- [ ] Define role model (e.g., owner/admin/editor/viewer/service) and permissions matrix.
- [ ] Implement space-level access checks (list/read/write/admin operations).
- [ ] Implement entry/resource-level checks where stricter controls are required.
- [ ] Add policy evaluation in `ugoite-core` and thin backend adapters.
- [ ] Add explicit authorization error schema and consistent HTTP statuses.
- [ ] Add policy tests for allow/deny cases and privilege escalation prevention.

### Acceptance Criteria
- [ ] Unauthorized actions are blocked consistently across REST/MCP/CLI flows.
- [ ] Authorization behavior is deterministic and centrally test-covered.
- [ ] No duplicated authorization business logic exists in backend layer.

---

## Phase 3: Space Membership & Collaboration

**Objective**: Deliver collaborative space sharing aligned with STORY-010.

### Key Tasks
- [ ] Define member lifecycle (`invited`, `active`, `revoked`) and invitation rules.
- [ ] Implement member APIs (`POST/GET/DELETE /spaces/{space_id}/members...`) in backend + core.
- [ ] Add invite token/email abstraction boundary (provider-agnostic).
- [ ] Support role assignment and role changes with audit trail hooks.
- [ ] Implement concurrency-safe membership updates.
- [ ] Add e2e tests for invitation, accept, revoke, and permission transitions.

### Acceptance Criteria
- [ ] Space owner/admin can invite and manage members.
- [ ] Members can access only permitted spaces/resources.
- [ ] Collaboration flows satisfy STORY-010 acceptance criteria.

---

## Phase 4: Audit Logging & Attribution

**Objective**: Persist security-relevant events and user attribution for accountability.

### Key Tasks
- [ ] Define audit event schema (actor, action, target, timestamp, outcome, request metadata).
- [ ] Add event emission for auth, authorization denials, membership, and data mutations.
- [ ] Add tamper-evident strategy aligned with existing integrity model.
- [ ] Add query/list APIs for audit events with pagination and filters.
- [ ] Add retention/redaction policy documentation.
- [ ] Add tests for event completeness and immutability guarantees.

### Acceptance Criteria
- [ ] Critical security and collaboration events are audit-logged.
- [ ] Actor attribution is available for user-facing change history.
- [ ] Audit retrieval is reliable and bounded for large datasets.

---

## Phase 5: API Keys & Service Accounts

**Objective**: Provide secure automation access without interactive user login.

### Key Tasks
- [ ] Define service account model and key scopes.
- [ ] Implement key create/list/revoke/rotate APIs with one-time secret reveal.
- [ ] Enforce least-privilege scopes in authorization checks.
- [ ] Add key usage metrics/events for monitoring and forensics.
- [ ] Add CLI support for service-account key workflows.
- [ ] Add tests for scope enforcement and key revocation propagation.

### Acceptance Criteria
- [ ] Service accounts can automate scoped operations securely.
- [ ] Revoked/rotated keys stop working immediately.
- [ ] Key workflows are documented and tested end-to-end.

---

## Phase 6: UX & DevEx Integration

**Objective**: Expose user-management behavior clearly in frontend and developer workflows.

### Key Tasks
- [ ] Add login/session UX for remote mode and clear localhost-mode messaging.
- [ ] Add space member management screens with role editing and revoke flows.
- [ ] Add frontend handling for authz failures (permission-aware UI states).
- [ ] Add CLI ergonomics for auth login/profile/token management.
- [ ] Update docs/guide and API docs with setup, troubleshooting, and examples.
- [ ] Add integration/e2e coverage for full user lifecycle.

### Acceptance Criteria
- [ ] Remote users can sign in and manage collaboration from UI/CLI.
- [ ] Permission errors are understandable and actionable.
- [ ] Documentation supports first-time setup without guesswork.

---

## Verification Matrix

- [ ] `mise run test` passes.
- [ ] `mise run e2e` passes.
- [ ] Requirement coverage check confirms all new `REQ-*` are linked to tests.
- [ ] Security regression checks pass for localhost and remote modes.

---

## Definition of Done

- [ ] Milestone 4 acceptance criteria are met for all phases.
- [ ] New requirements and tests are traceable and CI-green.
- [ ] Roadmap/spec/tasks are mutually consistent.

---

## References

- [Roadmap](roadmap.md)
- [Specification Index](../spec/index.md)
- [Security Overview](../spec/security/overview.md)
- [REST API Spec](../spec/api/rest.md)
- [MCP API Spec](../spec/api/mcp.md)
- [Experimental Stories](../spec/stories/experimental.yaml)
