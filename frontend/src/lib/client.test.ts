// REQ-API-001: Space CRUD
// REQ-API-002: Entry CRUD
// REQ-API-009: Sample data space generation
// REQ-API-010: Sample data scenarios and async jobs
// REQ-FE-043: Frontend error formatting
import { describe, it, expect, beforeEach } from "vitest";
import { assetApi } from "./asset-api";
import { formApi } from "./form-api";
import { entryApi, RevisionConflictError } from "./entry-api";
import { searchApi } from "./search-api";
import { spaceApi } from "./space-api";
import { resetMockData, seedSpace, seedEntry } from "~/test/mocks/handlers";
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
	});

	describe("createSampleSpace", () => {
		it("should create a sample-data space [REQ-API-009]", async () => {
			const summary = await spaceApi.createSampleSpace({
				space_id: "sample-ws",
				scenario: "renewable-ops",
				entry_count: 120,
				seed: 42,
			});
			expect(summary.space_id).toBe("sample-ws");
			expect(summary.entry_count).toBe(120);
			expect(summary.form_count).toBeGreaterThanOrEqual(3);
			expect(summary.form_count).toBeLessThanOrEqual(6);
		});

		it("should surface validation errors without object placeholders [REQ-FE-043]", async () => {
			await expect(
				spaceApi.createSampleSpace({
					space_id: "bad-sample",
					scenario: "renewable-ops",
					entry_count: 10,
				}),
			).rejects.toThrow("entry_count must be >= 100");
		});
	});

	describe("sample scenarios and jobs", () => {
		it("should list sample-data scenarios [REQ-API-010]", async () => {
			const scenarios = await spaceApi.listSampleScenarios();
			expect(scenarios.length).toBeGreaterThan(1);
			expect(scenarios[0].id).toBeDefined();
		});

		it("should create and fetch sample-data jobs [REQ-API-010]", async () => {
			const job = await spaceApi.createSampleSpaceJob({
				space_id: "sample-job",
				scenario: "renewable-ops",
				entry_count: 120,
			});
			expect(job.status).toBe("completed");
			expect(job.summary?.space_id).toBe("sample-job");

			const fetched = await spaceApi.getSampleSpaceJob(job.job_id);
			expect(fetched.job_id).toBe(job.job_id);
			expect(fetched.summary?.space_id).toBe("sample-job");
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
