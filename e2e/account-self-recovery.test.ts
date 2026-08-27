import { expect, test } from "@playwright/test";
import { getBackendUrl, waitForServers } from "./lib/client.ts";
import { totpCodeAt } from "./lib/totp.ts";
import { addVirtualAuthenticator } from "./lib/webauthn.ts";

type RecoveryFixture = {
  accountId: string;
  recoveryCodes: string[];
  totpSecret: string;
};

test.describe("Account Self-Recovery", () => {
  test.beforeAll(async ({ request }) => await waitForServers(request));

  test("replaces the credential generation and preserves the HumanAccount", async ({ browser }) => {
    const fixture = JSON.parse(
      await Deno.readTextFile(".auth/account-recovery.json"),
    ) as RecoveryFixture;
    const target = await browser.newContext({
      storageState: { cookies: [], origins: [] },
    });
    const page = await target.newPage();
    const cdp = await target.newCDPSession(page);
    await cdp.send("WebAuthn.enable");
    await addVirtualAuthenticator(cdp);

    try {
      await page.goto(
        "/recover/account?next=" + encodeURIComponent("/spaces"),
      );
      await page.getByLabel("Account ID").fill(fixture.accountId);
      await page.getByLabel("Recovery Code").fill(fixture.recoveryCodes[0]);
      await page.getByLabel("Authenticator code").fill(
        await totpCodeAt(fixture.totpSecret),
      );
      const finishResponse = page.waitForResponse((response) =>
        response.url().endsWith("/api/auth/recovery/finish") &&
        response.request().method() === "POST"
      );
      await page.getByRole("button", { name: "Register new Passkey" }).click();
      expect((await finishResponse).status()).toBe(201);
      await expect(
        page.getByRole("heading", { name: "Save your new recovery codes" }),
      ).toBeVisible();
      const newCodes = await page.locator("ul.font-mono li").allTextContents();
      expect(newCodes).toHaveLength(8);
      expect(newCodes).not.toContain(fixture.recoveryCodes[0]);

      const session = await target.request.get(getBackendUrl("/auth/session"));
      expect(session.ok()).toBeTruthy();
      const sessionBody = await session.json() as {
        authenticated: boolean;
        account: { account_id: string };
      };
      expect(sessionBody.account.account_id).toBe(fixture.accountId);

      await page.getByRole("button", { name: "I saved the codes" }).click();
      await expect(page).toHaveURL(/\/spaces$/);

      const oldSessionContext = await browser.newContext({
        storageState: ".auth/session.json",
      });
      try {
        const oldSession = await oldSessionContext.request.get(
          getBackendUrl("/auth/session"),
        );
        expect(oldSession.ok()).toBeTruthy();
        await expect(oldSession.json()).resolves.toMatchObject({
          authenticated: false,
        });
      } finally {
        await oldSessionContext.close();
      }

      // The E2E suite intentionally shares one Node state. Preserve the
      // current generation for later tests while the old session above has
      // already proved stale.
      await target.storageState({ path: ".auth/session.json" });

      const oldCodeAttempt = await target.request.post(
        getBackendUrl("/auth/recovery/start"),
        {
          data: {
            account_id: fixture.accountId,
            recovery_code: fixture.recoveryCodes[0],
            totp_code: await totpCodeAt(fixture.totpSecret),
          },
        },
      );
      expect(oldCodeAttempt.status()).toBe(401);

      const newCodeAttempt = await target.request.post(
        getBackendUrl("/auth/recovery/start"),
        {
          data: {
            account_id: fixture.accountId,
            recovery_code: newCodes[0],
            totp_code: await totpCodeAt(fixture.totpSecret),
          },
        },
      );
      expect(newCodeAttempt.status()).toBe(200);

      const audit = await target.request.get(getBackendUrl("/auth/audit"));
      expect(audit.ok()).toBeTruthy();
      const auditBody = await audit.json() as Array<{
        action: string;
        subject_account_id: string | null;
        safe_metadata: Record<string, unknown>;
      }>;
      const recoveryEvent = auditBody.find((event) =>
        event.action === "account.recovered" &&
        event.subject_account_id === fixture.accountId
      );
      expect(recoveryEvent).toBeDefined();
      const auditText = JSON.stringify(recoveryEvent);
      expect(auditText).not.toContain(fixture.recoveryCodes[0]);
      expect(auditText).not.toContain(fixture.totpSecret);
      expect(recoveryEvent?.safe_metadata.method).toBe("recovery_code+totp");
    } finally {
      await target.close();
    }
  });
});
