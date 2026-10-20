import {
  type APIRequestContext,
  expect,
  type Page,
  test,
} from "@playwright/test";
import { getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client.ts";
import {
  completeCompositionGraphYaml,
  type CompositionGraphSeed,
} from "./lib/composition-graph.ts";

const FIRST_CUTOFF = "2026-03-10";
const SECOND_CUTOFF = "2026-03-11";
const FIRST_TOTAL = 15;
const SECOND_TOTAL = 8;

type SeedState = {
  spaceId: string;
  compositionId: string;
  firstRevisionId: string;
  secondRevisionId: string;
};

type RevisionProbe = {
  held: number;
  delivered: number;
  targets: string[];
  oldValues: unknown[];
  release: () => void;
};

type ProbeWindow = Window & { __compositionRevisionProbe?: RevisionProbe };

function sqlIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
}

async function createSpace(request: APIRequestContext): Promise<string> {
  const suffix = `${Date.now()}-${crypto.randomUUID().slice(0, 8)}`;
  const response = await request.post(getBackendUrl("/spaces"), {
    data: {
      slug: `composition-revision-race-${suffix}`,
      name: `Composition Revision Race ${suffix}`,
    },
  });
  expect([200, 201]).toContain(response.status());
  const result = await response.json() as { space_uid?: string };
  expect(result.space_uid).toBeTruthy();
  return result.space_uid!;
}

function compositionGraph(
  name: string,
  savedSqlId: string,
  savedSqlRevisionId: string,
  cutoff: string,
): CompositionGraphSeed {
  return {
    name,
    variables: [
      {
        id: "cutoff_date",
        kind: "input",
        value_type: { scalar_type: "date", nullable: false },
        required: true,
        default: { kind: "date", value: cutoff },
      },
      {
        id: "total_value",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "float", nullable: true },
        },
        query: {
          kind: "saved_sql",
          entry_id: savedSqlId,
          revision_id: savedSqlRevisionId,
          column: "total",
          arguments: [{ name: "start_date", variable: "cutoff_date" }],
          form_bindings: [],
          schema_fingerprint: "",
        },
      },
    ],
    components: [{ id: "total", kind: "metric", variable: "total_value" }],
    rows: [{
      id: "main",
      items: [{ kind: "component", component: "total" }],
    }],
  };
}

