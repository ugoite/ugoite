import { expect, test, type Page } from "@playwright/test";
import { ensureDefaultForm, getBackendUrl, waitForServers } from "./lib/client";

const spaceId = "default";
const maxVisitedPages = 16;

test.describe("Dynamic navigation traversal", () => {
	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
		await ensureDefaultForm(request);
	});

	test("REQ-E2E-004: dynamic traversal has no console or SolidJS errors", async ({
		page,
		request,
	}) => {
		test.setTimeout(120_000);

		const createdEntry = await request.post(getBackendUrl(`/spaces/${spaceId}/entries`), {
			data: {
				content: `---\nform: Entry\n---\n# E2E Dynamic Traversal ${Date.now()}\n\n## Body\nTraversal seed entry.`,
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

				await page.goto(path, { waitUntil: "networkidle" });
				visited.add(path);

				await expect(page.locator("body")).not.toContainText("Visit solidjs.com");
				await expect(page.locator("body")).not.toContainText("NOT FOUND");

				const discoveredLinks = await collectInternalLinks(page, spaceId);
				for (const discovered of discoveredLinks) {
					if (!visited.has(discovered.path) && !queue.includes(discovered.path)) {
						queue.push(discovered.path);
					}
				}

				const nextLink = discoveredLinks.find((candidate) => !visited.has(candidate.path));
				if (nextLink) {
					const link = page.locator(`a[href="${nextLink.href}"]`).first();
					if ((await link.count()) > 0 && (await link.isVisible())) {
						await link.click();
						await page.waitForLoadState("networkidle");
						await expect(page.locator("body")).not.toContainText("Visit solidjs.com");
						await expect(page.locator("body")).not.toContainText("NOT FOUND");
						visited.add(nextLink.path);
					}
				}

				await page.waitForTimeout(200);
			}

			expect(visited.size).toBeGreaterThanOrEqual(6);
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

async function collectInternalLinks(page: Page, currentSpaceId: string): Promise<InternalLink[]> {
	const allowedPrefixes = [`/spaces/${currentSpaceId}`, "/spaces", "/about"];
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
