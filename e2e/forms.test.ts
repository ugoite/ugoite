import { expect, test } from "@playwright/test";
import { getBackendUrl, waitForServers } from "./lib/client";

const spaceId = "default";

test.describe("Form", () => {
	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
	});

	test("Create and List Forms", async ({ page, request }) => {
		const formName = `E2ETestForm-${Date.now()}`;
		const formDef = {
			name: formName,
			version: 1,
			template: "# E2ETestForm\n\n## Field1\n",
			fields: {
				Field1: { type: "string", required: true },
			},
		};

		const createRes = await request.post(
			getBackendUrl(`/spaces/${spaceId}/forms`),
			{ data: formDef },
		);
		expect([200, 201]).toContain(createRes.status());

		await page.goto(`/spaces/${spaceId}/forms`);
		const formSelect = page.getByRole("combobox");
		await expect(formSelect).toBeVisible({ timeout: 15000 });
		await formSelect.selectOption({ label: formName });
		await expect(page.getByRole("heading", { name: formName })).toBeVisible({ timeout: 15000 });
	});

	test("Query Entries by Form", async ({ page, request }) => {
		const formName = `QueryTestForm-${Date.now()}`;
		const formDef = {
			name: formName,
			version: 1,
			template: "# QueryTestForm\n\n## Status\n",
			fields: {
				Status: { type: "string", required: true },
			},
		};

		await request.post(
			getBackendUrl(`/spaces/${spaceId}/forms`),
			{ data: formDef },
		);

		const entryTitle = `Query Entry ${Date.now()}`;
		const entryContent = `---
form: ${formName}
---
# ${entryTitle}

## Status
Active
`;
		const entryRes = await request.post(
			getBackendUrl(`/spaces/${spaceId}/entries`),
			{ data: { content: entryContent } },
		);
		expect(entryRes.status()).toBe(201);
		const entry = (await entryRes.json()) as { id: string };

		await request.get(getBackendUrl(`/spaces/${spaceId}/search?q=Active`));

		await page.goto(`/spaces/${spaceId}/forms`, {
			waitUntil: "domcontentloaded",
		});
		const formSelect = page.getByRole("combobox");
		await expect(formSelect).toBeVisible({ timeout: 15000 });
		await formSelect.selectOption({ label: formName });
		await expect(page.getByRole("heading", { name: formName })).toBeVisible({ timeout: 15000 });
		await expect(page.getByText("Active")).toBeVisible({ timeout: 15000 });
		await expect(page.getByText(entryTitle)).toBeVisible({ timeout: 15000 });

		await request.delete(
			getBackendUrl(`/spaces/${spaceId}/entries/${entry.id}`),
		);
	});
});
