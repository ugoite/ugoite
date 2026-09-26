---
title: "Query surface measurement: 2026-09"
---

This is one server-backed local measurement of EntryQuery and Saved SQL on two
fixed-seed Spaces. It records current-build behavior; it does not establish an
improvement against an earlier build. The pre-QRY-02 comparison is tracked in
[#3171](https://github.com/ugoite/ugoite/issues/3171).

## Reproduce

From the repository root, run:

```sh
UGOITE_QUERY_MEASURE_OUTPUT=/tmp/query-surfaces-measurement-daeb6914.json \
UGOITE_SKIP_PLAYWRIGHT_DEPS=1 \
bash scripts/measure-query-surfaces.sh
```

The script refuses a non-empty measurement root. It seeds Space A with 6,000
Entries (`renewable-ops`, seed `3134001`) and Space B with 4,000 Entries (seed
`3134002`), starts the server against that filesystem root, and runs the
`query-measurement` Playwright selector. Each Space contains Sites (2%), Arrays
(8%), Inspections (20%), Maintenance tickets (25%), and Energy reports (45%).
The query surface measures the `MaintenanceTicket` Form. The generated Space
UIDs, Form IDs and SQL relation names are saved in the raw report.

The exact checkout measured was `daeb69143850326a4f2e7e9a107410c5bba2347b`.
The raw machine-readable output is
[`query-surfaces-2026-09.json`](measurements/query-surfaces-2026-09.json).

## Results

The browser ran five first-visible-row trials per Space and surface (ten per
surface total). Results were:

| Surface | Trials | p50 | p95 | Rows rendered per trial |
| --- | ---: | ---: | ---: | ---: |
| EntryQuery | 10 | 1,853 ms | 2,404 ms | 50 |
| Saved SQL first page | 10 | 1,351 ms | 1,356 ms | 100 |

Both SQL Spaces loaded page one without an automatic count request. Selecting
Next loaded page two with 100 rows and a continuation token in each Space. The
browser recorded three aborted EntryQuery fetches during rapid filter changes
and zero residual pending fetches after the observation interval. After switching
Spaces, the Forms index had no result rows before the test reopened the same
EntryQuery in Space B; Space B then issued its own EntryQuery request and rendered
rows. This checks that the target query surface loads in the destination Space;
it does not compare row contents with a pre-switch snapshot. The 400 ms
interception delay applied only to EntryQuery lifecycle observations, not to
performance trials.

The local backend startup took 128 seconds for both seeded Spaces. The
measurement does not isolate audit recovery from other server initialization.
`performance.memory.usedJSHeapSize` reported 10,000,000 bytes in these Chromium
trials; this is a browser API value, not process RSS. Results include browser
navigation, rendering, local HTTP, server, and filesystem work. This run does
not compare a pre-QRY-02 build, exercise screen readers, or establish that
aborting browser fetches cancels work already running in the SQL engine.
