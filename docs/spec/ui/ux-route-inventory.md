---
title: 'UX Route Inventory'
---

This page records the current inventory of every frontend route surface
covered by the UX improvement plan. It changes no UI behavior.

## Overview

Current route inventory (docs-only).

Source: `find frontend/src/routes -type f | sort` at HEAD. No UI behavior
change.

Plan mapping: WP-01 §§4–9 (UX Improvement Implementation Plan).

- §4 Navigation shell, back chain, and context singularity.
- §5 Entry workspace (detail, new, info; no duplicate context, no decorative
  cards).
- §6 Lists, search, forms, SQL/query surfaces (full-row activation, UUIDs
  advanced-only, one-row action bar at 390px).
- §7 Localization (dictionary copy; legacy hardcoded strings absent) —
  cross-cutting, applies to every surface below.
- §8 Responsive and accessibility (usable at 390px; accessible names and visible
  focus; fixed surface tokens) — cross-cutting, applies to every surface below.
- §9 History and append-only restore.

Current UX notes (same plan sections, no new sections):

- Navigation is Form-first: Forms is the entry point to Entries, and Entries
  is not an independent destination (§4).
- Each Form's current Entries live in its Form Entries workspace; there is no
  unscoped all-Forms Entries list surface (§5 + §6).
- Search owns a raw draft that commits on submit; `?q=` seeds the field and
  the first search (§6).
- Surfaces present structure first; prose remains only where it instructs,
  recovers, warns, or disambiguates (§7).

Coverage: 100% — all 41 route implementation files under `frontend/src/routes`
are listed below. Companion `*.test.*` files are listed in the second table and
map to the same surface as their route.

## Route implementation files (41)