async function seedRevisionPair(
  request: APIRequestContext,
): Promise<SeedState> {
  const spaceId = await createSpace(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const formName = `RevisionRace${suffix}`;
  const compositionName = `Revision race ${suffix}`;

  const formResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: {
        name: formName,
        version: 1,
        template: `# ${formName}\n\n## Date\n\n## Amount\n`,
        fields: {
          Date: { type: "date", required: true },
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
  const amountColumn = form?.fields?.Amount?.sql_column;
  expect(relation).toBeTruthy();
  expect(dateColumn).toBeTruthy();
  expect(amountColumn).toBeTruthy();

  for (
    const entry of [
      { Date: FIRST_CUTOFF, Amount: 7 },
      { Date: SECOND_CUTOFF, Amount: 8 },
    ]
  ) {
    const response = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      { data: { form: formName, fields: entry } },
    );
    expect(response.status()).toBe(201);
  }

  const sqlResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/sql`),
    {
      data: {
        name: `Revision race total ${suffix}`,
        kind: "user-query",
        sql: `SELECT SUM(${sqlIdentifier(amountColumn!)}) AS total FROM ${
          sqlIdentifier(relation!)
        } WHERE ${sqlIdentifier(dateColumn!)} >= $start_date`,
        variables: [{
          type: "date",
          name: "start_date",
          description: "First included date.",
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

  const firstYaml = await completeCompositionGraphYaml(
    request,
    spaceId,
    compositionGraph(
      compositionName,
      savedSql.id!,
      savedSql.revision_id!,
      FIRST_CUTOFF,
    ),
  );
  const secondYaml = await completeCompositionGraphYaml(
    request,
    spaceId,
    compositionGraph(
      compositionName,
      savedSql.id!,
      savedSql.revision_id!,
      SECOND_CUTOFF,
    ),
  );

  const firstSaveResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/compositions`),
    {
      headers: { "Idempotency-Key": crypto.randomUUID() },
      data: { yaml: firstYaml },
    },
  );
  expect([200, 201]).toContain(firstSaveResponse.status());
  const firstSave = await firstSaveResponse.json() as {
    composition_id?: string;
    revision_id?: string;
  };
  expect(firstSave.composition_id).toBeTruthy();
  expect(firstSave.revision_id).toBeTruthy();

  const secondSaveResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/compositions`),
    {
      headers: { "Idempotency-Key": crypto.randomUUID() },
      data: {
        composition_id: firstSave.composition_id,
        base_revision_id: firstSave.revision_id,
        yaml: secondYaml,
      },
    },
  );
  expect(secondSaveResponse.status()).toBe(200);
  const secondSave = await secondSaveResponse.json() as {
    composition_id?: string;
    revision_id?: string;
  };
  expect(secondSave.composition_id).toBe(firstSave.composition_id);
  expect(secondSave.revision_id).toBeTruthy();
  expect(secondSave.revision_id).not.toBe(firstSave.revision_id);

  return {
    spaceId,
    compositionId: firstSave.composition_id!,
    firstRevisionId: firstSave.revision_id!,
    secondRevisionId: secondSave.revision_id!,
  };
}

async function readProbe(page: Page): Promise<
  {
    held: number;
    delivered: number;
    targets: string[];
    oldValues: unknown[];
  } | undefined
> {
  return await page.evaluate(() => {
    const probe = (window as ProbeWindow).__compositionRevisionProbe;
    return probe
      ? {
        held: probe.held,
        delivered: probe.delivered,
        targets: probe.targets,
        oldValues: probe.oldValues,
      }
      : undefined;
  });
}

test.describe("Composition revision stale response", () => {
  let seed: SeedState;

  test.beforeAll(async ({ request }) => {
    test.setTimeout(120_000);
    await waitForServers(request);
    seed = await seedRevisionPair(request);
  });

  test("discard a delayed metric response after navigating to another exact revision", async ({ page }) => {
    test.setTimeout(120_000);
    await page.addInitScript((config) => {
      type InitProbe = RevisionProbe;
      type InitWindow = Window & {
        __compositionRevisionProbe?: InitProbe;
      };
      const { spaceId, oldRevisionId } = config as {
        spaceId: string;
        oldRevisionId: string;
      };
      let releaseGate: (() => void) | undefined;
      const gate = new Promise<void>((resolve) => {
        releaseGate = resolve;
      });
      const probe: InitProbe = {
        held: 0,
        delivered: 0,
        targets: [],
        oldValues: [],
        release: () => releaseGate?.(),
      };
      (window as InitWindow).__compositionRevisionProbe = probe;
      const nativeFetch = window.fetch.bind(window);
      window.fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
        const rawUrl = input instanceof Request ? input.url : String(input);
        let url: URL;
        try {
          url = new URL(rawUrl, window.location.href);
        } catch {
          return await nativeFetch(input, init);
        }
        if (
          url.pathname !== `/api/spaces/${spaceId}/compositions/evaluate`
        ) return await nativeFetch(input, init);

        let bodyText = typeof init?.body === "string" ? init.body : "";
        if (!bodyText && input instanceof Request) {
          bodyText = await input.clone().text().catch(() => "");
        }
        let targetRevisionId: string | undefined;
        try {
          const body = JSON.parse(bodyText) as {
            target?: { kind?: string; revision_id?: string };
          };
          if (body.target?.kind === "saved_revision") {
            targetRevisionId = body.target.revision_id;
          }
        } catch {
          return await nativeFetch(input, init);
        }
        if (targetRevisionId !== oldRevisionId) {
          return await nativeFetch(input, init);
        }

        probe.held += 1;
        probe.targets.push(targetRevisionId);
        const response = input instanceof Request
          ? await nativeFetch(new Request(input, { signal: null }))
          : await nativeFetch(input, { ...init, signal: undefined });
        const payload = await response.clone().json().catch(() => undefined) as
          | {
            query_results?: {
              total_value?: { value?: { kind?: string; value?: unknown } };
            };
          }
          | undefined;
        probe.oldValues.push(
          payload?.query_results?.total_value?.value?.value,
        );
        await gate;
        probe.delivered += 1;
        return response;
      };
    }, { spaceId: seed.spaceId, oldRevisionId: seed.firstRevisionId });

    const firstPath =
      `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/${seed.firstRevisionId}`;
    const secondPath =
      `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/${seed.secondRevisionId}`;
    await page.goto(getFrontendUrl(firstPath), {
      waitUntil: "domcontentloaded",
    });
    const historyPath =
      `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/history`;
    const historyLink = page.locator(`a[href="${historyPath}"]`);
    await expect(historyLink).toBeVisible();
    await page.waitForFunction(
      (oldRevisionId) => {
        const probe = (window as ProbeWindow).__compositionRevisionProbe;
        return probe?.held === 1 && probe.targets[0] === oldRevisionId;
      },
      seed.firstRevisionId,
      { timeout: 30_000 },
    );

    await historyLink.click();
    await expect(page).toHaveURL(
      new RegExp(
        `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/history$`,
      ),
    );
    const secondRevisionLink = page.locator(`a[href="${secondPath}"]`);
    await expect(secondRevisionLink).toBeVisible();
    await secondRevisionLink.click();
    await expect(page).toHaveURL(new RegExp(`${secondPath}$`));
    await expect(page.locator(".compositionGraphMetric strong")).toHaveText(
      String(SECOND_TOTAL),
      { timeout: 15_000 },
    );
    await expect(page.locator("[role='alert']")).toHaveCount(0);
    await expect(page.locator("[role='status']")).toHaveCount(0);
    const currentBeforeRelease = {
      url: new URL(page.url()).pathname,
      metric: await page.locator(".compositionGraphMetric strong")
        .textContent(),
      alerts: await page.locator("[role='alert']").allTextContents(),
      loading: await page.locator("[role='status']").count(),
    };
    expect(currentBeforeRelease.url).toBe(secondPath);
    expect(currentBeforeRelease.metric?.trim()).toBe(String(SECOND_TOTAL));

    await page.evaluate(() => {
      (window as ProbeWindow).__compositionRevisionProbe!.release();
    });
    await page.waitForFunction(
      () => {
        const probe = (window as ProbeWindow).__compositionRevisionProbe;
        return probe?.held === 1 && probe.delivered === probe.held;
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

    const currentAfterRelease = {
      url: new URL(page.url()).pathname,
      metric: await page.locator(".compositionGraphMetric strong")
        .textContent(),
      alerts: await page.locator("[role='alert']").allTextContents(),
      loading: await page.locator("[role='status']").count(),
    };
    expect(currentAfterRelease).toEqual(currentBeforeRelease);
    expect(currentAfterRelease.metric?.trim()).toBe(String(SECOND_TOTAL));
    const probe = await readProbe(page);
    expect(probe?.targets).toEqual([seed.firstRevisionId]);
    expect(probe?.oldValues).toEqual([FIRST_TOTAL]);
  });
});
