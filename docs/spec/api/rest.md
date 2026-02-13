# REST API Specification

## Overview

The REST API is the primary interface for the frontend and external integrations.

**Base URL**: `http://localhost:8000` (development)

## Authentication

By default, the API binds to localhost only and requires no authentication.
When `UGOITE_ALLOW_REMOTE=true`, authentication is required (future milestone).

## Endpoints

### Spaces

#### List Spaces
```http
GET /spaces
```

**Response**: `200 OK`
```json
[
  {
    "id": "ws-main",
    "name": "Personal Knowledge",
    "created_at": "2025-08-12T12:00:00Z"
  }
]
```

#### Create Space
```http
POST /spaces
Content-Type: application/json

{
  "id": "space-new",
  "name": "New Space"
}
```

**Response**: `201 Created`

#### Get Space
```http
GET /spaces/{id}
```

**Response**: `200 OK`

#### Update Space
```http
PATCH /spaces/{id}
Content-Type: application/json

{
  "name": "Updated Name",
  "storage_config": { "uri": "s3://bucket/path" },
  "settings": { "default_form": "Meeting" }
}
```

**Response**: `200 OK`

#### Test Connection
```http
POST /spaces/{id}/test-connection
Content-Type: application/json

{
  "uri": "s3://bucket/path",
  "credentials_profile": "default"
}
```

**Response**: `200 OK` or `400 Bad Request`

---

### Entries

#### List Entries
```http
GET /spaces/{space_id}/entries
```

**Response**: `200 OK`
```json
[
  {
    "id": "entry-uuid",
    "title": "Weekly Sync",
    "form": "Meeting",
    "updated_at": "2025-11-29T10:00:00Z",
    "properties": { "Date": "2025-11-29" },
    "tags": ["project-alpha"],
    "links": []
  }
]
```

#### Create Entry
```http
POST /spaces/{space_id}/entries
Content-Type: application/json

{
  "markdown": "# My Entry\n\n## Field\nValue"
}
```

**Response**: `201 Created`
```json
{
  "id": "entry-new-uuid",
  "title": "My Entry",
  "revision_id": "rev-0001",
  "properties": { "Field": "Value" }
}
```

#### Get Entry
```http
GET /spaces/{space_id}/entries/{entry_id}
```

**Response**: `200 OK`
```json
{
  "id": "entry-uuid",
  "markdown": "# My Entry\n\n## Field\nValue",
  "revision_id": "rev-0001",
  "properties": { "Field": "Value" }
}
```

#### Update Entry
```http
PUT /spaces/{space_id}/entries/{entry_id}
Content-Type: application/json

{
  "markdown": "# Updated Entry",
  "parent_revision_id": "rev-0001"
}
```

**Response**: `200 OK`
```json
{
  "id": "entry-uuid",
  "revision_id": "rev-0002"
}
```

**Error**: `409 Conflict` if `parent_revision_id` doesn't match current

#### Delete Entry
```http
DELETE /spaces/{space_id}/entries/{entry_id}
```

**Response**: `204 No Content`

#### Get Entry History
```http
GET /spaces/{space_id}/entries/{entry_id}/history
```

**Response**: `200 OK`
```json
{
  "entry_id": "entry-uuid",
  "revisions": [
    { "revision_id": "rev-0001", "timestamp": "2025-11-01T12:00:00Z" },
    { "revision_id": "rev-0002", "timestamp": "2025-11-29T10:00:00Z" }
  ]
}
```

#### Get Revision
```http
GET /spaces/{space_id}/entries/{entry_id}/history/{revision_id}
```

**Response**: `200 OK`

#### Restore Revision
```http
POST /spaces/{space_id}/entries/{entry_id}/restore
Content-Type: application/json

{
  "revision_id": "rev-0001"
}
```

**Response**: `200 OK`

---

### Forms

#### List Forms
```http
GET /spaces/{space_id}/forms
```

**Response**: `200 OK`
```json
[
  {
    "name": "Meeting",
    "version": 1,
    "fields": { "Date": { "type": "date", "required": true } }
  }
]
```

#### Get Form
```http
GET /spaces/{space_id}/forms/{name}
```

**Response**: `200 OK`

#### Create/Update Form
```http
PUT /spaces/{space_id}/forms/{name}
Content-Type: application/json

{
  "name": "Meeting",
  "version": 1,
  "fields": {
    "Date": { "type": "date", "required": true },
    "Attendees": { "type": "list", "required": false },
    "Related": { "type": "row_reference", "required": false, "target_form": "Project" }
  }
}
```

**Info**: The entry template is fixed globally (`# {form_name}` + H2 columns) and is not
customizable per form.

**Response**: `200 OK`

#### Delete Form
```http
DELETE /spaces/{space_id}/forms/{name}
```

**Response**: `204 No Content` or `409 Conflict` if entries still reference it

#### List Column Types
```http
GET /spaces/{space_id}/forms/types
```

---

### SQL (Saved Queries)

#### List SQL
```http
GET /spaces/{space_id}/sql
```

**Response**: `200 OK`
```json
[
  {
    "id": "sql-uuid",
    "name": "Recent Meetings",
    "sql": "SELECT * FROM Meeting WHERE Date >= {{since}} ORDER BY updated_at DESC LIMIT 50",
    "variables": [
      { "type": "date", "name": "since", "description": "Lower bound date" }
    ],
    "created_at": "2026-02-01T00:00:00Z",
    "updated_at": "2026-02-01T00:00:00Z",
    "revision_id": "rev-0001"
  }
]
```

