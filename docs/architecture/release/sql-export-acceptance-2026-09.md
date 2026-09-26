---
title: "SQL export acceptance: 2026-09"
---

This records the local 10,000-row SQL export measurement and the focused CLI
acceptance tests run for [#3140](https://github.com/ugoite/ugoite/issues/3140).
It is evidence for local filesystem-backed Core only. The Remote authorization
revocation acceptance remains open.

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

Result: 4 passed. The existing tests cover complete atomic file publication,
empty output, refusal to overwrite an existing path, and `--max-rows` failure
with nonzero status, partial stdout accounting, existing destination
preservation, and temporary file cleanup.

This run did not exercise a repeated token, empty page with `has_more`, changed
columns, Ctrl-C, disk-full, broken pipe, ACL revocation after page one, or
credential expiry. No Remote credentials were configured in the execution
environment, so there is no live authorization-change evidence. Mock denial is
not substituted for that acceptance. Continue #3140 for those missing cases;
this measurement does not satisfy its full close criteria.
