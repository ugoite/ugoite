import { expect, test, type APIRequestContext, type Playwright } from "@playwright/test";
import { getBackendUrl, waitForServers } from "./lib/client";

const OWNER_TOKEN = "local-dev-token";
const ALICE_TOKEN = "alice-token";
const BOB_TOKEN = "bob-token";

async function authContext(
	playwright: Playwright,
	token: string,
): Promise<APIRequestContext> {
	return playwright.request.newContext({
		extraHTTPHeaders: {
			Authorization: `Bearer ${token}`,
		},
	});
}

async function createSpace(ownerRequest: APIRequestContext): Promise<string> {
	const response = await ownerRequest.post(getBackendUrl("/spaces"), {
		data: { name: `e2e-members-${Date.now()}-${Math.floor(Math.random() * 1000)}` },
	});
	expect(response.status()).toBe(201);
	const body = (await response.json()) as { id: string };
	return body.id;
}

test.describe("Space Membership", () => {
	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
	});

	test("REQ-SEC-007: invitation lifecycle invite/accept/revoke", async ({ playwright }) => {
		const owner = await authContext(playwright, OWNER_TOKEN);
		const alice = await authContext(playwright, ALICE_TOKEN);

		try {
			const spaceId = await createSpace(owner);

			const invite = await owner.post(
				getBackendUrl(`/spaces/${spaceId}/members/invitations`),
				{ data: { user_id: "alice-user", role: "viewer" } },
			);
			expect(invite.status()).toBe(201);
			const inviteBody = (await invite.json()) as {
				invitation: { token: string };
			};
			const token = inviteBody.invitation.token;

			const beforeAccept = await alice.get(getBackendUrl(`/spaces/${spaceId}`));
			expect(beforeAccept.status()).toBe(403);

			const accept = await alice.post(getBackendUrl(`/spaces/${spaceId}/members/accept`), {
				data: { token },
			});
			expect(accept.status()).toBe(200);

			const afterAccept = await alice.get(getBackendUrl(`/spaces/${spaceId}`));
			expect(afterAccept.status()).toBe(200);

			const revoke = await owner.delete(getBackendUrl(`/spaces/${spaceId}/members/alice-user`));
			expect(revoke.status()).toBe(200);

			const afterRevoke = await alice.get(getBackendUrl(`/spaces/${spaceId}`));
			expect(afterRevoke.status()).toBe(403);
		} finally {
			await owner.dispose();
			await alice.dispose();
		}
	});

	test("REQ-SEC-007: role changes switch admin privileges", async ({ playwright }) => {
		const owner = await authContext(playwright, OWNER_TOKEN);
		const bob = await authContext(playwright, BOB_TOKEN);

		try {
			const spaceId = await createSpace(owner);

			const invite = await owner.post(
				getBackendUrl(`/spaces/${spaceId}/members/invitations`),
				{ data: { user_id: "bob-user", role: "viewer" } },
			);
			expect(invite.status()).toBe(201);
			const inviteBody = (await invite.json()) as {
				invitation: { token: string };
			};
			const token = inviteBody.invitation.token;

			const accept = await bob.post(getBackendUrl(`/spaces/${spaceId}/members/accept`), {
				data: { token },
			});
			expect(accept.status()).toBe(200);

			const deniedPatch = await bob.patch(getBackendUrl(`/spaces/${spaceId}`), {
				data: { settings: { default_form: "Entry" } },
			});
			expect(deniedPatch.status()).toBe(403);

			const promote = await owner.post(
				getBackendUrl(`/spaces/${spaceId}/members/bob-user/role`),
				{ data: { role: "admin" } },
			);
			expect(promote.status()).toBe(200);

			const allowedPatch = await bob.patch(getBackendUrl(`/spaces/${spaceId}`), {
				data: { settings: { default_form: "Entry" } },
			});
			expect(allowedPatch.status()).toBe(200);
		} finally {
			await owner.dispose();
			await bob.dispose();
		}
	});

	test("REQ-SEC-007: members endpoint reflects lifecycle states", async ({ playwright }) => {
		const owner = await authContext(playwright, OWNER_TOKEN);
		const alice = await authContext(playwright, ALICE_TOKEN);

		try {
			const spaceId = await createSpace(owner);

			const invite = await owner.post(
				getBackendUrl(`/spaces/${spaceId}/members/invitations`),
				{ data: { user_id: "alice-user", role: "viewer" } },
			);
			expect(invite.status()).toBe(201);
			const inviteBody = (await invite.json()) as {
				invitation: { token: string };
			};

			const listedAfterInvite = await owner.get(getBackendUrl(`/spaces/${spaceId}/members`));
			expect(listedAfterInvite.status()).toBe(200);
			const invitedMembers = (await listedAfterInvite.json()) as Array<{ user_id: string; state: string }>;
			expect(invitedMembers.some((member) => member.user_id === "alice-user" && member.state === "invited")).toBeTruthy();

			const accept = await alice.post(getBackendUrl(`/spaces/${spaceId}/members/accept`), {
				data: { token: inviteBody.invitation.token },
			});
			expect(accept.status()).toBe(200);

			const listedAfterAccept = await owner.get(getBackendUrl(`/spaces/${spaceId}/members`));
			expect(listedAfterAccept.status()).toBe(200);
			const activeMembers = (await listedAfterAccept.json()) as Array<{ user_id: string; state: string }>;
			expect(activeMembers.some((member) => member.user_id === "alice-user" && member.state === "active")).toBeTruthy();
		} finally {
			await owner.dispose();
			await alice.dispose();
		}
	});

});
