import { describe, expect, it, vi } from "vitest";
import { compositionApi } from "./composition-api";
import {
  type CompositionQueryApi,
  createCompositionQueryHandle,
} from "./composition-query-handle";
import { sqlApi } from "./sql-api";

vi.mock("./sql-api", () => ({ sqlApi: { query: vi.fn() } }));

const identity = {
  spaceId: "space-1",
  compositionId: "composition-1",
  revisionId: "revision-1",
};

const resolved = (parameter: unknown) => ({
  ok: true,
  parameter_definitions: [{
    id: "month",
    type: "string" as const,
    required: true,
  }],
  plan: {
    composition_revision: {
      entry_id: identity.compositionId,
      revision_id: identity.revisionId,
    },
    sources: [{
      kind: "entry_query" as const,
      source_id: "entries",
      source_schema_fingerprint: "fingerprint",
      request: {
        query: {
          scope: { kind: "form" as const, form_id: "form-1" },
          filters: [{
            field: { kind: "property" as const, field_id: 7 },
            operator: "equals" as const,
            value: parameter,
          }],
          sort: [],
        },
        projection: {
          kind: "fields" as const,
          fields: [{ kind: "property" as const, field_id: 7 }],
        },
        limit: 10,
      },
    }],
    component_bindings: [],
  },
  diagnostics: [],
});

const entryPage = (id: string) => ({
  kind: "entry_query" as const,
  page: {
    rows: [{
      id,
      form_id: "form-1",
      revision_id: `revision-${id}`,
      created_at_micros: 1,
      updated_at_micros: 1,
      properties: { name: id },
    }],
    has_more: false,
  },
});

const resolvedSavedSql = (monthStart: string) => ({
  ok: true,
  parameter_definitions: [{
    id: "month_start",
    type: "date" as const,
    required: true,
    default: "2026-03-01",
  }],
  plan: {
    composition_revision: {
      entry_id: identity.compositionId,
      revision_id: identity.revisionId,
    },
    sources: [{
      kind: "saved_sql" as const,
      source_id: "sql_results",
      source_schema_fingerprint: "sql-fingerprint",
      request: {
        sql: "SELECT amount FROM expenses WHERE month >= :month_start",
        parameters: { month_start: monthStart },
        parameter_types: { month_start: "date" },
        limit: 10,
        saved_sql: { id: "sql-1", revision_id: "sql-revision-4" },
      },
    }],
    component_bindings: [],
  },
  diagnostics: [],
});

const savedSqlPage = (row: string, next?: string) => ({
  kind: "saved_sql" as const,
  page: {
    columns: ["amount"],
    rows: [[row]],
    has_more: next !== undefined,
    ...(next === undefined ? {} : { next }),
  },
});

const deferred = <T>() => {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => resolve = done);
  return { promise, resolve };
};

