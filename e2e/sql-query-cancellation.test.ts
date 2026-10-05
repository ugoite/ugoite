import { expect, test } from "@playwright/test";
import {
  ensureDefaultForm,
  getBackendUrl,
  getDefaultFormRelation,
  getDefaultSpaceId,
  getFrontendUrl,
  waitForServers,
} from "./lib/client.ts";

/**
 * Intentionally slow read without large fixtures: ten tiny seeded rows
 * self-cross-joined eight ways (10^8 counted rows). It is one SELECT
 * statement over the Space's own default Form relation, so it passes
 * read-only validation and authorized execution; the count streams, so
 * the query stays in flight for seconds instead of failing on limits.
 */
const SEEDED_ROW_COUNT = 10;
const CROSS_JOIN_COUNT = 8;

function slowCountSql(relation: string): string {
  const quoted = `"${relation.replaceAll('"', '""')}"`;
  const sources = Array.from(
    { length: CROSS_JOIN_COUNT },
    (_, index) => `${quoted} AS slow_${index}`,
  ).join(" CROSS JOIN ");
  return `SELECT count(*) AS slow_count FROM ${sources}`;
}

test.describe("browser-to-server cancellation for reads", () => {
  let spaceId = "";

  test.beforeAll(async ({ request }) => {
    await waitForServers(request);
    spaceId = await getDefaultSpaceId(request);
  });

  test("aborting a slow SQL run ends the browser request and writes no stale rows", async ({ page, request }) => {
    await ensureDefaultForm(request, spaceId);
    const relation = await getDefaultFormRelation(request, spaceId);
    const seededEntryIds: string[] = [];
    for (let index = 0; index < SEEDED_ROW_COUNT; index += 1) {
      const created = await request.post(
        getBackendUrl(`/spaces/${spaceId}/entries`),
        {
          data: {
            form: "Entry",
            fields: { Body: `Slow cancellation seed ${index}` },
          },
        },
      );
      expect(created.status()).toBe(201);
      seededEntryIds.push(
        ((await created.json()) as { id: string }).id,
      );
    }
    const queryName = `Slow Cancellation Probe ${Date.now()}`;
    const createResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/sql`),
      {
        data: {
          name: queryName,
          kind: "user-query",
          sql: slowCountSql(relation),
          variables: [],
        },
      },
    );
    expect([200, 201]).toContain(createResponse.status());
    const savedSql = (await createResponse.json()) as { id: string };

    try {
      await page.goto(
        getFrontendUrl(`/spaces/${spaceId}/sql/${savedSql.id}`),
        { waitUntil: "domcontentloaded" },
      );
      await expect(
        page.getByRole("heading", { level: 1, name: queryName }),
      ).toBeVisible();

      // Stable request identity: the single POST the run route issues for
      // this saved query.
      const queryRequestPromise = page.waitForRequest((candidate) => {
        const url = new URL(candidate.url());
        return candidate.method() === "POST" &&
          url.pathname === `/api/spaces/${spaceId}/sql/query`;
      });
      await page.getByRole("button", { name: "Run Query" }).click();
      const queryRequest = await queryRequestPromise;
      await expect(page).toHaveURL(
        new RegExp(`/spaces/${spaceId}/sql/${savedSql.id}/run`),
      );

      // Representative lifecycle event: route disposal. The browser fetch
      // for the disposed run must end instead of lingering.
      const abortedPromise = page
        .waitForEvent("requestfailed", {
          predicate: (candidate) => candidate === queryRequest,
          timeout: 30_000,
        })
        .then(() => "aborted" as const);
      const finishedPromise = page
        .waitForEvent("requestfinished", {
          predicate: (candidate) => candidate === queryRequest,
          timeout: 30_000,
        })
        .then(() => "finished" as const);
      await page.goto(getFrontendUrl(`/spaces/${spaceId}/dashboard`), {
        waitUntil: "domcontentloaded",
      });
      const outcome = await Promise.race([
        abortedPromise.catch(() => "unobserved" as const),
        finishedPromise.catch(() => "unobserved" as const),
      ]);
      // The 10^8-row cross join cannot complete before disposal
      // navigation lands, so "finished" would mean the slow source was not
      // slow (a test bug), not that cancellation is unnecessary.
      expect(outcome, "disposed run ends its in-flight query request").toBe(
        "aborted",
      );
      expect(queryRequest.url()).toContain(`/api/spaces/${spaceId}/sql/query`);
      expect(queryRequest.failure()?.errorText).toMatch(
        /aborted|cancelled|canceled|failed/i,
      );

      // Discarding the result is a separate observation from stopping
      // execution: give a late server response its window, then confirm the
      // disposed run wrote no stale rows into the browser view.
      await page.waitForTimeout(5_000);
      await expect(page.locator("table.result-table--sql")).toHaveCount(0);
      await expect(page.getByText("slow_count")).toHaveCount(0);

      // The server keeps serving new reads after the abort: active work did
      // not wedge the Space. (Direct observation of the DataFusion stream
      // drop and the active-work gauge is covered at the Rust layer by
      // read_query_cancellation_tests; no public API exposes them here.)
      const fastQuery = await request.post(
        getBackendUrl(`/spaces/${spaceId}/sql/query`),
        {
          data: { sql: "SELECT 1 AS alive", limit: 10 },
        },
      );
      expect(fastQuery.status()).toBe(200);
      const fastPage = (await fastQuery.json()) as {
        columns?: string[];
        rows?: unknown[];
      };
      expect(fastPage.columns).toContain("alive");
      expect(fastPage.rows?.[0]).toEqual([1]);
    } finally {
      await request.delete(
        getBackendUrl(`/spaces/${spaceId}/sql/${savedSql.id}`),
      );
      for (const entryId of seededEntryIds) {
        await request.delete(
          getBackendUrl(`/spaces/${spaceId}/entries/${entryId}`),
        );
      }
    }
  });
});
