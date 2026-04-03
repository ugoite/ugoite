import { expect, test } from "@playwright/test";
import { getFrontendUrl, waitForServers } from "./lib/client";

const spaceId = "default";

test.describe("Saved SQL route", () => {
	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
	});

	test("REQ-FE-061: saved SQL route provides recovery links instead of a dead end", async ({
		page,
	}) => {
		await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql`), {
			waitUntil: "domcontentloaded",
		});

		await expect(page.getByRole("heading", { level: 1, name: "Saved SQL" })).toBeVisible();
		await expect(page.getByText(/named-query management/i)).toBeVisible();
		await expect(page.getByRole("link", { name: "Open Search" })).toHaveAttribute(
			"href",
			`/spaces/${spaceId}/search`,
		);
		await expect(page.getByRole("link", { name: "Back to Dashboard" })).toHaveAttribute(
			"href",
			`/spaces/${spaceId}/dashboard`,
		);
		await expect(page.getByRole("link", { name: "Browse Entries" })).toHaveAttribute(
			"href",
			`/spaces/${spaceId}/entries`,
		);
		await expect(page.getByText("Saved SQL management is not yet in the UI.")).toHaveCount(0);
	});
});
