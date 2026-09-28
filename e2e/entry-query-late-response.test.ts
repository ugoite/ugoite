import { expect, type Page, test } from "@playwright/test";
import {
  getBackendUrl,
  getDefaultSpaceId,
  getFrontendUrl,
  waitForServers,
} from "./lib/client.ts";

type ProbeEvent = {
  name: string;
  at: number;
  key?: string;
  spaceId?: string;
  text?: string;
  details?: Record<string, unknown>;
};

type ProbeRequest = {
  key: string;
  spaceId: string;
  text: string;
  outcome: "deliver" | "reject";
  held: boolean;
  internalStatus?: number;
  originalSignalAborted: boolean;
  internalSignalForwarded: false;
  delivered: boolean;
  rejected: boolean;
  uiCycleObserved: boolean;
};

type BrowserProbe = {
  events: ProbeEvent[];
  requests: Record<string, ProbeRequest>;
  visibleHistory: Record<string, string[][]>;
  record: (name: string, event?: Partial<ProbeEvent>) => void;
  release: (key: string) => void;
  startVisibleHistory: (key: string) => void;
};

type ProbeWindow = Window & { __entryQueryLateProbe?: BrowserProbe };

type SpaceRecord = { space_uid: string; slug?: string; name?: string };
type QueryPage = {
  rows: Array<{ id: string }>;
  has_more: boolean;
  next?: string;
};

type UiSnapshot = {
  visibleEntryIds: string[];
  loading: boolean;
  errors: string[];
  pageIdentity: string | null;
  previousDisabled: boolean | null;
  nextDisabled: boolean | null;
};

test.use({ trace: "on" });

