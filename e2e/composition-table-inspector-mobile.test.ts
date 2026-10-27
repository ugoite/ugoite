import { expect, test } from "@playwright/test";
import {
  getBackendUrl,
  getDefaultSpaceId,
  getFrontendUrl,
} from "./lib/client.ts";

test("Composition Table inspector keeps selected fields legible on mobile", async ({ page, request }) => {
  test.setTimeout(120_000);
  const spaceId = await getDefaultSpaceId(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const formName = `InspectorRows${suffix}`;
  const variableId = `${formName.toLowerCase()}_rows`;
  const formResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: {
        name: formName,
        version: 1,
        template: `# ${formName}\n\n## Name\n\n## Quantity\n\n## Date\n`,
        fields: {
          Name: { type: "string", required: true },
          Quantity: { type: "integer", required: true },
          Date: { type: "date", required: true },
        },
      },
    },
  );
  expect([200, 201]).toContain(formResponse.status());
  const entryResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/entries`),
    {
      data: {
        form: formName,
        fields: {
          Name: "Inspector example row",
          Quantity: 17,
          Date: "2026-10-27",
        },
      },
    },
  );
  expect(entryResponse.status()).toBe(201);

  await page.goto(
    getFrontendUrl(`/spaces/${spaceId}/compositions/new`),
    { waitUntil: "domcontentloaded" },
  );
  await expect(page.getByPlaceholder("Composition name")).toBeVisible();
  await page.getByRole("group", { name: "Add to design" })
    .getByRole("button", { name: "Table", exact: true }).click();
  const picker = page.getByRole("dialog", { name: "Table", exact: true });
  await picker.getByRole("tab", { name: "Forms", exact: true }).click();
  await picker.getByRole("button", { name: formName, exact: true }).click();
  await expect(picker).toHaveCount(0, { timeout: 30_000 });
  const designTable = page.getByRole("table", {
    name: `${variableId} results`,
  });
  await expect(designTable).toBeVisible({ timeout: 30_000 });
  await expect(designTable).toContainText("Inspector example row");
  const componentButton = page.getByRole("button", {
    name: `table, ${variableId}`,
    exact: true,
  });
  await componentButton.click();
  const inspector = page.locator(".compositionGraphInspector");
  await expect(inspector).toBeVisible();
  await expect(inspector.getByRole("heading", { name: "Columns to display" }))
    .toBeVisible();
  await expect(
    inspector.getByRole("textbox", { name: "Display label for Name" }),
  )
    .toHaveValue("Name");
  await expect(
    inspector.getByRole("textbox", { name: "Display label for Quantity" }),
  )
    .toHaveValue("Quantity");
  await expect(
    inspector.getByRole("textbox", { name: "Display label for Date" }),
  )
    .toHaveValue("Date");
  await expect(
    inspector.getByRole("button", { name: "Show every result column" }),
  ).toBeVisible();
  await expect(
    inspector.getByText("Add result columns", { exact: true }),
  ).toBeVisible();
  await expect(
    inspector.getByRole("checkbox", { name: "Add result column _ugoite_id" }),
  ).toHaveCount(0);

  const screenshotDir = Deno.env.get("TABLE_INSPECTOR_SCREENSHOT_DIR");
  if (screenshotDir) await Deno.mkdir(screenshotDir, { recursive: true });
  for (const [width, height] of [[320, 812], [375, 812], [390, 844]]) {
    await page.setViewportSize({ width, height });
    const closeButton = inspector.getByRole("button", { name: "Close" });
    await closeButton.focus();
    await expect(closeButton).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(
      inspector.getByRole("button", {
        name: `Change variable, ${variableId}`,
      }),
    ).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(
      inspector.getByRole("textbox", { name: "Display label for Date" }),
    )
      .toBeFocused();
    await page.keyboard.press("Tab");
    await expect(
      inspector.getByRole("button", { name: "Move column later" }).first(),
    )
      .toBeFocused();

    if (screenshotDir) {
      await page.screenshot({
        path: `${screenshotDir}/table-inspector-${width}.png`,
        fullPage: true,
      });
    }

    const metrics = await page.evaluate(() => {
      const element = document.querySelector<HTMLElement>(
        ".compositionGraphInspector",
      );
      if (!element) throw new Error("Table inspector is missing");
      const bounds = element.getBoundingClientRect();
      const disclosure = element.querySelector("summary");
      if (!disclosure) {
        throw new Error("Optional columns disclosure is missing");
      }
      return {
        viewport: window.innerWidth,
        document: document.documentElement.scrollWidth,
        body: document.body.scrollWidth,
        inspectorClient: element.clientWidth,
        inspectorScroll: element.scrollWidth,
        disclosureTargetHeight: disclosure.getBoundingClientRect().height,
        overflow: Array.from(element.querySelectorAll<HTMLElement>("*"))
          .map((node) => ({
            tag: node.tagName,
            className: typeof node.className === "string" ? node.className : "",
            text: node.textContent?.trim().slice(0, 50),
            clientWidth: node.clientWidth,
            scrollWidth: node.scrollWidth,
            right: Math.round(node.getBoundingClientRect().right - bounds.left),
          }))
          .filter((node) =>
            node.scrollWidth > node.clientWidth + 2 ||
            node.right > bounds.width + 2
          ),
      };
    });
    expect(
      metrics.document,
      `page overflow at ${width}px: ${JSON.stringify(metrics)}`,
    )
      .toBeLessThanOrEqual(width);
    expect(
      metrics.body,
      `body overflow at ${width}px: ${JSON.stringify(metrics)}`,
    )
      .toBeLessThanOrEqual(width);
    expect(
      metrics.inspectorScroll,
      `inspector overflow at ${width}px: ${JSON.stringify(metrics)}`,
    ).toBeLessThanOrEqual(metrics.inspectorClient);
    expect(
      metrics.disclosureTargetHeight,
      `disclosure target is too small at ${width}px: ${
        JSON.stringify(metrics)
      }`,
    ).toBeGreaterThanOrEqual(44);

    const disclosure = inspector.getByText("Add result columns", {
      exact: true,
    });
    await disclosure.evaluate((element) =>
      element.scrollIntoView({ block: "center" })
    );
    const bottomNavigation = await page.getByRole("navigation", {
      name: "Space navigation",
    }).boundingBox();
    expect(bottomNavigation).not.toBeNull();
    for (
      const control of [
        disclosure,
        inspector.getByRole("button", {
          name: "Remove Quantity from the Table",
        }),
        inspector.getByRole("button", { name: "Show every result column" }),
      ]
    ) {
      const controlBounds = await control.boundingBox();
      expect(controlBounds, `control missing at ${width}px`).not.toBeNull();
      expect(
        controlBounds!.y + controlBounds!.height,
        `control is behind bottom navigation at ${width}px`,
      ).toBeLessThanOrEqual(bottomNavigation!.y);
    }
    if (screenshotDir) {
      await page.screenshot({
        path: `${screenshotDir}/table-inspector-${width}-controls.png`,
      });
    }
  }
});
