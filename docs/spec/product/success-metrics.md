# Success metrics

Metrics must distinguish the current server-backed product from the browser-local North Star and must not treat planned features as shipped.

## Newcomer path

- **Time to healthy runtime:** from starting the source or release Compose flow to a successful `/health`.
- **Time to authenticated session:** includes selecting the supported auth mode and completing development mock OAuth or supplying an accepted credential. Passkey/TOTP is excluded until implemented.
- **Time to first writable Space:** includes creating or joining a Space and obtaining an active role.
- **Time to first Entry:** measured separately for browser/server-backed and CLI core/local workflows.
- **Time to first structured field:** user creates or edits a Form, then saves a conforming Entry.

## Data safety and portability

- restore and history tests demonstrate append-only revision recovery;
- checksums and HMAC verification detect changed content;
- backup/restore drills preserve a complete operator-owned storage root;
- no documentation path requires an unpublished artifact or hidden hosted service.

## Current performance metrics

- Entry list, structured query, SQL session row/count, and keyword-scan latency at documented dataset sizes;
- memory use and startup time of the single runtime image;
- browser route responsiveness while server requests are pending.

Persistent inverted-index rebuild cost and watch-loop lag are **future** metrics because those operations are not implemented. Keyword search currently performs a substring scan and has no relevance-ranking metric.

## Developer experience

- every feature-registry file/function reference resolves;
- every OpenAPI operation is intentionally represented or explicitly excluded from the feature registry;
- requirement trace status distinguishes traced from untraced items;
- Markdown links, documented `mise` tasks, YAML parsing, and current-stack terminology pass repository checks.