test(
  "@smoke EntryQuery ignores delayed superseded responses in Chromium",
  async ({
    page,
    request,
  }, testInfo) => {
    test.setTimeout(180_000);
    await waitForServers(request);

    const runId = `${Date.now()}${Math.random().toString(36).slice(2, 8)}`;
    const defaultSpaceId = await getDefaultSpaceId(request);
    const secondSpaceSlug = `late-query-${runId}`;
    const secondSpaceResponse = await request.post(getBackendUrl("/spaces"), {
      data: {
        slug: secondSpaceSlug,
        name: `Late query ${runId}`,
      },
    });
    expect([200, 201]).toContain(secondSpaceResponse.status());
    const spacesResponse = await request.get(getBackendUrl("/spaces"));
    expect(spacesResponse.ok()).toBeTruthy();
    const spaces = await spacesResponse.json() as SpaceRecord[];
    const secondSpace = spaces.find((space) => space.slug === secondSpaceSlug);
    expect(secondSpace?.space_uid).toBeTruthy();
    const secondSpaceId = secondSpace!.space_uid;

    const formName = `LateQuery${runId}`;
    await createForm(request, defaultSpaceId, formName);
    await createForm(request, secondSpaceId, formName);

    const successOldText = `lateold${runId}`;
    const rejectOldText = `latereject${runId}`;
    const currentText = `currentnew${runId}`;
    const rejectionCurrentText = `currentafterreject${runId}`;
    const priorSpaceText = `spaceaprior${runId}`;
    const crossSpaceOldText = `spaceaold${runId}`;
    const secondSpaceCurrentText = `spacebcurrent${runId}`;

    const entryIds = {
      successOld: await createEntry(
        request,
        defaultSpaceId,
        formName,
        successOldText,
      ),
      rejectOld: await createEntry(
        request,
        defaultSpaceId,
        formName,
        rejectOldText,
      ),
      priorSpace: await createEntry(
        request,
        defaultSpaceId,
        formName,
        priorSpaceText,
      ),
      crossSpaceOld: await createEntry(
        request,
        defaultSpaceId,
        formName,
        crossSpaceOldText,
      ),
      secondSpaceCurrent: await createEntry(
        request,
        secondSpaceId,
        formName,
        secondSpaceCurrentText,
      ),
      rejectionCurrent: await createEntry(
        request,
        defaultSpaceId,
        formName,
        rejectionCurrentText,
      ),
      current: [] as string[],
    };
    for (let firstIndex = 0; firstIndex < 51; firstIndex += 10) {
      const indexes = Array.from(
        { length: Math.min(10, 51 - firstIndex) },
        (_, offset) => firstIndex + offset,
      );
      entryIds.current.push(
        ...await Promise.all(indexes.map((index) =>
          createEntry(
            request,
            defaultSpaceId,
            formName,
            `${currentText} row${index}`,
          )
        )),
      );
    }

    const expectedPageOne = await waitForQueryPage(
      request,
      defaultSpaceId,
      currentText,
      50,
    );
    expect(expectedPageOne.rows).toHaveLength(50);
    expect(expectedPageOne.has_more).toBe(true);
    expect(expectedPageOne.next).toBeTruthy();
    expect(
      expectedPageOne.rows.every((row) => entryIds.current.includes(row.id)),
    )
      .toBe(true);
    const expectedPageTwo = await queryPage(
      request,
      defaultSpaceId,
      currentText,
      50,
      expectedPageOne.next,
    );
    expect(expectedPageTwo.rows).toHaveLength(1);
    expect(expectedPageTwo.has_more).toBe(false);

    const heldQueries = [
      {
        key: "same-space-success",
        spaceId: defaultSpaceId,
        text: successOldText,
        outcome: "deliver" as const,
      },
      {
        key: "same-space-rejection",
        spaceId: defaultSpaceId,
        text: rejectOldText,
        outcome: "reject" as const,
      },
      {
        key: "space-switch",
        spaceId: defaultSpaceId,
        text: crossSpaceOldText,
        outcome: "deliver" as const,
      },
    ];
    await page.addInitScript((config) => {
      type HeldQuery = {
        key: string;
        spaceId: string;
        text: string;
        outcome: "deliver" | "reject";
      };
      type CurrentQuery = { key: string; spaceId: string; text: string };
      type RequestRecord = ProbeRequest & { releaseGate?: () => void };
      type InitWindow = Window & { __entryQueryLateProbe?: BrowserProbe };

      const target = config as {
        heldQueries: HeldQuery[];
        currentQueries: CurrentQuery[];
      };
      const requests: Record<string, RequestRecord> = {};
      const startedCurrentQueries = new Set<string>();
      const visibleHistory: Record<string, string[][]> = {};
      let visibleObserver: MutationObserver | undefined;
      const events: ProbeEvent[] = [];
      const record = (name: string, event: Partial<ProbeEvent> = {}) => {
        events.push({ name, at: performance.now(), ...event });
      };
      const probe = {
        events,
        requests,
        visibleHistory,
        record,
        release: (key: string) => requests[key]?.releaseGate?.(),
        startVisibleHistory: (key: string) => {
          visibleObserver?.disconnect();
          const history = visibleHistory[key] ??= [];
          const capture = () => {
            const ids = Array.from(document.querySelectorAll(
              "tbody tr[data-entry-id]",
            )).flatMap((row) => row.getAttribute("data-entry-id") ?? []);
            history.push(ids);
          };
          capture();
          visibleObserver = new MutationObserver(capture);
          visibleObserver.observe(document.body, {
            childList: true,
            subtree: true,
            attributes: true,
          });
        },
      } satisfies BrowserProbe;
      (window as InitWindow).__entryQueryLateProbe = probe;

      const nativeFetch = window.fetch.bind(window);
      window.fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
        const rawUrl = input instanceof Request ? input.url : String(input);
        const url = new URL(rawUrl, window.location.href);
        const match = url.pathname.match(/\/spaces\/([^/]+)\/entries\/query$/);
        if (!match) return await nativeFetch(input, init);

        let bodyText = typeof init?.body === "string" ? init.body : "";
        if (!bodyText && input instanceof Request) {
          bodyText = await input.clone().text().catch(() => "");
        }
        let queryText: string | undefined;
        try {
          const body = JSON.parse(bodyText) as { query?: { text?: unknown } };
          queryText = typeof body.query?.text === "string"
            ? body.query.text
            : undefined;
        } catch {
          queryText = undefined;
        }
        const spaceId = decodeURIComponent(match[1]);
        const heldQuery = target.heldQueries.find((candidate) =>
          candidate.spaceId === spaceId && candidate.text === queryText &&
          !requests[candidate.key]
        );
        if (!heldQuery) {
          const currentQuery = target.currentQueries.find((candidate) =>
            candidate.spaceId === spaceId && candidate.text === queryText &&
            !startedCurrentQueries.has(candidate.key)
          );
          if (currentQuery) {
            startedCurrentQueries.add(currentQuery.key);
            record("new_started", {
              key: currentQuery.key,
              spaceId,
              text: queryText,
            });
          }
          return await nativeFetch(input, init);
        }

        const signal = init?.signal ??
          (input instanceof Request ? input.signal : undefined);
        const requestRecord: RequestRecord = {
          key: heldQuery.key,
          spaceId,
          text: heldQuery.text,
          outcome: heldQuery.outcome,
          held: false,
          originalSignalAborted: signal?.aborted ?? false,
          internalSignalForwarded: false,
          delivered: false,
          rejected: false,
          uiCycleObserved: false,
        };
        requests[heldQuery.key] = requestRecord;
        record("old_started", {
          key: heldQuery.key,
          spaceId,
          text: heldQuery.text,
        });
        signal?.addEventListener("abort", () => {
          requestRecord.originalSignalAborted = true;
          record("original_signal_aborted", {
            key: heldQuery.key,
            spaceId,
            text: heldQuery.text,
          });
        }, { once: true });
        if (signal?.aborted) {
          record("original_signal_aborted", {
            key: heldQuery.key,
            spaceId,
            text: heldQuery.text,
          });
        }

        const response = input instanceof Request
          ? await nativeFetch(new Request(input, { signal: null }))
          : await nativeFetch(input, { ...init, signal: undefined });
        requestRecord.internalStatus = response.status;
        record("internal_http_response", {
          key: heldQuery.key,
          spaceId,
          text: heldQuery.text,
          details: { status: response.status, signalForwarded: false },
        });
        await new Promise<void>((resolve) => {
          requestRecord.releaseGate = resolve;
          requestRecord.held = true;
        });

        if (heldQuery.outcome === "reject") {
          requestRecord.rejected = true;
          record("old_fetch_rejected", {
            key: heldQuery.key,
            spaceId,
            text: heldQuery.text,
          });
          throw new TypeError("test-delayed old fetch rejection");
        }

        requestRecord.delivered = true;
        record("old_fetch_returning", {
          key: heldQuery.key,
          spaceId,
          text: heldQuery.text,
        });
        return response;
      };
    }, {
      heldQueries,
      currentQueries: [
        {
          key: "same-space-success",
          spaceId: defaultSpaceId,
          text: currentText,
        },
        {
          key: "same-space-rejection",
          spaceId: defaultSpaceId,
          text: rejectionCurrentText,
        },
        {
          key: "space-switch",
          spaceId: secondSpaceId,
          text: secondSpaceCurrentText,
        },
      ],
    });

    const artifactFixture = {
      runId,
      spaces: { source: defaultSpaceId, current: secondSpaceId },
      formName,
      ids: entryIds,
    };
    let evidenceAttached = false;
    const attachEvidence = async () => {
      const probeArtifact = await readProbeArtifact(page).catch(() => null);
      const evidence = {
        fixture: artifactFixture,
        expectedPageOneIds: expectedPageOne.rows.map((row) => row.id),
        expectedPageTwoIds: expectedPageTwo.rows.map((row) => row.id),
        requiredEventOrder: {
          sameSpaceSuccess: [
            "old_started",
            "new_started",
            "new_visible",
            "old_fetch_delivered",
            "final_assertion",
          ],
          sameSpaceRejection: [
            "old_started",
            "new_started",
            "new_visible",
            "old_fetch_rejected",
            "final_assertion",
          ],
          spaceSwitch: [
            "old_started",
            "new_started",
            "new_visible",
            "old_fetch_delivered",
            "final_assertion",
          ],
        },
        trace: "Playwright trace is attached for this test (trace=on).",
        probe: probeArtifact,
      };
      const reportPath = testInfo.outputPath(
        "entry-query-late-response-events.json",
      );
      await Deno.mkdir(testInfo.outputDir, { recursive: true });
      await Deno.writeTextFile(reportPath, JSON.stringify(evidence, null, 2));
      await testInfo.attach("entry-query-late-response-events.json", {
        path: reportPath,
        contentType: "application/json",
      });
      evidenceAttached = true;
    };

    try {
      await page.goto(getFrontendUrl(`/spaces/${defaultSpaceId}/search`), {
        waitUntil: "domcontentloaded",
      });

      await submitSearch(page, successOldText);
      await waitForHeldRequest(page, "same-space-success", "deliver");
      expect(await readProbeRequest(page, "same-space-success")).toMatchObject({
        internalStatus: 200,
        held: true,
        internalSignalForwarded: false,
      });

      await submitSearch(page, currentText);
      await expectUi(page, {
        visibleEntryIds: expectedPageOne.rows.map((row) => row.id),
        loading: false,
        errors: [],
        previousDisabled: true,
        nextDisabled: false,
      });
      await recordUiEvent(page, "new_visible", "same-space-success");
      expect(await readProbeRequest(page, "same-space-success"))
        .toMatchObject({ originalSignalAborted: true });
      expect(await hasEvent(page, "new_started", "same-space-success"))
        .toBe(true);

      const successBeforeRelease = await readUi(page);
      await page.evaluate(() => {
        (window as ProbeWindow).__entryQueryLateProbe!.startVisibleHistory(
          "same-space-success",
        );
      });
      await releaseAndObserve(page, "same-space-success", "deliver");
      const successAfterRelease = await readUi(page);
      expect(successAfterRelease).toEqual(successBeforeRelease);
      expect(successAfterRelease.visibleEntryIds).not.toContain(
        entryIds.successOld,
      );
      expect(await readVisibleHistory(page, "same-space-success"))
        .not.toContain(entryIds.successOld);

      await page.locator(".paged-result-pagination button").nth(1).click();
      await expectUi(page, {
        visibleEntryIds: expectedPageTwo.rows.map((row) => row.id),
        loading: false,
        errors: [],
        previousDisabled: false,
        nextDisabled: true,
      });
      await page.locator(".paged-result-pagination button").nth(0).click();
      await expectUi(page, {
        visibleEntryIds: expectedPageOne.rows.map((row) => row.id),
        loading: false,
        errors: [],
        previousDisabled: true,
        nextDisabled: false,
      });
      await recordFinalAssertion(page, "same-space-success");

      await submitSearch(page, rejectOldText);
      await waitForHeldRequest(page, "same-space-rejection", "reject");
      expect(await readProbeRequest(page, "same-space-rejection"))
        .toMatchObject({
          internalStatus: 200,
          held: true,
          internalSignalForwarded: false,
        });
      await submitSearch(page, rejectionCurrentText);
      await expectUi(page, {
        visibleEntryIds: [entryIds.rejectionCurrent],
        loading: false,
        errors: [],
        previousDisabled: true,
        nextDisabled: true,
      });
      await recordUiEvent(page, "new_visible", "same-space-rejection");
      expect(await readProbeRequest(page, "same-space-rejection"))
        .toMatchObject({ originalSignalAborted: true });
      const rejectBeforeRelease = await readUi(page);
      await releaseAndObserve(page, "same-space-rejection", "reject");
      expect(await readUi(page)).toEqual(rejectBeforeRelease);
      expect(await readUi(page)).toMatchObject({ errors: [], loading: false });
      await recordFinalAssertion(page, "same-space-rejection");

      await submitSearch(page, priorSpaceText);
      await expectUi(page, {
        visibleEntryIds: [entryIds.priorSpace],
        loading: false,
        errors: [],
        previousDisabled: true,
        nextDisabled: true,
      });
      await submitSearch(page, crossSpaceOldText);
      await waitForHeldRequest(page, "space-switch", "deliver");
      expect(await readProbeRequest(page, "space-switch")).toMatchObject({
        internalStatus: 200,
        held: true,
        internalSignalForwarded: false,
      });

      await page.getByLabel("Space", { exact: true }).selectOption(
        secondSpaceId,
      );
      await expect(page).toHaveURL(new RegExp(`/spaces/${secondSpaceId}/`));
      if (!page.url().includes(`/spaces/${secondSpaceId}/search`)) {
        await page.getByRole("link", { name: "Search", exact: true }).click();
      }
      await expect(page).toHaveURL(
        new RegExp(`/spaces/${secondSpaceId}/search$`),
      );
      expect(await readProbeRequest(page, "space-switch"))
        .toMatchObject({ originalSignalAborted: true });

      await submitSearch(page, secondSpaceCurrentText);
      await expectUi(page, {
        visibleEntryIds: [entryIds.secondSpaceCurrent],
        loading: false,
        errors: [],
        previousDisabled: true,
        nextDisabled: true,
      });
      await recordUiEvent(page, "new_visible", "space-switch");
      await page.evaluate(() => {
        (window as ProbeWindow).__entryQueryLateProbe!.startVisibleHistory(
          "space-switch",
        );
      });
      await releaseAndObserve(page, "space-switch", "deliver");
      const crossSpaceState = await readUi(page);
      expect(crossSpaceState).toMatchObject({
        visibleEntryIds: [entryIds.secondSpaceCurrent],
        loading: false,
        errors: [],
        previousDisabled: true,
        nextDisabled: true,
      });
      const postSwitchHistory = await readVisibleHistory(page, "space-switch");
      expect(postSwitchHistory.length).toBeGreaterThan(0);
      expect(postSwitchHistory.flat()).not.toContain(entryIds.crossSpaceOld);
      expect(
        postSwitchHistory.every((ids) =>
          ids.every((id) => id === entryIds.secondSpaceCurrent)
        ),
      ).toBe(true);
      await recordFinalAssertion(page, "space-switch");

      await attachEvidence();
      for (
        const key of [
          "same-space-success",
          "same-space-rejection",
          "space-switch",
        ]
      ) {
        const requiredOrder = [
          "old_started",
          "new_started",
          "new_visible",
          key === "same-space-rejection"
            ? "old_fetch_rejected"
            : "old_fetch_delivered",
          "final_assertion",
        ];
        const observedOrder = await readEventNames(page, key);
        expect(
          await isOrdered(observedOrder, requiredOrder),
          `${key} event order: ${observedOrder.join(" < ")}`,
        ).toBe(true);
      }
    } finally {
      if (!evidenceAttached) await attachEvidence().catch(() => undefined);
      await page.evaluate(() => {
        const probe = (window as ProbeWindow).__entryQueryLateProbe;
        for (const key of Object.keys(probe?.requests ?? {})) {
          probe?.release(key);
        }
      }).catch(() => undefined);
    }
  },
);

