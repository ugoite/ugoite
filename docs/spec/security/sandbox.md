# Sandbox status

No general-purpose code-execution sandbox is shipped. The earlier Wasm `run_script` surface was removed and must not be advertised as an available API or MCP tool.

The current `ugoite-wasm` crate exposes portable domain/API protocol logic for browser adapters; it is not an untrusted-code execution environment. Any future computational-block feature requires a separate threat model, capability policy, resource limits, and explicit user consent.
