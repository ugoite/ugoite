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

	test("REQ-SRCH-006: search toolbar keeps its query and saved SQL routes reachable", async ({ page }) => {
		await page.goto(getFrontendUrl(`/spaces/${spaceId}/search`), {
			waitUntil: "domcontentloaded",
		});

		await expect(page.getByRole("search")).toHaveCount(1);
		await expect(page.getByLabel("Search keywords")).toBeVisible();
		await expect(page.getByRole("textbox", { name: "Search keywords" }))
			.toHaveCount(1);
		await expect(page.getByRole("button", { name: "Search entries" }))
			.toBeVisible();
		for (const name of ["Columns", "Filter", "Sort"]) {
			await expect(page.getByRole("button", { name, exact: true }))
				.toHaveCount(1);
		}
		const savedQueries = page.getByRole("link", {
			name: "Open saved queries",
		});
		await expect(savedQueries)
			.toHaveAttribute("href", `/spaces/${spaceId}/sql`);
		await expect(savedQueries).toHaveAttribute("title", "Open saved queries");
		await expect(savedQueries.locator("svg")).toHaveAttribute(
			"aria-hidden",
			"true",
		);
		await expect(page.getByText("Saved SQL", { exact: true }))
			.not.toBeAttached();

		await savedQueries.click();
		await expect(page).toHaveURL(`/spaces/${spaceId}/sql`);
		await expect(page.getByRole("heading", { name: "Saved SQL" }))
			.toBeVisible();
		await page.getByRole("link", { name: "Search", exact: true }).click();
		await expect(page).toHaveURL(`/spaces/${spaceId}/search`);
		await expectNoObjectCoercion(page);
	});

	test("REQ-SRCH-006: toolbar controls fit at phone and 200% zoom widths", async ({ page }) => {
		await page.setViewportSize({ width: 390, height: 844 });
		await page.goto(getFrontendUrl(`/spaces/${spaceId}/search`), {
			waitUntil: "domcontentloaded",
		});
		await expect(page.getByLabel("Search keywords")).toBeVisible();

		// 160 CSS pixels models the effective viewport of a 320px display at 200% zoom.
		for (const width of [160, 320, 375, 390]) {
			await page.setViewportSize({ width, height: 844 });
			const layout = await page.evaluate(() => {
				const controls = Array.from(document.querySelectorAll(
					".entry-browser-display-button, .entry-browser-sql-link",
				)).map((element) => {
					const rect = element.getBoundingClientRect();
					return { width: rect.width, height: rect.height };
				});
				const groups = Array.from(document.querySelectorAll(
					".entry-browser-search-form, .entry-browser-display-actions, .entry-browser-toolbar-navigation",
				)).map((element) => element.getBoundingClientRect().toJSON());
				return {
					documentWidth: document.documentElement.scrollWidth,
					viewportWidth: document.documentElement.clientWidth,
					controls,
					groups,
				};
			});
			expect(layout.documentWidth).toBeLessThanOrEqual(
				layout.viewportWidth + 1,
			);
			expect(layout.controls).toHaveLength(4);
			for (const control of layout.controls) {
				expect(control.width).toBeGreaterThanOrEqual(44);
				expect(control.height).toBeGreaterThanOrEqual(44);
			}
			for (
				let leftIndex = 0;
				leftIndex < layout.groups.length;
				leftIndex++
			) {
				for (
					let rightIndex = leftIndex + 1;
					rightIndex < layout.groups.length;
					rightIndex++
				) {
					const left = layout.groups[leftIndex];
					const right = layout.groups[rightIndex];
					expect(
						left.right <= right.left || right.right <= left.left ||
						left.bottom <= right.top || right.bottom <= left.top,
					).toBe(true);
				}
			}
		}
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
			await page.getByRole("search").waitFor();
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

	test("REQ-SRCH-007: a late superseded query cannot replace current results", async ({ page, request }) => {
		test.setTimeout(120_000);
		const runId = Date.now();
		const formName = `SearchRaceForm${runId}`;
		let oldEntryId: string | null = null;
		let newEntryId: string | null = null;
		let releaseOldResponse!: () => void;
		let oldRequestObserved!: () => void;
		let oldResponseObserved!: () => void;
		const oldResponseReleased = new Promise<void>((resolve) => {
			releaseOldResponse = resolve;
		});
		const oldRequestStarted = new Promise<void>((resolve) => {
			oldRequestObserved = resolve;
		});
		const oldResponseFinished = new Promise<void>((resolve) => {
			oldResponseObserved = resolve;
		});

		try {
			await ensureSearchForm(request, formName, spaceId);
			oldEntryId = await createEntry(request, spaceId, {
				form: formName,
				fields: { Body: `stale-result-${runId}` },
			});
			newEntryId = await createEntry(request, spaceId, {
				form: formName,
				fields: { Body: `current-result-${runId}` },
			});

			// Model an abort-insensitive transport: let both network responses reach
			// the application so the controller's request-generation fence is tested.
			await page.addInitScript(() => {
				const nativeFetch = window.fetch.bind(window);
				window.fetch = (input, init) => {
					const url = typeof input === "string"
						? input
						: input instanceof URL
						? input.href
						: input.url;
					if (url.includes("/entries/query") && init?.signal) {
						const { signal: _signal, ...transportInit } = init;
						return nativeFetch(input, transportInit);
					}
					return nativeFetch(input, init);
				};
			});
			await page.route(`**/api/spaces/${spaceId}/entries/query`, async (route) => {
				const payload = route.request().postDataJSON() as {
					query?: { text?: string };
				};
				const isStaleQuery = payload.query?.text === `stale-result-${runId}`;
				if (isStaleQuery) {
					oldRequestObserved();
					await oldResponseReleased;
				}
				const response = await route.fetch();
				await route.fulfill({ response });
				if (isStaleQuery) oldResponseObserved();
			});

			await page.goto(getFrontendUrl(`/spaces/${spaceId}/search`), {
				waitUntil: "domcontentloaded",
			});
			const search = page.getByLabel("Search keywords");
			const submit = page.getByRole("button", { name: "Search entries" });
			await search.fill(`stale-result-${runId}`);
			await submit.click();
			await oldRequestStarted;

			await search.fill(`current-result-${runId}`);
			await submit.click();
			await expect(page.getByText(`current-result-${runId}`)).toBeVisible();
			expect(oldEntryId).not.toBeNull();
			expect(newEntryId).not.toBeNull();
			await expect(page.getByText(`stale-result-${runId}`)).toHaveCount(0);

			releaseOldResponse();
			await oldResponseFinished;
			await expect(page.getByText(`current-result-${runId}`)).toBeVisible();
			await expect(page.getByText(`stale-result-${runId}`)).toHaveCount(0);
		} finally {
			releaseOldResponse?.();
			if (oldEntryId) {
				await request.delete(getBackendUrl(`/spaces/${spaceId}/entries/${oldEntryId}`));
			}
			if (newEntryId) {
				await request.delete(getBackendUrl(`/spaces/${spaceId}/entries/${newEntryId}`));
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
