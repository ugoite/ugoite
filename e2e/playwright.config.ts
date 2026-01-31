import { defineConfig } from "@playwright/test";

const frontendUrl = process.env.FRONTEND_URL ?? "http://localhost:3000";

export default defineConfig({
	testDir: ".",
	testMatch: ["**/*.test.ts"],
	timeout: 60_000,
	fullyParallel: false,
	workers: 1,
	reporter: "list",
	use: {
		baseURL: frontendUrl,
		trace: "retain-on-failure",
	},
});
