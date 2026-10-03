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
  abortedAt?: number;
  abortedBeforeEnd?: boolean;
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
  rapidSearchChanges?: QueryLifecycleMeasurement;
  spaceChange?: QueryLifecycleMeasurement;
  sqlPageChange?: QueryLifecycleMeasurement;
  sqlCountIdentityChange?: QueryLifecycleMeasurement;
  sqlPreviousPage?: QueryLifecycleMeasurement;
  sqlParameterValueChange?: QueryLifecycleMeasurement;
  sqlParameterTypeChange?: QueryLifecycleMeasurement;
  sqlCountRetry?: QueryLifecycleMeasurement & {
    countRequestCount: number;
    queryRequestCountBeforeRetry: number;
    queryRequestCountAfterRetry: number;
  };
  sqlRouteDispose?: QueryLifecycleMeasurement;
};
type QueryLifecycleMeasurement = {
  supersededInFlightCount: number;
  actualAbortCount: number;
  endedAbortCount: number;
  residualPendingCount: number;
  events: QueryEvent[];
};

function summarizeLifecycleEvents(
  events: QueryEvent[],
): QueryLifecycleMeasurement {
  const abortedEvents = events.filter((event) =>
    event.abortedBeforeEnd === true
  );
  return {
    supersededInFlightCount: abortedEvents.length,
    actualAbortCount: abortedEvents.length,
    endedAbortCount:
      abortedEvents.filter((event) => event.endedAt !== undefined).length,
    residualPendingCount:
      events.filter((event) => event.endedAt === undefined).length,
    events,
  };
}

test("counts an in-flight abort when abort and settlement timestamps tie", () => {
  const atSameTime: QueryEvent = {
    path: "/spaces/test/entries/query",
    method: "POST",
    startedAt: 1,
    abortedAt: 2,
    endedAt: 2,
    aborted: true,
    abortedBeforeEnd: true,
  };
  const afterSettlement: QueryEvent = {
    ...atSameTime,
    abortedBeforeEnd: false,
  };

  const summary = summarizeLifecycleEvents([atSameTime, afterSettlement]);

  expect(summary.actualAbortCount).toBe(1);
  expect(summary.endedAbortCount).toBe(1);
  expect(summary.residualPendingCount).toBe(0);
});

const MEASUREMENT_SLUGS = ["query-space-a", "query-space-b"] as const;
const TRIAL_COUNT = 5;
const SQL_FORM_NAME = "MaintenanceTicket";
const SQL_PAGE_SIZE = 100;
const QUERY_RESULT_TIMEOUT_MS = 30_000;

function percentile(values: number[], p: number): number {
  const sorted = [...values].sort((left, right) => left - right);
  return sorted[Math.ceil(p * sorted.length) - 1] ?? 0;
}

type ExpectedFixture = { seed: number; entries: number };

