# REST API Specification

## Overview

The REST API is the primary interface for the frontend and external integrations.

**Base URL**: `http://localhost:8000` (development)

## Authentication

By default, the API binds to localhost only and requires no authentication.
When `IEAPP_ALLOW_REMOTE=true`, authentication is required (future milestone).

## Endpoints

### Workspaces

#### List Workspaces
```http
GET /workspaces
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

#### Create Workspace
```http
POST /workspaces
Content-Type: application/json

{
  "id": "ws-new",
  "name": "New Workspace"
}
```

**Response**: `201 Created`

#### Get Workspace
```http
GET /workspaces/{id}
```

**Response**: `200 OK`

#### Update Workspace
```http
PATCH /workspaces/{id}
Content-Type: application/json

{
  "name": "Updated Name",
  "storage_config": { "uri": "s3://bucket/path" },
  "settings": { "default_class": "Meeting" }
}
```

**Response**: `200 OK`

#### Test Connection
```http
POST /workspaces/{id}/test-connection
Content-Type: application/json

{
  "uri": "s3://bucket/path",
  "credentials_profile": "default"
}
```

**Response**: `200 OK` or `400 Bad Request`

---

### Notes

#### List Notes
```http
GET /workspaces/{ws_id}/notes
```

**Response**: `200 OK`
```json
[
  {
    "id": "note-uuid",
    "title": "Weekly Sync",
    "class": "Meeting",
    "updated_at": "2025-11-29T10:00:00Z",
    "properties": { "Date": "2025-11-29" },
    "tags": ["project-alpha"],
    "links": []
  }
]
```

#### Create Note
```http
POST /workspaces/{ws_id}/notes
Content-Type: application/json

{
  "markdown": "# My Note\n\n## Field\nValue"
}
```

**Response**: `201 Created`
```json
{
  "id": "note-new-uuid",
  "title": "My Note",
  "revision_id": "rev-0001",
  "properties": { "Field": "Value" }
}
```

#### Get Note
```http
GET /workspaces/{ws_id}/notes/{note_id}
```

**Response**: `200 OK`
```json
{
  "id": "note-uuid",
  "markdown": "# My Note\n\n## Field\nValue",
  "revision_id": "rev-0001",
  "properties": { "Field": "Value" }
}
```

#### Update Note
```http
PUT /workspaces/{ws_id}/notes/{note_id}
Content-Type: application/json

{
  "markdown": "# Updated Note",
  "parent_revision_id": "rev-0001"
}
```

**Response**: `200 OK`
```json
{
  "id": "note-uuid",
  "revision_id": "rev-0002"
}
```

**Error**: `409 Conflict` if `parent_revision_id` doesn't match current

#### Delete Note
```http
DELETE /workspaces/{ws_id}/notes/{note_id}
```

**Response**: `204 No Content`

#### Get Note History
```http
GET /workspaces/{ws_id}/notes/{note_id}/history
```

**Response**: `200 OK`
```json
{
  "note_id": "note-uuid",
  "revisions": [
    { "revision_id": "rev-0001", "timestamp": "2025-11-01T12:00:00Z" },
    { "revision_id": "rev-0002", "timestamp": "2025-11-29T10:00:00Z" }
  ]
}
```

#### Get Revision
```http
GET /workspaces/{ws_id}/notes/{note_id}/history/{revision_id}
```

**Response**: `200 OK`

#### Restore Revision
```http
POST /workspaces/{ws_id}/notes/{note_id}/restore
Content-Type: application/json

{
  "revision_id": "rev-0001"
}
```

**Response**: `200 OK`

---

### Classes

#### List Classes
```http
GET /workspaces/{ws_id}/classes
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

#### Get Class
```http
GET /workspaces/{ws_id}/classes/{name}
```

**Response**: `200 OK`

#### Create/Update Class
```http
PUT /workspaces/{ws_id}/classes/{name}
Content-Type: application/json

{
  "name": "Meeting",
  "version": 1,
  "fields": {
    "Date": { "type": "date", "required": true },
    "Attendees": { "type": "list", "required": false }
  }
}
```

**Note**: The note template is fixed globally (`# {class_name}` + H2 columns) and is not
customizable per class.

**Response**: `200 OK`

#### Delete Class
```http
DELETE /workspaces/{ws_id}/classes/{name}
```

**Response**: `204 No Content` or `409 Conflict` if notes still reference it

#### List Column Types
```http
GET /workspaces/{ws_id}/classes/types
```

**Response**: `200 OK`
```json
["string", "number", "date", "list", "markdown"]
```

---

### Attachments

#### Upload Attachment
```http
POST /workspaces/{ws_id}/attachments
Content-Type: multipart/form-data

file=@audio.m4a
```

**Response**: `201 Created`
```json
{
  "id": "a1b2c3d4",
  "name": "audio.m4a",
  "path": "attachments/a1b2c3d4.m4a"
}
```

#### Delete Attachment
```http
DELETE /workspaces/{ws_id}/attachments/{id}
```

**Response**: `204 No Content` or `409 Conflict` if still referenced

---

### Links

#### List Links
```http
GET /workspaces/{ws_id}/links
```

**Response**: `200 OK`

#### Create Link
```http
POST /workspaces/{ws_id}/links
Content-Type: application/json

{
  "source": "note-id-1",
  "target": "note-id-2",
  "kind": "related"
}
```

**Response**: `201 Created`

#### Delete Link
```http
DELETE /workspaces/{ws_id}/links/{id}
```

**Response**: `204 No Content`

---

### Query & Search

#### Structured Query
```http
POST /workspaces/{ws_id}/query
Content-Type: application/json

{
  "filter": {
    "class": "Meeting",
    "properties.Date": { "$gt": "2025-01-01" }
  }
}
```

**Response**: `200 OK`

#### Keyword Search
```http
GET /workspaces/{ws_id}/search?q=project
```

**Response**: `200 OK`

---

## Error Responses

| Status | Description |
|--------|-------------|
| `400` | Bad Request - Invalid input |
| `404` | Not Found - Resource doesn't exist |
| `409` | Conflict - Duplicate or version mismatch |
| `422` | Validation Error - Class violation |
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
