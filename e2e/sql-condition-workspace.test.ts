import { expect, test } from "@playwright/test";
import {
  getBackendUrl,
  getDefaultSpaceId,
  getFrontendUrl,
  waitForServers,
} from "./lib/client.ts";
import { expectMobileControlFontSize } from "./lib/ui-safety.ts";

async function runQueryAndReadFixtureNames(
  page: import("@playwright/test").Page,
  spaceId: string,
) {
  const responsePromise = page.waitForResponse((response) =>
    response.request().method() === "POST" &&
    new URL(response.url()).pathname === `/api/spaces/${spaceId}/sql/query`
  );
  await page.getByRole("button", { name: "Run Query", exact: true }).click();
  const response = await responsePromise;
  expect(response.ok(), await response.text()).toBe(true);
  const output = await response.json() as {
    rows: Record<string, unknown>[];
  };
  const names = output.rows.map((row) => {
    const name = Object.values(row).find((value) =>
      ["A", "B", "C", "D"].includes(String(value))
    );
    expect(name, `fixture name in ${JSON.stringify(row)}`).toBeDefined();
    return String(name);
  });
  return names.sort();
}

test("REQ-SQL-003 @smoke: edits nested Filter conditions, runs, and reopens the saved query", async ({ page, request }) => {
  test.setTimeout(120_000);
  page.setDefaultTimeout(15_000);
  await waitForServers(request);
  const spaceId = await getDefaultSpaceId(request);
  const formName = `ConditionJourney${crypto.randomUUID().slice(0, 8)}`;
  const created = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: {
        name: formName,
        version: 1,
        template: `# ${formName}\n\n## name\n## status\n## amount\n`,
        fields: {
          name: { type: "string", required: true, label: "Name" },
          status: { type: "string", required: true, label: "Status" },
          amount: { type: "integer", required: true, label: "Amount" },
        },
      },
    },
  );
  expect([200, 201]).toContain(created.status());
  for (
    const [name, status, amount] of [
      ["A", "open", 15],
      ["B", "blocked", 20],
      ["C", "closed", 25],
      ["D", "open", 5],
    ] as const
  ) {
    const entry = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      { data: { form: formName, fields: { name, status, amount } } },
    );
    expect(entry.status()).toBe(201);
  }

  await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/new`), {
    waitUntil: "domcontentloaded",
  });
  const sqlEditor = page.getByRole("textbox", { name: "SQL", exact: true });
  await page.getByRole("button", { name: "Form", exact: true }).click();
  await page.getByRole("dialog", { name: "Form", exact: true }).getByRole(
    "button",
    { name: formName, exact: true },
  ).click();

  await page.getByRole("button", { name: "Columns", exact: true }).click();
  const columns = page.getByRole("dialog", { name: "Columns", exact: true });
  await columns.getByRole("combobox", { name: "Add a field", exact: true })
    .selectOption({ label: "Name" });
  await columns.getByRole("button", { name: "Add", exact: true }).click();
  await expect(columns.getByRole("list", { name: "Result fields" }))
    .toContainText("Name");
  await columns.locator("footer").getByRole("button", { name: "Close" })
    .click();

  await page.getByRole("button", { name: "Filter", exact: true }).click();
  const filter = page.getByRole("dialog", { name: "Filter", exact: true });
  const filters = filter.getByRole("region", { name: "Filters", exact: true });
  const filterGroups = filters.getByRole("group", { name: "Filter group" });
  await filters.getByRole("combobox", { name: "Column for new filter" })
    .selectOption({ label: "Amount" });
  await filters.getByRole("combobox", { name: "Operator for new filter" })
    .selectOption("gte");
  await filters.getByRole("combobox", { name: "Value source" })
    .selectOption("argument");
  await filters.getByRole("textbox", { name: "New argument name" })
    .fill("minimum");
  await filters.getByRole("textbox", { name: "Default value (optional)" })
    .fill("10");
  await filters.getByRole("button", { name: "Add group", exact: true }).click();
  const rootComposer = filters.getByRole("group", {
    name: "New filter group",
  });
  await rootComposer.getByRole("combobox", { name: "Filter group operator" })
    .selectOption({ label: "All conditions" });
  await rootComposer.getByRole("combobox", {
    name: "Column for second condition",
  }).selectOption({ label: "Name" });
  await rootComposer.getByRole("combobox", {
    name: "Operator for second condition",
  }).selectOption("is_not_null");
  await rootComposer.getByRole("button", { name: "Create group", exact: true })
    .click();
  await expect(sqlEditor).toContainText("$minimum");
  await filter.getByRole("button", { name: "Undo", exact: true }).click();
  await expect(sqlEditor).not.toContainText("$minimum");
  await filter.getByRole("button", { name: "Redo", exact: true }).click();
  await expect(sqlEditor).toContainText("$minimum");
  await expect(filterGroups).toHaveCount(1);

  const rootGroup = filterGroups.first();
  await filters.getByRole("button", { name: "Add condition", exact: true })
    .click();
  await filters.getByRole("combobox", { name: "Column for new filter" })
    .selectOption({ label: "Status" });
  await filters.getByRole("combobox", { name: "Operator for new filter" })
    .selectOption("eq");
  await filters.getByRole("combobox", { name: "Value source" }).last()
    .selectOption("literal");
  await filters.getByRole("textbox", { name: "Value for new filter" })
    .fill("open");
  await rootGroup.getByRole("button", { name: "Add group", exact: true })
    .click();
  const nestedComposer = filters.getByRole("group", {
    name: "New filter group",
  });
  await nestedComposer.getByRole("combobox", {
    name: "Filter group operator",
  }).selectOption({ label: "Any condition" });
  await nestedComposer.getByRole("combobox", {
    name: "Column for second condition",
  }).selectOption({ label: "Status" });
  await nestedComposer.getByRole("combobox", {
    name: "Operator for second condition",
  }).selectOption("eq");
  await nestedComposer.getByRole("textbox", {
    name: "Value for second condition",
  }).fill("blocked");
  await nestedComposer.getByRole("button", {
    name: "Create group",
    exact: true,
  })
    .click();
  await expect(filters.getByRole("textbox", { name: "Value" }))
    .toHaveCount(0);
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.screenshot({
    path: test.info().outputPath("sql-filters-nested-desktop.png"),
    fullPage: true,
  });
  await page.setViewportSize({ width: 390, height: 844 });
  const mobileOverflow = await page.evaluate(() => ({
    clientWidth: document.documentElement.clientWidth,
    scrollWidth: document.documentElement.scrollWidth,
  }));
  expect(mobileOverflow.scrollWidth).toBeLessThanOrEqual(
    mobileOverflow.clientWidth + 1,
  );
  await page.screenshot({
    path: test.info().outputPath("sql-filters-nested-390px.png"),
    fullPage: true,
  });
  await page.setViewportSize({ width: 1280, height: 900 });
  await expect(filterGroups).toHaveCount(2);
  const createdNestedGroup = filterGroups.nth(1);
  await expect(
    createdNestedGroup.getByRole("combobox", {
      name: "Filter group operator",
    }),
  ).toHaveValue("or");
  const createdStatusRows = createdNestedGroup.getByRole("group", {
    name: "Status",
    exact: true,
  });
  await expect(createdStatusRows).toHaveCount(2);
  await expect(createdStatusRows.first()).toContainText("open");
  await expect(createdStatusRows.last()).toContainText("blocked");
  await expect(createdNestedGroup.getByRole("textbox", { name: "Value" }))
    .toHaveCount(0);

  await filter.locator("footer").getByRole("button", { name: "Close" })
    .click();

  await page.getByRole("button", { name: "Sort", exact: true }).click();
  const sort = page.getByRole("dialog", { name: "Sort", exact: true });
  await sort.getByRole("combobox", { name: "Column" }).selectOption({
    label: "Amount",
  });
  await sort.getByRole("button", { name: "Add sort", exact: true }).click();
  await sort.locator("footer").getByRole("button", { name: "Close" }).click();

  await sqlEditor.click();
  await sqlEditor.press("ControlOrMeta+End");
  await sqlEditor.pressSequentially("\n-- condition journey comment");
  await expect(sqlEditor).toContainText("-- condition journey comment");

  expect(await runQueryAndReadFixtureNames(page, spaceId)).toEqual(["A", "B"]);
  const results = page.getByRole("table");
  await expect(results).toContainText("A", { timeout: 30_000 });
  await expect(results).toContainText("B");
  await expect(results.getByRole("row")).toHaveCount(3);
  await expect(results).not.toContainText("C");
  await expect(results).not.toContainText("D");

  await page.getByRole("button", { name: "Filter", exact: true }).click();
  const editFilter = page.getByRole("dialog", { name: "Filter", exact: true });
  const editGroups = editFilter.getByRole("region", { name: "Filters" })
    .getByRole("group", { name: "Filter group" });
  const nested = editGroups.nth(1);
  const statusRows = nested.getByRole("group", { name: "Status", exact: true });
  await expect(statusRows).toHaveCount(2);
  await statusRows.nth(1).getByRole("button", {
    name: "Edit condition",
    exact: true,
  }).click();
  await statusRows.nth(1).getByRole("textbox", { name: "Value" }).fill(
    "closed",
  );
  await statusRows.nth(1).getByRole("button", { name: "Done", exact: true })
    .click();
  await editFilter.getByRole("button", { name: "Apply changes", exact: true })
    .click();
  await expect(statusRows.nth(1)).toContainText("closed");
  await expect(statusRows.nth(1).getByRole("textbox", { name: "Value" }))
    .toHaveCount(0);
  await editFilter.locator("footer").getByRole("button", { name: "Close" })
    .click();
  expect(await runQueryAndReadFixtureNames(page, spaceId)).toEqual(["A", "C"]);
  await expect(results).toContainText("A", { timeout: 30_000 });
  await expect(results).toContainText("C");
  await expect(results).not.toContainText("B");
  await expect(results.getByRole("row")).toHaveCount(3);

  await page.getByRole("button", { name: "Filter", exact: true }).click();
  const connectorDialog = page.getByRole("dialog", {
    name: "Filter",
    exact: true,
  });
  const nestedConnector = connectorDialog.getByRole("region", {
    name: "Filters",
  }).getByRole("group", { name: "Filter group" }).nth(1).getByRole(
    "combobox",
    { name: "Filter group operator" },
  );
  await nestedConnector.selectOption({ label: "All conditions" });
  await connectorDialog.getByRole("button", {
    name: "Apply changes",
    exact: true,
  }).click();
  await connectorDialog.locator("footer").getByRole("button", { name: "Close" })
    .click();
  expect(await runQueryAndReadFixtureNames(page, spaceId)).toEqual([]);
  await expect(results).not.toContainText("A", { timeout: 30_000 });
  await expect(results).not.toContainText("C");

  await page.getByRole("button", { name: "Filter", exact: true }).click();
  const undoDialog = page.getByRole("dialog", { name: "Filter", exact: true });
  await undoDialog.getByRole("button", { name: "Undo", exact: true }).click();
  const undoGroups = undoDialog.getByRole("region", { name: "Filters" })
    .getByRole("group", { name: "Filter group" });
  await expect(
    undoGroups.nth(1).getByRole("combobox", {
      name: "Filter group operator",
    }),
  ).toHaveValue("or");
  const removable = undoGroups.nth(1).getByRole("group", {
    name: "Status",
    exact: true,
  }).last();
  await removable.getByRole("button", { name: "Remove filter" }).click();
  await undoDialog.locator("footer").getByRole("button", { name: "Close" })
    .click();
  expect(await runQueryAndReadFixtureNames(page, spaceId)).toEqual(["A"]);
  await expect(results).toContainText("A", { timeout: 30_000 });
  await expect(results).not.toContainText("B");
  await expect(results).not.toContainText("C");
  await expect(results).not.toContainText("D");
  await expect(results.getByRole("row")).toHaveCount(2);

  await page.getByRole("button", { name: "Filter", exact: true }).click();
  const removeThresholdDialog = page.getByRole("dialog", {
    name: "Filter",
    exact: true,
  });
  const threshold = removeThresholdDialog.getByRole("region", {
    name: "Filters",
  }).getByRole("group", { name: "Filter group" }).first().getByRole(
    "group",
    { name: "Amount", exact: true },
  );
  await threshold.getByRole("button", { name: "Remove filter" }).click();
  await removeThresholdDialog.locator("footer").getByRole("button", {
    name: "Close",
  }).click();
  await expect(sqlEditor).not.toContainText("$minimum");
  const runQuery = page.getByRole("button", { name: "Run Query", exact: true });
  await expect(runQuery).toBeDisabled();
  await page.getByRole("button", { name: "Arguments", exact: true }).click();
  const unusedArguments = page.getByRole("dialog", {
    name: "Arguments",
    exact: true,
  });
  await expect(unusedArguments.getByRole("alert")).toContainText(/unused/i);
  await unusedArguments.getByRole("button", {
    name: /Remove.*minimum/,
  }).click();
  await unusedArguments.locator("footer").getByRole("button", {
    name: "Close",
  }).click();
  await expect(runQuery).toBeEnabled();
  const finalNames = await runQueryAndReadFixtureNames(page, spaceId);
  expect(finalNames).toEqual(["A", "D"]);
  await expect(results).toContainText("A", { timeout: 30_000 });
  await expect(results).toContainText("D");
  await expect(results).not.toContainText("B");
  await expect(results).not.toContainText("C");
  await expect(results.getByRole("row")).toHaveCount(3);

  await page.getByLabel("Query name", { exact: true }).fill(
    `Conditions ${formName}`,
  );
  const savePromise = page.waitForResponse((response) =>
    response.request().method() === "POST" &&
    new URL(response.url()).pathname === `/api/spaces/${spaceId}/sql`
  );
  await page.getByRole("button", { name: "Save", exact: true }).click();
  const save = await savePromise;
  expect([200, 201]).toContain(save.status());
  const savedId = (await save.json() as { id: string }).id;
  await page.waitForURL((url) => url.pathname === `/spaces/${spaceId}/sql`);
  const savedBody = await request.get(
    getBackendUrl(`/spaces/${spaceId}/sql/${savedId}`),
  );
  expect(savedBody.ok()).toBe(true);
  expect((await savedBody.json() as { sql: string }).sql).toContain(
    "-- condition journey comment",
  );

  await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${savedId}`));
  await expect(sqlEditor).not.toContainText("$minimum");
  await page.getByRole("button", { name: "Arguments", exact: true }).click();
  const args = page.getByRole("dialog", { name: "Arguments", exact: true });
  await expect(args.getByRole("listitem")).toHaveCount(0);
  await args.locator("footer").getByRole("button", { name: "Close" }).click();

  await page.getByRole("button", { name: "Filter", exact: true }).click();
  const reopenedFilter = page.getByRole("dialog", {
    name: "Filter",
    exact: true,
  });
  for (const width of [320, 375, 390]) {
    await page.setViewportSize({ width, height: 844 });
    const dimensions = await reopenedFilter.evaluate((dialog) => {
      const buttons = Array.from(dialog.querySelectorAll("button"))
        .filter((button) => {
          const rect = button.getBoundingClientRect();
          return rect.width > 0 && rect.height > 0;
        })
        .map((button) => {
          const rect = button.getBoundingClientRect();
          return { width: rect.width, height: rect.height };
        });
      return {
        documentWidth: document.documentElement.scrollWidth,
        viewportWidth: document.documentElement.clientWidth,
        buttons,
      };
    });
    expect(dimensions.documentWidth).toBeLessThanOrEqual(
      dimensions.viewportWidth + 1,
    );
    for (const button of dimensions.buttons) {
      expect(button.width).toBeGreaterThanOrEqual(44);
      expect(button.height).toBeGreaterThanOrEqual(44);
    }
    await expectMobileControlFontSize(page);
    if (width === 390) {
      const screenshot = test.info().outputPath("sql-condition-filter-390.png");
      await page.screenshot({ path: screenshot, fullPage: true });
      await test.info().attach("SQL condition Filter at 390px", {
        path: screenshot,
        contentType: "image/png",
      });
    }
  }
});
