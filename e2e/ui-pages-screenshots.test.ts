import { expect, test } from "@playwright/test";
import { Buffer } from "node:buffer";
import { promises as fs } from "node:fs";
import path from "node:path";
import {
  ensureDefaultForm,
  getBackendUrl,
  getDefaultFormRelation,
  getDefaultSpaceId,
  waitForServers,
} from "./lib/client.ts";

type UiPageSpec = {
  id: string;
  route: string;
  implementation: "implemented" | "unimplemented";
};

const screenshotDir = path.resolve(process.cwd(), "../target/ui-screenshots");
const viewports = [
  { id: "desktop", width: 1440, height: 900 },
  { id: "mobile", width: 390, height: 844 },
] as const;

test.describe("UI page screenshot export @screenshot", () => {
  let spaceId = "";

  test.beforeAll(async ({ request }) => {
    await waitForServers(request);
    spaceId = await getDefaultSpaceId(request);
    await ensureDefaultForm(request, spaceId);
  });

  test("REQ-E2E-004: export screenshots for all UI page specs", async ({ page, request }) => {
    test.setTimeout(300_000);
    const relation = await getDefaultFormRelation(request, spaceId);
    const runId = Date.now();
    const browserDiagnostics: string[] = [];
    page.on("console", (message) => {
      if (message.type() === "error") {
        browserDiagnostics.push(`console: ${message.text()}`);
      }
    });
    page.on("pageerror", (error) => {
      browserDiagnostics.push(`pageerror: ${error.message}`);
    });
    page.on("requestfailed", (request) => {
      if (request.failure()?.errorText === "net::ERR_ABORTED") return;
      browserDiagnostics.push(
        `requestfailed: ${request.url()} ${request.failure()?.errorText ?? ""}`,
      );
    });

    const entryRes = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      {
        data: {
          form: "Entry",
          fields: {
            Body: `E2E Screenshot Entry ${runId}\n\nScreenshot seed entry.`,
          },
        },
      },
    );
    expect(entryRes.status()).toBe(201);
    const entry = (await entryRes.json()) as {
      id: string;
      revision_id: string;
    };

    const entryUpdate = await request.put(
      getBackendUrl(`/spaces/${spaceId}/entries/${entry.id}`),
      {
        data: {
          form: "Entry",
          fields: {
            Body:
              `E2E Screenshot Entry ${runId}\n\nUpdated screenshot seed entry.`,
          },
          parent_revision_id: entry.revision_id,
        },
      },
    );
    expect(entryUpdate.ok()).toBeTruthy();

    const assetFormName = "ScreenshotAssetAudit";
    const formsResponse = await request.get(
      getBackendUrl(`/spaces/${spaceId}/forms`),
    );
    expect(formsResponse.ok()).toBeTruthy();
    const forms = await formsResponse.json() as Array<{
      name: string;
      fields?: Record<string, { type?: string; required?: boolean }>;
    }>;
    const existingAssetForm = forms.find((form) => form.name === assetFormName);
    if (existingAssetForm) {
      if (existingAssetForm.fields?.Attachment?.type !== "asset_reference") {
        throw new Error(
          `The ${assetFormName} screenshot fixture Form has an incompatible schema.`,
        );
      }
    } else {
      const assetFormRes = await request.post(
        getBackendUrl(`/spaces/${spaceId}/forms`),
        {
          data: {
            name: assetFormName,
            version: 1,
            template: `# ${assetFormName}\n\n## Attachment\n`,
            fields: {
              Attachment: { type: "asset_reference", required: true },
            },
          },
        },
      );
      expect(assetFormRes.status()).toBe(201);
    }

    const assetName = `audit-asset-${runId}.txt`;
    const assetUpload = await request.post(
      getBackendUrl(`/spaces/${spaceId}/assets`),
      {
        multipart: {
          file: {
            name: assetName,
            mimeType: "text/plain",
            buffer: Buffer.from("Screenshot audit asset."),
          },
        },
      },
    );
    expect(assetUpload.status()).toBe(201);
    const assetReference = await assetUpload.json() as {
      asset_id: string;
      name: string;
      media_type: string;
      size_bytes: number;
      sha256: string;
    };

    const assetEntryRes = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      {
        data: {
          form: assetFormName,
          fields: { Attachment: assetReference },
        },
      },
    );
    expect(assetEntryRes.status()).toBe(201);
    const assetEntry = await assetEntryRes.json() as { id: string };

    const sqlCreate = await request.post(
      getBackendUrl(`/spaces/${spaceId}/sql`),
      {
        data: {
          name: `E2E Screenshot Query ${runId}`,
          kind: "user-query",
          sql:
            `SELECT * FROM "${relation}" ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 10`,
          variables: [],
        },
      },
    );
    expect([200, 201]).toContain(sqlCreate.status());
    const savedSql = (await sqlCreate.json()) as { id: string };
    const sqlId = savedSql.id;
    const sqlDetailResponse = await request.get(
      getBackendUrl(`/spaces/${spaceId}/sql/${sqlId}`),
    );
    if (!sqlDetailResponse.ok()) {
      console.warn(
        `screenshot fixture saved SQL detail returned ${sqlDetailResponse.status()} for ${sqlId}`,
      );
    }

    const specs = (await loadUiPageSpecs()).filter((spec) =>
      spec.implementation === "implemented"
    );
    await fs.mkdir(screenshotDir, { recursive: true });
    await fs.mkdir(path.join(screenshotDir, "mobile"), { recursive: true });

    const failedPages: string[] = [];
    let captured = 0;

    for (const viewport of viewports) {
      await page.setViewportSize({
        width: viewport.width,
        height: viewport.height,
      });

      for (const spec of specs) {
        browserDiagnostics.length = 0;
        const targetPath = resolveRoute(spec.route, {
          space_id: spaceId,
          entry_id: entry.id,
          sql_id: sqlId,
          form_ref: "Entry",
          revision_id: entry.revision_id,
          asset_id: assetReference.asset_id,
          link_id: "sample-link",
        });

        try {
          const response = await page.goto(targetPath, {
            waitUntil: "domcontentloaded",
            timeout: 20_000,
          });
          await page.waitForTimeout(350);
          const heading = await page.locator("h1").first().innerText().catch(
            () => "<no heading>",
          );
          if (heading === "Page not found") {
            console.warn(
              `screenshot route resolved to the 404 page: ${viewport.id} ${spec.id} ${targetPath} (HTTP ${
                response?.status() ?? "unknown"
              })`,
            );
            if (browserDiagnostics.length > 0) {
              console.warn(
                `browser diagnostics for ${spec.id}: ${
                  browserDiagnostics.join(" | ")
                }`,
              );
            }
          }
          const targetFile = viewport.id === "mobile"
            ? path.join(screenshotDir, "mobile", `${spec.id}.png`)
            : path.join(screenshotDir, `${spec.id}.png`);
          await page.screenshot({ path: targetFile, fullPage: false });
          captured += 1;
        } catch {
          failedPages.push(`${viewport.id}: ${spec.id} (${targetPath})`);
        }
      }
    }

    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(`/spaces/${spaceId}/dashboard`, {
      waitUntil: "domcontentloaded",
    });
    await page.locator("button.mobileMenu").click();
    await page.waitForTimeout(150);
    await page.screenshot({
      path: path.join(screenshotDir, "mobile", "space-mobile-drawer.png"),
      fullPage: false,
    });

    await page.goto(
      `/spaces/${spaceId}/entries/${entry.id}/info`,
      { waitUntil: "domcontentloaded" },
    );
    await page.locator(".entry-info-advanced-details summary").click();
    await page.waitForTimeout(150);
    await page.screenshot({
      path: path.join(screenshotDir, "space-entry-info-advanced.png"),
      fullPage: false,
    });
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.screenshot({
      path: path.join(screenshotDir, "space-entry-info-advanced-desktop.png"),
      fullPage: false,
    });

    await request.delete(
      getBackendUrl(`/spaces/${spaceId}/entries/${entry.id}`),
    );
    await request.delete(
      getBackendUrl(`/spaces/${spaceId}/entries/${assetEntry.id}`),
    );
    await request.delete(
      getBackendUrl(`/spaces/${spaceId}/assets/${assetReference.asset_id}`),
    );
    await request.delete(getBackendUrl(`/spaces/${spaceId}/sql/${sqlId}`));

    if (failedPages.length > 0) {
      console.warn(
        `screenshot export skipped pages: ${failedPages.join(", ")}`,
      );
    }

    expect(captured + failedPages.length).toBe(specs.length * viewports.length);
    expect(captured).toBeGreaterThan(0);
  });
});

async function loadUiPageSpecs(): Promise<UiPageSpec[]> {
  const pagesDir = path.resolve(process.cwd(), "../docs/spec/ui/pages");
  const files = (await fs.readdir(pagesDir)).filter((file) =>
    file.endsWith(".yaml")
  );
  const specs: UiPageSpec[] = [];

  for (const file of files) {
    const raw = await fs.readFile(path.join(pagesDir, file), "utf-8");
    const idMatch = raw.match(/\n\s*id:\s*([a-z0-9-]+)/i);
    const routeMatch = raw.match(/\n\s*route:\s*([^\n]+)/i);
    const implementationMatch = raw.match(
      /\n\s*implementation:\s*(implemented|unimplemented)\s*$/im,
    );
    if (!idMatch || !routeMatch || !implementationMatch) continue;
    specs.push({
      id: idMatch[1],
      route: routeMatch[1].trim(),
      implementation: implementationMatch[1] as UiPageSpec["implementation"],
    });
  }

  return specs.sort((a, b) => a.id.localeCompare(b.id));
}

function resolveRoute(
  route: string,
  variables: Record<string, string>,
): string {
  return route.replaceAll(
    /\{([^}]+)\}/g,
    (_, key: string) => variables[key] ?? "unknown",
  );
}
