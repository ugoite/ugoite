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

  test("REQ-FE-061: saved SQL route provides recovery links instead of a dead end", async ({ page }) => {
    await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql`), {
      waitUntil: "domcontentloaded",
    });

    await expect(page.getByRole("heading", { level: 1, name: "Saved SQL" }))
      .toBeVisible();
    await expect(page.getByText(/named-query management/i)).toBeVisible();
    await expect(page.getByRole("link", { name: "Open Search" }))
      .toHaveAttribute(
        "href",
        `/spaces/${spaceId}/search`,
      );
    await expect(page.getByRole("link", { name: "Back to Dashboard" }))
      .toHaveAttribute(
        "href",
        `/spaces/${spaceId}/dashboard`,
      );
    await expect(page.getByRole("link", { name: "Browse Entries" }))
      .toHaveAttribute(
        "href",
        `/spaces/${spaceId}/entries`,
      );
    await expect(page.getByText("Saved SQL management is not yet in the UI."))
      .toHaveCount(0);
  });

	test("REQ-FE-062: saved SQL detail loads query text and routes variable-free runs to entries", async ({
		page,
		request,
	}) => {
		await ensureDefaultForm(request, spaceId);
		const relation = await getDefaultFormRelation(request, spaceId);
		const sqlCreate = await request.post(getBackendUrl(`/spaces/${spaceId}/sql`), {
			data: {
				name: `Saved Detail Query ${Date.now()}`,
				kind: "user-query",
				sql: `SELECT * FROM "${relation}" ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 1`,
				variables: [],
			},
		});
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
        new RegExp(`/spaces/${spaceId}/entries\\?session=`),
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
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/queries/new`), {
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
        new RegExp(`/spaces/${spaceId}/entries\\?session=`),
      );
    } finally {
      if (savedSqlId) {
        await request.delete(
          getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
        );
      }
    }
  });
});
