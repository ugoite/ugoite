import { expect, test } from "@playwright/test";
import { getBackendUrl, waitForServers } from "./lib/client";

const workspaceId = "default";

test.describe("Class", () => {
	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
	});

	test("Create and List Classes", async ({ page, request }) => {
		const className = `E2ETestClass-${Date.now()}`;
		const classDef = {
			name: className,
			version: 1,
			template: "# E2ETestClass\n\n## Field1\n",
			fields: {
				Field1: { type: "string", required: true },
			},
		};

		const createRes = await request.post(
			getBackendUrl(`/workspaces/${workspaceId}/classes`),
			{ data: classDef },
		);
		expect([200, 201]).toContain(createRes.status());

		await page.goto(`/workspaces/${workspaceId}/classes`);
		await expect(page.getByText(className)).toBeVisible({ timeout: 15000 });
	});

	test("Query Notes by Class", async ({ page, request }) => {
		const className = `QueryTestClass-${Date.now()}`;
		const classDef = {
			name: className,
			version: 1,
			template: "# QueryTestClass\n\n## Status\n",
			fields: {
				Status: { type: "string", required: true },
			},
		};

		await request.post(
			getBackendUrl(`/workspaces/${workspaceId}/classes`),
			{ data: classDef },
		);

		const noteTitle = `Query Note ${Date.now()}`;
		const noteContent = `---
class: ${className}
---
# ${noteTitle}

## Status
Active
`;
		const noteRes = await request.post(
			getBackendUrl(`/workspaces/${workspaceId}/notes`),
			{ data: { content: noteContent } },
		);
		expect(noteRes.status()).toBe(201);
		const note = (await noteRes.json()) as { id: string };

		await request.get(getBackendUrl(`/workspaces/${workspaceId}/search?q=Active`));

		await page.goto(
			`/workspaces/${workspaceId}/classes/${encodeURIComponent(className)}`,
			{ waitUntil: "networkidle" },
		);
		await expect(
			page.getByRole("heading", { name: className }).first(),
		).toBeVisible({ timeout: 15000 });
		await expect(page.getByText("Active")).toBeVisible({ timeout: 15000 });
		await expect(page.getByText(noteTitle)).toBeVisible({ timeout: 15000 });

		await request.delete(
			getBackendUrl(`/workspaces/${workspaceId}/notes/${note.id}`),
		);
	});
});