async function createForm(
  request: Parameters<typeof waitForServers>[0],
  spaceId: string,
  name: string,
): Promise<void> {
  const response = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: {
        name,
        version: 1,
        template: `# ${name}\n\n## Body\n`,
        fields: { Body: { type: "markdown", required: false } },
      },
    },
  );
  expect([200, 201]).toContain(response.status());
}

async function createEntry(
  request: Parameters<typeof waitForServers>[0],
  spaceId: string,
  formName: string,
  body: string,
): Promise<string> {
  const response = await request.post(
    getBackendUrl(`/spaces/${spaceId}/entries`),
    { data: { form: formName, fields: { Body: body } } },
  );
  expect(response.status()).toBe(201);
  return ((await response.json()) as { id: string }).id;
}

async function queryPage(
  request: Parameters<typeof waitForServers>[0],
  spaceId: string,
  text: string,
  limit: number,
  after?: string,
): Promise<QueryPage> {
  const response = await request.post(
    getBackendUrl(`/spaces/${spaceId}/entries/query`),
    {
      data: {
        query: {
          scope: { kind: "all" },
          text,
          filters: [],
          sort: [],
        },
        projection: { kind: "preview" },
        limit,
        ...(after ? { after } : {}),
      },
    },
  );
  expect(response.ok()).toBeTruthy();
  return await response.json() as QueryPage;
}

