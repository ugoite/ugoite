import { expect, test } from "@playwright/test";
import {
  getBackendUrl,
  getDefaultSpaceId,
  getFrontendUrl,
} from "./lib/client.ts";
import {
  completeCompositionGraphYaml,
  saveCompositionGraph,
} from "./lib/composition-graph.ts";

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

test("U-04 partial evidence: Composition runtime remains usable at native 200% zoom", async ({ page, request }) => {
  const spaceId = await getDefaultSpaceId(request);
  const name = `Native zoom runtime ${crypto.randomUUID().slice(0, 8)}`;
  const graphYaml = await completeCompositionGraphYaml(request, spaceId, {
    name,
    variables: [
      {
        id: "first_amount",
        kind: "input",
        value_type: { scalar_type: "integer", nullable: false },
        required: true,
        default: { kind: "integer", value: 1 },
      },
      {
        id: "second_amount",
        kind: "input",
        value_type: { scalar_type: "integer", nullable: false },
        required: true,
        default: { kind: "integer", value: 2 },
      },
      {
        id: "total",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "integer", nullable: false },
        },
        query: {
          kind: "inline_sql",
          sql: "SELECT $first_amount + $second_amount AS total",
          arguments: [
            { name: "first_amount", variable: "first_amount" },
            { name: "second_amount", variable: "second_amount" },
          ],
          form_bindings: [],
          schema_fingerprint: "",
        },
      },
    ],
    components: [
      { id: "first_input", kind: "input", variable: "first_amount" },
      { id: "second_input", kind: "input", variable: "second_amount" },
      { id: "total_metric", kind: "metric", variable: "total" },
    ],
    rows: [
      {
        id: "first_input_row",
        items: [{ kind: "component", component: "first_input" }],
      },
      {
        id: "second_input_row",
        items: [{ kind: "component", component: "second_input" }],
      },
      {
        id: "result",
        items: [{ kind: "component", component: "total_metric" }],
      },
    ],
  });
  const saved = await saveCompositionGraph(request, spaceId, graphYaml);

  const buildInfoResponse = await page.request.get(
    getFrontendUrl("/build-info.json"),
  );
  expect(buildInfoResponse.ok()).toBe(true);
  const buildInfo = await buildInfoResponse.json() as { source_sha?: string };
  expect(buildInfo.source_sha).toBe(Deno.env.get("UGOITE_SOURCE_SHA"));
  const backendSourceSha = Deno.env.get("UGOITE_BACKEND_SOURCE_SHA");
  expect(backendSourceSha).toMatch(/^[0-9a-f]{40}$/);

  await page.goto(
    getFrontendUrl(
      `/spaces/${spaceId}/compositions/${saved.composition_id}/${saved.revision_id}`,
    ),
    { waitUntil: "domcontentloaded" },
  );
  await expect(page.getByRole("heading", { level: 1, name })).toBeVisible();
  const inputs = page.locator(".compositionGraphInput input");
  const firstInput = inputs.nth(0);
  const secondInput = inputs.nth(1);
  await expect(inputs).toHaveCount(2);
  await expect(firstInput).toHaveAccessibleName("first_amount");
  await expect(secondInput).toHaveAccessibleName("second_amount");
  const metric = page.locator(".compositionGraphMetric strong");
  await expect(metric).toHaveText("3", { timeout: 30_000 });

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
  await expect(firstInput).toBeVisible();
  await expect(secondInput).toBeVisible();
  const assertIntersectsViewport = async (
    locator: import("@playwright/test").Locator,
  ) => {
    const intersection = await locator.evaluate((element) => {
      const rect = element.getBoundingClientRect();
      const left = Math.max(0, rect.left);
      const top = Math.max(0, rect.top);
      const right = Math.min(innerWidth, rect.right);
      const bottom = Math.min(innerHeight, rect.bottom);
      return {
        left,
        top,
        right,
        bottom,
        width: Math.max(0, right - left),
        height: Math.max(0, bottom - top),
      };
    });
    expect(intersection.width).toBeGreaterThan(0);
    expect(intersection.height).toBeGreaterThan(0);
    return intersection;
  };
  const assertFullyWithinViewport = async (
    locator: import("@playwright/test").Locator,
  ) => {
    const bounds = await locator.boundingBox();
    expect(bounds).toBeTruthy();
    expect(bounds!.x).toBeGreaterThanOrEqual(0);
    expect(bounds!.y).toBeGreaterThanOrEqual(0);
    expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(
      afterNativeZoom.innerWidth,
    );
    expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(
      afterNativeZoom.innerHeight,
    );
    return bounds;
  };
  const firstInputIntersection = await assertIntersectsViewport(firstInput);
  const secondInputIntersection = await assertIntersectsViewport(secondInput);
  const firstInputBox = await firstInput.boundingBox();
  const secondInputBox = await secondInput.boundingBox();
  expect(firstInputBox).toBeTruthy();
  expect(secondInputBox).toBeTruthy();
  expect(firstInputBox!.height).toBeGreaterThanOrEqual(44);
  expect(secondInputBox!.height).toBeGreaterThanOrEqual(44);

  await firstInput.focus();
  await expect(firstInput).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(secondInput).toBeFocused();
  await firstInput.fill("4");
  await expect(firstInput).toHaveValue("4");
  await expect(secondInput).toHaveValue("2");
  await expect(metric).toHaveText("6", { timeout: 30_000 });

  const afterInputChange = await measure(page);
  expect(afterInputChange.innerWidth).toBe(640);
  expect(afterInputChange.devicePixelRatio).toBe(2);
  expect(afterInputChange.horizontalOverflow).toBe(false);
  await expect(metric).toBeVisible();
  await expect(metric).toBeInViewport({ ratio: 0.9 });
  await expect(firstInput).toBeInViewport({ ratio: 0.9 });
  await expect(secondInput).toBeInViewport({ ratio: 0.9 });
  const metricIntersection = await assertIntersectsViewport(metric);

  const evidenceDir = "../docs/architecture/testing/evidence";
  await Deno.mkdir(evidenceDir, { recursive: true });
  const screenshots = {
    firstInput:
      `${evidenceDir}/composition-runtime-native-zoom-200-first-input.png`,
    secondInput:
      `${evidenceDir}/composition-runtime-native-zoom-200-second-input.png`,
    metric: `${evidenceDir}/composition-runtime-native-zoom-200-metric.png`,
    metricViewport:
      `${evidenceDir}/composition-runtime-native-zoom-200-metric-viewport.png`,
    metricResult:
      `${evidenceDir}/composition-runtime-native-zoom-200-metric-result.png`,
  };
  await firstInput.evaluate((element) =>
    element.scrollIntoView({ block: "center", inline: "nearest" })
  );
  await expect(firstInput).toBeInViewport({ ratio: 0.9 });
  const firstInputScreenshotIntersection = await assertIntersectsViewport(
    firstInput,
  );
  await firstInput.focus();
  await expect(firstInput).toBeFocused();
  await page.screenshot({ path: screenshots.firstInput });
  await secondInput.evaluate((element) =>
    element.scrollIntoView({ block: "center", inline: "nearest" })
  );
  await expect(secondInput).toBeInViewport({ ratio: 0.9 });
  const secondInputScreenshotIntersection = await assertIntersectsViewport(
    secondInput,
  );
  await secondInput.focus();
  await expect(secondInput).toBeFocused();
  await page.screenshot({ path: screenshots.secondInput });
  const metricResult = page.locator(".compositionGraphMetric");
  await metricResult.evaluate((element) =>
    element.scrollIntoView({ block: "center", inline: "nearest" })
  );
  await expect(metric).toBeInViewport({ ratio: 0.9 });
  const metricPaintText = await metricResult.evaluate(async (element) => {
    await new Promise<void>((resolve) => {
      requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
    });
    return element.querySelector("strong")?.textContent?.trim() ?? "";
  });
  expect(metricPaintText).toBe("6");
  const metricScreenshotIntersection = await assertIntersectsViewport(metric);
  await page.screenshot({ path: screenshots.metric, fullPage: true });
  await page.screenshot({ path: screenshots.metricViewport });
  await expect(metric).toHaveText("6");
  await expect(metricResult).toContainText("6");
  const metricResultScreenshotIntersection = await assertIntersectsViewport(
    metricResult,
  );
  const metricResultScreenshotBounds = await assertFullyWithinViewport(
    metricResult,
  );
  await expect(metricResult.locator("strong")).toHaveText("6");
  const metricScreenshotText = await metricResult.locator("strong").innerText();
  expect(metricScreenshotText).toBe("6");
  await metricResult.screenshot({ path: screenshots.metricResult });
  const report = {
    sourceSha: buildInfo.source_sha,
    backendSourceSha,
    chromiumVersion: page.context().browser()?.version(),
    nativeShortcut: "Ctrl+Plus x5 via xdotool",
    initialViewport:
      `${beforeNativeZoom.innerWidth}x${beforeNativeZoom.innerHeight} CSS px; DPR 1`,
    zoomedViewport:
      `${afterNativeZoom.innerWidth}x${afterNativeZoom.innerHeight} CSS px; DPR 2`,
    cssZoom: "1",
    visualViewportScale: 1,
    horizontalOverflow: false,
    zoomedDocumentScrollWidth: afterInputChange.documentScrollWidth,
    zoomedBodyScrollWidth: afterInputChange.bodyScrollWidth,
    inputs: [
      {
        name: "first_amount",
        box: firstInputBox,
        viewportIntersection: firstInputIntersection,
        screenshotViewportIntersection: firstInputScreenshotIntersection,
      },
      {
        name: "second_amount",
        box: secondInputBox,
        viewportIntersection: secondInputIntersection,
        screenshotViewportIntersection: secondInputScreenshotIntersection,
      },
    ],
    tabOrder: ["first_amount", "second_amount"],
    metric: { before: "3", afterChangingFirstInputTo: "6", visible: true },
    metricViewportIntersection: metricIntersection,
    metricScreenshotViewportIntersection: metricScreenshotIntersection,
    metricResultScreenshotViewportIntersection:
      metricResultScreenshotIntersection,
    metricResultScreenshotViewportBounds: metricResultScreenshotBounds,
    metricScreenshotText,
    beforeNativeZoom,
    afterNativeZoom,
    afterInputChange,
    screenshots,
    metricPaintText,
    metricScreenshotMode:
      "two animation frames after ordinary scroll; full-page supplement, viewport capture, and Metric result capture",
  };
  console.log(`NATIVE_ZOOM_RUNTIME_EVIDENCE ${JSON.stringify(report)}`);
});
