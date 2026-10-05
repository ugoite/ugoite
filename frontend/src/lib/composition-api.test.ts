import { beforeEach, describe, expect, it, vi } from "vitest";
import { entryApi } from "./entry-api";
import {
  buildSavedSqlCompositionDocument,
  canCreateSavedSqlComposition,
  compositionApi,
  type CompositionResolvedSource,
  type CompositionSaveResponse,
} from "./composition-api";
import { sqlApi } from "./sql-api";
import { canonicalizeCompositionDocument } from "./ugoite-client";
import { protocolFetch } from "./ugoite-client/protocol";

vi.mock("./entry-api", () => ({ entryApi: { query: vi.fn() } }));
vi.mock("./sql-api", () => ({ sqlApi: { query: vi.fn() } }));
vi.mock("./ugoite-client/protocol", () => ({
  protocolFetch: vi.fn(),
  canonicalizeCompositionDocument: vi.fn(),
  evaluateCompositionMetricPage: vi.fn(),
}));

const signal = new AbortController().signal;

const entrySource = {
  kind: "entry_query" as const,
  source_id: "entries",
  source_schema_fingerprint: "fingerprint",
  request: {
    query: {
      scope: { kind: "form", form_id: "form-1" },
      filters: [],
      sort: [],
    },
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
      .mockResolvedValueOnce({
        items: [],
        limit: 3,
        offset: 0,
        has_more: false,
      })
      .mockResolvedValueOnce({
        revision: { revision_id: "revision-2" },
        fields: {},
      });

    await compositionApi.list("space/one", 3, 0, signal);
    await compositionApi.get(
      "space/one",
      "composition-1",
      "revision-2",
      signal,
    );

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

  it("saves canonical YAML through the portable operation with its retry key", async () => {
    const response = {
      composition_id: "tool-1",
      revision_id: "revision-2",
      canonical_yaml: "canonical yaml",
      receipt: {
        command_id: "command-1",
        catalog_generation: 4,
        snapshot_id: 42,
        committed_revision_ids: ["revision-2"],
        committed_at_micros: 1,
        data_file_count: 1,
      },
    } satisfies CompositionSaveResponse;
    vi.mocked(protocolFetch).mockResolvedValue(response);

    const saved = await compositionApi.save(
      "space-1",
      "canonical yaml",
      "attempt-1",
      signal,
    );

    expect(typeof saved.receipt.snapshot_id).toBe("number");
    expect(saved.receipt.snapshot_id).toBe(42);

    expect(protocolFetch).toHaveBeenCalledWith(
      "composition.save",
      { space_id: "space-1", idempotency_key: "attempt-1" },
      { yaml: "canonical yaml" },
      { signal },
    );
  });

  it("canonicalizes a typed document through the shared Rust domain bridge", async () => {
    const document = {
      format: "ugoite.composition" as const,
      format_version: 1 as const,
      kind: "dashboard" as const,
      name: "Tool",
      tags: [],
      spec: { parameters: [], sources: [], components: [], sections: [] },
    };
    vi.mocked(canonicalizeCompositionDocument).mockResolvedValue({
      document,
      canonical_yaml: "canonical yaml",
      fingerprint: "fingerprint",
    });

    await compositionApi.canonicalizeDocument(document);

    expect(canonicalizeCompositionDocument).toHaveBeenCalledWith(document);
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

  it("builds a typed table tool bound to the exact Saved SQL revision", () => {
    const entry = {
      id: "sql-1",
      name: "Monthly expenses",
      kind: "user-query" as const,
      sql: "SELECT amount FROM expenses WHERE day >= :month_start",
      variables: [
        { name: "month_start", type: "date", description: "" },
        { name: "include_zero", type: "boolean", description: "" },
      ],
      created_at: "2026-10-01T00:00:00Z",
      updated_at: "2026-10-02T00:00:00Z",
      revision_id: "sql-revision-3",
    };
    expect(canCreateSavedSqlComposition(entry)).toBe(true);

    const document = buildSavedSqlCompositionDocument(
      entry,
      "Monthly expenses",
      ["amount", "day"],
      { month_start: "2026-10-01", include_zero: false },
    );

    expect(document.spec.parameters).toEqual([
      {
        id: "month_start",
        type: "date",
        required: true,
        default: "2026-10-01",
      },
      { id: "include_zero", type: "boolean", required: true, default: false },
    ]);
    expect(document.spec.sources).toEqual([{
      id: "sql_results",
      kind: "saved_sql",
      entry_id: "sql-1",
      revision_id: "sql-revision-3",
      expected_result: [
        { name: "amount", type: "json" },
        { name: "day", type: "json" },
      ],
      variables: {
        month_start: { parameter: "month_start" },
        include_zero: { parameter: "include_zero" },
      },
    }]);
    expect(document.spec.components).toEqual([{
      id: "results_table",
      kind: "table",
      source: "sql_results",
    }]);
  });

  it("stores the server-owned column types unchanged and never infers from rows", () => {
    const entry = {
      id: "sql-1",
      name: "Monthly expenses",
      kind: "user-query" as const,
      sql: "SELECT total, label FROM monthly",
      variables: [],
      created_at: "2026-10-01T00:00:00Z",
      updated_at: "2026-10-02T00:00:00Z",
      revision_id: "sql-revision-3",
    };

    const document = buildSavedSqlCompositionDocument(
      entry,
      "Monthly expenses",
      ["total", "label", "note"],
      {},
      [
        { name: "total", type: "float" },
        { name: "label", type: "string" },
      ],
    );

    expect(document.spec.sources[0].expected_result).toEqual([
      { name: "total", type: "float" },
      { name: "label", type: "string" },
      // Columns absent from the server descriptor keep the json fallback.
      { name: "note", type: "json" },
    ]);
  });

  it("does not offer save-as for search history or unsupported parameter types", () => {
    const base = {
      id: "sql-1",
      name: "Saved SQL",
      sql: "SELECT 1",
      variables: [],
      created_at: "2026-10-01T00:00:00Z",
      updated_at: "2026-10-02T00:00:00Z",
      revision_id: "sql-revision-1",
    };
    expect(canCreateSavedSqlComposition({ ...base, kind: "search-history" }))
      .toBe(false);
    expect(canCreateSavedSqlComposition({
      ...base,
      kind: "user-query",
      variables: [{ name: "v", type: "unknown", description: "" }],
    })).toBe(false);
  });

  it("forwards EntryQuery and Saved SQL pages through their existing query adapters", async () => {
    vi.mocked(entryApi.query).mockResolvedValue({ rows: [], has_more: false });
    vi.mocked(sqlApi.query).mockResolvedValue({
      columns: ["total"],
      rows: [],
      has_more: false,
    });

    await compositionApi.querySource(
      "space-1",
      entrySource,
      "entry-cursor",
      signal,
    );
    await compositionApi.querySource(
      "space-1",
      sqlSource,
      "sql-cursor",
      signal,
    );

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
