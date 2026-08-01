// REQ-FE-050: SQL query management
import { beforeEach, describe, expect, it } from "vitest";
import { http, HttpResponse } from "msw";
import { sqlApi } from "./ugoite-client";
import { resetMockData, seedSpace } from "~/test/mocks/handlers";
import { server } from "~/test/mocks/server";
import type { Space } from "./types";
import type { SqlUpdatePayload } from "./types";
import { testApiUrl } from "~/test/http-origin";

const testSpace: Space = {
  id: "sql-ws",
  name: "SQL Space",
  created_at: "2025-01-01T00:00:00Z",
};

describe("sqlApi", () => {
  beforeEach(() => {
    resetMockData();
    seedSpace(testSpace);
  });

  it("lists SQL entries (empty by default)", async () => {
    const entries = await sqlApi.list("sql-ws");
    expect(entries).toEqual([]);
  });

  it("REQ-FE-054: sqlApi normalizes unix-second timestamps for saved queries", async () => {
    server.use(
      http.get(testApiUrl("/spaces/sql-ws/sql"), () =>
        HttpResponse.json([
          {
            id: "query-1",
            name: "Recent Query",
            kind: "user-query",
            metadata: null,
            sql: "SELECT 1",
            variables: [],
            created_at: 1772960822.056,
            updated_at: 1772960822.056,
            revision_id: "rev-1",
          },
        ])),
    );

    const [entry] = await sqlApi.list("sql-ws");
    expect(entry.created_at).toBe(
      new Date(1772960822.056 * 1000).toISOString(),
    );
    expect(entry.updated_at).toBe(
      new Date(1772960822.056 * 1000).toISOString(),
    );
  });

  it("creates a SQL entry and returns id/revisionId", async () => {
    const result = await sqlApi.create("sql-ws", {
      name: "My Query",
      kind: "user-query",
      sql: "SELECT 1",
      variables: [],
    });
    expect(result.id).toBeDefined();
    expect(result.revisionId).toBeDefined();
  });

  it("gets a SQL entry by id", async () => {
    const created = await sqlApi.create("sql-ws", {
      name: "Get Test",
      kind: "user-query",
      sql: "SELECT 2",
      variables: [],
    });
    const fetched = await sqlApi.get("sql-ws", created.id);
    expect(fetched.name).toBe("Get Test");
    expect(fetched.sql).toBe("SELECT 2");
  });

  it("updates a SQL entry", async () => {
    const created = await sqlApi.create("sql-ws", {
      name: "Orig",
      kind: "user-query",
      sql: "SELECT 1",
      variables: [],
    });
    const result = await sqlApi.update("sql-ws", created.id, {
      name: "Updated",
      kind: "user-query",
      sql: "SELECT 2",
      variables: [],
      parent_revision_id: created.revisionId,
    });
    expect(result.id).toBe(created.id);
    expect(result.revisionId).toBeDefined();
  });

  it("enforces the saved SQL update revision contract in the mock server", async () => {
    const created = await sqlApi.create("sql-ws", {
      name: "Revisioned",
      kind: "user-query",
      sql: "SELECT 1",
      variables: [],
    });

    const updated = await sqlApi.update("sql-ws", created.id, {
      name: "Revisioned again",
      kind: "user-query",
      sql: "SELECT 2",
      variables: [],
      parent_revision_id: created.revisionId,
    });
    expect(updated.revisionId).not.toBe(created.revisionId);

    await expect(sqlApi.update("sql-ws", created.id, {
      name: "Stale",
      kind: "user-query",
      sql: "SELECT 3",
      variables: [],
      parent_revision_id: created.revisionId,
    })).rejects.toMatchObject({ status: 409, code: "REVISION_CONFLICT" });
  });

  it("rejects unknown top-level saved SQL update fields", async () => {
    const created = await sqlApi.create("sql-ws", {
      name: "Strict payload",
      kind: "user-query",
      sql: "SELECT 1",
      variables: [],
    });
    const invalidPayload = {
      name: "Strict payload",
      kind: "user-query" as const,
      sql: "SELECT 2",
      variables: [],
      author: "unexpected",
    } as unknown as SqlUpdatePayload;

    await expect(sqlApi.update("sql-ws", created.id, invalidPayload))
      .rejects.toThrow("unknown field: author");
  });

  it("deletes a SQL entry", async () => {
    const created = await sqlApi.create("sql-ws", {
      name: "ToDelete",
      kind: "user-query",
      sql: "SELECT 1",
      variables: [],
    });
    await expect(sqlApi.delete("sql-ws", created.id)).resolves.toBeUndefined();
  });

  it("throws on list failure", async () => {
    server.use(
      http.get(
        testApiUrl("/spaces/sql-ws/sql"),
        () => HttpResponse.json({ detail: "Server error" }, { status: 500 }),
      ),
    );
    await expect(sqlApi.list("sql-ws")).rejects.toThrow(
      "Failed to list saved SQL",
    );
  });

  it("throws on get failure", async () => {
    server.use(
      http.get(
        testApiUrl("/spaces/sql-ws/sql/bad-id"),
        () => HttpResponse.json({ detail: "Not found" }, { status: 404 }),
      ),
    );
    await expect(sqlApi.get("sql-ws", "bad-id")).rejects.toThrow(
      "Failed to get saved SQL",
    );
  });

  it("throws on create failure with detail", async () => {
    server.use(
      http.post(
        testApiUrl("/spaces/sql-ws/sql"),
        () => HttpResponse.json({ detail: "Invalid SQL" }, { status: 422 }),
      ),
    );
    await expect(sqlApi.create("sql-ws", {
      name: "Bad",
      kind: "user-query",
      sql: "SELECT",
      variables: [],
    }))
      .rejects.toThrow(
        "Invalid SQL",
      );
  });

  it("throws on update failure with detail", async () => {
    server.use(
      http.put(
        testApiUrl("/spaces/sql-ws/sql/bad-id"),
        () => HttpResponse.json({ detail: "Update failed" }, { status: 500 }),
      ),
    );
    await expect(sqlApi.update("sql-ws", "bad-id", {
      name: "X",
      kind: "user-query",
      sql: "SELECT 1",
      variables: [],
    })).rejects.toThrow("Update failed");
  });

  it("throws on delete failure", async () => {
    server.use(
      http.delete(
        testApiUrl("/spaces/sql-ws/sql/bad-id"),
        () => HttpResponse.json({ detail: "Not found" }, { status: 404 }),
      ),
    );
    await expect(sqlApi.delete("sql-ws", "bad-id")).rejects.toThrow(
      "Failed to delete saved SQL",
    );
  });
});
