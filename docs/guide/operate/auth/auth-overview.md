---
title: Authentication and authorization
sidebar:
  order: 2
---

> v0.1 support boundary: mandatory authentication, owner bootstrap,
> Passkey/WebAuthn registration and login, opaque browser sessions,
> owner-approved Space access recovery, recovery-code + recovery-TOTP Account
> Self-Recovery, Remote CLI device credentials, Space membership/ACL
> enforcement, authenticated MCP access, and authorized audit reads are
> supported. OIDC linking, administrator recovery, agent principals, and audit
> CRUD remain future scope. TOTP is recovery-only.

Ugoite separates identity that belongs to one server from identity that must
move with a Space.

## Identity boundaries

Node Identity contains human accounts, Passkeys, browser sessions, device
credentials, and Node administrator roles. CLI and MCP device credentials use
the same internal machinery but are separated by resource and audience; OIDC
links and agent principals are future designs. It is stored through the
atomic `NodeControlStore` below `_ugoite/nodes/{node_id}` and is never exported
as part of a Space. `UGOITE_NODE_CONTROL_URI` may select a separate durable
OpenDAL location. OIDC users are keyed by the exact `(issuer, subject)` pair;
email is display data only.

Each Space stores an immutable UUIDv7 `space_uid`, stable human or agent
principals, memberships, ACL policies, and append-only authorization audit
events in portable authorization state. Node-to-Space bindings are node-local.
Moving a Space preserves attribution and authorization; normal setup establishes
the destination Node binding.

Node administrators configure authentication and operate the server. Space
owners manage one Space. There is no administrative Space and neither role
implies the other.

## First setup

On the first server start, Ugoite writes only a SHA-256 hash of a
cryptographically random, 30-minute, one-use setup secret and prints a setup URL
to the local console/container log. Opening that URL starts WebAuthn
registration. The Node remains in setup until the administrator's second
Passkey ceremony is completed. Until then, the setup session can access only
credential-strengthening endpoints.

The first account receives `node_admin`, owner bindings for existing supported
operator-created Spaces (or a new `default` Space when none exist), a one-time
recovery-code set for safekeeping, and an opaque browser session. Account
Self-Recovery becomes available after the account explicitly enrolls a
recovery-only TOTP in Security Settings. The setup secret cannot be reused.
Visiting the server first never grants administrator access.

## Browser login and sessions

Passkey is the standard login method. Ugoite requests a discoverable credential
with user verification required. The browser receives only `ugoite_session`, an
opaque random session ID with `HttpOnly`, `SameSite=Lax`, and a 30-day maximum
lifetime. Idle sessions expire after 24 hours. `Secure` is added whenever
`UGOITE_PUBLIC_ORIGIN` uses HTTPS. Tokens and WebAuthn state are never stored in
browser-readable cookies.

The account security page lists browser sessions with creation, last-use,
expiry, and revocation state. A user can revoke any individual session; the
change is enforced on its next request.

Credential enrollment, MCP device approval, and role/ACL changes require a
Passkey authentication within the preceding five minutes. Agent lifecycle and
OIDC configuration are future scope. Accounts should register two or more Passkeys. Ugoite refuses to remove
the final Passkey.

## Account Self-Recovery

An account that has explicitly enrolled recovery protection can recover after
losing all Passkeys at `/recover/account`. The user enters the exact Account
ID, one currently valid offline Recovery Code, and a valid recovery-only TOTP
code, then completes a short WebAuthn registration ceremony. Successful
completion preserves the HumanAccount, Space principals, memberships, roles,
ACLs, ownership, and bindings while replacing the Node-local credential
generation with a new Passkey, invalidating pre-recovery authentication
authority, rotating all Recovery Codes, creating a new browser session, and
emitting a secret-free Node audit event. TOTP is not accepted as a normal login
factor, and neither factor works alone. OIDC linking, administrator recovery,
account discovery, and operator overrides remain outside the v0.1 contract.

### Supported: owner-approved Space access recovery

An active human Space Owner can recover another active human member with a
recent Passkey session. `POST /spaces/{space_id}/admin/recovery/force-reset`
generates a server-side request identity and returns a 15-minute one-time
token; give it to the member over a trusted channel. The member submits the
token to `/auth/recovery/owner/start`, completes the
five-minute WebAuthn challenge at `/auth/recovery/owner/finish`, and receives a
new Passkey, session, and eight recovery codes once. The target Space Principal
ID, membership, role, ACL, resource ownership, and audit identity are
preserved. Only its Node-local binding moves from the old HumanAccount to the
new one. The old account's credentials, sessions, device credentials, and
bindings in other Spaces remain valid; it is denied only by the recovered
Space's binding lookup.

The completion checks the current binding revision, lifecycle/authorization
fence, target membership, issuer ownership, expiry, and one-use state. Binding
replacement and approval consumption are one persisted Node state transition;
concurrent completion has exactly one winner. Space audit delivery is durable
and separate from completion, so the response reports `delivered` or `pending`
without reissuing secrets.

