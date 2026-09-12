---
title: "Troubleshooting"
description: Symptom-first fixes that keep Knowledge safe.
sidebar:
  order: 8
---

Start from the symptom, not the subsystem. Each entry names the symptom, the
likely cause, the check, the fix, and what remains safe.

## I cannot sign in

**Symptom:** Browser login fails or the session is rejected.

**Likely cause:** Passkey not registered, wrong origin, or expired session.

**Check:** confirm the public origin and WebAuthn relying-party ID, then retry
with a registered Passkey.

**Fix:** complete the one-use setup URL on first start; use the
[authentication overview](../guide/operate/auth/auth-overview.md) for bootstrap
and recovery.

**What remains safe:** the Space prefix and history are untouched by login
failures.

## I cannot see a Space

**Symptom:** The expected Space is missing from the switcher or list output.

**Likely cause:** wrong endpoint mode, Space ID versus path confusion, or
missing membership.

**Check:** run `ugoite config current`, confirm path versus ID, and list Spaces
in the active mode.

**Fix:** switch modes explicitly and request access through owner-approved
recovery. See [Spaces](../use/spaces.mdx) and
[unauthorized Spaces](../guide/troubleshoot/troubleshooting-unauthorized-spaces.md).

**What remains safe:** other Spaces and their histories.

## My Entry was rejected

**Symptom:** Entry create or update returns a validation error.

**Likely cause:** unknown Form name or invalid typed field values.

**Check:** list Forms and field types, then compare frontmatter.

**Fix:** correct the Form name and values, then retry. See
[Create and Edit Entries](../use/entries.mdx).

**What remains safe:** prior revisions stay readable.

## I got a revision conflict

**Symptom:** An update fails against a stale parent revision.

**Likely cause:** editing against a stale parent revision.

**Check:** re-read history for the newest revision.

**Fix:** reapply the edit with the newest parent. See
[Revisions and Recovery](../use/revisions.mdx).

**What remains safe:** no revision is overwritten; history only appends.

## Search does not show my change

**Symptom:** A saved Entry does not appear in keyword results.

**Likely cause:** the Entry did not save, or the query targets the wrong Space
or Form.

**Check:** confirm the saved status, then rerun the keyword search.

**Fix:** save first, then search the same Space. See
[Search and Query](../use/search.mdx).

**What remains safe:** the durable Entry even when the index lags.

## The server does not start

**Symptom:** The Compose service fails to start or accept connections.

**Likely cause:** bad origin or secret configuration, or an unready mount.

**Check:** find the mapped port with `docker compose port ugoite 8000` and read
the startup logs with redaction.

**Fix:** correct the environment, then restart. See
[Compose startup troubleshooting](../guide/troubleshoot/troubleshooting-compose-startup.md)
and [log redaction](../guide/troubleshoot/log-redaction.md).

**What remains safe:** Space content on the configured prefix when the three
recovery inputs are preserved separately.
