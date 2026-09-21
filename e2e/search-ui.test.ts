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
		await expect(navigation.getByRole("button", { name: "Quick search" }))
			.toBeVisible();
		await expect(navigation.getByRole("button", { name: "Advanced search" }))
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
			await expect(
				page.getByRole("button", { name: new RegExp(escapeRegExp(entryId)) }),
			).toBeVisible();
			await expectNoObjectCoercion(page);
		} finally {
			if (entryId) {
				await request.delete(getBackendUrl(`/spaces/${spaceId}/entries/${entryId}`));
			}
		}
	});

	test("REQ-SRCH-005: advanced search renders structured results inline", async ({
		page,
		request,
	}) => {
		test.setTimeout(120_000);
		const runId = Date.now();
		const formName = `SUA${String(runId).slice(-6)}`;
		let entryId: string | null = null;

		try {
			await ensureSearchForm(request, formName, spaceId);
			entryId = await createEntry(request, spaceId, {
				form: formName,
				tags: ["release", "search-ui"],
				fields: {
					"Owner Name": "alice",
					Body: "Structured advanced search should find this entry.",
				},
			});
			await waitForKeywordMatch(
				request,
				"Structured advanced search should find this entry.",
				entryId,
				spaceId,
			);

			await page.goto(getFrontendUrl(`/spaces/${spaceId}/search`), {
				waitUntil: "domcontentloaded",
			});
			await page.getByRole("button", { name: "Advanced search" }).click();
			await page.getByLabel("Form").selectOption(formName);
			await page.getByLabel("Field").selectOption("Owner Name");
			await page.getByLabel("Value").fill("alice");
			await page.getByRole("button", { name: "Run advanced search" }).click();

			await expect(page).toHaveURL(new RegExp(`/spaces/${spaceId}/search$`));
			await expect(
				page.getByRole("button", { name: new RegExp(escapeRegExp(entryId!)) }),
			).toBeVisible();
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

function escapeRegExp(value: string): string {
	return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
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
