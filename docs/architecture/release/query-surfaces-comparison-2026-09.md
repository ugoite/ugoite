---
title: "Query surface comparison: 2026-09"
---

This report compares the pre-QRY-02 frontend with the current virtualized
frontend on the same release server binary and seeded data. It also records
current-build query cancellation and Space-switch behavior. The latency samples
do not establish a general performance improvement.

## Source and environment

| Run        | Frontend source                            | Measurement checkout                       | Backend source                             | Run date (UTC)   | Server startup |
| ---------- | ------------------------------------------ | ------------------------------------------ | ------------------------------------------ | ---------------- | -------------: |
| Pre-QRY-02 | `69628fe6b47599d2b7e1e726e697ff76bb0d9939` | `69628fe6b47599d2b7e1e726e697ff76bb0d9939` | `ef6e7a2df4c0066172cafed955f9bf0d8a2114c8` | 2026-09-26 09:53 |           76 s |
| Current    | `fa1959f97eb1b950c3bcdcbddd2be789507526f6` | `4e66c9f74058b5d15d2603c543c6a6aed5d7dc95` | `ef6e7a2df4c0066172cafed955f9bf0d8a2114c8` | 2026-09-26 10:12 |           79 s |

Both frontend builds used the same release backend binary, built from the same
checkout and run through the same direct-process E2E runner. They used a local
filesystem, loopback browser/server connection, macOS 26.6.1 on Apple M4 arm64,
Chromium `148.0.7778.96`, a 1280×720 viewport, and the `MaintenanceTicket` Form.
Space A had 6,000 Entries (seed `3134001`); Space B had 4,000 Entries (seed
`3134002`). EntryQuery page size was 50 and SQL page size was 100. The browser
reported `performance.memory.usedJSHeapSize = 10,000,000` bytes in every trial;
this is the browser API value, not process RSS. Each frontend ran five trials
per Space and surface. p95 is nearest-rank (the maximum of five values). The
current measurement checkout adds only the E2E instrumentation to `fa1959f`; its
frontend files are unchanged from that main commit. The pre-QRY-02 run uses the
same instrumentation in baseline mode.

Raw per-trial records, including Space IDs, SQL relation names, row counts,
browser heap readings, and sanitized request events, are preserved in
[`query-surfaces-baseline-69628fe6.json`](measurements/query-surfaces-baseline-69628fe6.json)
and
[`query-surfaces-current-4e66c9f7.json`](measurements/query-surfaces-current-4e66c9f7.json).
Continuation values are redacted from the raw request bodies.

## First visible row

| Frontend   | Surface    | Space | Trials |    p50 |      p95 | DOM data rows |     Heap API |
| ---------- | ---------- | ----- | -----: | -----: | -------: | ------------: | -----------: |
| Pre-QRY-02 | EntryQuery | A     |      5 | 839 ms | 1,372 ms |            50 | 10,000,000 B |
| Pre-QRY-02 | EntryQuery | B     |      5 | 836 ms |   844 ms |            50 | 10,000,000 B |
| Current    | EntryQuery | A     |      5 | 845 ms |   901 ms |            50 | 10,000,000 B |
| Current    | EntryQuery | B     |      5 | 843 ms | 1,376 ms |            50 | 10,000,000 B |
| Pre-QRY-02 | Saved SQL  | A     |      5 | 839 ms |   845 ms |           100 | 10,000,000 B |
| Pre-QRY-02 | Saved SQL  | B     |      5 | 842 ms |   864 ms |           100 | 10,000,000 B |
| Current    | Saved SQL  | A     |      5 | 844 ms | 1,376 ms |           100 | 10,000,000 B |
| Current    | Saved SQL  | B     |      5 | 833 ms |   840 ms |           100 | 10,000,000 B |

The p50 values are close. Two current Space/surface p95 samples have outliers
while the other two are lower than baseline, so these samples do not show a
consistent latency improvement. Both frontends loaded page one and page two in
both Spaces. Each SQL page-two response contained 100 rows and a continuation
value. Neither frontend sent an automatic SQL count request before an explicit
Count action.

## Current-build lifecycle evidence

The EntryQuery requests were delayed by 400 ms during lifecycle observations;
performance trials were not delayed. Two superseded search requests were aborted
and settled. Switching Space A to Space B aborted and settled the remaining
in-flight Space A request. All three had zero pending fetches after the
observation interval. While the Space B request was pending, the table had zero
rows. After it completed, 50 rows were visible; no Space A Entry IDs or
unexpected IDs appeared in Space B.

On the SQL surface, changing Space while a page request was in flight aborted
and settled one request. Starting an explicit Count and then changing Space also
aborted and settled one count request. Both scenarios had zero residual pending
fetches after the observation interval. SQL page one was requested once in each
Space, Next requested page two once, and the initial count request total was
zero in both Spaces.

These aborts describe browser fetch behavior. They do not establish that an HTTP
disconnect stops SQL engine work after server execution begins.

## Reproduction

Build the release backend from the current checkout, seed the same two Spaces,
then run the measurement helper with the fixed backend binary:

```sh
cargo build -p ugoite-server --release --locked
UGOITE_QUERY_MEASURE_ROOT=/tmp/query-surfaces-current-root \
UGOITE_QUERY_MEASURE_OUTPUT=/tmp/query-surfaces-current.json \
UGOITE_SOURCE_SHA=4e66c9f74058b5d15d2603c543c6a6aed5d7dc95 \
UGOITE_BACKEND_SOURCE_SHA=ef6e7a2df4c0066172cafed955f9bf0d8a2114c8 \
UGOITE_E2E_BACKEND_BINARY="$PWD/target/rust/release/ugoite-server" \
UGOITE_E2E_STARTUP_TIMEOUT_SECONDS=300 \
UGOITE_SKIP_PLAYWRIGHT_DEPS=1 \
bash scripts/measure-query-surfaces.sh
```

For the baseline frontend, check out `69628fe6b47599d2b7e1e726e697ff76bb0d9939`
in a separate worktree and copy the measurement test, E2E runner, and seed
helper from the current checkout into that worktree. Seed Spaces A and B using
the values above. Run `e2e/scripts/run-e2e.sh query-measurement` with
`UGOITE_QUERY_MEASURE_ENABLED=true`, `UGOITE_QUERY_MEASURE_BASELINE=true`,
`UGOITE_SOURCE_SHA=69628fe6b47599d2b7e1e726e697ff76bb0d9939`, the same
`UGOITE_BACKEND_SOURCE_SHA` and `UGOITE_E2E_BACKEND_BINARY`, and
`UGOITE_QUERY_MEASURE_OUTPUT` plus `E2E_STORAGE_ROOT` set to fresh paths. The
baseline mode uses generic table-row selectors and omits current-only
cross-Space assertions; it does not change the baseline frontend source.
