import { fileURLToPath } from "node:url";
import { expect, test, type Page } from "@playwright/test";
import { join } from "node:path";
import {
	ensureDefaultForm,
	getBackendUrl,
	getDefaultSpaceId,
	getFrontendUrl,
	waitForServers,
} from "./lib/client.ts";

const maxVisitedPages = 16;
const repositoryRoot = fileURLToPath(new URL("../", import.meta.url));
const staticPublicDirectory = join(repositoryRoot, "frontend/.output/public");

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

			// A completed same-URL recovery must not suppress an unrelated retry
			// after the user leaves and returns to the route. Browser error messages
			// can omit the failed chunk URL, so route identity is the safe boundary.
			await page.locator(`nav a[href="/spaces/${spaceId}/forms"]`).first().click();
			await expect(page).toHaveURL(new RegExp(`${escapeRegExp(`/spaces/${spaceId}/forms`)}$`));
			await page.getByRole("button", { name: "Entry", exact: true }).click();
			const formEntryRow = page.locator(`[data-entry-id="${created.id}"]`);
			await formEntryRow.getByRole("button", { name: "Open entry" }).click();
			await expect(page).toHaveURL(routeBeforeRecovery);
			await expectAppHealthy(page, consoleErrors);

			const routeRetryReload = page.waitForNavigation({
				waitUntil: "domcontentloaded",
			});
			const routeRetryPrevented = await page.evaluate(() => {
				const event = new Event("vite:preloadError", { cancelable: true });
				Object.assign(event, { payload: new Error("temporary route chunk failure") });
				window.dispatchEvent(event);
				return event.defaultPrevented;
			});
			expect(routeRetryPrevented).toBe(true);
			await routeRetryReload;
			await expect(page).toHaveURL(routeBeforeRecovery);
			await expectAppHealthy(page, consoleErrors);

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

	test("REQ-E2E-010: a root-scoped production worker keeps lazy navigation on the current client", async ({ page }) => {
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
			const controller = navigator.serviceWorker.controller;
			if (!registration.active || !controller) {
				throw new Error("Expected the page to have an active service worker");
			}

			return {
				scope: registration.scope,
				activeScript: registration.active.scriptURL,
				controllerScript: controller.scriptURL,
				timeOrigin: performance.timeOrigin,
			};
		});
		expect(new URL(activeClient.scope).pathname).toBe("/");
		expect(new URL(activeClient.activeScript).pathname).toBe("/_build/sw.js");
		expect(activeClient.controllerScript).toBe(activeClient.activeScript);
		await page.evaluate(() => {
			(window as Window & { __ugoiteInitialController?: ServiceWorker })
				.__ugoiteInitialController = navigator.serviceWorker.controller ?? undefined;
		});

		const routeChunkUrl = await findRouteChunkUrl(
			page,
			"src/routes/spaces/[space_id]/forms/[form_ref]/entries.tsx",
		);
		const routeChunkPath = new URL(routeChunkUrl).pathname.replace(/^\//, "");
		const cachedRouteChunk = await page.evaluate(
			async (url) => (await caches.match(url))?.status === 200,
			routeChunkUrl,
		);
		expect(cachedRouteChunk).toBe(true);
		expect(
			await page.evaluate((url) => performance.getEntriesByName(url).length > 0, routeChunkUrl),
		).toBe(false);

		const serviceWorkerPath = "_build/sw.js";
		const originalServiceWorker = await readStaticFile(serviceWorkerPath);
		const waitingUpdateWorker = `
self.addEventListener("install", () => {});
self.addEventListener("message", (event) => {
	if (event.data?.type === "SKIP_WAITING") event.waitUntil(self.skipWaiting());
});
self.addEventListener("activate", (event) => {
	event.waitUntil(self.clients.claim());
});
`;
		const unavailableRouteChunk = `${routeChunkPath}.unavailable-during-update`;
		let displacedRouteChunk = false;

		try {
			// Replace the unversioned worker response in the static deployment with
			// a second worker build. The fixture can accept the update message sent
			// by auto-update clients, but otherwise uses normal worker activation.
			await writeStaticFile(serviceWorkerPath, waitingUpdateWorker);
			await page.evaluate(async () => {
				const registration = await navigator.serviceWorker.ready;
				await registration.update();
			});
			await page.waitForFunction(async () => {
				const registration = await navigator.serviceWorker.getRegistration();
				return registration?.waiting?.state === "installed";
			});

			// The old worker has precached this not-yet-loaded route. Removing the
			// network copy proves the existing client can still use its own cache.
			await moveStaticFile(routeChunkPath, unavailableRouteChunk);
			displacedRouteChunk = true;
			const chunkResponsePromise = page.waitForResponse((response) =>
				response.url() === routeChunkUrl
			);
			const targetPath = `/spaces/${spaceId}/forms/Entry/entries`;
			await page.getByRole("button", { name: "Entry", exact: true }).click();
			const chunkResponse = await chunkResponsePromise;
			expect(chunkResponse.status()).toBe(200);
			expect(chunkResponse.fromServiceWorker()).toBe(true);
			await expect(page).toHaveURL(new RegExp(`${escapeRegExp(targetPath)}$`));
			await expect(page.locator(".entriesPage h1")).toHaveText("Entry");
			await expectAppHealthy(page);

			const afterNavigation = await page.evaluate(async () => {
				const registration = await navigator.serviceWorker.getRegistration();
				return {
					waiting: registration?.waiting?.state,
					scope: registration?.scope,
					activeScript: registration?.active?.scriptURL,
					controllerScript: navigator.serviceWorker.controller?.scriptURL,
					sameController: navigator.serviceWorker.controller ===
						(window as Window & { __ugoiteInitialController?: ServiceWorker })
							.__ugoiteInitialController,
					timeOrigin: performance.timeOrigin,
				};
			});
			expect(afterNavigation.waiting).toBe("installed");
			expect(afterNavigation.scope).toBe(activeClient.scope);
			expect(afterNavigation.activeScript).toBe(activeClient.activeScript);
			expect(afterNavigation.controllerScript).toBe(activeClient.controllerScript);
			expect(afterNavigation.sameController).toBe(true);
			expect(afterNavigation.timeOrigin).toBe(activeClient.timeOrigin);
		} finally {
			try {
				if (displacedRouteChunk) {
					await moveStaticFile(unavailableRouteChunk, routeChunkPath);
				}
			} finally {
				await writeStaticFile(serviceWorkerPath, originalServiceWorker);
			}
		}
	});

	test("REQ-E2E-010: a waiting update keeps a cached lazy route available to an open client", async ({ page }) => {
		test.setTimeout(60_000);

		const workerScriptPath = await findProductionWorkerScriptPath();
		const originalWorker = await fetch(getFrontendUrl(workerScriptPath));
		expect(originalWorker.ok).toBe(true);
		const originalWorkerSource = await originalWorker.text();
		await page.goto(`/spaces/${spaceId}/forms`, { waitUntil: "load" });
		await settleUiLoading(page);
		await page.waitForFunction(async () => {
			const registration = await navigator.serviceWorker.getRegistration();
			return registration?.active?.state === "activated";
		});
		if (!(await page.evaluate(() => !!navigator.serviceWorker.controller))) {
			await page.reload({ waitUntil: "load" });
		}
		await page.waitForFunction(() => !!navigator.serviceWorker.controller);

		const initialClient = await page.evaluate(async () => {
			const registration = await navigator.serviceWorker.ready;
			const controller = navigator.serviceWorker.controller;
			if (!registration.active || !controller) {
				throw new Error("Expected an active worker to control the open client");
			}
			return {
				activeScript: registration.active.scriptURL,
				controllerScript: controller.scriptURL,
				timeOrigin: performance.timeOrigin,
			};
		});
		expect(new URL(initialClient.activeScript).pathname).toBe(workerScriptPath);

		// Serve a distinct worker at the same generated production URL while the
		// existing client stays controlled by its active production worker.
		const restoreWorker = await replaceServedStaticAsset(
			workerScriptPath,
			`${originalWorkerSource}\n// E2E updated worker version`,
			originalWorkerSource,
		);
		try {
			await page.evaluate(async () => {
				const registration = await navigator.serviceWorker.ready;
				await registration.update();
			});
			await expect.poll(() => page.evaluate(async () => {
				const registration = await navigator.serviceWorker.getRegistration();
				return registration?.waiting?.state ?? null;
			})).toBe("installed");
		} finally {
			await restoreWorker();
		}

		const routeChunkUrl = await findRouteChunkUrl(
			page,
			"src/routes/spaces/[space_id]/forms/[form_ref]/entries.tsx",
		);
		const chunkIsPrecached = await page.evaluate(async (url) =>
			!!(await caches.match(url, { ignoreSearch: true })), routeChunkUrl);
		expect(chunkIsPrecached).toBe(true);

		const originalChunkResponse = await page.request.get(routeChunkUrl);
		expect(originalChunkResponse.ok()).toBe(true);
		const routeChunkPath = new URL(routeChunkUrl).pathname;
		const unavailableChunkBody = "throw new Error('E2E route chunk unavailable');";
		const restoreRouteChunk = await replaceServedStaticAsset(
			routeChunkPath,
			unavailableChunkBody,
			await originalChunkResponse.text(),
		);
		try {
			const unavailableResponse = await page.request.get(routeChunkUrl);
			expect(await unavailableResponse.text()).toBe(unavailableChunkBody);
			await page.getByRole("button", { name: "Entry", exact: true }).click();
			await expect(page.locator(".entriesPage h1")).toHaveText("Entry");
			await expectAppHealthy(page);

			const afterNavigation = await page.evaluate(async () => {
				const registration = await navigator.serviceWorker.ready;
				return {
					activeScript: registration.active?.scriptURL,
					waitingState: registration.waiting?.state,
					controllerScript: navigator.serviceWorker.controller?.scriptURL,
					timeOrigin: performance.timeOrigin,
				};
			});
			expect(afterNavigation.activeScript).toBe(initialClient.activeScript);
			expect(afterNavigation.waitingState).toBe("installed");
			expect(afterNavigation.controllerScript).toBe(initialClient.controllerScript);
			expect(afterNavigation.timeOrigin).toBe(initialClient.timeOrigin);
		} finally {
			await restoreRouteChunk();
		}
	});
});

