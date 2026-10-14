import {
  type APIRequestContext,
  expect,
  type Page,
  test,
} from "@playwright/test";
import { getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client.ts";

type SeedState = {
  spaceId: string;
  compositionId: string;
  revisionId: string;
};

type HeldQueryKind = "metric" | "table";
type GenerationProbe = {
  held: Record<HeldQueryKind, number[]>;
  delivered: Record<HeldQueryKind, number[]>;
  release: (minimum: number) => void;
};
type ProbeWindow = Window & { __compositionGenerationProbe?: GenerationProbe };

type UiSnapshot = {
  metric: string | null;
  rows: string[];
  alerts: string[];
  loading: number;
};

function sqlIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
}

async function seedRapidVariableComposition(
  request: APIRequestContext,
): Promise<SeedState> {
  const suffix = `${Date.now()}-${crypto.randomUUID().slice(0, 8)}`;
  const spaceResponse = await request.post(getBackendUrl("/spaces"), {
    data: {
      slug: `composition-generation-${suffix}`,
      name: `Composition generation ${suffix}`,
    },
  });
  expect([200, 201]).toContain(spaceResponse.status());
  const space = await spaceResponse.json() as { space_uid?: string };
  expect(space.space_uid).toBeTruthy();
  const spaceId = space.space_uid!;

  const formName = `GenerationAmounts${crypto.randomUUID().slice(0, 8)}`;
  const formResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: {
        name: formName,
        version: 1,
        template: `# ${formName}\n\n## Amount\n`,
        fields: { Amount: { type: "double", required: true } },
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
  const amountColumn = form?.fields?.Amount?.sql_column;
  expect(relation).toBeTruthy();
  expect(amountColumn).toBeTruthy();

  for (const amount of [1, 2, 3]) {
    const entryResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      { data: { form: formName, fields: { Amount: amount } } },
    );
    expect(entryResponse.status()).toBe(201);
  }

  const sqlResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/sql`),
    {
      data: {
        name: `Generation aggregate ${suffix}`,
        kind: "user-query",
        sql: `SELECT SUM(${sqlIdentifier(amountColumn!)}) AS total FROM ${
          sqlIdentifier(relation!)
        } WHERE ${sqlIdentifier(amountColumn!)} >= $minimum`,
        variables: [{
          type: "float",
          name: "minimum",
          description: "Minimum amount included in the total.",
        }],
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
    `name: Latest generation ${suffix}`,
    "kind: dashboard",
    "tags: []",
    "spec:",
    "  variables:",
    "    - id: minimum",
    "      type: float",
    "      required: true",
    "      default: 0",
    "  sources:",
    "    - id: totals",
    "      kind: saved_sql",
    `      entry_id: "${savedSql.id}"`,
    `      revision_id: "${savedSql.revision_id}"`,
    "      expected_result:",
    "        - name: total",
    "          type: float",
    "      variables:",
    "        minimum:",
    "          variable: minimum",
    "  components:",
    "    - id: minimum_input",
    "      kind: input",
    "      variable: minimum",
    "      label: Minimum",
    "    - id: total",
    "      kind: metric",
    "      label: Total",
    "      source: totals",
    "      column: total",
    "    - id: totals_table",
    "      kind: table",
    "      source: totals",
    "  layout:",
    "    kind: flow",
    "    rows:",
    "      - id: controls",
    "        items:",
    "          - kind: component",
    "            component: minimum_input",
    "      - id: results",
    "        items:",
    "          - kind: component",
    "            component: total",
    "          - kind: component",
    "            component: totals_table",
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
  const savedComposition = await saveResponse.json() as {
    composition_id?: string;
    revision_id?: string;
  };
  expect(savedComposition.composition_id).toBeTruthy();
  expect(savedComposition.revision_id).toBeTruthy();

  return {
    spaceId,
    compositionId: savedComposition.composition_id!,
    revisionId: savedComposition.revision_id!,
  };
}

async function readUi(page: Page): Promise<UiSnapshot> {
  return await page.evaluate(() => ({
    metric: document.querySelector("output.compositionMetric")
      ?.textContent?.trim() ?? null,
    rows: Array.from(document.querySelectorAll("tbody tr td"))
      .map((cell) => cell.textContent?.trim() ?? ""),
    alerts: Array.from(document.querySelectorAll("[role='alert']"))
      .map((element) => element.textContent?.trim() ?? ""),
    loading: document.querySelectorAll("[role='status']").length,
  }));
}

async function readProbe(page: Page): Promise<{
  held: Record<HeldQueryKind, number[]>;
  delivered: Record<HeldQueryKind, number[]>;
}> {
  return await page.evaluate(() => {
    const probe = (window as ProbeWindow).__compositionGenerationProbe;
    return {
      held: {
        metric: [...(probe?.held.metric ?? [])],
        table: [...(probe?.held.table ?? [])],
      },
      delivered: {
        metric: [...(probe?.delivered.metric ?? [])],
        table: [...(probe?.delivered.table ?? [])],
      },
    };
  });
}

async function waitForValueInProbe(
  page: Page,
  field: "held" | "delivered",
  kind: HeldQueryKind,
  value: number,
): Promise<void> {
  await page.waitForFunction(
    ({ field, kind, value }) => {
      const probe = (window as ProbeWindow).__compositionGenerationProbe;
      return probe?.[field][kind].includes(value) ?? false;
    },
    { field, kind, value },
    { timeout: 30_000 },
  );
}

test.describe("Composition latest variable generation", () => {
  let seed: SeedState;

  test.beforeAll(async ({ request }) => {
    test.setTimeout(120_000);
    await waitForServers(request);
    seed = await seedRapidVariableComposition(request);
  });

  test("Rapid typed-variable edits keep only the latest Metric and Table result", async ({ page }) => {
    test.setTimeout(180_000);
    await page.addInitScript((spaceId) => {
      type Gate = { promise: Promise<void>; release: () => void };
      type Kind = "metric" | "table";
      type Probe = {
        held: Record<Kind, number[]>;
        delivered: Record<Kind, number[]>;
        release: (minimum: number) => void;
      };
      type InitWindow = Window & { __compositionGenerationProbe?: Probe };
      const gates = new Map<number, Gate>();
      for (const minimum of [1, 2, 3]) {
        let release!: () => void;
        const promise = new Promise<void>((resolve) => release = resolve);
        gates.set(minimum, { promise, release });
      }
      const probe: Probe = {
        held: { metric: [], table: [] },
        delivered: { metric: [], table: [] },
        release: (minimum) => gates.get(minimum)?.release(),
      };
      (window as InitWindow).__compositionGenerationProbe = probe;
      const nativeFetch = window.fetch.bind(window);
      window.fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
        const rawUrl = input instanceof Request ? input.url : String(input);
        let url: URL;
        try {
          url = new URL(rawUrl, window.location.href);
        } catch {
          return await nativeFetch(input, init);
        }
        const kind: Kind | undefined = url.pathname ===
            `/api/spaces/${spaceId}/compositions/metric-query`
          ? "metric"
          : url.pathname === `/api/spaces/${spaceId}/sql/query`
          ? "table"
          : undefined;
        if (!kind) return await nativeFetch(input, init);

        let bodyText = typeof init?.body === "string" ? init.body : "";
        if (!bodyText && input instanceof Request) {
          bodyText = await input.clone().text().catch(() => "");
        }
        let body: Record<string, unknown>;
        try {
          body = JSON.parse(bodyText) as Record<string, unknown>;
        } catch {
          return await nativeFetch(input, init);
        }
        const variables = body.variables as Record<string, unknown> | undefined;
        const parameters = body.parameters as
          | Record<string, unknown>
          | undefined;
        const minimum = Number(variables?.minimum ?? parameters?.minimum);
        const gate = gates.get(minimum);
        if (!gate) return await nativeFetch(input, init);

        // Ignore client cancellation so the test exercises the independent
        // generation guard after an older response has already returned.
        const response = input instanceof Request
          ? await nativeFetch(new Request(input, { signal: null }))
          : await nativeFetch(input, { ...init, signal: undefined });
        probe.held[kind].push(minimum);
        await gate.promise;
        probe.delivered[kind].push(minimum);
        return response;
      };
    }, seed.spaceId);

    await page.goto(
      getFrontendUrl(
        `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/${seed.revisionId}`,
      ),
      { waitUntil: "domcontentloaded" },
    );

    const minimumInput = page.getByLabel("Minimum", { exact: true });
    await expect(minimumInput).toHaveValue("0");
    await expect(page.locator("output.compositionMetric")).toHaveText("6");
    await expect(page.getByRole("table")).toContainText("6");
    for (const minimum of [1, 2, 3]) {
      await minimumInput.fill(String(minimum));
      await waitForValueInProbe(page, "held", "metric", minimum);
      await waitForValueInProbe(page, "held", "table", minimum);
    }

    const held = await readProbe(page);
    expect(held.held.metric).toEqual([1, 2, 3]);
    expect(held.held.table).toEqual([1, 2, 3]);

    await page.evaluate(() => {
      (window as ProbeWindow).__compositionGenerationProbe!.release(3);
    });
    await expect(page.locator("output.compositionMetric")).toHaveText("3");
    await expect(page.getByRole("table")).toContainText("3");
    const latestResult = await readUi(page);
    expect(latestResult.metric).toBe("3");
    expect(latestResult.rows).toContain("3");
    expect(latestResult.alerts).toEqual([]);

    for (const staleMinimum of [2, 1]) {
      await page.evaluate((minimum) => {
        (window as ProbeWindow).__compositionGenerationProbe!.release(minimum);
      }, staleMinimum);
      await waitForValueInProbe(page, "delivered", "metric", staleMinimum);
      await waitForValueInProbe(page, "delivered", "table", staleMinimum);
      await page.evaluate(() =>
        new Promise<void>((resolve) => {
          requestAnimationFrame(() =>
            requestAnimationFrame(() => setTimeout(resolve, 300))
          );
        })
      );
      expect(await readUi(page)).toEqual(latestResult);
    }

    const probe = await readProbe(page);
    expect(probe.delivered.metric).toEqual([3, 2, 1]);
    expect(probe.delivered.table).toEqual([3, 2, 1]);
    await expect(page.locator("output.compositionMetric")).toHaveText("3");
    await expect(page.getByRole("table")).toContainText("3");
    await expect(page.getByRole("table")).not.toContainText("5");
    await expect(page.getByRole("table")).not.toContainText("6");
  });
});
