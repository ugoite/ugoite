import { defineConfig } from "vitest/config";
import solidPlugin from "vite-plugin-solid";

export default defineConfig({
	plugins: [solidPlugin() as never],
	define: {
		"process.env.NODE_ENV": JSON.stringify("test"),
	},
	test: {
		environment: "jsdom",
		globals: true,
		setupFiles: ["./src/test/setup.ts"],
		include: ["src/**/*.{test,spec}.{js,ts,tsx}"],
		testTimeout: 10000,
		coverage: {
			provider: "v8",
			reporter: ["text", "json", "html"],
			exclude: [
				"src/test/**",
				"src/**/*.test.*",
				"src/**/*.spec.*",
				// Framework boilerplate - cannot be unit-tested in isolation
				"src/entry-client.tsx",
				"src/entry-server.tsx",
				"src/global.d.ts",
				"src/error-handler.ts",
				"src/app.tsx",
				// Route components - SolidStart SSR framework glue
				"src/routes/**",
				// Type-only and barrel re-export files
				"src/lib/types.ts",
				"src/lib/index.ts",
				"src/components/index.ts",
			],
			thresholds: {
				lines: 100,
				functions: 100,
				branches: 100,
				statements: 100,
			},
		},
	},
	resolve: {
		conditions: ["development", "browser"],
		alias: {
			"~": "/src",
		},
		dedupe: [
			"@codemirror/autocomplete",
			"@codemirror/lang-sql",
			"@codemirror/lint",
			"@codemirror/state",
			"@codemirror/view",
		],
	},
});
