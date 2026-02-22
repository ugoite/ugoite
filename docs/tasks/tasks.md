# Milestone 4: User Management

**Status**: 📋 Planned  
**Goal**: Introduce secure multi-user operation with authentication, authorization, and collaboration while preserving the local-first default model.

This milestone implements the future authentication and collaboration model already referenced in specs, especially:
- `docs/spec/security/overview.md` (current local-only behavior and Milestone 4 auth target)
- `docs/spec/api/rest.md` and `docs/spec/api/mcp.md` (authentication behavior)
- `docs/spec/stories/experimental.yaml` (STORY-010 Multi-User Collaboration)

---

## Constraints (MUST)

- **Local-first storage remains default**: data ownership/local storage principles stay unchanged.
- **Authentication is always required**: localhost and remote mode both require user login.
- **Core/backend boundary**: business logic and file I/O stay in `ugoite-core`; backend remains API orchestration.
- **No security-by-obscurity**: access checks must be explicit, testable, and requirement-linked.
- **Requirements traceability**: all new tests must map to `REQ-*` in `docs/spec/requirements/*.yaml`.

---

## Phase 0: Spec & Requirement Baseline

**Objective**: Freeze the User Management baseline spec so implementation can proceed with intentional breaking changes before production rollout.

### Key Tasks
- [ ] Define **space-scoped mandatory user authentication** in all channels (REST, MCP, UI, CLI via backend).
- [ ] Define metadata Forms `User` and `UserGroup` as default internal identity store.
- [ ] Define **passkey/WebAuthn as primary auth** (passwordless) and optional OAuth2 account linking.
- [ ] Define admin bootstrap with one-time invite token for first WebAuthn registration.
- [ ] Define recovery controls: admin forced reset + backup-code issuance with admin-audit visibility.
- [ ] Define authorization ownership model: space creator is initial admin; admin delegation is supported.
- [ ] Define mandatory Form attribution metadata (`author` / latest updater) for all Form-backed writes.
- [ ] Define Form-level read/write ACL by `User` or `UserGroup`, and materialized-view ACL inheritance.
- [ ] Add/extend `REQ-*` requirements and link to `security`, `api`, `data-model`, and `stories` specs.
- [ ] Add requirement-to-test mapping placeholders (backend/core/cli/frontend/e2e) to keep traceability green.

### Acceptance Criteria
- [ ] Requirements covering mandatory auth, passkey primary auth, OAuth2 linking, bootstrap/recovery, and form ACL exist as `REQ-*` entries.
- [ ] Specs in `security`, `api`, `data-model`, and `stories` are mutually consistent with the baseline.
- [ ] Requirement mappings include concrete planned coverage locations across backend/core/cli/frontend/e2e.
- [ ] Breaking-change policy is explicit for pre-production implementation (no migration required in this phase).

---

## Phase 1: Identity & Auth Foundation

**Objective**: Establish identity primitives and authentication mechanisms for local and remote modes.

### Key Tasks
- [x] Define identity model (`user_id`, principal type, display metadata, disabled state).
- [x] Implement auth provider interface (API key, bearer token; OAuth proxy adapter point).
- [x] Add secure token/key storage and rotation strategy.
- [x] Implement auth middleware and dependency injection for REST/MCP requests.
- [x] Enforce auth for localhost and remote modes, and document migration from current localhost-no-auth behavior.
- [x] Add negative-path handling (expired token, invalid signature, revoked key).

### Phase 1 Implementation Notes (2026-02)
- Authentication foundation is implemented in `ugoite-core` (`ugoite_core.auth`) and reused by backend/CLI adapters.
- Base authentication verification now runs in Rust (`ugoite-core/src/auth.rs`); Python `ugoite_core.auth` is a thin adapter surface.
- Bearer authentication supports static tokens and HMAC-signed tokens with key-id (`kid`) based rotation.
- API key authentication supports service principals and shared revocation by key-id.
- Runtime key material is environment-driven (`UGOITE_AUTH_*`) for pre-production breaking changes without migration.
- `request.state.identity` is populated for REST and MCP HTTP requests.

### Acceptance Criteria
- [x] Localhost and remote modes deny unauthenticated requests to protected endpoints.
- [x] Auth providers can be configured without changing API contracts.
- [x] Identity is available in request context for downstream authorization/audit.

---

## Phase 2: Authorization (RBAC + Resource Scope)

**Objective**: Enforce role-based authorization at space and entry/resource levels.

### Key Tasks
- [x] Define role model (owner/admin/editor/viewer/service) and permissions matrix.
- [x] Keep backend channel adapters thin and delegate authz policy logic to `ugoite-core`.
- [x] Implement space-level access checks (list/read/write/admin operations).
- [x] Implement entry/resource-level checks for forms, entries, assets, search, and SQL.
- [x] Add policy evaluation in `ugoite-core` with thin backend adapters.
- [x] Add explicit authorization error schema and consistent HTTP statuses.
- [x] Add policy tests for allow/deny cases and privilege escalation prevention.

### Phase 2 Implementation Notes (2026-02)
- Central policy engine is implemented in `ugoite-core` (`ugoite_core.authz`).
- Role matrix is space-scoped and supports `owner`, `admin`, `editor`, `viewer`, and `service`.
- Form ACL (`read_principals` / `write_principals`) is enforced on entry read/write paths.
- Backend endpoints use a shared adapter to map authorization failures to a consistent HTTP 403 schema.
- Space creation assigns the authenticated creator as `owner_user_id` and initial admin.
- Policy tests now cover deny/allow behavior and admin-only mutation enforcement.

### Acceptance Criteria
- [x] Unauthorized actions are blocked consistently across REST API flows.
- [x] Authorization behavior is deterministic and centrally test-covered.
- [x] No duplicated authorization business logic exists in backend layer.

