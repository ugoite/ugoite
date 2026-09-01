import { expect, type Page, test } from "@playwright/test";
import {
  type DocsiteServer,
  startDocsiteServer,
} from "./support/docsite-server.ts";

const docPath = "/docs/spec/";
const editLinkDocPath = "/docs/guide/automate/cli/";
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
      "https://github.com/ugoite/ugoite/edit/main/docs/guide/automate/cli.md",
    );
  });

  test("REQ-E2E-005: the beginner path follows its documented learning order", async ({ page }) => {
    // Mitase evidence: REQ-E2E-005#criterion.beginner-path.
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto(buildDocsiteUrl("/docs/guide/"), {
      waitUntil: "networkidle",
    });

    const startHereLinks = page.locator(
      '#starlight__sidebar a[href*="/docs/guide/start/"]',
    );
    await expect(startHereLinks).toHaveText([
      "Overview",
      "Core concepts",
      "Container quick start",
      "Create the first browser entry",
    ]);
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

  await openSidebarGroup(page, "Automate", "CLI guide");
  await expect(sidebar.getByRole("link", { name: "CLI guide" }))
    .toHaveAttribute(
      "href",
      /\/docs\/guide\/automate\/cli\/$/,
    );
  await expect(
    sidebar.getByRole("link", { name: "Container quick start" }),
  ).toBeVisible();
  await openSidebarGroup(page, "Architecture", "Architecture North Star");
  await openSidebarGroup(page, "Principles", "Architecture North Star");
  await expect(
    sidebar.getByRole("link", { name: "Architecture North Star" }),
  ).toBeVisible();
  await expect(sidebar.getByText("Specification", { exact: true }).first())
    .toBeVisible();
  if (options.expectSpecificationLink) {
    await openSidebarGroup(page, "Specification", "Ugoite specification index");
    await expect(
      sidebar.getByRole("link", { name: "Ugoite specification index" }),
    ).toBeVisible();
  }
}

async function openSidebarGroup(
  page: Page,
  label: string,
  expectedLink: string,
): Promise<void> {
  const sidebar = page.locator("#starlight__sidebar");
  const link = sidebar.getByRole("link", { name: expectedLink });
  if (await link.isVisible()) {
    return;
  }
  const summary = sidebar.locator("summary").filter({ hasText: label }).first();
  await summary.click();
}
