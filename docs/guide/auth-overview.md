---
title: Authentication and authorization
---

Ugoite separates identity that belongs to one server from identity that must
move with a Space.

## Identity boundaries

Node Identity contains human accounts, Passkeys, optional OIDC links, browser
sessions, CLI devices, and Node administrator roles. It is stored through the
atomic `NodeControlStore` below `_ugoite/nodes/{node_id}` and is never exported
as part of a Space. `UGOITE_NODE_CONTROL_URI` may select a separate durable
OpenDAL location. OIDC users are keyed by the
exact `(issuer, subject)` pair; email is display data only.

Each Space stores an immutable UUIDv7 `space_uid`, stable human or agent
principals, memberships, ACL policies, and append-only authorization audit
events. Node-to-Space bindings are node-local. Moving a Space preserves
attribution and authorization while requiring an owner to bind a new Node
account.

Node administrators configure authentication and operate the server. Space
owners manage one Space. There is no administrative Space and neither role
implies the other.

## First setup

On the first server start, Ugoite writes only a SHA-256 hash of a
cryptographically random, 30-minute, one-use setup secret and prints a setup URL
to the local console/container log. Opening that URL starts WebAuthn
registration. The Node remains uninitialized until either two Passkeys are
registered, or one Passkey plus confirmed TOTP and the issued recovery codes
are established. Until then, the setup session can access only credential
strengthening endpoints.

The first account receives `node_admin`, an owner principal in the initial
`default` Space, eight one-use recovery codes, and an opaque browser session.
Save the recovery codes immediately. The setup secret cannot be reused. Visiting
the server first never grants administrator access.

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

Credential enrollment, device approval, role/ACL changes, agent lifecycle, OIDC
configuration, and owner rebinding require a Passkey authentication within the
preceding five minutes. Accounts should register two or more Passkeys. Ugoite
refuses to remove the final Passkey.

## Recovery

Initial setup returns eight one-use recovery codes. Their hashes are stored; the
plaintext values are shown once. After signing in with a Passkey, enroll TOTP
through `POST /auth/recovery/totp/start` and confirm the first code at
`POST /auth/recovery/totp/finish`. The TOTP seed is encrypted at rest with a
Node-local key.

Recovery requires both one unused recovery code and a current TOTP at
`POST /auth/recovery/start`. A valid pair is consumed before Ugoite issues a
five-minute WebAuthn registration challenge. Completing
`POST /auth/recovery/finish` adds a new Passkey and opaque session. TOTP by
itself never authorizes credential registration. The last owner and the final
Passkey cannot be removed.

## Invitations and OIDC

Space owners issue a one-use invitation URL. Only the invitation hash, expiry,
creator, Space UID, and requested role are stored. The recipient registers a
Passkey, accepts with an existing signed-in account, or starts an enabled OIDC
authorization-code flow with PKCE. A new OIDC
subject cannot create an account without an invitation. Ugoite validates the
provider ID token and nonce, then issues its own opaque session; provider tokens
are never accepted by the Ugoite API. A signed-in user can add an OIDC method
only after recent Passkey authentication; Ugoite refuses to link an
issuer/subject pair already owned by another account.

## CLI devices

`ugoite auth login` generates a P-256 key, requests a device code, and prints a
verification URL and short user code. Approval shows the target Space and
requested actions. The CLI stores its private key in the OS keychain when
available, otherwise in an owner-only file. Access tokens last five minutes;
refresh credentials rotate on every use and expire after 30 days.

Access tokens are opaque random values; the Node control store saves only their hashes and
server-side issuer, Node, Space, action, actor-chain, expiry, credential, and
key-binding metadata. They can be revoked immediately and expose no claims to
the holder. Every API request uses DPoP. Ugoite validates the proof signature, registered
key thumbprint, method, URI, access-token hash, timestamp, and one-use `jti`. A
copied access token is unusable without the device key. Devices and last-use
state are visible and individually revocable. The expected URI comes from the
Node's canonical public URL and the actual request path, never a client-provided
forwarding header.

## Agents and MCP

Agents are Space principals, not API keys. Every agent has a human sponsor and
at least one human owner, its own public-key credential, explicit actions,
status, and a required expiry or review deadline. Autonomous tokens contain only the agent grant.
Delegated tokens contain an actor chain and use the intersection of the agent
grant and the current human's permissions.

Agents cannot manage members, transfer ownership, or manage other agents.
`delete` and `share` are unavailable to all noninteractive credentials until an
interactive Passkey approval protocol exists.

HTTP MCP is an OAuth protected resource. Discovery is available at
`/.well-known/oauth-protected-resource` and
`/.well-known/oauth-authorization-server`. Human clients use device
authorization; autonomous agents use an ES256 client assertion. Tokens are
restricted to one node, one `space_uid`, actions, principal, credential, and
short expiry.

## Data authorization

Space roles map to actions: owner has read/create/update/delete/share, editor
has read/create/update, and viewer has read. The first ACL version applies only
to Entry and Asset. A policy inherits the Space role by default and adds
explicit grants. Setting `inherit_space_role` to false creates a private
allow-list without introducing deny rules. Typed Form references do not create
an inferred ACL edge: Entry reads authorize the containing Entry, while Asset
byte operations use the Asset and Space resource policy.

The shared Rust Authorizer is called by REST, CLI, MCP, search, structured
query, and SQL sessions. SQL receives the authorized Entry ID set before tables,
joins, counts, or aggregates are built, preventing inference from post-query
filtering.

## Removed credentials

Ugoite has no fixed password or preconfigured long-lived bearer credential,
default credential, bootstrap bearer, or development authentication bypass.
Tests may use in-memory persistence and clocks, but product configuration cannot
activate a fake identity path.
