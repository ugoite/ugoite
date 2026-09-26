---
title: "SQL export acceptance: 2026-09"
---

This records the local 10,000-row SQL export measurement and focused CLI
acceptance tests for [#3140](https://github.com/ugoite/ugoite/issues/3140).
The local measurement is evidence for filesystem-backed Core only. Live Remote
authorization revocation is recorded separately in
[`v0.2.1-remote-export-auth.md`](v0.2.1-remote-export-auth.md).

## Reproduce

From the repository root:

```sh
bash scripts/measure-sql-export.sh
```

The script refuses a non-empty supplied data root
(`UGOITE_SQL_EXPORT_MEASURE_ROOT`) and output directory
(`UGOITE_SQL_EXPORT_MEASURE_OUTPUT`), seeds one `renewable-ops` Space with
10,000 Entries and seed `3140001`, and builds a read-only `UNION ALL` query over
every relation returned by `ugoite form list`. It exports the same bounded
result with page sizes 100 and 1,000, requires 10,000 completed rows and the
expected page counts, and checks canonical row equivalence. It retains
`measurement.json`, both NDJSON files, the SQL, summaries, and raw platform time
logs under `UGOITE_SQL_EXPORT_MEASURE_OUTPUT` (default
`target/sql-export-measurement`).

Measured source SHA: `5e8a1a17de77976f39bd37095fe6d3ab1602db43`. Environment:
macOS 26.6.1, arm64, local filesystem-backed Core. The exact Space UID and
machine-readable run summary are in
[`sql-export-measurement-2026-09.json`](measurements/sql-export-measurement-2026-09.json);
the relevant unedited `/usr/bin/time -l` output is in
[`sql-export-time-2026-09.txt`](measurements/sql-export-time-2026-09.txt).

## Results

| Page size |   Rows | Pages fetched | Real time |   CLI maximum RSS |
| --------: | -----: | ------------: | --------: | ----------------: |
|       100 | 10,000 |           100 |   46.73 s | 238,125,056 bytes |
|     1,000 | 10,000 |            10 |    4.83 s | 169,361,408 bytes |

Both exports have the same canonical SHA-256 after sorting normalized rows:
`88c78853dde1989818e664ea52bc2c14407fef2060d084de2c98a7ec7bf44c72`. These are
single runs, not a performance comparison or a claim about improvement. RSS is
the CLI process maximum and excludes the server, seed process, and other system
memory. The filesystem-backed CLI starts an in-process query engine; this does
not represent Remote latency or memory.

## Focused failure coverage

Command run:

```sh
cargo test --locked -p ugoite-cli --test test_sql_stateless_cli cli_sql_export_ -- --nocapture
```

The original four `cli_sql_export_` tests cover complete atomic file
publication, empty output, refusal to overwrite an existing path, and
`--max-rows` failure with nonzero status, partial stdout accounting, existing
destination preservation, and temporary file cleanup.

The #3140 follow-up acceptance was run with:

```sh
cargo test --locked -p ugoite-cli --lib --test test_cli_endpoint_routing --test test_journey_remote
```

It covers repeated continuation tokens, empty pages with `has_more`,
inconsistent `has_more`/`next` metadata, changed columns, Ctrl-C, a broken
stdout pipe, injected partial writer failure, output finalization failure, and
real Space membership revocation during a Remote export. The live authorization
test runs the CLI against the in-process Ugoite server over loopback, removes a
Viewer membership after page one, and verifies page two returns the existing
`FORBIDDEN`/`forbidden` classification, reports exactly one exported row,
does not issue a third request, does not expose a continuation, and does not
publish or leave a temporary output file. The server-side test helper mutates
the real Space authorization state; the existing HTTP-denial mock remains a
separate transport/error projection test.

The live Remote test recorded in
[`v0.2.1-remote-export-auth.md`](v0.2.1-remote-export-auth.md) verifies a real
membership revocation between pages. Credential expiry was not exercised and
remains tracked separately in
[#3186](https://github.com/ugoite/ugoite/issues/3186). The local 10,000-row
memory/timing measurement above is from the earlier recorded source SHA; it
remains a single local-Core run per page size, not a performance comparison or
Remote measure.
