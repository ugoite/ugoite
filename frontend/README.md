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

To keep dev/prod and Codespaces simple and consistent, we require `DEV_BACKEND_URL` to be set when running in development. Use these rules:

- `DEV_BACKEND_URL` (dev-only): MUST be set and MUST be a path (e.g., `/api`). This signals the frontend dev server that the backend is available under the same domain and path.
- `DEV_BACKEND_PROXY_TARGET` (dev-only): Set this to the backend service reachable from the dev server (e.g., `http://localhost:8000` or `http://backend:8000` in a container environment).
- `VITE_BACKEND_URL` (exposed to client code via import.meta.env): In development we set this to `/api` (so client code resolves a same-origin path). In production builds you can set a public absolute URL like `https://api.example.com`.

Examples:
- Docker Compose (dev): set `DEV_BACKEND_URL=/api`, `DEV_BACKEND_PROXY_TARGET=http://backend:8000`, and `VITE_BACKEND_URL=/api` so the dev server proxies `/api` to the backend container.
- Local dev (mise run dev): `npm run dev` will use `VITE_BACKEND_URL=/api`, `DEV_BACKEND_URL=/api`, and `DEV_BACKEND_PROXY_TARGET=http://localhost:8000` (see `frontend/mise.toml`).

Note about container hostnames and Codespaces: Do not set `VITE_BACKEND_URL` to container-only DNS names in builds (like `http://backend:8000`), because browsers outside the container won't resolve them. For development, using `/api` plus the dev server proxy is the recommended approach.


## Building

Solid apps are built with _presets_, which optimise your project for deployment to different environments.

By default, `npm run build` will generate a Node app that you can run with `npm start`. To use a different preset, add it to the `devDependencies` in `package.json` and specify in your `app.config.js`.

## This project was created with the [Solid CLI](https://github.com/solidjs-community/solid-cli)
