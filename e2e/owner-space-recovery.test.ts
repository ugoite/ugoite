import { type Browser, expect, test } from "@playwright/test";
import { getBackendUrl, waitForServers } from "./lib/client.ts";
import { startMockOidcServer } from "./lib/mock-oidc.ts";
import { addVirtualAuthenticator } from "./lib/webauthn.ts";

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

test.describe("Owner-approved Space access recovery", () => {
  test.beforeAll(async ({ request }) => await waitForServers(request));

  // The supported journey below runs first on pristine backend state; the
  // path-separated probes after it passed in every position tried.
  // Journey-last ordering stalled its recovery step in local repetition
  // (server already returned 201 with valid codes); see #2583.
  async function createSpace(
    request: import("@playwright/test").APIRequestContext,
    slug: string,
  ) {
    const created = await request.post(getBackendUrl("/spaces"), {
      data: { slug, name: `Recovery ${slug}` },
    });
    expect(created.status()).toBe(201);
    return ((await created.json()) as { id: string }).id;
  }

  async function createInvitation(
    request: import("@playwright/test").APIRequestContext,
    spaceId: string,
    label: string,
  ) {
    const response = await request.post(
      getBackendUrl(`/spaces/${spaceId}/members/invitations`),
      { data: { label, role: "viewer" } },
    );
    const responseBody = await response.text();
    expect(response.status(), responseBody).toBe(201);
    return (JSON.parse(responseBody) as { invitation_url: string })
      .invitation_url;
  }

  // Each path-separated test below uses a fresh browser context with its own
  // virtual authenticator so recovery paths cannot leak credentials or
  // sessions into each other. The long supported journey keeps its flow
  // untouched.
  async function newPasskeyPage(browser: Browser) {
    const target = await browser.newContext({
      storageState: { cookies: [], origins: [] },
    });
    const page = await target.newPage();
    const cdp = await target.newCDPSession(page);
    await cdp.send("WebAuthn.enable");
    await addVirtualAuthenticator(cdp);
    return { target, page };
  }

  async function acceptInvitation(
    page: import("@playwright/test").Page,
    invitationUrl: string,
  ) {
    await page.goto(invitationUrl);
    await page.getByRole("button", { name: "Accept invitation" }).click();
    await expect(page).toHaveURL(/\/spaces$/);
    await expect(page).not.toHaveURL(/\/spaces\/join/);
  }

  test("req_sec_012_013_owner_space_access_recovery_supported_journey", async ({ browser, request }) => {
    const recoveredSpace = `e2e-recovery-${Date.now()}`;
    const unrelatedSpace = `e2e-recovery-other-${Date.now()}`;
    const spaceIds = new Map<string, string>();
    for (const spaceId of [recoveredSpace, unrelatedSpace]) {
      const created = await request.post(getBackendUrl("/spaces"), {
        data: { slug: spaceId, name: `Recovery ${spaceId}` },
      });
      expect(created.status()).toBe(201);
      spaceIds.set(spaceId, ((await created.json()) as { id: string }).id);
    }
    const recoveredSpaceId = spaceIds.get(recoveredSpace)!;
    const unrelatedSpaceId = spaceIds.get(unrelatedSpace)!;

    const invitations: string[] = [];
    for (const spaceId of [recoveredSpaceId, unrelatedSpaceId]) {
      const response = await request.post(
        getBackendUrl(`/spaces/${spaceId}/members/invitations`),
        { data: { label: "Recovery target", role: "viewer" } },
      );
      const responseBody = await response.text();
      expect(response.status(), responseBody).toBe(201);
      invitations.push(
        (JSON.parse(responseBody) as { invitation_url: string }).invitation_url,
      );
    }

    const mockOidc = await startMockOidcServer("space-recovery-subject");
    const target = await browser.newContext({
      storageState: { cookies: [], origins: [] },
    });
    const page = await target.newPage();
    const cdp = await target.newCDPSession(page);
    await cdp.send("WebAuthn.enable");
    const authenticatorId = await addVirtualAuthenticator(cdp);

    try {
      const configuredProvider = await request.post(
        getBackendUrl("/auth/oidc/providers"),
        { data: { issuer: mockOidc.issuer, client_id: "e2e-client" } },
      );
      const configuredProviderBody = await configuredProvider.text();
      expect(configuredProvider.status(), configuredProviderBody).toBe(201);
      const providerId = (JSON.parse(configuredProviderBody) as {
        provider_id: string;
      }).provider_id;

      // The first invitation creates the target's original HumanAccount and
      // binding; the second binds that same account to another Space.
      await page.goto(invitations[0]);
      await page.getByRole("button", { name: "Accept invitation" }).click();
      await expect(page).toHaveURL(/\/spaces$/);
      await page.goto(invitations[1]);
      await page.getByRole("button", { name: "Accept invitation" }).click();
      await expect(page).toHaveURL(/\/spaces$/);

      const linkResponse = await target.request.get(
        getBackendUrl(`/auth/oidc/${providerId}/link`),
      );
      expect(linkResponse.ok(), await linkResponse.text()).toBeTruthy();
      const beforeOidcLinksResponse = await target.request.get(
        getBackendUrl("/auth/oidc/links"),
      );
      expect(beforeOidcLinksResponse.ok()).toBeTruthy();
      const beforeOidcLinks = await beforeOidcLinksResponse.json() as Array<{
        method_id: string;
        issuer: string;
      }>;
      expect(beforeOidcLinks).toHaveLength(1);
      expect(beforeOidcLinks[0].issuer).toBe(mockOidc.issuer);

      const oldSession = await target.request.get(
        getBackendUrl("/auth/session"),
      );
      expect(oldSession.ok()).toBeTruthy();
      const oldAccountId =
        ((await oldSession.json()).account.account_id) as string;
      const beforeRecovered = await members(request, recoveredSpaceId);
      const beforeUnrelated = await members(request, unrelatedSpaceId);
      const targetMember = beforeRecovered.find((member) =>
        member.principal.display_name === "Recovery target"
      );
      expect(targetMember).toBeDefined();
      const principalId = targetMember!.principal.principal_id;

      const createdEntry = await request.post(
        getBackendUrl(`/spaces/${recoveredSpaceId}/entries`),
        { data: { markdown: "---\nform: Entry\n---\n# Recovery ACL\n" } },
      );
      expect(createdEntry.status()).toBe(201);
      const entryId = ((await createdEntry.json()) as { id: string }).id;
      const policy = {
        policy_id: crypto.randomUUID(),
        inherit_space_role: true,
        grants: [{ principal_id: principalId, actions: ["read", "update"] }],
      };
      const policyUpdate = await request.put(
        getBackendUrl(
          `/spaces/${recoveredSpaceId}/policies/entry/${entryId}`,
        ),
        { data: policy },
      );
      expect(policyUpdate.status()).toBe(200);
      const beforePolicy = await request.get(
        getBackendUrl(`/spaces/${recoveredSpaceId}/policies/entry/${entryId}`),
      );
      expect(beforePolicy.ok()).toBeTruthy();
      const policySnapshot = await beforePolicy.json();

      const oldCookie = (await target.cookies()).find((cookie) =>
        cookie.name === "ugoite_session"
      );
      expect(oldCookie?.value).toBeTruthy();
      const originalCredentialId = ((await cdp.send("WebAuthn.getCredentials", {
        authenticatorId,
      })).credentials as Array<{ credentialId: string }>)[0]?.credentialId;
      expect(originalCredentialId).toBeTruthy();

      const approval = await request.post(
        getBackendUrl(`/spaces/${recoveredSpaceId}/admin/recovery/force-reset`),
        { data: { principal_id: principalId } },
      );
      expect(approval.status()).toBe(201);
      const approvalBody = await approval.json() as {
        owner_approval_token: string;
      };

      await page.goto(
        `/recover?owner_approval_token=${
          encodeURIComponent(
            approvalBody.owner_approval_token,
          )
        }`,
      );
      await page.getByRole("button", { name: "Continue" }).click();
      await expect(
        page.getByRole("heading", { name: "Save your new recovery codes" }),
      ).toBeVisible();

      const freshSession = await target.request.get(
        getBackendUrl("/auth/session"),
      );
      const freshAccountId =
        ((await freshSession.json()).account.account_id) as string;
      expect(freshAccountId).not.toBe(oldAccountId);
      const freshOidcLinks = await target.request.get(
        getBackendUrl("/auth/oidc/links"),
      );
      expect(freshOidcLinks.ok()).toBeTruthy();
      expect(await freshOidcLinks.json()).toEqual([]);
      expect(
        (await target.request.get(getBackendUrl(`/spaces/${recoveredSpaceId}`)))
          .status(),
      )
        .toBe(200);
      expect(
        (await target.request.get(
          getBackendUrl(`/spaces/${recoveredSpaceId}/entries/${entryId}`),
        )).status(),
      ).toBe(200);
      expect([401, 403]).toContain(
        (await target.request.get(getBackendUrl(`/spaces/${unrelatedSpaceId}`)))
          .status(),
      );

      const afterRecovered = await members(request, recoveredSpaceId);
      const afterUnrelated = await members(request, unrelatedSpaceId);
      expect(afterRecovered).toEqual(beforeRecovered);
      expect(afterUnrelated).toEqual(beforeUnrelated);
      const afterPolicy = await request.get(
        getBackendUrl(`/spaces/${recoveredSpaceId}/policies/entry/${entryId}`),
      );
      expect(await afterPolicy.json()).toEqual(policySnapshot);
      expect(
        afterRecovered.find((member) =>
          member.principal.principal_id === principalId
        )?.role,
      ).toBe(targetMember!.role);

      // The old session and its unrelated Space binding survive, while the
      // recovered Space binding is intentionally no longer available to it.
      await target.clearCookies();
      await target.addCookies([oldCookie!]);
      const afterOidcLinksResponse = await target.request.get(
        getBackendUrl("/auth/oidc/links"),
      );
      expect(afterOidcLinksResponse.ok()).toBeTruthy();
      expect(await afterOidcLinksResponse.json()).toEqual(beforeOidcLinks);
      await expect(
        (await target.request.get(getBackendUrl("/auth/session"))).json(),
      ).resolves.toMatchObject({
        authenticated: true,
        account: { account_id: oldAccountId },
      });
      expect([401, 403]).toContain(
        (await target.request.get(getBackendUrl(`/spaces/${recoveredSpaceId}`)))
          .status(),
      );
      expect([401, 403]).toContain(
        (await target.request.get(
          getBackendUrl(`/spaces/${recoveredSpaceId}/entries/${entryId}`),
        )).status(),
      );
      expect(
        (await target.request.get(getBackendUrl(`/spaces/${unrelatedSpaceId}`)))
          .status(),
      )
        .toBe(200);

      const audit = await request.get(
        getBackendUrl(
          `/spaces/${recoveredSpaceId}/audit?action=recovery.space_binding_replaced`,
        ),
      );
      expect(audit.ok()).toBeTruthy();
      const auditBody = await audit.json() as {
        total: number;
        items: Array<Record<string, unknown>>;
      };
      expect(auditBody.total).toBe(1);
      const serializedAudit = JSON.stringify(auditBody.items[0]);
      expect(serializedAudit).not.toContain("recovery_codes");
      expect(serializedAudit).not.toContain("attestationObject");

      // Registration created two credentials on the virtual authenticator;
      // keep only the original one to verify the old HumanAccount remains
      // authentically usable after its Space binding moved.
      const credentials = (await cdp.send("WebAuthn.getCredentials", {
        authenticatorId,
      })).credentials as Array<{ credentialId: string }>;
      for (const credential of credentials) {
        if (credential.credentialId === originalCredentialId) continue;
        await cdp.send("WebAuthn.removeCredential", {
          authenticatorId,
          credentialId: credential.credentialId,
        });
      }
      await target.clearCookies();
      await page.goto("/login");
      await page.getByRole("button", { name: "Sign in with a passkey" })
        .click();
      await expect(page).toHaveURL(/\/spaces$/);
      const passkeySession = await target.request.get(
        getBackendUrl("/auth/session"),
      );
      expect((await passkeySession.json()).account.account_id).toBe(
        oldAccountId,
      );
    } finally {
      await target.close();
      mockOidc.close();
    }
  });

  test("invitation accept creates an account and completes to the Space list", async ({ browser, request }) => {
    const spaceId = await createSpace(
      request,
      `e2e-invite-new-${Date.now()}`,
    );
    const invitationUrl = await createInvitation(
      request,
      spaceId,
      "Accept target",
    );
    const { target, page } = await newPasskeyPage(browser);
    try {
      await acceptInvitation(page, invitationUrl);
      const list = await members(request, spaceId);
      expect(
        list.some((member) =>
          member.principal.display_name === "Accept target"
        ),
      ).toBe(true);
    } finally {
      await target.close();
    }
  });

  test("invitation accept binds an existing account and completes to the Space list", async ({ browser, request }) => {
    const firstSpaceId = await createSpace(
      request,
      `e2e-invite-first-${Date.now()}`,
    );
    const secondSpaceId = await createSpace(
      request,
      `e2e-invite-second-${Date.now()}`,
    );
    const firstUrl = await createInvitation(
      request,
      firstSpaceId,
      "Second target",
    );
    const secondUrl = await createInvitation(
      request,
      secondSpaceId,
      "Second target",
    );
    const { target, page } = await newPasskeyPage(browser);
    try {
      await acceptInvitation(page, firstUrl);
      await acceptInvitation(page, secondUrl);
      for (const spaceId of [firstSpaceId, secondSpaceId]) {
        const list = await members(request, spaceId);
        expect(
          list.some((member) =>
            member.principal.display_name === "Second target"
          ),
        ).toBe(true);
      }
    } finally {
      await target.close();
    }
  });

  test("unknown invitation shows a reason and stays on join", async ({ browser }) => {
    const { target, page } = await newPasskeyPage(browser);
    try {
      await page.goto("/spaces/join#token=not-a-real-invitation");
      await page.getByRole("button", { name: "Accept invitation" }).click();
      await expect(page.getByRole("alert")).toContainText("was not found");
      await expect(page.getByRole("alert")).toContainText(
        "Check the invitation link",
      );
      await expect(page).toHaveURL(/\/spaces\/join/);
    } finally {
      await target.close();
    }
  });

  test("consumed invitation shows the reuse reason to a new visitor", async ({ browser, request }) => {
    const spaceId = await createSpace(
      request,
      `e2e-invite-reuse-${Date.now()}`,
    );
    const primerSpaceId = await createSpace(
      request,
      `e2e-invite-reuse-primer-${Date.now()}`,
    );
    const primerUrl = await createInvitation(
      request,
      primerSpaceId,
      "Reuse primer",
    );
    const invitationUrl = await createInvitation(
      request,
      spaceId,
      "Reuse target",
    );
    const first = await newPasskeyPage(browser);
    try {
      // The first visitor creates an account, then consumes the target
      // invitation as an existing account.
      await acceptInvitation(first.page, primerUrl);
      await acceptInvitation(first.page, invitationUrl);
    } finally {
      await first.target.close();
    }
    const before = await members(request, spaceId);
    const second = await newPasskeyPage(browser);
    try {
      await second.page.goto(invitationUrl);
      await second.page.getByRole("button", { name: "Accept invitation" })
        .click();
      await expect(second.page.getByRole("alert")).toContainText(
        "no longer pending",
      );
      await expect(
        second.page.getByRole("link", { name: "Go to Spaces" }),
      ).toBeVisible();
      await expect(second.page).toHaveURL(/\/spaces\/join/);
      expect(await members(request, spaceId)).toEqual(before);
    } finally {
      await second.target.close();
    }
  });

  test("double activation is safe and completes once", async ({ browser, request }) => {
    const spaceId = await createSpace(
      request,
      `e2e-invite-double-${Date.now()}`,
    );
    const invitationUrl = await createInvitation(
      request,
      spaceId,
      "Double target",
    );
    const { target, page } = await newPasskeyPage(browser);
    try {
      await page.goto(invitationUrl);
      await page.getByRole("button", { name: "Accept invitation" }).dblclick();
      await expect(page).toHaveURL(/\/spaces$/);
      const list = await members(request, spaceId);
      expect(
        list.filter((member) =>
          member.principal.display_name === "Double target"
        ),
      ).toHaveLength(1);
    } finally {
      await target.close();
    }
  });

  test("reopening a consumed invitation after success lands on Spaces", async ({ browser, request }) => {
    const spaceId = await createSpace(
      request,
      `e2e-invite-revisit-${Date.now()}`,
    );
    const invitationUrl = await createInvitation(
      request,
      spaceId,
      "Revisit target",
    );
    const { target, page } = await newPasskeyPage(browser);
    try {
      await acceptInvitation(page, invitationUrl);
      await acceptInvitation(page, invitationUrl);
    } finally {
      await target.close();
    }
  });
});
