import { expect, test } from "@playwright/test";
import {
  getBackendUrl,
  getDefaultSpaceId,
  getFrontendUrl,
  waitForServers,
} from "./lib/client.ts";

test("A05 @smoke: text-edited SQL keeps its typed integer argument default after saving", async ({ page, request }) => {
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
  const formsResponse = await request.get(
    getBackendUrl(`/spaces/${spaceId}/forms`),
  );
  expect(formsResponse.ok()).toBe(true);
  const forms = await formsResponse.json() as Array<{
    name: string;
    fields?: Record<string, { sql_column?: string }>;
  }>;
  const form = forms.find((candidate) => candidate.name === formName);
  const amountColumn = form?.fields?.amount?.sql_column;
  expect(amountColumn).toBeTruthy();

  await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/new`), {
    waitUntil: "domcontentloaded",
  });
  await page.getByRole("button", { name: "Form", exact: true }).click();
  await page.getByRole("dialog", { name: "Form", exact: true }).getByRole(
    "button",
    { name: formName, exact: true },
  ).click();
  const sqlEditor = page.getByRole("textbox", { name: "SQL", exact: true });
  const quote = (value: string) => `"${value.replaceAll('"', '""')}"`;
  const sql = `SELECT ${quote(amountColumn!)} AS "Amount" FROM ${quote(formName)} WHERE ${quote(amountColumn!)} >= $minimum ORDER BY ${quote(amountColumn!)} ASC`;
  await sqlEditor.fill(sql);
  await expect(sqlEditor).toHaveText(sql);
  await page.getByLabel("Query name", { exact: true }).fill(
    `Filter arguments ${formName}`,
  );

  await page.getByRole("textbox", { name: "Argument name", exact: true })
    .fill("minimum");
  await page.getByRole("button", { name: "Add argument", exact: true })
    .click();
  await page.getByRole("combobox", { name: "Type for minimum", exact: true })
    .selectOption("integer");
  await page.getByRole("checkbox", { name: "Default for minimum", exact: true })
    .check();
  await page.getByRole("textbox", { name: "Default for minimum", exact: true })
    .fill("10");
  await expect(sqlEditor).toContainText("$minimum");

  const runButton = page.getByRole("button", {
    name: "Run Query",
    exact: true,
  });
  await expect(runButton).toBeEnabled({ timeout: 30_000 });
  const runPromise = page.waitForResponse((response) =>
    response.request().method() === "POST" &&
    new URL(response.url()).pathname === `/api/spaces/${spaceId}/sql/query`
  );
  await runButton.click();
  const run = await runPromise;
  expect(run.ok(), await run.text()).toBe(true);
  expect(run.request().postDataJSON()).toMatchObject({
    parameters: { minimum: 10 },
    parameter_types: { minimum: "integer" },
  });
  await expect(page.getByRole("table")).toContainText("11", {
    timeout: 30_000,
  });
  const savePromise = page.waitForResponse((response) =>
    response.request().method() === "POST" &&
    new URL(response.url()).pathname === `/api/spaces/${spaceId}/sql`
  );
  await page.getByRole("button", { name: "Save", exact: true }).click();
  const save = await savePromise;
  expect([200, 201]).toContain(save.status());
  const savedId = (await save.json() as { id: string }).id;
  await page.waitForURL((url) => url.pathname === `/spaces/${spaceId}/sql`);

  await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${savedId}`));
  await expect(page.getByRole("textbox", { name: "SQL", exact: true }))
    .toContainText("$minimum");
  await expect(page.getByRole("combobox", {
    name: "Type for minimum",
    exact: true,
  })).toHaveValue("integer");
  await expect(page.getByRole("textbox", {
    name: "Default for minimum",
    exact: true,
  })).toHaveValue("10");
  const reopenedRunPromise = page.waitForResponse((response) =>
    response.request().method() === "POST" &&
    new URL(response.url()).pathname === `/api/spaces/${spaceId}/sql/query`
  );
  await page.getByRole("button", { name: "Run Query", exact: true }).click();
  const reopenedRun = await reopenedRunPromise;
  expect(reopenedRun.ok(), await reopenedRun.text()).toBe(true);
  expect(reopenedRun.request().postDataJSON()).toMatchObject({
    parameters: { minimum: 10 },
    parameter_types: { minimum: "integer" },
  });
  await expect(page.getByRole("table")).toContainText("11", {
    timeout: 30_000,
  });
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

    await page.getByRole("textbox", { name: "Argument name", exact: true })
      .fill("take");
    await page.getByRole("button", { name: "Add argument", exact: true })
      .click();
    await page.getByRole("combobox", { name: "Type for take", exact: true })
      .selectOption("integer");
    await page.getByRole("checkbox", { name: "Default for take", exact: true })
      .check();
    await page.getByRole("textbox", { name: "Default for take", exact: true })
      .fill("1");
    await expect(editor).toContainText("LIMIT $take");
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
      expect(result.rows.map((row) => row[amountColumn])).toEqual(
        expectedAmounts,
      );
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
      await expect(
        page.getByRole("combobox", {
          name: "Type for take",
          exact: true,
        }),
      ).toHaveValue("integer");
      await expect(
        page.getByRole("textbox", {
          name: "Default for take",
          exact: true,
        }),
      ).toHaveValue("1");

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
