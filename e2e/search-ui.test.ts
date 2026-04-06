import { expect, test, type APIRequestContext } from "@playwright/test";
import { ensureDefaultForm, getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client";

const spaceId = "default";

test.describe("Search UI", () => {
	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
		await ensureDefaultForm(request);
	});

	test("REQ-SRCH-004: search page starts with direct keyword search", async ({ page, request }) => {
		test.setTimeout(120_000);
		const runId = Date.now();
		const formName = `SearchUiForm${runId}`;
		const entryTitle = `Search UI Keyword ${runId}`;
		const entryContent = `---\nform: ${formName}\n---\n# ${entryTitle}\n\n## Owner Name\nalice\n\n## Body\nKeyword-first search should find this entry quickly.\n`;
		let entryId: string | null = null;

		try {
			await ensureSearchForm(request, formName);
			entryId = await createEntry(request, entryContent);
			await waitForKeywordMatch(request, "keyword-first", entryId);

			await page.goto(getFrontendUrl(`/spaces/${spaceId}/search`), {
				waitUntil: "domcontentloaded",
			});
			await page.getByRole("heading", { name: "Search", level: 1 }).waitFor();
			await expect(page.getByLabel("Search keywords")).toBeVisible();
			await page.getByLabel("Search keywords").fill("keyword-first");
			await page.getByRole("button", { name: "Search entries" }).click();
			await expect(page.getByRole("button", { name: new RegExp(entryTitle) })).toBeVisible();
		} finally {
			if (entryId) {
				await request.delete(getBackendUrl(`/spaces/${spaceId}/entries/${entryId}`));
			}
		}
	});

	test("REQ-SRCH-005: advanced search saves reusable history and opens shared query results", async ({
		page,
		request,
	}) => {
		test.setTimeout(120_000);
		const runId = Date.now();
		const shortRunId = String(runId).slice(-6);
		const today = new Date().toISOString().slice(0, 10);
		const formName = `SUA${shortRunId}`;
		const entryTitle = `Search UI Advanced ${runId}`;
		const historyPrefix = `Advanced search - form: ${formName}`;
		const historyName =
			`${historyPrefix} - tag: release - ` +
			`updated-from: ${today} - updated-to: ${today} - Owner Name=alice`;
		const entryContent = `---\nform: ${formName}\ntags:\n  - release\n  - search-ui\n---\n# ${entryTitle}\n\n## Owner Name\nalice\n\n## Body\nAdvanced search history should stay reusable.\n`;
		let entryId: string | null = null;

		await cleanupSavedSearchesByPrefix(request, historyPrefix);

		try {
			await ensureSearchForm(request, formName);
			entryId = await createEntry(request, entryContent);
			await waitForKeywordMatch(request, "Advanced search history should stay reusable.", entryId);

			await page.goto(getFrontendUrl(`/spaces/${spaceId}/search`), {
				waitUntil: "domcontentloaded",
			});
			await page.getByRole("button", { name: "Advanced search" }).click();
			await page.getByLabel("Form").selectOption(formName);
			await page.getByLabel("Tags (comma-separated)").fill("release");
			await page.getByLabel("Updated from").fill(today);
			await page.getByLabel("Updated to").fill(today);
			await page.getByLabel("Field").selectOption("Owner Name");
			await page.getByLabel("Value").fill("alice");
			await page.getByRole("button", { name: "Run advanced search" }).click();

			await expect(page).toHaveURL(/\/spaces\/default\/entries\?session=/);
			await expect(page.getByRole("button", { name: new RegExp(entryTitle) })).toBeVisible();

			await page.goto(getFrontendUrl(`/spaces/${spaceId}/search`), {
				waitUntil: "domcontentloaded",
			});
			await expect(page.getByRole("button", { name: new RegExp(historyName) })).toBeVisible();
			await page.getByRole("button", { name: new RegExp(historyName) }).click();
			await expect(page).toHaveURL(/\/spaces\/default\/entries\?session=/);
		} finally {
			if (entryId) {
				await request.delete(getBackendUrl(`/spaces/${spaceId}/entries/${entryId}`));
			}
			await cleanupSavedSearchesByPrefix(request, historyPrefix);
		}
	});
});

async function ensureSearchForm(request: APIRequestContext, formName: string): Promise<void> {
	const response = await request.post(getBackendUrl(`/spaces/${spaceId}/forms`), {
		data: {
			name: formName,
			version: 1,
			template: "# Search UI\n\n## Owner Name\n\n## Body\n",
			fields: {
				"Owner Name": { type: "string", required: false },
				Body: { type: "markdown", required: false },
			},
		},
	});
	if (![200, 201, 409].includes(response.status())) {
		throw new Error(`Failed to ensure search form: ${response.status()} ${await response.text()}`);
	}
}

async function createEntry(request: APIRequestContext, markdown: string): Promise<string> {
	const response = await request.post(getBackendUrl(`/spaces/${spaceId}/entries`), {
		data: { markdown },
	});
	expect(response.status()).toBe(201);
	const entry = (await response.json()) as { id: string };
	return entry.id;
}

async function waitForKeywordMatch(
	request: APIRequestContext,
	query: string,
	entryId: string,
): Promise<void> {
	await expect
		.poll(
			async () => {
				const response = await request.get(
					getBackendUrl(`/spaces/${spaceId}/search?q=${encodeURIComponent(query)}`),
				);
				if (!response.ok()) return false;
				const rows = (await response.json()) as Array<{ id?: string }>;
				return rows.some((row) => row.id === entryId);
			},
			{ timeout: 30_000 },
		)
		.toBe(true);
}

async function cleanupSavedSearchesByPrefix(
	request: APIRequestContext,
	namePrefix: string,
): Promise<void> {
	const response = await request.get(getBackendUrl(`/spaces/${spaceId}/sql`));
	if (!response.ok()) {
		return;
	}
	const list = (await response.json()) as Array<{ id: string; name: string }>;
	for (const entry of list.filter((item) => item.name.startsWith(namePrefix))) {
		await request.delete(getBackendUrl(`/spaces/${spaceId}/sql/${entry.id}`));
	}
}
