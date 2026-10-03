import { beforeEach, describe, expect, it, vi } from "vitest";
import { entryApi } from "./entry-api";
import { compositionApi, type CompositionResolvedSource } from "./composition-api";
import { sqlApi } from "./sql-api";
import { protocolFetch } from "./ugoite-client/protocol";

vi.mock("./entry-api", () => ({ entryApi: { query: vi.fn() } }));
vi.mock("./sql-api", () => ({ sqlApi: { query: vi.fn() } }));
vi.mock("./ugoite-client/protocol", () => ({
  protocolFetch: vi.fn(),
  evaluateCompositionMetricPage: vi.fn(),
}));

const signal = new AbortController().signal;

const entrySource = {
  kind: "entry_query" as const,
  source_id: "entries",
  source_schema_fingerprint: "fingerprint",
  request: {
    query: { scope: { kind: "form", form_id: "form-1" }, filters: [], sort: [] },
    projection: { kind: "fields", fields: [{ kind: "property", field_id: 7 }] },
    limit: 100,
  },
} as const satisfies CompositionResolvedSource;

const sqlSource = {
  kind: "saved_sql" as const,
  source_id: "total",
  source_schema_fingerprint: "sql-fingerprint",
  request: {
    sql: "SELECT total",
    parameters: { month: "2026-03-01" },
    parameter_types: { month: "date" },
    limit: 1,
    saved_sql: { id: "sql-1", revision_id: "sql-rev-1" },
  },
};

describe("compositionApi", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("uses bounded portable list and exact-revision get operations", async () => {
    vi.mocked(protocolFetch)
      .mockResolvedValueOnce({ items: [], limit: 3, offset: 0, has_more: false })
      .mockResolvedValueOnce({ revision: { revision_id: "revision-2" }, fields: {} });

    await compositionApi.list("space/one", 3, 0, signal);
    await compositionApi.get("space/one", "composition-1", "revision-2", signal);

    expect(protocolFetch).toHaveBeenNthCalledWith(
      1,
      "composition.list",
      { space_id: "space/one", limit: 3, offset: 0 },
      undefined,
      { signal },
    );
    expect(protocolFetch).toHaveBeenNthCalledWith(
      2,
      "composition.get",
      {
        space_id: "space/one",
        composition_id: "composition-1",
        revision_id: "revision-2",
      },
      undefined,
      { signal },
    );
  });

  it("resolves parameters against the exact revision through the portable operation", async () => {
    vi.mocked(protocolFetch).mockResolvedValue({ ok: true });

    await compositionApi.resolve(
      "space-1",
      "composition-1",
      "revision-2",
      { month_start: "2026-03-01" },
      signal,
    );

    expect(protocolFetch).toHaveBeenCalledWith(
      "composition.resolve",
      { space_id: "space-1", composition_id: "composition-1" },
      { revision_id: "revision-2", parameters: { month_start: "2026-03-01" } },
      { signal },
    );
  });

  it("forwards EntryQuery and Saved SQL pages through their existing query adapters", async () => {
    vi.mocked(entryApi.query).mockResolvedValue({ rows: [], has_more: false });
    vi.mocked(sqlApi.query).mockResolvedValue({
      columns: ["total"],
      rows: [],
      has_more: false,
    });

    await compositionApi.querySource("space-1", entrySource, "entry-cursor", signal);
    await compositionApi.querySource("space-1", sqlSource, "sql-cursor", signal);

    expect(entryApi.query).toHaveBeenCalledWith(
      "space-1",
      { ...entrySource.request, after: "entry-cursor" },
      signal,
    );
    expect(sqlApi.query).toHaveBeenCalledWith(
      "space-1",
      { ...sqlSource.request, continuation: "sql-cursor" },
      signal,
    );
  });
});
