import { expect, test } from "@playwright/test";
import {
  ensureDefaultForm,
  getBackendUrl,
  getDefaultFormRelation,
  getDefaultSpaceId,
  getFrontendUrl,
  waitForServers,
} from "./lib/client.ts";

test.describe("Saved SQL route", () => {
  let spaceId = "";

  test.beforeAll(async ({ request }) => {
    await waitForServers(request);
    spaceId = await getDefaultSpaceId(request);
  });

  test(
    "A07 @smoke: code-edited aliased ARRAY survives a scoped SQL structure edit and save",
    async ({ page, request }) => {
      const name = `A07 aliased ARRAY ${Date.now()}`;
      const sql = "SELECT ARRAY[] AS empty";
      const editedSql = `${sql} LIMIT 1`;
      let savedSqlId: string | undefined;

      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/new`), {
        waitUntil: "domcontentloaded",
      });

      const editor = page.getByRole("textbox", { name: "SQL", exact: true });
      const initialPlan = page.waitForResponse((response) =>
        response.request().method() === "POST" &&
        new URL(response.url()).pathname ===
          `/api/spaces/${spaceId}/sql/query/plan` &&
        response.request().postDataJSON().sql === sql
      );
      await editor.fill(sql);
      const initialPlanResponse = await initialPlan;
      expect(initialPlanResponse.ok(), await initialPlanResponse.text()).toBe(
        true,
      );
      await expect(editor).toHaveText(sql);
      await page.getByLabel("Query name", { exact: true }).fill(name);

      const toolbar = page.getByRole("group", { name: "SQL", exact: true });
      await toolbar.getByRole("button", { name: "Structure", exact: true })
        .click();
      const structureOptions = page.getByRole("group", {
        name: "Structure options",
      });
      await structureOptions.getByRole("button", {
        name: "SQL tree",
        exact: true,
      }).click();

      const treeDialog = page.getByRole("dialog", { name: "SQL tree" });
      const aliasedProjection = treeDialog.getByRole("treeitem", {
        name: /Selected expression: ARRAY\[\] AS empty/,
      });
      await aliasedProjection.click();
      const expressionKind = treeDialog.getByRole("combobox", {
        name: "Expression value",
        exact: true,
      });
      await expect(expressionKind).toBeVisible();
      await expect(
        expressionKind.getByRole("option", { name: "ARRAY literal" }),
      ).toHaveCount(0);
      await treeDialog.getByRole("button", { name: "Close", exact: true })
        .last().click();

      await toolbar.getByRole("button", { name: "Structure", exact: true })
        .click();
      const rowLimitAction = page.getByRole("group", {
        name: "Structure options",
      }).getByRole("button", { name: "Row limit", exact: true });
      await expect(rowLimitAction).toBeEnabled();
      await rowLimitAction.click();

      const rowLimitDialog = page.getByRole("dialog", { name: "Row limit" });
      const tree = rowLimitDialog.getByRole("tree", {
        name: "SQL syntax tree",
      });
      await tree.getByRole("treeitem", { name: /Query/ }).click();
      await rowLimitDialog.getByLabel("Limit", { exact: true }).fill("1");
      const setLimit = rowLimitDialog.getByRole("button", {
        name: "Set row limit",
        exact: true,
      });
      await expect(setLimit).toBeEnabled();
      await setLimit.click();
      await expect(editor).toHaveText(sql);
      await rowLimitDialog.getByRole("button", { name: "Apply", exact: true })
        .click();

      await expect(editor).toHaveText(editedSql);
      await expect(editor).toContainText("ARRAY[] AS empty");
      await rowLimitDialog.locator("header").getByRole("button", {
        name: "Close",
        exact: true,
      }).click();
      await expect(rowLimitDialog).toHaveCount(0);

      const run = page.waitForResponse((response) =>
        response.request().method() === "POST" &&
        new URL(response.url()).pathname ===
          `/api/spaces/${spaceId}/sql/query`
      );
      await page.getByRole("button", { name: "Run Query", exact: true })
        .click();
      const runResponse = await run;
      expect(runResponse.ok(), await runResponse.text()).toBe(true);
      const result = await runResponse.json() as {
        columns: string[];
        rows: Record<string, unknown>[];
      };
      expect(result.columns).toEqual(["empty"]);
      expect(result.rows).toEqual([{ empty: [] }]);

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
        expect((await readback.json() as { sql: string }).sql).toBe(editedSql);

        await page.goto(
          getFrontendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
          {
            waitUntil: "domcontentloaded",
          },
        );
        await expect(page.getByRole("textbox", { name: "SQL", exact: true }))
          .toHaveText(editedSql);
      } finally {
        if (savedSqlId) {
          await request.delete(
            getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
          );
        }
      }
    },
  );

  test("REQ-FE-061: saved SQL route shows its empty state and keeps current navigation", async ({ page }) => {
    await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql`), {
      waitUntil: "domcontentloaded",
    });

    await expect(page.getByRole("heading", { level: 1, name: "Saved SQL" }))
      .toBeVisible();
    await expect(page.getByText("No saved SQL", { exact: true }))
      .toBeVisible();
    await expect(page.getByRole("link", { name: "New SQL query" }))
      .toHaveAttribute("href", `/spaces/${spaceId}/sql/new`);
    await expect(page.getByRole("link", { name: "Search" }).first())
      .toHaveAttribute("href", `/spaces/${spaceId}/search`);
    await expect(page.getByRole("link", { name: "Forms" }).first())
      .toHaveAttribute("href", `/spaces/${spaceId}/forms`);
    await expect(page.getByRole("link", { name: "Home" }).first())
      .toHaveAttribute("href", `/spaces/${spaceId}/dashboard`);
  });

  test("REQ-FE-062: saved SQL detail loads query text and runs variable-free queries inline", async ({ page, request }) => {
    await ensureDefaultForm(request, spaceId);
    const relation = await getDefaultFormRelation(request, spaceId);
    const sqlCreate = await request.post(
      getBackendUrl(`/spaces/${spaceId}/sql`),
      {
        data: {
          name: `Saved Detail Query ${Date.now()}`,
          kind: "user-query",
          sql:
            `SELECT * FROM "${relation}" ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 1`,
          variables: [],
        },
      },
    );
    expect([200, 201]).toContain(sqlCreate.status());
    const savedSql = (await sqlCreate.json()) as { id: string };

    try {
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${savedSql.id}`), {
        waitUntil: "domcontentloaded",
      });

      await expect(
        page.getByRole("heading", { level: 1, name: /Saved Detail Query/ }),
      ).toBeVisible();
      await expect(page.locator(".ui-sql-editor")).toContainText(
        `SELECT * FROM "${relation}" ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 1`,
      );
      const queryResponsePromise = page.waitForResponse((response) => {
        const url = new URL(response.url());
        return response.request().method() === "POST" &&
          url.pathname === `/api/spaces/${spaceId}/sql/query`;
      });
      await page.getByRole("button", { name: "Run Query" }).click();
      const queryResponse = await queryResponsePromise;
      expect(queryResponse.ok()).toBe(true);
      await expect(page).toHaveURL(
        new RegExp(`/spaces/${spaceId}/sql/${savedSql.id}$`),
      );
      await expect(
        page.getByRole("region", { name: /^(SQL results|SQL 結果)$/ }).first(),
      )
        .toBeVisible();
    } finally {
      await request.delete(
        getBackendUrl(`/spaces/${spaceId}/sql/${savedSql.id}`),
      );
    }
  });

  test("REQ-FE-080: Saved SQL Run applies a declared default and preserves an unset parameter", async ({ page, request }) => {
    const name = `Transient integer parameter ${Date.now()}`;
    const createResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/sql`),
      {
        data: {
          name,
          kind: "user-query",
          sql: "SELECT $value AS result",
          variables: [{
            name: "value",
            type: "integer",
            description: "Value returned by the Saved SQL query.",
            default: 7,
          }],
        },
      },
    );
    const createBody = await createResponse.text();
    expect([200, 201], createBody).toContain(createResponse.status());
    const savedSqlId = (JSON.parse(createBody) as { id: string }).id;
    const variablesPath = `/spaces/${spaceId}/sql/${savedSqlId}/variables`;
    const queryRequests: Array<{
      parameters: Record<string, unknown>;
      parameter_types: Record<string, string>;
    }> = [];
    page.on("request", (outgoing) => {
      if (
        outgoing.method() === "POST" &&
        new URL(outgoing.url()).pathname ===
          `/api/spaces/${spaceId}/sql/query`
      ) queryRequests.push(outgoing.postDataJSON());
    });

    try {
      const savedSqlLoad = page.waitForResponse((response) =>
        response.request().method() === "GET" &&
        new URL(response.url()).pathname ===
          `/api/spaces/${spaceId}/sql/${savedSqlId}`
      );
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`), {
        waitUntil: "domcontentloaded",
      });
      const savedSqlResponse = await savedSqlLoad;
      expect(savedSqlResponse.ok()).toBe(true);
      await expect(page.getByRole("heading", { level: 1, name })).toBeVisible();
      await page.getByRole("link", { name: "Open Variables", exact: true })
        .click();
      await expect(page).toHaveURL(new RegExp(`${variablesPath}$`));
      const valueInput = page.getByRole("textbox", {
        name: "value integer",
        exact: true,
      });
      await expect(valueInput).toHaveValue("7");
      expect(queryRequests).toEqual([]);

      const defaultRun = page.waitForResponse((response) =>
        response.request().method() === "POST" &&
        new URL(response.url()).pathname ===
          `/api/spaces/${spaceId}/sql/query`
      );
      await page.getByRole("button", { name: "Run", exact: true }).click();
      const defaultResponse = await defaultRun;
      expect(defaultResponse.ok(), await defaultResponse.text()).toBe(true);
      expect(queryRequests).toMatchObject([{
        parameters: { value: 7 },
        parameter_types: { value: "integer" },
      }]);
      const defaultResult = await defaultResponse.json() as {
        columns: string[];
        rows: Array<Record<string, unknown>>;
      };
      expect(defaultResult.columns).toEqual(["result"]);
      expect(defaultResult.rows).toEqual([{ result: 7 }]);
      await expect(page.getByRole("table", { name: "SQL results" }))
        .toContainText("7");

      const reloadedSavedSql = page.waitForResponse((response) =>
        response.request().method() === "GET" &&
        new URL(response.url()).pathname ===
          `/api/spaces/${spaceId}/sql/${savedSqlId}`
      );
      await page.goto(getFrontendUrl(variablesPath), {
        waitUntil: "domcontentloaded",
      });
      const reloadedSavedSqlResponse = await reloadedSavedSql;
      expect(reloadedSavedSqlResponse.ok()).toBe(true);
      const unsetInput = page.getByRole("textbox", {
        name: "value integer",
        exact: true,
      });
      await expect(unsetInput).toHaveValue("7");
      await unsetInput.fill("");
      const unsetRun = page.waitForResponse((response) =>
        response.request().method() === "POST" &&
        new URL(response.url()).pathname ===
          `/api/spaces/${spaceId}/sql/query`
      );
      await page.getByRole("button", { name: "Run", exact: true }).click();
      const unsetResponse = await unsetRun;
      expect(unsetResponse.ok(), await unsetResponse.text()).toBe(true);
      expect(queryRequests).toMatchObject([
        {
          parameters: { value: 7 },
          parameter_types: { value: "integer" },
        },
        {
          parameters: { value: null },
          parameter_types: { value: "integer" },
        },
      ]);
      const unsetResult = await unsetResponse.json() as {
        columns: string[];
        rows: Array<Record<string, unknown>>;
      };
      expect(unsetResult.columns).toEqual(["result"]);
      expect(unsetResult.rows).toHaveLength(1);
      expect(unsetResult.rows[0]?.result ?? null).toBeNull();
      await expect(page.getByRole("table", { name: "SQL results" }))
        .toContainText("—");
    } finally {
      await request.delete(
        getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
      );
    }
  });

  test("invalid SQL draft shows a diagnostic, blocks unsafe actions, and keeps comments through repair", async ({ page, request }) => {
    const name = `Invalid SQL draft ${Date.now()}`;
    const rowsSql = Array.from(
      { length: 101 },
      (_, index) => `SELECT ${index + 1} AS value`,
    ).join(" UNION ALL ");
    const originalSql = `${rowsSql} ORDER BY value -- keep this comment`;
    const createResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/sql`),
      {
        data: {
          name,
          kind: "user-query",
          sql: originalSql,
          variables: [],
        },
      },
    );
    const createBody = await createResponse.text();
    expect([200, 201], createBody).toContain(createResponse.status());
    const savedSqlId = (JSON.parse(createBody) as { id: string }).id;

    try {
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`), {
        waitUntil: "domcontentloaded",
      });
      const editor = page.getByRole("textbox", { name: "SQL", exact: true });
      await expect(editor).toContainText(originalSql);

      const runRequest = () =>
        page.waitForRequest((request) =>
          request.method() === "POST" &&
          new URL(request.url()).pathname === `/api/spaces/${spaceId}/sql/query`
        );
      const initialRun = runRequest();
      await page.getByRole("button", { name: "Run Query", exact: true })
        .click();
      await initialRun;
      await expect(
        page.getByRole("region", { name: /^(SQL results|SQL 結果)$/ }).first(),
      )
        .toBeVisible();
      const nextPage = page.getByRole("button", { name: "Next", exact: true });
      await expect(nextPage).toBeVisible();
      const countRequest = page.waitForRequest((request) =>
        request.method() === "POST" &&
        new URL(request.url()).pathname ===
          `/api/spaces/${spaceId}/sql/query/count`
      );
      await page.getByRole("button", { name: "Count rows", exact: true })
        .click();
      await countRequest;
      await nextPage.click();
      await expect(page.getByRole("button", { name: "Previous", exact: true }))
        .toBeVisible();

      const invalidSql = originalSql.replace(
        "SELECT 1 AS value",
        "SELECT FROM AS value",
      );
      const invalidPlan = page.waitForResponse((response) =>
        response.request().method() === "POST" &&
        new URL(response.url()).pathname ===
          `/api/spaces/${spaceId}/sql/query/plan` &&
        response.request().postDataJSON().sql === invalidSql
      );
      const queryRequests: string[] = [];
      page.on("request", (request) => {
        if (
          request.method() === "POST" &&
          new URL(request.url()).pathname === `/api/spaces/${spaceId}/sql/query`
        ) queryRequests.push(request.url());
      });
      await editor.fill(invalidSql);
      const planResponse = await invalidPlan;
      expect(planResponse.ok()).toBe(false);
      await expect(page.getByRole("button", { name: "Save", exact: true }))
        .toBeDisabled();
      await expect(
        page.getByRole("region", { name: /^(SQL results|SQL 結果)$/ }),
      )
        .toHaveCount(0);
      await expect(
        page.getByRole("button", { name: "Count rows", exact: true }),
      )
        .toHaveCount(0);
      await expect(page.getByRole("button", { name: "Next", exact: true }))
        .toHaveCount(0);

      const runButton = page.getByRole("button", {
        name: "Run Query",
        exact: true,
      });
      await expect(runButton).toBeDisabled();
      await expect(runButton).toHaveAccessibleDescription(/invalid SQL query/);
      await expect.poll(() => queryRequests.length).toBe(0);

      const repairedSql = originalSql.replace(
        "SELECT 1 AS value",
        "SELECT 0 AS value",
      );
      await editor.fill(repairedSql);
      await expect(page.getByRole("button", { name: "Save", exact: true }))
        .toBeEnabled();
      await expect(editor).toContainText("-- keep this comment");

      const savedUpdate = page.waitForResponse((response) =>
        response.request().method() === "PUT" &&
        new URL(response.url()).pathname ===
          `/api/spaces/${spaceId}/sql/${savedSqlId}`
      );
      await page.getByRole("button", { name: "Save", exact: true }).click();
      const updateResponse = await savedUpdate;
      expect(updateResponse.ok(), await updateResponse.text()).toBe(true);
      const persisted = await request.get(
        getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
      );
      expect(persisted.ok()).toBe(true);
      expect((await persisted.json() as { sql: string }).sql).toBe(repairedSql);
    } finally {
      await request.delete(
        getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
      );
    }
  });

  test("@smoke SQL tree edits and executes a SQL-authored ARRAY literal draft", async ({ page, request }) => {
    const name = `Array order ${Date.now()}`;
    const created = await request.post(
      getBackendUrl(`/spaces/${spaceId}/sql`),
      {
        data: {
          name,
          kind: "user-query",
          sql: "SELECT ARRAY[4, 9]",
          variables: [],
        },
      },
    );
    const body = await created.json() as { id?: string };
    expect([200, 201], JSON.stringify(body)).toContain(created.status());
    const savedSqlId = body.id!;

    try {
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`), {
        waitUntil: "domcontentloaded",
      });
      const editor = page.locator(".ui-sql-editor");
      await expect(editor).toContainText("ARRAY[4, 9]");
      await page.getByRole("button", { name: "Structure", exact: true })
        .click();
      await page.getByRole("button", { name: "SQL tree", exact: true }).click();
      const tree = page.getByRole("dialog", { name: "SQL tree" });
      await tree.getByRole("treeitem", { name: /Value: 9/ }).first().click();
      await tree.getByRole("button", { name: "Move ARRAY element up" }).click();
      await tree.getByRole("button", { name: "Apply", exact: true }).click();
      await expect(editor).toContainText("ARRAY[9, 4]");
      await tree.getByRole("button", { name: "Close", exact: true }).first()
        .click();

      const ran = page.waitForResponse((response) =>
        response.request().method() === "POST" &&
        new URL(response.url()).pathname === `/api/spaces/${spaceId}/sql/query`
      );
      await page.getByRole("button", { name: "Run Query", exact: true })
        .click();
      const result = await ran;
      expect(result.ok(), await result.text()).toBe(true);
      const output = await result.json() as {
        columns: string[];
        rows: Record<string, unknown>[];
      };
      expect(output.columns).toHaveLength(1);
      expect(output.rows).toEqual([{ [output.columns[0]]: [9, 4] }]);
      await expect(
        page.getByRole("region", { name: /^(SQL results|SQL 結果)$/ }).first(),
      )
        .toBeVisible();
      await expect(page.locator('p[role="alert"]')).toHaveCount(0);
    } finally {
      await request.delete(
        getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
      );
    }
  });

  test("@smoke SQL Workspace creates, runs, saves, and reloads the exact EXISTS query", async ({ page }) => {
    const existsSql = "SELECT 1 WHERE EXISTS (SELECT 1)";
    const queryName = `Exists round trip ${Date.now()}`;
    let savedSqlId: string | undefined;
    try {
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/new`), {
        waitUntil: "domcontentloaded",
      });
      const sqlEditor = page.getByRole("textbox", {
        name: "SQL",
        exact: true,
      });
      await sqlEditor.fill(existsSql);
      await expect(sqlEditor).toContainText(existsSql);

      const firstRun = page.waitForResponse((response) =>
        response.request().method() === "POST" &&
        new URL(response.url()).pathname === `/api/spaces/${spaceId}/sql/query`
      );
      await page.getByRole("button", { name: "Run Query", exact: true })
        .click();
      const firstResult = await firstRun;
      expect(firstResult.ok(), await firstResult.text()).toBe(true);
      const firstOutput = await firstResult.json() as {
        columns: string[];
        rows: Record<string, unknown>[];
      };
      expect(firstOutput.columns).toHaveLength(1);
      expect(firstOutput.rows).toEqual([{ [firstOutput.columns[0]]: 1 }]);

      await page.getByLabel("Query name").fill(queryName);
      const createSavedQuery = page.waitForResponse((response) =>
        response.request().method() === "POST" &&
        new URL(response.url()).pathname === `/api/spaces/${spaceId}/sql`
      );
      await page.getByRole("button", { name: "Save", exact: true }).click();
      const created = await createSavedQuery;
      expect([200, 201], await created.text()).toContain(created.status());
      savedSqlId = (await created.json() as { id: string }).id;
      await expect(page).toHaveURL(new RegExp(`/spaces/${spaceId}/sql$`));
      await page.getByRole("link", { name: new RegExp(queryName) }).click();
      await expect(page).toHaveURL(
        new RegExp(`/spaces/${spaceId}/sql/${savedSqlId}$`),
      );

      await page.reload({ waitUntil: "domcontentloaded" });
      await expect(page.getByRole("heading", { level: 1, name: queryName }))
        .toBeVisible();
      await expect(page.getByRole("textbox", { name: "SQL", exact: true }))
        .toContainText(existsSql);
      const persisted = await page.request.get(
        getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
      );
      expect(persisted.ok()).toBe(true);
      expect((await persisted.json() as { sql: string }).sql).toBe(existsSql);

      const reloadRun = page.waitForResponse((response) =>
        response.request().method() === "POST" &&
        new URL(response.url()).pathname === `/api/spaces/${spaceId}/sql/query`
      );
      await page.getByRole("button", { name: "Run Query", exact: true })
        .click();
      const reloadResult = await reloadRun;
      expect(reloadResult.ok(), await reloadResult.text()).toBe(true);
      const reloadOutput = await reloadResult.json() as {
        columns: string[];
        rows: Record<string, unknown>[];
      };
      expect(reloadOutput.columns).toHaveLength(1);
      expect(reloadOutput.rows).toEqual([{ [reloadOutput.columns[0]]: 1 }]);
      await expect(
        page.getByRole("region", { name: /^(SQL results|SQL 結果)$/ }).first(),
      ).toBeVisible();
    } finally {
      if (savedSqlId) {
        await page.request.delete(
          getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
        );
      }
    }
  });

  test("@smoke SQL tree authors IN and NOT IN and records HTTP 422 planning rejections", async ({ page }) => {
    const inSql = "SELECT (1) IN (SELECT 1)";
    const notInSql = "SELECT (1) NOT IN (SELECT 1)";
    const assertRejectedForm = async (
      choice: "in" | "not_in",
      expectedSql: string,
    ) => {
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/new`), {
        waitUntil: "domcontentloaded",
      });
      const editor = page.locator(".ui-sql-editor");
      await expect(editor).toContainText("SELECT 1");
      await page.getByRole("button", { name: "Structure", exact: true })
        .click();
      await page.getByRole("button", { name: "SQL tree", exact: true }).click();
      const tree = page.getByRole("dialog", { name: "SQL tree" });
      await tree.getByRole("treeitem", { name: /Value: 1/ }).first().click();
      await tree.getByLabel("Subquery condition").selectOption(choice);
      const plan = page.waitForResponse((response) =>
        response.request().method() === "POST" &&
        new URL(response.url()).pathname ===
          `/api/spaces/${spaceId}/sql/query/plan` &&
        response.request().postDataJSON().sql === expectedSql
      );
      await tree.getByRole("button", { name: "Apply", exact: true }).click();
      await expect(editor.locator(".cm-content")).toHaveText(expectedSql);
      const response = await plan;
      expect(response.status()).toBe(422);
      await expect(page.getByRole("button", { name: "Run Query", exact: true }))
        .toBeDisabled();
      await expect(page.getByRole("button", { name: "Save", exact: true }))
        .toBeDisabled();
    };

    await assertRejectedForm("in", inSql);
    await assertRejectedForm("not_in", notInSql);
  });

  test("REQ-FE-063 @smoke: scalar SELECT without a Form can use SQL structure editing", async ({ page, request }) => {
    const name = `Scalar SQL ${Date.now()}`;
    const createResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/sql`),
      {
        data: {
          name,
          kind: "user-query",
          sql: "SELECT 1",
          variables: [],
        },
      },
    );
    const createResult = await createResponse.json() as { id?: string };
    expect([200, 201], JSON.stringify(createResult)).toContain(
      createResponse.status(),
    );
    const savedSqlId = createResult.id!;

    try {
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`), {
        waitUntil: "domcontentloaded",
      });
      await expect(page.locator(".ui-sql-editor")).toContainText("SELECT 1");

      const structureAction = page.getByRole("button", {
        name: "Structure",
        exact: true,
      });
      await expect(structureAction).toBeEnabled();
      await structureAction.click();
      const structureOptions = page.getByRole("group", {
        name: "Structure options",
      });
      const distinctAction = structureOptions.getByRole("button", {
        name: "Distinct",
        exact: true,
      });
      await expect(distinctAction).toBeEnabled();
      await distinctAction.click();
      const dialog = page.getByRole("dialog", { name: "Distinct" });
      await dialog.getByLabel("Distinct rows", { exact: true }).check();
      await dialog.getByRole("button", { name: "Apply", exact: true }).click();

      await expect(page.locator(".ui-sql-editor")).toContainText(
        "SELECT DISTINCT 1",
      );
    } finally {
      await request.delete(
        getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
      );
    }
  });

  test("Form selection builds a relation from a relation-free SELECT without replacing SQL outside the seed", async ({ page, request }) => {
    await ensureDefaultForm(request, spaceId);
    const relation = await getDefaultFormRelation(request, spaceId);
    const name = `Relation-free SQL ${Date.now()}`;
    const createResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/sql`),
      {
        data: {
          name,
          kind: "user-query",
          sql: "SELECT 1",
          variables: [],
        },
      },
    );
    const createBody = await createResponse.text();
    expect([200, 201], createBody).toContain(createResponse.status());
    const savedSqlId = (JSON.parse(createBody) as { id: string }).id;

    try {
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`), {
        waitUntil: "domcontentloaded",
      });
      const toolbar = page.getByRole("group", { name: "SQL" });
      await toolbar.getByRole("button", { name: "Form", exact: true })
        .click();
      const dialog = page.getByRole("dialog", { name: "Form" });
      await expect(dialog.getByRole("tree")).toHaveCount(0);
      await dialog.getByRole("button", { name: "Entry", exact: true }).click();
      await expect(page.locator(".ui-sql-editor")).toContainText(
        'SELECT 1 FROM "Entry"',
      );
      await expect(page.getByRole("dialog")).toHaveCount(0);
      await expect(
        page.getByRole("group", { name: "SQL", exact: true }).getByRole(
          "button",
          { name: "Form", exact: true },
        ),
      ).toBeFocused();

      await expect(page.locator(".ui-sql-editor")).toContainText(
        'SELECT 1 FROM "Entry"',
      );
      await expect(page.getByRole("dialog")).toHaveCount(0);
    } finally {
      await request.delete(
        getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
      );
    }
  });

  test("REQ-FE-063 @smoke: relation-free SELECT 1 keeps shared SQL actions reachable", async ({ page, request }) => {
    await ensureDefaultForm(request, spaceId);
    const name = `Relation-free actions ${Date.now()}`;
    const createResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/sql`),
      {
        data: {
          name,
          kind: "user-query",
          sql: "SELECT 1",
          variables: [],
        },
      },
    );
    const createBody = await createResponse.text();
    expect([200, 201], createBody).toContain(createResponse.status());
    const savedSqlId = (JSON.parse(createBody) as { id: string }).id;

    try {
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`), {
        waitUntil: "domcontentloaded",
      });
      const editor = page.getByRole("textbox", { name: "SQL", exact: true });
      const toolbar = page.getByRole("group", { name: "SQL", exact: true });
      const assertSqlUnchanged = async () => {
        await expect.poll(() =>
          editor.evaluate((element) => element.textContent)
        )
          .toBe("SELECT 1");
      };
      await assertSqlUnchanged();

      const openAndClose = async (action: string) => {
        const button = toolbar.getByRole("button", {
          name: action,
          exact: true,
        });
        await expect(button).toBeEnabled();
        await button.click();
        const dialog = page.getByRole("dialog", {
          name: action,
          exact: true,
        });
        await expect(dialog).toBeVisible();
        await dialog.locator("footer").getByRole("button", {
          name: "Close",
          exact: true,
        }).click();
        await expect(dialog).toHaveCount(0);
        await assertSqlUnchanged();
      };

      for (const action of ["Form", "Columns", "Filter", "Sort"]) {
        await openAndClose(action);
      }

      const structure = toolbar.getByRole("button", {
        name: "Structure",
        exact: true,
      });
      await expect(structure).toBeEnabled();
      await structure.click();
      const structureOptions = page.getByRole("group", {
        name: "Structure options",
        exact: true,
      });
      const sqlTreeAction = structureOptions.getByRole("button", {
        name: "SQL tree",
        exact: true,
      });
      await expect(sqlTreeAction).toBeEnabled();
      await sqlTreeAction.click();
      const sqlTree = page.getByRole("dialog", {
        name: "SQL tree",
        exact: true,
      });
      await expect(sqlTree).toBeVisible();
      await sqlTree.locator("footer").getByRole("button", {
        name: "Close",
        exact: true,
      }).click();
      await expect(sqlTree).toHaveCount(0);
      await assertSqlUnchanged();

      await openAndClose("Arguments");
    } finally {
      const deleteResponse = await request.delete(
        getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
      );
      expect(
        deleteResponse.ok(),
        `Failed to delete relation-free SQL fixture (${deleteResponse.status()}): ${await deleteResponse
          .text()}`,
      ).toBe(true);
    }
  });

  test("REQ-FE-063 @smoke: empty SQL keeps shared structure actions reachable", async ({ page, request }) => {
    await ensureDefaultForm(request, spaceId);
    await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/new`), {
      waitUntil: "domcontentloaded",
    });

    const editor = page.getByRole("textbox", { name: "SQL", exact: true });
    await expect(editor).toHaveText("SELECT 1");
    await editor.click();
    await editor.press("ControlOrMeta+A");
    await page.keyboard.press("Backspace");
    await expect(editor).toHaveText("");

    const toolbar = page.getByRole("group", { name: "SQL", exact: true });
    await toolbar.getByRole("button", { name: "Form", exact: true }).click();
    const formDialog = page.getByRole("dialog", { name: "Form", exact: true });
    await formDialog.getByRole("button", { name: "Entry", exact: true })
      .click();
    await expect(editor).toHaveText('SELECT * FROM "Entry"');

    await toolbar.getByRole("button", { name: "Columns", exact: true })
      .click();
    const columns = page.getByRole("dialog", {
      name: "Columns",
      exact: true,
    });
    await columns.getByRole("button", { name: "Choose fields", exact: true })
      .click();
    const fields = columns.getByRole("list", { name: "Result fields" });
    await expect(fields.getByText("Body", { exact: true })).toBeVisible();
    const fieldNames = await fields.locator(".sql-columns-editor__name")
      .allTextContents();
    expect(fieldNames).toContain("Body");
    for (const name of fieldNames.filter((name) => name !== "Body")) {
      const remove = columns.getByRole("button", {
        name: `Remove ${name}`,
        exact: true,
      });
      await remove.click();
      await expect(remove).toHaveCount(0);
    }
    await expect(fields.getByRole("listitem")).toHaveText([/Body/]);
    await expect(editor).toContainText('FROM "Entry"');
    await expect(editor).not.toContainText("*");
    const selectedProjection = (await editor.innerText()).match(
      /^SELECT\s+([\s\S]+?)\s+FROM\s+"Entry"/,
    )?.[1];
    expect(selectedProjection).toBeTruthy();
    await columns.locator("footer").getByRole("button", { name: "Close" })
      .click();

    await toolbar.getByRole("button", { name: "Filter", exact: true })
      .click();
    const filter = page.getByRole("dialog", { name: "Filter", exact: true });
    await filter.getByRole("combobox", {
      name: "Column for new filter",
    }).selectOption({ label: "Body" });
    await filter.getByRole("combobox", { name: "Operator" }).selectOption(
      "eq",
    );
    await filter.getByRole("combobox", { name: "Value source" }).selectOption(
      "argument",
    );
    await filter.getByRole("textbox", { name: "New argument name" }).fill(
      "body_value",
    );
    await filter.getByRole("textbox", { name: "Default value (optional)" })
      .fill("initial body");
    await filter.getByRole("button", { name: "Add filter", exact: true })
      .click();
    await expect(editor).toContainText('FROM "Entry"');
    await expect(editor).not.toContainText("*");
    await expect(editor).toContainText(selectedProjection!);
    await expect(editor).toContainText("WHERE");
    await expect(editor).toContainText("$body_value");
    await filter.locator("footer").getByRole("button", { name: "Close" })
      .click();

    await toolbar.getByRole("button", { name: "Sort", exact: true }).click();
    const sort = page.getByRole("dialog", { name: "Sort", exact: true });
    await sort.getByRole("combobox", { name: "Column" }).selectOption({
      label: "Body",
    });
    await sort.getByRole("combobox", { name: "Direction" }).selectOption(
      "desc",
    );
    await sort.getByRole("button", { name: "Add sort", exact: true }).click();
    await expect(editor).toContainText('FROM "Entry"');
    await expect(editor).not.toContainText("*");
    await expect(editor).toContainText(selectedProjection!);
    await expect(editor).toContainText("$body_value");
    await expect(editor).toContainText("WHERE");
    await expect(editor).toContainText("ORDER BY");
    await expect(editor).toContainText("DESC");
    await sort.locator("footer").getByRole("button", { name: "Close" })
      .click();

    await toolbar.getByRole("button", { name: "Structure", exact: true })
      .click();
    const structureOptions = page.getByRole("group", {
      name: "Structure options",
    });
    await structureOptions.getByRole("button", {
      name: "SQL tree",
      exact: true,
    }).click();
    const tree = page.getByRole("dialog", { name: "SQL tree", exact: true });
    await tree.getByText("Query", { exact: true }).click();
    const limit = tree.getByLabel("Limit", { exact: true });
    await expect(limit).toBeVisible();
    await limit.fill("1");
    await tree.getByRole("button", { name: "Set row limit", exact: true })
      .click();
    await tree.getByRole("button", { name: "Apply", exact: true }).click();
    await expect(editor).toContainText('FROM "Entry"');
    await expect(editor).not.toContainText("*");
    await expect(editor).toContainText(selectedProjection!);
    await expect(editor).toContainText("$body_value");
    await expect(editor).toContainText("WHERE");
    await expect(editor).toContainText("ORDER BY");
    await expect(editor).toContainText("DESC");
    await expect(editor).toContainText(/LIMIT\s+1/);
    await tree.locator("footer").getByRole("button", { name: "Close" })
      .click();

    const argumentsAction = toolbar.getByRole("button", {
      name: "Arguments",
      exact: true,
    });
    await argumentsAction.click();
    const argumentsDialog = page.getByRole("dialog", {
      name: "Arguments",
      exact: true,
    });
    await expect(argumentsDialog.getByRole("listitem")).toHaveCount(1);
    await expect(argumentsDialog.getByRole("combobox", {
      name: "Type for body_value",
      exact: true,
    })).toHaveValue("string");
    const argumentDefault = argumentsDialog.getByRole("textbox", {
      name: "Default for body_value",
      exact: true,
    });
    await expect(argumentDefault).toHaveValue("initial body");
    await argumentDefault.fill("updated body");
    await expect(argumentDefault).toHaveValue("updated body");
    await expect(editor).toContainText('FROM "Entry"');
    await expect(editor).not.toContainText("*");
    await expect(editor).toContainText(selectedProjection!);
    await expect(editor).toContainText("$body_value");
    await expect(editor).toContainText("WHERE");
    await expect(editor).toContainText("ORDER BY");
    await expect(editor).toContainText("DESC");
    await expect(editor).toContainText(/LIMIT\s+1/);
    await argumentsDialog.locator("footer").getByRole("button", {
      name: "Close",
    }).click();
  });

  test("REQ-FE-064 @smoke: a Form query exposes every shared SQL action", async ({ page, request }) => {
    await ensureDefaultForm(request, spaceId);
    const relation = await getDefaultFormRelation(request, spaceId);
    const name = `Form SQL Workspace ${Date.now()}`;
    const createResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/sql`),
      {
        data: {
          name,
          kind: "user-query",
          sql: `SELECT * FROM "${relation}"`,
          variables: [],
        },
      },
    );
    const createBody = await createResponse.text();
    expect([200, 201], createBody).toContain(createResponse.status());
    const savedSqlId = (JSON.parse(createBody) as { id: string }).id;

    try {
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`), {
        waitUntil: "domcontentloaded",
      });
      await expect(page.locator(".ui-sql-editor")).toContainText(
        `SELECT * FROM "${relation}"`,
      );

      for (
        const action of [
          "Form",
          "Columns",
          "Filter",
          "Sort",
          "Structure",
          "Arguments",
        ]
      ) {
        await expect(page.getByRole("button", { name: action })).toBeEnabled();
      }
    } finally {
      await request.delete(
        getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
      );
    }
  });

  test("@smoke keeps SQL actions in a horizontally scrollable row on narrow screens", async ({ page }) => {
    await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/new`), {
      waitUntil: "domcontentloaded",
    });
    const actions = page.getByRole("group", { name: "SQL" });
    await expect(actions).toBeVisible();

    for (const width of [320, 375, 390]) {
      await page.setViewportSize({ width, height: 844 });
      const dimensions = await page.evaluate(() => {
        const toolbar = document.querySelector<HTMLElement>(
          ".sql-workspace__actions",
        );
        if (!toolbar) throw new Error("SQL action toolbar is missing");
        return {
          documentWidth: document.documentElement.scrollWidth,
          viewportWidth: document.documentElement.clientWidth,
          toolbarWidth: toolbar.clientWidth,
          toolbarScrollWidth: toolbar.scrollWidth,
          buttons: Array.from(toolbar.querySelectorAll("button"), (button) => {
            const rect = button.getBoundingClientRect();
            return { width: rect.width, height: rect.height, top: rect.top };
          }),
        };
      });

      expect(dimensions.documentWidth).toBeLessThanOrEqual(
        dimensions.viewportWidth + 1,
      );
      expect(dimensions.buttons).toHaveLength(6);
      expect(
        Math.max(...dimensions.buttons.map((button) => button.top)) -
          Math.min(...dimensions.buttons.map((button) => button.top)),
      ).toBeLessThanOrEqual(1);
      if (width === 320) {
        expect(dimensions.toolbarScrollWidth).toBeGreaterThan(
          dimensions.toolbarWidth,
        );
      } else {
        expect(dimensions.toolbarScrollWidth).toBeLessThanOrEqual(
          dimensions.toolbarWidth + 1,
        );
      }
      for (const button of dimensions.buttons) {
        expect(button.width).toBeGreaterThanOrEqual(44);
        expect(button.height).toBeGreaterThanOrEqual(44);
      }
    }
  });

  test("keeps deep boolean filters usable at 320px", async ({ page, request }) => {
    await ensureDefaultForm(request, spaceId);
    const relation = await getDefaultFormRelation(request, spaceId);
    const predicate = '"_ugoite_id" = $entry_id';
    let expression = predicate;
    for (let depth = 0; depth < 32; depth += 1) {
      const operator = depth % 2 === 0 ? "AND" : "OR";
      expression = `(${predicate} ${operator} (${expression}))`;
    }
    const createResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/sql`),
      {
        data: {
          name: `Deep Boolean Filter ${Date.now()}`,
          kind: "user-query",
          sql: `SELECT "_ugoite_id" FROM "${relation}" WHERE ${expression}`,
          variables: [{
            type: "string",
            name: "entry_id",
            description: "Select matching rows.",
            default: "no-match",
          }],
        },
      },
    );
    const createBody = await createResponse.text();
    expect([200, 201], createBody).toContain(createResponse.status());
    const savedSqlId = (JSON.parse(createBody) as { id: string }).id;

    try {
      await page.setViewportSize({ width: 320, height: 844 });
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`), {
        waitUntil: "domcontentloaded",
      });
      const filterAction = page.getByRole("group", { name: "SQL" })
        .getByRole("button", { name: "Filter", exact: true });
      await expect(filterAction).toBeEnabled();
      await filterAction.click();

      const filterDialog = page.getByRole("dialog", { name: "Filter" });
      await filterDialog.getByRole("button", {
        name: /Advanced structure/,
      }).click();
      const dialog = page.getByRole("dialog", { name: "SQL tree" });
      const leaves = dialog.getByRole("treeitem", {
        name: /Binary expression: "_ugoite_id" = \$entry_id$/,
      });
      await expect(leaves).toHaveCount(33);
      const deepest = leaves.last();
      await deepest.scrollIntoViewIfNeeded();
      await expect(deepest).toBeInViewport();
      await deepest.click();
      const operator = dialog.getByRole("combobox", {
        name: "Operator",
        exact: true,
      });
      await expect(operator).toHaveValue("eq");
      await operator.selectOption("not_eq");
      const apply = dialog.getByRole("button", { name: "Apply", exact: true });
      await expect(apply).toBeEnabled();
      const dimensions = await dialog.evaluate((element) => {
        const selected = element.querySelector<HTMLElement>(
          '[role="treeitem"][aria-selected="true"]',
        )!;
        const rect = selected.getBoundingClientRect();
        return {
          documentWidth: document.documentElement.scrollWidth,
          viewportWidth: document.documentElement.clientWidth,
          dialogWidth: element.clientWidth,
          dialogScrollWidth: element.scrollWidth,
          targetWidth: rect.width,
          targetHeight: rect.height,
        };
      });
      expect(dimensions.documentWidth).toBeLessThanOrEqual(
        dimensions.viewportWidth + 1,
      );
      expect(dimensions.dialogScrollWidth).toBeLessThanOrEqual(
        dimensions.dialogWidth + 1,
      );
      expect(dimensions.targetWidth).toBeGreaterThanOrEqual(44);
      expect(dimensions.targetHeight).toBeGreaterThanOrEqual(44);
      await apply.click();
      await expect(page.locator(".ui-sql-editor")).toContainText(
        '"_ugoite_id" <> $entry_id',
      );
      await dialog.getByRole("button", { name: "Close", exact: true }).last()
        .click();
      await expect(page.locator(".ui-sql-editor")).toContainText(
        '"_ugoite_id" <> $entry_id',
      );
      const saved = page.waitForResponse((response) =>
        response.request().method() === "PUT" &&
        new URL(response.url()).pathname ===
          `/api/spaces/${spaceId}/sql/${savedSqlId}`
      );
      await page.getByRole("button", { name: "Save", exact: true }).click();
      expect((await saved).ok()).toBe(true);
      const stored = await request.get(
        getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
      );
      expect(stored.ok()).toBe(true);
      const editedSql = (await stored.json() as { sql: string }).sql;
      expect(editedSql).toContain('"_ugoite_id" <> $entry_id');

      const queryResponse = await request.post(
        getBackendUrl(`/spaces/${spaceId}/sql/query`),
        {
          data: {
            sql: editedSql,
            parameters: { entry_id: "no-match" },
            parameter_types: { entry_id: "string" },
            limit: 50,
          },
        },
      );
      expect(queryResponse.ok(), await queryResponse.text()).toBe(true);
    } finally {
      await request.delete(
        getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
      );
    }
  });

  test("REQ-FE-063 @smoke: shared SQL Workspace round-trips a Form query through save and run", async ({ page, request }) => {
    const timestamp = Date.now();
    const queryName = `Form-bound draft ${timestamp}`;
    const alternateFormName = `SQL_Workspace_Alternate_${timestamp}`;
    let savedSqlId: string | undefined;
    await ensureDefaultForm(request, spaceId);
    const formResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/forms`),
      {
        data: {
          name: alternateFormName,
          version: 1,
          template: `# ${alternateFormName}\n\n## Body\n`,
          fields: { Body: { type: "markdown", required: false } },
        },
      },
    );
    expect([200, 201], await formResponse.text()).toContain(
      formResponse.status(),
    );
    const entryResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      {
        data: {
          form: alternateFormName,
          fields: { Body: `SQL journey ${timestamp}` },
        },
      },
    );
    expect(entryResponse.status(), await entryResponse.text()).toBe(201);
    const entryId = (await entryResponse.json() as { id: string }).id;
    const open = async (name: string) => {
      await page.getByRole("group", { name: "SQL", exact: true }).getByRole(
        "button",
        { name, exact: true },
      ).click();
      return page.getByRole("dialog", { name, exact: true });
    };
    const applyAndClose = async (dialog: ReturnType<typeof page.getByRole>) => {
      const previousSql = await page.locator(".ui-sql-editor").textContent();
      await dialog.getByRole("button", { name: "Apply", exact: true }).click();
      await expect(page.locator(".ui-sql-editor")).not.toHaveText(previousSql!);
      await dialog.getByRole("button", { name: "Close", exact: true }).last()
        .click();
    };
    try {
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/new`), {
        waitUntil: "domcontentloaded",
      });
      await expect(page.locator(".ui-sql-editor")).toContainText("SELECT 1");
      const formDialog = await open("Form");
      await expect(formDialog.getByRole("tree")).toHaveCount(0);
      await formDialog.getByRole("button", { name: "Entry", exact: true })
        .click();
      await expect(page.locator(".ui-sql-editor")).toContainText(
        'SELECT 1 FROM "Entry"',
      );
      await expect(page.getByRole("dialog")).toHaveCount(0);
      await expect(
        page.getByRole("group", { name: "SQL", exact: true }).getByRole(
          "button",
          { name: "Form", exact: true },
        ),
      ).toBeFocused();
      await expect(page.locator(".ui-sql-editor")).toContainText(
        'SELECT 1 FROM "Entry"',
      );
      for (
        const action of [
          "Form",
          "Columns",
          "Filter",
          "Sort",
          "Structure",
          "Arguments",
        ]
      ) {
        await expect(
          page.getByRole("group", { name: "SQL", exact: true }).getByRole(
            "button",
            { name: action, exact: true },
          ),
        ).toBeEnabled();
      }
      const argumentsDialog = await open("Arguments");
      await argumentsDialog.getByLabel("Argument name", { exact: true }).fill(
        "entry_id",
      );
      await argumentsDialog.getByRole("button", {
        name: "Add argument",
        exact: true,
      }).click();
      await argumentsDialog.getByRole("checkbox", {
        name: "Default for entry_id",
        exact: true,
      }).check();
      await argumentsDialog.getByRole("textbox", {
        name: "Default for entry_id",
        exact: true,
      }).fill(entryId);
      await expect(
        argumentsDialog.getByText("Argument is not referenced in SQL."),
      ).toBeVisible();
      await argumentsDialog.getByRole("button", { name: "Close", exact: true })
        .last().click();
      const editor = page.getByRole("textbox", { name: "SQL", exact: true });
      await editor.fill(
        `SELECT COUNT(*) FROM "Entry" WHERE "_ugoite_id" <> $entry_id GROUP BY "_ugoite_id" ORDER BY "_ugoite_id" DESC LIMIT 9`,
      );
      const taskFilterDialog = await open("Filter");
      const filterRegion = taskFilterDialog.getByRole("region", {
        name: "Filters",
        exact: true,
      });
      await filterRegion.getByRole("button", {
        name: "Add condition",
        exact: true,
      }).click();
      await filterRegion.getByRole("combobox", {
        name: "Column for new filter",
        exact: true,
      }).selectOption("_ugoite_id");
      await taskFilterDialog.getByRole("combobox", {
        name: "Operator for new filter",
        exact: true,
      }).selectOption("eq");
      await taskFilterDialog.getByRole("textbox", {
        name: "Value for new filter",
        exact: true,
      }).fill(entryId);
      await taskFilterDialog.getByRole("button", {
        name: "Add filter",
        exact: true,
      }).click();
      await expect(page.locator(".ui-sql-editor")).toContainText(entryId);
      await taskFilterDialog.getByRole("button", {
        name: "Close",
        exact: true,
      }).last().click();
      await page.getByRole("group", { name: "SQL", exact: true })
        .getByRole("button", { name: "Structure", exact: true }).click();
      await page.getByRole("button", { name: "SQL tree", exact: true })
        .click();
      const filterDialog = page.getByRole("dialog", { name: "SQL tree" });
      await filterDialog.getByRole("treeitem", {
        name:
          /^Binary expression Binary expression: "_ugoite_id" <> \$entry_id$/,
      }).click();
      const operator = filterDialog.getByRole("combobox", {
        name: "Operator",
        exact: true,
      });
      await expect(operator).toHaveValue("not_eq");
      await operator.selectOption("eq");
      await applyAndClose(filterDialog);
      await expect(page.locator(".ui-sql-editor")).toContainText(
        '"_ugoite_id" = $entry_id',
      );
      const sortDialog = await open("Sort");
      const firstSort = sortDialog.getByRole("group", { name: "Sort 1" });
      await firstSort.getByRole("combobox", { name: "Direction", exact: true })
        .selectOption("asc");
      await firstSort.getByRole("button", {
        name: "Apply sort changes",
        exact: true,
      }).click();
      await expect(page.locator(".ui-sql-editor")).toContainText(
        'ORDER BY "_ugoite_id" ASC',
      );
      await sortDialog.locator("footer").getByRole("button", {
        name: "Close",
        exact: true,
      }).click();
      await expect(page.locator(".ui-sql-editor")).toContainText(
        'ORDER BY "_ugoite_id" ASC',
      );
      const replacementDialog = await open("Form");
      await expect(replacementDialog.getByRole("tree")).toHaveCount(0);
      await replacementDialog.getByRole("button", {
        name: alternateFormName,
        exact: true,
      }).click();
      await expect(page.locator(".ui-sql-editor")).toContainText(
        `FROM "${alternateFormName}"`,
      );
      await expect(page.getByRole("dialog")).toHaveCount(0);
      await expect(
        page.getByRole("group", { name: "SQL", exact: true }).getByRole(
          "button",
          { name: "Form", exact: true },
        ),
      ).toBeFocused();
      await expect(page.locator(".ui-sql-editor")).toContainText(
        `FROM "${alternateFormName}"`,
      );
      await expect(page.locator(".ui-sql-editor")).toContainText(
        "SELECT COUNT(*)",
      );
      await expect(page.locator(".ui-sql-editor")).toContainText(
        'WHERE "_ugoite_id" = $entry_id',
      );
      await expect(page.locator(".ui-sql-editor")).toContainText(
        'GROUP BY "_ugoite_id"',
      );
      await expect(page.locator(".ui-sql-editor")).toContainText(
        'ORDER BY "_ugoite_id" ASC',
      );
      await expect(page.locator(".ui-sql-editor")).toContainText("LIMIT 9");
      const retainedArguments = await open("Arguments");
      await expect(
        retainedArguments.getByRole("textbox", {
          name: "Default for entry_id",
          exact: true,
        }),
      ).toHaveValue(entryId);
      await retainedArguments.getByRole("button", {
        name: "Close",
        exact: true,
      }).last().click();
      await page.getByLabel("Query name").fill(queryName);
      const saveButton = page.getByRole("button", {
        name: "Save",
        exact: true,
      });
      await expect(saveButton).toBeEnabled();
      const created = page.waitForResponse((response) =>
        response.request().method() === "POST" &&
        new URL(response.url()).pathname === `/api/spaces/${spaceId}/sql`
      );
      await saveButton.click();
      const response = await created;
      expect([200, 201], await response.text()).toContain(response.status());
      savedSqlId = (await response.json() as { id: string }).id;
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`), {
        waitUntil: "domcontentloaded",
      });
      await expect(page.locator(".ui-sql-editor")).toContainText(
        `FROM "${alternateFormName}"`,
      );
      await expect(page.locator(".ui-sql-editor")).toContainText(
        'GROUP BY "_ugoite_id"',
      );
      await page.getByRole("textbox", { name: "entry_id", exact: true }).fill(
        entryId,
      );
      const ran = page.waitForResponse((response) =>
        response.request().method() === "POST" &&
        new URL(response.url()).pathname === `/api/spaces/${spaceId}/sql/query`
      );
      await page.getByRole("button", { name: "Run Query", exact: true })
        .click();
      const result = await ran;
      expect(result.ok(), await result.text()).toBe(true);
      const output = await result.json() as {
        columns: string[];
        rows: Record<string, unknown>[];
      };
      expect(output.columns).toHaveLength(1);
      expect(output.rows).toHaveLength(1);
      expect(Number(output.rows[0][output.columns[0]])).toBe(1);
      await expect(
        page.getByRole("region", { name: /^(SQL results|SQL 結果)$/ }).first(),
      ).toBeVisible();
      await expect(page.locator('p[role="alert"]')).toHaveCount(0);
    } finally {
      if (savedSqlId) {
        await request.delete(
          getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
        );
      }
      await request.delete(
        getBackendUrl(`/spaces/${spaceId}/entries/${entryId}`),
      );
    }
  });

  test("REQ-FE-062 @smoke: complex authorized SQL (Form-bound) keeps shared SQL actions reachable", async ({ page, request }) => {
    const timestamp = Date.now();
    const queryName = `Complex Form query ${timestamp}`;
    const formName = `SQL_Workspace_Alternate_${timestamp}`;
    const sql =
      `SELECT COUNT(*) FROM "${formName}" WHERE "_ugoite_id" = $entry_id GROUP BY "_ugoite_id" ORDER BY "_ugoite_id" ASC LIMIT 9`;
    let savedSqlId: string | undefined;
    let entryId: string | undefined;
    await ensureDefaultForm(request, spaceId);
    try {
      const formResponse = await request.post(
        getBackendUrl(`/spaces/${spaceId}/forms`),
        {
          data: {
            name: formName,
            version: 1,
            template: `# ${formName}\n\n## Body\n`,
            fields: { Body: { type: "markdown", required: false } },
          },
        },
      );
      expect([200, 201], await formResponse.text()).toContain(
        formResponse.status(),
      );
      const formsResponse = await request.get(
        getBackendUrl(`/spaces/${spaceId}/forms`),
      );
      expect(formsResponse.ok(), await formsResponse.text()).toBe(true);
      const forms = await formsResponse.json() as Array<{
        id: string;
        name: string;
      }>;
      const form = forms.find((candidate) => candidate.name === formName);
      expect(form?.id).toBeTruthy();

      const entryResponse = await request.post(
        getBackendUrl(`/spaces/${spaceId}/entries`),
        {
          data: {
            form: formName,
            fields: { Body: `SQL journey ${timestamp}` },
          },
        },
      );
      expect(entryResponse.status(), await entryResponse.text()).toBe(201);
      entryId = (await entryResponse.json() as { id: string }).id;

      const createResponse = await request.post(
        getBackendUrl(`/spaces/${spaceId}/sql`),
        {
          data: {
            name: queryName,
            kind: "user-query",
            metadata: { formBindings: [{ name: formName, formId: form!.id }] },
            sql,
            variables: [{
              type: "string",
              name: "entry_id",
              description: "",
              default: entryId,
            }],
          },
        },
      );
      const createBody = await createResponse.text();
      expect([200, 201], createBody).toContain(createResponse.status());
      savedSqlId = (JSON.parse(createBody) as { id: string }).id;

      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`), {
        waitUntil: "domcontentloaded",
      });
      const editor = page.getByRole("textbox", { name: "SQL", exact: true });
      const rawSql = () => editor.evaluate((element) => element.textContent);
      await expect.poll(rawSql).toBe(sql);

      const parameters = { entry_id: entryId };
      const parameterTypes = { entry_id: "string" };
      const formBindingGuards = [{ name: formName, form_id: form!.id }];
      const planResponse = await request.post(
        getBackendUrl(`/spaces/${spaceId}/sql/query/plan`),
        {
          data: {
            sql,
            parameters,
            parameter_types: parameterTypes,
            form_binding_guards: formBindingGuards,
          },
        },
      );
      expect(planResponse.ok(), await planResponse.text()).toBe(true);

      const queryResponse = await request.post(
        getBackendUrl(`/spaces/${spaceId}/sql/query`),
        {
          data: {
            sql,
            parameters,
            parameter_types: parameterTypes,
            form_binding_guards: formBindingGuards,
            limit: 50,
          },
        },
      );
      expect(queryResponse.ok(), await queryResponse.text()).toBe(true);
      const output = await queryResponse.json() as {
        columns: string[];
        rows: Record<string, unknown>[];
      };
      expect(output.columns).toHaveLength(1);
      expect(output.rows).toHaveLength(1);
      expect(Number(output.rows[0][output.columns[0]])).toBe(1);

      const toolbar = page.getByRole("group", { name: "SQL", exact: true });
      const assertSqlUnchanged = async () => {
        await expect.poll(rawSql).toBe(sql);
      };
      const openAndClose = async (action: string) => {
        const button = toolbar.getByRole("button", {
          name: action,
          exact: true,
        });
        await expect(button).toBeEnabled();
        await button.click();
        const dialog = page.getByRole("dialog", {
          name: action,
          exact: true,
        });
        await expect(dialog).toBeVisible();
        await dialog.locator("footer").getByRole("button", {
          name: "Close",
          exact: true,
        }).click();
        await expect(dialog).toHaveCount(0);
        await assertSqlUnchanged();
      };

      for (const action of ["Form", "Columns", "Filter", "Sort"]) {
        await openAndClose(action);
      }

      const structure = toolbar.getByRole("button", {
        name: "Structure",
        exact: true,
      });
      await expect(structure).toBeEnabled();
      await structure.click();
      const structureOptions = page.getByRole("group", {
        name: "Structure options",
        exact: true,
      });
      await structureOptions.getByRole("button", {
        name: "SQL tree",
        exact: true,
      }).click();
      const sqlTree = page.getByRole("dialog", {
        name: "SQL tree",
        exact: true,
      });
      await expect(sqlTree).toBeVisible();
      await sqlTree.locator("footer").getByRole("button", {
        name: "Close",
        exact: true,
      }).click();
      await expect(sqlTree).toHaveCount(0);
      await assertSqlUnchanged();

      await openAndClose("Arguments");
    } finally {
      const cleanupResults = await Promise.all([
        ...(savedSqlId
          ? [{
            resource: "Saved SQL",
            path: `/spaces/${spaceId}/sql/${savedSqlId}`,
            response: request.delete(
              getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
            ),
          }]
          : []),
        ...(entryId
          ? [{
            resource: "Entry",
            path: `/spaces/${spaceId}/entries/${entryId}`,
            response: request.delete(
              getBackendUrl(`/spaces/${spaceId}/entries/${entryId}`),
            ),
          }]
          : []),
      ]);
      for (const cleanup of cleanupResults) {
        const response = await cleanup.response;
        const body = await response.text();
        expect(
          response.ok(),
          `DELETE ${cleanup.path} (${cleanup.resource}) returned HTTP ${response.status()}: ${body}`,
        ).toBe(true);
      }
    }
  });
});
