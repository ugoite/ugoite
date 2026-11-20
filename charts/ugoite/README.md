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
setup URL from the pod log, create the administrator Passkey, save the returned
recovery codes, then register a second Passkey in the setup flow. TOTP can be
enrolled later for account self-recovery; it is not a setup-strengthening
alternative or a normal login method. The chart creates no default credential
or authentication Secret.

Release packaging and chart validation establish the Helm artifact, not
production Kubernetes support. The chart defaults to one replica and uses a
single PVC; package verification does not certify failover or high availability.
Operators remain responsible for evaluating cluster storage, ingress/TLS,
backups, and recovery.