async function waitForQueryPage(
  request: Parameters<typeof waitForServers>[0],
  spaceId: string,
  text: string,
  limit: number,
): Promise<QueryPage> {
  let current: QueryPage | undefined;
  await expect.poll(async () => {
    current = await queryPage(request, spaceId, text, limit);
    return current.rows.length;
  }, { timeout: 30_000 }).toBe(limit);
  return current!;
}

async function submitSearch(page: Page, text: string): Promise<void> {
  await page.getByLabel("Search keywords").fill(text);
  await page.getByRole("button", { name: "Search entries" }).click();
}

async function readUi(page: Page): Promise<UiSnapshot> {
  return await page.evaluate(() => {
    const pagination = document.querySelector(
      ".paged-result-pagination",
    );
    const buttons = Array.from(pagination?.querySelectorAll("button") ?? []);
    return {
      visibleEntryIds: Array.from(document.querySelectorAll(
        "tbody tr[data-entry-id]",
      )).flatMap((row) => row.getAttribute("data-entry-id") ?? []),
      loading: document.querySelector(
        ".paged-result-table[aria-busy='true']",
      ) !== null,
      errors: Array.from(document.querySelectorAll("[role='alert']")).map(
        (element) => element.textContent?.trim() ?? "",
      ),
      pageIdentity: document.querySelector(
        ".entry-browser-table-scroll",
      )?.getAttribute("data-page-identity") ?? null,
      previousDisabled: buttons[0]?.hasAttribute("disabled") ?? null,
      nextDisabled: buttons[1]?.hasAttribute("disabled") ?? null,
    };
  });
}

