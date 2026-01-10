import { describe, expect, test, beforeAll } from "bun:test";
import { E2EClient, waitForServers } from "./lib/client";

const client = new E2EClient();
const workspaceId = "default";

describe("Note Class", () => {
	beforeAll(async () => {
		await waitForServers(client, { timeout: 60000 });
	});

	test("Create and List Classes", async () => {
		const className = "E2ETestClass";
		const classDef = {
			name: className,
			version: 1,
			template: "# E2ETestClass\n\n## Field1\n",
			fields: {
				Field1: { type: "string", required: true },
			},
		};

		// Create Class
		const createRes = await client.postApi(
			`/workspaces/${workspaceId}/classes`,
			classDef,
		);
		expect(createRes.status).toBe(201);

		// List Classes
		const listRes = await client.getApi(`/workspaces/${workspaceId}/classes`);
		expect(listRes.status).toBe(200);
		const noteClasses = (await listRes.json()) as any[];
		expect(Array.isArray(noteClasses)).toBe(true);
		const found = noteClasses.find((s: any) => s.name === className);
		expect(found).toBeDefined();
	});

	test("Query Notes by Class", async () => {
		const className = "QueryTestClass";
		const classDef = {
			name: className,
			version: 1,
			template: "# QueryTestClass\n\n## Status\n",
			fields: {
				Status: { type: "string", required: true },
			},
		};

		// Create Class
		await client.postApi(`/workspaces/${workspaceId}/classes`, classDef);

		// Create Note with this class
		const noteContent = `---
class: ${className}
---
## Status
Active
`;
		const noteRes = await client.postApi(`/workspaces/${workspaceId}/notes`, {
			content: noteContent,
		});
		expect(noteRes.status).toBe(201);
		const note = (await noteRes.json()) as { id: string };

		// Wait for indexing (search endpoint triggers run_once)
		await client.getApi(`/workspaces/${workspaceId}/search?q=Active`);

		// Query
		const queryRes = await client.postApi(`/workspaces/${workspaceId}/query`, {
			filter: { class: className },
		});
		expect(queryRes.status).toBe(200);
		const results = (await queryRes.json()) as any[];
		expect(Array.isArray(results)).toBe(true);
		expect(results.length).toBeGreaterThan(0);
		const foundNote = results.find((n: any) => n.id === note.id);
		expect(foundNote).toBeDefined();
		expect(foundNote.properties.Status).toBe("Active");

		// Cleanup
		await client.deleteApi(`/workspaces/${workspaceId}/notes/${note.id}`);
	});
});
