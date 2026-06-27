# Ugoite

Ugoite is a local-first knowledge-space system built around operator-owned files.

> A private, portable knowledge space you can run with Docker, automate from the CLI, and keep on infrastructure you control.

The repository-level [`docs/`](docs/index.md) directory is the single source of truth for product, operator, architecture, and specification documentation. The Starlight site renders those files directly; this README intentionally stays small so it cannot drift into a second manual.

## Start

- [Container quick start](docs/guide/container-quickstart.md)
- [Local development](docs/guide/local-dev-auth-login.md)
- [CLI guide](docs/guide/cli.md)
- [Architecture](docs/architecture/index.md)
- [REST and OpenAPI](docs/spec/api/rest.md)
- [Executable specification](docs/spec/index.md)

For repository development, install [mise](https://mise.jdx.dev/) and run:

```bash
mise run setup
mise run dev
```

Validation is centralized at the repository root:

```bash
mise run fmt
mise run lint
mise run check
mise run test
```

## License

MIT
