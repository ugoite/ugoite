import { chromium, expect, type FullConfig } from "@playwright/test";
import {
  gotoPageWithOneEnvironmentRetry,
  gotoWithOneEnvironmentRetry,
} from "./lib/navigation-retry.ts";
import {
  addVirtualAuthenticator,
  removeVirtualAuthenticator,
} from "./lib/webauthn.ts";
import { totpCodeAt } from "./lib/totp.ts";
import {
  createSetupReadinessDiagnostics,
  formatSetupReadinessDiagnostics,
  observeSetupReadiness,
  snapshotSetupDom,
  type SetupReadinessDiagnostics,
} from "./lib/readiness-diagnostics.ts";
import { safeNavigationFailure } from "./lib/security-context.ts";

const SETUP_TIMEOUT_MS = 30_000;

async function assertBuildProvenance(
  page: import("@playwright/test").Page,
  frontendUrl: string,
): Promise<void> {
  const expectedSourceSha = process.env.UGOITE_SOURCE_SHA?.trim();
  if (!expectedSourceSha || !/^[0-9a-f]{40}$/.test(expectedSourceSha)) {
    throw new Error("UGOITE_SOURCE_SHA must identify the checkout under test");
  }

  const backendUrl = (process.env.BACKEND_URL?.trim() || frontendUrl).replace(
    /\/$/,
    "",
  );
  const health = await page.request.get(`${backendUrl}/health`);
  expect(health.ok()).toBeTruthy();
  expect(health.headers()["x-ugoite-source-sha"]).toBe(expectedSourceSha);

  const buildInfo = await page.request.get(
    new URL("/build-info.json", frontendUrl).toString(),
  );
  expect(buildInfo.ok()).toBeTruthy();
  expect(await buildInfo.json()).toMatchObject({
    schema_version: 1,
    source_sha: expectedSourceSha,
  });
}

async function setupDiagnostics(
  page: import("@playwright/test").Page,
  diagnostics: SetupReadinessDiagnostics,
): Promise<string> {
  return formatSetupReadinessDiagnostics(
    page.url(),
    diagnostics,
    await snapshotSetupDom(page),
  );
}

async function waitForSetupState(
  page: import("@playwright/test").Page,
  locator: import("@playwright/test").Locator,
  state: string,
  diagnostics: SetupReadinessDiagnostics,
): Promise<void> {
  try {
    await expect(locator).toBeVisible({ timeout: SETUP_TIMEOUT_MS });
  } catch {
    throw new Error(
      `${state} did not become visible within ${SETUP_TIMEOUT_MS}ms; ${await setupDiagnostics(
        page,
        diagnostics,
      )}`,
    );
  }
}

function isPostResponse(path: string) {
  return (response: import("@playwright/test").Response): boolean => {
    const url = new URL(response.url());
    return response.request().method() === "POST" && url.pathname === path;
  };
}

