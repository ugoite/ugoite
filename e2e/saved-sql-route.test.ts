import { expect, test } from "@playwright/test";
import { getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client";

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

	test("REQ-FE-062: saved SQL detail loads query text and routes variable-free runs to entries", async ({
		page,
		request,
	}) => {
		const sqlCreate = await request.post(getBackendUrl(`/spaces/${spaceId}/sql`), {
			data: {
				name: `Saved Detail Query ${Date.now()}`,
				sql: "SELECT * FROM entries LIMIT 1",
				variables: [],
			},
		});
		expect([200, 201]).toContain(sqlCreate.status());
		const savedSql = (await sqlCreate.json()) as { id: string };

		try {
			await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${savedSql.id}`), {
				waitUntil: "domcontentloaded",
			});

			await expect(page.getByRole("heading", { level: 1, name: /Saved Detail Query/ })).toBeVisible();
			await expect(page.locator(".ui-sql-editor")).toContainText("SELECT * FROM entries LIMIT 1");
			await page.getByRole("button", { name: "Run Query" }).click();
			await expect(page).toHaveURL(new RegExp(`/spaces/${spaceId}/entries\\?session=`));
		} finally {
			await request.delete(getBackendUrl(`/spaces/${spaceId}/sql/${savedSql.id}`));
		}
	});
});
