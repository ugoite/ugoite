# Local development login

```bash
mise run setup
mise run dev
```

The integrated launcher starts three processes:

- Rust server: REST API and auth backend.
- Browser app: the server-backed SolidStart UI.
- Docsite: the Astro docs site at `http://127.0.0.1:4321`.

The integrated launcher defaults to `mock-oauth`.

```bash
ugoite config set --mode backend --backend-url http://127.0.0.1:8000
eval "$(ugoite auth login --mock-oauth)"
ugoite auth profile
```

Use the server and browser URLs printed by the launcher when their ports differ. Direct loopback use reads the configured bootstrap token; proxied/container development may additionally require `UGOITE_DEV_AUTH_PROXY_TOKEN`.

Although username/TOTP options exist in the CLI shape, the current Rust server rejects `/auth/login`.