async function expectUi(
  page: Page,
  expected: Partial<UiSnapshot>,
): Promise<void> {
  await expect.poll(() => readUi(page), { timeout: 30_000 })
    .toMatchObject(expected);
}

async function waitForHeldRequest(
  page: Page,
  key: string,
  outcome: ProbeRequest["outcome"],
): Promise<void> {
  await page.waitForFunction(
    ({ key, outcome }) => {
      const request = (window as ProbeWindow).__entryQueryLateProbe
        ?.requests[key];
      return request?.held === true && request.outcome === outcome;
    },
    { key, outcome },
    { timeout: 30_000 },
  );
}

async function readProbeRequest(
  page: Page,
  key: string,
): Promise<ProbeRequest | undefined> {
  return await page.evaluate((requestKey) => {
    const request = (window as ProbeWindow).__entryQueryLateProbe?.requests[
      requestKey
    ];
    if (!request) return undefined;
    return { ...request };
  }, key);
}

async function hasEvent(
  page: Page,
  name: string,
  key: string,
): Promise<boolean> {
  return await page.evaluate(
    ({ name, key }) =>
      (window as ProbeWindow).__entryQueryLateProbe?.events.some((event) =>
        event.name === name && event.key === key
      ) ?? false,
    { name, key },
  );
}

