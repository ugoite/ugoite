import { defineConfig } from "@solidjs/start/config";
import tailwindcss from "@tailwindcss/vite";
import type { ProxyOptions } from "vite";

const backendUrl = process.env.BACKEND_URL;

const proxyRule: Record<string, ProxyOptions> = {};

if (backendUrl) {
	proxyRule["/api"] = {
		target: backendUrl,
		changeOrigin: true,
		secure: false,
		rewrite: (path: string) => path.replace(/^\/api/, ""),
	};
} else if (process.env.NODE_ENV === "development") {
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
