import { expect, test, type Page } from "@playwright/test";
import {
	ensureDefaultForm,
	getBackendUrl,
	getDefaultSpaceId,
	waitForServers,
} from "./lib/client.ts";

const maxVisitedPages = 16;

// Canonical Mitase evidence for dynamic route traversal and browser/runtime
// error stability. The Mitase runner invokes this file from the repository
// root with e2e/playwright.config.ts.
test.describe("Dynamic navigation traversal", () => {
	let spaceId = "";

	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
		spaceId = await getDefaultSpaceId(request);
		await ensureDefaultForm(request, spaceId);
	});

	test("REQ-E2E-004: dynamic traversal has no console or SolidJS errors", async ({
		page,
		request,
	}) => {
		// Mitase evidence: REQ-E2E-004 route, console, and runtime stability.
		test.setTimeout(120_000);

		const createdEntry = await request.post(getBackendUrl(`/spaces/${spaceId}/entries`), {
			data: {
				form: "Entry",
				fields: {
					Body: `E2E Dynamic Traversal ${Date.now()}\n\nTraversal seed entry.`,
				},
			},
		});
		expect(createdEntry.status()).toBe(201);
		const created = (await createdEntry.json()) as { id: string };

		const consoleErrors: string[] = [];
		const runtimeErrors: string[] = [];
		page.on("console", (msg) => {
			if (msg.type() === "error") {
				consoleErrors.push(msg.text());
			}
		});
		page.on("pageerror", (err) => {
			runtimeErrors.push(err.message);
		});

		const queue = [
			`/spaces/${spaceId}/dashboard`,
			`/spaces/${spaceId}/settings`,
			`/spaces/${spaceId}/entries/${created.id}`,
		];
		const visited = new Set<string>();

		try {
			while (queue.length > 0 && visited.size < maxVisitedPages) {
				const path = queue.shift();
				if (!path || visited.has(path)) {
					continue;
				}

				await visitPath(page, path, consoleErrors);
				visited.add(path);

				const discoveredLinks = await collectInternalLinks(page, spaceId);
				for (const discovered of discoveredLinks) {
					if (!visited.has(discovered.path) && !queue.includes(discovered.path)) {
						queue.push(discovered.path);
					}
				}
			}

			expect(visited.size).toBeGreaterThanOrEqual(6);

			// Exercise the same SPA transitions users take while moving between a
			// Form's Entry list, the Entry detail, and its Info/History surfaces.
			// Direct page.goto traversal above does not keep route components and
			// pending resources alive across sibling navigations.
			const formEntriesPath = `/spaces/${spaceId}/forms/Entry/entries`;
			const entryPath = `/spaces/${spaceId}/entries/${created.id}`;
			await page.goto(formEntriesPath, { waitUntil: "domcontentloaded" });
			await settleUiLoading(page);
			const documentTimeOrigin = await page.evaluate(() => performance.timeOrigin);
			const row = page.locator(`[data-entry-id="${created.id}"]`);
			await expect(row).toBeVisible();
			await row.getByRole("button", { name: "Open entry" }).click();
			await expect(page).toHaveURL(new RegExp(`${escapeRegExp(entryPath)}$`));
			await expectAppHealthy(page, consoleErrors);

			for (let cycle = 0; cycle < 2; cycle += 1) {
				const actionBar = page.locator(".ui-entry-action-bar");
				await actionBar.getByRole("link", { name: "Info" }).click();
				await expect(page).toHaveURL(new RegExp(`${escapeRegExp(entryPath)}/info$`));
				await expectAppHealthy(page, consoleErrors);
				await page.getByRole("link", { name: "Back to Entry" }).click();
				await expect(page).toHaveURL(new RegExp(`${escapeRegExp(entryPath)}$`));
				await expectAppHealthy(page, consoleErrors);

				await actionBar.getByRole("link", { name: "History & recovery" }).click();
				await expect(page).toHaveURL(new RegExp(`${escapeRegExp(entryPath)}/history$`));
				await expectAppHealthy(page, consoleErrors);
				await page.locator(`a[href*="${escapeRegExp(entryPath)}/history/"]`).first().click();
				await expect(page).toHaveURL(new RegExp(`${escapeRegExp(entryPath)}/history/.+$`));
				await expectAppHealthy(page, consoleErrors);
				await page.getByRole("link", { name: "Back to history" }).click();
				await expect(page).toHaveURL(new RegExp(`${escapeRegExp(entryPath)}/history$`));
				await page.getByRole("link", { name: "Back to Entry" }).click();
				await expect(page).toHaveURL(new RegExp(`${escapeRegExp(entryPath)}$`));
				await expectAppHealthy(page, consoleErrors);

				await page.locator(`nav a[href="/spaces/${spaceId}/forms"]`).first().click();
				await expect(page).toHaveURL(new RegExp(`${escapeRegExp(`/spaces/${spaceId}/forms`)}$`));
				await expectAppHealthy(page, consoleErrors);
				await page.getByRole("button", { name: "Entry", exact: true }).click();
				await expect(page).toHaveURL(new RegExp(`${escapeRegExp(formEntriesPath)}$`));
				await expectAppHealthy(page, consoleErrors);
				await row.getByRole("button", { name: "Open entry" }).click();
				await expect(page).toHaveURL(new RegExp(`${escapeRegExp(entryPath)}$`));
				await expectAppHealthy(page, consoleErrors);

				await page.goBack();
				await expect(page).toHaveURL(new RegExp(`${escapeRegExp(formEntriesPath)}$`));
				await expectAppHealthy(page, consoleErrors);
				await page.goForward();
				await expect(page).toHaveURL(new RegExp(`${escapeRegExp(entryPath)}$`));
				await expectAppHealthy(page, consoleErrors);
				expect(await page.evaluate(() => performance.timeOrigin)).toBe(documentTimeOrigin);
			}

			// Vite's dynamic-import preloader emits this event when a route chunk
			// fetch fails. Verify the client recovers through one reload and that a
			// repeat failure is allowed to reach the app boundary without looping.
			const routeBeforeRecovery = page.url();
			const recoveryReload = page.waitForNavigation({
				waitUntil: "domcontentloaded",
			});
			await page.evaluate(() => {
				const event = new Event("vite:preloadError", { cancelable: true });
				Object.assign(event, { payload: new Error("temporary route chunk failure") });
				window.dispatchEvent(event);
			});
			await recoveryReload;
			expect(page.url()).toBe(routeBeforeRecovery);
			await expectAppHealthy(page, consoleErrors);
			const repeatedRecoveryPrevented = await page.evaluate(() => {
				const event = new Event("vite:preloadError", { cancelable: true });
				Object.assign(event, { payload: new Error("temporary route chunk failure") });
				window.dispatchEvent(event);
				return event.defaultPrevented;
			});
			expect(repeatedRecoveryPrevented).toBe(false);

			expect(consoleErrors, `console errors: ${consoleErrors.join("\n")}`).toEqual([]);
			expect(runtimeErrors, `runtime errors: ${runtimeErrors.join("\n")}`).toEqual([]);
		} finally {
			await request.delete(getBackendUrl(`/spaces/${spaceId}/entries/${created.id}`));
		}
	});
});

