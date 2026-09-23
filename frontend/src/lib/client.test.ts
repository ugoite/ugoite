// REQ-API-001: Space CRUD
// REQ-API-002: Entry CRUD
import { beforeEach, describe, expect, it } from "vitest";
import { http, HttpResponse } from "msw";
import { assetApi } from "./ugoite-client";
import { formApi } from "./ugoite-client";
import { entryApi } from "./ugoite-client";
import { spaceApi } from "./ugoite-client";
import { joinUrl } from "./api";
import { UgoiteApiError } from "./ugoite-client/protocol";
import { resetMockData, seedEntry, seedSpace } from "~/test/mocks/handlers";
import { server } from "~/test/mocks/server";
import type { Entry, EntryRecord, Space } from "./types";
import { testApiUrl } from "~/test/http-origin";

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
      const ws1: Space = {
        space_uid: "ws1",
        name: "Space 1",
        created_at: "2025-01-01T00:00:00Z",
      };
      const ws2: Space = {
        space_uid: "ws2",
        name: "Space 2",
        created_at: "2025-01-02T00:00:00Z",
      };
      seedSpace(ws1);
      seedSpace(ws2);

      const spaces = await spaceApi.list();
      expect(spaces).toHaveLength(2);
      expect(spaces.map((w) => w.space_uid)).toContain("ws1");
      expect(spaces.map((w) => w.space_uid)).toContain("ws2");
    });
  });

  describe("create", () => {
    it("should create a new space", async () => {
      const result = await spaceApi.create({
        name: "my-space",
        slug: "my-space",
      });
      expect(result.space_uid).toBe("my-space");
      expect(result.name).toBe("my-space");

      // Verify it exists
      const spaces = await spaceApi.list();
      expect(spaces).toHaveLength(1);
    });

    it("should throw error for duplicate space", async () => {
      await spaceApi.create({ name: "my-space", slug: "my-space" });
      await expect(spaceApi.create({ name: "my-space", slug: "my-space" }))
        .rejects.toThrow(
          "already exists",
        );
    });

    it("should surface validation errors without object placeholders [REQ-FE-043]", async () => {
      server.use(
        http.post(testApiUrl("/spaces"), () =>
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
          )),
      );

      await expect(spaceApi.create({ name: "", slug: "" })).rejects.toThrow(
        "Input should be at least 1 character",
      );
      await expect(spaceApi.create({ name: "", slug: "" })).rejects.not.toThrow(
        "[object Object]",
      );
    });
  });

  describe("patch and test connection", () => {
    it("patches space metadata and settings", async () => {
      await spaceApi.create({ name: "patched", slug: "patched" });
      const updated = await spaceApi.patch("patched", {
        storage_config: { uri: "file:///tmp/data" },
        settings: { default_form: "Meeting" },
      });

      expect(updated.storage_config?.uri).toBe("file:///tmp/data");
      expect(updated.settings?.default_form).toBe("Meeting");
    });

    it("tests storage connection", async () => {
      await spaceApi.create({ name: "patched", slug: "patched" });
      const result = await spaceApi.testConnection("patched", {
        storage_config: { uri: "file:///tmp/data" },
      });
      expect(result.status).toBe("ok");
    });
  });
});

