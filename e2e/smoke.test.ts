/**
 * Smoke Tests for IEapp
 *
 * These tests verify that the basic infrastructure is working:
 * - Frontend serves pages
 * - API endpoints respond correctly
 */

import { describe, expect, test, beforeAll } from "bun:test";
import {
	E2EClient,
	waitForServers,
} from "./lib/client";

const client = new E2EClient();

describe("Smoke Tests", () => {
	beforeAll(async () => {
		// Wait for servers to be ready with a longer timeout
		await waitForServers(client, { timeout: 60000 });
	});

	describe("Frontend Accessibility", () => {
		test("GET / returns HTML with DOCTYPE", async () => {
			const res = await client.getFrontend("/");
			expect(res.ok).toBe(true);

			const body = await res.text();
			expect(body.toLowerCase()).toContain("<!doctype html>");
		});

		test("GET / has correct content-type", async () => {
			const res = await client.getFrontend("/");
			expect(res.headers.get("content-type")).toContain("text/html");
		});

		test("GET /notes returns HTML", async () => {
			const res = await client.getFrontend("/notes");
			expect(res.ok).toBe(true);

			const body = await res.text();
			expect(body.toLowerCase()).toContain("<!doctype html>");
		});

			test("GET /notes/:id returns HTML", async () => {
			// Create a note first so we can navigate to a real detail route
				const createRes = await client.postApi("/api/workspaces/default/notes", {
				content: `# E2E Detail Route Note\n\nCreated at ${new Date().toISOString()}`,
			});
			expect(createRes.status).toBe(201);

			const created = (await createRes.json()) as { id: string };
			expect(created).toHaveProperty("id");

			const res = await client.getFrontend(`/notes/${created.id}`);
			expect(res.ok).toBe(true);

			const body = await res.text();
			expect(body.toLowerCase()).toContain("<!doctype html>");

			// Cleanup
				await client.deleteApi(`/api/workspaces/default/notes/${created.id}`);
		});

		test("GET /about returns HTML", async () => {
			const res = await client.getFrontend("/about");
			expect(res.ok).toBe(true);

			const body = await res.text();
			expect(body.toLowerCase()).toContain("<!doctype html>");
		});
	});

	describe("API Workspaces", () => {
		test("GET /api/workspaces returns list", async () => {
			const res = await client.getApi("/api/workspaces");
			expect(res.ok).toBe(true);

			const json = await res.json();
			expect(Array.isArray(json)).toBe(true);
		});

		test("GET /api/workspaces includes default workspace", async () => {
			const res = await client.getApi("/api/workspaces");
			const workspaces = await res.json();

			const defaultWs = workspaces.find(
				(ws: { name: string }) => ws.name === "default",
			);
			expect(defaultWs).toBeDefined();
		});
	});

	describe("API Notes", () => {
		test("GET /api/workspaces/default/notes returns list", async () => {
			const res = await client.getApi("/api/workspaces/default/notes");
			expect(res.ok).toBe(true);

			const json = await res.json();
			expect(Array.isArray(json)).toBe(true);
		});
	});

	describe("Error Handling", () => {
		test("GET /nonexistent-api returns 404", async () => {
			const res = await client.getApi("/nonexistent-endpoint-xyz");
			expect(res.status).toBe(404);
		});
	});
});
