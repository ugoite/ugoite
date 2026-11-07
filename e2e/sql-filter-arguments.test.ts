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
  await columns.getByRole("button", { name: "Choose fields", exact: true })
    .click();
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

test(
  "A05 @smoke: named LIMIT argument applies its integer default and saved override",
  async ({ page, request }) => {
    test.setTimeout(120_000);
    await waitForServers(request);
    const spaceId = await getDefaultSpaceId(request);
    const formName = `LimitArgs${crypto.randomUUID().slice(0, 8)}`;
    const formResponse = await request.post(
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
    expect([200, 201]).toContain(formResponse.status());
    for (const amount of [1, 2, 3]) {
      const entry = await request.post(
        getBackendUrl(`/spaces/${spaceId}/entries`),
        { data: { form: formName, fields: { amount } } },
      );
      expect(entry.status()).toBe(201);
    }
    const formsResponse = await request.get(
      getBackendUrl(`/spaces/${spaceId}/forms`),
    );
    expect(formsResponse.ok()).toBe(true);
    const forms = await formsResponse.json() as Array<{
      name: string;
      sql_relation?: string;
      fields?: Record<string, { sql_column?: string }>;
    }>;
    const form = forms.find((candidate) => candidate.name === formName);
    const relation = form?.sql_relation;
    const amountColumn = form?.fields?.amount?.sql_column;
    if (!relation || !amountColumn) {
      throw new Error(`Form ${formName} is missing its SQL schema`);
    }
    const sql =
      `SELECT "${amountColumn}" FROM "${relation}" ORDER BY "${amountColumn}" ASC LIMIT $take`;

    await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/new`), {
      waitUntil: "domcontentloaded",
    });
    const editor = page.getByRole("textbox", { name: "SQL", exact: true });
    await editor.fill(sql);
    await expect(editor).toHaveText(sql);
    await page.getByLabel("Query name", { exact: true }).fill(
      `Named limit ${formName}`,
    );

    await page.getByRole("button", { name: "Arguments", exact: true }).click();
    const args = page.getByRole("dialog", { name: "Arguments", exact: true });
    await args.getByRole("textbox", { name: "Argument name", exact: true })
      .fill("take");
    await args.getByRole("button", { name: "Add argument", exact: true })
      .click();
    await args.getByRole("combobox", { name: "Type for take", exact: true })
      .selectOption("integer");
    await args.getByRole("checkbox", { name: "Default for take", exact: true })
      .check();
    await args.getByRole("textbox", { name: "Default for take", exact: true })
      .fill("1");
    await expect(editor).toContainText("LIMIT $take");
    await args.locator("footer").getByRole("button", { name: "Close" })
      .click();
    const runButton = page.getByRole("button", {
      name: "Run Query",
      exact: true,
    });

    const runAndExpectAmounts = async (
      take: number,
      expectedAmounts: number[],
    ) => {
      await expect(runButton).toBeEnabled();
      const responsePromise = page.waitForResponse((response) =>
        response.request().method() === "POST" &&
        new URL(response.url()).pathname === `/api/spaces/${spaceId}/sql/query`
      );
      await page.getByRole("button", { name: "Run Query", exact: true })
        .click();
      const response = await responsePromise;
      const requestBody = response.request().postDataJSON() as {
        parameters: Record<string, unknown>;
        parameter_types: Record<string, string>;
      };
      expect(requestBody).toMatchObject({
        parameters: { take },
        parameter_types: { take: "integer" },
      });
      expect(response.ok(), await response.text()).toBe(true);
      const result = await response.json() as {
        rows: Record<string, unknown>[];
      };
      expect(result.rows.map((row) => row.amount)).toEqual(expectedAmounts);
    };

    await expect(page.getByRole("textbox", { name: "take", exact: true }))
      .toHaveValue("1");
    await runAndExpectAmounts(1, [1]);
    const takeInput = page.getByRole("textbox", { name: "take", exact: true });
    await takeInput.fill("2");
    await runAndExpectAmounts(2, [1, 2]);

    let savedSqlId: string | undefined;
    const saveResponsePromise = page.waitForResponse((response) =>
      response.request().method() === "POST" &&
      new URL(response.url()).pathname === `/api/spaces/${spaceId}/sql`
    );
    await page.getByRole("button", { name: "Save", exact: true }).click();
    const saveResponse = await saveResponsePromise;
    try {
      const saveBody = await saveResponse.json() as { id?: string };
      savedSqlId = saveBody.id;
      expect([200, 201], JSON.stringify(saveBody)).toContain(
        saveResponse.status(),
      );
      if (!savedSqlId) throw new Error("Saved SQL response omitted its id");

      const readback = await request.get(
        getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
      );
      expect(readback.ok(), await readback.text()).toBe(true);
      const saved = await readback.json() as {
        sql: string;
        variables: Array<{ name: string; type: string; default?: number }>;
      };
      expect(saved.sql).toContain("LIMIT $take");
      expect(saved.variables).toContainEqual(expect.objectContaining({
        name: "take",
        type: "integer",
        default: 1,
      }));

      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`), {
        waitUntil: "domcontentloaded",
      });
      await expect(editor).toContainText("LIMIT $take");
      await page.getByRole("button", { name: "Arguments", exact: true })
        .click();
      const reopenedArgs = page.getByRole("dialog", {
        name: "Arguments",
        exact: true,
      });
      await expect(
        reopenedArgs.getByRole("combobox", {
          name: "Type for take",
          exact: true,
        }),
      ).toHaveValue("integer");
      await expect(
        reopenedArgs.getByRole("textbox", {
          name: "Default for take",
          exact: true,
        }),
      ).toHaveValue("1");
      await reopenedArgs.locator("footer").getByRole("button", {
        name: "Close",
      })
        .click();

      const reopenedTakeInput = page.getByRole("textbox", {
        name: "take",
        exact: true,
      });
      await reopenedTakeInput.fill("2");
      await runAndExpectAmounts(2, [1, 2]);
    } finally {
      if (savedSqlId) {
        await request.delete(
          getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
        );
      }
    }
  },
);
