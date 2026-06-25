# Local development login

```bash
mise run setup
mise run dev
```

The integrated launcher starts the Rust server, browser, and docsite and defaults to `mock-oauth`.

```bash
ugoite config set --mode backend --backend-url http://127.0.0.1:8000
eval "$(ugoite auth login --mock-oauth)"
ugoite auth profile
```

Use the server URL printed by the launcher when its port differs. Direct loopback use reads the configured bootstrap token; proxied/container development may additionally require `UGOITE_DEV_AUTH_PROXY_TOKEN`.

Although username/TOTP options exist in the CLI shape, the current Rust server rejects `/auth/login`.
