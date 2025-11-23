import { defineConfig } from "vite";
import type { UserConfig } from "vite";

// Configure dev server proxy so that API requests to the configured DEV_BACKEND_URL
// path are forwarded to the backend. The DEV_BACKEND_URL must be set to a path
// (such as "/api") in development to ensure requests are made on the same domain.
// The DEV_BACKEND_PROXY_TARGET points the dev-server proxy to the backend host
// (for example http://localhost:8000 or http://backend:8000 in docker-compose).
const devBackendUrl = process.env.DEV_BACKEND_URL;
if (!devBackendUrl) {
	throw new Error(
		"DEV_BACKEND_URL must be set for development (e.g., DEV_BACKEND_URL=/api).",
	);
}
if (!devBackendUrl.startsWith("/")) {
	throw new Error(
		"DEV_BACKEND_URL must start with a leading '/' when used as a path.",
	);
}
const proxyTarget =
	process.env.DEV_BACKEND_PROXY_TARGET ||
	process.env.VITE_BACKEND_URL ||
	"http://localhost:8000";

const proxyRule: Record<string, unknown> = {};
proxyRule[devBackendUrl] = {
	target: proxyTarget,
	changeOrigin: true,
	secure: false,
	rewrite: (path: string) => path.replace(new RegExp(`^${devBackendUrl}`), ""),
};

const config: UserConfig = {
	server: {
		proxy: proxyRule,
	},
};

export default defineConfig(config);
