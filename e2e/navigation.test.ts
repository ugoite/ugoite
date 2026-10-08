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
