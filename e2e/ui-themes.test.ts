import { expect, test } from "@playwright/test";
import { ensureDefaultForm, getBackendUrl, waitForServers } from "./lib/client";

const spaceId = "default";
const themes = ["materialize", "classic", "pop"] as const;
const themeTestTitles: Record<(typeof themes)[number], string> = {
	materialize: "REQ-E2E-003: UI flows work in materialize theme",
	classic: "REQ-E2E-003: UI flows work in classic theme",
	pop: "REQ-E2E-003: UI flows work in pop theme",
};

test.describe("UI theme flows", () => {
	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
		await ensureDefaultForm(request);
	});

	for (const theme of themes) {
		test(themeTestTitles[theme], async ({ page, request }) => {
			const entryTitle = `E2E Theme Entry ${theme} ${Date.now()}`;
			const entryRes = await request.post(getBackendUrl(`/spaces/${spaceId}/entries`), {
				data: {
					content: `---\nform: Entry\n---\n# ${entryTitle}\n\n## Body\nTheme flow test.`,
				},
			});
			expect(entryRes.status()).toBe(201);
			const entry = (await entryRes.json()) as { id: string };

			const variableQueryId = `e2e-theme-var-${theme}-${Date.now()}`;
			const variableQueryName = `E2E Variables ${theme}`;
			const variableQueryRes = await request.post(getBackendUrl(`/spaces/${spaceId}/sql`), {
				data: {
					id: variableQueryId,
					name: variableQueryName,
					sql: "SELECT * FROM entries WHERE title = {{title}} LIMIT 10",
					variables: [
						{ type: "string", name: "title", description: "Title" },
					],
				},
			});
			expect([200, 201]).toContain(variableQueryRes.status());

			await page.goto(`/spaces/${spaceId}/dashboard`);
			await page.waitForLoadState("networkidle");

			const settingsButton = page.getByRole("button", { name: "Theme settings" });
			await settingsButton.click();
			await page.getByRole("radio", { name: new RegExp(`^${theme}$`, "i") }).click();
			await page.getByRole("radio", { name: "light" }).click();
			await page.keyboard.press("Escape");

			await expect(page.locator("html")).toHaveAttribute("data-ui-theme", theme);
			await expect(page.locator("html")).toHaveAttribute("data-color-mode", "light");

			await page.getByRole("link", { name: "search" }).click();
			await page.getByRole("button", { name: "Filter" }).click();
			await page.getByRole("heading", { name: "Filters" }).waitFor();
			await page.getByRole("button", { name: "Close" }).click();

			await page.getByRole("link", { name: "Create query" }).click();

			const queryName = `E2E Theme Query ${theme} ${Date.now()}`;
			await page.getByLabel("Query name").fill(queryName);
			await page.getByRole("button", { name: "Save" }).click();
			await page.getByRole("button", { name: variableQueryName }).waitFor();
			await page.getByRole("button", { name: variableQueryName }).click();
			await page.getByRole("heading", { name: "Query variables" }).waitFor();

			await page.getByLabel("title").fill(entryTitle);
			await page.getByRole("button", { name: "Run" }).click();
			await page.getByRole("heading", { name: /Query Results|Entries/ }).waitFor();

			await page.getByRole("link", { name: "grid" }).click();
			await page.getByRole("heading", { name: /Query Results|Form Grid/ }).waitFor();
			await page.getByPlaceholder("Global Search...").fill(entryTitle.slice(0, 6));
			await page.getByRole("button", { name: "Sort" }).click();
			await page.getByRole("combobox", { name: "Sort field" }).selectOption("updated_at");
			await page.getByRole("radio", { name: "Descending" }).click();
			await page.keyboard.press("Escape");
			await page.getByRole("button", { name: "Filter" }).click();

			await page.getByRole("link", { name: "object" }).click();
			await page.getByRole("heading", { name: /Query Results|Entries/ }).waitFor();
			await page.getByRole("button", { name: entryTitle }).waitFor();
			await page.getByRole("button", { name: entryTitle }).click();
			await page.getByRole("heading", { name: entryTitle, level: 2 }).waitFor();

			await settingsButton.click();
			await page.getByRole("radio", { name: "dark" }).click();
			await page.getByRole("link", { name: "Space settings" }).click();
			await page.getByRole("heading", { name: "Space Settings", level: 1 }).waitFor();

			await expect(page.locator("html")).toHaveAttribute("data-color-mode", "dark");

			await request.delete(getBackendUrl(`/spaces/${spaceId}/entries/${entry.id}`));
			await request.delete(getBackendUrl(`/spaces/${spaceId}/sql/${variableQueryId}`));

			const listRes = await request.get(getBackendUrl(`/spaces/${spaceId}/sql`));
			if (listRes.ok()) {
				const list = (await listRes.json()) as Array<{ id: string; name: string }>;
				const created = list.filter((item) => item.name.startsWith("E2E Theme Query"));
				for (const item of created) {
					await request.delete(getBackendUrl(`/spaces/${spaceId}/sql/${item.id}`));
				}
			}
		});
	}
});
