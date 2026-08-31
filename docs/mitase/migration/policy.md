---
title: 'Specification migration authority'
---

This document defines how Ugoite moves from its legacy specification registry
to the canonical Mitase graph without creating two competing meanings.

## Authority by migration state

- A migrated domain is authoritative in `docs/mitase`.
- An unmigrated domain remains authoritative in its existing `docs/spec`
  records.
- Migrated legacy records remain read-only migration evidence until the domain
  migration is complete; they are not a second editable specification source.
- The legacy Foundation, Policy, Requirement, and Feature records for a
  migrated domain are read-only snapshots. The repository's migration review
  compares each canonical record with its legacy source before authority is
  changed; this convention does not make Mitase execute migration tooling.

The downstream `linked_requirements` and `linked_specifications` edges of the
legacy Policy registry are preserved as typed deferred relations in
`docs/mitase-migration/policy-edges.yaml`. Their targets remain external to the
current Mitase graph until their domains are migrated; the edge itself is not
discarded or replaced by a partial local node.

The explicit Ugoite-owned normative decision for every migrated Policy is
recorded in `docs/mitase-migration/policy-levels.yaml`. Its levels are mirrored
in the canonical Policy rules and reviewed for completeness; they are not an
implicit conversion from the legacy registry. `enforces` claims are reserved
for repository validators or required workflows that mechanically check the
rule. Architectural placement and documented responsibility are represented
as `evidences` claims.

## Meaning before evidence

A migration preserves the source Requirement meaning and expresses it as one
or more Mitase Criteria. Missing, ambiguous, or unavailable evidence is
represented as an unverified Criterion or an explicit migration gap. It must
not be represented by weakening or deleting the Criterion.

Implementation responsibility and Verification Claims remain separate. A
resolved implementation target may show where a behavior belongs, while a
Verification Claim is added only for exact current evidence with complete
declarative runner metadata. Mitase records and validates these relationships;
Ugoite repository tooling and CI remain responsible for executing checks.

## Staged adoption

The initial gate validates the written migrated graph and its declared
relationships. `readiness.target: off` and
`changed.require_owned_changes: false` are intentional while domains are
migrated incrementally; they do not claim that all Ugoite meaning is already
represented in Mitase. Each domain migration must preserve its full meaning,
make evidence gaps visible, and raise its own validation coverage before the
global gate is tightened. The migration review requires an explicit
normative decision for every migrated Policy and an exact enforcement or
evidence binding for each `must` rule. For migrated Search, Entry, and Form,
the staged domain ratchet is a repository review rule: implementation-affecting
changes update the canonical Requirement/Feature records and add exact proof
or record an explicit unverified gap, even while the global owned-change gate
remains off.

`docs/mitase` is a migration staging surface, not a new permanent Ugoite
information architecture. Once the legacy registry machinery is retired,
Ugoite may consolidate executable specification records under its `docs/spec`
information architecture while preserving the Mitase schema and authority
semantics.
