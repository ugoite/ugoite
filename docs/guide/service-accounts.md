---
title: Agent identities
---

Automation uses Agent Principals with registered P-256 public keys. Shared
secrets and long-lived API keys are not supported.

A Space owner creates an agent with a display name, human sponsor/owners,
explicit actions, expiry, and public JWK. The agent signs an ES256 client
assertion to obtain a five-minute opaque DPoP access token. Autonomous access is
limited to the agent grant. A delegated token additionally records the human
principal and intersects both permission sets.

Revoke the agent to disable all of its credentials. Agent creation, delegation,
use and revocation are attributed in the Space audit chain. Agents cannot
manage membership, ownership, or agents, and cannot receive `delete` or `share`.
