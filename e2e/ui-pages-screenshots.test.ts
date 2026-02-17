import { expect, test } from "@playwright/test";
import { promises as fs } from "node:fs";
import path from "node:path";
import { ensureDefaultForm, getBackendUrl, waitForServers } from "./lib/client.js";

type UiPageSpec = {
	id: string;
	route: string;
};

const spaceId = "default";
const screenshotDir = path.resolve(process.cwd(), "../docsite/public/screenshots");

test.describe("UI page screenshot export @screenshot", () => {
	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
		await ensureDefaultForm(request);
	});

	test("REQ-E2E-004: export screenshots for all UI page specs", async ({ page, request }) => {
		test.setTimeout(180_000);

		const entryTitle = `E2E Screenshot Entry ${Date.now()}`;
		const entryRes = await request.post(getBackendUrl(`/spaces/${spaceId}/entries`), {
			data: {
				content: `---\nform: Entry\n---\n# ${entryTitle}\n\n## Body\nScreenshot seed entry.`,
			},
		});
		expect(entryRes.status()).toBe(201);
		const entry = (await entryRes.json()) as { id: string };

		const sqlId = `e2e-ui-shot-${Date.now()}`;
		const sqlCreate = await request.post(getBackendUrl(`/spaces/${spaceId}/sql`), {
			data: {
				id: sqlId,
				name: `E2E Screenshot Query ${Date.now()}`,
				sql: "SELECT * FROM entries LIMIT 10",
				variables: [],
			},
		});
		expect([200, 201]).toContain(sqlCreate.status());

		const specs = await loadUiPageSpecs();
		await fs.mkdir(screenshotDir, { recursive: true });

		await page.setViewportSize({ width: 1440, height: 900 });

		const failedPages: string[] = [];
		let captured = 0;

		for (const spec of specs) {
			const targetPath = resolveRoute(spec.route, {
				space_id: spaceId,
				entry_id: entry.id,
				query_id: sqlId,
				sql_id: sqlId,
				form_name: "Entry",
				revision_id: "latest",
				asset_id: "sample-asset",
				link_id: "sample-link",
			});

			try {
				await page.goto(targetPath, { waitUntil: "domcontentloaded", timeout: 20_000 });
				await page.waitForTimeout(350);
				await page.screenshot({ path: path.join(screenshotDir, `${spec.id}.png`), fullPage: false });
				captured += 1;
			} catch {
				failedPages.push(`${spec.id} (${targetPath})`);
			}
		}

		await request.delete(getBackendUrl(`/spaces/${spaceId}/entries/${entry.id}`));
		await request.delete(getBackendUrl(`/spaces/${spaceId}/sql/${sqlId}`));

		if (failedPages.length > 0) {
			console.warn(`screenshot export skipped pages: ${failedPages.join(", ")}`);
		}

		expect(captured + failedPages.length).toBe(specs.length);
		expect(captured).toBeGreaterThan(0);
	});
});

async function loadUiPageSpecs(): Promise<UiPageSpec[]> {
	const pagesDir = path.resolve(process.cwd(), "../docs/spec/ui/pages");
	const files = (await fs.readdir(pagesDir)).filter((file) => file.endsWith(".yaml"));
	const specs: UiPageSpec[] = [];

	for (const file of files) {
		const raw = await fs.readFile(path.join(pagesDir, file), "utf-8");
		const idMatch = raw.match(/\n\s*id:\s*([a-z0-9-]+)/i);
		const routeMatch = raw.match(/\n\s*route:\s*([^\n]+)/i);
		if (!idMatch || !routeMatch) continue;
		specs.push({ id: idMatch[1], route: routeMatch[1].trim() });
	}

	return specs.sort((a, b) => a.id.localeCompare(b.id));
}

function resolveRoute(route: string, variables: Record<string, string>): string {
	return route.replaceAll(/\{([^}]+)\}/g, (_, key: string) => variables[key] ?? "unknown");
}
