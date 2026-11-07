import { expect, test } from "@playwright/test";
import {
  getBackendUrl,
  getDefaultSpaceId,
  getFrontendUrl,
} from "./lib/client.ts";

test.use({
  launchOptions: {
    headless: false,
    args: ["--no-sandbox", "--window-size=1280,1200"],
  },
  viewport: null,
});

type BrowserMeasurement = {
  innerWidth: number;
  innerHeight: number;
  outerWidth: number;
  outerHeight: number;
  devicePixelRatio: number;
  visualViewportScale: number | null;
  rootZoom: string;
  documentScrollWidth: number;
  bodyScrollWidth: number;
  horizontalOverflow: boolean;
};

async function measure(page: import("@playwright/test").Page) {
  return await page.evaluate((): BrowserMeasurement => ({
    innerWidth,
    innerHeight,
    outerWidth,
    outerHeight,
    devicePixelRatio,
    visualViewportScale: visualViewport?.scale ?? null,
    rootZoom: getComputedStyle(document.documentElement).zoom,
    documentScrollWidth: document.documentElement.scrollWidth,
    bodyScrollWidth: document.body.scrollWidth,
    horizontalOverflow: document.documentElement.scrollWidth > innerWidth ||
      document.body.scrollWidth > innerWidth,
  }));
}

async function runXdotool(args: string[]): Promise<string> {
  const result = await new Deno.Command("xdotool", { args }).output();
  if (!result.success) {
    throw new Error(
      `xdotool ${args.join(" ")} failed: ${
        new TextDecoder().decode(result.stderr)
      }`,
    );
  }
  return new TextDecoder().decode(result.stdout).trim();
}

