import {
  type APIRequestContext,
  expect,
  type Page,
  test,
} from "@playwright/test";
import { getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client.ts";

const PARAM_A_START = "2026-03-10";
const PARAM_A_END = "2026-03-11";
const PARAM_B_START = "2026-03-11";
const PARAM_B_END = "2026-03-12";
const STALE_MERCHANT = "Stale-A";
const CURRENT_MERCHANT = "Current-B";
const CURRENT_AMOUNT = 8;

type SeedState = {
  spaceId: string;
  compositionId: string;
  revisionId: string;
};

type UiSnapshot = {
  rows: string[];
  metric: string | null;
  alerts: string[];
  loading: number;
  previousDisabled: boolean | null;
  nextDisabled: boolean | null;
  pageIdentity: string | null;
};

type StaleProbe = {
  held: number;
  delivered: number;
  release: () => void;
};

type ProbeWindow = Window & { __compositionStaleProbe?: StaleProbe };

function sqlIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
}

async function createSpace(request: APIRequestContext): Promise<string> {
  const suffix = `${Date.now()}-${crypto.randomUUID().slice(0, 8)}`;
  const response = await request.post(getBackendUrl("/spaces"), {
    data: {
      slug: `composition-stale-${suffix}`,
      name: `Composition Stale ${suffix}`,
    },
  });
  expect([200, 201]).toContain(response.status());
  const result = await response.json() as { space_uid?: string };
  expect(result.space_uid).toBeTruthy();
  return result.space_uid!;
}

async function seedStaleComposition(
  request: APIRequestContext,
): Promise<SeedState> {
  const spaceId = await createSpace(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const formName = `StaleWindow${suffix}`;

  const formResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: {
        name: formName,
        version: 1,
        template: `# ${formName}\n\n## Date\n\n## Merchant\n\n## Amount\n`,
        fields: {
          Date: { type: "date", required: true },
          Merchant: { type: "string", required: true },
          Amount: { type: "double", required: true },
        },
      },
    },
  );
  expect([200, 201]).toContain(formResponse.status());

  const formsResponse = await request.get(
    getBackendUrl(`/spaces/${spaceId}/forms`),
  );
  expect(formsResponse.ok()).toBe(true);
  const forms = await formsResponse.json() as Array<{
    name?: string;
    sql_relation?: string;
    fields?: Record<string, { sql_column?: string }>;
  }>;
  const form = forms.find((candidate) => candidate.name === formName);
  const relation = form?.sql_relation;
  const dateColumn = form?.fields?.Date?.sql_column;
  const merchantColumn = form?.fields?.Merchant?.sql_column;
  const amountColumn = form?.fields?.Amount?.sql_column;
  expect(relation).toBeTruthy();
  expect(dateColumn).toBeTruthy();
  expect(merchantColumn).toBeTruthy();
  expect(amountColumn).toBeTruthy();

  for (
    const row of [
      { Date: PARAM_A_START, Merchant: STALE_MERCHANT, Amount: 7 },
      {
        Date: PARAM_B_START,
        Merchant: CURRENT_MERCHANT,
        Amount: CURRENT_AMOUNT,
      },
    ]
  ) {
    const entryResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      { data: { form: formName, fields: row } },
    );
    expect(entryResponse.status()).toBe(201);
  }

  const sqlResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/sql`),
    {
      data: {
        name: `Stale window query ${suffix}`,
        kind: "user-query",
        sql: `SELECT ${sqlIdentifier(amountColumn!)} AS total, ${
          sqlIdentifier(merchantColumn!)
        } AS merchant FROM ${sqlIdentifier(relation!)} WHERE ${
          sqlIdentifier(dateColumn!)
        } >= $month_start AND ${
          sqlIdentifier(dateColumn!)
        } < $month_end ORDER BY ${sqlIdentifier(merchantColumn!)} ASC`,
        variables: [
          {
            type: "date",
            name: "month_start",
            description: "First date included in this window.",
          },
          {
            type: "date",
            name: "month_end",
            description: "First date excluded from this window.",
          },
        ],
      },
    },
  );
  expect([200, 201]).toContain(sqlResponse.status());
  const savedSql = await sqlResponse.json() as {
    id?: string;
    revision_id?: string;
  };
  expect(savedSql.id).toBeTruthy();
  expect(savedSql.revision_id).toBeTruthy();

  const yaml = [
    "format: ugoite.composition",
    "format_version: 1",
    `name: Stale window ${suffix}`,
    "kind: dashboard",
    "tags: []",
    "spec:",
    "  parameters:",
    "    - id: month_start",
    "      type: date",
    "      required: true",
    "    - id: month_end",
    "      type: date",
    "      required: true",
    "  sources:",
    "    - id: window_rows",
    "      kind: saved_sql",
    `      entry_id: "${savedSql.id}"`,
    `      revision_id: "${savedSql.revision_id}"`,
    "      expected_result:",
    "        - name: total",
    "          type: float",
    "        - name: merchant",
    "          type: string",
    "      variables:",
    "        month_start:",
    "          parameter: month_start",
    "        month_end:",
    "          parameter: month_end",
    "  components:",
    "    - id: total",
    "      kind: metric",
    "      source: window_rows",
    "      value_field:",
    "        kind: sql_column",
    "        name: total",
    "    - id: rows",
    "      kind: table",
    "      source: window_rows",
    "  layout:",
    "    kind: flow",
    "    rows:",
    "      - id: main",
    "        items:",
    "          - kind: component",
    "            component: total",
    "          - kind: component",
    "            component: rows",
    "",
  ].join("\n");
  const saveResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/compositions`),
    {
      headers: { "Idempotency-Key": crypto.randomUUID() },
      data: { yaml },
    },
  );
  expect([200, 201]).toContain(saveResponse.status());
  const saved = await saveResponse.json() as {
    composition_id?: string;
    revision_id?: string;
  };
  expect(saved.composition_id).toBeTruthy();
  expect(saved.revision_id).toBeTruthy();

  return {
    spaceId,
    compositionId: saved.composition_id!,
    revisionId: saved.revision_id!,
  };
}

