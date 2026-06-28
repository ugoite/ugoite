import { expect, type Page, test } from "@playwright/test";
import {
  type DocsiteServer,
  startDocsiteServer,
} from "./support/docsite-server.ts";

const docPath = "/docs/spec/";
const editLinkDocPath = "/docs/guide/cli/";
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
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto(buildDocsiteUrl(docPath), { waitUntil: "networkidle" });

    await expect(page.getByRole("button", { name: "Menu" })).toBeHidden();
    await expect(page.locator("#starlight__sidebar")).toBeVisible();
    await expect(page.locator(".right-sidebar-container")).toBeVisible();
    await expectSidebarToContainLinks(page, { expectSpecificationLink: true });

    await page.goto(buildDocsiteUrl(editLinkDocPath), { waitUntil: "networkidle" });
    await expect(
      page.getByRole("link", { name: "Edit page" }),
    ).toHaveAttribute(
      "href",
      "https://github.com/ugoite/ugoite/edit/main/docs/guide/cli.md",
    );
  });

  test("REQ-E2E-005: the homepage keeps the hero and Starlight navigation", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(buildDocsiteUrl(homepagePath), { waitUntil: "networkidle" });

    await expect(page.getByText("A private, portable knowledge space")).toBeVisible();
    await page.getByRole("button", { name: "Menu" }).click();
    await expectSidebarToContainLinks(page, { expectSpecificationLink: false });

    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto(buildDocsiteUrl(homepagePath), { waitUntil: "networkidle" });
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

  await expect(sidebar.getByRole("link", { name: "CLI guide" })).toHaveAttribute(
    "href",
    /\/docs\/guide\/cli\/$/,
  );
  await expect(
    sidebar.getByRole("link", { name: "Container quick start" }),
  ).toBeVisible();
  await expect(
    sidebar.getByRole("link", { name: "Architecture North Star" }),
  ).toBeVisible();
  await expect(sidebar.getByText("Specification", { exact: true }).first())
    .toBeVisible();
  if (options.expectSpecificationLink) {
    await expect(
      sidebar.getByRole("link", { name: "Ugoite specification index" }),
    ).toBeVisible();
  }
}
