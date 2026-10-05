# CodeQL `rust/hard-coded-cryptographic-value` triage

Reviewed from the open code-scanning alerts on the default branch
(issue #3305). The fixed HMAC test keys in
`crates/ugoite-iceberg/src/audit.rs` were already replaced with per-test
UUID-generated values; no open alert remains there.

## Suppressed false positives

Each location below carries an inline
`// codeql[rust/hard-coded-cryptographic-value]` comment stating why the
flagged constant is not a secret. Suppression is per line only: no query
is excluded in `codeql-config.yml`, so genuine hard-coded cryptographic
credentials remain detectable everywhere, including at these files.

### Unix owner-only permission modes

These constants restrict Space data to its owner; they are filesystem
permission bits, not keys or credentials.

| Alert | Location | Constant |
| ----- | -------- | -------- |
| 138 | `crates/ugoite-iceberg/src/space.rs:256` | `0o700` |
| 150 | `crates/ugoite-iceberg/src/space.rs:257` | `0o700` |
| 140 | `crates/ugoite-iceberg/src/space.rs:259` | `0o700` |
| 141 | `crates/ugoite-iceberg/src/space.rs:262` | `0o600` |
| 142 | `crates/ugoite-iceberg/src/space.rs:265` | `0o700` |
| 143 | `crates/ugoite-iceberg/src/space.rs:267` | `0o600` |
| 144 | `crates/ugoite-iceberg/src/space.rs:1110` | `0o700` |
| 145 | `crates/ugoite-iceberg/src/space.rs:1148` | `0o600` |
| 171 | `crates/ugoite-storage/src/lib.rs:4880` | `0o700` |

### Fixed Space storage path segments

These strings keep same-filesystem atomic-write temporary files on the
Space filesystem; they are path constants, not keys or credentials.

| Alert | Location | Constant |
| ----- | -------- | -------- |
| 167 | `crates/ugoite-storage/src/lib.rs:4849` | `"spaces"` |
| 168 | `crates/ugoite-storage/src/lib.rs:4851` | `".ugoite-atomic-writes"` |
| 169, 170 | `crates/ugoite-storage/src/lib.rs:4859` | `"spaces"`, `".ugoite-atomic-writes"` |

## Still detectable

- Space HMAC key material is generated at runtime
  (`generate_hmac_material` in `crates/ugoite-iceberg/src/space.rs` uses
  `SysRng` bytes with UUID key IDs); a hard-coded key there would still
  alert.
- Alerts outside this triage (for example in `entry_query.rs`,
  `sql_query.rs`, or `node_identity.rs`) are intentionally unsuppressed
  here and need their own review before any suppression.

## Dismissal record

Alerts 138, 140–145, 150, 167–171 were dismissed via the code-scanning
API with reason `false positive`, each pointing at this triage. No query
was excluded and no detection configuration changed: the inline
`// codeql[rust/hard-coded-cryptographic-value]` comments mark the
reviewed lines in tree, while GitHub records the per-alert dismissal
state.
