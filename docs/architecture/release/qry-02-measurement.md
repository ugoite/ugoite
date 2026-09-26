---
title: "QRY-02 visible-row measurement"
---

This page records the synthetic browser measurement for QRY-02 and the evidence still tracked separately.

## Synthetic 1,000-row browser fixture

- Implementation commit: `10a302dc9ac12731989d19bb1d524ad69857c464`
- Test: `e2e/qry02-visible-rows.test.ts` (Chromium / Playwright)
- Runs: 5 sequential, fresh Compose/browser test runs; all 5 passed
- Host: macOS 26.6.1, Mac16,10 (Apple M4), 32 GiB RAM
- Docker: Server 29.7.2; Compose VM reports 8 CPUs and 12,526,637,056 bytes RAM
- Browser: Chromium 148.0.7778.96
- Browser viewport: 1280 × 720 px; virtual scroll viewport: 480 px maximum
- Fixture: 1,000 synthetic EntryQuery response rows, supplied by a Playwright route
- Query limit: the controller still requests 50 rows; the intercepted fixture returns 1,000 to exercise the rendering path
- Network/server condition: the E2E server runs in local Docker Compose, while the EntryQuery request is fulfilled by Playwright. These numbers measure browser rendering and interaction; they do not measure storage, server query latency, or production network latency.
- Heap: Chromium `performance.memory.usedJSHeapSize` reported 10,000,000 bytes on each run. This is the browser API's reported value, not a full process-memory measurement.
- DOM: 22 rendered data rows, 24 total table rows (header + data + one spacer) on each run.
- Focus: ArrowDown from the last fully visible Entry action reaches and focuses the next virtual row in all runs.

| Run | First visible row (ms) | Data rows | Total `<tr>` | Reported JS heap (bytes) |
| --- | ---: | ---: | ---: | ---: |
| 1 | 78.4 | 22 | 24 | 10,000,000 |
| 2 | 84.4 | 22 | 24 | 10,000,000 |
| 3 | 91.4 | 22 | 24 | 10,000,000 |
| 4 | 81.9 | 22 | 24 | 10,000,000 |
| 5 | 86.0 | 22 | 24 | 10,000,000 |
| **p50** | **84.4** |  |  |  |
| **p95** | **91.4** |  |  |  |

Percentiles use the nearest-rank method (`ceil(p × N)`). The raw measurement records are preserved here:

```text
QRY02_VISIBLE_ROWS_MEASUREMENT {"elapsedToFirstRowMs":78.40000009536743,"measuredRows":24,"measuredDataRows":22,"usedHeapBytes":10000000,"viewport":{"width":1280,"height":720},"suppliedRows":1000}
QRY02_VISIBLE_ROWS_MEASUREMENT {"elapsedToFirstRowMs":84.40000009536743,"measuredRows":24,"measuredDataRows":22,"usedHeapBytes":10000000,"viewport":{"width":1280,"height":720},"suppliedRows":1000}
QRY02_VISIBLE_ROWS_MEASUREMENT {"elapsedToFirstRowMs":91.40000009536743,"measuredRows":24,"measuredDataRows":22,"usedHeapBytes":10000000,"viewport":{"width":1280,"height":720},"suppliedRows":1000}
QRY02_VISIBLE_ROWS_MEASUREMENT {"elapsedToFirstRowMs":81.90000009536743,"measuredRows":24,"measuredDataRows":22,"usedHeapBytes":10000000,"viewport":{"width":1280,"height":720},"suppliedRows":1000}
QRY02_VISIBLE_ROWS_MEASUREMENT {"elapsedToFirstRowMs":86,"measuredRows":24,"measuredDataRows":22,"usedHeapBytes":10000000,"viewport":{"width":1280,"height":720},"suppliedRows":1000}
```

## Additional acceptance evidence

This synthetic fixture remains distinct from the real-data comparison. The
fixed-seed 6,000/4,000-entry, two-Space comparison and lifecycle observations
are recorded in
[`query-surfaces-comparison-2026-09.md`](query-surfaces-comparison-2026-09.md),
with exact source SHAs and raw trial records. Those samples do not show a
consistent latency improvement. The completed measurement work is tracked by
closed issues [#3134](https://github.com/ugoite/ugoite/issues/3134),
[#3147](https://github.com/ugoite/ugoite/issues/3147), and
[#3171](https://github.com/ugoite/ugoite/issues/3171).

Manual VoiceOver or NVDA verification remains open in
[#3148](https://github.com/ugoite/ugoite/issues/3148); this synthetic browser
test does not satisfy that gate. Browser `AbortSignal` behavior must not be
described as SQL-engine cancellation. See the
[v0.2.1 read-surface acceptance index](v0.2.1-read-acceptance.md) for the
current gate status.
