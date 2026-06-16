# Runtime Adapters

The release runtime treats `ugoite-core` as the application service boundary.
Adapters translate their local protocol into core calls and avoid owning
business rules.

```text
frontend UI
  -> frontend client boundary
    -> Rust ugoite-server HTTP adapter
      -> ugoite-core service boundary
        -> storage adapters

ugoite-cli
  -> ugoite-core service boundary
    -> storage adapters
```

`ugoite-server` is the current browser runtime, not the long-term source of
truth. It should stay focused on request decoding, auth/session extraction, DTO
conversion, core invocation, and HTTP error mapping.

`ugoite-storage` stays focused on current persistence adapters. Browser-local
storage, peer sync, signed operation logs, and relay behavior are future
architecture topics, not release implementation scope.
