import { chromium, expect, type FullConfig } from "@playwright/test";
import {
  addVirtualAuthenticator,
  removeVirtualAuthenticator,
} from "./lib/webauthn.ts";
import { totpCodeAt } from "./lib/totp.ts";

const SETUP_TIMEOUT_MS = 30_000;

async function setupDiagnostics(
  page: import("@playwright/test").Page,
  browserErrors: string[],
): Promise<string> {
  const body = await page.locator("body").innerText().catch(() =>
    "<unavailable>"
  );
  const errors = browserErrors.length > 0 ? browserErrors.join(" | ") : "none";
  return `url=${page.url()}; browserErrors=${errors.slice(0, 2000)}; body=${
    body.slice(0, 2000)
  }`;
}

async function waitForSetupState(
  page: import("@playwright/test").Page,
  locator: import("@playwright/test").Locator,
  state: string,
  browserErrors: string[],
): Promise<void> {
  try {
    await expect(locator).toBeVisible({ timeout: SETUP_TIMEOUT_MS });
  } catch {
    throw new Error(
      `${state} did not become visible within ${SETUP_TIMEOUT_MS}ms; ${await setupDiagnostics(
        page,
        browserErrors,
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
  const context = await browser.newContext();
  const page = await context.newPage();
  const browserErrors: string[] = [];
  page.on("pageerror", (error) => browserErrors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error") browserErrors.push(message.text());
  });
  const cdp = await context.newCDPSession(page);
  await cdp.send("WebAuthn.enable");
  // REQ-SEC-004: the shipped browser gate uses a real WebAuthn ceremony.
  const firstAuthenticator = await addVirtualAuthenticator(cdp);

  try {
    const setupResponse = await page.goto(
      `${baseURL}/setup#secret=${encodeURIComponent(setupSecret)}`,
    );
    if (!setupResponse?.ok()) {
      throw new Error(
        `setup page returned ${setupResponse?.status()}: ${
          (await setupResponse?.text())?.slice(0, 2000)
        }`,
      );
    }
    const displayName = page.getByLabel("Display name");
    await waitForSetupState(page, displayName, "setup form", browserErrors);
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
        throw new Error(
          `setup finish returned ${setupResponse.status()}: ${
            (await setupResponse.text()).slice(0, 2000)
          }`,
        );
      }
    } catch (error) {
      throw new Error(
        `administrator passkey setup failed: ${
          error instanceof Error ? error.message : String(error)
        }; ${await setupDiagnostics(page, browserErrors)}`,
      );
    }
    await waitForSetupState(
      page,
      page.getByText("Save these bootstrap-only recovery codes now."),
      "recovery-code screen",
      browserErrors,
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
        throw new Error(
          `second Passkey finish returned ${passkeyResponse.status()}: ${
            (await passkeyResponse.text()).slice(0, 2000)
          }`,
        );
      }
    } catch (error) {
      throw new Error(
        `second Passkey setup failed: ${
          error instanceof Error ? error.message : String(error)
        }; ${await setupDiagnostics(page, browserErrors)}`,
      );
    }
    const continueButton = page.getByRole("button", { name: "Continue" });
    await waitForSetupState(
      page,
      continueButton,
      "second Passkey completion screen",
      browserErrors,
    );
    await continueButton.click();
    await expect(page).toHaveURL(/\/spaces$/);

    await page.goto(new URL("/settings/security", baseURL).toString());
    const setupRecoveryAuthenticator = page.getByRole("button", {
      name: "Set up or replace recovery authenticator",
    });
    await waitForSetupState(
      page,
      setupRecoveryAuthenticator,
      "account recovery settings",
      browserErrors,
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
