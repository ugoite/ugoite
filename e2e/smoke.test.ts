/**
 * Smoke Tests for Ugoite
 *
 * These tests verify that the basic infrastructure is working:
 * - Frontend serves pages
 * - API endpoints respond correctly
 */

import { expect, test } from "@playwright/test";
import {
  ensureDefaultForm,
  getBackendUrl,
  getDefaultSpaceId,
  waitForServers,
} from "./lib/client.ts";

test.describe("Smoke Tests", { tag: "@smoke" }, () => {
  let spaceId = "";

  test.beforeAll(async ({ request }) => {
    await waitForServers(request);
    spaceId = await getDefaultSpaceId(request);
    await ensureDefaultForm(request, spaceId);
  });

  test("GET / returns HTML with DOCTYPE", async ({ page }) => {
    await page.goto("/");
    await page.waitForLoadState("networkidle");
    const body = await page.content();
    expect(body.toLowerCase()).toContain("<!doctype html>");
  });

  test("GET / has correct content-type", async ({ page }) => {
    const response = await page.goto("/");
    expect(response).not.toBeNull();
    const contentType = response?.headers()["content-type"] ?? "";
    expect(contentType).toContain("text/html");
  });

  test("GET /spaces returns HTML", async ({ page }) => {
    await page.goto("/spaces");
    await page.waitForLoadState("networkidle");
    const body = await page.content();
    expect(body.toLowerCase()).toContain("<!doctype html>");
  });

  test("GET /spaces/:space_id/entries/:id returns HTML", async ({ page, request }) => {
    const createRes = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      {
        data: {
          markdown:
            `---\nform: Entry\n---\n# E2E Detail Route Entry\n\n## Body\nCreated at ${
              new Date().toISOString()
            }`,
        },
      },
    );
    expect(createRes.status()).toBe(201);
    const created = (await createRes.json()) as { id: string };

    await page.goto(`/spaces/${spaceId}/entries/${created.id}`);
    await page.waitForLoadState("networkidle");
    const body = await page.content();
    expect(body.toLowerCase()).toContain("<!doctype html>");
    await expect(page.getByRole("heading", { name: "E2E Detail Route Entry" }))
      // Opening a freshly published Iceberg table can require one cold metadata
      // read. Keep this UI assertion within the test's 60-second budget rather
      // than Playwright's unrelated five-second matcher default.
      .toBeVisible({ timeout: 50_000 });
    await expect(page.getByRole("link", { name: "Back to Form" }))
      .toHaveAttribute(
        "href",
        `/spaces/${spaceId}/forms?form=Entry`,
      );

    await request.delete(
      getBackendUrl(`/spaces/${spaceId}/entries/${created.id}`),
    );
  });

  test(
    "plain Entries list keeps the Entries index in the Forms shell",
    async ({ page, request }) => {
      await page.goto(`/spaces/${spaceId}/entries`);
      await expect(page).toHaveURL(`/spaces/${spaceId}/entries`);
      await expect(page.getByRole("tab")).toHaveCount(0);
      const formsLinks = page.getByRole("link", { name: "Forms", exact: true });
      await expect(formsLinks).toHaveCount(1);
      await expect(formsLinks.first()).toHaveAttribute(
        "href",
        `/spaces/${spaceId}/forms`,
      );
      await expect(formsLinks.first()).toHaveAttribute("aria-current", "page");
    },
  );

  test("GET /about returns HTML", async ({ page }) => {
    await page.goto("/about");
    await page.waitForLoadState("networkidle");
    const body = await page.content();
    expect(body.toLowerCase()).toContain("<!doctype html>");
  });

  test("REQ-OPS-015: Passkey login produces only an opaque HttpOnly session cookie", async ({ page, context }) => {
    await page.goto("/spaces");
    await expect(page.getByText("Available Spaces")).toBeVisible();
    const cookies = await context.cookies();
    const session = cookies.find((cookie) => cookie.name === "ugoite_session");
    expect(session).toBeDefined();
    expect(session?.httpOnly).toBe(true);
    expect(await page.evaluate(() => document.cookie)).not.toContain(
      "ugoite_session=",
    );
    expect(cookies.some((cookie) => cookie.name.includes("bearer"))).toBe(
      false,
    );
  });

  test("GET /spaces returns list", async ({ request }) => {
    const res = await request.get(getBackendUrl("/spaces"));
    expect(res.ok()).toBeTruthy();

    const json = await res.json();
    expect(Array.isArray(json)).toBe(true);
  });

  test("GET /spaces includes the resolved fixture Space", async ({ request }) => {
    const res = await request.get(getBackendUrl("/spaces"));
    const spaces = (await res.json()) as Array<{ id: string; name: string }>;
    expect(
      spaces.some((space) => space.id === spaceId && space.name === "default"),
    )
      .toBe(true);
  });

  test("GET /spaces/:space_id/entries returns list", async ({ request }) => {
    const res = await request.get(getBackendUrl(`/spaces/${spaceId}/entries`));
    expect(res.ok()).toBeTruthy();

    const json = await res.json();
    expect(Array.isArray(json)).toBe(true);
  });

  test("GET /nonexistent-api returns 404", async ({ request }) => {
    const res = await request.get(getBackendUrl("/nonexistent-endpoint-xyz"));
    expect(res.status()).toBe(404);
  });
});