function expectedFixtureFor(slug: string): ExpectedFixture {
  const raw = Deno.env.get("UGOITE_QUERY_MEASURE_EXPECTED_JSON") ?? "";
  let mapping: unknown;
  try {
    mapping = JSON.parse(raw);
  } catch {
    throw new Error(
      "UGOITE_QUERY_MEASURE_EXPECTED_JSON is missing or is not valid JSON",
    );
  }
  if (typeof mapping !== "object" || mapping === null) {
    throw new Error("UGOITE_QUERY_MEASURE_EXPECTED_JSON must be an object");
  }
  const entry = (mapping as Record<string, unknown>)[slug];
  if (typeof entry !== "object" || entry === null) {
    throw new Error(
      `UGOITE_QUERY_MEASURE_EXPECTED_JSON has no entry for ${slug}`,
    );
  }
  const { seed, entries } = entry as { seed: unknown; entries: unknown };
  if (
    !Number.isSafeInteger(seed) || (seed as number) <= 0 ||
    !Number.isSafeInteger(entries) || (entries as number) <= 0
  ) {
    throw new Error(
      `UGOITE_QUERY_MEASURE_EXPECTED_JSON entry for ${slug} must hold a positive seed and entry count`,
    );
  }
  return { seed: seed as number, entries: entries as number };
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
    seed: number;
    expected_entries: number;
    saved_sql_id?: string;
    parameterized_sql_id?: string;
  }> = MEASUREMENT_SLUGS.map((slug) => {
    const space = spaces.find((candidate) =>
      candidate.slug === slug || candidate.name === slug
    );
    expect(space, `seeded Space ${slug} is visible to the test account`)
      .toBeTruthy();
    // Expected seed and entry counts come from the shared CP1 fixture
    // specification via UGOITE_QUERY_MEASURE_EXPECTED_JSON (set by
    // scripts/measure-query-surfaces.sh). There is no hardcoded fallback:
    // a missing or malformed mapping fails the test instead of asserting
    // against stale counts.
    const expected = expectedFixtureFor(slug);
    return {
      slug,
      space_uid: space!.space_uid,
      seed: expected.seed,
      expected_entries: expected.entries,
    };
  });

  for (const space of measuredSpaces) {
    const countResponse = await request.post(
      getBackendUrl(`/spaces/${space.space_uid}/entries/query/count`),
      {
        data: {
          query: { scope: { kind: "all" }, filters: [], sort: [] },
        },
      },
    );
    expect(countResponse.ok()).toBeTruthy();
    expect(await countResponse.json()).toEqual({
      count: space.expected_entries,
    });
  }

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

    const sqlCountResponse = await request.post(
      getBackendUrl(`/spaces/${space.space_uid}/sql/query/count`),
      { data: { sql } },
    );
    expect(sqlCountResponse.ok()).toBeTruthy();
    expect(await sqlCountResponse.json()).toEqual({
      count: space.expected_entries / 4,
    });

    if (space === measuredSpaces[0]) {
      const parameterizedSql =
        `SELECT CAST($threshold AS BIGINT) AS threshold_value ` +
        `FROM "${form!.sql_relation}" ` +
        "ORDER BY _ugoite_id";
      const createParameterizedSqlResponse = await request.post(
        getBackendUrl(`/spaces/${space.space_uid}/sql`),
        {
          data: {
            name: `Query measurement ${space.slug} parameters`,
            kind: "user-query",
            sql: parameterizedSql,
            variables: [{
              name: "threshold",
              type: "integer",
              description: "Synthetic lifecycle threshold",
            }],
          },
        },
      );
      expect([200, 201]).toContain(createParameterizedSqlResponse.status());
      const parameterizedSavedSql = await createParameterizedSqlResponse
        .json() as {
          id: string;
        };
      space.parameterized_sql_id = parameterizedSavedSql.id;
    }
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
        !path.includes("/sql/query/count")
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
                const body = JSON.parse(init.body as string) as unknown;
                if (
                  typeof body === "object" && body !== null &&
                  "continuation" in body &&
                  typeof body.continuation === "string"
                ) {
                  return { ...body, continuation: "[redacted]" };
                }
                return body;
              } catch {
                return undefined;
              }
            })(),
          }
          : {}),
      };
      signal?.addEventListener("abort", () => {
        event.aborted = true;
        event.abortedAt ??= performance.now();
        if (event.endedAt === undefined) event.abortedBeforeEnd = true;
      }, { once: true });
      measured.__ugoiteQueryEvents!.push(event);
      try {
        const response = await nativeFetch(input, init);
        event.endedAt = performance.now();
        event.abortedBeforeEnd = false;
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
        // Fetch can reject before the AbortSignal event listener is delivered.
        // Sample the signal here so an in-flight abort is still ordered before
        // fetch settlement; an abort after a resolved response stays ordered
        // after endedAt and is not counted as cancellation.
        if (signal?.aborted) {
          event.aborted = true;
          event.abortedAt ??= performance.now();
          event.abortedBeforeEnd = true;
        }
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
        await expect(rowLocator).toBeVisible({
          timeout: QUERY_RESULT_TIMEOUT_MS,
        });
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
        await expect(rowLocator).toBeVisible({
          timeout: QUERY_RESULT_TIMEOUT_MS,
        });
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
            event.path.includes("/sql/query/count")
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
      await expect(rowLocator).toBeVisible({
        timeout: QUERY_RESULT_TIMEOUT_MS,
      });
      const initialPageCount = await page.evaluate(() =>
        (window as Window & { __ugoiteQueryEvents?: QueryEvent[] })
          .__ugoiteQueryEvents?.filter((event) =>
            event.path.includes("/sql/query")
          ).length ?? 0
      );
      const initialCountRequests = await page.evaluate(() =>
        (window as Window & { __ugoiteQueryEvents?: QueryEvent[] })
          .__ugoiteQueryEvents?.filter((event) =>
            event.path.includes("/sql/query/count")
          ).length ?? 0
      );
      await page.getByRole("button", { name: "Next" }).click();
      await expect(page.getByText("Page 2")).toBeVisible();
      const paginationState = await page.evaluate(() => ({
        events: (window as Window & {
          __ugoiteQueryEvents?: QueryEvent[];
        }).__ugoiteQueryEvents ?? [],
        dataRows: document.querySelectorAll(
          "tbody tr:not(.paged-result-spacer)",
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
    await expect(rowLocator).toBeVisible({
      timeout: QUERY_RESULT_TIMEOUT_MS,
    });
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
      const searchRouteGates = new Map<string, {
        reached: Promise<void>;
        finished: Promise<void>;
        held: Promise<void>;
        release: () => void;
        markReached: () => void;
        markFinished: () => void;
        wasReached: boolean;
      }>();
      let releaseTargetSpaceQuery!: () => void;
      let markTargetSpaceQueryReached!: () => void;
      let markTargetSpaceQueryFinished!: () => void;
      const targetSpaceQueryHeld = new Promise<void>((resolve) => {
        releaseTargetSpaceQuery = resolve;
      });
      const targetSpaceQueryReached = new Promise<void>((resolve) => {
        markTargetSpaceQueryReached = resolve;
      });
      const targetSpaceQueryFinished = new Promise<void>((resolve) => {
        markTargetSpaceQueryFinished = resolve;
      });
      for (const text of ["solar", "energy", "inspection"]) {
        let release!: () => void;
        let markReached!: () => void;
        let markFinished!: () => void;
        const held = new Promise<void>((resolve) => release = resolve);
        const reached = new Promise<void>((resolve) => {
          markReached = resolve;
        });
        const finished = new Promise<void>((resolve) => {
          markFinished = resolve;
        });
        searchRouteGates.set(text, {
          reached,
          finished,
          held,
          release,
          markReached,
          markFinished,
          wasReached: false,
        });
      }
      await page.route("**/entries/query", async (route) => {
        if (
          route.request().url().includes(
            `/spaces/${secondSpace.space_uid}/entries/query`,
          )
        ) {
          markTargetSpaceQueryReached();
          await targetSpaceQueryHeld;
          try {
            await route.continue();
          } catch {
            // The browser may cancel the held target request first.
          } finally {
            markTargetSpaceQueryFinished();
          }
          return;
        }
        const body = route.request().postDataJSON() as {
          query?: { text?: unknown };
        } | null;
        const text = body?.query?.text;
        const gate = typeof text === "string"
          ? searchRouteGates.get(text)
          : undefined;
        if (!gate) {
          await route.continue();
          return;
        }
        gate.wasReached = true;
        gate.markReached();
        await gate.held;
        try {
          await route.continue();
        } catch {
          // The browser may cancel the held request before it is continued.
        } finally {
          gate.markFinished();
        }
      });
      const searchbox = page.getByRole("searchbox");
      const waitForSearchRequest = (text: string) =>
        page.waitForRequest((request) => {
          if (!request.url().includes("/entries/query")) return false;
          const body = request.postDataJSON() as {
            query?: { text?: unknown };
          } | null;
          return body?.query?.text === text;
        });
      let rowsBeforeTargetQuery = 0;
      let rowsWhileTargetQueryPending = 0;
      try {
        const firstRapidRequest = waitForSearchRequest("solar");
        await searchbox.fill("solar");
        await Promise.all([
          firstRapidRequest,
          searchRouteGates.get("solar")!.reached,
        ]);
        const secondRapidRequest = waitForSearchRequest("energy");
        await searchbox.fill("energy");
        await Promise.all([
          secondRapidRequest,
          searchRouteGates.get("energy")!.reached,
        ]);
        const switchingRequest = waitForSearchRequest("inspection");
        await searchbox.fill("inspection");
        await Promise.all([
          switchingRequest,
          searchRouteGates.get("inspection")!.reached,
        ]);
        await page.getByLabel("Space", { exact: true }).selectOption(
          secondSpace.space_uid,
        );
        await expect(page).toHaveURL(
          new RegExp(`/spaces/${secondSpace.space_uid}/forms$`),
        );
        await page.waitForTimeout(500);
        rowsBeforeTargetQuery = await page.locator("tbody tr").count();
        await page.getByText(SQL_FORM_NAME, { exact: true }).click();
        await targetSpaceQueryReached;
        rowsWhileTargetQueryPending = await page.locator(
          "tbody tr",
        ).count();
        expect(rowsWhileTargetQueryPending).toBe(0);
        releaseTargetSpaceQuery();
        await expect(rowLocator).toBeVisible({
          timeout: QUERY_RESULT_TIMEOUT_MS,
        });
      } finally {
        releaseTargetSpaceQuery();
        for (const gate of searchRouteGates.values()) gate.release();
      }
      await targetSpaceQueryFinished;
      await Promise.all(
        [...searchRouteGates.values()]
          .filter((gate) => gate.wasReached)
          .map((gate) => gate.finished),
      );
      await page.unroute("**/entries/query");
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
        const abortedInFlightEvents = events.filter((event) =>
          event.abortedBeforeEnd === true
        );
        return {
          events,
          supersededInFlightCount: abortedInFlightEvents.length,
          actualAbortCount: abortedInFlightEvents.length,
          endedAbortCount: abortedInFlightEvents.filter((event) =>
            event.endedAt !== undefined
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

      const queryText = (event: QueryEvent): string | undefined => {
        const body = event.body as { query?: { text?: unknown } } | undefined;
        return typeof body?.query?.text === "string"
          ? body.query.text
          : undefined;
      };
      const entryEvents = lifecycle.events ?? [];
      lifecycle.rapidSearchChanges = summarizeLifecycleEvents(
        entryEvents.filter((event) =>
          ["solar", "energy"].includes(queryText(event) ?? "")
        ),
      );
      lifecycle.spaceChange = summarizeLifecycleEvents(
        entryEvents.filter((event) =>
          queryText(event) === "inspection" &&
          event.path.includes(`/spaces/${firstSpace.space_uid}/`)
        ),
      );
      expect(lifecycle.rapidSearchChanges.actualAbortCount).toBeGreaterThan(0);
      expect(lifecycle.rapidSearchChanges.residualPendingCount).toBe(0);
      expect(lifecycle.spaceChange.actualAbortCount).toBeGreaterThan(0);
      expect(lifecycle.spaceChange.endedAbortCount).toBe(
        lifecycle.spaceChange.actualAbortCount,
      );
      expect(lifecycle.spaceChange.residualPendingCount).toBe(0);

      const measureSqlLifecycle = async (
        path: string,
        requestPath: string,
        trigger: () => Promise<void>,
      ): Promise<QueryLifecycleMeasurement> => {
        await page.goto(getFrontendUrl(path), {
          waitUntil: "domcontentloaded",
        });
        await expect(rowLocator).toBeVisible({
          timeout: QUERY_RESULT_TIMEOUT_MS,
        });
        await page.route(`**${requestPath}`, async (route) => {
          await new Promise((resolve) => setTimeout(resolve, 400));
          try {
            await route.continue();
          } catch {
            // The route may be canceled while deliberately held in flight.
          }
        });
        const pendingRequest = page.waitForRequest(
          (request) => request.url().includes(requestPath),
          { timeout: 15_000 },
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
        return summarizeLifecycleEvents(events);
      };

      lifecycle.sqlPageChange = await measureSqlLifecycle(
        `/spaces/${firstSpace.space_uid}/sql/${firstSpace.saved_sql_id}/run`,
        "/sql/query",
        async () => {
          const nextButton = page.getByRole("button", { name: "Next" });
          await expect(nextButton).toBeEnabled();
          await nextButton.click();
        },
      );
      expect(lifecycle.sqlPageChange.actualAbortCount).toBeGreaterThan(0);
      expect(lifecycle.sqlPageChange.endedAbortCount).toBe(
        lifecycle.sqlPageChange.actualAbortCount,
      );
      expect(lifecycle.sqlPageChange.residualPendingCount).toBe(0);

      lifecycle.sqlCountIdentityChange = await measureSqlLifecycle(
        `/spaces/${firstSpace.space_uid}/sql/${firstSpace.saved_sql_id}/run`,
        "/sql/query/count",
        async () => {
          const countButton = page.getByRole("button", {
            name: "Count rows",
          });
          await expect(countButton).toBeEnabled();
          await countButton.click();
        },
      );
      expect(
        lifecycle.sqlCountIdentityChange.actualAbortCount,
      ).toBeGreaterThan(0);
      expect(lifecycle.sqlCountIdentityChange.endedAbortCount).toBe(
        lifecycle.sqlCountIdentityChange.actualAbortCount,
      );
      expect(lifecycle.sqlCountIdentityChange.residualPendingCount).toBe(0);

      const parameterizedSqlPath =
        `/spaces/${firstSpace.space_uid}/sql/${firstSpace.parameterized_sql_id}`;
      const openParameterizedSql = async (threshold: string) => {
        await page.goto(getFrontendUrl(`${parameterizedSqlPath}/variables`), {
          waitUntil: "domcontentloaded",
        });
        const thresholdInput = page.getByLabel(/threshold/);
        await expect(thresholdInput).toBeVisible();
        await thresholdInput.fill(threshold);
        await page.getByRole("button", { name: "Run" }).click();
        await expect(page).toHaveURL(
          new RegExp(`${parameterizedSqlPath}/run$`),
        );
        await expect(rowLocator).toBeVisible({
          timeout: QUERY_RESULT_TIMEOUT_MS,
        });
      };
      const changeSqlRunState = async (
        threshold: number | string,
        type: string,
      ) => {
        await page.evaluate(({ threshold, type }) => {
          const current = history.state as
            | Record<string, unknown>
            | null;
          const next = {
            ...(current ?? {}),
            parameters: { threshold },
            parameterTypes: { threshold: type },
          };
          history.replaceState(next, "", location.href);
          dispatchEvent(new PopStateEvent("popstate", { state: next }));
        }, { threshold, type });
      };
      const resetEvents = async () => {
        await page.evaluate(() => {
          (window as Window & { __ugoiteQueryEvents?: QueryEvent[] })
            .__ugoiteQueryEvents = [];
        });
      };
      const readSqlQueryEvents = async () =>
        await page.evaluate(() =>
          ((window as Window & {
            __ugoiteQueryEvents?: QueryEvent[];
          }).__ugoiteQueryEvents ?? []).filter((event) =>
            event.path.endsWith("/sql/query")
          )
        );

      await openParameterizedSql("100");
      await resetEvents();
      await page.route("**/sql/query", async (route) => {
        await new Promise((resolve) => setTimeout(resolve, 400));
        try {
          await route.continue();
        } catch {
          // The browser may cancel this deliberately delayed request first.
        }
      });
      const pendingParameterizedPage = page.waitForRequest((request) =>
        new URL(request.url()).pathname.endsWith("/sql/query")
      );
      await page.getByRole("button", { name: "Next" }).click();
      await pendingParameterizedPage;
      const changedValuePage = page.waitForRequest((request) => {
        if (
          !new URL(request.url()).pathname.endsWith("/sql/query")
        ) return false;
        const body = request.postDataJSON() as {
          parameters?: Record<string, unknown>;
        } | null;
        return body?.parameters?.threshold === 200;
      });
      await changeSqlRunState(200, "integer");
      const changedValueRequest = await changedValuePage;
      expect(changedValueRequest.postDataJSON()).toMatchObject({
        parameters: { threshold: 200 },
        parameter_types: { threshold: "integer" },
      });
      await page.waitForFunction(() =>
        ((window as Window & {
          __ugoiteQueryEvents?: QueryEvent[];
        }).__ugoiteQueryEvents ?? []).some((event) =>
          event.path.endsWith("/sql/query") && event.endedAt !== undefined &&
          (event.body as { parameters?: Record<string, unknown> } | undefined)
              ?.parameters?.threshold === 200
        )
      );
      await page.unroute("**/sql/query");
      lifecycle.sqlParameterValueChange = summarizeLifecycleEvents(
        await readSqlQueryEvents(),
      );
      expect(lifecycle.sqlParameterValueChange.actualAbortCount)
        .toBeGreaterThan(0);
      expect(lifecycle.sqlParameterValueChange.endedAbortCount).toBe(
        lifecycle.sqlParameterValueChange.actualAbortCount,
      );
      expect(lifecycle.sqlParameterValueChange.residualPendingCount).toBe(0);

      await openParameterizedSql("200");
      await resetEvents();
      let releaseCountRoute: (() => void) | undefined;
      let markCountRouteReached: (() => void) | undefined;
      let markCountRouteFinished: (() => void) | undefined;
      const countRouteHeld = new Promise<void>((resolve) => {
        releaseCountRoute = resolve;
      });
      const countRouteReached = new Promise<void>((resolve) => {
        markCountRouteReached = resolve;
      });
      const countRouteFinished = new Promise<void>((resolve) => {
        markCountRouteFinished = resolve;
      });
      await page.route("**/sql/query/count", async (route) => {
        markCountRouteReached?.();
        await countRouteHeld;
        try {
          await route.continue();
        } catch {
          // The browser may cancel this deliberately delayed request first.
        } finally {
          markCountRouteFinished?.();
        }
      });
      const pendingParameterizedCount = page.waitForRequest((request) =>
        new URL(request.url()).pathname.endsWith("/sql/query/count")
      );
      try {
        await page.getByRole("button", { name: "Count rows" }).click();
        await Promise.all([pendingParameterizedCount, countRouteReached]);
        const changedTypePage = page.waitForRequest((request) => {
          if (
            !new URL(request.url()).pathname.endsWith("/sql/query")
          ) return false;
          const body = request.postDataJSON() as {
            parameters?: Record<string, unknown>;
            parameter_types?: Record<string, string>;
          } | null;
          return body?.parameters?.threshold === "200" &&
            body.parameter_types?.threshold === "string";
        });
        await changeSqlRunState("200", "string");
        const changedTypeRequest = await changedTypePage;
        expect(changedTypeRequest.postDataJSON()).toMatchObject({
          parameters: { threshold: "200" },
          parameter_types: { threshold: "string" },
        });
        await page.waitForFunction(() =>
          ((window as Window & {
            __ugoiteQueryEvents?: QueryEvent[];
          }).__ugoiteQueryEvents ?? []).some((event) =>
            event.path.endsWith("/sql/query") &&
            event.endedAt !== undefined &&
            (event.body as
                | { parameter_types?: Record<string, string> }
                | undefined)
                ?.parameter_types?.threshold === "string"
          )
        );
      } finally {
        // Let Playwright drain an intercepted request even if an assertion fails.
        releaseCountRoute?.();
      }
      await countRouteFinished;
      await page.unroute("**/sql/query/count");
      lifecycle.sqlParameterTypeChange = summarizeLifecycleEvents(
        await page.evaluate(() =>
          ((window as Window & {
            __ugoiteQueryEvents?: QueryEvent[];
          }).__ugoiteQueryEvents ?? []).filter((event) =>
            event.path.endsWith("/sql/query/count")
          )
        ),
      );
      expect(lifecycle.sqlParameterTypeChange.actualAbortCount)
        .toBeGreaterThan(0);
      expect(lifecycle.sqlParameterTypeChange.endedAbortCount).toBe(
        lifecycle.sqlParameterTypeChange.actualAbortCount,
      );
      expect(lifecycle.sqlParameterTypeChange.residualPendingCount).toBe(0);

      await openParameterizedSql("100");
      await resetEvents();
      const pageOneQueryCountBeforePagination =
        (await readSqlQueryEvents()).length;
      await page.getByRole("button", { name: "Next" }).click();
      await expect(page.getByText("Page 2", { exact: true })).toBeVisible();
      await page.getByRole("button", { name: "Previous" }).click();
      await expect(page.getByText("Page 1", { exact: true })).toBeVisible();
      lifecycle.sqlPreviousPage = summarizeLifecycleEvents(
        await readSqlQueryEvents(),
      );
      expect(lifecycle.sqlPreviousPage.events).toHaveLength(
        pageOneQueryCountBeforePagination + 2,
      );

      await openParameterizedSql("100");
      await resetEvents();
      const countRouteAttempts: string[] = [];
      await page.route("**/sql/query/count", async (route) => {
        countRouteAttempts.push("/sql/query/count");
        if (countRouteAttempts.length === 1) {
          await route.abort("failed");
          return;
        }
        await route.continue();
      });
      const queryRequestCountBeforeRetry = (await readSqlQueryEvents()).length;
      expect(
        await page.evaluate(() =>
          ((window as Window & {
            __ugoiteQueryEvents?: QueryEvent[];
          }).__ugoiteQueryEvents ?? []).filter((event) =>
            event.path.endsWith("/sql/query/count")
          ).length
        ),
      ).toBe(0);
      await page.getByRole("button", { name: "Count rows" }).click();
      await expect(page.getByText(/count query rows/i)).toBeVisible();
      const queryRequestCountBeforeSuccessfulRetry =
        (await readSqlQueryEvents()).length;
      await page.getByRole("button", { name: "Count rows" }).click();
      await expect(page.getByText(/^\d+ rows$/)).toBeVisible();
      await page.unroute("**/sql/query/count");
      const countEvents = await page.evaluate(() =>
        ((window as Window & {
          __ugoiteQueryEvents?: QueryEvent[];
        }).__ugoiteQueryEvents ?? []).filter((event) =>
          event.path.endsWith("/sql/query/count")
        )
      );
      expect(countRouteAttempts).toHaveLength(2);
      expect((await readSqlQueryEvents()).length)
        .toBe(queryRequestCountBeforeSuccessfulRetry);
      lifecycle.sqlCountRetry = {
        ...summarizeLifecycleEvents(countEvents),
        countRequestCount: countEvents.length,
        queryRequestCountBeforeRetry,
        queryRequestCountAfterRetry: (await readSqlQueryEvents()).length,
      };
      expect(lifecycle.sqlCountRetry.countRequestCount).toBe(2);
      expect(lifecycle.sqlCountRetry.queryRequestCountAfterRetry).toBe(
        lifecycle.sqlCountRetry.queryRequestCountBeforeRetry,
      );

      await openParameterizedSql("100");
      await resetEvents();
      await page.route("**/sql/query", async (route) => {
        await new Promise((resolve) => setTimeout(resolve, 400));
        try {
          await route.continue();
        } catch {
          // The route may be canceled when the result route is disposed.
        }
      });
      const pendingDisposedPage = page.waitForRequest((request) =>
        new URL(request.url()).pathname.endsWith("/sql/query")
      );
      await page.getByRole("button", { name: "Next" }).click();
      await pendingDisposedPage;
      await page.getByRole("link", { name: "Back to Saved SQL" }).click();
      await expect(page).toHaveURL(
        new RegExp(`${parameterizedSqlPath}$`),
      );
      await page.waitForTimeout(500);
      await page.unroute("**/sql/query");
      lifecycle.sqlRouteDispose = summarizeLifecycleEvents(
        await readSqlQueryEvents(),
      );
      expect(lifecycle.sqlRouteDispose.actualAbortCount).toBeGreaterThan(0);
      expect(lifecycle.sqlRouteDispose.endedAbortCount).toBe(
        lifecycle.sqlRouteDispose.actualAbortCount,
      );
      expect(lifecycle.sqlRouteDispose.residualPendingCount).toBe(0);
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
              seed: "renewable-ops; " +
                measuredSpaces.map((space) =>
                  `${space.slug} ${space.seed}/${space.expected_entries} Entries`
                ).join("; "),
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
                "Playwright holds selected superseded EntryQuery and SQL page/count requests until identity changes; first-visible-row performance trials are not delayed",
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
      for (
        const savedSqlId of [
          space.saved_sql_id,
          space.parameterized_sql_id,
        ]
      ) {
        if (savedSqlId) {
          await request.delete(
            getBackendUrl(`/spaces/${space.space_uid}/sql/${savedSqlId}`),
          );
        }
      }
    }
  }
});
