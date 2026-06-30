import { chromium, expect, type FullConfig } from "@playwright/test";

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
  const firstAuthenticator = await cdp.send(
    "WebAuthn.addVirtualAuthenticator",
    {
      options: {
        protocol: "ctap2",
        transport: "internal",
        hasResidentKey: true,
        hasUserVerification: true,
        isUserVerified: true,
        automaticPresenceSimulation: true,
      },
    },
  );

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
    try {
      await displayName.waitFor({ state: "visible", timeout: 5_000 });
    } catch {
      throw new Error(
        `setup form missing at ${page.url()}; errors=${
          browserErrors.join(" | ")
        }; html=${(await page.content()).slice(0, 2000)}`,
      );
    }
    await displayName.fill("E2E owner");
    await page.getByRole("button", { name: "Create administrator passkey" })
      .click();
    await expect(page.getByText("Save these one-time recovery codes now."))
      .toBeVisible();
    await cdp.send("WebAuthn.removeVirtualAuthenticator", {
      authenticatorId: firstAuthenticator.authenticatorId,
    });
    await cdp.send("WebAuthn.addVirtualAuthenticator", {
      options: {
        protocol: "ctap2",
        transport: "internal",
        hasResidentKey: true,
        hasUserVerification: true,
        isUserVerified: true,
        automaticPresenceSimulation: true,
      },
    });
    await page.getByRole("button", { name: "Register second Passkey" }).click();
    const continueButton = page.getByRole("button", { name: "Continue" });
    try {
      await continueButton.waitFor({ state: "visible", timeout: 10_000 });
    } catch {
      throw new Error(
        `second Passkey setup failed; errors=${
          browserErrors.join(" | ")
        }; body=${(await page.locator("body").innerText()).slice(0, 2000)}`,
      );
    }
    await continueButton.click();
    await expect(page).toHaveURL(/\/spaces$/);

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