async function findProductionWorkerScriptPath(): Promise<string> {
	const registerScript = await fetch(getFrontendUrl("/_build/registerSW.js"));
	if (!registerScript.ok) {
		throw new Error("Production service worker registration script is unavailable");
	}
	const match = (await registerScript.text()).match(
		/serviceWorker\.register\(["']([^"']+)["']\s*,/,
	);
	if (!match) throw new Error("Could not find the generated production worker URL");
	const workerUrl = new URL(match[1], getFrontendUrl("/"));
	if (
		workerUrl.origin !== new URL(getFrontendUrl("/")).origin ||
		!workerUrl.pathname.startsWith("/_build/")
	) {
		throw new Error("Generated production worker URL is outside the expected build path");
	}
	return workerUrl.pathname;
}

async function replaceServedStaticAsset(
	assetPath: string,
	updatedSource: string,
	originalSource: string,
): Promise<() => Promise<void>> {
	if (!assetPath.startsWith("/_build/") || assetPath.includes("..")) {
		throw new Error("E2E static asset path is outside the generated build directory");
	}
	const localStaticDirectory = Deno.env.get("E2E_STATIC_DIR");
	const staticContainerId = Deno.env.get("E2E_STATIC_CONTAINER_ID");
	const containerStaticDirectory = Deno.env.get("E2E_STATIC_CONTAINER_DIR");
	if (
		(!localStaticDirectory && !staticContainerId) ||
		(staticContainerId && !containerStaticDirectory)
	) {
		throw new Error("E2E runner did not expose a writable static asset location");
	}

	const temporaryFile = await Deno.makeTempFile({ suffix: ".js" });
	const containerPath = staticContainerId
		? `${staticContainerId}:${containerStaticDirectory}${assetPath}`
		: null;
	const writeServedAsset = async (source: string) => {
		if (localStaticDirectory) {
			await Deno.writeTextFile(join(localStaticDirectory, assetPath.slice(1)), source);
			return;
		}
		await Deno.writeTextFile(temporaryFile, source);
		await Deno.chmod(temporaryFile, 0o644);
		const result = await new Deno.Command("docker", {
			args: ["cp", temporaryFile, containerPath!],
			stdout: "null",
			stderr: "null",
		}).output();
		if (!result.success) throw new Error("Could not update the E2E static asset");
	};

	let restored = false;
	try {
		await writeServedAsset(updatedSource);
	} catch (error) {
		await writeServedAsset(originalSource).catch(() => {});
		await Deno.remove(temporaryFile).catch(() => {});
		throw error;
	}
	return async () => {
		if (restored) return;
		try {
			await writeServedAsset(originalSource);
			restored = true;
		} finally {
			await Deno.remove(temporaryFile).catch(() => {});
		}
	};
}

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

async function readStaticFile(relativePath: string): Promise<Uint8Array> {
	const containerId = Deno.env.get("UGOITE_E2E_STATIC_CONTAINER_ID")?.trim();
	if (!containerId) {
		return await Deno.readFile(join(staticPublicDirectory, relativePath));
	}

	const temporaryFile = await Deno.makeTempFile({ prefix: "ugoite-static-read-" });
	try {
		await runDocker(["cp", `${containerId}:/app/static/${relativePath}`, temporaryFile]);
		return await Deno.readFile(temporaryFile);
	} finally {
		await Deno.remove(temporaryFile);
	}
}

async function writeStaticFile(
	relativePath: string,
	contents: string | Uint8Array,
): Promise<void> {
	const bytes = typeof contents === "string"
		? new TextEncoder().encode(contents)
		: contents;
	const containerId = Deno.env.get("UGOITE_E2E_STATIC_CONTAINER_ID")?.trim();
	if (!containerId) {
		await Deno.writeFile(join(staticPublicDirectory, relativePath), bytes);
		return;
	}

	const temporaryFile = await Deno.makeTempFile({ prefix: "ugoite-static-write-" });
	try {
		await Deno.writeFile(temporaryFile, bytes);
		await Deno.chmod(temporaryFile, 0o644);
		await runDocker([
			"cp",
			temporaryFile,
			`${containerId}:/app/static/${relativePath}`,
		]);
	} finally {
		await Deno.remove(temporaryFile);
	}
}

async function moveStaticFile(from: string, to: string): Promise<void> {
	const containerId = Deno.env.get("UGOITE_E2E_STATIC_CONTAINER_ID")?.trim();
	if (!containerId) {
		await Deno.rename(
			join(staticPublicDirectory, from),
			join(staticPublicDirectory, to),
		);
		return;
	}

	await runDocker([
		"exec",
		"--user",
		"0:0",
		containerId,
		"mv",
		`/app/static/${from}`,
		`/app/static/${to}`,
	]);
}

async function runDocker(args: string[]): Promise<void> {
	const result = await new Deno.Command("docker", {
		args,
		stderr: "piped",
	}).output();
	if (result.code !== 0) {
		const details = new TextDecoder().decode(result.stderr).trim();
		throw new Error(`Docker command failed${details ? `: ${details}` : ""}`);
	}
}
