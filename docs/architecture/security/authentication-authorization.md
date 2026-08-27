---
title: Authentication and authorization architecture
description: Production identity, portable Space authorization, and constrained access credentials.
sidebar:
  order: 2
---

> v0.1 support boundary: Passkey/WebAuthn bootstrap and login, opaque browser
> sessions, recovery-code + recovery-TOTP Account Self-Recovery, Space
> membership/ACL enforcement, authenticated MCP access, and authorized audit
> reads are supported. OIDC linking, administrator recovery, device
> credentials, agent principals, and audit mutation remain future/reference
> architecture. TOTP is recovery-only.

Ugoite has one production authentication architecture. There is no selectable
development authentication mode, default credential, fixed password, or shared
API key.

## Trust boundaries

Node Identity authenticates a human account on one node with a Passkey. Space
Authorization stores UUIDv7 human and agent
principals, owner/editor/viewer membership, additive resource grants, and Space
audit history below the portable Space directory. Authenticated MCP clients use
short-lived, node-, audience-, Space-, action-, and sender-constrained access
credentials; CLI/device credentials and agent credentials are future designs.

OIDC identities, CLI/device credentials, and agent principals are retained in
the architecture as future designs; they are not v0.1 support commitments.

Host root, direct storage access, and operator-local CLI core mode remain
outside the application authentication boundary. A Node administrator can
administer accounts and credentials but cannot read a Space without a binding to
an active Space principal.

## Durable Node control state

Node state uses the `NodeControlStore` atomic object contract rather than a
mandatory database:

```text
get(key) -> value + version
create_if_absent(key, value)
compare_and_swap(key, expected_version, value)
delete_if_version(key, expected_version)
list_prefix(prefix)
```

The default durable layout is `_ugoite/nodes/<node-id>/`. It is separate from
`spaces/<space-id>/` and excluded from Space export. Local files use an
exclusive lock, owner-only permissions, fsync, and atomic replacement. Remote
OpenDAL storage must advertise conditional create and `If-Match` writes or
startup fails. It never falls back to read-then-write. Browser sessions,
access-token records, and audit events are directly addressed objects; expired
records may be removed lazily or by storage lifecycle rules.

Portable Space authorization state has its own monotonic revision. The command
authorization point is an exact load of `AuthorizationState`; the subsequent
state mutation compares the expected logical revision and uses the storage
revision with conditional replacement. Conflicting member, agent, or policy
changes fail closed instead of overwriting a newer revocation. A revoke
published after that point does not retroactively cancel the admitted command;
the next command observes the newer state. Ordinary authorization/content
mutation does not acquire a durable lease, heartbeat, or task-local write
fence. Independently constructed core services still share the local-process
write lock where the existing single-writer deployment contract uses one.

`UGOITE_NODE_SECRET_KEY` or `UGOITE_NODE_SECRET_FILE` supplies at least 32 bytes
of encryption-root material. Keep it outside the control namespace. Missing key
material is a startup error. TOTP seeds and OIDC client secrets use
authenticated encryption. Setup, invitation, recovery, session, access, and
refresh values are stored only as hashes where recovery of the raw value is
unnecessary.

## Setup and human login

First boot creates a 256-bit, 30-minute setup secret, stores its hash, and shows
the setup URL once in the local log. Normal APIs return `423 Locked` until the
initial Passkey setup is complete. Setup claims existing operator-created
Spaces and creates a UUIDv7 `default` Space only when none exist. It does not
rewrite an older Space layout.

Passkeys require discoverable credentials and user verification. Non-loopback
deployments require HTTPS. Changing the canonical public origin or RP ID after
enrollment fails closed instead of silently invalidating credentials.

The browser holds only an opaque `HttpOnly`, `SameSite=Lax` session cookie;
`Secure` is set for HTTPS. Sessions have a 24-hour idle and 30-day absolute
timeout and are immediately checked against account and revocation state.
Session records can be listed and revoked individually without exposing their
stored verifier hashes. Cookie-authenticated unsafe requests must carry the
canonical `Origin`. CORS is off by default; `UGOITE_CORS_ALLOWED_ORIGINS`
enables an exact comma-separated allowlist.

## External identity and recovery boundary

OIDC linking, administrator recovery, account discovery, and operator overrides
remain future/reference architecture. Account Self-Recovery is available only
for a HumanAccount with explicitly enrolled recovery TOTP and issued Recovery
Codes.

OIDC uses Authorization Code with PKCE, discovery, exact issuer/redirect checks,
state, nonce, and signature validation. The account key is exact issuer plus
subject; email is never an identity key. An upstream token is never accepted by
Ugoite resources. A new OIDC account requires an unexpired Ugoite invitation.
Existing local accounts may accept invitations directly and may link an OIDC
issuer/subject pair after recent Passkey authentication.