test.describe("Manifest-discovered route chunk recovery", () => {
	test.use({ serviceWorkers: "block" });

	let spaceId = "";

	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
		spaceId = await getDefaultSpaceId(request);
		await ensureDefaultForm(request, spaceId);
	});

	test("REQ-E2E-004: recovers after a failed route chunk request", async ({ page }) => {
		test.setTimeout(60_000);
		await page.goto(`/spaces/${spaceId}/forms`, { waitUntil: "load" });
		await settleUiLoading(page);

		const routeChunkUrl = await findRouteChunkUrl(
			page,
			"src/routes/spaces/[space_id]/forms/[form_ref]/entries.tsx",
		);
		let chunkRequests = 0;
		await page.route(routeChunkUrl, async (route) => {
			chunkRequests += 1;
			if (chunkRequests === 1) {
				await route.abort("failed");
				return;
			}
			await route.continue();
		});

		const targetPath = `/spaces/${spaceId}/forms/Entry/entries`;
		const reload = page.waitForEvent("load");
		await page.getByRole("button", { name: "Entry", exact: true }).click();
		await reload;

		await expect(page).toHaveURL(new RegExp(`${escapeRegExp(targetPath)}$`));
		await expect(page.locator(".entriesPage h1")).toHaveText("Entry");
		await expectAppHealthy(page);
		expect(chunkRequests).toBe(2);
	});

	test("REQ-E2E-004: bounds retries when the real route chunk keeps failing", async ({ page }) => {
		test.setTimeout(60_000);
		await page.goto(`/spaces/${spaceId}/forms`, { waitUntil: "load" });
		await settleUiLoading(page);

		const routeChunkUrl = await findRouteChunkUrl(
			page,
			"src/routes/spaces/[space_id]/forms/[form_ref]/entries.tsx",
		);
		let chunkRequests = 0;
		await page.route(routeChunkUrl, async (route) => {
			chunkRequests += 1;
			await route.abort("failed");
		});

		const targetPath = `/spaces/${spaceId}/forms/Entry/entries`;
		const reload = page.waitForEvent("load");
		await page.getByRole("button", { name: "Entry", exact: true }).click();
		await reload;

		await expect(page).toHaveURL(new RegExp(`${escapeRegExp(targetPath)}$`));
		await expect(
			page.getByText(/This page could not be displayed|ページを表示できませんでした/),
		).toBeVisible();
		await expect.poll(() => chunkRequests).toBe(2);
		await page.waitForTimeout(300);
		expect(chunkRequests).toBe(2);
	});
});

