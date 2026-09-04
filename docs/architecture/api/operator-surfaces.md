---
title: "Operator surfaces"
---

| Surface              | Primary user                    | Current use                                                                |
| -------------------- | ------------------------------- | -------------------------------------------------------------------------- |
| CLI core mode        | local human/script              | direct portable workspace operations and local index maintenance           |
| CLI backend/API mode | remote human/script             | authenticated REST operations through the portable client                  |
| REST                 | browser and application clients | complete current product HTTP contract                                     |
| MCP                  | AI client                       | small authenticated search/save/delete facade with lazy semantic resources |

Choose the narrowest surface that matches the operator. All surfaces delegate to
the same core behavior rather than implementing separate business rules. MCP
intentionally omits broad orchestration, storage details, and revision CRUD.
