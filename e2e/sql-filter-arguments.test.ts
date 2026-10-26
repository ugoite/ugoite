import { expect, test } from "@playwright/test";
import {
  getBackendUrl,
  getDefaultSpaceId,
  getFrontendUrl,
  waitForServers,
} from "./lib/client.ts";

test("SQL filter argument creation preserves typed declarations through undo, run, and save", async ({ page, request }) => {
  test.setTimeout(120_000);
  await waitForServers(request);
  const spaceId = await getDefaultSpaceId(request);
  const formName = `FilterArgs${crypto.randomUUID().slice(0, 8)}`;
  const created = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: {
        name: formName,
        version: 1,
        template: `# ${formName}\n\n## amount\n`,
        fields: {
          amount: { type: "integer", required: true, label: "Amount" },
        },
      },
    },
  );
  expect([200, 201]).toContain(created.status());
  const entry = await request.post(
    getBackendUrl(`/spaces/${spaceId}/entries`),
    { data: { form: formName, fields: { amount: 11 } } },
  );
  expect(entry.status()).toBe(201);

  await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/new`), {
    waitUntil: "domcontentloaded",
  });
  const sqlEditor = page.getByRole("textbox", { name: "SQL", exact: true });
  await expect(sqlEditor).toHaveText("SELECT 1");
  await sqlEditor.click();
  await sqlEditor.press("ControlOrMeta+A");
  await sqlEditor.pressSequentially("SELECT *");
  await expect(sqlEditor).toHaveText("SELECT *");
  await page.getByRole("button", { name: "Form", exact: true }).click();
  await page.getByRole("dialog", { name: "Form", exact: true }).getByRole(
    "button",
    { name: formName, exact: true },
  ).click();

  await page.getByRole("button", { name: "Columns", exact: true }).click();
  const columns = page.getByRole("dialog", { name: "Columns", exact: true });
  await columns.getByRole("combobox", { name: "Add a field", exact: true })
    .selectOption({ label: "Amount" });
  await columns.getByRole("button", { name: "Add", exact: true }).click();
  const fields = columns.getByRole("list", { name: "Result fields" });
  await expect(fields.getByText("Amount", { exact: true })).toBeVisible();
  const fieldNames = await fields.locator(".sql-columns-editor__name")
    .allTextContents();
  for (const name of fieldNames.filter((name) => name !== "Amount")) {
    const remove = columns.getByRole("button", {
      name: `Remove ${name}`,
      exact: true,
    });
    await expect(remove).not.toHaveAttribute("aria-disabled", "true");
    await remove.click();
    await expect(remove).toHaveCount(0);
  }
  await expect(fields.getByRole("listitem")).toHaveText([/Amount/]);
  const outputAlias = columns.getByRole("textbox", {
    name: "Output name for result 1",
    exact: true,
  });
  await outputAlias.fill("amount_alias");
  await outputAlias.press("Tab");
  await expect(sqlEditor).toContainText(/AS\s+"amount_alias"/);
  await columns.locator("footer").getByRole("button", { name: "Close" })
    .click();

  await page.getByRole("button", { name: "Filter", exact: true }).click();
  const filter = page.getByRole("dialog", { name: "Filter", exact: true });
  await filter.getByRole("combobox", { name: "Column for new filter" })
    .selectOption({
      label: "Amount",
    });
  await filter.getByRole("combobox", { name: "Operator" }).selectOption(
    "gte",
  );
  await filter.getByRole("combobox", { name: "Value source" }).selectOption(
    "argument",
  );
  await filter.getByRole("textbox", { name: "New argument name" }).fill(
    "minimum",
  );
  await filter.getByRole("textbox", { name: "Default value (optional)" })
    .fill("10");
  await expect(filter.getByRole("combobox", { name: "Value type" }))
    .toHaveCount(0);
  await filter.getByRole("button", { name: "Add filter", exact: true })
    .click();
  await expect(sqlEditor).toContainText(/WHERE\s+.+\s*>?=\s+\$minimum/);
  await filter.getByRole("button", { name: "Undo", exact: true }).click();
  await expect(sqlEditor).not.toContainText("$minimum");
  await filter.getByRole("button", { name: "Redo", exact: true }).click();
  await expect(sqlEditor).toContainText(/WHERE\s+.+\s*>?=\s+\$minimum/);
  await filter.locator("footer").getByRole("button", { name: "Close" })
    .click();

  await page.getByRole("button", { name: "Arguments", exact: true }).click();
  const args = page.getByRole("dialog", { name: "Arguments", exact: true });
  await expect(
    args.getByRole("combobox", { name: "Type for minimum", exact: true }),
  )
    .toHaveValue("integer");
  await expect(args.getByRole("textbox", { name: "Default for minimum" }))
    .toHaveValue("10");
  await args.locator("footer").getByRole("button", { name: "Close" }).click();

  await page.getByRole("button", { name: "Run Query", exact: true }).click();
  await expect(page.getByRole("table")).toContainText("11", {
    timeout: 30_000,
  });
  await expect(
    page.getByRole("table").getByRole("columnheader", {
      name: "amount_alias",
      exact: true,
    }),
  ).toBeVisible();
  await page.getByLabel("Query name", { exact: true }).fill(
    `Filter arguments ${formName}`,
  );
  const savedResponse = page.waitForResponse((response) =>
    response.request().method() === "POST" &&
    new URL(response.url()).pathname === `/api/spaces/${spaceId}/sql`
  );
  await page.getByRole("button", { name: "Save", exact: true }).click();
  const save = await savedResponse;
  expect([200, 201]).toContain(save.status());
  const savedId = (await save.json() as { id: string }).id;
  await page.waitForURL((url) => url.pathname === `/spaces/${spaceId}/sql`);

  const savedResponseBody = await request.get(
    getBackendUrl(`/spaces/${spaceId}/sql/${savedId}`),
  );
  expect(savedResponseBody.ok()).toBe(true);
  const saved = await savedResponseBody.json() as {
    sql: string;
    variables: Array<{ name: string; type: string; default?: number }>;
  };
  expect(saved.sql).toContain('AS "amount_alias"');
  expect(saved.sql).toContain("$minimum");
  expect(saved.variables).toContainEqual(expect.objectContaining({
    name: "minimum",
    type: "integer",
    default: 10,
  }));

  await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${savedId}`));
  await expect(page.getByRole("textbox", { name: "SQL", exact: true }))
    .toContainText("$minimum");
  await page.getByRole("button", { name: "Arguments", exact: true }).click();
  const reopenedArgs = page.getByRole("dialog", {
    name: "Arguments",
    exact: true,
  });
  await expect(
    reopenedArgs.getByRole("combobox", {
      name: "Type for minimum",
      exact: true,
    }),
  )
    .toHaveValue("integer");
  await expect(
    reopenedArgs.getByRole("textbox", { name: "Default for minimum" }),
  ).toHaveValue("10");
});