describe("entryApi", () => {
  const testSpace: Space = {
    space_uid: "test-ws",
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
        updated_at: "2025-01-01T00:00:00Z",
        properties: {},
        tags: [],
      };
      seedEntry("test-ws", entry, record);

      const entries = await entryApi.list("test-ws");
      expect(entries).toHaveLength(1);
      expect(entries[0].id).toBe("entry-1");
    });

    it("REQ-FE-054: entryApi normalizes unix-second timestamps for entry lists", async () => {
      server.use(
        http.get(
          testApiUrl("/spaces/test-ws/entries"),
          () =>
            HttpResponse.json([
              {
                id: "entry-1",
                created_at: 1772960822.056,
                updated_at: 1772960822.056,
                properties: {},
                tags: [],
              },
            ]),
        ),
      );

      const entries = await entryApi.list("test-ws");
      expect(entries[0].created_at).toBe(
        new Date(1772960822.056 * 1000).toISOString(),
      );
      expect(entries[0].updated_at).toBe(
        new Date(1772960822.056 * 1000).toISOString(),
      );
    });

    it("forwards limit and offset for durable entry list pages", async () => {
      server.use(
        http.get(
          testApiUrl("/spaces/test-ws/entries"),
          ({ request }) => {
            const url = new URL(request.url);
            expect(url.searchParams.get("limit")).toBe("25");
            expect(url.searchParams.get("offset")).toBe("50");
            return HttpResponse.json([]);
          },
        ),
      );

      await expect(entryApi.list("test-ws", 25, 50)).resolves.toEqual([]);
    });
  });

  describe("create", () => {
    it("should create a structured entry", async () => {
      const result = await entryApi.create("test-ws", {
        form: "Meeting",
        fields: { Date: "2025-01-15", Attendees: "Alice, Bob" },
      });

      expect(result.id).toBeDefined();
      expect(result.revision_id).toBeDefined();

      // Verify the structured fields are indexed without a Markdown authoring step.
      const entries = await entryApi.list("test-ws");
      expect(entries).toHaveLength(1);
      expect(entries[0].id).toBe(result.id);
      expect(entries[0].properties).toHaveProperty("Date");
      expect(entries[0].properties).toHaveProperty("Attendees");
    });
  });

  describe("get", () => {
    it("should return full entry fields", async () => {
      const fields = { Body: "With body content" };
      server.use(
        http.get(
          testApiUrl("/spaces/test-ws/entries/entry-get"),
          () =>
            HttpResponse.json({
              id: "entry-get",
              fields,
              revision_id: "rev-get",
              created_at: "2025-01-01T00:00:00Z",
              updated_at: "2025-01-01T00:00:00Z",
            }),
        ),
        http.get(
          testApiUrl("/spaces/test-ws/entries/entry-empty"),
          () =>
            HttpResponse.json({
              id: "entry-empty",
              fields: {},
              revision_id: "rev-empty",
              created_at: "2025-01-01T00:00:00Z",
              updated_at: "2025-01-01T00:00:00Z",
            }),
        ),
      );

      const fetched = await entryApi.get("test-ws", "entry-get");
      expect(fetched.fields).toEqual(fields);
      expect(fetched.revision_id).toBe("rev-get");

      const emptyFetched = await entryApi.get("test-ws", "entry-empty");
      expect(emptyFetched.fields).toEqual({});
    });

    it("should throw error for non-existent entry", async () => {
      await expect(entryApi.get("test-ws", "non-existent")).rejects.toThrow();
    });
  });

  describe("update", () => {
    it("should update entry with correct parent_revision_id", async () => {
      const createResult = await entryApi.create("test-ws", {
        form: "Task",
        fields: { Status: "Draft" },
      });

      const updateResult = await entryApi.update("test-ws", createResult.id, {
        form: "Task",
        fields: { Status: "Published" },
        parent_revision_id: createResult.revision_id,
      });

      expect(updateResult.revision_id).not.toBe(createResult.revision_id);

      // Verify index was updated
      const entries = await entryApi.list("test-ws");
      const entry = entries.find((n) => n.id === createResult.id);
      expect(entry?.id).toBe(createResult.id);
      expect(entry?.properties.Status).toBe("Published");
    });

    it("should throw RevisionConflictError (409) on revision mismatch", async () => {
      const createResult = await entryApi.create("test-ws", {
        form: "Task",
        fields: { Status: "Draft" },
      });

      // First update succeeds
      await entryApi.update("test-ws", createResult.id, {
        form: "Task",
        fields: { Status: "Published" },
        parent_revision_id: createResult.revision_id,
      });

      // Second update with stale revision should fail
      await expect(
        entryApi.update("test-ws", createResult.id, {
          form: "Task",
          fields: { Status: "Stale" },
          parent_revision_id: createResult.revision_id, // Stale!
        }),
      ).rejects.toMatchObject({
        name: "RevisionConflictError",
        currentRevisionId: expect.any(String),
      });
    });

    it("reads current_revision_id from canonical nested 409 detail", async () => {
      server.use(
        http.put(
          testApiUrl("/spaces/test-ws/entries/nested-conflict"),
          () =>
            HttpResponse.json(
              {
                code: "REVISION_CONFLICT",
                message: "Revision conflict",
                detail: { current_revision_id: "server-rev-nested" },
              },
              { status: 409 },
            ),
        ),
      );

      await expect(
        entryApi.update("test-ws", "nested-conflict", {
          form: "Task",
          fields: { Status: "Stale" },
          parent_revision_id: "client-rev",
        }),
      ).rejects.toMatchObject({
        name: "RevisionConflictError",
        currentRevisionId: "server-rev-nested",
      });
    });
  });

  describe("delete", () => {
    it("should remove entry from list", async () => {
      const result = await entryApi.create("test-ws", {
        form: "Task",
        fields: { Status: "Draft" },
      });

      let entries = await entryApi.list("test-ws");
      expect(entries).toHaveLength(1);

      await entryApi.delete("test-ws", result.id);

      entries = await entryApi.list("test-ws");
      expect(entries).toHaveLength(0);
    });
  });

  describe("entry query collection reads", () => {
    it("queries entries by canonical text", async () => {
      const created = await entryApi.create("test-ws", {
        form: "Project",
        fields: { Notes: "Rocket Project entries about propulsion" },
      });

      server.use(
        http.post(
          testApiUrl("/spaces/test-ws/entries/query"),
          async ({ request }) => {
            const body = await request.json() as {
              query?: { text?: string };
            };
            expect(body.query?.text).toBe("rocket");
            return HttpResponse.json({
              rows: [{
                id: created.id,
                form_id: "form-1",
                revision_id: "rev-1",
                created_at_micros: 1_000_000,
                updated_at_micros: 1_000_000,
                preview: "Rocket Project",
              }],
              has_more: false,
            });
          },
        ),
      );

      const page = await entryApi.query("test-ws", {
        query: {
          scope: { kind: "all" },
          text: "rocket",
          filters: [],
          sort: [],
        },
        projection: { kind: "preview" },
        limit: 50,
      });
      expect(page.rows.find((row) => row.id === created.id)).toBeDefined();
    });

    it("forwards limit for entry query pages", async () => {
      server.use(
        http.post(
          testApiUrl("/spaces/test-ws/entries/query"),
          async ({ request }) => {
            const body = await request.json() as { limit?: number };
            expect(body.limit).toBe(25);
            return HttpResponse.json({ rows: [], has_more: false });
          },
        ),
      );

      await expect(entryApi.query("test-ws", {
        query: { scope: { kind: "all" }, filters: [], sort: [] },
        projection: { kind: "preview" },
        limit: 25,
      })).resolves.toEqual({ rows: [], has_more: false });
    });

    it("forwards limit and offset for entry history pages", async () => {
      server.use(
        http.get(
          testApiUrl("/spaces/test-ws/entries/entry-1/history"),
          ({ request }) => {
            const url = new URL(request.url);
            expect(url.searchParams.get("limit")).toBe("25");
            expect(url.searchParams.get("offset")).toBe("50");
            return HttpResponse.json({ revisions: [] });
          },
        ),
      );

      await expect(entryApi.history("test-ws", "entry-1", undefined, 25, 50))
        .resolves.toEqual({ revisions: [] });
    });

    it("REQ-FE-065: row_reference picker pages form-scoped entries via entry.query", async () => {
      server.use(
        http.post(
          testApiUrl("/spaces/test-ws/entries/query"),
          async ({ request }) => {
            const body = await request.json() as {
              query?: { scope?: { kind?: string } };
              limit?: number;
            };
            expect(body.query?.scope?.kind).toBe("form");
            expect(body.limit).toBe(8);
            return HttpResponse.json({
              rows: [{
                id: "project-alpha",
                form_id: "form-1",
                revision_id: "rev-1",
                created_at_micros: 1_000_000,
                updated_at_micros: 1_000_000,
                preview: "Project Alpha",
              }],
              has_more: false,
            });
          },
        ),
      );

      const page = await entryApi.query("test-ws", {
        query: {
          scope: { kind: "form", form_id: "form-1" },
          text: "alpha",
          filters: [],
          sort: [],
        },
        projection: { kind: "preview" },
        limit: 8,
      });
      expect(page.rows).toEqual([{
        id: "project-alpha",
        form_id: "form-1",
        revision_id: "rev-1",
        created_at_micros: 1_000_000,
        updated_at_micros: 1_000_000,
        preview: "Project Alpha",
      }]);
    });

    it("uploads a typed asset reference and deletes by stable asset id", async () => {
      const file = new File(["data"], "voice.m4a", { type: "audio/m4a" });
      const asset = await assetApi.upload("test-ws", file);
      expect(asset.asset_id).toBeTruthy();
      await assetApi.delete("test-ws", asset.asset_id);
    });

    it("reads asset bytes with the containing Form and Entry context", async () => {
      const file = new File(["data"], "contract.pdf", {
        type: "application/pdf",
      });
      const asset = await assetApi.upload("test-ws", file);
      const bytes = await assetApi.read(
        "test-ws",
        asset.asset_id,
        "Contracts",
        "entry-1",
      );

      expect(bytes).toBeInstanceOf(Blob);
      expect(bytes.type).toBe("application/octet-stream");
    });

    it("#2824: rejects a context-free asset read with ASSET_CONTEXT_REQUIRED", async () => {
      resetMockData();
      seedSpace({
        space_uid: "ws-ctx",
        name: "Context Space",
        created_at: "2025-01-01T00:00:00Z",
      });
      // Every partial query combination fails closed with the same additive
      // machine code and a context-only message (no hidden references).
      for (
        const query of ["", "?entry_id=some-entry", "?form=Doc"]
      ) {
        const response = await fetch(
          testApiUrl(`/spaces/ws-ctx/assets/asset-1${query}`),
        );
        expect(response.status).toBe(403);
        const body = await response.json() as Record<string, unknown>;
        expect(body.code).toBe("ASSET_CONTEXT_REQUIRED");
        expect(body.message).toBe(
          "asset reads require a containing Form and Entry context",
        );
      }
    });

    it("#2824: surfaces the ASSET_CONTEXT_REQUIRED code through assetApi", async () => {
      server.use(
        http.get(
          testApiUrl("/spaces/ws-asset-ctx/assets/asset-1"),
          () =>
            HttpResponse.json({
              code: "ASSET_CONTEXT_REQUIRED",
              message:
                "asset reads require a containing Form and Entry context",
            }, { status: 403 }),
        ),
      );
      const failure = await assetApi.read(
        "ws-asset-ctx",
        "asset-1",
        "Contracts",
        "entry-1",
      ).catch((error: unknown) => error);
      expect(failure).toBeInstanceOf(UgoiteApiError);
      expect((failure as UgoiteApiError).code).toBe(
        "ASSET_CONTEXT_REQUIRED",
      );
    });
  });
});

