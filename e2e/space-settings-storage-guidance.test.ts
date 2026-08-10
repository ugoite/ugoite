import { expect, test } from "@playwright/test";
import { getDefaultSpaceId, getFrontendUrl, waitForServers } from "./lib/client.ts";

test.describe("Space settings storage guidance", () => {
	let spaceId = "";

  test.beforeAll(async ({ request }) => {
    await waitForServers(request);
		spaceId = await getDefaultSpaceId(request);
  });

  test("REQ-FE-017: storage settings explain that configuration does not move the current Space", async ({ page }) => {
    await page.goto(getFrontendUrl(`/spaces/${spaceId}/settings`), {
      waitUntil: "networkidle",
    });

    await expect(page.getByText(/saved configuration metadata only/i))
      .toBeVisible();
    await expect(
      page.getByText(
        /does not move existing data or change the backend's current storage root/i,
      ),
    ).toBeVisible();
  });
});
