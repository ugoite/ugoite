## Summary

-

## Related Issue (required)

close: # (or: closes #)

## Knowledge Compatibility Review

Review the [v0.1 Knowledge compatibility floor](../docs/architecture/release/v0.1-knowledge-compatibility.md)
before selecting exactly one classification.

- [ ] No effect on the v0.1 Knowledge semantic contract.
- [ ] Preserving implementation change; the canonical fixture and focused tests remain passing.
- [ ] Breaking semantic change; an explicit versioned contract or migration/re-encoding decision is documented.

Evidence: <required for preserving changes>
Decision: <required for breaking changes>

## Testing

- [ ] `mise run test`
