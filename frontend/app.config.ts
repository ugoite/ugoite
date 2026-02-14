import { defineConfig } from "@solidjs/start/config";
import tailwindcss from "@tailwindcss/vite";
import type { ProxyOptions } from "vite";
import { VitePWA } from "vite-plugin-pwa";

const env =
	(globalThis as { process?: { env?: Record<string, string | undefined> } }).process?.env ?? {};

const backendUrl = env.BACKEND_URL;
const useViteProxy = env.VITE_API_PROXY === "true";

const sharedDir = new URL("../shared", import.meta.url).pathname;

const proxyRule: Record<string, ProxyOptions> = {};

if (backendUrl && useViteProxy) {
	proxyRule["/api"] = {
		target: backendUrl,
		changeOrigin: true,
		secure: false,
		rewrite: (path: string) => path.replace(/^\/api/, ""),
	};
} else if (env.NODE_ENV === "development") {
	throw new Error(
		"BACKEND_URL must be set for development (e.g., BACKEND_URL=http://localhost:8000).",
	);
}

export default defineConfig({
	server: {
		errorHandler: "~/error-handler",
	},
	vite: {
		plugins: [
			tailwindcss(),
			VitePWA({
				registerType: "autoUpdate",
				injectRegister: "auto",
				includeAssets: ["favicon.ico"],
				manifest: {
					name: "Ugoite",
					short_name: "Ugoite",
					description: "Local-first, AI-native knowledge space",
					theme_color: "#111827",
					background_color: "#111827",
					display: "standalone",
					start_url: "/",
					scope: "/",
					icons: [
						{
							src: "/favicon.ico",
							sizes: "64x64 32x32 24x24 16x16",
							type: "image/x-icon",
						},
					],
				},
				workbox: {
					globPatterns: ["**/*.{js,css,html,ico,png,svg}"],
				},
			}),
		],
		server: {
			proxy: proxyRule,
			fs: {
				allow: [sharedDir],
			},
		},
		resolve: {
			dedupe: [
				"@codemirror/autocomplete",
				"@codemirror/lang-sql",
				"@codemirror/lint",
				"@codemirror/state",
				"@codemirror/view",
			],
		},
	},
});
