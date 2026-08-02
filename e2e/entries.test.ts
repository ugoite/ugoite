/**
 * Entries E2E Tests for Ugoite
 *
 * These tests verify the full entries CRUD functionality:
 * - Create entries
 * - Update entries
 * - Delete entries
 */

import { expect, test, type Page } from "@playwright/test";
import { Buffer } from "node:buffer";
import { ensureDefaultForm, getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client.ts";

async function settleUiLoading(page: Page): Promise<void> {
	await page.waitForTimeout(150);
	await page
		.waitForFunction(() => !document.querySelector(".ui-loading-bar"), undefined, {
			timeout: 5_000,
		})
		.catch(() => undefined);
	await page.waitForTimeout(150);
}

test.describe("Entries CRUD", () => {
	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
		await ensureDefaultForm(request);
	});

	test("POST /spaces/default/entries creates a new entry", async ({ request }) => {
		const timestamp = Date.now();
		const res = await request.post(
			getBackendUrl("/spaces/default/entries"),
			{
				data: {
					markdown: `---\nform: Entry\n---\n# E2E Test Entry ${timestamp}\n\n## Body\nCreated at ${new Date().toISOString()}`,
				},
			},
		);
		expect(res.status()).toBe(201);

		const entry = (await res.json()) as { id: string };
		expect(entry).toHaveProperty("id");

		await request.delete(getBackendUrl(`/spaces/default/entries/${entry.id}`));
	});

	test("GET /spaces/default/entries returns entry list", async ({ request }) => {
		const res = await request.get(
			getBackendUrl("/spaces/default/entries"),
		);
		expect(res.ok()).toBeTruthy();

		const entries = await res.json();
		expect(Array.isArray(entries)).toBe(true);
	});

	test("consecutive PUT should succeed with updated revision_id", async ({ request }) => {
		const createRes = await request.post(
			getBackendUrl("/spaces/default/entries"),
			{
				data: {
					markdown:
						"---\nform: Entry\n---\n# Initial Content\n\n## Body\nThis is the first version.",
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string; revision_id: string };

		const firstUpdateRes = await request.put(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
			{
				data: {
					markdown:
						"---\nform: Entry\n---\n# Updated Content\n\n## Body\nThis is the second version.",
					parent_revision_id: created.revision_id,
				},
			},
		);
		expect(firstUpdateRes.ok()).toBeTruthy();
		const firstResult = (await firstUpdateRes.json()) as {
			revision_id: string;
		};

		const secondUpdateRes = await request.put(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
			{
				data: {
					markdown:
						"---\nform: Entry\n---\n# Third Version\n\n## Body\nThis is the third version.",
					parent_revision_id: firstResult.revision_id,
				},
			},
		);
		expect(secondUpdateRes.ok()).toBeTruthy();

		await request.delete(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
		);
	});

	test("PUT with stale revision_id should return 409 conflict", async ({ request }) => {
		const createRes = await request.post(
			getBackendUrl("/spaces/default/entries"),
			{
				data: {
					markdown:
						"---\nform: Entry\n---\n# Conflict Test\n\n## Body\nTesting revision conflicts.",
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string; revision_id: string };

		const firstUpdateRes = await request.put(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
			{
				data: {
					markdown:
						"---\nform: Entry\n---\n# After First Update\n\n## Body\nFirst update body",
					parent_revision_id: created.revision_id,
				},
			},
		);
		expect(firstUpdateRes.ok()).toBeTruthy();

		const conflictRes = await request.put(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
			{
				data: {
					markdown:
						"---\nform: Entry\n---\n# This Should Fail\n\n## Body\nStale revision",
					parent_revision_id: created.revision_id,
				},
			},
		);
		expect(conflictRes.status()).toBe(409);

		await request.delete(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
		);
	});

	test("saved content should persist after reload (REQ-FE-010)", async ({ page, request }) => {
		const createRes = await request.post(
			getBackendUrl("/spaces/default/entries"),
			{
				data: {
					markdown:
						"---\nform: Entry\n---\n# Persistence Test\n\n## Body\nOriginal content.",
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string; revision_id: string };

		const updateRes = await request.put(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
			{
				data: {
					markdown:
						"---\nform: Entry\n---\n# Persistence Test\n\n## Body\nUpdated content that should persist.",
					parent_revision_id: created.revision_id,
				},
			},
		);
		expect(updateRes.ok()).toBeTruthy();

		await page.goto(`/spaces/default/entries/${created.id}`);
		await page.waitForLoadState("networkidle");
		const html = await page.content();
		expect(html).toContain("Updated content that should persist");
		expect(html).not.toContain("Original content");

		await request.delete(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
		);
	});

	test("REQ-FE-037: entries route opens the starter entry flow for new spaces", async ({
		page,
		request,
	}) => {
		const spaceId = `entries-first-form-${Date.now()}`;
		const createSpace = await request.post(getBackendUrl("/spaces"), {
			data: { name: spaceId },
		});
		expect([200, 201, 409]).toContain(createSpace.status());

		await page.goto(getFrontendUrl(`/spaces/${spaceId}/entries`), {
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
	});

	test("REQ-ENTRY-1872: form entry creation is one POST and one clean revision", async ({
		page,
		request,
	}) => {
		const timestamp = Date.now();
		const formName = `EntryCreateFields-${timestamp}`;
		const title = `Entry create boundary ${timestamp}`;
		const formResponse = await request.post(
			getBackendUrl("/spaces/default/forms"),
			{
				data: {
					name: formName,
					version: 1,
					template: `# ${formName}\n\n## Body\n\n## test number\n\n## ts\n`,
					fields: {
						Body: { type: "markdown", required: false },
						"test number": { type: "double", required: false },
						ts: { type: "timestamp", required: false },
					},
				},
			},
		);
		expect(formResponse.status()).toBe(201);

		let entryPostCount = 0;
		let entryPutCount = 0;
		page.on("request", (requestEvent) => {
			const url = new URL(requestEvent.url());
			if (
				requestEvent.method() === "POST" &&
				url.pathname === "/api/spaces/default/entries"
			) entryPostCount += 1;
			if (
				requestEvent.method() === "PUT" &&
				/^\/api\/spaces\/default\/entries\/[^/]+$/.test(url.pathname)
			) entryPutCount += 1;
		});

		await page.goto(
			getFrontendUrl(
				`/spaces/default/entries/new?form=${encodeURIComponent(formName)}`,
			),
			{ waitUntil: "domcontentloaded" },
		);
		await settleUiLoading(page);
		await expect(page.getByLabel("Title")).toHaveValue(formName);
		await page.getByLabel("Title").fill(title);
		await page.getByLabel("test number").fill("0");
		await page.getByLabel("ts").fill("2026-08-21T10:48");

		const createResponsePromise = page.waitForResponse((response) => {
			const url = new URL(response.url());
			return response.request().method() === "POST" &&
				url.pathname === "/api/spaces/default/entries";
		});
		const detailResponsePromise = page.waitForResponse((response) => {
			const url = new URL(response.url());
			return response.request().method() === "GET" &&
				/^\/api\/spaces\/default\/entries\/[^/]+$/.test(url.pathname);
		});
		await page.getByRole("button", { name: "Save" }).click();
		const createResponse = await createResponsePromise;
		expect(createResponse.status()).toBe(201);
		const created = (await createResponse.json()) as {
			id: string;
			revision_id: string;
		};
		const detailResponse = await detailResponsePromise;
		expect(detailResponse.status()).toBe(200);
		const detail = (await detailResponse.json()) as { revision_id: string };

		await expect(page).toHaveURL(
			new RegExp(`/spaces/default/entries/${created.id}$`),
		);
		await expect(page.getByText("All changes saved")).toBeVisible();
		await expect(page.getByText("Unsaved changes")).toHaveCount(0);
		await expect(page.locator(".ui-alert-error")).toHaveCount(0);
		expect(entryPostCount).toBe(1);
		expect(entryPutCount).toBe(0);
		expect(detail.revision_id).toBe(created.revision_id);

		const historyResponse = await request.get(
			getBackendUrl(`/spaces/default/entries/${created.id}/history`),
		);
		expect(historyResponse.ok()).toBeTruthy();
		const history = (await historyResponse.json()) as {
			revisions: Array<{ revision_id: string }>;
		};
		expect(history.revisions).toHaveLength(1);
		expect(history.revisions[0]?.revision_id).toBe(created.revision_id);

		const entryResponse = await request.get(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
		);
		expect(entryResponse.ok()).toBeTruthy();
		const entry = (await entryResponse.json()) as { content: string };
		expect(entry.content).toContain("## test number\n0");
		expect(entry.content).toContain("## ts\n2026-08-21T10:48:00");

		await request.delete(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
		);
	});

	test("REQ-FE-065: create-entry uses a searchable row_reference picker and stores the selected entry_id", async ({
		page,
		request,
	}) => {
		const timestamp = Date.now();
		const spaceId = `row-reference-picker-${timestamp}`;
		const projectAlphaId = `project-alpha-${timestamp}`;
		const projectBetaId = `project-beta-${timestamp}`;
		const taskTitle = `Task referencing alpha project ${timestamp}`;
		const createSpace = await request.post(getBackendUrl("/spaces"), {
			data: { name: spaceId },
		});
		expect([200, 201, 409]).toContain(createSpace.status());

		const createProjectForm = await request.post(getBackendUrl(`/spaces/${spaceId}/forms`), {
			data: {
				name: "Project",
				template: "# Project\n\n## Summary\n",
				fields: {
					Summary: { type: "string", required: true },
				},
			},
		});
		expect(createProjectForm.status()).toBe(201);

		const createTaskForm = await request.post(getBackendUrl(`/spaces/${spaceId}/forms`), {
			data: {
				name: "Task",
				template: "# Task\n\n## Summary\n\n## Project\n",
				fields: {
					Summary: { type: "string", required: true },
					Project: { type: "row_reference", required: true, target_form: "Project" },
				},
			},
		});
		expect(createTaskForm.status()).toBe(201);

		const createAlphaProject = await request.post(getBackendUrl(`/spaces/${spaceId}/entries`), {
			data: {
				id: projectAlphaId,
				markdown: `---\nform: Project\n---\n# Alpha Project ${timestamp}\n\n## Summary\nPrimary project`,
			},
		});
		expect(createAlphaProject.status()).toBe(201);

		const createBetaProject = await request.post(getBackendUrl(`/spaces/${spaceId}/entries`), {
			data: {
				id: projectBetaId,
				markdown: `---\nform: Project\n---\n# Beta Project ${timestamp}\n\n## Summary\nSecondary project`,
			},
		});
		expect(createBetaProject.status()).toBe(201);

		await page.goto(getFrontendUrl(`/spaces/${spaceId}/entries`), {
			waitUntil: "domcontentloaded",
		});
		await expect(page.locator("body")).toBeVisible();
		await settleUiLoading(page);
		await page.waitForLoadState("networkidle");

		const newEntryButton = page.getByRole("button", { name: "New entry" });
		const createEntryDialog = page.getByRole("dialog");
		await expect(newEntryButton).toBeEnabled();
		await newEntryButton.click();
		await expect(
			createEntryDialog.getByRole("heading", { name: "Create New Entry" }),
		).toBeVisible({ timeout: 10_000 });

		await createEntryDialog.getByLabel("Title").fill(taskTitle);
		await createEntryDialog.getByLabel("Form").selectOption("Task");
		const summaryInput = createEntryDialog.locator("#webform-1-summary");
		const projectInput = createEntryDialog.locator("#webform-0-project");
		await expect(summaryInput).toBeVisible();
		await summaryInput.fill("Choose the alpha project by search");
		await expect(summaryInput).toHaveValue("Choose the alpha project by search");
		await projectInput.fill("alpha");

		const alphaOption = createEntryDialog.getByRole("button", {
			name: new RegExp(`Alpha Project ${timestamp}.*${projectAlphaId}`),
		});
		await expect(alphaOption).toBeVisible();
		await alphaOption.click();
		await expect(createEntryDialog.getByText(projectAlphaId)).toBeVisible();
		await expect(summaryInput).toHaveValue("Choose the alpha project by search");

		await createEntryDialog.getByRole("button", { name: "Create" }).click();
		await expect(page).toHaveURL(new RegExp(`/spaces/${spaceId}/entries/[^/]+$`));

		const entriesResponse = await request.get(getBackendUrl(`/spaces/${spaceId}/entries`));
		expect(entriesResponse.ok()).toBeTruthy();
		const entries = (await entriesResponse.json()) as Array<{ id: string; title: string }>;
		const createdTask = entries.find((entry) => entry.title === taskTitle);
		expect(createdTask).toBeTruthy();
		if (!createdTask) {
			throw new Error("Created task entry was not found in the index response");
		}

		const entryResponse = await request.get(getBackendUrl(`/spaces/${spaceId}/entries/${createdTask.id}`));
		expect(entryResponse.ok()).toBeTruthy();
		const entry = (await entryResponse.json()) as { content: string };
		expect(entry.content).toContain("## Project");
		expect(entry.content).toContain(projectAlphaId);
		expect(entry.content).not.toContain(projectBetaId);
	});

	test("REQ-FE-1877: unrelated Forms own independently named scalar and list Assets", async ({
		page,
		request,
	}) => {
		test.setTimeout(120_000);
		const timestamp = Date.now();
		const spaceSlug = `form-owned-assets-${timestamp}`;
		const mediaForm = `MediaAssets-${timestamp}`;
		const contractsForm = `ContractsAssets-${timestamp}`;
		const entryIds: string[] = [];

		const createSpace = await request.post(getBackendUrl("/spaces"), {
			data: { name: spaceSlug },
		});
		expect([200, 201, 409]).toContain(createSpace.status());
		const createdSpace = (await createSpace.json()) as { id: string };
		const spaceId = createdSpace.id;

		const createForm = async (
			name: string,
			fields: Record<string, unknown>,
		) => {
			const response = await request.post(
				getBackendUrl(`/spaces/${spaceId}/forms`),
				{
					data: {
						name,
						template: `# ${name}\n\n${Object.keys(fields).map((field) => `## ${field}\n`).join("\n")}`,
						fields,
					},
				},
			);
			expect(response.status()).toBe(201);
		};

		try {
			await createForm(mediaForm, {
				thumbnail: { type: "asset_reference", required: true },
				microscope_images: {
					type: "list",
					required: true,
					items: { type: "asset_reference" },
				},
			});
			await createForm(contractsForm, {
				contract: { type: "asset_reference", required: true },
				raw_data: {
					type: "list",
					required: true,
					items: { type: "asset_reference" },
				},
			});
			await page.goto(
				getFrontendUrl(
					`/spaces/${spaceId}/entries/new?form=${encodeURIComponent(mediaForm)}`,
				),
				{ waitUntil: "domcontentloaded" },
			);
			const thumbnail = page.locator('[data-field-name="thumbnail"]');
			const microscopeImages = page.locator(
				'[data-field-name="microscope_images"]',
			);
			await expect(thumbnail).toBeVisible();
			await thumbnail.locator('input[type="file"]').setInputFiles({
				name: "thumbnail.txt",
				mimeType: "text/plain",
				buffer: Buffer.from("thumbnail"),
			});
			await microscopeImages.locator('input[type="file"]').setInputFiles([
				{
					name: "microscope-a.txt",
					mimeType: "text/plain",
					buffer: Buffer.from("a"),
				},
				{
					name: "microscope-b.txt",
					mimeType: "text/plain",
					buffer: Buffer.from("b"),
				},
			]);
			await expect(
				page.getByText("Uploaded; entry not saved yet"),
			).toHaveCount(3, { timeout: 15_000 });
			const createResponse = page.waitForResponse(
				(response) =>
					response.request().method() === "POST" &&
					response.url().endsWith(`/api/spaces/${spaceId}/entries`),
				{ timeout: 15_000 },
			);
			await page.getByRole("button", { name: "Save" }).click();
			expect((await createResponse).status()).toBe(201);
			await expect(page).toHaveURL(
				new RegExp(`/spaces/${spaceId}/entries/[^/]+$`),
			);
			const mediaEntryId = decodeURIComponent(
				new URL(page.url()).pathname.split("/").pop() ?? "",
			);
			entryIds.push(mediaEntryId);

			let mediaEntryResponse = await request.get(
				getBackendUrl(`/spaces/${spaceId}/entries/${mediaEntryId}`),
			);
			expect(mediaEntryResponse.ok()).toBeTruthy();
			let mediaEntry = await mediaEntryResponse.json() as { content: string };
			expect(mediaEntry.content).toContain('"name":"thumbnail.txt"');
			expect(mediaEntry.content).toContain('"name":"microscope-a.txt"');
			expect(mediaEntry.content).toContain('"name":"microscope-b.txt"');

			await page.reload({ waitUntil: "domcontentloaded" });
			await expect(page.getByText("thumbnail.txt")).toBeVisible();
			const readResponse = page.waitForResponse(
				(response) => {
					const requestEvent = response.request();
					const url = new URL(response.url());
					return requestEvent.method() === "GET" &&
						url.pathname.includes(`/api/spaces/${spaceId}/assets/`) &&
						url.searchParams.get("form") === mediaForm &&
						url.searchParams.get("entry_id") === mediaEntryId;
				},
				{ timeout: 15_000 },
			);
			await thumbnail.getByRole("button", { name: "Open or download" }).click();
			const assetReadResponse = await readResponse;
			expect(assetReadResponse.status()).toBe(200);
			expect(await assetReadResponse.body()).toEqual(Buffer.from("thumbnail"));

			const replacement = page.locator('[data-field-name="thumbnail"]');
			await replacement.getByLabel("Replace").setInputFiles({
				name: "thumbnail-replaced.txt",
				mimeType: "text/plain",
				buffer: Buffer.from("replacement"),
			});
			await expect(page.getByText("Uploaded; entry not saved yet")).toHaveCount(1, {
				timeout: 15_000,
			});
			const replaceResponse = page.waitForResponse(
				(response) =>
					response.request().method() === "PUT" &&
					response.url().endsWith(`/api/spaces/${spaceId}/entries/${mediaEntryId}`),
				{ timeout: 15_000 },
			);
			await page.getByRole("button", { name: "Save" }).click();
			expect((await replaceResponse).ok()).toBeTruthy();
			const orderedList = page.locator('[data-field-name="microscope_images"]');
			await orderedList.getByRole("button", {
				name: "microscope-b.txt up",
			}).click();
			const reorderResponse = page.waitForResponse(
				(response) =>
					response.request().method() === "PUT" &&
					response.url().endsWith(`/api/spaces/${spaceId}/entries/${mediaEntryId}`),
				{ timeout: 15_000 },
			);
			await page.getByRole("button", { name: "Save" }).click();
			expect((await reorderResponse).ok()).toBeTruthy();
			await expect(page.getByText("All changes saved")).toBeVisible({
				timeout: 15_000,
			});

			mediaEntryResponse = await request.get(
				getBackendUrl(`/spaces/${spaceId}/entries/${mediaEntryId}`),
			);
			mediaEntry = await mediaEntryResponse.json() as { content: string };
			expect(mediaEntry.content).toContain('"name":"thumbnail-replaced.txt"');
			expect(
				mediaEntry.content.indexOf('"name":"microscope-b.txt"'),
			).toBeLessThan(mediaEntry.content.indexOf('"name":"microscope-a.txt"'));

			const removeMicroscopeB = orderedList
				.locator(".ui-asset-item")
				.filter({ hasText: "microscope-b.txt" })
				.getByRole("button", { name: "Remove" });
			await expect(removeMicroscopeB).toBeVisible({ timeout: 15_000 });
			await expect(removeMicroscopeB).toBeEnabled({ timeout: 15_000 });
			await removeMicroscopeB.click({ timeout: 15_000 });
			const removeResponse = page.waitForResponse(
				(response) =>
					response.request().method() === "PUT" &&
					response.url().endsWith(`/api/spaces/${spaceId}/entries/${mediaEntryId}`),
				{ timeout: 15_000 },
			);
			await page.getByRole("button", { name: "Save" }).click();
			expect((await removeResponse).ok()).toBeTruthy();
			const removedMediaEntryResponse = await request.get(
				getBackendUrl(`/spaces/${spaceId}/entries/${mediaEntryId}`),
				{ timeout: 15_000 },
			);
			const removedMediaEntry = await removedMediaEntryResponse.json() as {
				content: string;
			};
			expect(removedMediaEntry.content).not.toContain(
				'"name":"microscope-b.txt"',
			);

			await page.goto(
				getFrontendUrl(
					`/spaces/${spaceId}/entries/new?form=${encodeURIComponent(contractsForm)}`,
				),
				{ waitUntil: "domcontentloaded", timeout: 15_000 },
			);
			const contract = page.locator('[data-field-name="contract"]');
			const rawData = page.locator('[data-field-name="raw_data"]');
			await expect(contract).toBeVisible({ timeout: 15_000 });
			await contract.locator('input[type="file"]').setInputFiles({
				name: "contract.pdf",
				mimeType: "application/pdf",
				buffer: Buffer.from("contract"),
			});
			await rawData.locator('input[type="file"]').setInputFiles({
				name: "raw-data.csv",
				mimeType: "text/csv",
				buffer: Buffer.from("raw"),
			});
			await expect(
				page.getByText("Uploaded; entry not saved yet"),
			).toHaveCount(2, { timeout: 15_000 });
			const secondCreateResponse = page.waitForResponse(
				(response) =>
					response.request().method() === "POST" &&
					response.url().endsWith(`/api/spaces/${spaceId}/entries`),
				{ timeout: 15_000 },
			);
			await page.getByRole("button", { name: "Save" }).click();
			expect((await secondCreateResponse).status()).toBe(201);
			const contractsEntryId = decodeURIComponent(
				new URL(page.url()).pathname.split("/").pop() ?? "",
			);
			entryIds.push(contractsEntryId);
			const contractsEntryResponse = await request.get(
				getBackendUrl(`/spaces/${spaceId}/entries/${contractsEntryId}`),
			);
			const contractsEntry = await contractsEntryResponse.json() as { content: string };
			expect(contractsEntry.content).toContain('"name":"contract.pdf"');
			expect(contractsEntry.content).toContain('"name":"raw-data.csv"');
		} finally {
			for (const entryId of entryIds) {
				await request.delete(
					getBackendUrl(`/spaces/${spaceId}/entries/${entryId}`),
				);
			}
		}
	});

	test("REQ-FE-033: frontend entry detail route renders (not SolidJS Not Found)", async ({ page, request }) => {
		const createRes = await request.post(
			getBackendUrl("/spaces/default/entries"),
			{
				data: {
					markdown:
						"---\nform: Entry\n---\n# Detail Route Test\n\n## Body\nRoute render check.",
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string };

		await page.goto(`/spaces/default/entries/${created.id}`);
		await page.waitForLoadState("networkidle");
		const html = await page.content();
		expect(html).not.toContain("Visit solidjs.com");
		expect(html).not.toContain("NOT FOUND");

		await request.delete(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
		);
	});

	test("REQ-FE-005: entry detail preview escapes raw HTML in markdown content", async ({ page, request }) => {
		const createRes = await request.post(
			getBackendUrl("/spaces/default/entries"),
			{
				data: {
					content:
						'---\nform: Entry\n---\n# Preview Safety\n\n## Body\n<img src=x onerror="window.__ugoiteXss=\'ran\'">\n\n**bold**',
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string };

		await page.goto(`/spaces/default/entries/${created.id}`);
		await page.waitForLoadState("networkidle");
		await settleUiLoading(page);

		const preview = page.locator(".preview").first();
		await expect(preview).toBeVisible();
		await expect(preview.locator("img")).toHaveCount(0);
		await expect(preview).toContainText('<img src=x onerror="window.__ugoiteXss=\'ran\'">');
		await expect(preview.locator("strong")).toHaveText("bold");

		const marker = await page.evaluate(() => {
			const target = globalThis as typeof globalThis & { __ugoiteXss?: string };
			return target.__ugoiteXss ?? null;
		});
		expect(marker).toBeNull();

		await request.delete(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
		);
	});

	test("REQ-FE-033: Retrieve entry with special characters", async ({ page, request }) => {
		const timestamp = Date.now();
		const title = `Special Entry @ ${timestamp} % &`;
		const createRes = await request.post(
			getBackendUrl("/spaces/default/entries"),
			{
				data: {
					markdown: `---\nform: Entry\n---\n# ${title}\n\n## Body\nTesting special chars in title.`,
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string };

		await page.goto(`/spaces/default/entries/${encodeURIComponent(created.id)}`);
		await page.waitForLoadState("networkidle");
		const html = await page.content();
		expect(html).toContain(title);

		await request.delete(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
		);
	});

	test("REQ-FE-034: Multi-entry navigation should not get stuck in loading state", async ({ page, request }) => {
		const formEntries = await Promise.all([
			request.post(getBackendUrl("/spaces/default/entries"), {
				data: {
					markdown: "---\nform: Entry\n---\n# Entry A\n\n## Body\nContent A",
				},
			}),
			request.post(getBackendUrl("/spaces/default/entries"), {
				data: {
					markdown: "---\nform: Entry\n---\n# Entry B\n\n## Body\nContent B",
				},
			}),
			request.post(getBackendUrl("/spaces/default/entries"), {
				data: {
					markdown: "---\nform: Entry\n---\n# Entry C\n\n## Body\nContent C",
				},
			}),
		]);

		const entries = (await Promise.all(
			formEntries.map((res) => res.json()),
		)) as Array<{ id: string }>;

		for (const entry of entries) {
			await page.goto(`/spaces/default/entries/${encodeURIComponent(entry.id)}`);
			await page.waitForLoadState("networkidle");
			const entryHtml = await page.content();
			expect(entryHtml).not.toContain("Loading entry...");
			expect(entryHtml).toContain("<div id=\"app\">");
		}

		for (const entry of entries) {
			await page.goto(`/spaces/default/entries/${encodeURIComponent(entry.id)}`);
			await page.waitForLoadState("networkidle");
			const html = await page.content();
			expect(html).not.toContain("Loading entry...");
			expect(html).toContain("<div id=\"app\">");
		}

		await Promise.all(
			entries.map((entry) =>
				request.delete(
					getBackendUrl(`/spaces/default/entries/${entry.id}`),
				),
			),
		);
	});

	test("REQ-FE-035: Navigation timeout handling and recovery", async ({ page, request }) => {
		const createRes = await request.post(
			getBackendUrl("/spaces/default/entries"),
			{
				data: {
					markdown:
						"---\nform: Entry\n---\n# Timeout Recovery Test\n\n## Body\nEnsure navigation resolves.",
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string };

		await page.goto(`/spaces/default/entries/${created.id}`);
		await page.waitForLoadState("networkidle");
		const html = await page.content();
		expect(html).not.toContain("Loading...");

		await request.delete(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
		);
	});

	test("PUT /spaces/default/entries/:id updates entry", async ({ request }) => {
		const createRes = await request.post(
			getBackendUrl("/spaces/default/entries"),
			{
				data: {
					markdown:
						"---\nform: Entry\n---\n# Update Test Entry\n\n## Body\nOriginal content",
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string };

		const getRes = await request.get(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
		);
		const current = (await getRes.json()) as { revision_id: string };

		const updateRes = await request.put(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
			{
				data: {
					markdown:
						"---\nform: Entry\n---\n# Updated Title\n\n## Body\nUpdated content by E2E test",
					parent_revision_id: current.revision_id,
				},
			},
		);
		expect(updateRes.ok()).toBeTruthy();

		await request.delete(getBackendUrl(`/spaces/default/entries/${created.id}`));
	});

	test("DELETE /spaces/default/entries/:id removes entry", async ({ request }) => {
		const createRes = await request.post(
			getBackendUrl("/spaces/default/entries"),
			{
				data: {
					markdown:
						"---\nform: Entry\n---\n# Delete Test Entry\n\n## Body\nTo be deleted",
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string };

		const deleteRes = await request.delete(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
		);
		expect([200, 204]).toContain(deleteRes.status());

		const fetchRes = await request.get(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
		);
		expect(fetchRes.status()).toBe(404);
	});
});
