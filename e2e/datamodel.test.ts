import { describe, expect, test, beforeAll } from "bun:test";
import { E2EClient, waitForServers } from "./lib/client";

const client = new E2EClient();
const workspaceId = "default";

describe("Data Model (Schemas)", () => {
	beforeAll(async () => {
		await waitForServers(client, { timeout: 60000 });
	});

	test("Create and List Schemas", async () => {
		const schemaName = "E2ETestSchema";
		const schemaDef = {
			name: schemaName,
			version: 1,
			template: "# E2ETestSchema\n\n## Field1\n",
			fields: {
				Field1: { type: "string", required: true },
			},
		};

		// Create Schema
		const createRes = await client.postApi(
			`/workspaces/${workspaceId}/schemas`,
			schemaDef,
		);
		expect(createRes.status).toBe(201);

		// List Schemas
		const listRes = await client.getApi(`/workspaces/${workspaceId}/schemas`);
		expect(listRes.status).toBe(200);
		const schemas = (await listRes.json()) as any[];
		expect(Array.isArray(schemas)).toBe(true);
		const found = schemas.find((s: any) => s.name === schemaName);
		expect(found).toBeDefined();
	});

	test("Query Notes by Schema", async () => {
		const schemaName = "QueryTestSchema";
		const schemaDef = {
			name: schemaName,
			version: 1,
			template: "# QueryTestSchema\n\n## Status\n",
			fields: {
				Status: { type: "string", required: true },
			},
		};

		// Create Schema
		await client.postApi(`/workspaces/${workspaceId}/schemas`, schemaDef);

		// Create Note with this schema
		const noteContent = `---
class: ${schemaName}
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
			filter: { class: schemaName },
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
