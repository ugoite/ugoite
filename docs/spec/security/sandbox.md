# Wasm Sandbox Security

## Overview

The `run_script` MCP tool executes arbitrary JavaScript code, requiring strict isolation.

## Runtime Environment

| Component | Technology |
|-----------|------------|
| Wasm Runtime | wasmtime |
| JS Engine | QuickJS (compiled to Wasm) |
| Host Interface | `host.call()` function |

## Isolation Guarantees

### Network Access
- **BLOCKED**: No direct network access from sandbox
- All external communication goes through `host.call()` which proxies to internal API

### Filesystem Access
- **BLOCKED**: No direct filesystem access
- Data access only through API endpoints

### Process Isolation
- Wasm runs within backend process
- Strong memory isolation via Wasm memory model
- No access to host process memory

## Resource Limits

### Fuel (CPU Cycles)
```python
# Default fuel limit
SANDBOX_FUEL_LIMIT = 1_000_000

# Prevents infinite loops
# Each Wasm instruction consumes fuel
```

### Memory
```python
# Default memory limit
SANDBOX_MEMORY_LIMIT_MB = 128

# Wasm linear memory is bounded
```

### Execution Time
```python
# Timeout for script execution
SANDBOX_TIMEOUT_SECONDS = 30
```

## Host Interface

The `host` object provides controlled API access:

```javascript
// Only available method
host.call(method, path, body)

// Returns: JSON response from API
// Throws: Error if API call fails
```

### Allowed Operations

All operations go through the REST API:
- Read/write notes
- Query/search
- Manage classes
- Handle attachments

### Blocked Operations

Not available in sandbox:
- Direct filesystem access
- Network requests
- Process spawning
- Environment variables
- System information

## Error Handling

| Error Type | Cause | Response |
|------------|-------|----------|
| `FuelExhausted` | Script exceeded CPU limit | Script terminated |
| `MemoryExceeded` | Script exceeded memory limit | Script terminated |
| `Timeout` | Script exceeded time limit | Script terminated |
| `ExecutionError` | JavaScript runtime error | Error returned to AI |
| `HostCallError` | API call failed | Error returned to script |

## Audit Trail

All `run_script` executions are logged:

```json
{
  "timestamp": "2025-11-29T10:00:00Z",
  "workspace_id": "ws-main",
  "code_hash": "sha256:abc123...",
  "fuel_used": 50000,
  "memory_used_mb": 12,
  "result_status": "success",
  "duration_ms": 150
}
```

## Security Testing

Tests verify sandbox isolation:

| Test | Verifies |
|------|----------|
| `test_simple_execution` | Basic JS works |
| `test_host_call` | API access works |
| `test_execution_error` | Errors handled |
| `test_infinite_loop_fuel` | Fuel limits work |
| `test_missing_wasm_raises` | Missing artifact handled |

## Future Improvements

1. **Capabilities Model**: Fine-grained permissions per script
2. **Rate Limiting**: Per-workspace execution quotas
3. **Code Signing**: Verify script provenance
4. **Snapshot/Restore**: Checkpoint Wasm state
