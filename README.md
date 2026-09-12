# Ugoite

Ugoite is a private, portable Knowledge Space for humans and AI. Knowledge stays
in an operator-owned Space; a server, browser session, model provider, or
generated experience does not become its owner.

> Knowledge persists. Work may disappear. Knowledge can become tools.

The repository-level [`docs/`](docs/index.md) directory is the single source of
truth for product, operator, architecture, and specification documentation. The
Starlight site renders those files directly; this README intentionally stays
small so it cannot drift into a second manual.

## Why Ugoite exists

- **Own your Knowledge.** Knowledge lives in your Space and remains portable
  across the runtimes that work with it.
- **Let humans and agents work with it.** The CLI, browser, MCP, and Konase
  operate on the same Space-owned Knowledge through shared semantics.
- **Turn Knowledge into tools.** The same Knowledge can eventually become
  purpose-built views and task-specific applications without moving into a
  second system of record.

## Quick start

- **Use it:** [Get started](docs/get-started/index.md) and the
  [Quickstart](docs/get-started/quickstart.mdx)
- **Operate it:** [Operate Ugoite](docs/operate/index.md)
- **Understand it:** [Vision and Concepts](docs/vision/index.md)

CLI core mode is the shipped direct-local path. The browser is currently
server-backed; browser-local persistence and optional synchronization are
planned, not shipped. Knowledge-to-tools is North Star, not a shipped builder.
Details live in the docsite: [Use Ugoite](docs/use/index.md),
[Reference](docs/reference/index.md), and the
[executable specification](docs/spec/index.md).

## Documentation

Start at [`docs/`](docs/index.md). Task pages, operator procedures, Vision,
engineering docs, reference entries, and the specification all live there and
render directly to the Starlight site.

## Development

Install [mise](https://mise.jdx.dev/) and run:

```bash
mise run setup
mise run dev
```

Validate at the repository root:

```bash
mise run fmt
mise run lint
mise run check
mise run test
```

## Release

`version.txt` is the canonical prepared product version. Ordinary pushes do not
update release metadata. See the
[release contract](docs/architecture/release/release-contract.md) and the
docsite [Upgrade and Compatibility](docs/operate/upgrade-compatibility.md) entry
point.

## License

MIT
