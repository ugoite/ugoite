// REQ-API-001: Space CRUD
// REQ-API-002: Entry CRUD
import { describe, it, expect, beforeEach } from "vitest";
import { http, HttpResponse } from "msw";
import { assetApi } from "./asset-api";
import { formApi } from "./form-api";
import { entryApi, RevisionConflictError } from "./entry-api";
import { searchApi } from "./search-api";
import { spaceApi } from "./space-api";
import { joinUrl } from "./api";
import { resetMockData, seedSpace, seedEntry } from "~/test/mocks/handlers";
import { server } from "~/test/mocks/server";
import type { Entry, EntryRecord, Space } from "./types";

describe("spaceApi", () => {
	beforeEach(() => {
		resetMockData();
	});

	describe("list", () => {
		it("should return empty array when no spaces exist", async () => {
			const spaces = await spaceApi.list();
			expect(spaces).toEqual([]);
		});

		it("should return all spaces", async () => {
			const ws1: Space = { id: "ws1", name: "Space 1", created_at: "2025-01-01T00:00:00Z" };
			const ws2: Space = { id: "ws2", name: "Space 2", created_at: "2025-01-02T00:00:00Z" };
			seedSpace(ws1);
			seedSpace(ws2);

			const spaces = await spaceApi.list();
			expect(spaces).toHaveLength(2);
			expect(spaces.map((w) => w.id)).toContain("ws1");
			expect(spaces.map((w) => w.id)).toContain("ws2");
		});
	});

	describe("create", () => {
		it("should create a new space", async () => {
			const result = await spaceApi.create("my-space");
			expect(result.id).toBe("my-space");
			expect(result.name).toBe("my-space");

			// Verify it exists
			const spaces = await spaceApi.list();
			expect(spaces).toHaveLength(1);
		});

		it("should throw error for duplicate space", async () => {
			await spaceApi.create("my-space");
			await expect(spaceApi.create("my-space")).rejects.toThrow("already exists");
		});

		it("should surface validation errors without object placeholders [REQ-FE-043]", async () => {
			server.use(
				http.post("http://localhost:3000/api/spaces", () =>
					HttpResponse.json(
						{
							detail: [
								{
									loc: ["body", "name"],
									msg: "Input should be at least 1 character",
									type: "string_too_short",
								},
							],
						},
						{ status: 422 },
					),
				),
			);

			await expect(spaceApi.create("")).rejects.toThrow("Input should be at least 1 character");
			await expect(spaceApi.create("")).rejects.not.toThrow("[object Object]");
		});
	});

	describe("patch and test connection", () => {
		it("patches space metadata and settings", async () => {
			await spaceApi.create("patched");
			const updated = await spaceApi.patch("patched", {
				storage_config: { uri: "file:///tmp/data" },
				settings: { default_form: "Meeting" },
			});

			expect(updated.storage_config?.uri).toBe("file:///tmp/data");
			expect(updated.settings?.default_form).toBe("Meeting");
		});

		it("tests storage connection", async () => {
			await spaceApi.create("patched");
			const result = await spaceApi.testConnection("patched", {
				storage_config: { uri: "file:///tmp/data" },
			});
			expect(result.status).toBe("ok");
		});
	});
});