Account Self-Recovery requires the exact Account ID, one currently valid
offline Recovery Code, and a valid recovery-only TOTP code. Attempts are
throttled and locked after five consecutive failures for 15 minutes. Neither
factor works alone. Start only issues a short-lived WebAuthn replacement
challenge; the code is consumed and all credential authority is replaced only
when finish succeeds. Successful self-recovery preserves the HumanAccount and
Space identity, advances credential generation, replaces the account's
Passkeys, invalidates pre-recovery authentication authority, rotates all
Recovery Codes, establishes a new browser session, and emits a secret-free
Node audit event. A separate owner-approved Space access recovery accepts a
15-minute token whose hash is authoritative; an encrypted bearer is retained
only for a bounded one-time response window. It is issued by an active human
Space Owner with a recent Passkey; the target completes a five-minute WebAuthn
challenge. A fresh HumanAccount is created and only the target Space
Principal's Node binding is replaced. Principal ID, membership, role, ACL,
resource ownership, audit identity, old account credentials/sessions, and
unrelated bindings are preserved. A durable recovery fence and binding
revision reject stale or concurrent completion. A crash after the Node CAS
leaves a `node_committed_space_fence_pending` marker; startup reconciliation
completes the matching fence before another recovery mutation is accepted.

## Space authorization

Each Space directory is its immutable UUIDv7 ID. `slug` and `name` are mutable
metadata. Portable authorization is stored in
`spaces/<space-id>/security/principals.json`; Node account bindings remain in
the Node control store. At least one active human owner is mandatory.

Entry, Asset, Form, Saved SQL, search, link candidates, history, SQL sessions,
counts, joins, aggregates, and MCP use the same core authorizer. Policies
inherit the Space role by default and add explicit grants. There are no deny
rules in this release. Typed Form references do not create an inferred ACL
edge: Asset byte operations require an explicit caller-authorized containing
Entry context before applying the Asset and Space resource policy. Query
engines receive the authorized Entry ID set before filtering, pagination, joins,
or aggregation.

## Authenticated MCP and Remote CLI credentials

Authenticated MCP access is supported when the client presents a valid
server-issued scoped credential. Agent principals and human-approval flows
described below remain future/reference architecture.

`ugoite auth login` uses device authorization. The CLI creates a fresh P-256 key,
shows a user code and verification URL, and polls at the server-provided
interval. Approval shows the device, Space, and action set and requires a recent
Passkey. CLI access tokens are five-minute opaque credentials DPoP-bound to the
device key; human MCP device credentials are resource-bound Bearer credentials
or an explicitly presented DPoP credential.
30-day refresh credentials rotate on every use. Reuse revokes the device grant.
The private key uses the OS keychain where available and otherwise an owner-only
file. The proof `htu` is checked against the configured canonical public URL
plus the actual scheme, authority, and path; query and fragment are excluded.
Client-supplied forwarding headers cannot replace it. REST CLI credentials omit
`resource` and use the issuer audience; MCP credentials use exactly
`{issuer}/mcp` for resource and audience.

Agents are Space principals with a human sponsor, human owners, an expiry/review
deadline, an autonomous/delegated mode, explicit grants, and independent public
keys. Autonomous agents inherit no sponsor rights. Delegated requests evaluate
the intersection of human policy, agent policy, token constraints, and resource
policy. Agents cannot manage members or agents. An active human with the
current permission can issue a
single-use human approval for the exact supported delete/share mutation; the
server binds it to the operation, resource, intent, actor credential, expiry,
and lifecycle epochs before consuming it. Member, owner, agent, and
Space-management changes remain Passkey-only.

OAuth authorization-server and protected-resource metadata are published under
the standard `.well-known` paths. Ugoite resources accept only Ugoite-issued
opaque credentials. MCP credentials are audience-, Space-, action-, lifetime-,
and sender-constrained. Browser-capable clients use an approval page and
Authorization Code with S256 PKCE; the authorization code is five-minute,
single-use, redirect-bound, client-bound, and combined with a registered DPoP
public key. Input-constrained clients use Device Authorization and autonomous
agents use private-key client assertions.

## Moving and recovering a Space

There is no in-place Space migration or owner-claim compatibility flow before
v1. Move a Space only as a complete operator-controlled prefix or a
backend-native consistent snapshot, including the Catalog Head, reachable
publication records, Iceberg metadata/data, assets, SQL metadata, and portable
authorization state. Catalog Head object versioning is required for disaster
recovery. An old or incomplete layout fails explicitly instead of being
rewritten.

After restoring a Space on another Node, run normal setup to establish the
destination Node binding. Node sessions, invitations, agent credentials, and
tokens remain Node-local.
