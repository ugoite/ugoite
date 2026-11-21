---
title: "Maintenance budget and evidence"
---

The normative rules live in Mitase `POL-018`, under `PHIL-003`. This page is
a short decision procedure for the existing pull-request Maintenance Budget
Review, not another authoritative feature inventory or quality gate.

## Semantic ownership

Identify the **one implementation** that decides the rule. Portable product
semantics belong in Rust Domain/Core; authoritative Space execution and Node
identity are distinct Rust boundaries. Browser, CLI, Server, MCP, and WASM may
adapt transport and hold disposable Work. Trusted Local and authorized Remote
entry checks are not interchangeable; share execution **after** scopes and
leases have been established, rather than inventing an authorization bypass.

When refactoring, remove the old duplicate and its redundant fixtures. Avoid
creating a generic framework only to eliminate a few similar-looking lines
whose invariants differ.

## Compatibility evidence

Classify each compatibility branch using an **actual released writer or
versioned public contract**: tag/artifact, exact persisted carrier, reader, and
a frozen semantic assertion. The v0.1 Knowledge compatibility floor and
separately released contracts remain non-negotiable. Do not preserve code
solely because it once existed on main or is mentioned in an old design plan.

DerivedRelation and other non-authoritative caches must be rebuildable from
the current Space. Removing old derived encodings must still protect exact
Head publication, stale-writer rejection, in-flight reads, and GC fencing.

## Verification cost

For every invariant, select the smallest unit/property test that proves the
meaning and add real integration evidence **only** for a genuinely distinct
Storage, authorization, process, browser, or provider boundary. Preserve
negative and concurrency cases. Prefer structural boundedness assertions to
arbitrary large repetition counts; keep high-volume profiles outside
mandatory correctness gates. Use measured job duration and critical-path
data when changing CI lanes.

A deleted or consolidated test is acceptable only if the remaining tests
prove the same protected behavior. A named test or a source-file binding
does not, by itself, demonstrate a passing runtime assertion.

## Specification lifecycle

Change the active Mitase Requirement, Feature, and verification bindings with
the code. A retired editor, protocol, or requirement must not linger as an
active reference, duplicated type registry, or old-name guard. Keep historical
release acceptance tied to its original candidate SHA and label it historical;
do not recast it as evidence of the current build.

The PR Maintenance Budget Review checks that a reviewer records whether the
change adds a maintained surface and, if so, its owner, release basis, and
minimal proof. This review is **not** a substitute for the existing Knowledge
Compatibility Review or execution of the CI quality gates.
