import { expect, test } from "@playwright/test";
import { getDefaultSpaceId, getFrontendUrl } from "./lib/client.ts";
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
  const compositionName = `Native zoom composition ${suffix}`;

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
  await page.getByRole("tablist", { name: "Studio mode" })
    .getByRole("tab", { name: "Variables", exact: true }).click();
  await page.getByRole("button", { name: "Add variable", exact: true }).click();
  await page.getByRole("group", { name: "Add variable" })
    .getByRole("button", { name: "SQL", exact: true }).click();
  const queryName = page.getByLabel("Variable ID", { exact: true });
  await queryName.fill("zoom_total");
  await queryName.press("Enter");
  await page.getByRole("textbox", { name: "SQL", exact: true })
    .fill("SELECT 1 AS total");
  const queryPreviewResponse = page.waitForResponse((response) =>
    new URL(response.url()).pathname.endsWith("/compositions/evaluate") &&
    response.request().method() === "POST"
  );
  await page.getByRole("button", { name: "Run Query", exact: true }).click();
  expect((await (await queryPreviewResponse).json() as { ok?: boolean }).ok)
    .toBe(true);
  await page.getByLabel("Query result", { exact: true })
    .selectOption("scalar");
  await page.getByRole("tablist", { name: "Studio mode" })
    .getByRole("tab", { name: "Design", exact: true }).click();
  await palette.getByRole("button", { name: "Text", exact: true }).click();
  const textEditor = page.locator("#composition-text-editor-text_1");
  await expect(textEditor).toBeVisible();
  await textEditor.fill("{{zoom_total}}");
  const compositionText = page.locator(
    '[data-component-id="text_1"] .compositionGraphText',
  );
  await expect(compositionText).toHaveText("1");

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
  await expect(
    page.locator('[data-component-id="text_1"] .compositionGraphText'),
  ).toHaveText(
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
  const savedText = page.locator(
    '[data-component-id="text_1"] .compositionGraphText',
  );
  await expect(savedText).toBeVisible();
  await savedText.evaluate((element) =>
    element.scrollIntoView({ block: "center" })
  );
  await expect(savedText).toBeInViewport({ ratio: 0.9 });
  await expect(page.getByRole("textbox", { name: "zoom_input" }))
    .toBeInViewport({ ratio: 0.9 });

  const evidenceDir = "../docs/architecture/testing/evidence";
  await Deno.mkdir(evidenceDir, { recursive: true });
  const screenshot = `${evidenceDir}/composition-native-zoom-200.png`;
  const resultRow = page.locator(".compositionGraphCanvasRow").filter({
    has: savedText,
  });
  await expect(resultRow).toBeVisible();
  const resultBox = await resultRow.boundingBox();
  const tabsBox = await page.getByRole("tablist", { name: "Studio mode" })
    .boundingBox();
  const inputBox = await page.getByRole("textbox", { name: "zoom_input" })
    .boundingBox();
  const textBox = await savedText.boundingBox();
  expect(resultBox).toBeTruthy();
  expect(tabsBox).toBeTruthy();
  expect(inputBox).toBeTruthy();
  expect(textBox).toBeTruthy();
  const intersects = (
    left: NonNullable<typeof tabsBox>,
    right: NonNullable<typeof resultBox>,
  ) =>
    left.x < right.x + right.width &&
    left.x + left.width > right.x &&
    left.y < right.y + right.height &&
    left.y + left.height > right.y;
  expect(intersects(tabsBox!, resultBox!)).toBe(false);
  const hitTestAt = async (box: NonNullable<typeof textBox>) =>
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
    textBox,
    inputHit: await hitTestAt(inputBox!),
    textHit: await hitTestAt(textBox!),
  };
  expect(screenshotTarget.inputHit).toMatchObject({
    tag: "INPUT",
    className: "ui-input",
  });
  expect(screenshotTarget.textHit).toMatchObject({
    tag: "P",
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
    textValue: "1",
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
      {
        id: "total_text",
        kind: "text",
        content: "{{total}}",
        style: "body",
      },
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
        items: [{ kind: "component", component: "total_text" }],
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
  const text = page.locator(
    '[data-component-id="total_text"] .compositionGraphText',
  );
  await expect(text).toHaveText("3", { timeout: 30_000 });

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
  await expect(text).toHaveText("6", { timeout: 30_000 });

  const afterInputChange = await measure(page);
  expect(afterInputChange.innerWidth).toBe(640);
  expect(afterInputChange.devicePixelRatio).toBe(2);
  expect(afterInputChange.horizontalOverflow).toBe(false);
  await expect(text).toBeVisible();
  await expect(text).toBeInViewport({ ratio: 0.9 });
  await expect(firstInput).toBeInViewport({ ratio: 0.9 });
  await expect(secondInput).toBeInViewport({ ratio: 0.9 });
  const textIntersection = await assertIntersectsViewport(text);

  const evidenceDir = "../docs/architecture/testing/evidence";
  await Deno.mkdir(evidenceDir, { recursive: true });
  const screenshots = {
    firstInput:
      `${evidenceDir}/composition-runtime-native-zoom-200-first-input.png`,
    secondInput:
      `${evidenceDir}/composition-runtime-native-zoom-200-second-input.png`,
    text: `${evidenceDir}/composition-runtime-native-zoom-200-text.png`,
    textViewport:
      `${evidenceDir}/composition-runtime-native-zoom-200-text-viewport.png`,
    textViewportDevice:
      `${evidenceDir}/composition-runtime-native-zoom-200-text-viewport-device.png`,
    textResult:
      `${evidenceDir}/composition-runtime-native-zoom-200-text-result.png`,
    textResultDevice:
      `${evidenceDir}/composition-runtime-native-zoom-200-text-result-device.png`,
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
  const textResult = page.locator('[data-component-id="total_text"]');
  await textResult.evaluate((element) =>
    element.scrollIntoView({ block: "center", inline: "nearest" })
  );
  await expect(text).toBeInViewport({ ratio: 0.9 });
  const textPaintValue = await textResult.evaluate(async (element) => {
    await new Promise<void>((resolve) => {
      requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
    });
    return element.querySelector(".compositionGraphText")?.textContent
      ?.trim() ?? "";
  });
  expect(textPaintValue).toBe("6");
  const textScreenshotIntersection = await assertIntersectsViewport(text);
  await page.screenshot({ path: screenshots.text, fullPage: true });
  await page.screenshot({ path: screenshots.textViewport });
  await page.screenshot({
    path: screenshots.textViewportDevice,
    scale: "device",
  });
  await expect(text).toHaveText("6");
  await expect(textResult).toContainText("6");
  const textResultScreenshotIntersection = await assertIntersectsViewport(
    textResult,
  );
  const textResultScreenshotBounds = await assertFullyWithinViewport(
    textResult,
  );
  await expect(text).toHaveText("6");
  const textScreenshotValue = await text.innerText();
  expect(textScreenshotValue).toBe("6");
  await textResult.screenshot({ path: screenshots.textResult });
  await textResult.screenshot({
    path: screenshots.textResultDevice,
    scale: "device",
  });
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
    text: { before: "3", afterChangingFirstInputTo: "6", visible: true },
    textViewportIntersection: textIntersection,
    textScreenshotViewportIntersection: textScreenshotIntersection,
    textResultScreenshotViewportIntersection: textResultScreenshotIntersection,
    textResultScreenshotViewportBounds: textResultScreenshotBounds,
    textScreenshotValue,
    beforeNativeZoom,
    afterNativeZoom,
    afterInputChange,
    screenshots,
    textPaintValue,
    textScreenshotMode:
      "two animation frames after ordinary scroll; full-page, CSS-scale viewport/result, and device-scale viewport/result captures",
  };
  console.log(`NATIVE_ZOOM_RUNTIME_EVIDENCE ${JSON.stringify(report)}`);
});