describe("CompositionQueryHandle", () => {
  it("rejects a Composition read returned for a different Entry", async () => {
    const api: CompositionQueryApi = {
      get: vi.fn().mockResolvedValue({
        revision: {
          entry_id: "another-composition",
          revision_id: identity.revisionId,
        },
        fields: { name: "Budget" },
        unmapped_field_values: {},
      }),
      resolve: vi.fn(),
      querySource: vi.fn(),
    };
    const handle = createCompositionQueryHandle(api);

    await handle.open(identity);

    expect(api.resolve).not.toHaveBeenCalled();
    expect(handle.state().resolveError).toBeInstanceOf(Error);
    handle.dispose();
  });

  it("pins get and resolve to the selected revision and ignores a stale parameter response", async () => {
    const oldPage = deferred<ReturnType<typeof entryPage>>();
    const currentPage = deferred<ReturnType<typeof entryPage>>();
    const querySignals: AbortSignal[] = [];
    const api: CompositionQueryApi = {
      get: vi.fn().mockResolvedValue({
        revision: {
          entry_id: identity.compositionId,
          revision_id: identity.revisionId,
        },
        fields: { name: "Budget" },
        unmapped_field_values: {},
      }),
      resolve: vi.fn((_space, _composition, _revision, parameters) =>
        Promise.resolve(resolved(parameters.month))
      ),
      querySource: vi.fn((_space, _source, _cursor, signal) => {
        querySignals.push(signal);
        return querySignals.length === 1
          ? oldPage.promise
          : currentPage.promise;
      }),
    };
    const handle = createCompositionQueryHandle(api);

    const opening = handle.open(identity);
    await vi.waitFor(() => expect(api.querySource).toHaveBeenCalledTimes(1));
    handle.setParameter("month", "2026-04");
    await vi.waitFor(() => expect(api.querySource).toHaveBeenCalledTimes(2));

    expect(api.get).toHaveBeenCalledWith(
      identity.spaceId,
      identity.compositionId,
      identity.revisionId,
      expect.any(AbortSignal),
    );
    expect(api.resolve).toHaveBeenNthCalledWith(
      1,
      identity.spaceId,
      identity.compositionId,
      identity.revisionId,
      {},
      expect.any(AbortSignal),
    );
    expect(api.resolve).toHaveBeenLastCalledWith(
      identity.spaceId,
      identity.compositionId,
      identity.revisionId,
      { month: "2026-04" },
      expect.any(AbortSignal),
    );
    expect(querySignals[0].aborted).toBe(true);

    currentPage.resolve(entryPage("current"));
    await vi.waitFor(() =>
      expect(handle.state().sources.entries?.status).toBe("ready")
    );
    oldPage.resolve(entryPage("stale"));
    await opening;

    expect(handle.state().sources.entries?.page).toEqual(entryPage("current"));
    expect(handle.parameters()).toEqual({ month: "2026-04" });
    handle.dispose();
  });

  it("uses the existing opaque Entry continuation for next and previous pages", async () => {
    const cursors: (string | undefined)[] = [];
    const api: CompositionQueryApi = {
      get: vi.fn().mockResolvedValue({
        revision: {
          entry_id: identity.compositionId,
          revision_id: identity.revisionId,
        },
        fields: { name: "Budget" },
        unmapped_field_values: {},
      }),
      resolve: vi.fn().mockResolvedValue(resolved("2026-01")),
      querySource: vi.fn((_space, _source, cursor) => {
        cursors.push(cursor);
        return Promise.resolve({
          kind: "entry_query" as const,
          page: {
            rows: [],
            has_more: cursor === undefined,
            ...(cursor === undefined ? { next: "opaque-next" } : {}),
          },
        });
      }),
    };
    const handle = createCompositionQueryHandle(api);

    await handle.open(identity);
    handle.next("entries");
    await vi.waitFor(() => expect(cursors).toHaveLength(2));
    handle.previous("entries");
    await vi.waitFor(() => expect(cursors).toHaveLength(3));

    expect(cursors).toEqual([undefined, "opaque-next", undefined]);
    expect(handle.state().sources.entries?.cursorStack).toEqual([undefined]);
    handle.dispose();
  });

  it("reuses the exact Saved SQL revision and opaque continuation for next and previous pages", async () => {
    vi.mocked(sqlApi.query).mockReset()
      .mockResolvedValueOnce({
        columns: ["amount"],
        rows: [["page 1"]],
        has_more: true,
        next: "sql-cursor-page-2",
      })
      .mockResolvedValueOnce({
        columns: ["amount"],
        rows: [["page 2"]],
        has_more: false,
      })
      .mockResolvedValueOnce({
        columns: ["amount"],
        rows: [["page 1"]],
        has_more: true,
        next: "sql-cursor-page-2",
      });
    const api: CompositionQueryApi = {
      get: vi.fn().mockResolvedValue({
        revision: {
          entry_id: identity.compositionId,
          revision_id: identity.revisionId,
        },
        fields: { name: "Budget" },
        unmapped_field_values: {},
      }),
      resolve: vi.fn().mockResolvedValue(resolvedSavedSql("2026-03-01")),
      querySource: compositionApi.querySource,
    };
    const handle = createCompositionQueryHandle(api);

    await handle.open(identity);
    await vi.waitFor(() =>
      expect(handle.state().sources.sql_results?.status).toBe("ready")
    );
    handle.next("sql_results");
    await vi.waitFor(() => expect(sqlApi.query).toHaveBeenCalledTimes(2));
    await vi.waitFor(() =>
      expect(handle.state().sources.sql_results?.page).toEqual(
        savedSqlPage("page 2"),
      )
    );
    handle.previous("sql_results");
    await vi.waitFor(() => expect(sqlApi.query).toHaveBeenCalledTimes(3));
    await vi.waitFor(() =>
      expect(handle.state().sources.sql_results?.page).toEqual(
        savedSqlPage("page 1", "sql-cursor-page-2"),
      )
    );

    expect(sqlApi.query).toHaveBeenNthCalledWith(
      1,
      identity.spaceId,
      {
        sql: "SELECT amount FROM expenses WHERE month >= :month_start",
        parameters: { month_start: "2026-03-01" },
        parameter_types: { month_start: "date" },
        limit: 10,
        saved_sql: { id: "sql-1", revision_id: "sql-revision-4" },
      },
      expect.any(AbortSignal),
    );
    expect(sqlApi.query).toHaveBeenNthCalledWith(
      2,
      identity.spaceId,
      {
        sql: "SELECT amount FROM expenses WHERE month >= :month_start",
        parameters: { month_start: "2026-03-01" },
        parameter_types: { month_start: "date" },
        limit: 10,
        saved_sql: { id: "sql-1", revision_id: "sql-revision-4" },
        continuation: "sql-cursor-page-2",
      },
      expect.any(AbortSignal),
    );
    expect(sqlApi.query).toHaveBeenNthCalledWith(
      3,
      identity.spaceId,
      {
        sql: "SELECT amount FROM expenses WHERE month >= :month_start",
        parameters: { month_start: "2026-03-01" },
        parameter_types: { month_start: "date" },
        limit: 10,
        saved_sql: { id: "sql-1", revision_id: "sql-revision-4" },
      },
      expect.any(AbortSignal),
    );
    expect(api.resolve).toHaveBeenCalledTimes(1);
    expect(api.resolve).toHaveBeenCalledWith(
      identity.spaceId,
      identity.compositionId,
      identity.revisionId,
      {},
      expect.any(AbortSignal),
    );
    expect(handle.parameters()).toEqual({ month_start: "2026-03-01" });
    expect(handle.state().sources.sql_results?.cursorStack).toEqual([
      undefined,
    ]);
    handle.dispose();
  });

  it("resets Saved SQL continuations on parameter change and discards its stale page", async () => {
    const stalePage = deferred<ReturnType<typeof savedSqlPage>>();
    const currentPage = deferred<ReturnType<typeof savedSqlPage>>();
    const signals: AbortSignal[] = [];
    const sources: Parameters<CompositionQueryApi["querySource"]>[1][] = [];
    const api: CompositionQueryApi = {
      get: vi.fn().mockResolvedValue({
        revision: {
          entry_id: identity.compositionId,
          revision_id: identity.revisionId,
        },
        fields: { name: "Budget" },
        unmapped_field_values: {},
      }),
      resolve: vi.fn((_space, _composition, _revision, parameters) =>
        Promise.resolve(
          resolvedSavedSql(
            typeof parameters.month_start === "string"
              ? parameters.month_start
              : "2026-03-01",
          ),
        )
      ),
      querySource: vi.fn((_space, source, cursor, signal) => {
        sources.push(source);
        signals.push(signal);
        if (signals.length === 1) {
          return Promise.resolve(
            savedSqlPage("initial", "sql-cursor-stale-page"),
          );
        }
        return signals.length === 2 ? stalePage.promise : currentPage.promise;
      }),
    };
    const handle = createCompositionQueryHandle(api);

    await handle.open(identity);
    await vi.waitFor(() =>
      expect(handle.state().sources.sql_results?.status).toBe("ready")
    );
    handle.next("sql_results");
    await vi.waitFor(() => expect(api.querySource).toHaveBeenCalledTimes(2));
    handle.setParameter("month_start", "2026-04-01");
    await vi.waitFor(() => expect(api.querySource).toHaveBeenCalledTimes(3));

    expect(signals[1].aborted).toBe(true);
    expect(handle.state().sources.sql_results?.cursorStack).toEqual([
      undefined,
    ]);
    expect(sources[2]).toMatchObject({
      kind: "saved_sql",
      request: {
        parameters: { month_start: "2026-04-01" },
        saved_sql: { id: "sql-1", revision_id: "sql-revision-4" },
      },
    });
    expect(api.resolve).toHaveBeenNthCalledWith(
      2,
      identity.spaceId,
      identity.compositionId,
      identity.revisionId,
      { month_start: "2026-04-01" },
      expect.any(AbortSignal),
    );

    currentPage.resolve(savedSqlPage("current"));
    await vi.waitFor(() =>
      expect(handle.state().sources.sql_results?.page).toEqual(
        savedSqlPage("current"),
      )
    );
    stalePage.resolve(savedSqlPage("stale"));
    await Promise.resolve();

    expect(handle.state().sources.sql_results?.page).toEqual(
      savedSqlPage("current"),
    );
    expect(handle.state().sources.sql_results?.cursorStack).toEqual([
      undefined,
    ]);
    expect(handle.parameters()).toEqual({ month_start: "2026-04-01" });
    handle.dispose();
  });
});
