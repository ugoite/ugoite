import { expect, test } from "@playwright/test";
import {
  getBackendUrl,
  getDefaultSpaceId,
  getFrontendUrl,
  waitForServers,
} from "./lib/client.ts";
import {
  completeCompositionGraphYaml,
  saveCompositionGraph,
} from "./lib/composition-graph.ts";

function sqlIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
}

test("Composition inline SQL filters preserve scalar bindings through nested groups, history, and save", async ({ page, request }) => {
  test.setTimeout(120_000);
  page.setDefaultTimeout(15_000);
  await waitForServers(request);
  const spaceId = await getDefaultSpaceId(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const formName = `FilterRows${suffix}`;
  const compositionName = `Nested filter ${suffix}`;
  const formResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: {
        name: formName,
        version: 1,
        template: `# ${formName}\n\n## Amount\n`,
        fields: { Amount: { type: "integer", required: true } },
      },
    },
  );
  expect([200, 201]).toContain(formResponse.status());
  const formsResponse = await request.get(
    getBackendUrl(`/spaces/${spaceId}/forms`),
  );
  expect(formsResponse.ok()).toBe(true);
  const forms = await formsResponse.json() as Array<{
    id?: string;
    name?: string;
    sql_relation?: string;
    fields?: Record<string, { sql_column?: string }>;
  }>;
  const form = forms.find((entry) => entry.name === formName);
  expect(form?.id).toBeTruthy();
  expect(form?.sql_relation).toBeTruthy();
  const amountColumn = form?.fields?.Amount?.sql_column;
  expect(amountColumn).toBeTruthy();
  for (const amount of [15, 5]) {
    const entryResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      { data: { form: formName, fields: { Amount: amount } } },
    );
    expect(entryResponse.status()).toBe(201);
  }

  const amount = sqlIdentifier(amountColumn!);
  const relation = sqlIdentifier(form!.sql_relation!);
  const yaml = await completeCompositionGraphYaml(request, spaceId, {
    name: compositionName,
    variables: [
      {
        id: "minimum",
        kind: "constant",
        value_type: { scalar_type: "integer", nullable: false },
        value: { kind: "integer", value: 10 },
      },
      {
        id: "rows",
        kind: "query",
        returns: {
          kind: "table",
          columns: [{
            name: "Amount",
            value_type: { scalar_type: "integer", nullable: false },
          }],
        },
        query: {
          kind: "inline_sql",
          sql:
            `SELECT ${amount} AS amount FROM ${relation} WHERE ${amount} >= $minimum`,
          arguments: [{ name: "minimum", variable: "minimum" }],
          form_bindings: [],
          schema_fingerprint: "",
        },
      },
    ],
    components: [{ id: "results", kind: "table", variable: "rows" }],
    rows: [{
      id: "results_row",
      items: [{ kind: "component", component: "results" }],
    }],
  });
  const saved = await saveCompositionGraph(request, spaceId, yaml);
  await page.goto(
    getFrontendUrl(
      `/spaces/${spaceId}/compositions/${saved.composition_id}/${saved.revision_id}/edit`,
    ),
    { waitUntil: "domcontentloaded" },
  );
  const variables = page.getByRole("tablist", { name: "Studio mode" });
  await variables.getByRole("tab", { name: "Variables", exact: true }).click();
  await page.getByRole("button", { name: "rows", exact: true }).click();
  const minimumBinding = page.getByRole("combobox", {
    name: "minimum",
    exact: true,
  });
  await expect(minimumBinding).toHaveValue("minimum");
  const filterButton = page.getByRole("button", {
    name: "Filter",
    exact: true,
  });
  await filterButton.click();
  const filter = page.getByRole("dialog", { name: "Filter", exact: true });
  const filters = filter.getByRole("region", { name: "Filters", exact: true });
  const amountRow = filter.getByRole("group", { name: "Amount", exact: true });
  await amountRow.getByRole("button", {
    name: "Edit condition",
    exact: true,
  }).click();
  await amountRow.getByRole("combobox", { name: "Operator" }).selectOption(
    "gt",
  );
  await amountRow.getByRole("button", { name: "Done", exact: true }).click();
  await filter.getByRole("button", { name: "Apply changes", exact: true })
    .click();
  await expect(page.locator(".cm-content")).toContainText("> $minimum");
  await expect(amountRow).toContainText("minimum argument");
  await expect(amountRow.getByRole("textbox", { name: "Value" }))
    .toHaveCount(0);

  await filters.getByRole("button", { name: "Add condition", exact: true })
    .click();
  await filters.getByRole("combobox", { name: "Column for new filter" })
    .selectOption({ label: "Amount" });
  await filters.getByRole("textbox", { name: "Value for new filter" })
    .fill("15");
  await filter.getByRole("button", { name: "Add group", exact: true }).click();
  const groupComposer = filter.getByRole("group", { name: "New filter group" });
  await groupComposer.getByRole("combobox", { name: "Filter group operator" })
    .selectOption("or");
  await groupComposer.getByRole("combobox", {
    name: "Column for second condition",
  }).selectOption({ label: "Amount" });
  await groupComposer.getByRole("textbox", {
    name: "Value for second condition",
  })
    .fill("5");
  await groupComposer.getByRole("button", { name: "Create group", exact: true })
    .click();
  await expect.poll(() =>
    filter.evaluate((dialog) => dialog.contains(document.activeElement))
  ).toBe(true);
  const amountConditions = filters.getByRole("group", {
    name: "Amount",
    exact: true,
  });
  await expect(amountConditions).toHaveCount(3);
  await expect(amountConditions.first()).toContainText("minimum argument");
  await expect(amountConditions.nth(1)).toContainText("15");
  await expect(amountConditions.last()).toContainText("5");
  await expect(filters.getByRole("textbox", { name: "Value" }))
    .toHaveCount(0);
  await expect(
    filters.getByRole("group", { name: "Filter group" }).last()
      .getByRole("combobox", { name: "Filter group operator" }),
  )
    .toHaveValue("or");
  await expect(page.locator(".cm-content")).toContainText("OR");
  await expect(page.locator(".cm-content")).toContainText("$minimum");
  await expect(minimumBinding).toHaveValue("minimum");

  await page.keyboard.press("Escape");
  await expect(filter).toHaveCount(0);
  await filterButton.click();
  await expect(filter.getByRole("button", { name: "Undo", exact: true }))
    .toBeEnabled();
  await filter.getByRole("button", { name: "Undo", exact: true }).click();
  await expect(page.locator(".cm-content")).not.toContainText("OR");
  await expect(page.locator(".cm-content")).toContainText("> $minimum");
  await filter.getByRole("button", { name: "Redo", exact: true }).click();
  await expect(page.locator(".cm-content")).toContainText("OR");
  await expect(minimumBinding).toHaveValue("minimum");
  await expect.poll(() =>
    filter.evaluate((dialog) => dialog.contains(document.activeElement))
  ).toBe(true);
  await page.keyboard.press("Escape");

  await page.getByRole("button", { name: "Run Query", exact: true }).click();
  const results = page.getByRole("table", {
    name: "SQL results",
    exact: true,
  });
  await expect(results).toContainText("15", { timeout: 30_000 });
  await expect(results.locator("tbody tr")).toHaveCount(1);

  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page).toHaveURL(
    new RegExp(`/compositions/${saved.composition_id}/[^/]+$`),
    { timeout: 30_000 },
  );
  const reopenedUrl = new URL(page.url()).pathname + "/edit";
  await page.goto(getFrontendUrl(reopenedUrl), {
    waitUntil: "domcontentloaded",
  });
  await variables.getByRole("tab", { name: "Variables", exact: true }).click();
  await page.getByRole("button", { name: "rows", exact: true }).click();
  await expect(page.getByRole("combobox", { name: "minimum", exact: true }))
    .toHaveValue("minimum");
  await expect(page.locator(".cm-content")).toContainText("> $minimum");
  await expect(page.locator(".cm-content")).toContainText("OR");
});
