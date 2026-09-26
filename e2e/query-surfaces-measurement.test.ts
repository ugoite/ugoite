import { expect, test } from "@playwright/test";
import { getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client.ts";

type SpaceRecord = { space_uid: string; slug?: string; name?: string };
type FormRecord = {
  id?: string;
  name?: string;
  sql_relation?: string;
};
type QueryEvent = {
  path: string;
  method: string;
  startedAt: number;
  endedAt?: number;
  status?: number;
  aborted: boolean;
  error?: string;
  body?: unknown;
  entryIds?: string[];
};
type LifecycleMeasurement = {
  events?: QueryEvent[];
  supersededInFlightCount: number;
  actualAbortCount: number;
  endedAbortCount: number;
  residualPendingCount: number;
  visibleEntryIds: string[];
  staleSourceEntryIds: string[];
  unexpectedTargetEntryIds: string[];
  visibleDataRows: number;
  targetSpaceQueryCount: number;
  targetSpaceQueryPaths: string[];
  rowsBeforeTargetQuery: number;
  rowsWhileTargetQueryPending: number;
  instrumentOnly?: boolean;
  sqlPageChange?: QueryLifecycleMeasurement;
  sqlCountIdentityChange?: QueryLifecycleMeasurement;
};
type QueryLifecycleMeasurement = {
  supersededInFlightCount: number;
  actualAbortCount: number;
  endedAbortCount: number;
  residualPendingCount: number;
  events: QueryEvent[];
};

const MEASUREMENT_SLUGS = ["query-space-a", "query-space-b"] as const;
const TRIAL_COUNT = 5;
const SQL_FORM_NAME = "MaintenanceTicket";
const SQL_PAGE_SIZE = 100;

function percentile(values: number[], p: number): number {
  const sorted = [...values].sort((left, right) => left - right);
  return sorted[Math.ceil(p * sorted.length) - 1] ?? 0;
}

test("records real two-Space query surface measurements", async ({ page, request }) => {
  test.skip(
    Deno.env.get("UGOITE_QUERY_MEASURE_ENABLED") !== "true",
    "requires the fixed two-Space measurement dataset",
  );
  test.setTimeout(600_000);
  await waitForServers(request);
  for (const slug of MEASUREMENT_SLUGS) {
    const bindSeededSpace = await request.post(getBackendUrl("/spaces"), {
      data: { slug, name: `Query measurement ${slug}` },
    });
    expect([200, 201]).toContain(bindSeededSpace.status());
  }
  const spacesResponse = await request.get(getBackendUrl("/spaces"));
  expect(spacesResponse.ok()).toBeTruthy();
  const spaces = await spacesResponse.json() as SpaceRecord[];
  const measuredSpaces: Array<{
    slug: string;
    space_uid: string;
    saved_sql_id?: string;
  }> = MEASUREMENT_SLUGS.map((slug) => {
    const space = spaces.find((candidate) =>
      candidate.slug === slug || candidate.name === slug
    );
    expect(space, `seeded Space ${slug} is visible to the test account`)
      .toBeTruthy();
    return { slug, space_uid: space!.space_uid };
  });

  const sqlBySpace = new Map<string, string>();
  const formMetadata: Record<string, { id?: string; relation: string }> = {};
  for (const space of measuredSpaces) {
    const formsResponse = await request.get(
      getBackendUrl(`/spaces/${space.space_uid}/forms`),
    );
    expect(formsResponse.ok()).toBeTruthy();
    const forms = await formsResponse.json() as FormRecord[];
    const form = forms.find((candidate) => candidate.name === SQL_FORM_NAME);
    expect(form?.id, `${SQL_FORM_NAME} has an immutable id`).toBeTruthy();
    expect(form?.sql_relation, `${SQL_FORM_NAME} has a SQL relation`)
      .toBeTruthy();
    formMetadata[space.slug] = {
      id: form!.id,
      relation: form!.sql_relation!,
    };
    const sql = `SELECT _ugoite_id FROM "${
      form!.sql_relation
    }" ORDER BY _ugoite_id`;
    const createSqlResponse = await request.post(
      getBackendUrl(`/spaces/${space.space_uid}/sql`),
      {
        data: {
          name: `Query measurement ${space.slug}`,
          kind: "user-query",
          sql,
          variables: [],
        },
      },
    );
    expect([200, 201]).toContain(createSqlResponse.status());
    const savedSql = await createSqlResponse.json() as { id: string };
    sqlBySpace.set(space.space_uid, sql);
    space.saved_sql_id = savedSql.id;
  }

  await page.setViewportSize({ width: 1280, height: 720 });
  await page.addInitScript(() => {
    type MeasuredWindow = Window & { __ugoiteQueryEvents?: QueryEvent[] };
    const measured = window as MeasuredWindow;
    measured.__ugoiteQueryEvents = [];
    const nativeFetch = window.fetch.bind(window);
    window.fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = input instanceof Request ? input.url : String(input);
      const path = new URL(url, window.location.href).pathname;
      if (
        !path.includes("/entries/query") && !path.includes("/sql/query") &&
        !path.includes("/sql/count")
      ) {
        return nativeFetch(input, init);
      }
      const signal = init?.signal ??
        (input instanceof Request ? input.signal : undefined);
      const event: QueryEvent = {
        path,
        method: init?.method ??
          (input instanceof Request ? input.method : "GET"),
        startedAt: performance.now(),
        aborted: signal?.aborted ?? false,
        ...(typeof init?.body === "string"
          ? {
            body: (() => {
              try {
                return JSON.parse(init.body as string);
              } catch {
                return undefined;
              }
            })(),
          }
          : {}),
      };
      signal?.addEventListener("abort", () => {
        event.aborted = true;
      }, { once: true });
      measured.__ugoiteQueryEvents!.push(event);
      try {
        const response = await nativeFetch(input, init);
        event.endedAt = performance.now();
        event.status = response.status;
        if (path.includes("/entries/query") && response.ok) {
          const result = await response.clone().json() as {
            rows?: Array<{ id?: unknown }>;
            entries?: Array<{ id?: unknown }>;
          };
          const rows = result.rows ?? result.entries ?? [];
          event.entryIds = rows.flatMap((row) =>
            typeof row.id === "string" ? [row.id] : []
          );
        }
        return response;
      } catch (error) {
        event.endedAt = performance.now();
        event.error = error instanceof Error ? error.message : String(error);
        throw error;
      }
    };
  });

  const trials: Array<Record<string, unknown>> = [];
  const sqlPagination: Array<Record<string, unknown>> = [];
  const rowLocator = page.locator("tbody tr").first();

  try {
    for (const space of measuredSpaces) {
      const entryPath = getFrontendUrl(
        `/spaces/${space.space_uid}/forms/${SQL_FORM_NAME}/entries`,
      );
      for (let trial = 1; trial <= TRIAL_COUNT; trial += 1) {
        await page.evaluate(() => {
          (window as Window & { __ugoiteQueryEvents?: QueryEvent[] })
            .__ugoiteQueryEvents = [];
        }).catch(() => undefined);
        const startedAt = Date.now();
        await page.goto(entryPath, { waitUntil: "domcontentloaded" });
        await expect(rowLocator).toBeVisible();
        const elapsedToFirstVisibleRowMs = Date.now() - startedAt;
        const browserState = await page.evaluate(() => ({
          userAgent: navigator.userAgent,
          viewport: { width: innerWidth, height: innerHeight },
          usedHeapBytes: (performance as Performance & {
            memory?: { usedJSHeapSize?: number };
          }).memory?.usedJSHeapSize ?? null,
          dataRows: document.querySelectorAll(
            "tbody tr",
          ).length,
          queryEvents: (window as Window & {
            __ugoiteQueryEvents?: QueryEvent[];
          }).__ugoiteQueryEvents ?? [],
        }));
        expect(
          browserState.queryEvents.some((event) =>
            event.path.includes("/entries/query")
          ),
        ).toBeTruthy();
        trials.push({
          surface: "entry-query",
          space: space.slug,
          space_uid: space.space_uid,
          trial,
          elapsedToFirstVisibleRowMs,
          ...browserState,
        });
      }

      const sqlPath = getFrontendUrl(
        `/spaces/${space.space_uid}/sql/${space.saved_sql_id}/run`,
      );
      for (let trial = 1; trial <= TRIAL_COUNT; trial += 1) {
        await page.evaluate(() => {
          (window as Window & { __ugoiteQueryEvents?: QueryEvent[] })
            .__ugoiteQueryEvents = [];
        }).catch(() => undefined);
        const startedAt = Date.now();
        await page.goto(sqlPath, { waitUntil: "domcontentloaded" });
        await expect(rowLocator).toBeVisible();
        const elapsedToFirstVisibleRowMs = Date.now() - startedAt;
        const firstPageState = await page.evaluate(() => ({
          userAgent: navigator.userAgent,
          viewport: { width: innerWidth, height: innerHeight },
          usedHeapBytes: (performance as Performance & {
            memory?: { usedJSHeapSize?: number };
          }).memory?.usedJSHeapSize ?? null,
          dataRows: document.querySelectorAll(
            "tbody tr",
          ).length,
          queryEvents: (window as Window & {
            __ugoiteQueryEvents?: QueryEvent[];
          }).__ugoiteQueryEvents ?? [],
        }));
        expect(
          firstPageState.queryEvents.filter((event) =>
            event.path.includes("/sql/count")
          ),
        ).toHaveLength(0);
        const pageQuery = firstPageState.queryEvents.find((event) =>
          event.path.includes("/sql/query")
        );
        expect(pageQuery).toBeTruthy();
        expect((pageQuery?.body as { limit?: number } | undefined)?.limit)
          .toBe(SQL_PAGE_SIZE);
        trials.push({
          surface: "sql-page",
          space: space.slug,
          space_uid: space.space_uid,
          trial,
          elapsedToFirstVisibleRowMs,
          ...firstPageState,
        });
      }

      await page.goto(sqlPath, { waitUntil: "domcontentloaded" });
      await expect(rowLocator).toBeVisible();
      const initialPageCount = await page.evaluate(() =>
        (window as Window & { __ugoiteQueryEvents?: QueryEvent[] })
          .__ugoiteQueryEvents?.filter((event) =>
            event.path.includes("/sql/query")
          ).length ?? 0
      );
      const initialCountRequests = await page.evaluate(() =>
        (window as Window & { __ugoiteQueryEvents?: QueryEvent[] })
          .__ugoiteQueryEvents?.filter((event) =>
            event.path.includes("/sql/count")
          ).length ?? 0
      );
      await page.getByRole("button", { name: "Next" }).click();
      await expect(page.getByText("Page 2")).toBeVisible();
      const paginationState = await page.evaluate(() => ({
        events: (window as Window & {
          __ugoiteQueryEvents?: QueryEvent[];
        }).__ugoiteQueryEvents ?? [],
        dataRows: document.querySelectorAll(
          "tbody tr.paged-result-row",
        ).length,
      }));
      expect(
        paginationState.events.filter((event) =>
          event.path.includes("/sql/query")
        ),
      ).toHaveLength(initialPageCount + 1);
      expect(initialCountRequests).toBe(0);
      sqlPagination.push({
        space: space.slug,
        initialPageCount,
        initialCountRequests,
        pageCountAfterNext: paginationState.events.filter((event) =>
          event.path.includes("/sql/query")
        ).length,
        dataRowsOnPageTwo: paginationState.dataRows,
        pageSize: SQL_PAGE_SIZE,
        hasContinuation: Boolean(
          (paginationState.events.at(-1)?.body as {
            continuation?: string;
          } | undefined)?.continuation,
        ),
      });
    }

    const firstSpace = measuredSpaces[0]!;
    const secondSpace = measuredSpaces[1]!;
    await page.goto(
      getFrontendUrl(
        `/spaces/${firstSpace.space_uid}/forms/${SQL_FORM_NAME}/entries`,
      ),
      { waitUntil: "domcontentloaded" },
    );
    await expect(rowLocator).toBeVisible();
    let lifecycle: LifecycleMeasurement = {
      events: [],
      instrumentOnly: true,
      supersededInFlightCount: 0,
      actualAbortCount: 0,
      endedAbortCount: 0,
      residualPendingCount: 0,
      visibleEntryIds: [],
      staleSourceEntryIds: [],
      unexpectedTargetEntryIds: [],
      visibleDataRows: 0,
      targetSpaceQueryCount: 0,
      targetSpaceQueryPaths: [],
      rowsBeforeTargetQuery: 0,
      rowsWhileTargetQueryPending: 0,
    };
    if (Deno.env.get("UGOITE_QUERY_MEASURE_BASELINE") !== "true") {
      const sourceEntryIds = await page.locator(
        "tbody tr[data-entry-id]",
      ).evaluateAll((rows) =>
        rows.flatMap((row) => row.getAttribute("data-entry-id") ?? [])
      );
      expect(sourceEntryIds.length).toBeGreaterThan(0);
      await page.route("**/entries/query", async (route) => {
        await new Promise((resolve) => setTimeout(resolve, 400));
        try {
          await route.continue();
        } catch {
          // The browser may cancel this deliberately delayed request first.
        }
      });
      const filter = page.getByText("Filter", { exact: true });
      await filter.click();
      const searchbox = page.getByRole("searchbox");
      const firstRapidRequest = page.waitForRequest((request) =>
        request.url().includes("/entries/query")
      );
      await searchbox.fill("solar");
      await firstRapidRequest;
      const secondRapidRequest = page.waitForRequest((request) =>
        request.url().includes("/entries/query")
      );
      await searchbox.fill("energy");
      await secondRapidRequest;
      const switchingRequest = page.waitForRequest((request) =>
        request.url().includes("/entries/query")
      );
      await searchbox.fill("inspection");
      await switchingRequest;
      await page.getByLabel("Space", { exact: true }).selectOption(
        secondSpace.space_uid,
      );
      await expect(page).toHaveURL(
        new RegExp(`/spaces/${secondSpace.space_uid}/forms$`),
      );
      await page.waitForTimeout(500);
      const rowsBeforeTargetQuery = await page.locator(
        "tbody tr",
      ).count();
      const targetSpaceRequest = page.waitForRequest((request) =>
        request.url().includes(
          `/spaces/${secondSpace.space_uid}/entries/query`,
        )
      );
      await page.getByText(SQL_FORM_NAME, { exact: true }).click();
      await targetSpaceRequest;
      const rowsWhileTargetQueryPending = await page.locator(
        "tbody tr",
      ).count();
      expect(rowsWhileTargetQueryPending).toBe(0);
      await expect(rowLocator).toBeVisible();
      await page.waitForTimeout(500);
      lifecycle = await page.evaluate(({
        targetSpaceUid,
        sourceEntryIds,
        rowsBeforeTargetQuery,
        rowsWhileTargetQueryPending,
      }) => {
        const events = (window as Window & {
          __ugoiteQueryEvents?: QueryEvent[];
        }).__ugoiteQueryEvents ?? [];
        const targetSpaceEvents = events.filter((event) =>
          event.path.includes(`/spaces/${targetSpaceUid}/entries/query`)
        );
        const visibleEntryIds = Array.from(document.querySelectorAll(
          "tbody tr[data-entry-id]",
        )).flatMap((row) => row.getAttribute("data-entry-id") ?? []);
        const targetEntryIds = new Set(
          targetSpaceEvents.flatMap((event) => event.entryIds ?? []),
        );
        return {
          events,
          supersededInFlightCount: events.filter((event) =>
            event.aborted
          ).length,
          actualAbortCount: events.filter((event) => event.aborted).length,
          endedAbortCount: events.filter((event) =>
            event.aborted && event.endedAt !== undefined
          ).length,
          residualPendingCount: events.filter((event) =>
            event.endedAt === undefined
          ).length,
          visibleEntryIds,
          staleSourceEntryIds: visibleEntryIds.filter((id) =>
            sourceEntryIds.includes(id)
          ),
          unexpectedTargetEntryIds: visibleEntryIds.filter((id) =>
            !targetEntryIds.has(id)
          ),
          visibleDataRows: document.querySelectorAll(
            "tbody tr",
          ).length,
          targetSpaceQueryCount: targetSpaceEvents.length,
          targetSpaceQueryPaths: targetSpaceEvents.map((event) => event.path),
          rowsBeforeTargetQuery,
          rowsWhileTargetQueryPending,
        };
      }, {
        targetSpaceUid: secondSpace.space_uid,
        sourceEntryIds,
        rowsBeforeTargetQuery,
        rowsWhileTargetQueryPending,
      });
      expect(lifecycle.actualAbortCount).toBeGreaterThan(0);
      expect(lifecycle.residualPendingCount).toBe(0);
      expect(rowsBeforeTargetQuery).toBe(0);
      expect(lifecycle.targetSpaceQueryCount).toBeGreaterThan(0);
      expect(
        lifecycle.targetSpaceQueryPaths.every((path) =>
          path.includes(`/spaces/${secondSpace.space_uid}/`)
        ),
      ).toBe(true);
      expect(lifecycle.visibleDataRows).toBeGreaterThan(0);
      expect(lifecycle.staleSourceEntryIds).toEqual([]);
      expect(lifecycle.unexpectedTargetEntryIds).toEqual([]);

      const measureSqlLifecycle = async (
        path: string,
        requestPath: string,
        trigger: () => Promise<void>,
      ): Promise<QueryLifecycleMeasurement> => {
        await page.goto(getFrontendUrl(path), {
          waitUntil: "domcontentloaded",
        });
        await expect(rowLocator).toBeVisible();
        await page.route(`**${requestPath}`, async (route) => {
          await new Promise((resolve) => setTimeout(resolve, 400));
          try {
            await route.continue();
          } catch {
            // The route may be canceled while deliberately held in flight.
          }
        });
        const pendingRequest = page.waitForRequest((request) =>
          request.url().includes(requestPath)
        );
        await trigger();
        await pendingRequest;
        await page.getByLabel("Space", { exact: true }).selectOption(
          secondSpace.space_uid,
        );
        await expect(page).toHaveURL(
          new RegExp(`/spaces/${secondSpace.space_uid}/(?:forms|search)$`),
        );
        await page.waitForTimeout(500);
        const events = await page.evaluate(
          (pathFragment) =>
            ((window as Window & {
              __ugoiteQueryEvents?: QueryEvent[];
            }).__ugoiteQueryEvents ?? []).filter((event) =>
              event.path.includes(pathFragment)
            ),
          requestPath,
        );
        const abortedEvents = events.filter((event) => event.aborted);
        return {
          supersededInFlightCount: abortedEvents.length,
          actualAbortCount: abortedEvents.length,
          endedAbortCount: abortedEvents.filter((event) =>
            event.endedAt !== undefined
          ).length,
          residualPendingCount: events.filter((event) =>
            event.endedAt === undefined
          ).length,
          events,
        };
      };

      lifecycle.sqlPageChange = await measureSqlLifecycle(
        `/spaces/${firstSpace.space_uid}/sql/${firstSpace.saved_sql_id}/run`,
        "/sql/query",
        async () => {
          await page.getByRole("button", { name: "Next" }).click();
        },
      );
      expect(lifecycle.sqlPageChange.actualAbortCount).toBeGreaterThan(0);
      expect(lifecycle.sqlPageChange.endedAbortCount).toBe(
        lifecycle.sqlPageChange.actualAbortCount,
      );
      expect(lifecycle.sqlPageChange.residualPendingCount).toBe(0);

      lifecycle.sqlCountIdentityChange = await measureSqlLifecycle(
        `/spaces/${firstSpace.space_uid}/sql/${firstSpace.saved_sql_id}/run`,
        "/sql/count",
        async () => {
          await page.getByRole("button", { name: "Count rows" }).click();
        },
      );
      expect(
        lifecycle.sqlCountIdentityChange.actualAbortCount,
      ).toBeGreaterThan(0);
      expect(lifecycle.sqlCountIdentityChange.endedAbortCount).toBe(
        lifecycle.sqlCountIdentityChange.actualAbortCount,
      );
      expect(lifecycle.sqlCountIdentityChange.residualPendingCount).toBe(0);
    }

    const outputPath = Deno.env.get("UGOITE_QUERY_MEASURE_OUTPUT");
    if (outputPath) {
      const entryTrials = trials.filter((trial) =>
        trial.surface === "entry-query"
      );
      const sqlTrials = trials.filter((trial) => trial.surface === "sql-page");
      const summarize = (values: Array<Record<string, unknown>>) => {
        const durations = values.map((value) =>
          Number(value.elapsedToFirstVisibleRowMs)
        );
        return {
          trials: values.length,
          p50Ms: percentile(durations, 0.5),
          p95Ms: percentile(durations, 0.95),
        };
      };
      const outputDirectory = outputPath.slice(
        0,
        outputPath.lastIndexOf("/"),
      );
      if (outputDirectory) {
        await Deno.mkdir(outputDirectory, { recursive: true });
      }
      await Deno.writeTextFile(
        outputPath,
        JSON.stringify(
          {
            schema_version: 1,
            source_sha: Deno.env.get("UGOITE_SOURCE_SHA") ?? "unknown",
            date: new Date().toISOString(),
            dataset: {
              seed:
                "renewable-ops; Space A 3134001/6,000 Entries; Space B 3134002/4,000 Entries",
              spaces: measuredSpaces,
              entry_type_distribution: {
                Site: "2%",
                Array: "8%",
                Inspection: "20%",
                MaintenanceTicket: "25%",
                EnergyReport: "45%",
              },
              measured_form: SQL_FORM_NAME,
              forms: formMetadata,
              sql: Object.fromEntries(sqlBySpace),
              page_size: SQL_PAGE_SIZE,
            },
            environment: {
              backend: "Ugoite server under direct-process local E2E",
              backend_source_sha: Deno.env.get("UGOITE_BACKEND_SOURCE_SHA") ??
                "unknown",
              backend_startup_seconds: Number(
                Deno.env.get("UGOITE_BACKEND_STARTUP_SECONDS") ?? "NaN",
              ),
              storage: "local filesystem Space root",
              network:
                "browser and server on local loopback; no external network path",
              viewport: trials[0]?.viewport ?? null,
              user_agent: trials[0]?.userAgent ?? null,
              heap_api:
                "performance.memory.usedJSHeapSize when Chromium exposes it; otherwise null",
              lifecycle_interception:
                "400 ms Playwright delay on EntryQuery requests only; performance trials are not intercepted",
            },
            summaries: {
              entry_query_first_visible_row: summarize(entryTrials),
              sql_first_visible_row: summarize(sqlTrials),
            },
            trials,
            sql_pagination: sqlPagination,
            lifecycle,
            limitations: [
              "AbortSignal cancellation is measured in the browser fetch layer; it does not establish cancellation of SQL engine work after a server has begun execution.",
              "Performance values include frontend navigation, local HTTP, server, storage, and rendering; the per-trial browser API heap value is not process RSS.",
            ],
          },
          null,
          2,
        ) + "\n",
      );
    }
  } finally {
    for (const space of measuredSpaces) {
      const savedSqlId = space.saved_sql_id;
      if (savedSqlId) {
        await request.delete(
          getBackendUrl(`/spaces/${space.space_uid}/sql/${savedSqlId}`),
        );
      }
    }
  }
});
