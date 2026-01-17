import { defineConfig } from "@solidjs/start/config";
import tailwindcss from "@tailwindcss/vite";
import type { ProxyOptions } from "vite";

const env =
	(globalThis as { process?: { env?: Record<string, string | undefined> } }).process?.env ?? {};

const backendUrl = env.BACKEND_URL;

const proxyRule: Record<string, ProxyOptions> = {};

if (backendUrl) {
	proxyRule["/workspaces"] = {
		target: backendUrl,
		changeOrigin: true,
		secure: false,
	};
} else if (env.NODE_ENV === "development") {
	throw new Error(
		"BACKEND_URL must be set for development (e.g., BACKEND_URL=http://localhost:8000).",
	);
}

export default defineConfig({
	vite: {
		plugins: [tailwindcss()],
		server: {
			proxy: proxyRule,
		},
	},
});