async function releaseAndObserve(
  page: Page,
  key: string,
  outcome: ProbeRequest["outcome"],
): Promise<void> {
  await page.evaluate((requestKey) => {
    (window as ProbeWindow).__entryQueryLateProbe!.release(requestKey);
  }, key);
  await page.waitForFunction(
    ({ key, outcome }) => {
      const request = (window as ProbeWindow).__entryQueryLateProbe
        ?.requests[key];
      return outcome === "deliver"
        ? request?.delivered === true
        : request?.rejected === true;
    },
    { key, outcome },
    { timeout: 30_000 },
  );
  await page.evaluate((requestKey) =>
    new Promise<void>((resolve) => {
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          const probe = (window as ProbeWindow).__entryQueryLateProbe!;
          const request = probe.requests[requestKey];
          request.uiCycleObserved = true;
          probe.record(
            request.outcome === "deliver"
              ? "old_fetch_delivered"
              : "old_rejection_settlement_observed",
            {
              key: requestKey,
              spaceId: request.spaceId,
              text: request.text,
            },
          );
          resolve();
        })
      );
    }), key);
}

async function recordUiEvent(
  page: Page,
  name: string,
  key: string,
): Promise<void> {
  const snapshot = await readUi(page);
  await page.evaluate(({ name, key, snapshot }) => {
    (window as ProbeWindow).__entryQueryLateProbe!.record(name, {
      key,
      details: snapshot,
    });
  }, { name, key, snapshot });
}

