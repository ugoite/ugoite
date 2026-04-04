# Docsite

Astro + Bun + Tailwind based documentation site.

For the canonical auth-aware contributor workflow that starts backend,
frontend, and docsite together, return to the repository root and run
`mise run dev` as described in the main [README](../README.md#setup--development-mise).
Use the commands below when you intentionally want docsite-only iteration.

## Commands

```bash
mise run //docsite:install
mise run //docsite:check
mise run //docsite:build
mise run //docsite:dev
```

This site prefers rendering content from the repository `docs/` directory instead of duplicating full prose in `docsite/`.

## Localhost binding convention

Use `localhost` consistently for local URLs in docs and scripts.

- Preferred: `http://localhost:<port>`
- Avoid mixing with `127.0.0.1` unless a tool explicitly requires it.

## Deployment (subpath hosting)

When hosting under a subpath (for example `/ugoite`), set `DOCSITE_BASE` at build time.

```bash
cd docsite
DOCSITE_BASE=/ugoite DOCSITE_ORIGIN=https://example.com bun run build
```

- `DOCSITE_BASE`: mount path (for example `/`, `/ugoite`, `/docs/ugoite`)
- `DOCSITE_ORIGIN`: public site origin (used for Astro `site` config / absolute URL generation)
