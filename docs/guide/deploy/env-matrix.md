---
title: Environment variables
sidebar:
  order: 4
---

| Variable                  | Purpose                                                            | Default                  |
| ------------------------- | ------------------------------------------------------------------ | ------------------------ |
| `UGOITE_ROOT`             | Operator-owned Space storage and, by default, Node control state    | `./data`                 |
| `UGOITE_STORAGE_ENDPOINT` | Optional non-secret endpoint used when opening the active storage operator | unset                    |
| `UGOITE_SERVER_ADDRESS`   | HTTP listen address                                                | `127.0.0.1:8000`         |
| `UGOITE_STATIC_DIR`       | Optional compiled browser directory                                | unset                    |
| `UGOITE_PUBLIC_ORIGIN`    | Exact public WebAuthn/OAuth issuer origin                          | `http://localhost:8000`  |
| `UGOITE_API_BASE_URL`     | Public API base, including `/api` for the integrated browser image | public origin            |
| `UGOITE_WEBAUTHN_RP_ID`   | WebAuthn relying-party ID                                          | public-origin host       |
| `UGOITE_NODE_CONTROL_URI` | Optional separate durable OpenDAL URI for Node control state       | `UGOITE_ROOT` storage    |
| `UGOITE_NODE_SECRET_KEY`  | AEAD root key of at least 32 bytes                                 | required unless file set |
| `UGOITE_NODE_SECRET_FILE` | Mounted file containing the AEAD root key                          | unset                    |
| `BACKEND_URL`             | Frontend server proxy target                                       | deployment-specific      |

Remote deployments must use an HTTPS public origin. Authentication credentials
are generated and persisted by Ugoite; no environment variable accepts a
password, API key, bearer token, or setup credential. The Node secret encrypts
recoverable values and must be stored outside the control-store namespace. A
separate `UGOITE_NODE_CONTROL_URI` backend and the node secret are separate
backup inputs; a `/data` snapshot covers them only when they are actually in
that default local layout.
