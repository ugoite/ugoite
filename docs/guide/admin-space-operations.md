# Admin-space Operations

Use this guide when you need to understand who can create spaces, why the
reserved `admin-space` exists, or why a browser/CLI/backend request is being
told it cannot create a new space.

## 1) What `admin-space` is for

`admin-space` is the reserved workspace that gates server-backed space creation.
For backend, API, and browser flows, `POST /spaces` only succeeds when the
caller is an active admin of that reserved space.

This is separate from normal user-facing spaces. A regular space such as
`default`, `team-notes`, or `project-alpha` is where people store real content.
`admin-space` exists to hold administration authority, not everyday notes.

## 2) Who becomes an admin and when

The baseline rules are:

1. Local dev and source Docker Compose bootstrap the configured
   `UGOITE_DEV_USER_ID` into the reserved `admin-space` at startup.
2. Only active admins of `admin-space` can create additional spaces through the
   backend/API/browser surfaces.
3. The creator of each new non-admin space becomes that space's initial admin.

That means "can create spaces" and "can administer this particular user-facing
space" are related but not identical questions.

## 3) When the rule applies

The `admin-space` gate only matters for server-backed creation:

- **Browser**: creating a space from `/spaces` uses the backend and therefore
  needs an authenticated `admin-space` admin.
- **CLI in `backend` or `api` mode**: `ugoite space create my-space` is a remote
  create request and therefore needs `admin-space` admin authority.
- **Backend REST callers**: `POST /spaces` follows the same rule.
- **CLI in `core` mode**: `ugoite space create ./spaces/local-space` writes
  directly to the local filesystem. That path does not depend on server-side
  `admin-space` membership.

## 4) Common "why can't I create a space?" checks

### Browser

- Confirm you actually completed the login flow and reached `/spaces` as an
  authenticated user.
- In local development, confirm you signed in as the configured
  `UGOITE_DEV_USER_ID`.
- If `admin-space` appears only in the reserved admin section, that is expected;
  it still controls whether the create-space action is authorized.

### CLI

First confirm which topology the CLI is using:

```bash
ugoite config current
ugoite auth profile
```

Then interpret the result:

- If the CLI is in `backend` or `api` mode, a failed remote create usually means
  the authenticated user is not an active `admin-space` admin yet.
- If the CLI is in `core` mode, use a local path such as `./spaces/demo` instead
  of a bare `SPACE_ID` and do not expect remote admin membership checks.

## 5) Troubleshooting backend/API 403 responses

When `POST /spaces` returns `403 Forbidden`, work through these questions:

1. Which user identity is attached to the request right now?
2. Is that identity an active admin of `admin-space`?
3. Are you accidentally using a service credential or stale bearer token that
   belongs to a different user?
4. Are you expecting a `core`-mode local create, but actually sending a
   backend/API request?

If the request is remote and the answer to item 2 is "no", fix the membership
first; creating a new user-facing space is supposed to stay blocked until that
admin-space authorization exists.

## 6) Where to go next

- Need the exact local login bootstrap flow? Read
  [Local Development Authentication and Login](local-dev-auth-login.md).
- Need broader auth context? Read [Authentication Overview](auth-overview.md).
- Need the endpoint contract? Read [REST API](../spec/api/rest.md).
