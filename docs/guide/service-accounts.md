# Service Account Operations

Use this guide when automation needs a space-scoped API key instead of a human's
interactive bearer token.

## 1) When to use service accounts

Choose credentials by operator intent:

- **Bearer token**: a human signs in through the browser or CLI and gets an
  interactive user session.
- **Service account key**: a bot, CI job, sync worker, or integration needs
  non-interactive access that stays scoped to one space.

Prefer service accounts when you want least-privilege automation, a dedicated
display name for audit trails, and a key you can rotate or revoke without
breaking a real user's personal login flow.

## 2) Prerequisites

Before you create keys:

1. Run the backend/API surface that will receive the requests.
2. Authenticate as a user who already has `space_admin` on the target space.
3. Decide the narrowest scopes the automation actually needs, such as
   `entry_read` for read-only jobs or `entry_read,entry_write` for a writer.
4. Prepare a secret manager or deployment secret store, because key creation and
   rotation reveal the raw secret only once.

## 3) Create or inspect the service account

Today the CLI exposes the service-account list/create flows in `backend` or
`api` mode:

```bash
ugoite config set --mode backend --backend-url http://127.0.0.1:8000
ugoite space service-account-create my-space --display-name "CI Read Bot" --scopes entry_read
ugoite space service-account-list my-space
```

That creates the service principal itself. Key lifecycle actions are still most
straightforward through the REST endpoints below.

## 4) Create the first API key

```bash
export SPACE_ID=my-space
export SERVICE_ACCOUNT_ID=svc-account-id
export UGOITE_TOKEN=user-bearer-token

curl -sS -X POST \
  "http://127.0.0.1:8000/spaces/$SPACE_ID/service-accounts/$SERVICE_ACCOUNT_ID/keys" \
  -H "Authorization: Bearer $UGOITE_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"key_name":"ci-read-2026-04"}'
```

The response returns key metadata plus a one-time `secret`. Store that secret
immediately and hand it to the automation client through your normal secret
distribution path.

Use the secret as an API key header from then on:

```bash
export SERVICE_ACCOUNT_SECRET=replace-with-created-secret

curl -sS \
  "http://127.0.0.1:8000/spaces/$SPACE_ID/entries" \
  -H "X-API-Key: $SERVICE_ACCOUNT_SECRET"
```

## 5) Rotate a key

Use rotation for planned replacement when the automation still needs access but
you want a fresh secret:

```bash
export KEY_ID=svc-key-id

curl -sS -X POST \
  "http://127.0.0.1:8000/spaces/$SPACE_ID/service-accounts/$SERVICE_ACCOUNT_ID/keys/$KEY_ID/rotate" \
  -H "Authorization: Bearer $UGOITE_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"key_name":"ci-read-2026-05"}'
```

Rotation returns a new one-time `secret`. Update the automation to use the new
secret immediately after the call succeeds.

## 6) Revoke a key

Use revocation when the integration is retired, the secret may have leaked, or
you want to cut off access immediately:

```bash
curl -sS -X DELETE \
  "http://127.0.0.1:8000/spaces/$SPACE_ID/service-accounts/$SERVICE_ACCOUNT_ID/keys/$KEY_ID" \
  -H "Authorization: Bearer $UGOITE_TOKEN"
```

Revoked keys fail authentication immediately across the REST and MCP HTTP
surfaces.

## 7) Security expectations

- Keep scopes minimal. Do not give write access to a read-only bot.
- Use service accounts for automation instead of sharing a human's bearer token.
- Treat create/rotate responses as secret material: the raw key is for immediate
  capture, not repeated display.
- Prefer rotation for planned replacements and revocation for incident response
  or retirement.
- Expect key usage to stay audit-visible under the service account identity
  instead of blending into a user's personal session history.

## 8) Where to go next

- Need the broader auth model first? Read [Authentication Overview](auth-overview.md).
- Need exact endpoint contracts? Read [REST API](../spec/api/rest.md).
- Need the runtime capability snapshot? Run `ugoite auth overview`.
