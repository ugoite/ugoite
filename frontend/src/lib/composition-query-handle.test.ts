import { describe, expect, it, vi } from "vitest";
import {
  compositionApi,
  type CompositionResolveResponse,
} from "./composition-api";
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
  variable_definitions: [{
    id: "month",
    type: "string" as const,
    required: true,
    default: "2026-03",
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
    component_bindings: [{
      component_id: "entry_rows",
      kind: "table" as const,
      source_id: "entries",
    }],
    dependencies: {
      variable_consumers: {
        month: { source_ids: ["entries"] },
      },
    },
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
  variable_definitions: [{
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
    component_bindings: [{
      component_id: "sql_rows",
      kind: "table" as const,
      source_id: "sql_results",
    }],
    dependencies: {
      variable_consumers: {
        month_start: { source_ids: ["sql_results"] },
      },
    },
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

  it("keeps an explicit unset marker so a declared default is not restored", async () => {
    const response = resolved("2026-03-01") as CompositionResolveResponse;
    response.variable_definitions = [{
      id: "month_start",
      type: "string",
      required: true,
      default: "starting",
    }];
    response.plan!.sources = [];
    response.plan!.component_bindings = [];
    response.plan!.dependencies = {
      variable_consumers: { month_start: {} },
    };
    const api: CompositionQueryApi = {
      get: vi.fn().mockResolvedValue({
        revision: {
          entry_id: identity.compositionId,
          revision_id: identity.revisionId,
        },
        fields: { name: "Budget" },
        unmapped_field_values: {},
      }),
      resolve: vi.fn().mockResolvedValue(response),
      querySource: vi.fn(),
    };
    const handle = createCompositionQueryHandle(api);

    await handle.open(identity);
    handle.setVariableRaw("month_start", "");

    await vi.waitFor(() => expect(api.resolve).toHaveBeenCalledTimes(2));
    expect(api.resolve).toHaveBeenLastCalledWith(
      identity.spaceId,
      identity.compositionId,
      identity.revisionId,
      { month_start: null },
      expect.any(AbortSignal),
    );
    expect(handle.variables()).toEqual({ month_start: null });
    handle.dispose();
  });

  it("does not query a source when a required variable is explicitly unset", async () => {
    const missing = {
      ok: false,
      variable_definitions: [{
        id: "month_start",
        type: "date" as const,
        required: true,
        default: "2026-03-01",
      }],
      diagnostics: [{ code: "variable_missing", variable_id: "month_start" }],
    };
    const api: CompositionQueryApi = {
      get: vi.fn().mockResolvedValue({
        revision: {
          entry_id: identity.compositionId,
          revision_id: identity.revisionId,
        },
        fields: { name: "Budget" },
        unmapped_field_values: {},
      }),
      resolve: vi.fn()
        .mockResolvedValueOnce(resolvedSavedSql("2026-03-01"))
        .mockResolvedValueOnce(missing),
      querySource: vi.fn().mockResolvedValue(savedSqlPage("initial")),
    };
    const handle = createCompositionQueryHandle(api);

    await handle.open(identity);
    expect(api.querySource).toHaveBeenCalledOnce();
    handle.setVariableRaw("month_start", "");
    await vi.waitFor(() => expect(api.resolve).toHaveBeenCalledTimes(2));

    expect(api.resolve).toHaveBeenLastCalledWith(
      identity.spaceId,
      identity.compositionId,
      identity.revisionId,
      { month_start: null },
      expect.any(AbortSignal),
    );
    expect(handle.state().sources.sql_results?.status).toBe("invalid");
    expect(api.querySource).toHaveBeenCalledOnce();
    handle.dispose();
  });

  it("hides a ready source and skips queries as soon as raw input is malformed", async () => {
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
      querySource: vi.fn().mockResolvedValue(savedSqlPage("initial")),
    };
    const handle = createCompositionQueryHandle(api);

    await handle.open(identity);
    expect(handle.state().sources.sql_results).toMatchObject({
      status: "ready",
      page: savedSqlPage("initial"),
    });

    handle.setVariableRaw("month_start", "not-a-date");

    expect(handle.variableInputs().month_start).toEqual({
      raw: "not-a-date",
      status: "invalid",
    });
    expect(handle.invalidVariables()).toEqual(new Set(["month_start"]));
    expect(handle.state().sources.sql_results).toMatchObject({
      status: "invalid",
    });
    expect(handle.state().sources.sql_results?.page).toBeUndefined();

    // Let the normal debounce window elapse to prove malformed raw input
    // cannot accidentally trigger a resolve or Source query later.
    await new Promise((resolve) => setTimeout(resolve, 150));
    expect(api.resolve).toHaveBeenCalledOnce();
    expect(api.querySource).toHaveBeenCalledOnce();
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
      resolve: vi.fn((_space, _composition, _revision, variables) =>
        Promise.resolve(resolved(variables.month))
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
    handle.setVariableRaw("month", "2026-04");
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
    expect(handle.variables()).toEqual({ month: "2026-04" });
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
        include_exact_integer_values: true,
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
        include_exact_integer_values: true,
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
        include_exact_integer_values: true,
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
    expect(handle.variables()).toEqual({ month_start: "2026-03-01" });
    expect(handle.state().sources.sql_results?.cursorStack).toEqual([
      undefined,
    ]);
    handle.dispose();
  });

  it("skips paged SQL queries when a source is used only by metrics", async () => {
    const response = resolvedSavedSql(
      "2026-03-01",
    ) as CompositionResolveResponse;
    response.plan!.component_bindings = [{
      component_id: "total",
      kind: "metric",
      source_id: "sql_results",
      result_property_key: "amount",
      expected_result_type: "integer",
    }];
    const querySource = vi.fn();
    const api: CompositionQueryApi = {
      get: vi.fn().mockResolvedValue({
        revision: {
          entry_id: identity.compositionId,
          revision_id: identity.revisionId,
        },
        fields: { name: "Budget" },
        unmapped_field_values: {},
      }),
      resolve: vi.fn().mockResolvedValue(response),
      querySource,
    };
    const handle = createCompositionQueryHandle(api);

    await handle.open(identity);

    expect(api.resolve).toHaveBeenCalledWith(
      identity.spaceId,
      identity.compositionId,
      identity.revisionId,
      {},
      expect.any(AbortSignal),
    );
    expect(response.plan!.sources[0]).toMatchObject({
      kind: "saved_sql",
      request: { saved_sql: { id: "sql-1", revision_id: "sql-revision-4" } },
    });
    expect(querySource).not.toHaveBeenCalled();
    expect(handle.state().sources).toEqual({});
    handle.dispose();
  });

  it("does not page a Saved SQL source used only by Metrics", async () => {
    const response = resolvedSavedSql("2026-03-01") as CompositionResolveResponse;
    response.plan!.component_bindings = [{
      component_id: "total",
      kind: "metric",
      source_id: "sql_results",
      result_property_key: "amount",
      expected_result_type: "integer",
    }];
    const querySource = vi.fn();
    const api: CompositionQueryApi = {
      get: vi.fn().mockResolvedValue({
        revision: {
          entry_id: identity.compositionId,
          revision_id: identity.revisionId,
        },
        fields: { name: "Budget" },
        unmapped_field_values: {},
      }),
      resolve: vi.fn().mockResolvedValue(response),
      querySource,
    };
    const handle = createCompositionQueryHandle(api);

    await handle.open(identity);

    expect(querySource).not.toHaveBeenCalled();
    expect(handle.state().sources).toEqual({});
    handle.dispose();
  });

  it("keeps paged SQL queries when a metric shares its source with a table", async () => {
    const response = resolvedSavedSql(
      "2026-03-01",
    ) as CompositionResolveResponse;
    response.plan!.component_bindings = [{
      component_id: "total",
      kind: "metric",
      source_id: "sql_results",
      result_property_key: "amount",
      expected_result_type: "integer",
    }, {
      component_id: "details",
      kind: "table",
      source_id: "sql_results",
    }];
    const querySource = vi.fn().mockResolvedValue(savedSqlPage("17"));
    const api: CompositionQueryApi = {
      get: vi.fn().mockResolvedValue({
        revision: {
          entry_id: identity.compositionId,
          revision_id: identity.revisionId,
        },
        fields: { name: "Budget" },
        unmapped_field_values: {},
      }),
      resolve: vi.fn().mockResolvedValue(response),
      querySource,
    };
    const handle = createCompositionQueryHandle(api);

    await handle.open(identity);

    expect(querySource).toHaveBeenCalledWith(
      identity.spaceId,
      response.plan!.sources[0],
      undefined,
      expect.any(AbortSignal),
    );
    expect(handle.state().sources.sql_results).toMatchObject({
      status: "ready",
      page: savedSqlPage("17"),
    });
    handle.dispose();
  });

  it("preserves an unrelated page request while another source re-resolves", async () => {
    const secondPage = deferred<ReturnType<typeof entryPage>>();
    const sourceRequestCounts = new Map<string, number>();
    const queryCalls: Array<{
      sourceId: string;
      cursor: string | undefined;
      signal: AbortSignal;
    }> = [];
    const responseFor = (month: string): CompositionResolveResponse => ({
      ok: true,
      variable_definitions: [{
        id: "month",
        type: "string",
        required: true,
        default: "2026-03",
      }, {
        id: "status",
        type: "string",
        required: true,
        default: "active",
      }],
      plan: {
        composition_revision: {
          entry_id: identity.compositionId,
          revision_id: identity.revisionId,
        },
        sources: [{
          kind: "entry_query",
          source_id: "month_rows",
          source_schema_fingerprint: "month-schema",
          request: {
            query: {
              scope: { kind: "form", form_id: "form-1" },
              filters: [{
                field: { kind: "property", field_id: 7 },
                operator: "equals",
                value: month,
              }],
              sort: [],
            },
            projection: { kind: "fields", fields: [] },
            limit: 10,
          },
        }, {
          kind: "entry_query",
          source_id: "status_rows",
          source_schema_fingerprint: "status-schema",
          request: {
            query: {
              scope: { kind: "form", form_id: "form-1" },
              filters: [{
                field: { kind: "property", field_id: 8 },
                operator: "equals",
                value: "active",
              }],
              sort: [],
            },
            projection: { kind: "fields", fields: [] },
            limit: 10,
          },
        }],
        component_bindings: [{
          component_id: "month_table",
          kind: "table",
          source_id: "month_rows",
        }, {
          component_id: "status_table",
          kind: "table",
          source_id: "status_rows",
        }],
        dependencies: {
          variable_consumers: {
            month: { source_ids: ["month_rows"] },
            status: { source_ids: ["status_rows"] },
          },
        },
      },
      diagnostics: [],
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
      resolve: vi.fn((_space, _composition, _revision, variables) =>
        Promise.resolve(responseFor(
          typeof variables.month === "string" ? variables.month : "2026-03",
        ))
      ),
      querySource: vi.fn((_space, source, cursor, signal) => {
        const sourceId = source.source_id;
        const count = (sourceRequestCounts.get(sourceId) ?? 0) + 1;
        sourceRequestCounts.set(sourceId, count);
        queryCalls.push({ sourceId, cursor, signal });
        if (sourceId === "status_rows" && cursor === "status-page-2") {
          return secondPage.promise;
        }
        if (sourceId === "status_rows" && count === 1) {
          const initial = entryPage("status-page-1");
          return Promise.resolve({
            ...initial,
            page: {
              ...initial.page,
              has_more: true,
              next: "status-page-2",
            },
          });
        }
        return Promise.resolve(entryPage(`${sourceId}-${count}`));
      }),
    };
    const handle = createCompositionQueryHandle(api);

    await handle.open(identity);
    handle.next("status_rows");
    await vi.waitFor(() => expect(queryCalls).toHaveLength(3));
    handle.setVariableRaw("month", "2026-04");
    await vi.waitFor(() => expect(queryCalls).toHaveLength(4));

    expect(queryCalls[2].sourceId).toBe("status_rows");
    expect(queryCalls[2].cursor).toBe("status-page-2");
    expect(queryCalls[2].signal.aborted).toBe(false);
    expect(queryCalls[3].sourceId).toBe("month_rows");
    expect(sourceRequestCounts.get("status_rows")).toBe(2);
    expect(handle.state().sources.status_rows).toMatchObject({
      status: "loading",
      cursorStack: [undefined, "status-page-2"],
    });

    secondPage.resolve(entryPage("status-page-2-result"));
    await vi.waitFor(() =>
      expect(handle.state().sources.status_rows?.page)
        .toEqual(entryPage("status-page-2-result"))
    );
    expect(handle.state().sources.status_rows?.cursorStack).toEqual([
      undefined,
      "status-page-2",
    ]);
    handle.dispose();
  });

  it("does not retry an unchanged failed source during another variable refresh", async () => {
    const sourceCounts = new Map<string, number>();
    const responseFor = (month: unknown): CompositionResolveResponse => {
      const response = resolved(month) as CompositionResolveResponse;
      response.variable_definitions = [{
        id: "month",
        type: "string",
        required: true,
        default: "2026-03",
      }, {
        id: "status",
        type: "string",
        required: true,
        default: "active",
      }];
      const monthSource = response.plan!.sources[0];
      if (monthSource.kind !== "entry_query") throw new Error("expected entries");
      response.plan!.sources.push({
        ...monthSource,
        source_id: "status_rows",
        source_schema_fingerprint: "status-schema",
        request: {
          ...monthSource.request,
          query: {
            ...monthSource.request.query,
            filters: [{
              field: { kind: "property", field_id: 8 },
              operator: "equals",
              value: "active",
            }],
          },
        },
      });
      response.plan!.component_bindings.push({
        component_id: "status_table",
        kind: "table",
        source_id: "status_rows",
      });
      response.plan!.dependencies = {
        variable_consumers: {
          month: { source_ids: ["entries"] },
          status: { source_ids: ["status_rows"] },
        },
      };
      return response;
    };
    const api: CompositionQueryApi = {
      get: vi.fn().mockResolvedValue({
        revision: {
          entry_id: identity.compositionId,
          revision_id: identity.revisionId,
        },
        fields: { name: "Budget" },
        unmapped_field_values: {},
      }),
      resolve: vi.fn((_space, _composition, _revision, variables) =>
        Promise.resolve(responseFor(variables.month))
      ),
      querySource: vi.fn((_space, source) => {
        const count = (sourceCounts.get(source.source_id) ?? 0) + 1;
        sourceCounts.set(source.source_id, count);
        if (source.source_id === "status_rows") {
          return Promise.reject(new Error("temporary query failure"));
        }
        return Promise.resolve(entryPage(`entries-${count}`));
      }),
    };
    const handle = createCompositionQueryHandle(api);

    await handle.open(identity);
    expect(handle.state().sources.status_rows?.status).toBe("error");
    handle.setVariableRaw("month", "2026-04");
    await vi.waitFor(() => expect(api.resolve).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => expect(sourceCounts.get("entries")).toBe(2));

    expect(sourceCounts.get("status_rows")).toBe(1);
    expect(handle.state().sources.status_rows?.status).toBe("error");
    handle.dispose();
  });

  it("fails closed and hides prior pages when a variable dependency index is missing", async () => {
    const complete = resolved("2026-03") as CompositionResolveResponse;
    const incomplete = resolved("2026-04") as CompositionResolveResponse;
    delete incomplete.plan!.dependencies;
    const api: CompositionQueryApi = {
      get: vi.fn().mockResolvedValue({
        revision: {
          entry_id: identity.compositionId,
          revision_id: identity.revisionId,
        },
        fields: { name: "Budget" },
        unmapped_field_values: {},
      }),
      resolve: vi.fn()
        .mockResolvedValueOnce(complete)
        .mockResolvedValueOnce(incomplete),
      querySource: vi.fn().mockResolvedValue(entryPage("first-page")),
    };
    const handle = createCompositionQueryHandle(api);

    await handle.open(identity);
    expect(handle.state().sources.entries?.status).toBe("ready");
    handle.setVariableRaw("month", "2026-04");
    await vi.waitFor(() => expect(api.resolve).toHaveBeenCalledTimes(2));

    expect(handle.state().sources.entries).toMatchObject({ status: "invalid" });
    expect(handle.state().sources.entries?.page).toBeUndefined();
    expect(handle.state().resolveError).toBeInstanceOf(Error);
    handle.retry("entries");
    expect(api.querySource).toHaveBeenCalledTimes(1);
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
      resolve: vi.fn((_space, _composition, _revision, variables) =>
        Promise.resolve(
          resolvedSavedSql(
            typeof variables.month_start === "string"
              ? variables.month_start
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
    handle.setVariableRaw("month_start", "2026-04-01");
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
    expect(handle.variables()).toEqual({ month_start: "2026-04-01" });
    handle.dispose();
  });
});
