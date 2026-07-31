---
title: Helm chart
sidebar:
  order: 3
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

The PVC contains the default local Space and Node control-store files. A
backup of the PVC is not complete unless the Kubernetes Secret or other source
of the node secret is preserved too. If `UGOITE_NODE_CONTROL_URI` selects a
separate backend, back up that control-store prefix separately.