async function readUi(page: Page): Promise<UiSnapshot> {
  return await page.evaluate(() => {
    const cells = Array.from(
      document.querySelectorAll("tbody tr td"),
    ).map((cell) => cell.textContent?.trim() ?? "");
    const pagination = document.querySelector(".result-pagination");
    const buttons = Array.from(pagination?.querySelectorAll("button") ?? []);
    return {
      rows: cells,
      metric: document.querySelector("output.compositionMetric")
        ?.textContent?.trim() ?? null,
      alerts: Array.from(document.querySelectorAll("[role='alert']")).map(
        (element) => element.textContent?.trim() ?? "",
      ),
      loading: document.querySelectorAll("[role='status']").length,
      previousDisabled: buttons[0]?.hasAttribute("disabled") ?? null,
      nextDisabled: buttons[1]?.hasAttribute("disabled") ?? null,
      pageIdentity: document.querySelector(
        "[data-page-identity]",
      )?.getAttribute("data-page-identity") ?? null,
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

async function readProbe(page: Page): Promise<StaleProbe | undefined> {
  return await page.evaluate(() => {
    const probe = (window as ProbeWindow).__compositionStaleProbe;
    if (!probe) return undefined;
    return { held: probe.held, delivered: probe.delivered };
  }) as StaleProbe | undefined;
}

test.describe("Composition Stale Response", () => {
  let seed: SeedState;

  test.beforeAll(async ({ request }) => {
    test.setTimeout(120_000);
    await waitForServers(request);
    seed = await seedStaleComposition(request);
  });

  test("Late parameter-A responses change neither rows, metric, error, loading, finalization, nor pagination", async ({ page }) => {
    test.setTimeout(180_000);
    // Hold the parameter-A source-query response. The resolve leg always
    // passes: the route unmounts its parameter inputs while resolving, so a
    // held resolve would make switching to parameter B impossible. The held
    // query still reaches a stale requestSource continuation guarded by the
    // same generation rule that guards Space/source/revision change.
    await page.addInitScript((config) => {
      type InitProbe = StaleProbe & { events: string[] };
      type InitWindow = Window & { __compositionStaleProbe?: InitProbe };
      const { holdValue, spaceId } = config as {
        holdValue: string;
        spaceId: string;
      };
      let releaseGate: (() => void) | undefined;
      const gate = new Promise<void>((resolve) => {
        releaseGate = resolve;
      });
      const probe: InitProbe = {
        held: 0,
        delivered: 0,
        events: [],
        release: () => releaseGate?.(),
      };
      (window as InitWindow).__compositionStaleProbe = probe;
      const nativeFetch = window.fetch.bind(window);
      window.fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
        const rawUrl = input instanceof Request ? input.url : String(input);
        let url: URL;
        try {
          url = new URL(rawUrl, window.location.href);
        } catch {
          return await nativeFetch(input, init);
        }
        const isQuery = url.pathname === `/api/spaces/${spaceId}/sql/query`;
        if (!isQuery) return await nativeFetch(input, init);
        let bodyText = typeof init?.body === "string" ? init.body : "";
        if (!bodyText && input instanceof Request) {
          bodyText = await input.clone().text().catch(() => "");
        }
        if (!bodyText.includes(holdValue)) {
          return await nativeFetch(input, init);
        }
        probe.held += 1;
        const response = input instanceof Request
          ? await nativeFetch(new Request(input, { signal: null }))
          : await nativeFetch(input, { ...init, signal: undefined });
        await gate;
        probe.delivered += 1;
        return response;
      };
    }, {
      holdValue: PARAM_A_START,
      spaceId: seed.spaceId,
    });

    await page.goto(
      getFrontendUrl(
        `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/${seed.revisionId}`,
      ),
      { waitUntil: "domcontentloaded" },
    );

    await page.getByLabel("month_start").fill(PARAM_A_START);
    await page.getByLabel("month_end").fill(PARAM_A_END);
    await page.waitForFunction(
      () => {
        const probe = (window as ProbeWindow).__compositionStaleProbe;
        return (probe?.held ?? 0) >= 1;
      },
      undefined,
      { timeout: 30_000 },
    );

    await page.getByLabel("month_start").fill(PARAM_B_START);
    await page.getByLabel("month_end").fill(PARAM_B_END);
    await expectUi(page, {
      metric: String(CURRENT_AMOUNT),
      alerts: [],
      loading: 0,
      previousDisabled: null,
      nextDisabled: null,
    });
    await expect.poll(() => readUi(page), { timeout: 30_000 })
      .toMatchObject({ rows: expect.arrayContaining([CURRENT_MERCHANT]) });
    const beforeRelease = await readUi(page);
    expect(beforeRelease.rows).toContain(CURRENT_MERCHANT);
    expect(beforeRelease.rows).not.toContain(STALE_MERCHANT);

    await page.evaluate(() => {
      (window as ProbeWindow).__compositionStaleProbe!.release();
    });
    await page.waitForFunction(
      () => {
        const probe = (window as ProbeWindow).__compositionStaleProbe;
        return (probe?.held ?? 0) >= 1 &&
          (probe?.delivered ?? 0) >= (probe?.held ?? 0);
      },
      undefined,
      { timeout: 30_000 },
    );
    await page.evaluate(() =>
      new Promise<void>((resolve) => {
        requestAnimationFrame(() =>
          requestAnimationFrame(() => setTimeout(resolve, 750))
        );
      })
    );
    const afterRelease = await readUi(page);
    expect(afterRelease).toEqual(beforeRelease);
    const probeState = await readProbe(page);
    expect(probeState?.held).toBeGreaterThanOrEqual(1);
    expect(probeState?.delivered).toBe(probeState?.held);

    await expect(page.locator("output.compositionMetric")).toHaveText(
      String(CURRENT_AMOUNT),
    );
    const table = page.getByRole("table");
    await expect(table).toContainText(CURRENT_MERCHANT);
    await expect(table).not.toContainText(STALE_MERCHANT);
  });
});
