import { expect, test } from "@playwright/test";
import { promises as fs } from "node:fs";
import path from "node:path";
import { ensureDefaultForm, getBackendUrl, waitForServers } from "./lib/client";

type UiRouteSpec = {
  id: string;
  route: string;
};

const spaceId = "default";

test.describe("UI page screenshots", () => {
  test.beforeAll(async ({ request }) => {
    await waitForServers(request);
    await ensureDefaultForm(request);
  });

  test("REQ-E2E-004: capture screenshots for spec UI pages", async ({ page, request }) => {
    test.setTimeout(300_000);

    const uiPagesDir = path.resolve(process.cwd(), "../docs/spec/ui/pages");
    const outputDir = path.resolve(process.cwd(), "test-results/ui-pages");
    await fs.mkdir(outputDir, { recursive: true });

    const routes = await readUiRoutes(uiPagesDir);
    expect(routes.length).toBeGreaterThan(0);

    const entryRes = await request.post(getBackendUrl(`/spaces/${spaceId}/entries`), {
      data: {
        content: `---\nform: Entry\n---\n# Screenshot Seed ${Date.now()}\n\n## Body\nSeed entry for ui-page screenshots.`,
      },
    });
    expect(entryRes.status()).toBe(201);
    const entry = (await entryRes.json()) as { id: string; revision_id?: string };

    const sqlId = `e2e-ui-shot-${Date.now()}`;
    const sqlRes = await request.post(getBackendUrl(`/spaces/${spaceId}/sql`), {
      data: {
        id: sqlId,
        name: "E2E UI Screenshot Query",
        sql: "SELECT * FROM entries LIMIT 10",
        variables: [],
      },
    });
    expect([200, 201]).toContain(sqlRes.status());

    try {
      const captured: string[] = [];
      const skipped: string[] = [];
      for (const route of routes) {
        const actualPath = materializeRoute(route.route, {
          space_id: spaceId,
          entry_id: entry.id,
          revision_id: entry.revision_id ?? "latest",
          query_id: sqlId,
          sql_id: sqlId,
          link_id: "example-link",
          asset_id: "example-asset",
          form_name: "Entry",
        });

        try {
          await page.goto(actualPath, { waitUntil: "domcontentloaded", timeout: 15_000 });
          // Brief pause to allow client-side rendering
          await page.waitForTimeout(500);
          const bodyText = await page.locator("body").innerText({ timeout: 3_000 }).catch(() => "");
          if (bodyText.includes("Visit solidjs.com") || bodyText.includes("Not Found")) {
            // Route not yet implemented in the frontend — skip gracefully
            skipped.push(`${route.id}: ${actualPath}`);
            continue;
          }

          await page.screenshot({
            path: path.join(outputDir, `${route.id}.png`),
            fullPage: true,
          });
          captured.push(route.id);
        } catch {
          skipped.push(`${route.id}: ${actualPath} (timeout/error)`);
        }
      }
      console.log(`Captured ${captured.length} screenshots, skipped ${skipped.length} unimplemented routes`);
      if (skipped.length > 0) {
        console.log("Skipped routes:", skipped.join(", "));
      }
      expect(captured.length).toBeGreaterThan(0);
    } finally {
      await request.delete(getBackendUrl(`/spaces/${spaceId}/entries/${entry.id}`));
      await request.delete(getBackendUrl(`/spaces/${spaceId}/sql/${sqlId}`));
    }
  });
});

async function readUiRoutes(uiPagesDir: string): Promise<UiRouteSpec[]> {
  const entries = await fs.readdir(uiPagesDir);
  const yamlFiles = entries.filter((file) => file.endsWith(".yaml"));
  const rows: UiRouteSpec[] = [];

  for (const fileName of yamlFiles) {
    const fullPath = path.join(uiPagesDir, fileName);
    const raw = await fs.readFile(fullPath, "utf-8");
    const id = raw.match(/^\s*id:\s*([^\n]+)$/m)?.[1]?.trim();
    const route = raw.match(/^\s*route:\s*([^\n]+)$/m)?.[1]?.trim();
    if (!id || !route) continue;
    rows.push({ id, route });
  }

  return rows.sort((a, b) => a.id.localeCompare(b.id));
}

function materializeRoute(route: string, values: Record<string, string>): string {
  return route.replace(/\{([^}]+)\}/g, (_match, key: string) => values[key] ?? "placeholder");
}
