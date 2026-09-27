---
title: "Query surface lifecycle observations: 2026-09"
---

This record retains browser request-lifecycle observations from the QRY-02
work. It does not compare table-rendering performance and does not establish
that an HTTP disconnect stops SQL engine work after server execution begins.

## Source and environment

The extended lifecycle run used source
`6c6160d576075dde099e781c52539d5a5bf8a418`, a local filesystem Space root, a
loopback browser/server connection, a 1280×720 viewport, and Chromium
`148.0.7778.96` in headless mode. It used fixed 6,000-entry and 4,000-entry
Space seeds and 400 ms route delays for selected lifecycle requests.

## Browser lifecycle evidence

The run observed one superseded keyword-search fetch and one Space switch; each
old EntryQuery signal aborted in flight and settled, with no residual pending
fetch. Switching from Space A to Space B left no stale A IDs in the visible B
rows.

On the SQL surface, changing Space during a page request, changing SQL page
identity, changing a parameter value, changing a parameter type during an
explicit Count, and disposing the route each settled the superseded request
after abort. Previous pagination issued the expected Next and Previous page
requests. Both Spaces issued zero automatic Count requests. A deliberately
failed explicit Count was followed by a successful Count retry; the result-page
query count remained zero before and after that retry. Every lifecycle
observation ended with zero pending fetches.

The sanitized request events are preserved in
[`query-surfaces-lifecycle-6c6160d5.json`](measurements/query-surfaces-lifecycle-6c6160d5.json).
These are browser fetch observations; they do not establish server execution
cancellation. Server-side query lifetime requires separate evidence.