| #  | Route file                                                                           | URL shape                                                     | Plan section                 | Surface                                                 |
| -- | ------------------------------------------------------------------------------------ | ------------------------------------------------------------- | ---------------------------- | ------------------------------------------------------- |
| 1  | `frontend/src/routes/[...404].tsx`                                                   | `/...404` (not-found fallback)                                | §4                           | Not-found fallback inheriting the global shell          |
| 2  | `frontend/src/routes/about.tsx`                                                      | `/about`                                                      | §4                           | Docs redirect (no in-app About surface; deep links continue to Docs) |
| 3  | `frontend/src/routes/api/[...path].ts`                                               | `/api/...path` (technical proxy, no UX surface)               | §4 (explicit gap, see below) | Server API proxy endpoint, not a rendered surface       |
| 4  | `frontend/src/routes/device.tsx`                                                     | `/device`                                                     | §4                           | Device session surface (global shell)                   |
| 5  | `frontend/src/routes/index.tsx`                                                      | `/`                                                           | §4                           | Public landing (global shell)                           |
| 6  | `frontend/src/routes/login.tsx`                                                      | `/login`                                                      | §4                           | Login surface (global shell)                            |
| 7  | `frontend/src/routes/recover/account.tsx`                                            | `/recover/account`                                            | §4                           | Account recovery surface (global shell)                 |
| 8  | `frontend/src/routes/recover/index.tsx`                                              | `/recover`                                                    | §4                           | Recovery index (global shell)                           |
| 9  | `frontend/src/routes/settings/security.tsx`                                          | `/settings/security`                                          | §4                           | Security settings surface (global shell)                |
| 10 | `frontend/src/routes/setup.tsx`                                                      | `/setup`                                                      | §4                           | First-run setup surface (global shell)                  |
| 11 | `frontend/src/routes/spaces/index.tsx`                                               | `/spaces`                                                     | §4 + §6                      | Space list (global shell; list interaction rules apply) |
| 12 | `frontend/src/routes/spaces/join.tsx`                                                | `/spaces/join`                                                | §4                           | Space join surface (global shell)                       |
| 13 | `frontend/src/routes/spaces/[space_id].tsx`                                          | `/spaces/{space_id}` (layout)                                 | §4                           | Space layout boundary (Space shell)                     |
| 14 | `frontend/src/routes/spaces/[space_id]/index.tsx`                                    | `/spaces/{space_id}`                                          | §4                           | Space home (Space shell)                                |
| 15 | `frontend/src/routes/spaces/[space_id]/dashboard.tsx`                                | `/spaces/{space_id}/dashboard`                                | §4                           | Space dashboard (Space shell)                           |
| 16 | `frontend/src/routes/spaces/[space_id]/assets.tsx`                                   | `/spaces/{space_id}/assets` (layout)                          | §6                           | Asset reference layout boundary (Space shell)           |
| 17 | `frontend/src/routes/spaces/[space_id]/assets/index.tsx`                             | `/spaces/{space_id}/assets`                                   | §6                           | Asset inventory workspace (Space shell)                 |
| 18 | `frontend/src/routes/spaces/[space_id]/assets/[asset_id].tsx`                        | `/spaces/{space_id}/assets/{asset_id}`                        | §6                           | Asset detail workspace (Space shell)                    |
| 19 | `frontend/src/routes/spaces/[space_id]/entries.tsx`                                  | `/spaces/{space_id}/entries` (layout)                         | §5 + §6                      | Entries layout boundary (no standalone list surface)    |
| 20 | `frontend/src/routes/spaces/[space_id]/entries/new.tsx`                              | `/spaces/{space_id}/entries/new`                              | §5                           | Form-first new Entry workspace                          |
| 21 | `frontend/src/routes/spaces/[space_id]/entries/[entry_id].tsx`                       | `/spaces/{space_id}/entries/{entry_id}` (layout)              | §5                           | Entry detail layout boundary                            |
| 22 | `frontend/src/routes/spaces/[space_id]/entries/[entry_id]/index.tsx`                 | `/spaces/{space_id}/entries/{entry_id}`                       | §5                           | Entry detail workspace                                  |
| 23 | `frontend/src/routes/spaces/[space_id]/entries/[entry_id]/info.tsx`                  | `/spaces/{space_id}/entries/{entry_id}/info`                  | §5                           | Entry info workspace                                    |
| 24 | `frontend/src/routes/spaces/[space_id]/entries/[entry_id]/history.tsx`               | `/spaces/{space_id}/entries/{entry_id}/history` (layout)      | §9                           | Entry history layout boundary                           |
| 25 | `frontend/src/routes/spaces/[space_id]/entries/[entry_id]/history/index.tsx`         | `/spaces/{space_id}/entries/{entry_id}/history`               | §9                           | Entry history list (append-only)                        |
| 26 | `frontend/src/routes/spaces/[space_id]/entries/[entry_id]/history/[revision_id].tsx` | `/spaces/{space_id}/entries/{entry_id}/history/{revision_id}` | §9                           | Single revision view (append-only; hosts the restore action)        |
| 27 | `frontend/src/routes/spaces/[space_id]/forms.tsx`                                    | `/spaces/{space_id}/forms` (layout)                           | §6                           | Forms layout boundary                                   |
| 28 | `frontend/src/routes/spaces/[space_id]/forms/index.tsx`                              | `/spaces/{space_id}/forms`                                    | §6                           | List-only Forms list workspace                          |
| 29 | `frontend/src/routes/spaces/[space_id]/forms/[form_ref]/entries.tsx`                 | `/spaces/{space_id}/forms/{form_ref}/entries`                 | §5 + §6                      | Form Entries workspace (that Form's current Entries)    |
| 30 | `frontend/src/routes/spaces/[space_id]/forms/types.tsx`                              | `/spaces/{space_id}/forms/types`                              | §6                           | Form column-types surface                               |
| 31 | `frontend/src/routes/spaces/[space_id]/history.tsx`                                  | `/spaces/{space_id}/history`                                  | §9                           | Space history timeline                                  |
| 32 | `frontend/src/routes/spaces/[space_id]/queries/new.tsx`                              | `/spaces/{space_id}/queries/new`                              | §6                           | Saved-query create workspace                            |
| 33 | `frontend/src/routes/spaces/[space_id]/queries/[query_id]/variables.tsx`             | `/spaces/{space_id}/queries/{query_id}/variables`             | §6                           | Query variables workspace                               |
| 34 | `frontend/src/routes/spaces/[space_id]/search.tsx`                                   | `/spaces/{space_id}/search`                                   | §6                           | Keyword search workspace (draft commits on submit)      |
| 35 | `frontend/src/routes/spaces/[space_id]/settings.tsx`                                 | `/spaces/{space_id}/settings`                                 | §4 + §6                      | Space settings workspace (persistent sections)          |
| 36 | `frontend/src/routes/spaces/[space_id]/sql.tsx`                                      | `/spaces/{space_id}/sql` (layout)                             | §6                           | SQL layout boundary                                     |
| 37 | `frontend/src/routes/spaces/[space_id]/sql/index.tsx`                                | `/spaces/{space_id}/sql`                                      | §6                           | Saved SQL list and create action                        |
| 38 | `frontend/src/routes/spaces/[space_id]/sql/[sql_id].tsx`                             | `/spaces/{space_id}/sql/{sql_id}`                             | §6                           | Saved SQL detail workspace                              |
| 39 | `frontend/src/routes/spaces/[space_id]/sql/[sql_id]/run.tsx`                         | `/spaces/{space_id}/sql/{sql_id}/run`                         | §6                           | Saved SQL stateless run workspace                       |
| 40 | `frontend/src/routes/spaces/[space_id]/test-connection.tsx`                          | `/spaces/{space_id}/test-connection`                          | §4 + §6                      | Storage connection test surface                         |
| 41 | `frontend/src/routes/step-up.tsx`                                                    | `/step-up`                                                    | §4                           | Step-up authentication surface (global shell)           |

Cross-cutting: §§7–8 apply to every rendered surface above (all except row 3,
which renders no UI).

## Companion test files (35)

Each test file maps to the same plan section and surface as its route file.

- `frontend/src/routes/[...404].test.tsx` → row 1 (§4)
- `frontend/src/routes/about.test.tsx` → row 2 (§4)
- `frontend/src/routes/api/[...path].test.ts` → row 3 (§4 gap)
- `frontend/src/routes/device.test.tsx` → row 4 (§4)
- `frontend/src/routes/index.test.tsx` → row 5 (§4)
- `frontend/src/routes/login.test.tsx` → row 6 (§4)
- `frontend/src/routes/public-pages.test.tsx` → rows 2/5 (§4, shared public-page
  coverage)
- `frontend/src/routes/recover/account.test.tsx` → row 7 (§4)
- `frontend/src/routes/recover.test.tsx` → rows 7–8 (§4, shared recovery
  coverage)
- `frontend/src/routes/settings/security.test.tsx` → row 9 (§4)
- `frontend/src/routes/setup.test.tsx` → row 10 (§4)
- `frontend/src/routes/spaces/index.test.tsx` → row 11 (§4 + §6)
- `frontend/src/routes/spaces/join.test.tsx` → row 12 (§4)
- `frontend/src/routes/spaces/[space_id].test.tsx` → row 13 (§4)
- `frontend/src/routes/spaces/[space_id]/assets.test.tsx` → row 17 (§6)
- `frontend/src/routes/spaces/[space_id]/assets/[asset_id].test.tsx` → row 18
  (§6)
- `frontend/src/routes/spaces/[space_id]/dashboard.test.tsx` → row 15 (§4)
- `frontend/src/routes/spaces/[space_id]/entries/[entry_id]/history/index.test.tsx`
  → row 25 (§9)
- `frontend/src/routes/spaces/[space_id]/entries/[entry_id]/history/[revision_id].test.tsx`
  → row 26 (§9)
- `frontend/src/routes/spaces/[space_id]/entries/[entry_id]/index.test.tsx` →
  row 22 (§5)
- `frontend/src/routes/spaces/[space_id]/entries/[entry_id]/info.test.tsx` → row
  23 (§5)
- `frontend/src/routes/spaces/[space_id]/entries/new.test.tsx` → row 20 (§5)
- `frontend/src/routes/spaces/[space_id]/forms/index.test.tsx` → row 28 (§6)
- `frontend/src/routes/spaces/[space_id]/forms/[form_ref]/entries.test.tsx` →
  row 29 (§5 + §6)
- `frontend/src/routes/spaces/[space_id]/forms/types.test.tsx` → row 30 (§6)
- `frontend/src/routes/spaces/[space_id]/history.test.tsx` → row 31 (§9)
- `frontend/src/routes/spaces/[space_id]/queries/new.test.tsx` → row 32 (§6)
- `frontend/src/routes/spaces/[space_id]/queries/[query_id]/variables.test.tsx`
  → row 33 (§6)
- `frontend/src/routes/spaces/[space_id]/search.test.tsx` → row 34 (§6)
- `frontend/src/routes/spaces/[space_id]/settings.route.test.tsx` → row 35 (§4 +
  §6)
- `frontend/src/routes/spaces/[space_id]/settings.sections.test.ts` → row 35
  (§4 + §6)
- `frontend/src/routes/spaces/[space_id]/sql/index.test.tsx` → row 37 (§6)
- `frontend/src/routes/spaces/[space_id]/sql/[sql_id].test.tsx` → row 38 (§6)
- `frontend/src/routes/spaces/[space_id]/sql/[sql_id]/run.test.tsx` → row 39
  (§6)
- `frontend/src/routes/step-up.test.tsx` → row 41 (§4)

## Explicit gaps

1. `frontend/src/routes/api/[...path].ts` is a technical API proxy endpoint, not
   a rendered UX surface. It is inventoried above for 100% file coverage, but no
   §§4–9 UX acceptance applies to it. No UX requirement or feature targets it.
2. No route file found under `frontend/src/routes` falls outside §§4–9 as a
   rendered surface. If a future route is added without a plan-section mapping,
   this inventory must be updated and the gap recorded here before UX acceptance
   can claim full coverage.

## Baseline verification

This inventory is docs-only: no UI behavior change and no new frontend test.
Inventory parity is verified by re-running
`find frontend/src/routes -type f | sort` and confirming every file is listed
above, plus `./scripts/mitase check .` validating the `ux`
requirements/features and the `route-inventory` policy references.
