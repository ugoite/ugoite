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
    expect([200, 201]).toContain(createResponse.status());
    const savedSqlId = (await createResponse.json() as { id: string }).id;

    try {
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`), {
        waitUntil: "domcontentloaded",
      });
      await expect(page.locator(".ui-sql-editor")).toContainText("SELECT 1");

      const distinctAction = page.getByRole("button", { name: "Distinct" });
      await expect(distinctAction).toBeEnabled();
      await distinctAction.click();
      const dialog = page.getByRole("dialog", { name: "Distinct" });
      await dialog.getByRole("checkbox").check();
      await dialog.getByRole("button", { name: "Review change" }).click();
      const reviewDialog = page.getByRole("dialog", {
        name: "Review SQL change",
      });
      await expect(reviewDialog.getByText("SELECT DISTINCT 1")).toBeVisible();
      await reviewDialog.getByRole("button", { name: "Apply change" }).click();

      await expect(page.locator(".ui-sql-editor")).toContainText(
        "SELECT DISTINCT 1",
      );
    } finally {
      await request.delete(
        getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
      );
    }
  });

  test("REQ-FE-063 @smoke: new saved SQL uses the shared structure actions and runs through Saved SQL", async ({ page, request }) => {
    const queryName = `Relation-free draft ${Date.now()}`;
    let savedSqlId: string | undefined;

    try {
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/new`), {
        waitUntil: "domcontentloaded",
      });

      await expect(page.locator(".ui-sql-editor")).toContainText("SELECT 1");
      await expect(page.getByLabel("Form for this query")).toHaveCount(0);
      await expect(
        page.getByRole("button", { name: "Aggregate", exact: true }),
      ).toHaveCount(1);
      await expect(page.getByLabel("Filter field")).toHaveCount(0);

      const structureActions = page.getByRole("group", { name: "SQL" });
      await expect(structureActions.getByRole("button", { name: /^Columns/ }))
        .toBeEnabled();
      await expect(structureActions.getByRole("button", { name: /^Filter/ }))
        .toBeEnabled();
      await expect(structureActions.getByRole("button", { name: /^Sort/ }))
        .toBeEnabled();
      await expect(
        structureActions.getByRole("button", { name: /^Aggregate/ }),
      ).toBeEnabled();
      await expect(page.locator(".ui-sql-editor")).not.toContainText(
        /\b(?:LIMIT|OFFSET)\b/i,
      );
      await expect(page.getByLabel("Filter field")).toHaveCount(0);

      const distinct = page.getByRole("button", { name: "Distinct" });
      await expect(distinct).toBeEnabled();
      await distinct.click();
      const dialog = page.getByRole("dialog", { name: "Distinct" });
      await dialog.getByRole("checkbox").check();
      await dialog.getByRole("button", { name: "Review change" }).click();
      await dialog.getByRole("button", { name: "Apply change" }).click();
      await expect(page.locator(".ui-sql-editor")).toContainText(
        "SELECT DISTINCT 1",
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
      const queryResponsePromise = page.waitForResponse((response) => {
        const url = new URL(response.url());
        return response.request().method() === "POST" &&
          url.pathname === `/api/spaces/${spaceId}/sql/query`;
      });
      await page.getByRole("button", { name: "Run Query" }).click();
      const queryResponse = await queryResponsePromise;
      expect(queryResponse.ok()).toBe(true);
      await expect(page).toHaveURL(
        new RegExp(`/spaces/${spaceId}/sql/${savedSqlId}$`),
      );
      await expect(
        page.getByRole("region", { name: /^(SQL results|SQL 結果)$/ }).first(),
      )
        .toBeVisible();
      await expect(page.locator('section[aria-busy="true"]')).toHaveCount(0);
      await expect(page.locator('p[role="alert"]')).toHaveCount(0);
    } finally {
      if (savedSqlId) {
        await request.delete(
          getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
        );
      }
    }
  });
});