#### Create SQL
```http
POST /spaces/{space_id}/sql
Content-Type: application/json

{
  "name": "Recent Meetings",
  "sql": "SELECT * FROM Meeting WHERE Date >= {{since}} ORDER BY updated_at DESC LIMIT 50",
  "variables": [
    { "type": "date", "name": "since", "description": "Lower bound date" }
  ]
}
```

**Response**: `201 Created`
```json
{
  "id": "sql-uuid",
  "revision_id": "rev-0001"
}
```

#### Get SQL
```http
GET /spaces/{space_id}/sql/{sql_id}
```

**Response**: `200 OK`
```json
{
  "id": "sql-uuid",
  "name": "Recent Meetings",
  "sql": "SELECT * FROM Meeting WHERE Date >= {{since}} ORDER BY updated_at DESC LIMIT 50",
  "variables": [
    { "type": "date", "name": "since", "description": "Lower bound date" }
  ],
  "created_at": "2026-02-01T00:00:00Z",
  "updated_at": "2026-02-01T00:00:00Z",
  "revision_id": "rev-0001"
}
```

#### Update SQL
```http
PUT /spaces/{space_id}/sql/{sql_id}
Content-Type: application/json

{
  "name": "Recent Meetings",
  "sql": "SELECT * FROM Meeting WHERE Date >= {{since}}",
  "variables": [
    { "type": "date", "name": "since", "description": "Lower bound date" }
  ],
  "parent_revision_id": "rev-0001"
}
```

**Response**: `200 OK`
```json
{
  "id": "sql-uuid",
  "revision_id": "rev-0002"
}
```

#### Delete SQL
```http
DELETE /spaces/{space_id}/sql/{sql_id}
```

**Response**: `204 No Content`

---

### Assets

#### Upload Asset
```http
POST /spaces/{space_id}/assets
Content-Type: multipart/form-data

file=@audio.m4a
```

**Response**: `201 Created`
```json
{
  "id": "a1b2c3d4",
  "name": "audio.m4a",
  "path": "assets/a1b2c3d4.m4a"
}
```

#### Delete Asset
```http
DELETE /spaces/{space_id}/assets/{id}
```

**Response**: `204 No Content` or `409 Conflict` if still referenced

---

### Query & Search

#### Structured Query
```http
POST /spaces/{space_id}/query
Content-Type: application/json

{
  "filter": {
    "form": "Meeting",
    "properties.Date": { "$gt": "2025-01-01" }
  }
}
```

**Response**: `200 OK`

#### SQL Sessions

SQL queries run through session-based endpoints. Creating a session stores
**metadata only** under `sql_sessions/`. Result rows are never persisted; each
request re-runs the SQL against the current entries tables. The
`view.snapshot_id` is a logical marker reserved for future materialized view
support.

##### Create SQL Session
```http
POST /spaces/{space_id}/sql-sessions
Content-Type: application/json

{
  "sql": "SELECT * FROM Meeting WHERE Date >= '2025-01-01' ORDER BY updated_at DESC LIMIT 50"
}
```

**Response**: `201 Created`
```json
{
  "id": "session-uuid",
  "space_id": "space-uuid",
  "sql_id": "sql-uuid",
  "sql": "SELECT * FROM Meeting WHERE Date >= '2025-01-01' ORDER BY updated_at DESC LIMIT 50",
  "status": "ready",
  "created_at": "2026-02-03T12:00:00Z",
  "expires_at": "2026-02-03T12:10:00Z",
  "view": {
    "sql_id": "sql-uuid",
    "snapshot_id": 42,
    "snapshot_at": "2026-02-03T12:00:00Z",
    "schema_version": 1
  },
  "pagination": {"strategy": "offset", "order_by": ["updated_at", "id"], "default_limit": 50, "max_limit": 1000},
  "count": {"mode": "on_demand", "cached_at": null, "value": null},
  "error": null
}
```

##### Get SQL Session Status
```http
GET /spaces/{space_id}/sql-sessions/{session_id}
```

**Response**: `200 OK`

##### Get SQL Session Count
```http
GET /spaces/{space_id}/sql-sessions/{session_id}/count
```

**Response**: `200 OK`
```json
{
  "count": 42
}
```

##### Get SQL Session Rows (paged)
```http
GET /spaces/{space_id}/sql-sessions/{session_id}/rows?offset=0&limit=50
```

**Response**: `200 OK`
```json
{
  "rows": [/* entry records */],
  "offset": 0,
  "limit": 50,
  "total_count": 42
}
```

##### Stream SQL Session Rows
```http
GET /spaces/{space_id}/sql-sessions/{session_id}/stream
```

**Response**: `200 OK` (NDJSON stream)

#### Keyword Search
```http
GET /spaces/{space_id}/search?q=project
```

**Response**: `200 OK`

---

## Error Responses

| Status | Description |
|--------|-------------|
| `400` | Bad Request - Invalid input |
| `404` | Not Found - Resource doesn't exist |
| `409` | Conflict - Duplicate or version mismatch |
| `422` | Validation Error - Form violation |
| `500` | Internal Server Error |

Error response format:
```json
{
  "detail": "Error description"
}
```

## Headers

### Response Headers

| Header | Description |
|--------|-------------|
| `X-Request-ID` | Unique request identifier |
| `X-Content-HMAC` | HMAC signature for response body |
| `Content-Type` | Always `application/json` |
