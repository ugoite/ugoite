import {
  type APIRequestContext,
  type Browser,
  type BrowserContext,
  expect,
  type Page,
  test,
} from "@playwright/test";
import { getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client.ts";

const INITIAL_TOTAL = 20;
const UPDATED_TOTAL = 30;
const ENTRY_PAGE_LIMIT = 2;

type SeedState = {
  spaceId: string;
  formName: string;
  compositionName: string;
};

async function createSpace(request: APIRequestContext): Promise<string> {
  const suffix = `${Date.now()}-${crypto.randomUUID().slice(0, 8)}`;
  const response = await request.post(getBackendUrl("/spaces"), {
    data: {
      slug: `metric-studio-${suffix}`,
      name: `Metric Studio ${suffix}`,
    },
  });
  expect([200, 201]).toContain(response.status());
  const result = await response.json() as { space_uid?: string };
  expect(result.space_uid).toBeTruthy();
  return result.space_uid!;
}

async function addEntry(
  request: APIRequestContext,
  spaceId: string,
  formName: string,
  sequence: number,
  amount: number,
): Promise<void> {
  const response = await request.post(
    getBackendUrl(`/spaces/${spaceId}/entries`),
    {
      data: {
        form: formName,
        fields: {
          Amount: amount,
          Reference: `Metric row ${sequence.toString().padStart(3, "0")}`,
        },
      },
    },
  );
  expect(response.status()).toBe(201);
}

async function seedForm(request: APIRequestContext): Promise<SeedState> {
  const spaceId = await createSpace(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const formName = `MetricForm${suffix}`;
  const compositionName = `Amount total ${suffix}`;
  const formResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: {
        name: formName,
        version: 1,
        template: `# ${formName}\n\n## Amount\n\n## Reference\n`,
        fields: {
          Amount: { type: "integer", required: true },
          Reference: { type: "string", required: true },
        },
      },
    },
  );
  expect([200, 201]).toContain(formResponse.status());

  await addEntry(request, spaceId, formName, 1, 10);
  await addEntry(request, spaceId, formName, 2, 10);
  return { spaceId, formName, compositionName };
}

