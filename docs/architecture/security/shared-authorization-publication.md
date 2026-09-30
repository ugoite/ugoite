---
title: Shared authorization and content publication
description: S1 coordination contract for shared Space writers.
---

This page records the selected S1 coordination target, its current support
boundary, and the evidence required before shared multi-process writes can be
claimed as supported.

## Decision

**S1 is the selected target.** A shared Space may be written by multiple server
processes only when authorization changes and every protected content
publication participate in one backend-enforced, per-Space linearization point.
The current implementation does not meet this contract. Issue
[#3315](https://github.com/ugoite/ugoite/issues/3315) remains a release blocker
until implementation and the two-process backend acceptance evidence are
complete. No workaround or configuration flag makes the current shared
authorization path supported.

The linearization point is a single mutable Space coordinator Head updated with
an exact backend compare-and-swap. The Head binds the immutable current
authorization snapshot and the visible publication roots for protected Space
content. Authorization updates publish a new authorization snapshot by advancing
this Head. Content mutations publish prepared immutable content by advancing the
same Head while retaining the authorization snapshot against which they were
checked, except when the operation consumes a single-use approval. In that case
the same CAS advances both the content root and an authorization snapshot in
which the approval is consumed. All readers resolve current protected state
through the Head. A stale writer therefore cannot publish after a revocation:
its CAS loses once the revocation advances the Head.

The ordering is Node recovery fence, then an exact per-Space coordinator Head
read, authorization evaluation, immutable resource preparation, and one Head
CAS. There is no expiring Space owner lease to steal: the expected Head revision
is the fencing token. A competing or delayed writer with a stale revision loses
its CAS. Local filesystem callers keep the existing single-process and OS
file-lock path; local CLI operator authority remains separate from remote Server
principal authorization.

Storage owns only the mechanics needed to read the exact coordinator revision
and conditionally replace it. `ugoite-iceberg::authorization` owns the current
ACL snapshot, permission evaluation, and approval lifecycle. Adapters cannot
bypass the coordinator by calling a lower-level protected write API.

## Supported backend matrix

| Topology                                                          | Read                                      | Protected write                                                                 | Evidence                                          |
| ----------------------------------------------------------------- | ----------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------- |
| Local filesystem, one writer process                              | Supported                                 | Supported through process and OS file serialization                             | Existing local storage and recovery tests         |
| OpenDAL S3 backend, multiple server processes                     | Supported                                 | Admitted after exact configured-backend CAS probe                               | Required Silo 2026-08-04 S3 acceptance lane       |
| Other shared OpenDAL backend, multiple server processes           | Supported where exact reads are available | Rejected before the first content publication with `StorageMutationUnavailable` | No independent-process backend evidence yet       |
| Shared backend without exact, strongly consistent coordinator CAS | Supported where exact reads are available | Rejected before the first content publication with `StorageMutationUnavailable` | Capability probe and fail-closed tests            |

The S1 runtime admits the OpenDAL S3 backend family as one supported backend
class; other OpenDAL schemes remain read-only even when their generic capability
bits and single-object probes pass. Each configured S3 endpoint must pass the
exact behavioral probe before mutation. The required independent-process CI
acceptance for this backend class uses the pinned Silo 2026-08-04 configuration
(the community-maintained MinIO fork) and the same OpenDAL S3 adapter. This
evidence relies on that adapter's common S3 conditional-write semantics plus
the exact per-endpoint probe; it does not claim independent-process coverage
for another OpenDAL service adapter. A
generic capability bit, single-object probe alone, or in-memory test is not
sufficient to admit another backend class.

The protected publication scope includes all AuthorizationState changes
(including membership revocation, policy, lifecycle, and human approval
consumption) and every server-reachable authorization-dependent content
mutation: Entry, Form, Asset, Saved SQL, and Change revert/Run undo. A direct
storage or service call cannot bypass this boundary. Local CLI operator writes
remain in the local operator trust boundary and do not acquire a remote Server
principal identity.

The current `Authorizer` state-writer inventory is:

- `initialize_owner`;
- human approval issue, consume, audit queue, and audit-delivery
  acknowledgement;
- recovery-fence reserve, complete, and release;
- policy set (with and without audit), human-member add, role change, and
  principal revoke;
- agent create, create-or-recover, revoke, and last-used update.

The implementation must route each of these through the shared `write_state*`
publication boundary and mechanically detect bypasses. Adding a new
AuthorizationState writer requires updating the inventory and its boundary test.
This list covers state writes only; Node lifecycle fences remain an additional
outer ordering boundary.

## Publication contract

1. Read the latest exact coordinator Head and the authorization snapshot it
   references. Evaluate current permission, target-wide requirements, and any
   single-use approval from that snapshot.
2. Prepare immutable content and a canonical receipt. Do not expose prepared
   objects until the coordinator Head CAS makes them reachable.
3. Publish with a conditional update from the exact observed Head. The
   authorization snapshot and visible content roots are ordered by this one CAS.
   If the CAS loses, discard the unreachable preparation and start a new
   authorization decision from the latest Head; never publish against the stale
   decision. Revert validates every target before it prepares any inverse. Run
   undo keeps each inverse as an independently committed Change and reauthorizes
   before each next inverse.
4. If the backend response is ambiguous, resolve the outcome from the exact Head
   and canonical receipt. If it remains unknown, stop and report an unknown
   outcome; do not resend the write or treat `RunId` as an idempotency key. A
   later writer still uses CAS from the exact current Head, so an outstanding
   stale request cannot overwrite a newer committed Head.

Heartbeat loss and process timeout do not establish a commit outcome. The
backend CAS and immutable receipt chain must settle competing or delayed
requests without relying on owner expiry, cleanup, or clock comparisons.

Consuming a single-use human approval is part of the same publication CAS as the
protected effect: it advances both the authorization snapshot (with the approval
marked consumed and its outcome recorded) and the affected content root. If the
effect's outcome is unknown, another request must not reuse that approval while
resolving the canonical receipt.

## S1 acceptance evidence

The exit evidence uses two independent server processes over the same backing
bucket and prefix, with separate clients holding authorization snapshots. It
records coordinator Heads and canonical receipts for:

- Entry update racing member revocation at the final CAS;
- Form, Asset, and Saved SQL writes traversing the same final-CAS boundary;
- Change revert racing revocation after all target ACL checks;
- Run undo with at least two Changes, revocation after the first inverse, and
  history-based reconstruction of completed and pending work;
- owner loss, delayed publication, unknown Head and ACL outcomes, and competing
  writers, with no fail-open or automatic resend;
- backend capability failure before any protected content is published.

The focused publication-boundary matrix must demonstrate that Entry, Form,
Asset, and Saved SQL writers all route to the coordinator CAS and that every
AuthorizationState writer advances its authorization snapshot there.

The A/B race must show either content publication then revocation, or revocation
then rejection of the stale content publication. It must never show a protected
publication ordered after a committed revocation using the earlier authorization
snapshot. Revert must be all-target atomic; Run undo may retain already
committed inverse Changes and must report the remaining work.

The required independent-process test runs against each backend class claimed
in the supported matrix. Same-process memory tests are supplemental. Until the
S3 acceptance passes on the same candidate SHA as the implementation, shared
multi-process mutation remains unsupported and blocks release. Adding another
backend class requires an independent-process lane and a matrix entry before
runtime admission is widened.