---

## Phase 3: Space Membership & Collaboration

**Objective**: Deliver collaborative space sharing aligned with STORY-010.

### Key Tasks
- [x] Define member lifecycle (`invited`, `active`, `revoked`) and invitation rules.
- [x] Implement member APIs (`POST/GET/DELETE /spaces/{space_id}/members...`) in backend + core.
- [x] Add invite token/email abstraction boundary (provider-agnostic).
- [x] Support role assignment and role changes with audit trail hooks.
- [x] Implement concurrency-safe membership updates.
- [x] Add e2e tests for invitation, accept, revoke, and permission transitions.

### Phase 3 Implementation Notes (2026-02)
- Membership lifecycle is implemented in `ugoite-core` (`ugoite_core.membership`) and exposed to backend/CLI/frontend adapters.
- Core membership primitives now handle invite issuance, token acceptance, role updates, and revocation with provider-agnostic invitation delivery.
- Space authorization now requires active membership (or owner/admin), so non-members are denied even when authenticated.
- Backend provides member endpoints under `/spaces/{space_id}/members...` with thin API adapters over core logic.
- CLI and frontend API clients now expose member management calls without duplicating business logic.
- Membership writes are serialized per space in core using async lock-based update guards.

### Acceptance Criteria
- [x] Space owner/admin can invite and manage members.
- [x] Members can access only permitted spaces/resources.
- [x] Collaboration flows satisfy STORY-010 acceptance criteria.

---

## Phase 4: Audit Logging & Attribution

**Objective**: Persist security-relevant events and user attribution for accountability.

### Key Tasks
- [x] Define audit event schema (actor, action, target, timestamp, outcome, request metadata).
- [x] Add event emission for auth, authorization denials, membership, and data mutations.
- [x] Add tamper-evident strategy aligned with existing integrity model.
- [x] Add query/list APIs for audit events with pagination and filters.
- [x] Add retention/redaction policy documentation.
- [x] Add tests for event completeness and immutability guarantees.

### Phase 4 Implementation Notes (2026-02)
- Audit persistence is implemented in `ugoite-core` (`ugoite_core.audit`) and reused by backend adapters.
- Events are stored in an append-only JSONL file at `spaces/{space_id}/audit/events.jsonl` with hash chaining (`prev_hash` + `event_hash`) for tamper-evidence.
- Backend emits audit events for authentication outcomes, authorization denials, and successful mutating HTTP operations.
- Backend exposes `GET /spaces/{space_id}/audit/events` with offset/limit pagination and `action`/`actor_user_id`/`outcome` filters.
- Retention is bounded by `UGOITE_AUDIT_RETENTION_MAX_EVENTS` (default: 5000), and request metadata is intentionally redacted to non-secret fields.

### Acceptance Criteria
- [x] Critical security and collaboration events are audit-logged.
- [x] Actor attribution is available for user-facing change history.
- [x] Audit retrieval is reliable and bounded for large datasets.

---

## Phase 5: API Keys & Service Accounts

**Objective**: Provide secure automation access without interactive user login.

### Key Tasks
- [x] Define service account model and key scopes.
- [x] Implement key create/list/revoke/rotate APIs with one-time secret reveal.
- [x] Enforce least-privilege scopes in authorization checks.
- [x] Add key usage metrics/events for monitoring and forensics.
- [x] Add CLI support for service-account key workflows.
- [x] Add tests for scope enforcement and key revocation propagation.

### Phase 5 Implementation Notes (2026-02)
- Service-account lifecycle is implemented in `ugoite-core` (`ugoite_core.service_accounts`) and persists under space settings as the canonical automation identity store.
- Backend endpoints are thin adapters over core APIs for list/create/revoke/rotate key workflows with one-time secret reveal semantics.
- Space-scoped API key authentication is resolved in core (`authenticate_headers_for_space`) and consumed by backend middleware for all `/spaces/{space_id}/...` requests.
- Scope enforcement is centralized in core authorization (`require_space_action`) using identity-level enforced scopes for service principals.
- Service-key usage emits audit events (`service_account.key.use`) including request metadata and usage counters for forensic visibility.
- CLI adds remote/core routing commands for service-account create/list/key management without duplicating authorization logic.

### Acceptance Criteria
- [x] Service accounts can automate scoped operations securely.
- [x] Revoked/rotated keys stop working immediately.
- [x] Key workflows are documented and tested end-to-end.

---

## Phase 6: UX & DevEx Integration

**Objective**: Expose user-management behavior clearly in frontend and developer workflows.

### Key Tasks
- [x] Add login/session UX for remote mode and clear localhost-mode messaging.
- [x] Ensure frontend and CLI use shared credential env conventions (`UGOITE_AUTH_BEARER_TOKEN` / `UGOITE_AUTH_API_KEY`) when connecting to backend APIs.
- [x] Add space member management screens with role editing and revoke flows.
- [x] Add frontend handling for authz failures (permission-aware UI states).
- [x] Add CLI ergonomics for auth login/profile/token management.
- [x] Update docs/guide and API docs with setup, troubleshooting, and examples.
- [x] Add integration/e2e coverage for full user lifecycle.

### Acceptance Criteria
- [x] Remote users can sign in and manage collaboration from UI/CLI.
- [x] Permission errors are understandable and actionable.
- [x] Documentation supports first-time setup without guesswork.

---

## Verification Matrix

- [x] `mise run test` passes.
- [x] `mise run e2e` passes.
- [ ] Requirement coverage check confirms all new `REQ-*` are linked to tests.
- [x] Security regression checks pass for localhost and remote modes.

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
