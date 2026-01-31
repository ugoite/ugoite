import { expect, test } from "@playwright/test";
import { getBackendUrl, waitForServers } from "./lib/client";

const workspaceId = "default";

test.describe("Class", () => {
	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
	});

	test("Create and List Classes", async ({ request }) => {
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

		const listRes = await request.get(
			getBackendUrl(`/workspaces/${workspaceId}/classes`),
		);
		expect(listRes.status()).toBe(200);
		const noteClasses = (await listRes.json()) as Array<{ name: string }>;
		expect(Array.isArray(noteClasses)).toBe(true);
		const found = noteClasses.find((s) => s.name === className);
		expect(found).toBeDefined();
	});

	test("Query Notes by Class", async ({ request }) => {
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

		const noteContent = `---
class: ${className}
---
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

		const queryRes = await request.post(
			getBackendUrl(`/workspaces/${workspaceId}/query`),
			{ data: { filter: { class: className } } },
		);
		expect(queryRes.status()).toBe(200);
		const results = (await queryRes.json()) as Array<{
			id: string;
			properties: { Status?: string };
		}>;
		expect(Array.isArray(results)).toBe(true);
		expect(results.length).toBeGreaterThan(0);
		const foundNote = results.find((n) => n.id === note.id);
		expect(foundNote).toBeDefined();
		expect(foundNote?.properties.Status).toBe("Active");

		await request.delete(
			getBackendUrl(`/workspaces/${workspaceId}/notes/${note.id}`),
		);
	});
});
