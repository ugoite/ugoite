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
        `SELECT 1 FROM "${relation}"`,
      );
      await dialog.getByRole("button", { name: "Close", exact: true }).last()
        .click();

      await expect(page.locator(".ui-sql-editor")).toContainText(
        `SELECT 1 FROM "${relation}"`,
      );
      await expect(page.getByRole("dialog")).toHaveCount(0);
    } finally {
      await request.delete(
        getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
      );
    }
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

      const dialog = page.getByRole("dialog", { name: "Filter" });
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
    const relation = await getDefaultFormRelation(request, spaceId);
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
    const formsResponse = await request.get(
      getBackendUrl(`/spaces/${spaceId}/forms`),
    );
    const forms = await formsResponse.json() as Array<
      { name: string; sql_relation: string }
    >;
    const alternateRelation =
      forms.find((form) => form.name === alternateFormName)!.sql_relation;
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
        `SELECT 1 FROM "${relation}"`,
      );
      await formDialog.getByRole("button", { name: "Close", exact: true })
        .last().click();
      await expect(page.locator(".ui-sql-editor")).toContainText(
        `SELECT 1 FROM "${relation}"`,
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
      await expect(
        argumentsDialog.getByText("Argument is not referenced in SQL."),
      ).toBeVisible();
      await argumentsDialog.getByRole("button", { name: "Close", exact: true })
        .last().click();
      const editor = page.getByRole("textbox", { name: "SQL", exact: true });
      await editor.fill(
        `SELECT COUNT(*) FROM "${relation}" WHERE "_ugoite_id" <> $entry_id GROUP BY "_ugoite_id" ORDER BY "_ugoite_id" DESC LIMIT 9`,
      );
      const filterDialog = await open("Filter");
      await filterDialog.getByRole("treeitem", {
        name: /Binary expression: "_ugoite_id" <> \$entry_id/,
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
      await sortDialog.getByRole("treeitem", {
        name: /Order key:.*_ugoite_id/,
      }).click();
      await sortDialog.getByRole("combobox", { name: "Direction", exact: true })
        .selectOption("asc");
      await applyAndClose(sortDialog);
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
        `FROM "${alternateRelation}"`,
      );
      await replacementDialog.getByRole("button", {
        name: "Close",
        exact: true,
      }).last().click();
      await expect(page.locator(".ui-sql-editor")).toContainText(
        `FROM "${alternateRelation}"`,
      );
      await expect(page.locator(".ui-sql-editor")).toContainText(
        'GROUP BY "_ugoite_id"',
      );
      await expect(page.locator(".ui-sql-editor")).toContainText("LIMIT 9");
      await page.getByLabel("Query name").fill(queryName);
      const created = page.waitForResponse((response) =>
        response.request().method() === "POST" &&
        new URL(response.url()).pathname === `/api/spaces/${spaceId}/sql`
      );
      await page.getByRole("button", { name: "Save", exact: true }).click();
      const response = await created;
      expect([200, 201], await response.text()).toContain(response.status());
      savedSqlId = (await response.json() as { id: string }).id;
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`), {
        waitUntil: "domcontentloaded",
      });
      await expect(page.locator(".ui-sql-editor")).toContainText(
        `FROM "${alternateRelation}"`,
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
});