test.describe("PWA update lifecycle", () => {
	let spaceId = "";

	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
		spaceId = await getDefaultSpaceId(request);
		await ensureDefaultForm(request, spaceId);
	});

	test("REQ-E2E-010: a new production worker waits while an open client traverses a lazy route", async ({ page }) => {
		test.setTimeout(60_000);

		const formsPath = `/spaces/${spaceId}/forms`;
		await page.goto(formsPath, { waitUntil: "load" });
		await settleUiLoading(page);

		await page.waitForFunction(async () => {
			const registration = await navigator.serviceWorker.getRegistration();
			return registration?.active?.state === "activated";
		});
		if (!(await page.evaluate(() => !!navigator.serviceWorker.controller))) {
			await page.reload({ waitUntil: "load" });
		}
		await page.waitForFunction(() => !!navigator.serviceWorker.controller);

		const activeClient = await page.evaluate(async () => {
			const registration = await navigator.serviceWorker.ready;
			if (!registration.active || !navigator.serviceWorker.controller) {
				throw new Error("Expected the page to have an active service worker");
			}

			const activeScript = registration.active.scriptURL;
			const updateUrl = new URL(activeScript);
			updateUrl.searchParams.set("pwa-update-test", "next");
			await navigator.serviceWorker.register(updateUrl.href, {
				scope: `${location.origin}/`,
			});

			return {
				activeScript,
				controllerScript: navigator.serviceWorker.controller.scriptURL,
				timeOrigin: performance.timeOrigin,
			};
		});

		await page.waitForFunction(async () => {
			const registration = await navigator.serviceWorker.getRegistration();
			return registration?.waiting?.state === "installed";
		}, undefined, { timeout: 15_000 });

		const waitingState = await page.evaluate(async () => {
			const registration = await navigator.serviceWorker.getRegistration();
			return {
				waiting: registration?.waiting?.state === "installed",
				activeScript: registration?.active?.scriptURL,
				controllerScript: navigator.serviceWorker.controller?.scriptURL,
				timeOrigin: performance.timeOrigin,
			};
		});
		expect(waitingState.waiting).toBe(true);
		expect(waitingState.activeScript).toBe(activeClient.activeScript);
		expect(waitingState.controllerScript).toBe(activeClient.controllerScript);
		expect(waitingState.timeOrigin).toBe(activeClient.timeOrigin);

		const targetPath = `/spaces/${spaceId}/forms/Entry/entries`;
		await page.getByRole("button", { name: "Entry", exact: true }).click();
		await expect(page).toHaveURL(new RegExp(`${escapeRegExp(targetPath)}$`));
		await expect(page.locator(".entriesPage h1")).toHaveText("Entry");
		await expectAppHealthy(page);

		const afterNavigation = await page.evaluate(async () => {
			const registration = await navigator.serviceWorker.getRegistration();
			return {
				waiting: registration?.waiting?.state === "installed",
				controllerScript: navigator.serviceWorker.controller?.scriptURL,
				timeOrigin: performance.timeOrigin,
			};
		});
		expect(afterNavigation.waiting).toBe(true);
		expect(afterNavigation.controllerScript).toBe(activeClient.controllerScript);
		expect(afterNavigation.timeOrigin).toBe(activeClient.timeOrigin);
	});
});