describe("entryApi", () => {
	const testSpace: Space = {
		id: "test-ws",
		name: "Test Space",
		created_at: "2025-01-01T00:00:00Z",
	};

	beforeEach(() => {
		resetMockData();
		seedSpace(testSpace);
	});

	describe("list", () => {
		it("should return empty array when no entries exist", async () => {
			const entries = await entryApi.list("test-ws");
			expect(entries).toEqual([]);
		});

		it("should return all entries in space", async () => {
			const entry: Entry = {
				id: "entry-1",
				content: "# Test Entry\n\nContent",
				revision_id: "rev-1",
				created_at: "2025-01-01T00:00:00Z",
				updated_at: "2025-01-01T00:00:00Z",
			};
			const record: EntryRecord = {
				id: "entry-1",
				title: "Test Entry",
				updated_at: "2025-01-01T00:00:00Z",
				properties: {},
				tags: [],
				links: [],
			};
			seedEntry("test-ws", entry, record);

			const entries = await entryApi.list("test-ws");
			expect(entries).toHaveLength(1);
			expect(entries[0].title).toBe("Test Entry");
		});

		it("REQ-FE-054: entryApi normalizes unix-second timestamps for entry lists", async () => {
			server.use(
				http.get("http://localhost:3000/api/spaces/test-ws/entries", () =>
					HttpResponse.json([
						{
							id: "entry-1",
							title: "Test Entry",
							updated_at: 1772960822.056,
							properties: {},
							tags: [],
							links: [],
						},
					]),
				),
			);

			const entries = await entryApi.list("test-ws");
			expect(entries[0].updated_at).toBe(new Date(1772960822.056 * 1000).toISOString());
		});
	});

	describe("create", () => {
		it("should create a entry and extract title from markdown", async () => {
			const result = await entryApi.create("test-ws", {
				content: "# My Meeting Entries\n\n## Date\n2025-01-15\n\n## Attendees\nAlice, Bob",
			});

			expect(result.id).toBeDefined();
			expect(result.revision_id).toBeDefined();

			// Verify the entry was indexed with extracted properties
			const entries = await entryApi.list("test-ws");
			expect(entries).toHaveLength(1);
			expect(entries[0].title).toBe("My Meeting Entries");
			expect(entries[0].properties).toHaveProperty("Date");
			expect(entries[0].properties).toHaveProperty("Attendees");
		});

		it("should extract H2 headers as properties", async () => {
			const result = await entryApi.create("test-ws", {
				content: "# Task\n\n## Status\nPending\n\n## Priority\nHigh",
			});

			const entries = await entryApi.list("test-ws");
			const entry = entries.find((n) => n.id === result.id);
			expect(entry?.properties.Status).toBe("Pending");
			expect(entry?.properties.Priority).toBe("High");
		});
	});

	describe("get", () => {
		it("should return full entry content", async () => {
			const content = "# Full Entry\n\nWith body content";
			const entry: Entry = {
				id: "entry-get",
				content,
				revision_id: "rev-get",
				created_at: "2025-01-01T00:00:00Z",
				updated_at: "2025-01-01T00:00:00Z",
			};
			const record: EntryRecord = {
				id: "entry-get",
				title: "Full Entry",
				updated_at: "2025-01-01T00:00:00Z",
				properties: {},
				tags: [],
				links: [],
			};
			seedEntry("test-ws", entry, record);

			const fetched = await entryApi.get("test-ws", "entry-get");
			expect(fetched.content).toBe(content);
			expect(fetched.revision_id).toBe("rev-get");
		});

		it("should throw error for non-existent entry", async () => {
			await expect(entryApi.get("test-ws", "non-existent")).rejects.toThrow();
		});
	});

	describe("update", () => {
		it("should update entry with correct parent_revision_id", async () => {
			const createResult = await entryApi.create("test-ws", {
				content: "# Original\n\n## Status\nDraft",
			});

			const updateResult = await entryApi.update("test-ws", createResult.id, {
				markdown: "# Updated\n\n## Status\nPublished",
				parent_revision_id: createResult.revision_id,
			});

			expect(updateResult.revision_id).not.toBe(createResult.revision_id);

			// Verify index was updated
			const entries = await entryApi.list("test-ws");
			const entry = entries.find((n) => n.id === createResult.id);
			expect(entry?.title).toBe("Updated");
			expect(entry?.properties.Status).toBe("Published");
		});

		it("should throw RevisionConflictError (409) on revision mismatch", async () => {
			const createResult = await entryApi.create("test-ws", {
				content: "# Original",
			});

			// First update succeeds
			await entryApi.update("test-ws", createResult.id, {
				markdown: "# First Update",
				parent_revision_id: createResult.revision_id,
			});

			// Second update with stale revision should fail
			await expect(
				entryApi.update("test-ws", createResult.id, {
					markdown: "# Stale Update",
					parent_revision_id: createResult.revision_id, // Stale!
				}),
			).rejects.toThrow(RevisionConflictError);
		});
	});

	describe("delete", () => {
		it("should remove entry from list", async () => {
			const result = await entryApi.create("test-ws", {
				content: "# To Delete",
			});

			let entries = await entryApi.list("test-ws");
			expect(entries).toHaveLength(1);

			await entryApi.delete("test-ws", result.id);

			entries = await entryApi.list("test-ws");
			expect(entries).toHaveLength(0);
		});
	});

	describe("search, assets, and links", () => {
		it("searches entries by keyword", async () => {
			const created = await entryApi.create("test-ws", {
				content: "# Rocket Project\nEntries about propulsion",
			});

			const matches = await searchApi.keyword("test-ws", "rocket");
			expect(matches.find((m) => m.id === created.id)).toBeDefined();
		});

		it("uploads asset and blocks deletion when referenced", async () => {
			const { id, revision_id } = await entryApi.create("test-ws", {
				content: "# Audio Entry",
			});

			const file = new File(["data"], "voice.m4a", { type: "audio/m4a" });
			const asset = await assetApi.upload("test-ws", file);

			await entryApi.update("test-ws", id, {
				markdown: "# Audio Entry\nupdated",
				parent_revision_id: revision_id,
				assets: [asset],
			});

			await expect(assetApi.delete("test-ws", asset.id)).rejects.toThrow();
		});
	});
});

