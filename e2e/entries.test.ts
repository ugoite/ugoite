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
import {
	ensureDefaultForm,
	getBackendUrl,
	getDefaultSpaceId,
	getFrontendUrl,
	waitForServers,
} from "./lib/client.ts";
import { expectNoObjectCoercion } from "./lib/ui-safety.ts";

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
	let spaceId = "";

	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
		spaceId = await getDefaultSpaceId(request);
		await ensureDefaultForm(request, spaceId);
	});

	test("POST /spaces/:space_id/entries creates a new entry", async ({ request }) => {
		const timestamp = Date.now();
		const res = await request.post(
			getBackendUrl(`/spaces/${spaceId}/entries`),
			{
				data: {
					form: "Entry",
					fields: {
						Body: `E2E Test Entry ${timestamp}\n\nCreated at ${new Date().toISOString()}`,
					},
				},
			},
		);
		expect(res.status()).toBe(201);

		const entry = (await res.json()) as { id: string };
		expect(entry).toHaveProperty("id");

		await request.delete(getBackendUrl(`/spaces/${spaceId}/entries/${entry.id}`));
	});

	test("GET /spaces/:space_id/entries returns entry list", async ({ request }) => {
		const res = await request.get(
			getBackendUrl(`/spaces/${spaceId}/entries`),
		);
		expect(res.ok()).toBeTruthy();

		const entries = await res.json();
		expect(Array.isArray(entries)).toBe(true);
	});

	test("consecutive PUT should succeed with updated revision_id", async ({ request }) => {
		const createRes = await request.post(
			getBackendUrl(`/spaces/${spaceId}/entries`),
			{
				data: {
					form: "Entry",
					fields: { Body: "Initial Content\n\nThis is the first version." },
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string; revision_id: string };

		const firstUpdateRes = await request.put(
			getBackendUrl(`/spaces/${spaceId}/entries/${created.id}`),
			{
				data: {
					form: "Entry",
					fields: { Body: "Updated Content\n\nThis is the second version." },
					parent_revision_id: created.revision_id,
				},
			},
		);
		expect(firstUpdateRes.ok()).toBeTruthy();
		const firstResult = (await firstUpdateRes.json()) as {
			revision_id: string;
		};

		const secondUpdateRes = await request.put(
			getBackendUrl(`/spaces/${spaceId}/entries/${created.id}`),
			{
				data: {
					form: "Entry",
					fields: { Body: "Third Version\n\nThis is the third version." },
					parent_revision_id: firstResult.revision_id,
				},
			},
		);
		expect(secondUpdateRes.ok()).toBeTruthy();

		await request.delete(
			getBackendUrl(`/spaces/${spaceId}/entries/${created.id}`),
		);
	});

	test("PUT with stale revision_id should return 409 conflict", async ({ request }) => {
		const createRes = await request.post(
			getBackendUrl(`/spaces/${spaceId}/entries`),
			{
				data: {
					form: "Entry",
					fields: { Body: "Conflict Test\n\nTesting revision conflicts." },
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string; revision_id: string };

		const firstUpdateRes = await request.put(
			getBackendUrl(`/spaces/${spaceId}/entries/${created.id}`),
			{
				data: {
					form: "Entry",
					fields: { Body: "After First Update\n\nFirst update body" },
					parent_revision_id: created.revision_id,
				},
			},
		);
		expect(firstUpdateRes.ok()).toBeTruthy();

		const conflictRes = await request.put(
			getBackendUrl(`/spaces/${spaceId}/entries/${created.id}`),
			{
				data: {
					form: "Entry",
					fields: { Body: "This Should Fail\n\nStale revision" },
					parent_revision_id: created.revision_id,
				},
			},
		);
		expect(conflictRes.status()).toBe(409);

		await request.delete(
			getBackendUrl(`/spaces/${spaceId}/entries/${created.id}`),
		);
	});

	test("saved content should persist after reload (REQ-FE-010)", async ({ page, request }) => {
		const createRes = await request.post(
			getBackendUrl(`/spaces/${spaceId}/entries`),
			{
				data: {
					form: "Entry",
					fields: { Body: "Persistence Test\n\nOriginal content." },
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string; revision_id: string };

		const updateRes = await request.put(
			getBackendUrl(`/spaces/${spaceId}/entries/${created.id}`),
			{
				data: {
					form: "Entry",
					fields: {
						Body: "Persistence Test\n\nUpdated content that should persist.",
					},
					parent_revision_id: created.revision_id,
				},
			},
		);
		expect(updateRes.ok()).toBeTruthy();

		await page.goto(`/spaces/${spaceId}/entries/${created.id}`);
		await page.waitForLoadState("networkidle");
		await expect(page).toHaveURL(
			new RegExp(`/spaces/${spaceId}/entries/${created.id}$`),
		);
		await expect(page.getByLabel("Body")).toHaveValue(
			"Updated content that should persist.",
		);
		await expect(page.getByLabel("Body")).not.toHaveValue(
			"Original content.",
		);

		await page.reload();
		await page.waitForLoadState("networkidle");
		await expect(page).toHaveURL(
			new RegExp(`/spaces/${spaceId}/entries/${created.id}$`),
		);
		await expect(page.getByLabel("Body")).toHaveValue(
			"Updated content that should persist.",
		);
		await expect(page.getByLabel("Body")).not.toHaveValue(
			"Original content.",
		);

		await request.delete(
			getBackendUrl(`/spaces/${spaceId}/entries/${created.id}`),
		);
	});

	test("REQ-FE-037: entries route opens the starter entry flow for new spaces", async ({
		page,
		request,
	}) => {
		const spaceName = `entries-first-form-${Date.now()}`;
		const createSpace = await request.post(getBackendUrl("/spaces"), {
			data: { slug: spaceName, name: "Entries first-form test" },
		});
		expect([200, 201, 409]).toContain(createSpace.status());

		let spaceId: string;
		if (createSpace.status() === 409) {
			const spacesResponse = await request.get(getBackendUrl("/spaces"));
			expect(spacesResponse.ok()).toBe(true);
			const spaces = await spacesResponse.json() as Array<{
				space_uid: string;
				slug: string;
				name: string;
			}>;
			const existingSpace = spaces.find((space) => space.slug === spaceName);
			expect(existingSpace).toBeDefined();
			spaceId = existingSpace!.space_uid;
		} else {
			const createdSpace = await createSpace.json() as { space_uid?: string };
			expect(createdSpace.space_uid).toBeTruthy();
			spaceId = createdSpace.space_uid!;
		}
		expect(spaceId).not.toBe(spaceName);

		await page.goto(getFrontendUrl(`/spaces/${spaceId}/entries`), {
			waitUntil: "domcontentloaded",
		});
		await expect(page.locator("body")).toBeVisible();
		await settleUiLoading(page);

		await expect(page.getByRole("button", { name: "+ Entry" })).toBeEnabled();
		await expect(
			page.getByText("Start by creating your first form."),
		).toHaveCount(0);

		await page.getByRole("button", { name: "+ Entry" }).click();
		await expect(page).toHaveURL(
			new RegExp(`/spaces/${spaceId}/entries/new$`),
			{ timeout: 10_000 },
		);
		await expect(
			page.getByRole("heading", { name: "Create New Entry" }),
		).toBeVisible({
			timeout: 10_000,
		});
		await expect(page.locator("#entry-form-selector")).toHaveValue("Entry");
		// Title-less Entry: no Entry-level title input; the Body field carries
		// the content and the heading falls back to the stable entry ID.
		await page.getByLabel("Body").fill(
			`Starter entry from Entries ${Date.now()}`,
		);
		await page.getByRole("button", { name: "Save" }).click();
		await page.waitForURL(new RegExp(`/spaces/${spaceId}/entries/[^/]+$`), {
			timeout: 10_000,
		});
		const createdId = decodeURIComponent(
			new URL(page.url()).pathname.split("/").pop() ?? "",
		);
		expect(createdId).not.toBe("");
		await expect(
			page.getByRole("heading", {
				name: createdId,
				level: 1,
			}),
		).toBeVisible();
	});

	test("REQ-ENTRY-1872: form entry creation is one POST and one clean revision", async ({
		page,
		request,
	}) => {
		const timestamp = Date.now();
		const formName = `EntryCreateFields-${timestamp}`;
		const formResponse = await request.post(
			getBackendUrl(`/spaces/${spaceId}/forms`),
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
				url.pathname === `/api/spaces/${spaceId}/entries`
			) entryPostCount += 1;
			if (
				requestEvent.method() === "PUT" &&
				new RegExp(`^/api/spaces/${spaceId}/entries/[^/]+$`).test(url.pathname)
			) entryPutCount += 1;
		});

		await page.goto(
			getFrontendUrl(
				`/spaces/${spaceId}/entries/new?form=${encodeURIComponent(formName)}`,
			),
			{ waitUntil: "domcontentloaded" },
		);
		await settleUiLoading(page);
		// Title-less structured create: no title input is rendered.
		await expect(page.getByLabel("Title")).toHaveCount(0);
		await page.getByLabel("test number").fill("0");
		await page.getByLabel("ts").fill("2026-08-21T10:48");

		const createResponsePromise = page.waitForResponse((response) => {
			const url = new URL(response.url());
			return response.request().method() === "POST" &&
				url.pathname === `/api/spaces/${spaceId}/entries`;
		});
		const detailResponsePromise = page.waitForResponse((response) => {
			const url = new URL(response.url());
			return response.request().method() === "GET" &&
				new RegExp(`^/api/spaces/${spaceId}/entries/[^/]+$`).test(url.pathname);
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
			new RegExp(`/spaces/${spaceId}/entries/${created.id}$`),
		);
		await expect(page.getByText("All changes saved")).toBeVisible();
		await expect(page.getByText("Unsaved changes")).toHaveCount(0);
		await expect(page.locator(".ui-alert-error")).toHaveCount(0);
		expect(entryPostCount).toBe(1);
		expect(entryPutCount).toBe(0);
		expect(detail.revision_id).toBe(created.revision_id);

		const historyResponse = await request.get(
			getBackendUrl(`/spaces/${spaceId}/entries/${created.id}/history`),
		);
		expect(historyResponse.ok()).toBeTruthy();
		const history = (await historyResponse.json()) as {
			revisions: Array<{ revision_id: string }>;
		};
		expect(history.revisions).toHaveLength(1);
		expect(history.revisions[0]?.revision_id).toBe(created.revision_id);

		const entryResponse = await request.get(
			getBackendUrl(`/spaces/${spaceId}/entries/${created.id}`),
		);
		expect(entryResponse.ok()).toBeTruthy();
		const entry = (await entryResponse.json()) as { content: string };
		expect(entry.content).toContain("## test number\n0");
		expect(entry.content).toContain("## ts\n2026-08-21T10:48:00");

		await request.delete(
			getBackendUrl(`/spaces/${spaceId}/entries/${created.id}`),
		);
	});

	test("REQ-FE-065: create-entry uses a searchable row_reference picker and stores the selected entry_id", async ({
		page,
		request,
	}) => {
		const timestamp = Date.now();
		const spaceName = `row-reference-picker-${timestamp}`;
		const projectAlphaId = `project-alpha-${timestamp}`;
		const projectBetaId = `project-beta-${timestamp}`;
		const createSpace = await request.post(getBackendUrl("/spaces"), {
			data: { slug: spaceName, name: "Entries relation test" },
		});
		expect([200, 201, 409]).toContain(createSpace.status());

		let spaceId: string;
		if (createSpace.status() === 409) {
			const spacesResponse = await request.get(getBackendUrl("/spaces"));
			expect(spacesResponse.ok()).toBe(true);
			const spaces = await spacesResponse.json() as Array<{
				space_uid: string;
				slug: string;
				name: string;
			}>;
			const existingSpace = spaces.find((space) => space.slug === spaceName);
			expect(existingSpace).toBeDefined();
			spaceId = existingSpace!.space_uid;
		} else {
			const createdSpace = await createSpace.json() as { space_uid?: string };
			expect(createdSpace.space_uid).toBeTruthy();
			spaceId = createdSpace.space_uid!;
		}
		expect(spaceId).not.toBe(spaceName);

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
				form: "Project",
				fields: { Summary: `Alpha primary project ${timestamp}` },
			},
		});
		expect(createAlphaProject.status()).toBe(201);

		const createBetaProject = await request.post(getBackendUrl(`/spaces/${spaceId}/entries`), {
			data: {
				id: projectBetaId,
				form: "Project",
				fields: { Summary: `Beta secondary project ${timestamp}` },
			},
		});
		expect(createBetaProject.status()).toBe(201);

		await page.goto(getFrontendUrl(`/spaces/${spaceId}/entries`), {
			waitUntil: "domcontentloaded",
		});
		await expect(page.locator("body")).toBeVisible();
		await settleUiLoading(page);
		await page.waitForLoadState("networkidle");

		const newEntryButton = page.getByRole("button", { name: "+ Entry" });
		await expect(newEntryButton).toBeEnabled();
		await newEntryButton.click();
		await expect(page).toHaveURL(
			new RegExp(`/spaces/${spaceId}/entries/new$`),
		);
		await expect(
			page.getByRole("heading", { name: "Create New Entry" }),
		).toBeVisible({ timeout: 10_000 });

		await page.getByLabel("Form").selectOption("Task");
		const summaryInput = page.getByRole("textbox", { name: "Summary" });
		const projectInput = page.getByRole("searchbox", { name: "Project" });
		await expect(summaryInput).toBeVisible();
		await summaryInput.fill("Choose the alpha project by search");
		await expect(summaryInput).toHaveValue("Choose the alpha project by search");
		await projectInput.fill("alpha");

		const alphaOption = page.getByRole("button", {
			name: new RegExp(projectAlphaId),
		});
		await expect(alphaOption).toBeVisible();
		await alphaOption.click();
		await expect(page.getByText(projectAlphaId)).toBeVisible();
		await expect(summaryInput).toHaveValue("Choose the alpha project by search");

		await page.getByRole("button", { name: "Save" }).click();
		await page.waitForURL(
			(url) => {
				const path = url.pathname.replace(/\/+$/, "");
				return path.startsWith(`/spaces/${spaceId}/entries/`) &&
					!path.endsWith("/new");
			},
			{ timeout: 10_000 },
		);

		const createdTaskId = decodeURIComponent(
			new URL(page.url()).pathname.split("/").pop() ?? "",
		);
		expect(createdTaskId).not.toBe("");

		const entryResponse = await request.get(getBackendUrl(`/spaces/${spaceId}/entries/${createdTaskId}`));
		expect(entryResponse.ok()).toBeTruthy();
		const entry = (await entryResponse.json()) as { content: string };
		expect(entry.content).toContain("## Project");
		expect(entry.content).toContain(projectAlphaId);
		expect(entry.content).not.toContain(projectBetaId);
	});

	test("REQ-FE-1877: unrelated Forms own independently named scalar and list Assets", { tag: "@asset-owned" }, async ({
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
			data: { slug: spaceSlug, name: "Entries media test" },
		});
		expect([200, 201, 409]).toContain(createSpace.status());
		const createdSpace = (await createSpace.json()) as { space_uid: string };
		const spaceId = createdSpace.space_uid;

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
			page.getByRole("button", { name: "Preview thumbnail.txt" }),
		).toBeVisible({ timeout: 15_000 });
		await expect(
			page.getByRole("button", { name: "Preview microscope-a.txt" }),
		).toBeVisible({ timeout: 15_000 });
		await expect(
			page.getByRole("button", { name: "Preview microscope-b.txt" }),
		).toBeVisible({ timeout: 15_000 });
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

			const mediaEntryUrl = getBackendUrl(
				`/spaces/${spaceId}/entries/${mediaEntryId}`,
			);
			let mediaEntryResponse = await request.get(mediaEntryUrl);
			for (let attempt = 0; attempt < 10 && !mediaEntryResponse.ok(); attempt++) {
				await new Promise((resolve) => setTimeout(resolve, 500));
				mediaEntryResponse = await request.get(mediaEntryUrl);
			}
			expect(mediaEntryResponse.ok()).toBeTruthy();
			let mediaEntry = await mediaEntryResponse.json() as { content: string };
			expect(mediaEntry.content).toContain('"name":"thumbnail.txt"');
			expect(mediaEntry.content).toContain('"name":"microscope-a.txt"');
			expect(mediaEntry.content).toContain('"name":"microscope-b.txt"');

		await page.reload({ waitUntil: "domcontentloaded" });
		await expect(
			page.getByRole("button", { name: "Preview thumbnail.txt" }),
		).toBeVisible();
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
			await thumbnail.getByRole("button", { name: "Download thumbnail.txt" }).click();
			const assetReadResponse = await readResponse;
			expect(assetReadResponse.status()).toBe(200);
			expect(await assetReadResponse.body()).toEqual(Buffer.from("thumbnail"));

			const previewResponse = page.waitForResponse(
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
			await thumbnail.getByRole("button", { name: "Preview thumbnail.txt" }).click();
			expect((await previewResponse).status()).toBe(200);
			const previewDialog = page.getByRole("dialog", {
				name: "thumbnail.txt",
			});
			await expect(previewDialog).toBeVisible();
			await expect(thumbnail.locator(".ui-asset-preview-panel")).toHaveCount(0);
			await previewDialog.getByRole("button", { name: "Close" }).click();
			await expect(page.getByRole("dialog")).toHaveCount(0);

			const replacement = page.locator('[data-field-name="thumbnail"]');
			await replacement.getByLabel("Replace").setInputFiles({
				name: "thumbnail-replaced.txt",
				mimeType: "text/plain",
				buffer: Buffer.from("replacement"),
			});
		await expect(
			page.getByRole("button", {
				name: "Preview thumbnail-replaced.txt",
			}),
		).toBeVisible({
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
			.locator(".ui-asset-item", {
				has: page.getByRole("button", {
					name: "Preview microscope-b.txt",
				}),
			})
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
			page.getByRole("button", { name: "Preview contract.pdf" }),
		).toBeVisible({ timeout: 15_000 });
		await expect(
			page.getByRole("button", { name: "Preview raw-data.csv" }),
		).toBeVisible({ timeout: 15_000 });
			const secondCreateResponse = page.waitForResponse(
				(response) =>
					response.request().method() === "POST" &&
					response.url().endsWith(`/api/spaces/${spaceId}/entries`),
				{ timeout: 15_000 },
			);
			await page.getByRole("button", { name: "Save" }).click();
			expect((await secondCreateResponse).status()).toBe(201);
			await expect(page).toHaveURL(
				new RegExp(`/spaces/${spaceId}/entries/[^/]+$`),
			);
			const contractsEntryId = decodeURIComponent(
				new URL(page.url()).pathname.split("/").pop() ?? "",
			);
			entryIds.push(contractsEntryId);
			const contractsEntryUrl = getBackendUrl(
				`/spaces/${spaceId}/entries/${contractsEntryId}`,
			);
			let contractsEntryResponse = await request.get(contractsEntryUrl);
			for (let attempt = 0; attempt < 10 && !contractsEntryResponse.ok(); attempt++) {
				await new Promise((resolve) => setTimeout(resolve, 500));
				contractsEntryResponse = await request.get(contractsEntryUrl);
			}
			expect(contractsEntryResponse.ok()).toBeTruthy();
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
			getBackendUrl(`/spaces/${spaceId}/entries`),
			{
				data: {
					form: "Entry",
					fields: { Body: "Detail Route Test\n\nRoute render check." },
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string };

		await page.goto(`/spaces/${spaceId}/entries/${created.id}`);
		await page.waitForLoadState("networkidle");
		const html = await page.content();
		expect(html).not.toContain("Visit solidjs.com");
		expect(html).not.toContain("NOT FOUND");
		await expectNoObjectCoercion(page);

		await request.delete(
			getBackendUrl(`/spaces/${spaceId}/entries/${created.id}`),
		);
	});

	test("REQ-FE-005: entry detail keeps raw HTML inert without a preview tab", async ({ page, request }) => {
		const createRes = await request.post(
			getBackendUrl(`/spaces/${spaceId}/entries`),
			{
				data: {
					form: "Entry",
					fields: {
						Body:
							'Preview Safety\n\n<img src=x onerror="window.__ugoiteXss=\'ran\'">\n\n**bold**',
					},
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string };

		await page.goto(`/spaces/${spaceId}/entries/${created.id}`);
		await page.waitForLoadState("networkidle");
		await settleUiLoading(page);
		// The simplified detail view offers no preview tab; raw markup stays
		// inside editable controls and is never rendered as HTML.
		await expect(page.getByRole("tab", { name: "Preview" })).toHaveCount(0);
		await expect(page.locator(".ui-entry-mode-tabs")).toHaveCount(0);
		await expect(page.locator(".ui-entry-main img")).toHaveCount(0);
		await expect(
			page.locator(".ui-entry-form-body textarea").first(),
		).toBeVisible();

		const marker = await page.evaluate(() => {
			const target = globalThis as typeof globalThis & { __ugoiteXss?: string };
			return target.__ugoiteXss ?? null;
		});
		expect(marker).toBeNull();

		await request.delete(
			getBackendUrl(`/spaces/${spaceId}/entries/${created.id}`),
		);
	});

	test("REQ-FE-033: Retrieve entry with special characters", async ({ page, request }) => {
		const timestamp = Date.now();
		const specialBody = `Special body @ ${timestamp} % & <tag> "quoted"`;
		const createRes = await request.post(
			getBackendUrl(`/spaces/${spaceId}/entries`),
			{
				data: {
					form: "Entry",
					fields: { Body: `${specialBody}\n\nTesting special chars in body.` },
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string };

		await page.goto(`/spaces/${spaceId}/entries/${encodeURIComponent(created.id)}`);
		await page.waitForLoadState("networkidle");
		await expect(page.getByLabel("Body")).toHaveValue(
			`${specialBody}\n\nTesting special chars in body.`,
		);
		const html = await page.content();
		expect(html).toContain(`Special body @ ${timestamp} % &amp;`);

		await request.delete(
			getBackendUrl(`/spaces/${spaceId}/entries/${created.id}`),
		);
	});

	test("REQ-FE-034: Multi-entry navigation should not get stuck in loading state", async ({ page, request }) => {
		const formEntries = await Promise.all([
			request.post(getBackendUrl(`/spaces/${spaceId}/entries`), {
				data: {
					form: "Entry",
					fields: { Body: "Entry A\n\nContent A" },
				},
			}),
			request.post(getBackendUrl(`/spaces/${spaceId}/entries`), {
				data: {
					form: "Entry",
					fields: { Body: "Entry B\n\nContent B" },
				},
			}),
			request.post(getBackendUrl(`/spaces/${spaceId}/entries`), {
				data: {
					form: "Entry",
					fields: { Body: "Entry C\n\nContent C" },
				},
			}),
		]);

		const entries = (await Promise.all(
			formEntries.map((res) => res.json()),
		)) as Array<{ id: string }>;

		for (const entry of entries) {
			await page.goto(`/spaces/${spaceId}/entries/${encodeURIComponent(entry.id)}`);
			await page.waitForLoadState("networkidle");
			const entryHtml = await page.content();
			expect(entryHtml).not.toContain("Loading entry...");
			expect(entryHtml).toContain("<div id=\"app\">");
		}

		for (const entry of entries) {
			await page.goto(`/spaces/${spaceId}/entries/${encodeURIComponent(entry.id)}`);
			await page.waitForLoadState("networkidle");
			const html = await page.content();
			expect(html).not.toContain("Loading entry...");
			expect(html).toContain("<div id=\"app\">");
		}

		await Promise.all(
			entries.map((entry) =>
				request.delete(
					getBackendUrl(`/spaces/${spaceId}/entries/${entry.id}`),
				),
			),
		);
	});

	test("REQ-FE-035: Navigation timeout handling and recovery", async ({ page, request }) => {
		const createRes = await request.post(
			getBackendUrl(`/spaces/${spaceId}/entries`),
			{
				data: {
					form: "Entry",
					fields: {
						Body: "Timeout Recovery Test\n\nEnsure navigation resolves.",
					},
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string };

		await page.goto(`/spaces/${spaceId}/entries/${created.id}`);
		await page.waitForLoadState("networkidle");
		const html = await page.content();
		expect(html).not.toContain("Loading...");

		await request.delete(
			getBackendUrl(`/spaces/${spaceId}/entries/${created.id}`),
		);
	});

	test("PUT /spaces/:space_id/entries/:id updates entry", async ({ request }) => {
		const createRes = await request.post(
			getBackendUrl(`/spaces/${spaceId}/entries`),
			{
				data: {
					form: "Entry",
					fields: { Body: "Update Test Entry\n\nOriginal content" },
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string };

		const getRes = await request.get(
			getBackendUrl(`/spaces/${spaceId}/entries/${created.id}`),
		);
		const current = (await getRes.json()) as { revision_id: string };

		const updateRes = await request.put(
			getBackendUrl(`/spaces/${spaceId}/entries/${created.id}`),
			{
				data: {
					form: "Entry",
					fields: { Body: "Updated Title\n\nUpdated content by E2E test" },
					parent_revision_id: current.revision_id,
				},
			},
		);
		expect(updateRes.ok()).toBeTruthy();

		await request.delete(getBackendUrl(`/spaces/${spaceId}/entries/${created.id}`));
	});

	test("DELETE /spaces/:space_id/entries/:id removes entry", async ({ request }) => {
		const createRes = await request.post(
			getBackendUrl(`/spaces/${spaceId}/entries`),
			{
				data: {
					form: "Entry",
					fields: { Body: "Delete Test Entry\n\nTo be deleted" },
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string };

		const deleteRes = await request.delete(
			getBackendUrl(`/spaces/${spaceId}/entries/${created.id}`),
		);
		expect([200, 204]).toContain(deleteRes.status());

		const fetchRes = await request.get(
			getBackendUrl(`/spaces/${spaceId}/entries/${created.id}`),
		);
		expect(fetchRes.status()).toBe(404);
	});
});