describe("formApi", () => {
  const testSpace: Space = {
    space_uid: "form-ws",
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
      allow_extra_attributes: "allow_columns",
    });

    const forms = await formApi.list("form-ws");
    expect(forms.find((s) => s.name === "Meeting")).toBeDefined();

    const fetched = await formApi.get("form-ws", "Meeting");
    expect(fetched.name).toBe("Meeting");
    expect(fetched.fields.Date.type).toBe("date");
    expect(fetched.allow_extra_attributes).toBe("allow_columns");
  });
});

describe("joinUrl", () => {
  it("joins base and path", () => {
    expect(joinUrl("http://example.com", "/foo")).toBe(
      "http://example.com/foo",
    );
  });

  it("returns path when base is empty", () => {
    expect(joinUrl("", "/bar")).toBe("/bar");
  });

  it("handles trailing slash on base", () => {
    expect(joinUrl("http://example.com/", "/foo")).toBe(
      "http://example.com/foo",
    );
  });

  it("handles missing leading slash on path", () => {
    expect(joinUrl("http://example.com", "foo")).toBe("http://example.com/foo");
  });
});

describe("spaceApi members", () => {
  it("lists members", async () => {
    resetMockData();
    await spaceApi.create({ name: "ws-members", slug: "ws-members" });
    const members = await spaceApi.listMembers("ws-members");
    expect(Array.isArray(members)).toBe(true);
  });

  it("invites a member", async () => {
    resetMockData();
    await spaceApi.create({ name: "ws-invite", slug: "ws-invite" });
    const result = await spaceApi.inviteMember("ws-invite", {
      label: "User One",
      role: "editor",
    });
    expect(result.invitation_url).toContain("test-token");
  });

  it("updates member role", async () => {
    resetMockData();
    await spaceApi.create({ name: "ws-role", slug: "ws-role" });
    const result = await spaceApi.updateMemberRole("ws-role", "user1", {
      role: "viewer",
    });
    expect(result.role).toBe("viewer");
  });

  it("revokes member", async () => {
    resetMockData();
    await spaceApi.create({ name: "ws-revoke", slug: "ws-revoke" });
    const result = await spaceApi.revokeMember("ws-revoke", "user1");
    expect(result.state).toBe("revoked");
  });
});

