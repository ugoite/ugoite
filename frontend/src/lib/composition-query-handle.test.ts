import { describe, expect, it, vi } from "vitest";
import {
  type CompositionQueryApi,
  createCompositionQueryHandle,
} from "./composition-query-handle";

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
});
