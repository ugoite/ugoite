# North Star

Ugoite's long-term direction is to make the Space independent from any single
server.

Today, the browser experience runs through the Rust `ugoite-server` runtime, and
the CLI can operate through the direct core path. Over time, Ugoite is designed
to move more of the Space engine into portable Rust/WASM and browser-local
storage.

Servers remain useful, but optional. They can relay, mirror, index,
authenticate, or expose MCP resources. They do not have to own the truth.

This document is intentionally directional. Browser-local storage, peer
replication, signed operation logs, and distributed trust are not formal release
requirements. They are constraints on how today's adapter boundaries should be
kept small and replaceable.
