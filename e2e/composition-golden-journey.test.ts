import {
  type APIRequestContext,
  type Browser,
  type BrowserContext,
  expect,
  type Page,
  test,
} from "@playwright/test";
import { readFile } from "node:fs/promises";
import { getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client.ts";

const SEED_MANIFEST_URL = new URL(
  "./fixtures/composition/space-seed/manifest.json",
  import.meta.url,
);

type SeedManifest = {
  model_connection: "disabled";
  composition_name: string;
  expected: { composition_publications: number };
};

type CompositionSaveResponse = {
  composition_id: string;
  revision_id: string;
  canonical_yaml: string;
  receipt: { committed_revision_ids: string[] };
};

type CompositionHistoryPage = {
  revisions: Array<{ revision: { revision_id: string } }>;
  total: number;
};

type CompositionRawRevision = {
  revision: { entry_id: string; revision_id: string };
  fields: Record<string, unknown>;
};

async function readSeedManifest(): Promise<SeedManifest> {
  return JSON.parse(
    await readFile(SEED_MANIFEST_URL, "utf8"),
  ) as SeedManifest;
}

async function createSpace(request: APIRequestContext): Promise<string> {
  const suffix = `${Date.now()}-${crypto.randomUUID().slice(0, 8)}`;
  const response = await request.post(getBackendUrl("/spaces"), {
    data: {
      slug: `composition-golden-${suffix}`,
      name: `Composition Golden ${suffix}`,
    },
  });
  expect([200, 201]).toContain(response.status());
  const result = await response.json() as { space_uid?: string };
  expect(result.space_uid).toBeTruthy();
  return result.space_uid!;
}

async function openOwnerContext(
  browser: Browser,
  storageState: Awaited<ReturnType<BrowserContext["storageState"]>>,
) {
  return await browser.newContext({ storageState });
}

function savePath(spaceId: string): string {
  return new URL(
    getBackendUrl(`/spaces/${spaceId}/compositions`),
  ).pathname;
}

function compositionPath(
  spaceId: string,
  compositionId: string,
  revisionId: string,
): string {
  return `/spaces/${spaceId}/compositions/${compositionId}/${revisionId}`;
}

async function openSavedCompositionFromHome(
  page: Page,
  spaceId: string,
  compositionId: string,
  revisionId: string,
  compositionName: string,
): Promise<string> {
  const path = compositionPath(spaceId, compositionId, revisionId);
  await page.goto(getFrontendUrl(`/spaces/${spaceId}/dashboard`), {
    waitUntil: "domcontentloaded",
  });
  const savedTools = page.getByRole("list", { name: "Saved tools" });
  const link = savedTools.getByRole("link", { name: compositionName });
  await expect(link).toHaveAttribute("href", path);
  await link.click();
  await expect(page).toHaveURL(new RegExp(`${path.replaceAll("/", "\\/")}$`));
  await expect(
    page.getByRole("heading", { level: 1, name: compositionName }),
  ).toBeVisible();
  return path;
}

test.describe("Composition Golden Journey", () => {
  let spaceId: string;
  let manifest: SeedManifest;

  test.beforeAll(async ({ request }) => {
    await waitForServers(request);
    manifest = await readSeedManifest();
    expect(manifest.model_connection).toBe("disabled");
    expect(manifest.expected.composition_publications).toBe(1);
    spaceId = await createSpace(request);
  });

  test("Browser saves a Text tool once, reopens its exact revision from Home, and exposes a read-only reader", async ({ browser, context, request }) => {
    test.setTimeout(120_000);
    const storageState = await context.storageState();
    const editorContext = await openOwnerContext(browser, storageState);
    let compositionId = "";
    let revisionId = "";
    let committedSave: CompositionSaveResponse | undefined;
    let replayedSave: CompositionSaveResponse | undefined;
    const idempotencyKeys: string[] = [];
    const requestBodies: unknown[] = [];
    try {
      const editorPage = await editorContext.newPage();
      await editorPage.goto(
        getFrontendUrl(`/spaces/${spaceId}/compositions/new`),
        { waitUntil: "domcontentloaded" },
      );
      const nameInput = editorPage.getByLabel("Name", { exact: true });
      await expect(nameInput).toHaveValue("");
      await nameInput.fill(manifest.composition_name);

      const palette = editorPage.getByRole("group", {
        name: "Add to design",
        exact: true,
      });
      await palette.getByRole("button", { name: "Text", exact: true }).click();
      const textEditor = editorPage.getByRole("textbox", {
        name: "Text",
        exact: true,
      });
      await expect(textEditor).toBeFocused();
      await textEditor.fill("Monthly expense summary");
      await expect(textEditor).toHaveValue("Monthly expense summary");

      const saveUrl = savePath(spaceId);
      await editorPage.route("**/api/spaces/*/compositions", async (route) => {
        const routed = new URL(route.request().url());
        if (
          route.request().method() !== "POST" ||
          routed.pathname !== saveUrl
        ) {
          await route.continue();
          return;
        }

        idempotencyKeys.push(
          route.request().headers()["idempotency-key"] ?? "",
        );
        requestBodies.push(route.request().postDataJSON());
        const response = await route.fetch();
        expect(response.ok()).toBe(true);
        const responseBody = await response.body();
        const saved = JSON.parse(
          new TextDecoder().decode(responseBody),
        ) as CompositionSaveResponse;
        if (idempotencyKeys.length === 1) {
          committedSave = saved;
          // The server committed the revision before the Browser loses this
          // response. Retry must reuse the exact request and idempotency key.
          await route.fulfill({
            status: 502,
            contentType: "application/json",
            body: JSON.stringify({
              code: "SAVE_RESPONSE_DELAYED",
              message: "The save response was interrupted after commit.",
            }),
          });
          return;
        }
        replayedSave = saved;
        await route.fulfill({
          status: response.status(),
          headers: response.headers(),
          body: responseBody,
        });
      });

      await editorPage.getByRole("button", { name: "Save", exact: true })
        .click();
      await expect(
        editorPage.getByText("Could not save this tool.", { exact: true }),
      ).toBeVisible({ timeout: 60_000 });

      const replayResponsePromise = editorPage.waitForResponse((response) => {
        const url = new URL(response.url());
        return response.request().method() === "POST" &&
          url.pathname === saveUrl && response.status() >= 200 &&
          response.status() < 300;
      });
      await editorPage.getByRole("button", { name: "Retry", exact: true })
        .click();
      const replayResponse = await replayResponsePromise;
      replayedSave = await replayResponse.json() as CompositionSaveResponse;

      expect(idempotencyKeys).toHaveLength(2);
      expect(idempotencyKeys[0]).toBeTruthy();
      expect(idempotencyKeys[1]).toBe(idempotencyKeys[0]);
      expect(requestBodies[1]).toEqual(requestBodies[0]);
      expect((requestBodies[0] as { yaml?: string }).yaml).toContain(
        "Monthly expense summary",
      );
      expect((requestBodies[0] as { yaml?: string }).yaml).not.toMatch(
        /saved_sql|metric|presentation/,
      );
      expect(committedSave).toBeTruthy();
      expect(replayedSave).toBeTruthy();
      expect(replayedSave).toEqual(committedSave);

      compositionId = replayedSave!.composition_id;
      revisionId = replayedSave!.revision_id;
      expect(replayedSave!.receipt.committed_revision_ids).toContain(
        revisionId,
      );
      const exactPath = compositionPath(spaceId, compositionId, revisionId);
      await expect(editorPage).toHaveURL(
        new RegExp(`${exactPath.replaceAll("/", "\\/")}$`),
        { timeout: 30_000 },
      );

      const historyResponse = await request.get(
        getBackendUrl(
          `/spaces/${spaceId}/compositions/${compositionId}/history?limit=100&offset=0`,
        ),
      );
      expect(historyResponse.ok()).toBe(true);
      const history = await historyResponse.json() as CompositionHistoryPage;
      expect(history.revisions).toHaveLength(
        manifest.expected.composition_publications,
      );
      expect(history.total).toBe(manifest.expected.composition_publications);
      expect(history.revisions[0]?.revision.revision_id).toBe(revisionId);

      const rawRevisionResponse = await request.get(
        getBackendUrl(
          `/spaces/${spaceId}/compositions/${compositionId}/history/${revisionId}`,
        ),
      );
      expect(rawRevisionResponse.ok()).toBe(true);
      const rawRevision = await rawRevisionResponse
        .json() as CompositionRawRevision;
      expect(rawRevision.revision.entry_id).toBe(compositionId);
      expect(rawRevision.revision.revision_id).toBe(revisionId);
      const rawSpec = rawRevision.fields["spec"];
      expect(typeof rawSpec).toBe("string");
      expect(rawSpec).toContain("Monthly expense summary");
    } finally {
      await editorContext.close();
    }

    // Reopen in a fresh Browser context to prove saved Knowledge, not editor
    // state, supplies the visible Text value.
    const readerContext = await openOwnerContext(browser, storageState);
    try {
      const readerPage = await readerContext.newPage();
      const exactPath = await openSavedCompositionFromHome(
        readerPage,
        spaceId,
        compositionId,
        revisionId,
        manifest.composition_name,
      );
      await expect(readerPage).toHaveURL(
        new RegExp(`${exactPath.replaceAll("/", "\\/")}$`),
      );
      await expect(
        readerPage.locator(".compositionGraphText"),
      ).toHaveText("Monthly expense summary");
      await expect(
        readerPage.getByRole("tablist", { name: "Studio mode" }),
      ).toHaveCount(0);
      await expect(
        readerPage.getByRole("group", { name: "Add to design" }),
      ).toHaveCount(0);
      await expect(
        readerPage.getByRole("textbox", { name: "Text", exact: true }),
      ).toHaveCount(0);
    } finally {
      await readerContext.close();
    }
  });
});
