import { expect, test, type Page } from "@playwright/test";
import { getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client.ts";

async function settleUiLoading(page: Page): Promise<void> {
	await page.waitForTimeout(150);
	await page
		.waitForFunction(
			() => !document.querySelector(".ui-loading-bar"),
			undefined,
			{
				timeout: 5_000,
			},
		)
		.catch(() => undefined);
	await page.waitForTimeout(150);
}

test.describe("Dashboard starter-entry onboarding", () => {
	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
	});

	test("REQ-FE-037: dashboard lets a new space create a first entry with the starter form", async ({
		page,
		request,
	}) => {
		const spaceId = `dashboard-first-form-${Date.now()}`;
		const createSpace = await request.post(getBackendUrl("/spaces"), {
			data: { slug: spaceId, name: "Dashboard first-entry test" },
		});
		expect([200, 201, 409]).toContain(createSpace.status());
		let createdSpaceId = "";
		if (createSpace.status() === 201 || createSpace.status() === 200) {
			createdSpaceId = ((await createSpace.json()) as { id: string }).id;
		} else {
			const spaces = await request.get(getBackendUrl("/spaces"));
			expect(spaces.ok()).toBeTruthy();
			const existing = (await spaces.json()) as Array<{ id: string; slug: string }>;
			createdSpaceId = existing.find((space) => space.slug === spaceId)?.id ?? "";
		}
		expect(createdSpaceId).not.toBe("");

		await page.goto(getFrontendUrl(`/spaces/${createdSpaceId}/dashboard`), {
			waitUntil: "domcontentloaded",
		});
		await expect(page.locator("body")).toBeVisible();
		await settleUiLoading(page);

		await expect(page.getByRole("button", { name: "New entry" })).toBeEnabled();
		await expect(
			page.getByText("Start by creating your first form."),
		).toHaveCount(0);

		await page.getByRole("button", { name: "New entry" }).click();

		await expect(
			page.getByRole("heading", { name: "Create New Entry" }),
		).toBeVisible({
			timeout: 10_000,
		});
		await expect(page.locator("#entry-form")).toHaveValue("Entry");
		await page.getByLabel("Title").fill("Starter quick note");
		await page.getByRole("button", { name: "Create" }).click();
		await page.waitForURL(new RegExp(`/spaces/${createdSpaceId}/entries/[^/]+$`), {
			timeout: 10_000,
		});
		await expect(
			page.getByRole("heading", { name: "Starter quick note", level: 1 }),
		).toBeVisible();
	});
});
