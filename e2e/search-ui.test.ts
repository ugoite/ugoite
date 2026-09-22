import { expect, test, type APIRequestContext } from "@playwright/test";
import {
	ensureDefaultForm,
	getBackendUrl,
	getDefaultSpaceId,
	getFrontendUrl,
	waitForServers,
} from "./lib/client.ts";
import { expectNoObjectCoercion } from "./lib/ui-safety.ts";

test.describe("Search UI", () => {
	let spaceId = "";

	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
		spaceId = await getDefaultSpaceId(request);
		await ensureDefaultForm(request, spaceId);
	});

	test("REQ-SRCH-006: search navigation exposes the four primary destinations", async ({ page }) => {
		await page.goto(getFrontendUrl(`/spaces/${spaceId}/search`), {
			waitUntil: "domcontentloaded",
		});

		const navigation = page.getByRole("navigation", { name: "Search" });
		await expect(navigation).toBeVisible();
		await expect(page.getByLabel("Search keywords")).toBeVisible();
		await expect(page.getByRole("button", { name: "Search entries" }))
			.toBeVisible();
		await expect(navigation.getByRole("link", { name: "Files" }))
			.toHaveAttribute("href", `/spaces/${spaceId}/assets`);
		await expect(navigation.getByRole("link", { name: "Saved" }))
			.toHaveAttribute("href", `/spaces/${spaceId}/sql`);
		await expect(navigation.getByRole("link", { name: "Open SQL editor" }))
			.not.toBeAttached();
		await expect(page.getByRole("heading", { name: "Search history" }))
			.not.toBeAttached();
		await expectNoObjectCoercion(page);
	});

	test("REQ-SRCH-004: search page starts with direct keyword search", async ({ page, request }) => {
		test.setTimeout(120_000);
		const runId = Date.now();
		const formName = `SearchUiForm${runId}`;
		let entryId: string | null = null;

		try {
			await ensureSearchForm(request, formName, spaceId);
			entryId = await createEntry(request, spaceId, {
				form: formName,
				fields: {
					"Owner Name": "alice",
					Body: "Keyword-first search should find this entry quickly.",
				},
			});
			await waitForKeywordMatch(request, "keyword-first", entryId, spaceId);

			await page.goto(getFrontendUrl(`/spaces/${spaceId}/search`), {
				waitUntil: "domcontentloaded",
			});
			await page.getByRole("navigation", { name: "Search" }).waitFor();
			await expect(page.getByLabel("Search keywords")).toBeVisible();
			await page.getByLabel("Search keywords").fill("keyword-first");
			await page.getByRole("button", { name: "Search entries" }).click();
			await expect(page.locator(".entry-browser-table").getByRole("button").first()).toBeVisible();
			await expect(
				page.locator(".entry-browser-table").getByText("alice", { exact: false }).first(),
			).toBeVisible();
			await expectNoObjectCoercion(page);
		} finally {
			if (entryId) {
				await request.delete(getBackendUrl(`/spaces/${spaceId}/entries/${entryId}`));
			}
		}
	});

});

async function ensureSearchForm(
	request: APIRequestContext,
	formName: string,
	spaceId: string,
): Promise<void> {
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

async function createEntry(
	request: APIRequestContext,
	spaceId: string,
	payload: {
		form: string;
		tags?: string[];
		fields: Record<string, unknown>;
	},
): Promise<string> {
	const response = await request.post(getBackendUrl(`/spaces/${spaceId}/entries`), {
		data: payload,
	});
	expect(response.status()).toBe(201);
	const entry = (await response.json()) as { id: string };
	return entry.id;
}


async function waitForKeywordMatch(
	request: APIRequestContext,
	query: string,
	entryId: string,
	spaceId: string,
): Promise<void> {
	await expect
		.poll(
			async () => {
				const response = await request.post(
					getBackendUrl(`/spaces/${spaceId}/entries/query`),
					{
						data: {
							query: { scope: { kind: "all" }, text: query },
							projection: { kind: "preview" },
							limit: 10,
						},
					},
				);
				if (!response.ok()) return false;
				const page = (await response.json()) as { rows?: Array<{ id?: string }> };
				return (page.rows ?? []).some((row) => row.id === entryId);
			},
			{ timeout: 30_000 },
		)
		.toBe(true);
}
