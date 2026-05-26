import type { ProxyOptions } from "vite";

type ProcessLike = {
	env?: Record<string, string | undefined>;
};

const processLike = globalThis.process as ProcessLike | undefined;
const currentEnv = processLike?.env;
if (!currentEnv || typeof currentEnv !== "object") {
	/* v8 ignore start */
	if (processLike) {
		Object.defineProperty(processLike, "env", {
			configurable: true,
			writable: true,
			value: {},
		});
	} else {
		Object.defineProperty(globalThis, "process", {
			configurable: true,
			writable: true,
			value: {
				env: {},
				cwd: () => new URL(".", import.meta.url).pathname,
			},
		});
	}
	/* v8 ignore stop */
}

/* v8 ignore next */
const env = (globalThis.process as ProcessLike | undefined)?.env ?? {};

const [{ defineConfig }, { default: tailwindcss }, { VitePWA }] = await Promise.all([
	import("@solidjs/start/config"),
	import("@tailwindcss/vite"),
	import("vite-plugin-pwa"),
]);

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
		"BACKEND_URL must be set for frontend-only development. Use `mise run dev` from the repository root for the canonical auth-aware workflow, or set BACKEND_URL=http://localhost:8000 only when you intentionally run `mise run //frontend:dev` against an already reachable backend.",
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
