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

  test("REQ-FE-062: saved SQL detail loads query text and routes variable-free runs to stateless results", async ({ page, request }) => {
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
      await page.getByRole("button", { name: "Run Query" }).click();
      await expect(page).toHaveURL(
        new RegExp(`/spaces/${spaceId}/sql/${savedSql.id}/run`),
      );
    } finally {
      await request.delete(
        getBackendUrl(`/spaces/${spaceId}/sql/${savedSql.id}`),
      );
    }
  });

  test("REQ-FE-063: new saved SQL starts with a runnable total-ordered query", async ({ page, request }) => {
    await ensureDefaultForm(request, spaceId);
    const queryName = `Runnable starter ${Date.now()}`;
    let savedSqlId: string | undefined;

    try {
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/new`), {
        waitUntil: "domcontentloaded",
      });

      await expect(page.locator(".ui-sql-editor")).toContainText(
        /SELECT \* FROM "[^"]+" ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 50/,
      );
      await page.getByLabel("Query name").fill(queryName);
      const createResponsePromise = page.waitForResponse((response) => {
        const url = new URL(response.url());
        return response.request().method() === "POST" &&
          url.pathname === `/api/spaces/${spaceId}/sql`;
      });
      await page.getByRole("button", { name: "Save" }).click();
      const createResponse = await createResponsePromise;
      expect([200, 201]).toContain(createResponse.status());
      savedSqlId = (await createResponse.json() as { id: string }).id;

      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`), {
        waitUntil: "domcontentloaded",
      });
      await page.getByRole("button", { name: "Run Query" }).click();
      await expect(page).toHaveURL(
        new RegExp(`/spaces/${spaceId}/sql/${savedSqlId}/run`),
      );
    } finally {
      if (savedSqlId) {
        await request.delete(
          getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
        );
      }
    }
  });

  test("REQ-FE-063 @smoke: aggregate builder saves a typed filter and runs through the ordinary Saved SQL route", async ({ page, request }) => {
    const suffix = Date.now();
    const formName = `Sales_${suffix}`;
    const entryIds: string[] = [];
    let savedSqlId: string | undefined;
    try {
      const createForm = await request.post(
        getBackendUrl(`/spaces/${spaceId}/forms`),
        {
          data: {
            name: formName,
            version: 1,
            template:
              `# ${formName}\n\n## Quantity\n\n## Revenue\n\n## Category\n`,
            fields: {
              Quantity: { type: "integer", required: false, label: "Quantity" },
              Revenue: { type: "integer", required: false, label: "Revenue" },
              Category: { type: "string", required: false, label: "Category" },
            },
          },
        },
      );
      expect([200, 201]).toContain(createForm.status());

      const formsResponse = await request.get(
        getBackendUrl(`/spaces/${spaceId}/forms`),
      );
      expect(formsResponse.ok()).toBe(true);
      const forms = await formsResponse.json() as Array<{
        id?: string;
        name?: string;
        fields?: Record<string, { sql_column?: string }>;
      }>;
      const form = forms.find((candidate) => candidate.name === formName);
      expect(form?.id).toBeTruthy();
      expect(form?.fields?.Quantity?.sql_column).toBeTruthy();
      expect(form?.fields?.Category?.sql_column).toBeTruthy();

      const createEntry = async (quantity: number, category: string) => {
        const response = await request.post(
          getBackendUrl(`/spaces/${spaceId}/entries`),
          {
            data: {
              form: formName,
              fields: { Quantity: quantity, Category: category },
            },
          },
        );
        expect(response.status()).toBe(201);
        return (await response.json() as { id: string }).id;
      };
      entryIds.push(await createEntry(42, "food"));
      entryIds.push(await createEntry(9, "travel"));

      const queryName = `Form aggregate ${suffix}`;
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/new`), {
        waitUntil: "domcontentloaded",
      });
      await page.getByLabel("Form for this query").selectOption(form!.id!);
      const aggregate = page.getByLabel("Aggregate");
      await aggregate.selectOption("sum");
      await expect(aggregate).toHaveValue("sum");
      const aggregateField = page.getByLabel("Field", { exact: true });
      await aggregateField.selectOption("Revenue");
      await expect(aggregateField).toHaveValue("Revenue");
      await aggregateField.press("ArrowUp");
      await expect(aggregateField).toHaveValue("Quantity");
      const filterField = page.getByLabel("Filter field");
      await filterField.selectOption("Category");
      await expect(filterField).toHaveValue("Category");
      await page.getByRole("button", { name: "Generate aggregate SQL" })
        .click();

      const amountColumn = form!.fields!.Quantity!.sql_column!;
      const categoryColumn = form!.fields!.Category!.sql_column!;
      await expect(page.locator(".ui-sql-editor")).toContainText(
        `SUM("${amountColumn}") AS "value" FROM "${formName}" WHERE "${categoryColumn}" = {{filter_value}}`,
      );
      await page.getByLabel("Query name").fill(queryName);
      const createResponsePromise = page.waitForResponse((response) => {
        const url = new URL(response.url());
        return response.request().method() === "POST" &&
          url.pathname === `/api/spaces/${spaceId}/sql`;
      });
      await page.getByRole("button", { name: "Save" }).click();
      const createQueryResponse = await createResponsePromise;
      expect([200, 201]).toContain(createQueryResponse.status());
      const createPayload = createQueryResponse.request().postDataJSON() as {
        kind: string;
        sql: string;
        variables: Array<{ name: string; type: string; description: string }>;
      };
      expect(createPayload.kind).toBe("user-query");
      expect(createPayload.sql).toBe(
        `SELECT SUM("${amountColumn}") AS "value" FROM "${formName}" WHERE "${categoryColumn}" = $filter_value`,
      );
      expect(createPayload.variables).toEqual([{
        name: "filter_value",
        type: "string",
        description: "Category",
      }]);
      savedSqlId = (await createQueryResponse.json() as { id: string }).id;

      await page.goto(
        getFrontendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
        { waitUntil: "domcontentloaded" },
      );
      await page.getByRole("link", { name: "Open Variables" }).click();
      await page.getByLabel("filter_value").fill("food");
      await page.getByRole("button", { name: "Run" }).click();
      await expect(page).toHaveURL(
        new RegExp(`/spaces/${spaceId}/sql/${savedSqlId}/run`),
      );
      await expect(page.getByText("42", { exact: true })).toBeVisible();
    } finally {
      if (savedSqlId) {
        await request.delete(
          getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
        );
      }
      for (const entryId of entryIds) {
        await request.delete(
          getBackendUrl(`/spaces/${spaceId}/entries/${entryId}`),
        );
      }
    }
  });
});