export default async function globalSetup(config: FullConfig): Promise<void> {
  const setupSecret = process.env.E2E_SETUP_SECRET?.trim();
  if (!setupSecret) throw new Error("E2E_SETUP_SECRET is required");
  const baseURL = config.projects[0]?.use.baseURL as string | undefined;
  if (!baseURL) throw new Error("Playwright baseURL is required");

  const browser = await chromium.launch();
  // The initial setup navigation tolerates exactly one browser-level
  // environment failure (ERR_NETWORK_CHANGED and kin) via a single context
  // rebuild. Non-OK responses and every later ceremony step still throw on
  // the first attempt: product verdicts are never retried.
  const setupUrl = `${baseURL}/setup#secret=${encodeURIComponent(setupSecret)}`;
  const readinessDiagnostics = createSetupReadinessDiagnostics();
  let context: import("@playwright/test").BrowserContext;
  let page: import("@playwright/test").Page;
  try {
    const navigation = await gotoWithOneEnvironmentRetry(browser, setupUrl, {
      label: "global setup",
      requireOkResponse: true,
      prepare: async (page) =>
        observeSetupReadiness(page, readinessDiagnostics),
    });
    context = navigation.target;
    page = navigation.page;
  } catch (error) {
    throw new Error(
      `setup navigation failed (${safeNavigationFailure(error)}); ${formatSetupReadinessDiagnostics(
        setupUrl,
        readinessDiagnostics,
      )}`,
    );
  }
  const cdp = await context.newCDPSession(page);
  await cdp.send("WebAuthn.enable");
  // REQ-SEC-004: the shipped browser gate uses a real WebAuthn ceremony.
  const firstAuthenticator = await addVirtualAuthenticator(cdp);

  try {
    await assertBuildProvenance(page, baseURL);
    const displayName = page.getByLabel("Display name");
    await waitForSetupState(
      page,
      displayName,
      "setup form",
      readinessDiagnostics,
    );
    await displayName.fill("E2E owner");
    const createAdministratorPasskey = page.getByRole("button", {
      name: "Create administrator passkey",
    });
    try {
      const [setupResponse] = await Promise.all([
        page.waitForResponse(isPostResponse("/api/auth/setup/finish"), {
          timeout: SETUP_TIMEOUT_MS,
        }),
        createAdministratorPasskey.click(),
      ]);
      if (!setupResponse.ok()) {
        throw new Error(`setup finish returned ${setupResponse.status()}`);
      }
    } catch (error) {
      throw new Error(
        `administrator passkey setup failed (${safeNavigationFailure(error)}); ${
          await setupDiagnostics(page, readinessDiagnostics)
        }`,
      );
    }
    await waitForSetupState(
      page,
      page.getByText("Save these bootstrap-only recovery codes now."),
      "recovery-code screen",
      readinessDiagnostics,
    );
    const accountId = await page.getByTestId("recovery-account-id").innerText();
    const recoveryCodes =
      (await page.getByTestId("bootstrap-recovery-codes").innerText())
        .split(/\s+/)
        .filter(Boolean);
    await removeVirtualAuthenticator(cdp, firstAuthenticator);
    await addVirtualAuthenticator(cdp);
    const registerSecondPasskey = page.getByRole("button", {
      name: "Register second Passkey",
    });
    try {
      const [passkeyResponse] = await Promise.all([
        page.waitForResponse(isPostResponse("/api/auth/passkeys/finish"), {
          timeout: SETUP_TIMEOUT_MS,
        }),
        registerSecondPasskey.click(),
      ]);
      if (!passkeyResponse.ok()) {
        throw new Error(`second Passkey finish returned ${passkeyResponse.status()}`);
      }
    } catch (error) {
      throw new Error(
        `second Passkey setup failed (${safeNavigationFailure(error)}); ${
          await setupDiagnostics(page, readinessDiagnostics)
        }`,
      );
    }
    const continueButton = page.getByRole("button", { name: "Continue" });
    await waitForSetupState(
      page,
      continueButton,
      "second Passkey completion screen",
      readinessDiagnostics,
    );
    await continueButton.click();
    await expect(page).toHaveURL(/\/spaces$/);

    // Same environment-aware policy as the initial setup navigation: exactly
    // one retry on browser-level failure (ERR_NETWORK_CHANGED and kin).
    // Product verdicts (4xx/5xx, validation, ceremony failures) never retry.
    // A context rebuild would drop the fresh session, so the retry stays on
    // the same page.
    const setupRecoveryAuthenticator = page.getByRole("button", {
      name: "Set up or replace recovery authenticator",
    });
    await gotoPageWithOneEnvironmentRetry(
      page,
      new URL("/settings/security", baseURL).toString(),
      {
        label: "global setup /settings/security",
        waitForReady: () =>
          waitForSetupState(
            page,
            setupRecoveryAuthenticator,
            "account recovery settings",
            readinessDiagnostics,
          ),
      },
    );
    await setupRecoveryAuthenticator.click();
    const recoverySecret = await page.getByTestId("recovery-secret")
      .innerText();
    await page.getByLabel("Current six-digit code").fill(
      await totpCodeAt(recoverySecret),
    );
    await page.getByRole("button", { name: "Confirm TOTP" }).click();
    await expect(page.getByRole("status")).toHaveText(
      "Recovery TOTP configured.",
    );
    await Deno.mkdir(".auth", { recursive: true });
    await Deno.writeTextFile(
      ".auth/account-recovery.json",
      JSON.stringify({ accountId, recoveryCodes, totpSecret: recoverySecret }),
    );

    const signOut = await page.request.delete(`${baseURL}/api/auth/session`, {
      headers: { Origin: new URL(baseURL).origin },
    });
    expect(signOut.ok()).toBeTruthy();
    await context.clearCookies();
    await page.goto(`${baseURL}/login`);
    await page.getByRole("button", { name: "Sign in with a passkey" }).click();
    await expect(page).toHaveURL(/\/spaces$/);
    await context.storageState({ path: ".auth/session.json" });
  } finally {
    await browser.close();
  }
}