describe("formApi", () => {
	const testSpace: Space = {
		id: "form-ws",
		name: "Form Space",
		created_at: "2025-01-01T00:00:00Z",
	};

	beforeEach(() => {
		resetMockData();
		seedSpace(testSpace);
	});

	it("lists forms (empty by default)", async () => {
		const forms = await formApi.list("form-ws");
		expect(forms).toEqual([]);
	});

	it("creates and gets a form", async () => {
		await formApi.create("form-ws", {
			name: "Meeting",
			version: 1,
			template: "# Meeting\n\n## Date\n",
			fields: { Date: { type: "date", required: true } },
		});

		const forms = await formApi.list("form-ws");
		expect(forms.find((s) => s.name === "Meeting")).toBeDefined();

		const fetched = await formApi.get("form-ws", "Meeting");
		expect(fetched.name).toBe("Meeting");
		expect(fetched.fields.Date.type).toBe("date");
	});
});

describe("joinUrl", () => {
	it("joins base and path", () => {
		expect(joinUrl("http://example.com", "/foo")).toBe("http://example.com/foo");
	});

	it("returns path when base is empty", () => {
		expect(joinUrl("", "/bar")).toBe("/bar");
	});

	it("handles trailing slash on base", () => {
		expect(joinUrl("http://example.com/", "/foo")).toBe("http://example.com/foo");
	});

	it("handles missing leading slash on path", () => {
		expect(joinUrl("http://example.com", "foo")).toBe("http://example.com/foo");
	});
});

describe("spaceApi members", () => {
	it("lists members", async () => {
		resetMockData();
		await spaceApi.create("ws-members");
		const members = await spaceApi.listMembers("ws-members");
		expect(Array.isArray(members)).toBe(true);
	});

	it("invites a member", async () => {
		resetMockData();
		await spaceApi.create("ws-invite");
		const result = await spaceApi.inviteMember("ws-invite", { user_id: "user1", role: "editor" });
		expect(result.invitation.token).toBeDefined();
	});

	it("accepts invitation", async () => {
		resetMockData();
		await spaceApi.create("ws-accept");
		const result = await spaceApi.acceptInvitation("ws-accept", { token: "tok", user_id: "u1" });
		expect(result.member.user_id).toBe("u1");
	});

	it("updates member role", async () => {
		resetMockData();
		await spaceApi.create("ws-role");
		const result = await spaceApi.updateMemberRole("ws-role", "user1", { role: "viewer" });
		expect(result.member.role).toBe("viewer");
	});

	it("revokes member", async () => {
		resetMockData();
		await spaceApi.create("ws-revoke");
		const result = await spaceApi.revokeMember("ws-revoke", "user1");
		expect(result.member.state).toBe("revoked");
	});
});

