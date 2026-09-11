import { expect, type Page, test } from "@playwright/test";
import {
  type DocsiteServer,
  startDocsiteServer,
} from "./support/docsite-server.ts";

const docPath = "/docs/spec/";
const editLinkDocPath = "/docs/get-started/";
const homepagePath = "/";

let docsiteServer: DocsiteServer | undefined;

test.describe("Docsite navigation layout", () => {
  test.beforeAll(async () => {
    test.setTimeout(180_000);
    docsiteServer = await startDocsiteServer();
  });

  test.afterAll(async () => {
    await docsiteServer?.stop();
  });

  test("REQ-E2E-005: Starlight exposes the documentation sidebar as a mobile menu", async ({ page }) => {
    // Mitase evidence: REQ-E2E-005#criterion.mobile-sidebar.
    // Mitase evidence: REQ-E2E-009#criterion.responsive-layout.
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(buildDocsiteUrl(docPath), { waitUntil: "networkidle" });

    const menuButton = page.getByRole("button", { name: "Menu" });
    const sidebar = page.locator("#starlight__sidebar");

    await expect(menuButton).toBeVisible();
    await expect(sidebar).toBeHidden();

    await menuButton.click();
    await expect(sidebar).toBeVisible();
    await expect(page.locator("body")).toHaveAttribute(
      "data-mobile-menu-expanded",
      "",
    );
    await expectSidebarToContainLinks(page, { expectSpecificationLink: true });
  });

  test("REQ-E2E-005: the mobile menu closes with Escape", async ({ page }) => {
    // Mitase evidence: REQ-E2E-005#criterion.mobile-sidebar.
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(buildDocsiteUrl(docPath), { waitUntil: "networkidle" });

    const menuButton = page.getByRole("button", { name: "Menu" });
    const sidebar = page.locator("#starlight__sidebar");
    await menuButton.click();
    await expect(sidebar).toBeVisible();

    await menuButton.press("Escape");
    await expect(sidebar).toBeHidden();
    await expect(page.locator("body")).not.toHaveAttribute(
      "data-mobile-menu-expanded",
      /.+/,
    );
  });

  test("REQ-E2E-009: desktop pages use Starlight's sidebar and table of contents", async ({ page }) => {
    // Mitase evidence: REQ-E2E-005#criterion.desktop-sidebar.
    // Mitase evidence: REQ-E2E-009#criterion.desktop-layout.
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto(buildDocsiteUrl(docPath), { waitUntil: "networkidle" });

    await expect(page.getByRole("button", { name: "Menu" })).toBeHidden();
    await expect(page.locator("#starlight__sidebar")).toBeVisible();
    await expect(page.locator(".right-sidebar-container")).toBeVisible();
    await expectSidebarToContainLinks(page, { expectSpecificationLink: true });

    await page.goto(buildDocsiteUrl(editLinkDocPath), {
      waitUntil: "networkidle",
    });
    await expect(
      page.getByRole("link", { name: "Edit page" }),
    ).toHaveAttribute(
      "href",
      "https://github.com/ugoite/ugoite/edit/main/docs/get-started/index.md",
    );
  });

  test("REQ-E2E-005: the beginner path follows its documented learning order", async ({ page }) => {
    // Mitase evidence: REQ-E2E-005#criterion.beginner-path.
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto(buildDocsiteUrl("/docs/get-started/"), {
      waitUntil: "networkidle",
    });

    const sidebar = page.locator("#starlight__sidebar");
    await expect(sidebar.getByText("Get started", { exact: true }).first())
      .toBeVisible();
    await expect(sidebar.getByText("Use Ugoite", { exact: true }).first())
      .toBeVisible();
    await expect(sidebar.getByText("Operate Ugoite", { exact: true }).first())
      .toBeVisible();

    const getStartedLinks = page.locator(
      '#starlight__sidebar a[href*="/docs/get-started/"]',
    );
    await expect(getStartedLinks).toHaveText(["Overview"]);
  });

  test("REQ-E2E-005: the homepage keeps the hero and Starlight navigation", async ({ page }) => {
    // Mitase evidence: REQ-E2E-005#criterion.homepage-navigation.
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(buildDocsiteUrl(homepagePath), {
      waitUntil: "networkidle",
    });

    await expect(page.getByText("A private, portable knowledge space"))
      .toBeVisible();
    await page.getByRole("button", { name: "Menu" }).click();
    await expectSidebarToContainLinks(page, { expectSpecificationLink: false });

    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto(buildDocsiteUrl(homepagePath), {
      waitUntil: "networkidle",
    });
    await expect(page.locator("#starlight__sidebar")).toBeVisible();
    await expectSidebarToContainLinks(page, { expectSpecificationLink: false });
  });
});

function buildDocsiteUrl(path: string): string {
  if (!docsiteServer) {
    throw new Error("Docsite server is unavailable");
  }
  return docsiteServer.buildUrl(path);
}

async function expectSidebarToContainLinks(
  page: Page,
  options: { expectSpecificationLink: boolean },
): Promise<void> {
  const sidebar = page.locator("#starlight__sidebar");

  for (
    const label of [
      "Get started",
      "Use Ugoite",
      "Operate Ugoite",
      "Vision & Concepts",
      "Develop Ugoite",
      "Reference",
      "Specification",
    ]
  ) {
    await expect(sidebar.getByText(label, { exact: true }).first())
      .toBeVisible();
  }

  await openSidebarGroup(page, "Get started");
  await expect(
    sidebar.locator('a[href$="/docs/get-started/"]'),
  ).toBeVisible();
  await openSidebarGroup(page, "Use Ugoite");
  await expect(
    sidebar.locator('a[href$="/docs/use/"]'),
  ).toBeVisible();
  if (options.expectSpecificationLink) {
    await openSidebarGroup(page, "Specification");
    await expect(
      sidebar.locator('a[href$="/docs/spec/"]'),
    ).toBeVisible();
  }
}

async function openSidebarGroup(
  page: Page,
  label: string,
): Promise<void> {
  const sidebar = page.locator("#starlight__sidebar");
  const summary = sidebar.locator("summary").filter({ hasText: label }).first();
  if (await summary.isVisible()) {
    // Expand collapsed groups; already-expanded groups keep their links visible.
    await summary.click().catch(() => {});
  }
}