async function addMetricFromCanvas(
  page: Page,
  formName: string,
): Promise<void> {
  await page.getByRole("button", { name: "Add to design", exact: true })
    .first().click();
  const palette = page.getByRole("dialog", {
    name: "Add to design",
    exact: true,
  });
  await palette.getByRole("button", { name: "Data", exact: true }).click();

  const picker = page.getByRole("dialog", {
    name: "Add data component",
    exact: true,
  });
  await picker.getByRole("tab", { name: "Metric", exact: true }).click();
  await picker.getByRole("button", { name: formName, exact: true }).click();
  await picker.getByLabel("Operation", { exact: true }).selectOption("sum");

  const field = picker.getByLabel("Field", { exact: true });
  await expect.poll(() => field.locator("option").allTextContents()).toEqual(
    expect.arrayContaining(["Amount"]),
  );
  const fieldLabels = await field.locator("option").allTextContents();
  expectNoOpaqueFieldLabels(fieldLabels);
  await field.selectOption({ label: "Amount" });

  await picker.getByRole("button", { name: "Preview", exact: true }).click();
  await expect(picker.locator('output[aria-label="Metric result"]')).toHaveText(
    String(INITIAL_TOTAL),
  );
  await picker.getByRole("button", { name: "Add", exact: true }).click();
  await expect(page.getByRole("dialog")).toHaveCount(1);
  await page.getByRole("dialog").getByRole("button", {
    name: "Close",
    exact: true,
  }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
}

async function addTableFromCanvas(
  page: Page,
  formName: string,
): Promise<void> {
  await page.getByRole("button", { name: "Add to design", exact: true })
    .first().click();
  const palette = page.getByRole("dialog", {
    name: "Add to design",
    exact: true,
  });
  await palette.getByRole("button", { name: "Data", exact: true }).click();

  const picker = page.getByRole("dialog", {
    name: "Add data component",
    exact: true,
  });
  await picker.getByRole("tab", { name: "Table", exact: true }).click();
  await picker.getByRole("button", { name: formName, exact: true }).click();
  await expect(page.getByRole("dialog")).toHaveCount(1);
  await page.getByRole("dialog").getByRole("button", {
    name: "Close",
    exact: true,
  }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
}

async function openSavedComposition(
  page: Page,
  seed: SeedState,
): Promise<void> {
  await page.goto(getFrontendUrl(`/spaces/${seed.spaceId}/compositions`), {
    waitUntil: "domcontentloaded",
  });
  const link = page.getByRole("link", {
    name: new RegExp(seed.compositionName),
  });
  await expect(link).toBeVisible();
  await link.click();
  await expect(page).toHaveURL(
    new RegExp(`/spaces/${seed.spaceId}/compositions/[^/]+/[^/]+$`),
  );
  await expect(
    page.getByRole("heading", { level: 1, name: seed.compositionName }),
  ).toBeVisible();
}

async function openOwnerContext(
  browser: Browser,
  storageState: Awaited<ReturnType<BrowserContext["storageState"]>>,
) {
  return await browser.newContext({ storageState });
}

function expectNoOpaqueFieldLabels(labels: string[]) {
  const combinedLabels = labels.join("\n");
  expect(combinedLabels).not.toMatch(
    /(?:\bfield[_\s-]?\d+\b|#\s*\d+\b|\b[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\b)/i,
  );
  expect(labels.some((label) => /(?:\s|\()\d{2,}\)?$/.test(label))).toBe(
    false,
  );
}

test.describe("Composition Studio Metric authoring", () => {
  test("REQ-FE-070: saves and updates a Form SUM across pagination", async ({ browser, context, request }) => {
    test.setTimeout(180_000);
    await waitForServers(request);
    const seed = await seedForm(request);
    const storageState = await context.storageState();
    const ownerContext = await openOwnerContext(browser, storageState);

    try {
      const page = await ownerContext.newPage();
      await page.setViewportSize({ width: 390, height: 844 });
      await page.goto(
        getFrontendUrl(`/spaces/${seed.spaceId}/compositions/new`),
        { waitUntil: "domcontentloaded" },
      );
      await page.getByLabel("Name", { exact: true }).fill(
        seed.compositionName,
      );

      await page.getByRole("radio", { name: "Data", exact: true }).click();
      await page.getByRole("button", { name: "Add source", exact: true })
        .click();
      const sourcePicker = page.getByRole("dialog", {
        name: "Choose a source",
        exact: true,
      });
      await sourcePicker.getByRole("button", {
        name: new RegExp(seed.formName),
      }).click();
      await expect(page.getByRole("dialog")).toHaveCount(0);
      await page.getByRole("radio", { name: "Design", exact: true }).click();

      await addMetricFromCanvas(page, seed.formName);
      await addTableFromCanvas(page, seed.formName);
      const table = page.locator("table.result-table--entry");
      await expect(page.locator("body")).not.toContainText(
        /(?:\bfield[_\s-]?\d+\b|#\s*\d+\b|\b[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\b)/i,
      );
      expectNoOpaqueFieldLabels(
        await table.locator("thead th").allTextContents(),
      );
      const pageWidth = await page.evaluate(() => ({
        client: document.documentElement.clientWidth,
        scroll: document.documentElement.scrollWidth,
      }));
      expect(pageWidth.scroll).toBeLessThanOrEqual(pageWidth.client + 1);

      // Composition's default EntryQuery page is intentionally large. Set a
      // supported page_limit in the saved test fixture so three rows exercise
      // real server pagination without hundreds of slow append-only writes.
      const compositionSaveRoute = `**/spaces/${seed.spaceId}/compositions`;
      let compositionSaveAdjusted = false;
      await page.route(compositionSaveRoute, async (route) => {
        if (route.request().method() !== "POST") {
          await route.continue();
          return;
        }
        const body = route.request().postDataJSON() as { yaml?: string };
        expect(body.yaml).toBeTruthy();
        const yaml = body.yaml!.replace(
          /^([ \t]*page_limit: )\d+/m,
          (_line, prefix: string) => `${prefix}${ENTRY_PAGE_LIMIT}`,
        );
        expect(yaml).not.toBe(body.yaml);
        compositionSaveAdjusted = true;
        await route.continue({
          postData: JSON.stringify({ ...body, yaml }),
        });
      });
      await page.getByRole("button", { name: "Save", exact: true }).click();
      await page.unroute(compositionSaveRoute);
      expect(compositionSaveAdjusted).toBe(true);
      await expect(page).toHaveURL(
        new RegExp(`/spaces/${seed.spaceId}/compositions/[^/]+/[^/]+$`),
      );

      // Closing the editor and rediscovering the Composition from Home proves
      // the saved definition is restored and queried from its current source.
      await openSavedComposition(page, seed);
      const metric = page.locator("output.compositionMetric");
      await expect(metric).toHaveText(String(INITIAL_TOTAL), {
        timeout: 30_000,
      });
      await expect(table).toBeVisible();
      const tableHeaders = await table.locator("thead th").allTextContents();
      expect(tableHeaders).toContain("Amount");
      expect(tableHeaders).toContain("Reference");
      expect(tableHeaders).not.toContain("Created");
      expect(tableHeaders).not.toContain("Updated");
      await expect(table.locator("tbody tr")).toHaveCount(2);

      // Reopen this saved revision in Studio and verify the authoring
      // definition came back as SUM over the human-readable Amount field.
      const viewerPath = new URL(page.url()).pathname;
      await page.goto(getFrontendUrl(`${viewerPath}/edit`), {
        waitUntil: "domcontentloaded",
      });
      await page.getByRole("button", {
        name: `Select ${seed.formName} / Amount`,
        exact: true,
      }).click();
      await expect(page.getByLabel("Operation", { exact: true })).toHaveValue(
        "sum",
      );
      await expect(
        page.getByLabel("Field", { exact: true }).locator("option:checked"),
      ).toHaveText("Amount");

      // Return to the saved viewer without creating a second revision.
      await page.goto(getFrontendUrl(viewerPath), {
        waitUntil: "domcontentloaded",
      });
      await expect(metric).toHaveText(String(INITIAL_TOTAL), {
        timeout: 30_000,
      });

      await addEntry(request, seed.spaceId, seed.formName, 3, 10);
      await page.reload({ waitUntil: "domcontentloaded" });
      await expect(page.locator("output.compositionMetric")).toHaveText(
        String(UPDATED_TOTAL),
        { timeout: 30_000 },
      );

      await expect(table.locator("tbody tr")).toHaveCount(ENTRY_PAGE_LIMIT, {
        timeout: 30_000,
      });
      const pagination = page.getByRole("navigation", {
        name: "Result pages",
      });
      await pagination.getByRole("button", { name: "Next", exact: true })
        .click();
      await expect(table.locator("tbody tr")).toHaveCount(1, {
        timeout: 30_000,
      });
      await expect(page.locator("output.compositionMetric")).toHaveText(
        String(UPDATED_TOTAL),
      );
    } finally {
      await ownerContext.close();
    }
  });
});
