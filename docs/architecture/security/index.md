---
title: "Security architecture"
description: The node and Space security boundaries that protect identities, authorization, and portable data.
sidebar:
  label: "Security"
  order: 3
---

Security follows the same portability rule as the rest of Ugoite: node-local
identity is not smuggled into a portable Space, while Space authorization stays
with the data it protects.

- [Authentication and authorization](authentication-authorization.md) is the
  implementation-level overview of node identities, Passkeys, OIDC, CLI devices,
  agents, ACLs, and audit state.
- The normative API and security contracts are grouped under the
  [specification security overview](../../spec/security/overview.md).
