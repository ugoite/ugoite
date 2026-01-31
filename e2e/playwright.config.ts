import { defineConfig } from "@playwright/test";

const frontendUrl = process.env.FRONTEND_URL ?? "http://localhost:3000";
const e2eTestTimeoutEnv = process.env.E2E_TEST_TIMEOUT_MS;
const e2eTestTimeoutMs =
	e2eTestTimeoutEnv !== undefined && !Number.isNaN(Number(e2eTestTimeoutEnv))
		? Number(e2eTestTimeoutEnv)
		: 60_000;

export default defineConfig({
	testDir: ".",
	testMatch: ["**/*.test.ts"],
	timeout: e2eTestTimeoutMs,
	fullyParallel: false,
	workers: 1,
	reporter: "list",
	use: {
		baseURL: frontendUrl,
		trace: "retain-on-failure",
	},
});
