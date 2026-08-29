---
title: "Authentication and agents"
description: Understand node identities, Space authorization, and automation credentials.
sidebar:
  label: "Authentication & agents"
  order: 2
---

Ugoite keeps node identity separate from portable Space authorization. Human
accounts and browser/CLI sessions belong to a node; Space memberships, ACLs,
and authorization history travel with the Space. Agent Principals remain future
scope.

> Release note: Agent/service-account workflows in this reference group are
> future capability and are not supported v0.1 product operations.

## Read this group in order

1. Start with [Authentication and authorization](auth-overview.md) for setup,
   sessions, recovery, invitations, CLI devices, agents, and policy boundaries.
2. Use [Agent identities](service-accounts.md) when an automation client needs
   access without a shared password or long-lived API key.
