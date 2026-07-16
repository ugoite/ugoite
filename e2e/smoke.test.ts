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
  getDefaultSpaceId,
  getBackendUrl,
  waitForServers,
} from "./lib/client.ts";

test.describe("Smoke Tests", () => {
  test.beforeAll(async ({ request }) => {
    await waitForServers(request);
    await ensureDefaultForm(request);
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
    const spaceId = await getDefaultSpaceId(request);
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
      .toBeVisible();
    await expect(page.getByRole("link", { name: "Back to Form" }))
      .toHaveAttribute(
        "href",
        `/spaces/${spaceId}/forms?form=Entry`,
      );

    await request.delete(
      getBackendUrl(`/spaces/${spaceId}/entries/${created.id}`),
    );
  });

  test("plain Entries list is integrated into the Forms workspace", async ({ page, request }) => {
    const spaceId = await getDefaultSpaceId(request);
    await page.goto(`/spaces/${spaceId}/entries`);
    await expect(page).toHaveURL(`/spaces/${spaceId}/forms?form=Entry`);
    await expect(page.getByRole("tab")).toHaveCount(0);
    await expect(
      page.getByRole("button", { name: "Form", exact: true }).locator("svg path"),
    )
      .toHaveCount(2);
  });

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

  test("GET /spaces includes default space", async ({ request }) => {
    const res = await request.get(getBackendUrl("/spaces"));
    const spaces = (await res.json()) as Array<{ name: string }>;
    const defaultWs = spaces.find((ws) => ws.name === "default");
    expect(defaultWs).toBeDefined();
  });

  test("GET /spaces/:space_id/entries returns list", async ({ request }) => {
    const spaceId = await getDefaultSpaceId(request);
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
