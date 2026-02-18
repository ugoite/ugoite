# Docsite

Astro + Bun + Tailwind based documentation site.

## Commands

```bash
mise run //docsite:install
mise run //docsite:check
mise run //docsite:build
mise run //docsite:dev
```

This site prefers rendering content from the repository `docs/` directory instead of duplicating full prose in `docsite/`.

## Deployment (subpath hosting)

When hosting under a subpath (for example `/ugoite`), set `DOCSITE_BASE` at build time.

```bash
cd docsite
DOCSITE_BASE=/ugoite DOCSITE_ORIGIN=https://example.com bun run build
```

- `DOCSITE_BASE`: mount path (for example `/`, `/ugoite`, `/docs/ugoite`)
- `DOCSITE_ORIGIN`: public origin used for canonical URLs
