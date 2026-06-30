# Ugoite Helm chart

```bash
kubectl create secret generic ugoite-node-secret \
  --from-literal=encryption-key="$(head -c 32 /dev/urandom | base64)"
helm upgrade --install ugoite charts/ugoite \
  --set publicOrigin=https://ugoite.example.com \
  --set webauthnRpId=ugoite.example.com \
  --set nodeSecret.existingSecret=ugoite-node-secret
```

The pod runs as non-root UID/GID `10001`, drops capabilities, and mounts one
PVC with separate `spaces/` and `_ugoite/nodes/{node_id}` data. The referenced
Secret must contain a 32-byte-or-longer `encryption-key`. Read the one-use
setup URL from the pod log and complete either two-Passkey or Passkey + TOTP setup.
The chart creates no default credential or authentication Secret.
