import {
  type APIRequestContext,
  expect,
  type Locator,
  type Page,
  test,
  type TestInfo,
} from "@playwright/test";
import { promises as fs } from "node:fs";
import {
  ensureDefaultForm,
  getBackendUrl,
  getDefaultSpaceId,
  getFrontendUrl,
  waitForServers,
} from "./lib/client.ts";

type CompactControlSeed = {
  spaceId: string;
  entryId: string;
  entryRevisionId: string;
  sqlId: string;
  compositionId: string;
  compositionRevisionId: string;
};

type Surface = {
  id: string;
  path: (seed: CompactControlSeed) => string;
  layout: "screen-head" | "composition-studio" | "entry-detail";
  backName: string;
};
type EvidenceResult = {
  source_sha: string;
  selector: string;
  surface: string;
  viewport: "390px" | "320px-effective@200%";
  locale: string;
  result: "passed";
  artifact: string;
  action_controls_count: number;
  evidence_gap: string;
};

const viewports = [
  { id: "390px", width: 390 },
  // Ugoite's existing responsive E2E convention uses a 320 CSS-pixel
  // viewport as the effective-width proxy for a 320px display at 200% zoom.
  { id: "320px-effective@200%", width: 320 },
] as const;

const surfaces: Surface[] = [
  {
    id: "sql-new",
    layout: "screen-head",
    backName: "Back to Saved SQL",
    path: (s) => `/spaces/${s.spaceId}/sql/new`,
  },
  {
    id: "sql-detail",
    layout: "screen-head",
    backName: "Back to Saved SQL",
    path: (s) => `/spaces/${s.spaceId}/sql/${s.sqlId}`,
  },
  {
    id: "sql-variables",
    layout: "screen-head",
    backName: "Back to Saved SQL",
    path: (s) => `/spaces/${s.spaceId}/sql/${s.sqlId}/variables`,
  },
  {
    id: "sql-run",
    layout: "screen-head",
    backName: "Back to Saved SQL",
    path: (s) => `/spaces/${s.spaceId}/sql/${s.sqlId}/run`,
  },
  {
    id: "composition-new",
    layout: "composition-studio",
    backName: "Saved tools",
    path: (s) => `/spaces/${s.spaceId}/compositions/new`,
  },
  {
    id: "composition-edit",
    layout: "composition-studio",
    backName: "Back to revision",
    path: (s) =>
      `/spaces/${s.spaceId}/compositions/${s.compositionId}/${s.compositionRevisionId}/edit`,
  },
  {
    id: "composition-history",
    layout: "screen-head",
    backName: "Back to saved tools",
    path: (s) => `/spaces/${s.spaceId}/compositions/${s.compositionId}/history`,
  },
  {
    id: "entry-detail",
    layout: "entry-detail",
    backName: "Back to Form",
    path: (s) => `/spaces/${s.spaceId}/entries/${s.entryId}`,
  },
  {
    id: "entry-info",
    layout: "screen-head",
    backName: "Back to Entry",
    path: (s) => `/spaces/${s.spaceId}/entries/${s.entryId}/info`,
  },
  {
    id: "entry-history",
    layout: "screen-head",
    backName: "Back to Entry",
    path: (s) => `/spaces/${s.spaceId}/entries/${s.entryId}/history`,
  },
  {
    id: "entry-revision",
    layout: "screen-head",
    backName: "Back to history",
    path: (s) =>
      `/spaces/${s.spaceId}/entries/${s.entryId}/history/${s.entryRevisionId}`,
  },
  {
    id: "storage-test-connection",
    layout: "screen-head",
    backName: "Back to Storage settings",
    path: (s) => `/spaces/${s.spaceId}/test-connection`,
  },
];

const evidenceGap =
  "Manual VoiceOver/NVDA reading-order and announcement evidence is unverified; automated accessible-name and keyboard-focus checks passed.";

let seed: CompactControlSeed;
let sqlCreated = false;
let entryCreated = false;
let compositionCreated = false;