type InternalLink = {
	path: string;
	href: string;
};

async function visitPath(page: Page, path: string, consoleErrors: string[]): Promise<void> {
	await page.goto(path, { waitUntil: "domcontentloaded" });
	await expect(page.locator("body")).toBeVisible();
	await settleUiLoading(page);
	await expectAppHealthy(page, consoleErrors);
	await expect(page.locator("body")).not.toContainText("Visit solidjs.com");
	await expect(page.locator("body")).not.toContainText("NOT FOUND");
}

async function expectAppHealthy(page: Page, consoleErrors: string[] = []): Promise<void> {
	await expect(
		page.getByText(/This page could not be displayed|ページを表示できませんでした/),
		`App fallback rendered. Browser console: ${consoleErrors.join("\n")}`,
	).toHaveCount(0);
	await expect(page.locator("body")).not.toContainText("Visit solidjs.com");
	await expect(page.locator("body")).not.toContainText("NOT FOUND");
}

function escapeRegExp(value: string): string {
	return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

async function findRouteChunkUrl(page: Page, manifestKeyPart: string): Promise<string> {
	const output = await page.evaluate((keyPart) => {
		const manifest = (window as Window & {
			manifest?: Record<string, { output?: string }>;
		}).manifest;
		return Object.entries(manifest ?? {}).find(([key, asset]) =>
			key.includes(keyPart) && asset.output?.endsWith(".js")
		)?.[1].output ?? null;
	}, manifestKeyPart);

	expect(output, `the build manifest contains ${manifestKeyPart}`).toBeTruthy();
	return new URL(output!, page.url()).href;
}

async function settleUiLoading(page: Page): Promise<void> {
	await page.waitForTimeout(150);
	await page
		.waitForFunction(() => !document.querySelector(".ui-loading-bar"), undefined, {
			timeout: 5_000,
		})
		.catch(() => undefined);
	await page.waitForTimeout(150);
}

async function collectInternalLinks(page: Page, currentSpaceId: string): Promise<InternalLink[]> {
	const allowedPrefixes = [`/spaces/${currentSpaceId}`, "/spaces"];
	const links = await page.evaluate(() => {
		return Array.from(document.querySelectorAll("a[href]"))
			.map((anchor) => anchor.getAttribute("href") ?? "")
			.filter((href) => href.length > 0);
	});

	const normalized = new Map<string, string>();
	for (const href of links) {
		if (href.startsWith("#")) {
			continue;
		}
		try {
			const url = new URL(href, page.url());
			const path = url.pathname;
			if (
				path === "/" ||
				allowedPrefixes.some((prefix) => path.startsWith(prefix))
			) {
				normalized.set(path, href);
			}
		} catch {
			continue;
		}
	}

	return Array.from(normalized.entries()).map(([path, href]) => ({ path, href }));
}
