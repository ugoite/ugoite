import { expect, test } from "@playwright/test";
import { getBackendUrl, waitForServers } from "./lib/client.ts";

test.describe("Space Membership", () => {
  test.beforeAll(async ({ request }) => await waitForServers(request));

  test("REQ-SEC-007: one-use invitation registers a real Passkey and can be revoked", async ({ browser, request }) => {
    const spaceId = `e2e-members-${Date.now()}`;
    const created = await request.post(getBackendUrl("/spaces"), {
      data: { name: spaceId },
    });
    expect(created.status()).toBe(201);
    const invite = await request.post(
      getBackendUrl(`/spaces/${spaceId}/members/invitations`),
      { data: { display_name: "Invited viewer", role: "viewer" } },
    );
    expect(invite.status()).toBe(201);
    const { invitation_url: invitationUrl } = await invite.json() as {
      invitation_url: string;
    };

    const invited = await browser.newContext({
      storageState: { cookies: [], origins: [] },
    });
    const page = await invited.newPage();
    const cdp = await invited.newCDPSession(page);
    await cdp.send("WebAuthn.enable");
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

    try {
      await page.goto(invitationUrl);
      await page.getByRole("button", { name: "Register Passkey and join" })
        .click();
      await expect(page).toHaveURL(/\/spaces$/);
      const space = await invited.request.get(
        getBackendUrl(`/spaces/${spaceId}`),
      );
      expect(space.status()).toBe(200);

      const members = await request.get(
        getBackendUrl(`/spaces/${spaceId}/members`),
      );
      const body = await members.json() as Array<{
        principal: {
          principal_id: string;
          display_name: string;
          state: string;
        };
        role: string;
      }>;
      const viewer = body.find((member) =>
        member.principal.display_name === "Invited viewer"
      );
      expect(viewer?.role).toBe("viewer");
      const revoked = await request.delete(
        getBackendUrl(
          `/spaces/${spaceId}/members/${viewer?.principal.principal_id}`,
        ),
      );
      expect(revoked.status()).toBe(200);
      const denied = await invited.request.get(
        getBackendUrl(`/spaces/${spaceId}`),
      );
      expect(denied.status()).toBe(403);
    } finally {
      await invited.close();
    }
  });
});