function compositionYaml(name: string, sqlId: string, revisionId: string) {
  return [
    "format: ugoite.composition",
    "format_version: 1",
    `name: ${name}`,
    "kind: dashboard",
    "tags: []",
    "spec:",
    "  parameters:",
    "    - id: from_date",
    "      type: date",
    "      required: true",
    "      default: 2026-01-01",
    "  sources:",
    "    - id: compact_rows",
    "      kind: saved_sql",
    `      entry_id: \"${sqlId}\"`,
    `      revision_id: \"${revisionId}\"`,
    "      expected_result:",
    "        - name: total",
    "          type: float",
    "        - name: merchant",
    "          type: string",
    "      variables:",
    "        from_date:",
    "          parameter: from_date",
    "  components:",
    "    - id: total",
    "      kind: metric",
    "      source: compact_rows",
    "      value_field:",
    "        kind: sql_column",
    "        name: total",
    "    - id: rows",
    "      kind: table",
    "      source: compact_rows",
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
}

async function seedCompactControlSurfaces(
  request: APIRequestContext,
): Promise<CompactControlSeed> {
  await waitForServers(request);
  const spaceId = await getDefaultSpaceId(request);
  await ensureDefaultForm(request, spaceId);
  const suffix = crypto.randomUUID().slice(0, 8);

  const formName = `CompactControls${suffix}`;
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

  const entryResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/entries`),
    {
      data: {
        form: "Entry",
        fields: { Body: `Compact controls evidence ${suffix}` },
      },
    },
  );
  expect(entryResponse.status()).toBe(201);
  const entry = await entryResponse.json() as {
    id: string;
    revision_id: string;
  };
  entryCreated = true;
  const updateResponse = await request.put(
    getBackendUrl(`/spaces/${spaceId}/entries/${entry.id}`),
    {
      data: {
        form: "Entry",
        fields: { Body: `Compact controls evidence ${suffix} revised` },
        parent_revision_id: entry.revision_id,
      },
    },
  );
  expect(updateResponse.ok()).toBe(true);

  const compositionDataEntry = await request.post(
    getBackendUrl(`/spaces/${spaceId}/entries`),
    {
      data: {
        form: formName,
        fields: {
          Date: "2026-01-15",
          Merchant: "Compact Controls",
          Amount: 1.25,
        },
      },
    },
  );
  expect(compositionDataEntry.status()).toBe(201);

  const sqlResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/sql`),
    {
      data: {
        name: `Compact Controls ${suffix}`,
        kind: "user-query",
        sql:
          `SELECT \"${amountColumn}\" AS total, \"${merchantColumn}\" AS merchant FROM \"${relation}\" WHERE \"${dateColumn}\" >= $from_date ORDER BY \"${merchantColumn}\" ASC`,
        variables: [{
          type: "date",
          name: "from_date",
          description: "Earliest included date.",
        }],
      },
    },
  );
  expect([200, 201]).toContain(sqlResponse.status());
  const sql = await sqlResponse.json() as {
    id: string;
    revision_id: string;
  };
  sqlCreated = true;

  const compositionResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/compositions`),
    {
      headers: { "Idempotency-Key": crypto.randomUUID() },
      data: {
        yaml: compositionYaml(
          `Compact Controls ${suffix}`,
          sql.id,
          sql.revision_id,
        ),
      },
    },
  );
  expect([200, 201]).toContain(compositionResponse.status());
  const composition = await compositionResponse.json() as {
    composition_id: string;
    revision_id: string;
  };
  compositionCreated = true;

  return {
    spaceId,
    entryId: entry.id,
    entryRevisionId: entry.revision_id,
    sqlId: sql.id,
    compositionId: composition.composition_id,
    compositionRevisionId: composition.revision_id,
  };
}

async function focusBackLinkWithKeyboard(page: Page, backLink: Locator) {
  await page.evaluate(() => {
    const active = document.activeElement;
    if (active instanceof HTMLElement) active.blur();
  });

  for (let tab = 0; tab < 60; tab += 1) {
    await page.keyboard.press("Tab");
    if (
      await backLink.evaluate((element) => element === document.activeElement)
    ) {
      await expect(backLink).toBeFocused();
      const ring = await backLink.evaluate((element) => {
        const style = getComputedStyle(element);
        return {
          focusVisible: element.matches(":focus-visible"),
          outlineStyle: style.outlineStyle,
          outlineWidth: Number.parseFloat(style.outlineWidth),
        };
      });
      expect(ring.focusVisible).toBe(true);
      expect(ring.outlineStyle).not.toBe("none");
      expect(ring.outlineWidth).toBeGreaterThanOrEqual(2);
      return;
    }
  }

  throw new Error("BackLink was not reachable in the page's keyboard order");
}

function selectorsForSurface(surface: Surface) {
  if (surface.layout === "composition-studio") {
    return {
      header: "header:has(> a.back-link)",
      back: "header:has(> a.back-link) > a.back-link",
      title: "header input[aria-label]",
      actions: "header button, header a:not(.back-link)",
    };
  }
  if (surface.layout === "entry-detail") {
    return {
      header: ".ui-entry-header",
      back: ".ui-entry-header a.back-link",
      title: ".ui-entry-header h1",
      actions: ".ui-entry-action-bar button, .ui-entry-action-bar a",
    };
  }
  return {
    header: ".screenHead",
    back: ".screenHeadStart > a.back-link",
    title: ".screenHead h1",
    actions: ".screenHead button, .screenHead a:not(.back-link)",
  };
}

async function assertCompactHeader(
  page: Page,
  surface: Surface,
  width: number,
) {
  const selectors = selectorsForSurface(surface);
  const backLink = page.locator(selectors.back);
  await expect(page.locator("a.back-link")).toHaveCount(1);
  await expect(backLink).toHaveCount(1);
  await expect(backLink).toBeVisible();
  await expect(backLink).toHaveAccessibleName(surface.backName);

  const title = page.locator(selectors.title);
  await expect(title).toHaveCount(1);
  await expect(title).toBeVisible();

  const geometry = await page.evaluate((query) => {
    const header = document.querySelector<HTMLElement>(query.header);
    const back = document.querySelector<HTMLElement>(query.back);
    const heading = document.querySelector<HTMLElement>(query.title);
    if (!header || !back || !heading) throw new Error("Header is incomplete");
    const rect = (element: HTMLElement) => {
      const box = element.getBoundingClientRect();
      return {
        left: box.left,
        right: box.right,
        top: box.top,
        bottom: box.bottom,
        width: box.width,
        height: box.height,
      };
    };
    const controls = Array.from(
      document.querySelectorAll<HTMLElement>(query.actions),
    ).filter((element) => {
      const style = getComputedStyle(element);
      return style.display !== "none" && style.visibility !== "hidden" &&
        element.getClientRects().length > 0;
    });
    return {
      viewportWidth: window.innerWidth,
      documentWidth: document.documentElement.scrollWidth,
      header: rect(header),
      back: rect(back),
      title: rect(heading),
      backBeforeTitle: (() => {
        const backBox = back.getBoundingClientRect();
        const titleBox = heading.getBoundingClientRect();
        return backBox.right <= titleBox.left + 1 ||
          backBox.bottom <= titleBox.top + 1;
      })(),
      controls: controls.map((element) => ({
        rect: rect(element),
        accessibleName: element.getAttribute("aria-label") ||
          element.getAttribute("title") || element.innerText.trim(),
        centerReceivesPointer: (() => {
          const box = element.getBoundingClientRect();
          const target = document.elementFromPoint(
            box.left + box.width / 2,
            box.top + box.height / 2,
          );
          return target === element ||
            Boolean(target && element.contains(target));
        })(),
      })),
    };
  }, selectors);

  expect(geometry.viewportWidth).toBe(width);
  expect(geometry.documentWidth).toBeLessThanOrEqual(width + 1);
  expect(geometry.back.left).toBeGreaterThanOrEqual(-1);
  expect(geometry.back.right).toBeLessThanOrEqual(width + 1);
  expect(geometry.back.width).toBeGreaterThanOrEqual(44);
  expect(geometry.back.height).toBeGreaterThanOrEqual(44);
  expect(geometry.title.left).toBeGreaterThanOrEqual(-1);
  expect(geometry.title.right).toBeLessThanOrEqual(width + 1);
  expect(geometry.backBeforeTitle).toBe(true);
  for (const control of geometry.controls) {
    expect(control.accessibleName.trim().length).toBeGreaterThan(0);
    expect(control.rect.left).toBeGreaterThanOrEqual(-1);
    expect(control.rect.right).toBeLessThanOrEqual(width + 1);
    expect(control.centerReceivesPointer).toBe(true);
  }

  const actionControls = page.locator(selectors.actions);
  for (let index = 0; index < await actionControls.count(); index += 1) {
    const action = actionControls.nth(index);
    if (await action.isVisible()) {
      await expect(action).toHaveAccessibleName(/\S+/);
    }
  }

  await focusBackLinkWithKeyboard(page, backLink);
  return geometry.controls.length;
}

test.describe("compact BackLink header evidence", () => {
  test.beforeAll(async ({ request }) => {
    test.setTimeout(180_000);
    seed = await seedCompactControlSurfaces(request);
  });

  test.afterAll(async ({ request }) => {
    if (!seed) return;
    if (compositionCreated) {
      await request.delete(
        getBackendUrl(
          `/spaces/${seed.spaceId}/compositions/${seed.compositionId}`,
        ),
      );
    }
    if (sqlCreated) {
      await request.delete(
        getBackendUrl(`/spaces/${seed.spaceId}/sql/${seed.sqlId}`),
      );
    }
    if (entryCreated) {
      await request.delete(
        getBackendUrl(`/spaces/${seed.spaceId}/entries/${seed.entryId}`),
      );
    }
  });

  test(
    "REQ-UX-NAV-001: keeps nested compact headers visible and keyboard reachable",
    async ({ page }, testInfo: TestInfo) => {
      test.setTimeout(180_000);
      const sourceSha = process.env.UGOITE_SOURCE_SHA ??
        process.env.GITHUB_SHA ??
        "local-unreported";
      const evidencePath = testInfo.outputPath(
        "compact-controls-evidence.json",
      );
      const evidence: EvidenceResult[] = [];
      await fs.mkdir(testInfo.outputDir, { recursive: true });

      for (const viewport of viewports) {
        await page.setViewportSize({ width: viewport.width, height: 844 });
        for (const surface of surfaces) {
          await page.goto(getFrontendUrl(surface.path(seed)), {
            waitUntil: "domcontentloaded",
          });
          await expect(page.locator(selectorsForSurface(surface).title))
            .toBeVisible({
              timeout: 15_000,
            });
          const actionCount = await assertCompactHeader(
            page,
            surface,
            viewport.width,
          );
          const selectors = selectorsForSurface(surface);
          const artifact = `${surface.id}-${viewport.id}.png`;
          await page.screenshot({
            path: testInfo.outputPath(artifact),
            fullPage: false,
          });
          evidence.push({
            source_sha: sourceSha,
            selector: selectors.back,
            surface: surface.id,
            viewport: viewport.id,
            locale: await page.locator("html").getAttribute("lang") ??
              "unknown",
            result: "passed",
            artifact,
            action_controls_count: actionCount,
            evidence_gap: evidenceGap,
          });
          await fs.writeFile(
            evidencePath,
            `${
              JSON.stringify(
                {
                  source_sha: sourceSha,
                  results: evidence,
                },
                null,
                2,
              )
            }\n`,
          );
        }
      }

      await testInfo.attach("compact-controls-evidence", {
        path: evidencePath,
        contentType: "application/json",
      });
      expect(evidence).toHaveLength(surfaces.length * viewports.length);
    },
  );

  test(
    "REQ-UX-ACTION-001: keeps over-capacity icon-only action bars on one row",
    async ({ page }) => {
      for (const viewport of viewports) {
        await page.setViewportSize({ width: viewport.width, height: 844 });
        const entryDetail = surfaces.find((surface) =>
          surface.id === "entry-detail"
        );
        if (!entryDetail) throw new Error("Entry detail surface is missing");
        await page.goto(getFrontendUrl(entryDetail.path(seed)), {
          waitUntil: "domcontentloaded",
        });
        await expect(page.locator(".ui-entry-action-bar")).toBeVisible({
          timeout: 15_000,
        });

        await page.locator(".ui-entry-action-bar").evaluate((element) => {
          const bar = element as HTMLElement;
          const originalTools = Array.from(
            bar.querySelectorAll<HTMLElement>(".tool"),
          );
          const source = originalTools.find((tool) =>
            !(tool instanceof HTMLButtonElement) || !tool.disabled
          );
          if (!source) throw new Error("ActionIconBar has no enabled action");

          bar.classList.add("actionbar--icon-only");
          bar.setAttribute("aria-label", "Icon-only actions");
          const nameTool = (tool: HTMLElement, index: number) => {
            const name = `Toolbar action ${index + 1}`;
            tool.classList.add("tool--icon-only");
            tool.setAttribute("aria-label", name);
            tool.setAttribute("title", name);
            tool.removeAttribute("id");
            const label = tool.querySelector<HTMLElement>(".toolLabel");
            if (!label) throw new Error("ActionIconBar tool has no label");
            label.textContent = name;
            label.classList.add("ui-sr-only");
          };

          originalTools.forEach(nameTool);
          for (let index = originalTools.length; index < 12; index += 1) {
            const clone = source.cloneNode(true) as HTMLElement;
            clone.setAttribute("data-overflow-test-action", "true");
            nameTool(clone, index);
            bar.append(clone);
          }
        });

        const bar = page.locator(".ui-entry-action-bar");
        const layout = await bar.evaluate((element) => {
          const toolbar = element as HTMLElement;
          const controls = Array.from(
            toolbar.querySelectorAll<HTMLElement>(":scope > .tool"),
          );
          const boxes = controls.map((control) => {
            const rect = control.getBoundingClientRect();
            return {
              left: rect.left,
              right: rect.right,
              top: rect.top,
              width: rect.width,
              height: rect.height,
            };
          });
          return {
            display: getComputedStyle(toolbar).display,
            flexWrap: getComputedStyle(toolbar).flexWrap,
            overflowX: getComputedStyle(toolbar).overflowX,
            scrollbarWidth: getComputedStyle(toolbar).scrollbarWidth,
            clientWidth: toolbar.clientWidth,
            scrollWidth: toolbar.scrollWidth,
            documentWidth: document.documentElement.scrollWidth,
            viewportWidth: window.innerWidth,
            boxes,
          };
        });

        expect(layout.viewportWidth).toBe(viewport.width);
        expect(layout.display).toBe("flex");
        expect(layout.flexWrap).toBe("nowrap");
        expect(layout.overflowX).toBe("auto");
        expect(layout.scrollbarWidth).not.toBe("none");
        expect(layout.scrollWidth).toBeGreaterThan(layout.clientWidth);
        expect(layout.documentWidth).toBeLessThanOrEqual(viewport.width + 1);
        expect(layout.boxes).toHaveLength(12);
        const rowTop = layout.boxes[0].top;
        for (const box of layout.boxes) {
          expect(box.width).toBe(44);
          expect(box.height).toBe(44);
          expect(Math.abs(box.top - rowTop)).toBeLessThanOrEqual(1);
        }

        const lastAction = bar.locator(
          '[data-overflow-test-action="true"]',
        ).last();
        await page.evaluate(() => {
          const active = document.activeElement;
          if (active instanceof HTMLElement) active.blur();
        });
        for (let tab = 0; tab < 80; tab += 1) {
          await page.keyboard.press("Tab");
          if (
            await lastAction.evaluate((element) =>
              element === document.activeElement
            )
          ) break;
        }
        await expect(lastAction).toBeFocused();
        await expect(lastAction).toHaveAccessibleName("Toolbar action 12");

        const focused = await lastAction.evaluate((element) => {
          const control = element as HTMLElement;
          const bar = control.parentElement!;
          const rect = control.getBoundingClientRect();
          const barRect = bar.getBoundingClientRect();
          const style = getComputedStyle(control);
          return {
            visibleInsideToolbar: rect.left >= barRect.left - 1 &&
              rect.right <= barRect.right + 1,
            scrollLeft: bar.scrollLeft,
            focusVisible: control.matches(":focus-visible"),
            outlineStyle: style.outlineStyle,
            outlineWidth: Number.parseFloat(style.outlineWidth),
          };
        });
        expect(focused.visibleInsideToolbar).toBe(true);
        expect(focused.scrollLeft).toBeGreaterThan(0);
        expect(focused.focusVisible).toBe(true);
        expect(focused.outlineStyle).not.toBe("none");
        expect(focused.outlineWidth).toBeGreaterThanOrEqual(2);
      }
    },
  );
});