async function recordFinalAssertion(page: Page, key: string): Promise<void> {
  await page.evaluate((eventKey) => {
    const probe = (window as ProbeWindow).__entryQueryLateProbe!;
    const visibleEntryIds = Array.from(document.querySelectorAll(
      "tbody tr[data-entry-id]",
    )).flatMap((row) => row.getAttribute("data-entry-id") ?? []);
    probe.record("final_assertion", {
      key: eventKey,
      details: { visibleEntryIds },
    });
  }, key);
}

async function readVisibleHistory(
  page: Page,
  key: string,
): Promise<string[][]> {
  return await page.evaluate(
    (historyKey) =>
      (window as ProbeWindow).__entryQueryLateProbe
        ?.visibleHistory[historyKey] ??
        [],
    key,
  );
}

async function readEventNames(page: Page, key: string): Promise<string[]> {
  return await page.evaluate((eventKey) => {
    const events = (window as ProbeWindow).__entryQueryLateProbe?.events ?? [];
    return events.filter((event) => event.key === eventKey).map((event) =>
      event.name
    );
  }, key);
}

function isOrdered(observed: string[], names: string[]): boolean {
  let previous = -1;
  for (const name of names) {
    const index = observed.findIndex((eventName, eventIndex) =>
      eventName === name && eventIndex > previous
    );
    if (index < 0) return false;
    previous = index;
  }
  return true;
}

async function readProbeArtifact(page: Page): Promise<unknown> {
  return await page.evaluate(() => {
    const probe = (window as ProbeWindow).__entryQueryLateProbe;
    if (!probe) return null;
    const requests = Object.fromEntries(
      Object.entries(probe.requests).map(([key, request]) => [key, {
        key: request.key,
        spaceId: request.spaceId,
        text: request.text,
        outcome: request.outcome,
        held: request.held,
        internalStatus: request.internalStatus,
        originalSignalAborted: request.originalSignalAborted,
        internalSignalForwarded: request.internalSignalForwarded,
        delivered: request.delivered,
        rejected: request.rejected,
        uiCycleObserved: request.uiCycleObserved,
      }]),
    );
    return {
      events: probe.events,
      requests,
      visibleHistory: probe.visibleHistory,
    };
  });
}