## Invitations and future identity providers

Space owners issue a one-use invitation URL. Only the invitation hash, expiry,
creator, Space UID, requested role, and its durable acceptance claim are stored.
The recipient registers a Passkey or accepts with an existing signed-in
account. OIDC authorization-code login and identity linking are future scope;
they are not required for v0.1 membership acceptance and provider tokens are not
accepted by the v0.1 API.

An account has at most one Node binding in a Space. If an already-bound account
opens an invitation for that same Space, acceptance is idempotent and keeps the
existing principal instead of creating a second membership.

Invitation registration claims keep their account and principal fixed. If the
Space membership or Node binding step fails after the claim is durable, opening
the same invitation again requires the claimant to complete normal Passkey
login before authenticated acceptance resumes finalization; the invitation
token never authenticates the claimed account. The original invitation expiry
does not block that retry. A conflicting or revoked Space principal is reported
as a conflict instead of being silently replaced.

An account has at most one Node binding in a Space. If an already-bound account
opens an invitation for that same Space, acceptance is idempotent and keeps the
existing principal instead of creating a second membership.

## Supported: Remote CLI devices

`ugoite auth login` generates a fresh P-256 key, requests a device code, and prints a
verification URL and short user code. Approval shows the target Space and
requested actions. The CLI stores its private key in the OS keychain when
available, otherwise in an owner-only file. Access tokens last five minutes;
refresh credentials rotate on every use and expire after 30 days.

The default actions are `read,create,update`; delete and share are never added
implicitly. REST CLI authorization requests omit `resource` and receive a token
whose audience is the Node issuer. MCP requests use exactly `{issuer}/mcp` for
both resource and audience, and the two credential types cannot cross-use.

Access tokens are opaque random values; the Node control store saves only their
hashes and server-side issuer, Node, Space, action, actor-chain, expiry,
credential, and key-binding metadata. They can be revoked immediately and expose
no claims to the holder. Every REST API request uses DPoP. Ugoite validates the
proof signature, registered key thumbprint, method, scheme/authority/path `htu`
(without query or fragment), access-token hash, timestamp, and persisted
one-use `jti`. A copied access token is unusable without the device key.
Devices and last-use state are visible and individually revocable. The expected
URI comes from the Node's canonical public URL and the actual request path,
never a client-provided forwarding header.

## Authenticated MCP and future agents

Authenticated MCP requests use the same user identity and Space ACL checks as
REST. Agent principals are future scope. When enabled, agents will be Space
principals, not API keys. Every agent has a human sponsor and
at least one human owner, its own public-key credential, explicit actions,
status, and a required expiry or review deadline. Autonomous tokens contain only
the agent grant. Delegated tokens contain an actor chain and use the
intersection of the agent grant and the current human's permissions.

Agents cannot manage members, transfer ownership, or manage other agents.
For automation, a recently reauthenticated human issues
`POST /spaces/{space_id}/approvals` with the operation, strict operation-specific
`mutation`, actor credential, and a 1–300 second `expires_in_seconds`. The
server derives action/resource and the actor principal from the credential. The
response contains a one-time `approval_token`; pass it as
`X-Ugoite-Human-Approval` (the CLI accepts `--human-approval` or
`UGOITE_HUMAN_APPROVAL`). The token is bound to the canonical intent and is
atomically consumed before the mutation. A replay, route/body mismatch, expiry,
or uncertain mutation result requires a new approval. Member, owner, agent,
and Space-management operations remain Passkey-only.

HTTP MCP itself is supported only after the caller is authenticated. Discovery
is available at `/.well-known/oauth-protected-resource` and
`/.well-known/oauth-authorization-server`. A human MCP client requests a
resource-bound device grant with `resource: {issuer}/mcp`, the signed-in owner
approves it at `/device`, and the client exchanges the device code at
`/oauth/token`. Autonomous agent credentials are future scope. MCP tokens are
restricted to one node, one `space_uid`, actions, principal, credential, and
short expiry.

## Data authorization

Space roles map to actions: owner has read/create/update/delete/share, editor
has read/create/update, and viewer has read. The first ACL version applies only
to Entry and Asset. A policy inherits the Space role by default and adds
explicit grants. Setting `inherit_space_role` to false creates a private
allow-list without introducing deny rules. Typed Form references do not create
an inferred ACL edge: Asset byte operations require an explicit authorized
containing Entry context and then apply Asset and Space resource policy.

The shared Rust Authorizer is called by REST, CLI, MCP, search, structured
query, and SQL sessions. SQL receives the authorized Entry ID set before tables,
joins, counts, or aggregates are built, preventing inference from post-query
filtering.

## Removed credentials

Ugoite has no fixed password or preconfigured long-lived bearer credential,
default credential, bootstrap bearer, or development authentication bypass.
Tests may use in-memory persistence and clocks, but product configuration cannot
activate a fake identity path.
