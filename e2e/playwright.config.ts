import { defineConfig, type ReporterDescription } from "@playwright/test";

const frontendUrl = process.env.FRONTEND_URL ?? "http://localhost:3000";
const e2eTestTimeoutEnv = process.env.E2E_TEST_TIMEOUT_MS;
const ciReporter = process.env.PLAYWRIGHT_CI_REPORTER;
const junitOutputFile = process.env.PLAYWRIGHT_JUNIT_OUTPUT_FILE ??
  "test-results/junit.xml";
const usesUgoiteAuthentication = Boolean(process.env.E2E_SETUP_SECRET?.trim());
const e2eTestTimeoutMs =
  e2eTestTimeoutEnv !== undefined && !Number.isNaN(Number(e2eTestTimeoutEnv))
    ? Number(e2eTestTimeoutEnv)
    : 60_000;

const reporter: "list" | ReporterDescription[] = ciReporter === "junit"
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
  globalSetup: usesUgoiteAuthentication ? "./global-setup.ts" : undefined,
  use: {
    baseURL: frontendUrl,
    storageState: usesUgoiteAuthentication ? ".auth/session.json" : undefined,
    // APIRequestContext does not synthesize a browser Origin header. Keep E2E
    // mutations subject to the same canonical-origin CSRF check as the UI.
    extraHTTPHeaders: { Origin: new URL(frontendUrl).origin },
    trace: "retain-on-failure",
  },
});
