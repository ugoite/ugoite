// REQ-FE-051: SQL session management
import { describe, it, expect, beforeEach } from "vitest";
import { http, HttpResponse } from "msw";
import { sqlSessionApi } from "./sql-session-api";
import { resetMockData, seedSpace } from "~/test/mocks/handlers";
import { server } from "~/test/mocks/server";
import type { Space } from "./types";
import { testApiUrl } from "~/test/http-origin";

const testSpace: Space = {
	id: "sess-ws",
	name: "Session Space",
	created_at: "2025-01-01T00:00:00Z",
};

describe("sqlSessionApi", () => {
	beforeEach(() => {
		resetMockData();
		seedSpace(testSpace);
	});

	it("creates a SQL session", async () => {
		const session = await sqlSessionApi.create("sess-ws", "SELECT 1");
		expect(session.id).toBeDefined();
		expect(session.status).toBe("ready");
	});

	it("gets a SQL session by id", async () => {
		const created = await sqlSessionApi.create("sess-ws", "SELECT 1");
		const fetched = await sqlSessionApi.get("sess-ws", created.id);
		expect(fetched.id).toBe(created.id);
	});

	it("counts rows in a session", async () => {
		const created = await sqlSessionApi.create("sess-ws", "SELECT 1");
		const count = await sqlSessionApi.count("sess-ws", created.id);
		expect(count).toBe(3);
	});

	it("fetches rows from a session", async () => {
		const created = await sqlSessionApi.create("sess-ws", "SELECT 1");
		const result = await sqlSessionApi.rows("sess-ws", created.id, 0, 25);
		expect(result.rows).toBeDefined();
		expect(result.offset).toBe(0);
		expect(result.limit).toBe(25);
		expect(result.totalCount).toBe(3);
	});

	it("REQ-FE-054: sqlSessionApi normalizes unix-second timestamps for session rows", async () => {
		server.use(
			http.get(testApiUrl("/spaces/sess-ws/sql-sessions/sess-1/rows"), () =>
				HttpResponse.json({
					rows: [
						{
							id: "entry-1",
							title: "Query Entry",
							form: "Meeting",
							updated_at: 1772960822.056,
							properties: {},
							tags: [],
							links: [],
						},
					],
					offset: 0,
					limit: 25,
					total_count: 1,
				}),
			),
		);

		const result = await sqlSessionApi.rows("sess-ws", "sess-1", 0, 25);
		expect(result.rows[0].updated_at).toBe(new Date(1772960822.056 * 1000).toISOString());
	});

	it("throws on create failure", async () => {
		server.use(
			http.post(testApiUrl("/spaces/sess-ws/sql-sessions"), () =>
				HttpResponse.json({ detail: "Failed" }, { status: 500 }),
			),
		);
		await expect(sqlSessionApi.create("sess-ws", "SELECT 1")).rejects.toThrow(
			"Failed to create SQL session",
		);
	});

	it("throws on get failure", async () => {
		server.use(
			http.get(testApiUrl("/spaces/sess-ws/sql-sessions/bad"), () =>
				HttpResponse.json({ detail: "Not found" }, { status: 404 }),
			),
		);
		await expect(sqlSessionApi.get("sess-ws", "bad")).rejects.toThrow("Failed to load SQL session");
	});

	it("throws on count failure", async () => {
		server.use(
			http.get(testApiUrl("/spaces/sess-ws/sql-sessions/bad/count"), () =>
				HttpResponse.json({ detail: "Not found" }, { status: 404 }),
			),
		);
		await expect(sqlSessionApi.count("sess-ws", "bad")).rejects.toThrow(
			"Failed to load SQL session count",
		);
	});

	it("throws on rows failure", async () => {
		server.use(
			http.get(testApiUrl("/spaces/sess-ws/sql-sessions/bad/rows"), () =>
				HttpResponse.json({ detail: "Not found" }, { status: 404 }),
			),
		);
		await expect(sqlSessionApi.rows("sess-ws", "bad", 0, 10)).rejects.toThrow(
			"Failed to load SQL session rows",
		);
	});
});