describe("error paths", () => {
  it("spaceApi.list throws on failure", async () => {
    server.use(
      http.get(
        testApiUrl("/spaces"),
        () => HttpResponse.json({ detail: "Error" }, { status: 500 }),
      ),
    );
    await expect(spaceApi.list()).rejects.toThrow("Failed to list spaces");
  });

  it("spaceApi.get throws on failure", async () => {
    server.use(
      http.get(
        testApiUrl("/spaces/nonexistent"),
        () => HttpResponse.json({ detail: "Not found" }, { status: 404 }),
      ),
    );
    await expect(spaceApi.get("nonexistent")).rejects.toThrow(
      "Failed to get space",
    );
  });

  it("spaceApi.get returns a space", async () => {
    resetMockData();
    await spaceApi.create({ name: "existing-space", slug: "existing-space" });
    const space = await spaceApi.get("existing-space");
    expect(space.space_uid).toBe("existing-space");
  });

  it("spaceApi.patch throws on failure", async () => {
    server.use(
      http.patch(
        testApiUrl("/spaces/nonexistent"),
        () => HttpResponse.json({ detail: "Space not found" }, { status: 404 }),
      ),
    );
    await expect(spaceApi.patch("nonexistent", {})).rejects.toThrow(
      "Space not found",
    );
  });

  it("spaceApi.testConnection throws on failure", async () => {
    server.use(
      http.post(
        testApiUrl("/spaces/nonexistent/test-connection"),
        () => HttpResponse.json({ detail: "Not found" }, { status: 404 }),
      ),
    );
    await expect(
      spaceApi.testConnection("nonexistent", { storage_config: { uri: "x" } }),
    ).rejects.toThrow();
  });

  it("spaceApi.listMembers throws on failure", async () => {
    server.use(
      http.get(
        testApiUrl("/spaces/nonexistent/members"),
        () => HttpResponse.json({ detail: "Not found" }, { status: 404 }),
      ),
    );
    await expect(spaceApi.listMembers("nonexistent")).rejects.toThrow();
  });

  it("spaceApi.inviteMember throws on failure", async () => {
    server.use(
      http.post(
        testApiUrl("/spaces/nonexistent/members/invitations"),
        () => HttpResponse.json({ detail: "Not found" }, { status: 404 }),
      ),
    );
    await expect(
      spaceApi.inviteMember("nonexistent", {
        label: "User One",
        role: "editor",
      }),
    ).rejects.toThrow();
  });

  it("spaceApi.updateMemberRole throws on failure", async () => {
    server.use(
      http.post(
        testApiUrl("/spaces/nonexistent/members/u1/role"),
        () => HttpResponse.json({ detail: "Not found" }, { status: 404 }),
      ),
    );
    await expect(
      spaceApi.updateMemberRole("nonexistent", "u1", { role: "viewer" }),
    ).rejects.toThrow();
  });

  it("spaceApi.revokeMember throws on failure", async () => {
    server.use(
      http.delete(
        testApiUrl("/spaces/nonexistent/members/u1"),
        () => HttpResponse.json({ detail: "Not found" }, { status: 404 }),
      ),
    );
    await expect(spaceApi.revokeMember("nonexistent", "u1")).rejects.toThrow();
  });

  it("entryApi.history returns revisions", async () => {
    resetMockData();
    seedSpace({
      space_uid: "ws-history",
      name: "H",
      created_at: "2025-01-01T00:00:00Z",
    });
    const created = await entryApi.create("ws-history", {
      form: "Entry",
      fields: { Body: "Entry" },
    });
    const history = await entryApi.history("ws-history", created.id);
    expect(history.revisions).toBeDefined();
  });

  it("entryApi.getRevision returns a revision", async () => {
    resetMockData();
    seedSpace({
      space_uid: "ws-rev",
      name: "R",
      created_at: "2025-01-01T00:00:00Z",
    });
    const created = await entryApi.create("ws-rev", {
      form: "Entry",
      fields: { Body: "Entry" },
    });
    const entry = await entryApi.get("ws-rev", created.id);
    const revision = await entryApi.getRevision(
      "ws-rev",
      created.id,
      entry.revision_id,
    );
    expect(revision.revision_id).toBe(entry.revision_id);
  });

  it("entryApi.restore succeeds", async () => {
    resetMockData();
    seedSpace({
      space_uid: "ws-restore",
      name: "RR",
      created_at: "2025-01-01T00:00:00Z",
    });
    const created = await entryApi.create("ws-restore", {
      form: "Entry",
      fields: { Body: "Entry" },
    });
    const entry = await entryApi.get("ws-restore", created.id);
    const restored = await entryApi.restore(
      "ws-restore",
      created.id,
      entry.revision_id,
    );
    expect(restored).toBeDefined();
  });

  it("entryApi.createFromWebform creates entry", async () => {
    resetMockData();
    seedSpace({
      space_uid: "ws-wf",
      name: "WF",
      created_at: "2025-01-01T00:00:00Z",
    });
    const formDef = {
      name: "Task",
      template: "# Task\n\n## Status\n",
      fields: { Status: { type: "text" } },
    };
    const result = await entryApi.createFromWebform(
      "ws-wf",
      formDef as never,
      {
        Status: "Open",
      },
    );
    expect(result.id).toBeDefined();
  });

  it("entryApi.createFromChat creates entry", async () => {
    resetMockData();
    seedSpace({
      space_uid: "ws-chat",
      name: "Chat",
      created_at: "2025-01-01T00:00:00Z",
    });
    const formDef = {
      name: "Task",
      template: "# Task\n\n## Status\n",
      fields: { Status: { type: "text" } },
    };
    const result = await entryApi.createFromChat(
      "ws-chat",
      formDef as never,
      {
        Status: "Pending",
      },
    );
    expect(result.id).toBeDefined();
  });

  it("entryApi.createFromChat uses the same structured payload as webform", async () => {
    resetMockData();
    seedSpace({
      space_uid: "ws-parity",
      name: "Parity",
      created_at: "2025-01-01T00:00:00Z",
    });
    const formDef = {
      name: "Task",
      template: "# Task\n\n## Status\n",
      fields: {
        Status: { type: "text" },
        Zoned: { type: "timestamp_tz" },
        Row: { type: "row_reference" },
        File: { type: "asset_reference" },
        Tags: { type: "list" },
        Rows: { type: "object_list" },
      },
    };
    const answers = {
      Status: "  Pending  ",
      Zoned: "2026-08-21T10:48",
      Row: "entry-01",
      File: {
        asset_id: "01900000-0000-7000-8000-000000000001",
        name: "report.pdf",
        media_type: "application/pdf",
        size_bytes: 10,
        sha256: "a".repeat(64),
      },
      Tags: ["alpha", "beta"],
      Rows: [{ label: "one" }],
      __control: "drop",
      Blank: "   ",
    };
    const calls: unknown[][] = [];
    const original = entryApi.create;
    entryApi.create = (async (...args: unknown[]) => {
      calls.push(args);
      return await (original as (
        ...a: never[]
      ) => Promise<{ id: string; revision_id: string }>)(...(args as never[]));
    }) as typeof entryApi.create;
    try {
      await entryApi.createFromWebform(
        "ws-parity",
        formDef as never,
        answers,
      );
      await entryApi.createFromChat(
        "ws-parity",
        formDef as never,
        answers,
      );
    } finally {
      entryApi.create = original;
    }
    expect(calls).toHaveLength(2);
    const [, webformPayload] = calls[0] as [string, Record<string, unknown>];
    const [, chatPayload] = calls[1] as [string, Record<string, unknown>];
    // Both ride structured create; no Markdown detour for chat.
    expect(webformPayload.markdown).toBeUndefined();
    expect(chatPayload.markdown).toBeUndefined();
    expect(chatPayload.form).toBe("Task");
    // Entry mutations carry only structured Form fields.
    expect(chatPayload.fields).toEqual(webformPayload.fields);
    expect(chatPayload.fields).toMatchObject({
      Status: "Pending",
      Row: "entry-01",
      File: answers.File,
      Tags: ["alpha", "beta"],
      Rows: [{ label: "one" }],
    });
    expect(chatPayload.fields).not.toHaveProperty("__control");
    expect(chatPayload.fields).not.toHaveProperty("Blank");
    expect(chatPayload.fields).toHaveProperty(
      "Zoned",
      expect.stringMatching(/^2026-08-21T10:48:00[+-]\d{2}:\d{2}$/),
    );
  });

  it("entryApi.get includes detail in error", async () => {
    server.use(
      http.get(
        testApiUrl("/spaces/ws-err/entries/bad-id"),
        () =>
          HttpResponse.json({ detail: "Custom error detail" }, { status: 404 }),
      ),
    );
    await expect(entryApi.get("ws-err", "bad-id")).rejects.toThrow(
      "Custom error detail",
    );
  });

  it("entryApi.get uses statusText fallback when no detail in error", async () => {
    server.use(
      http.get(
        testApiUrl("/spaces/ws-err/entries/no-detail-entry"),
        () => HttpResponse.json({ message: "Generic error" }, { status: 404 }),
      ),
    );
    await expect(entryApi.get("ws-err", "no-detail-entry")).rejects.toThrow(
      "Failed to get entry",
    );
  });

  it("entryApi.update throws generic error on non-409 failure", async () => {
    server.use(
      http.put(
        testApiUrl("/spaces/ws-err/entries/bad-id"),
        () => HttpResponse.json({ detail: "Server error" }, { status: 500 }),
      ),
    );
    await expect(
      entryApi.update("ws-err", "bad-id", {
        form: "Note",
        fields: { Body: "X" },
        parent_revision_id: "r1",
      }),
    ).rejects.toThrow("Server error");
  });

  it("entryApi.delete throws on failure", async () => {
    server.use(
      http.delete(
        testApiUrl("/spaces/ws-err/entries/bad-id"),
        () => HttpResponse.json({ detail: "Not found" }, { status: 404 }),
      ),
    );
    await expect(entryApi.delete("ws-err", "bad-id")).rejects.toThrow(
      "Failed to delete entry",
    );
  });

  it("entryApi.restore throws on failure", async () => {
    server.use(
      http.post(
        testApiUrl("/spaces/ws-err/entries/bad-id/restore"),
        () => HttpResponse.json({ detail: "Restore failed" }, { status: 500 }),
      ),
    );
    await expect(entryApi.restore("ws-err", "bad-id", "rev-1")).rejects.toThrow(
      "Restore failed",
    );
  });

  it("entryApi.history throws on failure", async () => {
    server.use(
      http.get(
        testApiUrl("/spaces/ws-err/entries/bad-id/history"),
        () => HttpResponse.json({ detail: "Not found" }, { status: 404 }),
      ),
    );
    await expect(entryApi.history("ws-err", "bad-id")).rejects.toThrow(
      "Failed to get entry history",
    );
  });

  it("entryApi.getRevision throws on failure", async () => {
    server.use(
      http.get(
        testApiUrl("/spaces/ws-err/entries/bad-id/history/rev-1"),
        () => HttpResponse.json({ detail: "Not found" }, { status: 404 }),
      ),
    );
    await expect(entryApi.getRevision("ws-err", "bad-id", "rev-1")).rejects
      .toThrow(
        "Failed to get entry revision",
      );
  });

  it("assetApi.upload throws on failure", async () => {
    server.use(
      http.post(
        testApiUrl("/spaces/ws-asset-err/assets"),
        () => HttpResponse.json({ detail: "Error" }, { status: 500 }),
      ),
    );
    const file = new File(["data"], "test.txt");
    await expect(assetApi.upload("ws-asset-err", file)).rejects.toThrow(
      "Failed to upload asset",
    );
  });

  it("assetApi.delete throws with detail on failure", async () => {
    server.use(
      http.delete(
        testApiUrl("/spaces/ws-asset-err/assets/bad-id"),
        () =>
          HttpResponse.json({ detail: "Asset is referenced" }, { status: 409 }),
      ),
    );
    await expect(assetApi.delete("ws-asset-err", "bad-id")).rejects.toThrow(
      "Asset is referenced",
    );
  });

  it("formApi.listTypes returns types", async () => {
    resetMockData();
    seedSpace({
      space_uid: "ws-types",
      name: "T",
      created_at: "2025-01-01T00:00:00Z",
    });
    const types = await formApi.listTypes("ws-types");
    expect(Array.isArray(types)).toBe(true);
  });

  it("formApi.listTypes throws on failure", async () => {
    server.use(
      http.get(
        testApiUrl("/spaces/ws-types-err/forms/types"),
        () => HttpResponse.json({ detail: "Error" }, { status: 500 }),
      ),
    );
    await expect(formApi.listTypes("ws-types-err")).rejects.toThrow(
      "Failed to list form types",
    );
  });

  it("formApi.list throws on failure", async () => {
    server.use(
      http.get(
        testApiUrl("/spaces/ws-form-err/forms"),
        () => HttpResponse.json({ detail: "Error" }, { status: 500 }),
      ),
    );
    await expect(formApi.list("ws-form-err")).rejects.toThrow(
      "Failed to list forms",
    );
  });

  it("formApi.get throws on failure", async () => {
    server.use(
      http.get(
        testApiUrl("/spaces/ws-form-err/forms/nonexistent"),
        () => HttpResponse.json({ detail: "Not found" }, { status: 404 }),
      ),
    );
    await expect(formApi.get("ws-form-err", "nonexistent")).rejects.toThrow(
      "Failed to get form",
    );
  });

  it("formApi.create throws on failure", async () => {
    server.use(
      http.post(
        testApiUrl("/spaces/ws-form-err/forms"),
        () => HttpResponse.json({ detail: "Invalid" }, { status: 422 }),
      ),
    );
    await expect(
      formApi.create("ws-form-err", {
        name: "Bad",
        version: 1,
        template: "",
        fields: {},
      }),
    ).rejects.toThrow("Failed to save form");
  });

  it("entryApi.query throws on failure", async () => {
    server.use(
      http.post(
        testApiUrl("/spaces/ws-search-err/entries/query"),
        () => HttpResponse.json({ detail: "Error" }, { status: 500 }),
      ),
    );
    await expect(entryApi.query("ws-search-err", {
      query: { scope: { kind: "all" }, filters: [], sort: [] },
      projection: { kind: "preview" },
      limit: 50,
    })).rejects.toThrow(
      "Failed to query entries",
    );
  });

  it("entryApi.count throws on failure", async () => {
    server.use(
      http.post(
        testApiUrl("/spaces/ws-search-err/entries/query/count"),
        () => HttpResponse.json({ detail: "Error" }, { status: 500 }),
      ),
    );
    await expect(entryApi.count("ws-search-err", {
      query: { scope: { kind: "all" }, filters: [], sort: [] },
    })).rejects.toThrow(
      "Failed to count entries",
    );
  });

  it("spaceApi.create uses fallback message when error response has no detail", async () => {
    server.use(
      http.post(
        testApiUrl("/spaces"),
        () => HttpResponse.json({ message: "No detail here" }, { status: 422 }),
      ),
    );
    await expect(
      spaceApi.create({ name: "test-no-detail", slug: "test-no-detail" }),
    ).rejects.toThrow(
      "Failed to create space",
    );
  });
});
