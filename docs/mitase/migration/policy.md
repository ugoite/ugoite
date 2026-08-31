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
  The legacy Foundation and Policy snapshots are guarded by
  `tools/mitase_migration_test.ts`.

The downstream `linked_requirements` and `linked_specifications` edges of the
legacy Policy registry are preserved as typed deferred relations in
`docs/mitase-migration/policy-edges.yaml`. Their targets remain external to the
current Mitase graph until their domains are migrated; the edge itself is not
discarded or replaced by a partial local node.

The explicit Ugoite-owned normative decision for every migrated Policy is
recorded in `docs/mitase-migration/policy-levels.yaml`. Its levels are mirrored
in the canonical Policy rules and checked for completeness; they are not an
implicit conversion from the legacy registry.

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
global gate is tightened. The migration guard also requires an explicit
normative decision for every migrated Policy and exact evidence bindings for
each `must` rule.
