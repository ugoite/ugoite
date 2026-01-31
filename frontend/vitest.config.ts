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
			include: ["src/**/*.{ts,tsx}"],
			exclude: ["src/test/**", "src/**/*.test.*", "src/**/*.spec.*"],
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
