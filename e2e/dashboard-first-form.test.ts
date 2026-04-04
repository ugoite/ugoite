import { expect, test, type Page } from "@playwright/test";
import { getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client";

async function settleUiLoading(page: Page): Promise<void> {
	await page.waitForTimeout(150);
	await page
		.waitForFunction(() => !document.querySelector(".ui-loading-bar"), undefined, {
			timeout: 5_000,
		})
		.catch(() => undefined);
	await page.waitForTimeout(150);
}

test.describe("Dashboard first-form onboarding", () => {
	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
	});

	test("REQ-FE-037: dashboard guides first-run spaces toward form creation", async ({
		page,
		request,
	}) => {
		const spaceId = `dashboard-first-form-${Date.now()}`;
		const createSpace = await request.post(getBackendUrl("/spaces"), {
			data: { name: spaceId },
		});
		expect([200, 201, 409]).toContain(createSpace.status());

		await page.goto(getFrontendUrl(`/spaces/${spaceId}/dashboard`), {
			waitUntil: "domcontentloaded",
		});
		await expect(page.locator("body")).toBeVisible();
		await settleUiLoading(page);

		await expect(page.getByText("Start by creating your first form.")).toBeVisible();
		await expect(
			page.getByText(
				"Entries depend on form templates and fields. Create one form first, then come back to add entries.",
			),
		).toBeVisible();
		await expect(page.getByText("Recommended first step")).toBeVisible();

		await page.getByRole("button", { name: "Create your first form" }).click();

		await expect(page.getByRole("heading", { name: "Create New Form" })).toBeVisible({
			timeout: 10_000,
		});
	});
});
