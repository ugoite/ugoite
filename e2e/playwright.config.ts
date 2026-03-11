import { defineConfig } from "@playwright/test";

const frontendUrl = process.env.FRONTEND_URL ?? "http://localhost:3000";
const frontendOrigin = new URL(frontendUrl);
const e2eTestTimeoutEnv = process.env.E2E_TEST_TIMEOUT_MS;
const ciReporter = process.env.PLAYWRIGHT_CI_REPORTER;
const junitOutputFile = process.env.PLAYWRIGHT_JUNIT_OUTPUT_FILE ?? "test-results/junit.xml";
const e2eAuthBearerToken = process.env.E2E_AUTH_BEARER_TOKEN;
const e2eCookieExpires = Math.floor(Date.now() / 1000) + 43_200;
if (!e2eAuthBearerToken || !e2eAuthBearerToken.trim()) {
	throw new Error("E2E_AUTH_BEARER_TOKEN is required");
}
const e2eTestTimeoutMs =
	e2eTestTimeoutEnv !== undefined && !Number.isNaN(Number(e2eTestTimeoutEnv))
		? Number(e2eTestTimeoutEnv)
		: 60_000;

const reporter =
	ciReporter === "junit"
		? [["list"], ["junit", { outputFile: junitOutputFile }]]
		: "list";

export default defineConfig({
	testDir: ".",
	testMatch: ["**/*.test.ts"],
	timeout: e2eTestTimeoutMs,
	// E2E tests share backend state; run serially to avoid cross-test interference.
	fullyParallel: false,
	workers: 1,
	reporter,
	use: {
		baseURL: frontendUrl,
		extraHTTPHeaders: {
			Authorization: `Bearer ${e2eAuthBearerToken}`,
		},
		storageState: {
			cookies: [
				{
					name: "ugoite_auth_bearer_token",
					value: e2eAuthBearerToken,
					domain: frontendOrigin.hostname,
					path: "/",
					expires: e2eCookieExpires,
					httpOnly: false,
					secure: frontendOrigin.protocol === "https:",
					sameSite: "Lax",
				},
			],
			origins: [],
		},
		trace: "retain-on-failure",
	},
});
