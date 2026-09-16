import { expect, test } from "@playwright/test";
import { getBackendUrl, waitForServers } from "./lib/client.ts";
import {
  installLongtaskObserver,
  logCeremonyStep,
  reportLongtasks,
} from "./lib/ceremony-log.ts";
import { startMockOidcServer } from "./lib/mock-oidc.ts";
import {
  describeFailure,
  openIsolatedPasskeyPage,
} from "./lib/security-context.ts";

type Member = {
  principal: {
    principal_id: string;
    display_name: string;
    state: string;
  };
  role: string;
};

async function members(
  request: import("@playwright/test").APIRequestContext,
  spaceId: string,
) {
  const response = await request.get(
    getBackendUrl(`/spaces/${spaceId}/members`),
  );
  expect(response.ok()).toBeTruthy();
  return await response.json() as Member[];
}

function invitationTokenFromUrl(invitationUrl: string): string {
  const token = new URL(invitationUrl).hash.match(/token=([^&]+)/)?.[1];
  if (!token) throw new Error("invitation URL carries no token fragment");
  return decodeURIComponent(token);
}

test.describe("Invitation OIDC primary journey", () => {
  // Ceremony-heavy group: stays serial even if the shared Playwright worker
  // count ever changes, on a fresh isolated context with its own virtual
  // authenticator (workers:1 in playwright.config.ts).
  test.describe.configure({ mode: "serial" });
  test.beforeAll(async ({ request }) => await waitForServers(request));

  test("invitation -> OIDC account create -> first Passkey -> logout/login keeps the same account", async ({ browser, request }) => {
    const ceremony = "oidc-invitation-journey";
    const slug = `e2e-oidc-journey-${Date.now()}`;
    const created = await request.post(getBackendUrl("/spaces"), {
      data: { slug, name: `OIDC journey ${slug}` },
    });
    expect(created.status()).toBe(201);
    const spaceId = ((await created.json()) as { id: string }).id;
    const invitation = await request.post(
      getBackendUrl(`/spaces/${spaceId}/members/invitations`),
      { data: { label: "OIDC journey", role: "viewer" } },
    );
    const invitationBody = await invitation.text();
    expect(invitation.status(), invitationBody).toBe(201);
    const invitationUrl =
      (JSON.parse(invitationBody) as { invitation_url: string }).invitation_url;
    expect(invitationTokenFromUrl(invitationUrl)).toBeTruthy();

    // The Rust mock issuer is in-process only (server unit tests), so the
    // browser journey uses the Deno mock issuer, which the server reaches
    // via E2E_OIDC_MOCK_HOST and the browser reaches directly.
    const mockOidc = await startMockOidcServer("oidc-journey-subject");
    const configuredProvider = await request.post(
      getBackendUrl("/auth/oidc/providers"),
      { data: { issuer: mockOidc.issuer, client_id: "e2e-client" } },
    );
    const configuredProviderBody = await configuredProvider.text();
    expect(configuredProvider.status(), configuredProviderBody).toBe(201);
    const providerId = (JSON.parse(configuredProviderBody) as {
      provider_id: string;
    }).provider_id;
    expect(providerId).toBeTruthy();

    const { target, page, close } = await openIsolatedPasskeyPage(browser);
    await installLongtaskObserver(page);
    try {
      logCeremonyStep(ceremony, "accepting invitation via OIDC");
      await page.goto(invitationUrl);
      const continueWithOidc = page.getByRole("button", {
        name: /Continue with/,
      });
      await expect(continueWithOidc).toBeVisible();
      // The OIDC start redirects through the mock issuer back to the
      // callback, which lands on first-Passkey bootstrap for the new
      // invitation-created account.
      await continueWithOidc.click();
      await expect(page).toHaveURL(/\/settings\/security\?bootstrap=1/, {
        timeout: 15_000,
      });

      logCeremonyStep(ceremony, "registering first passkey");
      const bootstrapFinish = page.waitForResponse(
        (response) =>
          response.url().endsWith("/api/auth/passkeys/bootstrap/finish") &&
          response.request().method() === "POST",
      );
      await page.getByRole("button", { name: "Register first Passkey" })
        .click();
      try {
        expect((await bootstrapFinish).status()).toBe(201);
      } catch (error) {
        throw describeFailure(error, "oidc bootstrap passkey");
      }
      await expect(
        page.getByRole("button", { name: "Register first Passkey" }),
      ).toBeHidden();

      const session = await target.request.get(
        getBackendUrl("/auth/session"),
      );
      expect(session.ok()).toBeTruthy();
      const firstAccountId =
        ((await session.json()).account.account_id) as string;
      expect(firstAccountId).toBeTruthy();
      const linksResponse = await target.request.get(
        getBackendUrl("/auth/oidc/links"),
      );
      expect(linksResponse.ok()).toBeTruthy();
      const links = await linksResponse.json() as Array<{ issuer: string }>;
      expect(links).toHaveLength(1);
      expect(links[0].issuer).toBe(mockOidc.issuer);
      const principalId = (await members(request, spaceId)).find((member) =>
        member.principal.display_name === "OIDC journey"
      )?.principal.principal_id;
      expect(principalId).toBeTruthy();

      logCeremonyStep(ceremony, "logout then passkey login");
      const signOut = await target.request.delete(
        getBackendUrl("/auth/session"),
      );
      expect(signOut.ok()).toBeTruthy();
      await target.clearCookies();
      await page.goto("/login");
      await page.getByRole("button", { name: "Sign in with a passkey" })
        .click();
      await expect(page).toHaveURL(/\/spaces$/);
      const secondSession = await target.request.get(
        getBackendUrl("/auth/session"),
      );
      expect(secondSession.ok()).toBeTruthy();
      expect((await secondSession.json()).account.account_id).toBe(
        firstAccountId,
      );
      expect(
        (await members(request, spaceId)).find((member) =>
          member.principal.principal_id === principalId
        )?.principal.display_name,
      ).toBe("OIDC journey");

      await reportLongtasks(page, ceremony, "oidc journey");
      logCeremonyStep(ceremony, "journey complete");
    } finally {
      await close();
      mockOidc.close();
    }
  });
});
