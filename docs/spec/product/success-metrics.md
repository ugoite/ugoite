---
title: "Success metrics"
---

Authentication metrics below include supported v0.1 browser Passkey/WebAuthn
bootstrap and passwordless login, invitation-gated OIDC account creation and
linking, Account Self-Recovery, and remote device login. Administrator recovery
remains a planned indicator only.

Metrics must distinguish the current server-backed product from the
browser-local North Star and must not treat planned features as shipped.

## Newcomer path

- **Time to healthy runtime:** from starting the source or release Compose flow
  to a successful `/health`.
- **Time to authenticated session:** measures first-run Passkey setup and
  subsequent discoverable Passkey login; CLI pairing is measured separately from
  browser login.
- **Time to first writable Space:** includes creating or joining a Space and
  obtaining an active role.
- **Time to first Entry:** measured separately for browser/server-backed and CLI
  core/local workflows.
- **Time to first structured field:** user creates or edits a Form, then saves a
  conforming Entry.

## Data safety and portability

- restore and history tests demonstrate append-only revision recovery;
- checksums and HMAC verification detect changed content;
- backup/restore drills preserve every configured recovery input: complete Space
  prefixes, the Node control-store prefix, and the node secret;
- no documentation path requires an unpublished artifact or hidden hosted
  service.

## Current performance metrics

- Entry list, structured query, SQL session row/count, and keyword-scan latency
  at documented dataset sizes;
- memory use and startup time of the single runtime image;
- browser route responsiveness while server requests are pending.

AssetText rebuild duration, parser failure/stale rates, derived recovery time,
and authorized AssetText search latency are current metrics. A persistent
inverted-index rebuild and watch-loop lag remain **future** metrics because
AssetText is still a scan-oriented searchable-text projection with no
relevance ranking.

## Developer experience

- every feature-registry file/function reference resolves;
- every OpenAPI operation is intentionally represented or explicitly excluded
  from the feature registry;
- requirement trace status distinguishes traced from untraced items;
- Markdown links, documented `mise` tasks, YAML parsing, and current-stack
  terminology pass repository checks.
