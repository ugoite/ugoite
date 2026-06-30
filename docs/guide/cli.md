---
title: CLI guide
---

Core mode is explicitly operator-local: it opens operator-owned Spaces directly,
uses OS/storage permissions as its trust boundary, and performs no human login:

```bash
ugoite config set --mode core
ugoite space list /path/to/workspace
```

Remote mode pairs a terminal by device authorization. The CLI does not require a
browser on that terminal:

```bash
ugoite config set --mode backend --backend-url https://ugoite.example.com
ugoite auth login --device-name workstation --actions read,create,update
```

Open the displayed verification URL on a signed-in phone or computer, compare
the code, Space, device name, and requested actions, then approve. The CLI
stores a P-256 private key in the OS keychain when available and otherwise in
`~/.ugoite/cli-credentials.json` with owner-only permissions. Five-minute DPoP
access tokens refresh using a rotating 30-day credential. `ugoite auth profile`
shows metadata; `ugoite auth logout` deletes local credentials. Revoke a lost
device from the browser credential page.
