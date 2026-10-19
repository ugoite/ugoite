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
      expect(dimensions.buttons).toHaveLength(8);
      expect(
        Math.max(...dimensions.buttons.map((button) => button.top)) -
          Math.min(...dimensions.buttons.map((button) => button.top)),
      ).toBeLessThanOrEqual(1);
      expect(dimensions.toolbarScrollWidth).toBeGreaterThan(
        dimensions.toolbarWidth,
      );
      for (const button of dimensions.buttons) {
        expect(button.width).toBeGreaterThanOrEqual(44);
        expect(button.height).toBeGreaterThanOrEqual(44);
      }

      const scrolled = await page.evaluate(() => {
        const toolbar = document.querySelector<HTMLElement>(
          ".sql-workspace__actions",
        );
        const lastButton = toolbar?.querySelector<HTMLButtonElement>(
          "button:last-of-type",
        );
        if (!toolbar || !lastButton) {
          throw new Error("SQL action toolbar is missing its final action");
        }
        toolbar.scrollLeft = 0;
        const initialScrollLeft = toolbar.scrollLeft;
        toolbar.scrollLeft = toolbar.scrollWidth;
        const toolbarRect = toolbar.getBoundingClientRect();
        const lastButtonRect = lastButton.getBoundingClientRect();
        return {
          documentWidth: document.documentElement.scrollWidth,
          viewportWidth: document.documentElement.clientWidth,
          initialScrollLeft,
          scrollLeft: toolbar.scrollLeft,
          finalActionVisible: lastButtonRect.left >= toolbarRect.left - 1 &&
            lastButtonRect.right <= toolbarRect.right + 1,
        };
      });

      expect(scrolled.scrollLeft).toBeGreaterThan(scrolled.initialScrollLeft);
      expect(scrolled.finalActionVisible).toBe(true);
      expect(scrolled.documentWidth).toBeLessThanOrEqual(
        scrolled.viewportWidth + 1,
      );
    }
  });

  test("REQ-FE-063 @smoke: new saved SQL uses the shared structure actions and runs through Saved SQL", async ({ page, request }) => {
    const queryName = `Form-bound draft ${Date.now()}`;
    let savedSqlId: string | undefined;
    await ensureDefaultForm(request, spaceId);
    const relation = await getDefaultFormRelation(request, spaceId);

    try {
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/new`), {
        waitUntil: "domcontentloaded",
      });

      await expect(page.locator(".ui-sql-editor")).toContainText("SELECT 1");
      await expect(page.getByLabel("Form for this query")).toHaveCount(0);
      await expect(
        page.getByRole("button", { name: /^Aggregate(?:$|:)/ }),
      ).toHaveCount(1);
      await expect(page.getByLabel("Filter field")).toHaveCount(0);

      const structureActions = page.getByRole("group", { name: "SQL" });
      const formAction = structureActions.getByRole("button", {
        name: "Form",
        exact: true,
      });
      await expect(formAction).toBeEnabled();
      await expect(
        structureActions.getByRole("button", {
          name: /^Aggregate(?:$|:)/,
        }),
      ).toBeDisabled();
      await expect(page.locator(".ui-sql-editor")).not.toContainText(
        /\b(?:LIMIT|OFFSET)\b/i,
      );
      await expect(page.getByLabel("Filter field")).toHaveCount(0);

      await formAction.click();
      const formDialog = page.getByRole("dialog", { name: "Form" });
      await formDialog.getByRole("button", { name: "Entry" }).click();
      const reviewDialog = page.getByRole("dialog", {
        name: "Review SQL change",
      });
      await expect(reviewDialog.getByText(`SELECT * FROM "${relation}"`))
        .toBeVisible();
      await reviewDialog.getByRole("button", { name: "Apply change" }).click();
      await expect(page.locator(".ui-sql-editor")).toContainText(
        `SELECT * FROM "${relation}"`,
      );

      const editor = page.getByRole("textbox", { name: "SQL", exact: true });
      await editor.fill(
        `SELECT "_ugoite_id" FROM "${relation}" AS entry_rows ORDER BY "_ugoite_id"`,
      );
      await expect(page.locator(".ui-sql-editor")).toContainText(
        `FROM "${relation}" AS entry_rows`,
      );

      const filter = structureActions.getByRole("button", {
        name: "Filter",
        exact: true,
      });
      await expect(filter).toBeEnabled();
      await filter.click();
      const filterDialog = page.getByRole("dialog", { name: "Filter" });
      await filterDialog.getByRole("button", { name: "Add filter" }).click();
      await filterDialog.getByRole("button", { name: "Review change" }).click();
      const filterReviewDialog = page.getByRole("dialog", {
        name: "Review SQL change",
      });
      await expect(filterReviewDialog.getByText(/\$filter__ugoite_id/))
        .toBeVisible();
      await filterReviewDialog.getByRole("button", { name: "Apply change" })
        .click();
      const filterValue = page.getByRole("textbox", {
        name: "filter__ugoite_id",
        exact: true,
      });
      await expect(filterValue).toBeVisible();
      await filterValue.fill("no-match");

      const groupBy = structureActions.getByRole("button", {
        name: "Group by",
        exact: true,
      });
      await expect(groupBy).toBeEnabled();
      await groupBy.click();
      const groupByDialog = page.getByRole("dialog", { name: "Group by" });
      await groupByDialog.getByLabel("_ugoite_id", { exact: true }).check();
      await groupByDialog.getByRole("button", { name: "Review change" })
        .click();
      const groupByReviewDialog = page.getByRole("dialog", {
        name: "Review SQL change",
      });
      await expect(
        groupByReviewDialog.getByText(/GROUP BY "_ugoite_id"/),
      ).toBeVisible();
      await groupByReviewDialog.getByRole("button", { name: "Apply change" })
        .click();

      const aggregate = structureActions.getByRole("button", {
        name: "Aggregate",
        exact: true,
      });
      await expect(aggregate).toBeEnabled();
      await aggregate.click();
      const aggregateDialog = page.getByRole("dialog", { name: "Aggregate" });
      await aggregateDialog.getByRole("button", { name: "Add aggregate" })
        .click();
      await aggregateDialog.getByRole("button", { name: "Review change" })
        .click();
      const aggregateReviewDialog = page.getByRole("dialog", {
        name: "Review SQL change",
      });
      await expect(aggregateReviewDialog.getByText(/COUNT\(\*\)/))
        .toBeVisible();
      await aggregateReviewDialog.getByRole("button", { name: "Apply change" })
        .click();
      await expect(page.locator(".ui-sql-editor")).toContainText(/COUNT\(\*\)/);
      await expect(page.locator(".ui-sql-editor")).toContainText(
        /GROUP BY "_ugoite_id"/,
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
      await expect(page.locator(".ui-sql-editor")).toContainText(
        `FROM "${relation}" AS entry_rows`,
      );
      await expect(page.locator(".ui-sql-editor")).toContainText(
        /GROUP BY "_ugoite_id"/,
      );
      await page.getByRole("textbox", {
        name: "filter__ugoite_id",
        exact: true,
      }).fill("no-match");
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
