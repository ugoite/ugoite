# Ugoite documentation site

This directory is only the Starlight build shell. Product and engineering documentation is authored once in the repository-level [`docs/`](../docs/) directory and loaded directly by `src/content.config.ts`.

```bash
deno task --cwd docsite dev
deno task --cwd docsite check
deno task --cwd docsite test
deno task --cwd docsite build
```

Repository-level orchestration stays at the root:

```bash
mise run build:docsite
mise run test:docsite
mise run package:docsite
mise run verify:docsite
```

Do not add product documentation, custom navigation, search code, or hand-authored routes under `docsite/`. Starlight owns those responsibilities; `docs/` owns the content.

## Why the collection loader is custom

Starlight normally loads `src/content/docs/` with `docsLoader()`. Ugoite deliberately keeps the canonical files at repository-level `docs/`, where GitHub, editors, release tooling, and the generated site all consume the same files. `src/content.config.ts` therefore uses Astro's glob loader with Starlight's `docsSchema()`, while `astro.config.mjs` adds `../docs` to Starlight's `markdown.processedDirs`.

This is the only intentional departure from the conventional Starlight project layout. Do not mirror or copy the files into `docsite/`.

## Deployment configuration

Local and CI builds default to `/` and omit a production origin. Production builds must set the public origin explicitly so canonical URLs and the sitemap are correct:

```bash
DOCSITE_ORIGIN=https://docs.example.com \
DOCSITE_BASE=/ \
  deno task --cwd docsite build
```

For GitHub project pages, use the account origin and repository base separately:

```bash
DOCSITE_ORIGIN=https://ugoite.github.io \
DOCSITE_BASE=/ugoite/ \
  deno task --cwd docsite build
```

`DOCSITE_BASE` is never inferred from `GITHUB_ACTIONS`; preview and validation workflows therefore cannot accidentally emit repository-prefixed links.

`DOCSITE_ORIGIN` and `DOCSITE_BASE` are explicit build inputs. Repository-wide checks are `mise run check`, `mise run test`, `mise run package`, and `mise run verify`.