test("U-04 partial evidence: Composition Design remains usable at native 200% zoom", async ({ page, request }) => {
  const spaceId = await getDefaultSpaceId(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const sqlName = `Native zoom metric ${suffix}`;
  const compositionName = `Native zoom composition ${suffix}`;
  const sqlResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/sql`),
    {
      data: {
        name: sqlName,
        kind: "user-query",
        sql: "SELECT 1 AS metric",
        variables: [],
      },
    },
  );
  expect([200, 201]).toContain(sqlResponse.status());
  const savedSql = await sqlResponse.json() as {
    id?: string;
    revision_id?: string;
  };
  expect(savedSql.id).toBeTruthy();
  expect(savedSql.revision_id).toBeTruthy();

  const buildInfoResponse = await page.request.get(
    getFrontendUrl("/build-info.json"),
  );
  expect(buildInfoResponse.ok()).toBe(true);
  const buildInfo = await buildInfoResponse.json() as { source_sha?: string };
  expect(buildInfo.source_sha).toBe(Deno.env.get("UGOITE_SOURCE_SHA"));

  await page.goto(
    getFrontendUrl(`/spaces/${spaceId}/compositions/new`),
    { waitUntil: "domcontentloaded" },
  );
  const nameInput = page.getByLabel("Name", { exact: true });
  await expect(nameInput).toBeVisible();
  await nameInput.fill(compositionName);
  const beforeNativeZoom = await measure(page);
  expect(beforeNativeZoom.innerWidth).toBeGreaterThanOrEqual(1200);
  expect(beforeNativeZoom.innerWidth).toBeLessThanOrEqual(1300);
  expect(beforeNativeZoom.devicePixelRatio).toBe(1);
  expect(beforeNativeZoom.rootZoom).toBe("1");
  expect(beforeNativeZoom.visualViewportScale).toBe(1);

  await page.bringToFront();
  const visibleBrowserWindows = (await runXdotool([
    "search",
    "--onlyvisible",
    "--class",
    "chromium",
  ])).split(/\s+/).filter(Boolean);
  const focusedWindow = await runXdotool(["getwindowfocus"]);
  expect(visibleBrowserWindows).toContain(focusedWindow);
  for (let step = 0; step < 5; step++) {
    await runXdotool(["key", "--clearmodifiers", "ctrl+plus"]);
    await page.waitForTimeout(300);
  }

  const afterNativeZoom = await measure(page);
  expect(afterNativeZoom.innerWidth).toBe(640);
  expect(afterNativeZoom.devicePixelRatio).toBe(2);
  expect(afterNativeZoom.rootZoom).toBe("1");
  expect(afterNativeZoom.visualViewportScale).toBe(1);
  expect(afterNativeZoom.horizontalOverflow).toBe(false);

  await page.getByRole("tablist", { name: "Studio mode" })
    .getByRole("tab", { name: "Variables", exact: true }).click();
  await page.getByRole("button", { name: "Add variable", exact: true }).click();
  const addVariable = page.getByRole("group", { name: "Add variable" });
  await addVariable.getByRole("button", { name: "Input", exact: true }).click();
  const variableId = page.getByLabel("Variable ID", { exact: true });
  await expect(variableId).toBeVisible();
  await variableId.fill("zoom_input");
  await page.getByLabel("Default", { exact: true }).fill("draft");
  await expect(page.getByRole("button", { name: "zoom_input", exact: true }))
    .toBeVisible();

  await page.getByRole("tablist", { name: "Studio mode" })
    .getByRole("tab", { name: "Design", exact: true }).click();
  const palette = page.getByRole("group", { name: "Add to design" });
  await palette.getByLabel("Input", { exact: true })
    .selectOption({ label: "zoom_input" });
  await palette.getByRole("button", { name: "Metric", exact: true }).click();
  const metricDialog = page.getByRole("dialog", {
    name: "Metric",
    exact: true,
  });
  await expect(metricDialog).toBeVisible();
  await metricDialog.getByRole("tab", { name: "Saved SQL", exact: true })
    .click();
  await metricDialog.getByRole("button", { name: sqlName, exact: true })
    .click();
  await expect(metricDialog).toHaveCount(0);
  await expect(page.locator(".compositionGraphMetric strong")).toHaveText("1");

  const afterCreation = await measure(page);
  expect(afterCreation.innerWidth).toBe(640);
  expect(afterCreation.horizontalOverflow).toBe(false);
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page).toHaveURL(
    new RegExp(`/spaces/${spaceId}/compositions/[^/]+/[^/]+$`),
    { timeout: 30_000 },
  );
  const savedPath = new URL(page.url()).pathname;
  const revisionMatch = savedPath.match(
    /\/compositions\/([^/]+)\/([^/]+)$/,
  );
  expect(revisionMatch).toBeTruthy();
  const [compositionId, revisionId] = [revisionMatch![1], revisionMatch![2]];

  await page.goto(
    getFrontendUrl(
      `/spaces/${spaceId}/compositions/${compositionId}/${revisionId}/edit`,
    ),
    { waitUntil: "domcontentloaded" },
  );
  await expect(
    page.getByRole("group", { name: "Add to design", exact: true }).first(),
  ).toBeVisible({ timeout: 30_000 });
  await expect(
    page.getByRole("tab", { name: "Design", exact: true }),
  ).toHaveAttribute("aria-selected", "true");
  await expect(page.locator(".compositionGraphMetric strong")).toHaveText(
    "1",
    { timeout: 30_000 },
  );
  await expect(page.getByRole("button", { name: /input, zoom_input/ }))
    .toBeVisible();
  await expect(page.getByRole("textbox", { name: "zoom_input" }))
    .toBeVisible();

  const savedDesignAt200 = await measure(page);
  expect(savedDesignAt200.innerWidth).toBe(640);
  expect(savedDesignAt200.devicePixelRatio).toBe(2);
  expect(savedDesignAt200.horizontalOverflow).toBe(false);
  const savedMetric = page.locator(".compositionGraphMetric");
  await expect(savedMetric).toBeVisible();
  await savedMetric.evaluate((element) =>
    element.scrollIntoView({ block: "center" })
  );
  await expect(savedMetric).toBeInViewport({ ratio: 0.9 });
  await expect(page.getByRole("textbox", { name: "zoom_input" }))
    .toBeInViewport({ ratio: 0.9 });

  const evidenceDir = "../docs/architecture/testing/evidence";
  await Deno.mkdir(evidenceDir, { recursive: true });
  const screenshot = `${evidenceDir}/composition-native-zoom-200.png`;
  const resultRow = page.locator(".compositionGraphCanvasRow").filter({
    has: savedMetric,
  });
  await expect(resultRow).toBeVisible();
  const resultBox = await resultRow.boundingBox();
  const tabsBox = await page.getByRole("tablist", { name: "Studio mode" })
    .boundingBox();
  const inputBox = await page.getByRole("textbox", { name: "zoom_input" })
    .boundingBox();
  const metricBox = await savedMetric.boundingBox();
  expect(resultBox).toBeTruthy();
  expect(tabsBox).toBeTruthy();
  expect(inputBox).toBeTruthy();
  expect(metricBox).toBeTruthy();
  const intersects = (
    left: NonNullable<typeof tabsBox>,
    right: NonNullable<typeof resultBox>,
  ) =>
    left.x < right.x + right.width &&
    left.x + left.width > right.x &&
    left.y < right.y + right.height &&
    left.y + left.height > right.y;
  expect(intersects(tabsBox!, resultBox!)).toBe(false);
  const hitTestAt = async (box: NonNullable<typeof metricBox>) =>
    await page.evaluate(({ x, y }) => {
      const target = document.elementFromPoint(x, y);
      return {
        tag: target?.tagName ?? null,
        className: typeof target?.className === "string"
          ? target.className
          : "",
        text: target?.textContent?.trim() ?? "",
      };
    }, { x: box.x + box.width / 2, y: box.y + box.height / 2 });
  const screenshotTarget = {
    resultRow: { text: await resultRow.innerText(), box: resultBox },
    tabsBox,
    inputBox,
    metricBox,
    inputHit: await hitTestAt(inputBox!),
    metricHit: await hitTestAt(metricBox!),
  };
  expect(screenshotTarget.inputHit).toMatchObject({
    tag: "INPUT",
    className: "ui-input",
  });
  expect(screenshotTarget.metricHit).toMatchObject({
    tag: "H2",
    className: "compositionGraphMetricLabel",
  });
  const cdp = await page.context().newCDPSession(page);
  const cdpCapture = await cdp.send("Page.captureScreenshot", {
    format: "png",
    fromSurface: true,
    captureBeyondViewport: false,
  });
  await Deno.writeFile(
    screenshot,
    Uint8Array.from(
      atob(cdpCapture.data),
      (character) => character.charCodeAt(0),
    ),
  );
  await cdp.detach();
  const report = {
    sourceSha: buildInfo.source_sha,
    chromiumVersion: page.context().browser()?.version(),
    nativeShortcut: "Ctrl+Plus x5 via xdotool",
    initialViewport:
      `${beforeNativeZoom.innerWidth}x${beforeNativeZoom.innerHeight} CSS px; DPR 1`,
    zoomedViewport:
      `${afterNativeZoom.innerWidth}x${afterNativeZoom.innerHeight} CSS px; DPR 2`,
    cssZoom: "1",
    visualViewportScale: 1,
    horizontalOverflow: false,
    zoomedDocumentScrollWidth: afterNativeZoom.documentScrollWidth,
    zoomedBodyScrollWidth: afterNativeZoom.bodyScrollWidth,
    beforeNativeZoom,
    afterNativeZoom,
    afterCreation,
    savedDesignAt200,
    inputVariable: "zoom_input",
    metricValue: "1",
    screenshotTarget,
    screenshot,
  };
  console.log(`NATIVE_ZOOM_EVIDENCE ${JSON.stringify(report)}`);
});
