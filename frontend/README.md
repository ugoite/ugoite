# SolidStart

Everything you need to build a Solid project, powered by [`solid-start`](https://start.solidjs.com);

## Creating a project

```bash
# create a new project in the current directory
npm init solid@latest

# create a new project in my-app
npm init solid@latest my-app
```

## Developing

Once you've created a project and installed dependencies with `npm install` (or `pnpm install` or `yarn`), start a development server:

```bash
npm run dev

# or start the server and open the app in a new browser tab
npm run dev -- --open
```

## Backend API configuration

To keep dev/prod and Codespaces simple and consistent, we use `BACKEND_URL` to configure the backend connection during development.

- `BACKEND_URL` (dev-only): Set this to the backend service reachable from the dev server (e.g., `http://localhost:8000` or `http://backend:8000` in a container environment).
- The frontend dev server will automatically proxy requests starting with `/api` to the configured `BACKEND_URL`.
- Client code always uses `/api` to access the backend.

Examples:
- Docker Compose (dev): set `BACKEND_URL=http://backend:8000` so the dev server proxies `/api` to the backend container.
- Local dev (mise run dev): `npm run dev` will use `BACKEND_URL=http://localhost:8000` (see `frontend/mise.toml`).



## Building

Solid apps are built with _presets_, which optimise your project for deployment to different environments.

By default, `npm run build` will generate a Node app that you can run with `npm start`. To use a different preset, add it to the `devDependencies` in `package.json` and specify in your `app.config.js`.

## This project was created with the [Solid CLI](https://github.com/solidjs-community/solid-cli)
