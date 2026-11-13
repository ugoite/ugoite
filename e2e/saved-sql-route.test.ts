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

  test("A07 @smoke: SQL text editor runs an aliased ARRAY query", async ({ page }) => {
    await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/new`), {
      waitUntil: "domcontentloaded",
    });
    const editor = page.getByRole("textbox", { name: "SQL", exact: true });
    const sql = "SELECT ARRAY[] AS empty LIMIT 1";
    await editor.fill(sql);
    await expect(editor).toHaveText(sql);
    const toolbar = page.getByRole("group", { name: "SQL", exact: true });
    for (
      const action of ["Columns", "Filter", "Sort", "Structure", "Arguments"]
    ) {
      await expect(toolbar.getByRole("button", { name: action, exact: true }))
        .toHaveCount(0);
    }
    await expect(page.getByRole("button", { name: "Run Query", exact: true }))
      .toBeEnabled();
    const ran = page.waitForResponse((response) =>
      response.request().method() === "POST" &&
      new URL(response.url()).pathname === `/api/spaces/${spaceId}/sql/query`
    );
    await page.getByRole("button", { name: "Run Query", exact: true }).click();
    const result = await ran;
    expect(result.ok(), await result.text()).toBe(true);
    const output = await result.json() as {
      columns: string[];
      rows: Record<string, unknown>[];
    };
    expect(output.columns).toEqual(["empty"]);
    expect(output.rows).toEqual([{ empty: [] }]);
  });
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

  test("@smoke SQL text editor runs an SQL-authored ARRAY literal", async ({ page }) => {
    await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/new`), {
      waitUntil: "domcontentloaded",
    });
    const editor = page.getByRole("textbox", { name: "SQL", exact: true });
    const sql = "SELECT ARRAY[4, 9] AS values";
    await editor.fill(sql);
    await expect(editor).toHaveText(sql);
    const ran = page.waitForResponse((response) =>
      response.request().method() === "POST" &&
      new URL(response.url()).pathname === `/api/spaces/${spaceId}/sql/query`
    );
    await page.getByRole("button", { name: "Run Query", exact: true }).click();
    const result = await ran;
    expect(result.ok(), await result.text()).toBe(true);
    const output = await result.json() as {
      columns: string[];
      rows: Record<string, unknown>[];
    };
    expect(output.columns).toEqual(["values"]);
    expect(output.rows).toEqual([{ values: [4, 9] }]);
  });
  test("@smoke SQL text edits, runs, saves, and reloads an EXISTS query", async ({ page }) => {
    const editedSql = "SELECT 1 WHERE EXISTS (SELECT 1 WHERE FALSE)";
    const queryName = `Exists text edit round trip ${Date.now()}`;
    let savedSqlId: string | undefined;
    try {
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/new`), {
        waitUntil: "domcontentloaded",
      });
      const editor = page.getByRole("textbox", { name: "SQL", exact: true });
      await editor.fill(editedSql);
      await expect(editor).toHaveText(editedSql);
      const toolbar = page.getByRole("group", { name: "SQL", exact: true });
      for (
        const action of ["Columns", "Filter", "Sort", "Structure", "Arguments"]
      ) {
        await expect(toolbar.getByRole("button", { name: action, exact: true }))
          .toHaveCount(0);
      }
      const firstRun = page.waitForResponse((response) =>
        response.request().method() === "POST" &&
        new URL(response.url()).pathname === `/api/spaces/${spaceId}/sql/query`
      );
      await page.getByRole("button", { name: "Run Query", exact: true })
        .click();
      const result = await firstRun;
      expect(result.ok(), await result.text()).toBe(true);
      const output = await result.json() as {
        columns: string[];
        rows: Record<string, unknown>[];
      };
      expect(output.columns).toHaveLength(1);
      expect(output.rows).toEqual([]);

      await page.getByLabel("Query name", { exact: true }).fill(queryName);
      const createdResponse = page.waitForResponse((response) =>
        response.request().method() === "POST" &&
        new URL(response.url()).pathname === `/api/spaces/${spaceId}/sql`
      );
      await page.getByRole("button", { name: "Save", exact: true }).click();
      const created = await createdResponse;
      expect([200, 201], await created.text()).toContain(created.status());
      savedSqlId = (await created.json() as { id: string }).id;
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`), {
        waitUntil: "domcontentloaded",
      });
      await expect(page.getByRole("heading", { level: 1, name: queryName }))
        .toBeVisible();
      await expect(page.getByRole("textbox", { name: "SQL", exact: true }))
        .toHaveText(editedSql);
    } finally {
      if (savedSqlId) {
        await page.request.delete(
          getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
        );
      }
    }
  });
  test("@smoke SQL text edits, runs, saves, and reloads a scalar subquery", async ({ page }) => {
    const editedSql = "SELECT (SELECT 2) AS value";
    const queryName = `Scalar subquery text edit ${Date.now()}`;
    let savedSqlId: string | undefined;
    try {
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/new`), {
        waitUntil: "domcontentloaded",
      });
      const editor = page.getByRole("textbox", { name: "SQL", exact: true });
      await editor.fill(editedSql);
      await expect(editor).toHaveText(editedSql);
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
      expect(output.rows).toEqual([{ value: 2 }]);

      await page.getByLabel("Query name", { exact: true }).fill(queryName);
      const createdResponse = page.waitForResponse((response) =>
        response.request().method() === "POST" &&
        new URL(response.url()).pathname === `/api/spaces/${spaceId}/sql`
      );
      await page.getByRole("button", { name: "Save", exact: true }).click();
      const created = await createdResponse;
      expect([200, 201], await created.text()).toContain(created.status());
      savedSqlId = (await created.json() as { id: string }).id;
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`), {
        waitUntil: "domcontentloaded",
      });
      await expect(page.getByRole("textbox", { name: "SQL", exact: true }))
        .toHaveText(editedSql);
      const reloadedRun = page.waitForResponse((response) =>
        response.request().method() === "POST" &&
        new URL(response.url()).pathname === `/api/spaces/${spaceId}/sql/query`
      );
      await page.getByRole("button", { name: "Run Query", exact: true })
        .click();
      const reloaded = await reloadedRun;
      expect(reloaded.ok(), await reloaded.text()).toBe(true);
    } finally {
      if (savedSqlId) {
        await page.request.delete(
          getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
        );
      }
    }
  });
  test("@smoke SQL text reports unsupported IN and NOT IN plans", async ({ page }) => {
    for (
      const sql of ["SELECT (1) IN (SELECT 1)", "SELECT (1) NOT IN (SELECT 1)"]
    ) {
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/new`), {
        waitUntil: "domcontentloaded",
      });
      const editor = page.getByRole("textbox", { name: "SQL", exact: true });
      await expect(editor).toHaveText("SELECT 1", { timeout: 30_000 });
      await expect(page.getByRole("button", { name: "Run Query", exact: true }))
        .toBeEnabled({ timeout: 30_000 });
      const rejectedPlan = page.waitForResponse((response) =>
        response.request().method() === "POST" &&
        new URL(response.url()).pathname ===
          `/api/spaces/${spaceId}/sql/query/plan` &&
        response.request().postDataJSON().sql === sql
      );
      await editor.fill(sql);
      await expect(editor).toHaveText(sql);
      const response = await rejectedPlan;
      expect(response.status()).toBe(422);
      await expect(page.getByRole("button", { name: "Run Query", exact: true }))
        .toBeDisabled();
      await expect(page.getByRole("button", { name: "Save", exact: true }))
        .toBeDisabled();
    }
  });
  test("REQ-FE-063 @smoke: scalar SQL stays directly editable without construction actions", async ({ page, request }) => {
    const name = `Scalar SQL ${Date.now()}`;
    const created = await request.post(
      getBackendUrl(`/spaces/${spaceId}/sql`),
      {
        data: { name, kind: "user-query", sql: "SELECT 1", variables: [] },
      },
    );
    const body = await created.json() as { id: string };
    expect([200, 201], JSON.stringify(body)).toContain(created.status());
    try {
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${body.id}`), {
        waitUntil: "domcontentloaded",
      });
      await expect(page.getByRole("textbox", { name: "SQL", exact: true }))
        .toHaveText("SELECT 1");
      const toolbar = page.getByRole("group", { name: "SQL", exact: true });
      for (
        const action of ["Columns", "Filter", "Sort", "Structure", "Arguments"]
      ) {
        await expect(toolbar.getByRole("button", { name: action, exact: true }))
          .toHaveCount(0);
      }
      await expect(toolbar.getByRole("button", { name: "Form", exact: true }))
        .toHaveCount(0);
    } finally {
      await request.delete(getBackendUrl(`/spaces/${spaceId}/sql/${body.id}`));
    }
  });
  test("Form selection seeds explicit projectable fields into a blank SQL draft", async ({ page, request }) => {
    await ensureDefaultForm(request, spaceId);
    await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/new`), {
      waitUntil: "domcontentloaded",
    });
    const toolbar = page.getByRole("group", { name: "SQL", exact: true });
    await toolbar.getByRole("button", { name: "Form", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "Form" });
    await dialog.getByRole("button", { name: "Entry", exact: true }).click();
    const editor = page.getByRole("textbox", { name: "SQL", exact: true });
    const sql = await editor.evaluate((element) => element.textContent ?? "");
    expect(sql).toContain('FROM "Entry"');
    const projection = sql.match(/^SELECT\s+([\s\S]+?)\s+FROM\s+"Entry"/)
      ?.[1];
    expect(projection).toBeTruthy();
    expect(projection).not.toBe("*");
    expect(projection).not.toMatch(/_ugoite_/);
    for (
      const action of ["Columns", "Filter", "Sort", "Structure", "Arguments"]
    ) {
      await expect(toolbar.getByRole("button", { name: action, exact: true }))
        .toHaveCount(0);
    }
  });
  test("REQ-FE-063 @smoke: relation-free Saved SQL stays text-first", async ({ page, request }) => {
    const name = `Relation-free SQL ${Date.now()}`;
    const created = await request.post(
      getBackendUrl(`/spaces/${spaceId}/sql`),
      {
        data: { name, kind: "user-query", sql: "SELECT 1", variables: [] },
      },
    );
    const body = await created.json() as { id: string };
    expect([200, 201], JSON.stringify(body)).toContain(created.status());
    try {
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${body.id}`), {
        waitUntil: "domcontentloaded",
      });
      await expect(page.getByRole("textbox", { name: "SQL", exact: true }))
        .toHaveText("SELECT 1");
      const toolbar = page.getByRole("group", { name: "SQL", exact: true });
      await expect(toolbar.getByRole("button", { name: "Form", exact: true }))
        .toHaveCount(0);
      for (
        const action of ["Columns", "Filter", "Sort", "Structure", "Arguments"]
      ) {
        await expect(toolbar.getByRole("button", { name: action, exact: true }))
          .toHaveCount(0);
      }
    } finally {
      await request.delete(getBackendUrl(`/spaces/${spaceId}/sql/${body.id}`));
    }
  });
  test("REQ-FE-063 @smoke: new SQL keeps a Form seed beside the text editor", async ({ page, request }) => {
    await ensureDefaultForm(request, spaceId);
    await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/new`), {
      waitUntil: "domcontentloaded",
    });
    const editor = page.getByRole("textbox", { name: "SQL", exact: true });
    await expect(editor).toHaveText("SELECT 1");
    const toolbar = page.getByRole("group", { name: "SQL", exact: true });
    await expect(toolbar.getByRole("button", { name: "Form", exact: true }))
      .toBeVisible();
    for (
      const action of ["Columns", "Filter", "Sort", "Structure", "Arguments"]
    ) {
      await expect(toolbar.getByRole("button", { name: action, exact: true }))
        .toHaveCount(0);
    }
    await editor.fill("");
    await expect(toolbar.getByRole("button", { name: "Form", exact: true }))
      .toBeDisabled();
    await editor.fill("SELECT 1");
    await toolbar.getByRole("button", { name: "Form", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "Form" });
    await dialog.getByRole("button", { name: "Entry", exact: true }).click();
    const sql = await editor.evaluate((element) => element.textContent ?? "");
    expect(sql).toContain('FROM "Entry"');
    const projection = sql.match(/^SELECT\s+([\s\S]+?)\s+FROM\s+"Entry"/)?.[1];
    expect(projection).toBeTruthy();
    expect(projection).not.toBe("*");
    expect(projection).not.toMatch(/_ugoite_/);
  });
  test("@smoke a Form query remains direct SQL without syntax construction actions", async ({ page, request }) => {
    await ensureDefaultForm(request, spaceId);
    const relation = await getDefaultFormRelation(request, spaceId);
    const name = `Form SQL Workspace ${Date.now()}`;
    const created = await request.post(
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
    const body = await created.json() as { id: string };
    expect([200, 201], JSON.stringify(body)).toContain(created.status());
    try {
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${body.id}`), {
        waitUntil: "domcontentloaded",
      });
      await expect(page.getByRole("textbox", { name: "SQL", exact: true }))
        .toHaveText(`SELECT * FROM "${relation}"`);
      const toolbar = page.getByRole("group", { name: "SQL", exact: true });
      await expect(toolbar.getByRole("button", { name: "Form", exact: true }))
        .toHaveCount(0);
      for (
        const action of ["Columns", "Filter", "Sort", "Structure", "Arguments"]
      ) {
        await expect(toolbar.getByRole("button", { name: action, exact: true }))
          .toHaveCount(0);
      }
    } finally {
      await request.delete(getBackendUrl(`/spaces/${spaceId}/sql/${body.id}`));
    }
  });
  test("@smoke keeps text editing and the Form starter usable at narrow widths", async ({ page, request }) => {
    await ensureDefaultForm(request, spaceId);
    await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/new`), {
      waitUntil: "domcontentloaded",
    });
    const editor = page.getByRole("textbox", { name: "SQL", exact: true });
    const form = page.getByRole("group", { name: "SQL", exact: true })
      .getByRole("button", { name: "Form", exact: true });
    await expect(editor).toBeVisible();
    for (const width of [320, 375, 390]) {
      await page.setViewportSize({ width, height: 844 });
      const dimensions = await page.evaluate(() => {
        const editor = document.querySelector<HTMLElement>(".ui-sql-editor");
        const form = Array.from(document.querySelectorAll("button")).find(
          (button) =>
            button.getAttribute("aria-label") === "Form" ||
            button.textContent?.trim() === "Form",
        );
        if (!editor || !form) {
          throw new Error("SQL editor or Form starter is missing");
        }
        const rect = form.getBoundingClientRect();
        return {
          documentWidth: document.documentElement.scrollWidth,
          viewportWidth: document.documentElement.clientWidth,
          editorFontSize: Number.parseFloat(getComputedStyle(editor).fontSize),
          formWidth: rect.width,
          formHeight: rect.height,
        };
      });
      expect(dimensions.documentWidth).toBeLessThanOrEqual(
        dimensions.viewportWidth + 1,
      );
      expect(dimensions.editorFontSize).toBeGreaterThanOrEqual(16);
      expect(dimensions.formWidth).toBeGreaterThanOrEqual(44);
      expect(dimensions.formHeight).toBeGreaterThanOrEqual(44);
      await expect(form).toBeVisible();
    }
  });
  test("keeps long SQL text editable at 320px without structure controls", async ({ page }) => {
    await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/new`), {
      waitUntil: "domcontentloaded",
    });
    await page.setViewportSize({ width: 320, height: 844 });
    const editor = page.getByRole("textbox", { name: "SQL", exact: true });
    const sql = `SELECT 1 WHERE ${
      Array.from({ length: 24 }, (_, index) => `(${index + 1} = ${index + 1})`)
        .join(" AND ")
    }`;
    await editor.fill(sql);
    await expect(editor).toHaveText(sql);
    const dimensions = await page.evaluate(() => ({
      documentWidth: document.documentElement.scrollWidth,
      viewportWidth: document.documentElement.clientWidth,
      editorFontSize: Number.parseFloat(
        getComputedStyle(document.querySelector<HTMLElement>(".ui-sql-editor")!)
          .fontSize,
      ),
    }));
    expect(dimensions.documentWidth).toBeLessThanOrEqual(
      dimensions.viewportWidth + 1,
    );
    expect(dimensions.editorFontSize).toBeGreaterThanOrEqual(16);
    const toolbar = page.getByRole("group", { name: "SQL", exact: true });
    for (
      const action of ["Columns", "Filter", "Sort", "Structure", "Arguments"]
    ) {
      await expect(toolbar.getByRole("button", { name: action, exact: true }))
        .toHaveCount(0);
    }
  });
  test("REQ-FE-063 @smoke: Form-seeded SQL saves and runs from the text editor", async ({ page, request }) => {
    const timestamp = Date.now();
    const formName = `SQL_Seed_${timestamp}`;
    const queryName = `Form seed round trip ${timestamp}`;
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
      const entryResponse = await request.post(
        getBackendUrl(`/spaces/${spaceId}/entries`),
        {
          data: { form: formName, fields: { Body: `SQL seed ${timestamp}` } },
        },
      );
      expect(entryResponse.status(), await entryResponse.text()).toBe(201);
      entryId = (await entryResponse.json() as { id: string }).id;

      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/new`), {
        waitUntil: "domcontentloaded",
      });
      const editor = page.getByRole("textbox", { name: "SQL", exact: true });
      await expect(editor).toHaveText("SELECT 1");
      await page.getByRole("group", { name: "SQL", exact: true })
        .getByRole("button", { name: "Form", exact: true }).click();
      const dialog = page.getByRole("dialog", { name: "Form" });
      await dialog.getByRole("button", { name: formName, exact: true }).click();
      const sql = await editor.evaluate((element) => element.textContent ?? "");
      expect(sql).toContain(`FROM "${formName}"`);
      const projection = sql.match(
        new RegExp(`^SELECT\\s+([\\s\\S]+?)\\s+FROM\\s+"${formName}"`),
      )?.[1];
      expect(projection).toBeTruthy();
      expect(projection).not.toBe("*");
      expect(projection).not.toMatch(/_ugoite_/);
      await page.getByLabel("Query name", { exact: true }).fill(queryName);
      await expect(page.getByRole("button", { name: "Save", exact: true }))
        .toBeEnabled();
      const createdResponse = page.waitForResponse((response) =>
        response.request().method() === "POST" &&
        new URL(response.url()).pathname === `/api/spaces/${spaceId}/sql`
      );
      await page.getByRole("button", { name: "Save", exact: true }).click();
      const created = await createdResponse;
      expect([200, 201], await created.text()).toContain(created.status());
      savedSqlId = (await created.json() as { id: string }).id;
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`), {
        waitUntil: "domcontentloaded",
      });
      await expect(page.getByRole("textbox", { name: "SQL", exact: true }))
        .toHaveText(sql);
      const ran = page.waitForResponse((response) =>
        response.request().method() === "POST" &&
        new URL(response.url()).pathname === `/api/spaces/${spaceId}/sql/query`
      );
      await page.getByRole("button", { name: "Run Query", exact: true })
        .click();
      const result = await ran;
      expect(result.ok(), await result.text()).toBe(true);
      const output = await result.json() as { rows: unknown[][] };
      expect(output.rows).toHaveLength(1);
    } finally {
      if (savedSqlId) {
        await request.delete(
          getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
        );
      }
      if (entryId) {
        await request.delete(
          getBackendUrl(`/spaces/${spaceId}/entries/${entryId}`),
        );
      }
      await request.delete(
        getBackendUrl(
          `/spaces/${spaceId}/forms/${encodeURIComponent(formName)}`,
        ),
      );
    }
  });
  test("REQ-FE-062 @smoke: complex authorized SQL stays text-first and runs with Form guards", async ({ page, request }) => {
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
      const forms = await formsResponse.json() as Array<
        { id: string; name: string }
      >;
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
      await expect(editor).toHaveText(sql);
      const toolbar = page.getByRole("group", { name: "SQL", exact: true });
      for (
        const action of ["Columns", "Filter", "Sort", "Structure", "Arguments"]
      ) {
        await expect(toolbar.getByRole("button", { name: action, exact: true }))
          .toHaveCount(0);
      }
      await expect(page.getByRole("link", { name: "Open Variables" }))
        .toHaveAttribute(
          "href",
          new RegExp(`/spaces/${spaceId}/sql/${savedSqlId}/variables$`),
        );

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
    } finally {
      if (savedSqlId) {
        await request.delete(
          getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
        );
      }
      if (entryId) {
        await request.delete(
          getBackendUrl(`/spaces/${spaceId}/entries/${entryId}`),
        );
      }
      await request.delete(
        getBackendUrl(
          `/spaces/${spaceId}/forms/${encodeURIComponent(formName)}`,
        ),
      );
    }
  });
});
