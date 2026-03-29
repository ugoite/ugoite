import { expect, test } from "@playwright/test";
import { getFrontendUrl, waitForServers } from "./lib/client";

test.describe("Space settings storage guidance", () => {
	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
	});

	test("REQ-FE-017: storage settings explain trade-offs and manual migration before switching backends", async ({
		page,
	}) => {
		const token = process.env.E2E_AUTH_BEARER_TOKEN;
		if (!token) {
			throw new Error("E2E_AUTH_BEARER_TOKEN is required");
		}
		await page.context().setExtraHTTPHeaders({
			Authorization: `Bearer ${token}`,
		});

		await page.goto(getFrontendUrl("/spaces/default/settings"), {
			waitUntil: "networkidle",
		});

		await expect(
			page.getByText(/local paths keep control and offline access on this machine/i),
		).toBeVisible();
		await expect(
			page.getByText(/does not migrate existing entries or assets to the new location/i),
		).toBeVisible();
		await expect(
			page.getByRole("link", { name: /storage migration guide/i }),
		).toBeVisible();
	});
});
