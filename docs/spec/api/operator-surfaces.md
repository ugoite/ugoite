---
title: 'Operator surfaces'
---

| Surface | Primary user | Current use |
|---|---|---|
| CLI core mode | local human/script | direct portable workspace operations and local index maintenance |
| CLI backend/API mode | remote human/script | authenticated REST operations through the portable client |
| REST | browser and application clients | complete current product HTTP contract |
| MCP resource | AI client | one authenticated, read-only, untrusted-content-framed Entry list |

Choose the narrowest surface that matches the operator. All surfaces delegate to the same core behavior rather than implementing separate business rules. Writes and broad orchestration currently belong to CLI/REST, not MCP.
