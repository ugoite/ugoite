---
title: Helm chart
---

```bash
helm upgrade --install ugoite charts/ugoite \
  --set publicOrigin=https://ugoite.example.com \
  --set webauthnRpId=ugoite.example.com
```

The chart mounts one PVC at `/data`, runs as non-root UID/GID `10001`, drops
capabilities, and does not install a default credential. Read the initial
one-use setup URL from the pod log. Keep one replica in this release: browser
sessions and access-token issuers are node-local and are not federated.