describe("error paths", () => {
	it("spaceApi.list throws on failure", async () => {
		server.use(
			http.get("http://localhost:3000/api/spaces", () =>
				HttpResponse.json({ detail: "Error" }, { status: 500 }),
			),
		);
		await expect(spaceApi.list()).rejects.toThrow("Failed to list spaces");
	});

	it("spaceApi.get throws on failure", async () => {
		server.use(
			http.get("http://localhost:3000/api/spaces/nonexistent", () =>
				HttpResponse.json({ detail: "Not found" }, { status: 404 }),
			),
		);
		await expect(spaceApi.get("nonexistent")).rejects.toThrow("Failed to get space");
	});

	it("spaceApi.get returns a space", async () => {
		resetMockData();
		await spaceApi.create("existing-space");
		const space = await spaceApi.get("existing-space");
		expect(space.id).toBe("existing-space");
	});

	it("spaceApi.patch throws on failure", async () => {
		server.use(
			http.patch("http://localhost:3000/api/spaces/nonexistent", () =>
				HttpResponse.json({ detail: "Space not found" }, { status: 404 }),
			),
		);
		await expect(spaceApi.patch("nonexistent", {})).rejects.toThrow("Space not found");
	});

	it("spaceApi.testConnection throws on failure", async () => {
		server.use(
			http.post("http://localhost:3000/api/spaces/nonexistent/test-connection", () =>
				HttpResponse.json({ detail: "Not found" }, { status: 404 }),
			),
		);
		await expect(
			spaceApi.testConnection("nonexistent", { storage_config: { uri: "x" } }),
		).rejects.toThrow();
	});

	it("spaceApi.listMembers throws on failure", async () => {
		server.use(
			http.get("http://localhost:3000/api/spaces/nonexistent/members", () =>
				HttpResponse.json({ detail: "Not found" }, { status: 404 }),
			),
		);
		await expect(spaceApi.listMembers("nonexistent")).rejects.toThrow();
	});

	it("spaceApi.inviteMember throws on failure", async () => {
		server.use(
			http.post("http://localhost:3000/api/spaces/nonexistent/members/invitations", () =>
				HttpResponse.json({ detail: "Not found" }, { status: 404 }),
			),
		);
		await expect(
			spaceApi.inviteMember("nonexistent", { user_id: "u1", role: "editor" }),
		).rejects.toThrow();
	});

	it("spaceApi.acceptInvitation throws on failure", async () => {
		server.use(
			http.post("http://localhost:3000/api/spaces/nonexistent/members/accept", () =>
				HttpResponse.json({ detail: "Not found" }, { status: 404 }),
			),
		);
		await expect(
			spaceApi.acceptInvitation("nonexistent", { token: "tok", user_id: "u1" }),
		).rejects.toThrow();
	});

	it("spaceApi.updateMemberRole throws on failure", async () => {
		server.use(
			http.post("http://localhost:3000/api/spaces/nonexistent/members/u1/role", () =>
				HttpResponse.json({ detail: "Not found" }, { status: 404 }),
			),
		);
		await expect(
			spaceApi.updateMemberRole("nonexistent", "u1", { role: "viewer" }),
		).rejects.toThrow();
	});

	it("spaceApi.revokeMember throws on failure", async () => {
		server.use(
			http.delete("http://localhost:3000/api/spaces/nonexistent/members/u1", () =>
				HttpResponse.json({ detail: "Not found" }, { status: 404 }),
			),
		);
		await expect(spaceApi.revokeMember("nonexistent", "u1")).rejects.toThrow();
	});

	it("entryApi.history returns revisions", async () => {
		resetMockData();
		seedSpace({ id: "ws-history", name: "H", created_at: "2025-01-01T00:00:00Z" });
		const created = await entryApi.create("ws-history", { content: "# Entry" });
		const history = await entryApi.history("ws-history", created.id);
		expect(history.revisions).toBeDefined();
	});

	it("entryApi.getRevision returns a revision", async () => {
		resetMockData();
		seedSpace({ id: "ws-rev", name: "R", created_at: "2025-01-01T00:00:00Z" });
		const created = await entryApi.create("ws-rev", { content: "# Entry" });
		const entry = await entryApi.get("ws-rev", created.id);
		const revision = await entryApi.getRevision("ws-rev", created.id, entry.revision_id);
		expect(revision.revision_id).toBe(entry.revision_id);
	});

	it("entryApi.restore succeeds", async () => {
		resetMockData();
		seedSpace({ id: "ws-restore", name: "RR", created_at: "2025-01-01T00:00:00Z" });
		const created = await entryApi.create("ws-restore", { content: "# Entry" });
		const entry = await entryApi.get("ws-restore", created.id);
		const restored = await entryApi.restore("ws-restore", created.id, entry.revision_id);
		expect(restored).toBeDefined();
	});

	it("entryApi.createFromMarkdown creates entry", async () => {
		resetMockData();
		seedSpace({ id: "ws-md", name: "MD", created_at: "2025-01-01T00:00:00Z" });
		const result = await entryApi.createFromMarkdown("ws-md", "# Markdown Entry");
		expect(result.id).toBeDefined();
	});

	it("entryApi.createFromWebform creates entry", async () => {
		resetMockData();
		seedSpace({ id: "ws-wf", name: "WF", created_at: "2025-01-01T00:00:00Z" });
		const formDef = {
			name: "Task",
			template: "# Task\n\n## Status\n",
			fields: { Status: { type: "text" } },
		};
		const result = await entryApi.createFromWebform("ws-wf", formDef as never, "My Task", {
			Status: "Open",
		});
		expect(result.id).toBeDefined();
	});

	it("entryApi.createFromChat creates entry", async () => {
		resetMockData();
		seedSpace({ id: "ws-chat", name: "Chat", created_at: "2025-01-01T00:00:00Z" });
		const formDef = {
			name: "Task",
			template: "# Task\n\n## Status\n",
			fields: { Status: { type: "text" } },
		};
		const result = await entryApi.createFromChat("ws-chat", formDef as never, "Chat Task", {
			Status: "Pending",
		});
		expect(result.id).toBeDefined();
	});

	it("entryApi.get includes detail in error", async () => {
		server.use(
			http.get("http://localhost:3000/api/spaces/ws-err/entries/bad-id", () =>
				HttpResponse.json({ detail: "Custom error detail" }, { status: 404 }),
			),
		);
		await expect(entryApi.get("ws-err", "bad-id")).rejects.toThrow("Custom error detail");
	});

	it("entryApi.get uses statusText fallback when no detail in error", async () => {
		server.use(
			http.get("http://localhost:3000/api/spaces/ws-err/entries/no-detail-entry", () =>
				HttpResponse.json({ message: "Generic error" }, { status: 404 }),
			),
		);
		await expect(entryApi.get("ws-err", "no-detail-entry")).rejects.toThrow("Failed to get entry");
	});

	it("entryApi.update throws generic error on non-409 failure", async () => {
		server.use(
			http.put("http://localhost:3000/api/spaces/ws-err/entries/bad-id", () =>
				HttpResponse.json({ detail: "Server error" }, { status: 500 }),
			),
		);
		await expect(
			entryApi.update("ws-err", "bad-id", { markdown: "# X", parent_revision_id: "r1" }),
		).rejects.toThrow("Server error");
	});

	it("entryApi.delete throws on failure", async () => {
		server.use(
			http.delete("http://localhost:3000/api/spaces/ws-err/entries/bad-id", () =>
				HttpResponse.json({ detail: "Not found" }, { status: 404 }),
			),
		);
		await expect(entryApi.delete("ws-err", "bad-id")).rejects.toThrow("Failed to delete entry");
	});

	it("entryApi.restore throws on failure", async () => {
		server.use(
			http.post("http://localhost:3000/api/spaces/ws-err/entries/bad-id/restore", () =>
				HttpResponse.json({ detail: "Restore failed" }, { status: 500 }),
			),
		);
		await expect(entryApi.restore("ws-err", "bad-id", "rev-1")).rejects.toThrow("Restore failed");
	});

	it("entryApi.history throws on failure", async () => {
		server.use(
			http.get("http://localhost:3000/api/spaces/ws-err/entries/bad-id/history", () =>
				HttpResponse.json({ detail: "Not found" }, { status: 404 }),
			),
		);
		await expect(entryApi.history("ws-err", "bad-id")).rejects.toThrow(
			"Failed to get entry history",
		);
	});

	it("entryApi.getRevision throws on failure", async () => {
		server.use(
			http.get("http://localhost:3000/api/spaces/ws-err/entries/bad-id/history/rev-1", () =>
				HttpResponse.json({ detail: "Not found" }, { status: 404 }),
			),
		);
		await expect(entryApi.getRevision("ws-err", "bad-id", "rev-1")).rejects.toThrow(
			"Failed to get entry revision",
		);
	});

	it("assetApi.list returns assets", async () => {
		resetMockData();
		seedSpace({ id: "ws-asset", name: "A", created_at: "2025-01-01T00:00:00Z" });
		const assets = await assetApi.list("ws-asset");
		expect(Array.isArray(assets)).toBe(true);
	});

	it("assetApi.list throws on failure", async () => {
		server.use(
			http.get("http://localhost:3000/api/spaces/ws-asset-err/assets", () =>
				HttpResponse.json({ detail: "Error" }, { status: 500 }),
			),
		);
		await expect(assetApi.list("ws-asset-err")).rejects.toThrow("Failed to list assets");
	});

	it("assetApi.upload throws on failure", async () => {
		server.use(
			http.post("http://localhost:3000/api/spaces/ws-asset-err/assets", () =>
				HttpResponse.json({ detail: "Error" }, { status: 500 }),
			),
		);
		const file = new File(["data"], "test.txt");
		await expect(assetApi.upload("ws-asset-err", file)).rejects.toThrow("Failed to upload asset");
	});

	it("assetApi.delete throws with detail on failure", async () => {
		server.use(
			http.delete("http://localhost:3000/api/spaces/ws-asset-err/assets/bad-id", () =>
				HttpResponse.json({ detail: "Asset is referenced" }, { status: 409 }),
			),
		);
		await expect(assetApi.delete("ws-asset-err", "bad-id")).rejects.toThrow("Asset is referenced");
	});

	it("formApi.listTypes returns types", async () => {
		resetMockData();
		seedSpace({ id: "ws-types", name: "T", created_at: "2025-01-01T00:00:00Z" });
		const types = await formApi.listTypes("ws-types");
		expect(Array.isArray(types)).toBe(true);
	});

	it("formApi.listTypes throws on failure", async () => {
		server.use(
			http.get("http://localhost:3000/api/spaces/ws-types-err/forms/types", () =>
				HttpResponse.json({ detail: "Error" }, { status: 500 }),
			),
		);
		await expect(formApi.listTypes("ws-types-err")).rejects.toThrow("Failed to list form types");
	});

	it("formApi.list throws on failure", async () => {
		server.use(
			http.get("http://localhost:3000/api/spaces/ws-form-err/forms", () =>
				HttpResponse.json({ detail: "Error" }, { status: 500 }),
			),
		);
		await expect(formApi.list("ws-form-err")).rejects.toThrow("Failed to list forms");
	});

	it("formApi.get throws on failure", async () => {
		server.use(
			http.get("http://localhost:3000/api/spaces/ws-form-err/forms/nonexistent", () =>
				HttpResponse.json({ detail: "Not found" }, { status: 404 }),
			),
		);
		await expect(formApi.get("ws-form-err", "nonexistent")).rejects.toThrow("Failed to get form");
	});

	it("formApi.create throws on failure", async () => {
		server.use(
			http.post("http://localhost:3000/api/spaces/ws-form-err/forms", () =>
				HttpResponse.json({ detail: "Invalid" }, { status: 422 }),
			),
		);
		await expect(
			formApi.create("ws-form-err", { name: "Bad", version: 1, template: "", fields: {} }),
		).rejects.toThrow("Failed to create form");
	});

	it("searchApi.keyword throws on failure", async () => {
		server.use(
			http.get("http://localhost:3000/api/spaces/ws-search-err/search", () =>
				HttpResponse.json({ detail: "Error" }, { status: 500 }),
			),
		);
		await expect(searchApi.keyword("ws-search-err", "test")).rejects.toThrow(
			"Failed to search entries",
		);
	});

	it("searchApi.query throws on failure", async () => {
		server.use(
			http.post("http://localhost:3000/api/spaces/ws-search-err/query", () =>
				HttpResponse.json({ detail: "Error" }, { status: 500 }),
			),
		);
		await expect(searchApi.query("ws-search-err", {})).rejects.toThrow("Failed to query space");
	});

	it("spaceApi.create uses fallback message when error response has no detail", async () => {
		server.use(
			http.post("http://localhost:3000/api/spaces", () =>
				HttpResponse.json({ message: "No detail here" }, { status: 422 }),
			),
		);
		await expect(spaceApi.create("test-no-detail")).rejects.toThrow("Failed to create space");
	});
});
