import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  canCreateSavedSqlComposition,
  compositionApi,
  type CompositionEvaluationRequest,
  type CompositionSaveResponse,
} from "./composition-api";
import { canonicalizeCompositionDocument } from "./ugoite-client";
import { protocolFetch } from "./ugoite-client/protocol";

vi.mock("./ugoite-client/protocol", () => ({
  protocolFetch: vi.fn(),
  canonicalizeCompositionDocument: vi.fn(),
}));

const signal = new AbortController().signal;

describe("compositionApi", () => {
  beforeEach(() => vi.clearAllMocks());

  it("uses bounded list and exact-revision get operations", async () => {
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

  it("saves and updates strict graph YAML with retry and base revision identity", async () => {
    const response = {
      composition_id: "tool-1",
      revision_id: "revision-2",
      canonical_yaml: "canonical graph yaml",
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
      "canonical graph yaml",
      "attempt-1",
      {
        compositionId: "tool-1",
        baseRevisionId: "revision-1",
        variables: { month: "2026-10" },
        unset_variables: ["optional_note"],
      },
      signal,
    );

    expect(saved.receipt.committed_revision_ids).toEqual(["revision-2"]);
    expect(protocolFetch).toHaveBeenCalledWith(
      "composition.save",
      { space_id: "space-1", idempotency_key: "attempt-1" },
      {
        yaml: "canonical graph yaml",
        composition_id: "tool-1",
        base_revision_id: "revision-1",
        variables: { month: "2026-10" },
        unset_variables: ["optional_note"],
      },
      { signal },
    );
  });

  it("lints a graph document and canonicalizes it through the Rust domain bridge", async () => {
    const document = {
      format: "ugoite.composition" as const,
      format_version: 1 as const,
      kind: "dashboard" as const,
      name: "Tool",
      spec: {
        variables: [],
        components: [],
        layout: { kind: "flow" as const, rows: [] },
      },
    };
    vi.mocked(protocolFetch).mockResolvedValue({
      ok: true,
      value: {
        document,
        canonical_yaml: "canonical graph yaml",
        fingerprint: "f",
      },
    });
    vi.mocked(canonicalizeCompositionDocument).mockResolvedValue({
      document,
      canonical_yaml: "canonical graph yaml",
      fingerprint: "f",
    });

    await compositionApi.lint("strict graph yaml", signal);
    await compositionApi.canonicalizeDocument(document);

    expect(protocolFetch).toHaveBeenCalledWith(
      "composition.lint",
      {},
      { yaml: "strict graph yaml" },
      { signal },
    );
    expect(canonicalizeCompositionDocument).toHaveBeenCalledWith(document);
  });

  it("evaluates an exact saved revision with typed values and requested query roots", async () => {
    const request: CompositionEvaluationRequest = {
      evaluation_id: "evaluation-1",
      target: {
        kind: "saved_revision",
        composition_id: "comp-1",
        revision_id: "rev-2",
      },
      variables: { minimum: 3 },
      requested_queries: ["rows", "total"],
    };
    vi.mocked(protocolFetch).mockResolvedValue({
      evaluation_id: "evaluation-1",
      ok: true,
      computed_query_ids: ["rows", "total"],
    });

    const result = await compositionApi.evaluate("space-1", request, signal);

    expect(result.ok).toBe(true);
    expect(protocolFetch).toHaveBeenCalledWith(
      "composition.evaluate",
      { space_id: "space-1" },
      request,
      { signal },
    );
  });

  it("sends table continuation using the evaluation and variable identity", async () => {
    const request: CompositionEvaluationRequest = {
      evaluation_id: "evaluation-2",
      target: {
        kind: "saved_revision",
        composition_id: "comp-1",
        revision_id: "rev-2",
      },
      variables: {},
      page: { variable_id: "rows", cursor: "cursor-1" },
    };
    vi.mocked(protocolFetch).mockResolvedValue({
      evaluation_id: "evaluation-2",
      ok: true,
      computed_query_ids: ["rows"],
    });

    await compositionApi.evaluate("space-1", request, signal);

    expect(protocolFetch).toHaveBeenCalledWith(
      "composition.evaluate",
      { space_id: "space-1" },
      request,
      { signal },
    );
  });

  it("keeps the exact edit base on graph preview and separates unset values", async () => {
    vi.mocked(protocolFetch).mockResolvedValue({ ok: false, diagnostics: [] });

    await compositionApi.previewGraph(
      "space-1",
      "strict graph yaml",
      { optional_date: null, label: "ready" },
      { entryId: "composition-1", revisionId: "revision-4" },
      signal,
    );

    expect(protocolFetch).toHaveBeenCalledWith(
      "composition.preview",
      { space_id: "space-1" },
      {
        yaml: "strict graph yaml",
        base_entry_id: "composition-1",
        base_revision_id: "revision-4",
        variables: { label: "ready" },
        unset_variables: ["optional_date"],
      },
      { signal },
    );
  });

  it("reads bounded history and restores an exact revision with a retry key", async () => {
    vi.mocked(protocolFetch)
      .mockResolvedValueOnce({
        entry_id: "comp-1",
        revisions: [],
        total: 0,
        offset: 20,
        limit: 50,
        has_more: false,
      })
      .mockResolvedValueOnce({
        composition_id: "tool-1",
        revision_id: "revision-3",
        restored_from_revision_id: "revision-1",
        canonical_yaml: "canonical graph yaml",
        receipt: {
          command_id: "command-2",
          catalog_generation: 5,
          snapshot_id: 43,
          committed_revision_ids: ["revision-3"],
          committed_at_micros: 2,
          data_file_count: 1,
        },
      });

    await compositionApi.history("space-1", "comp-1", 50, 20, signal);
    const restored = await compositionApi.restore(
      "space-1",
      "tool-1",
      "revision-1",
      "revision-2",
      "restore-attempt-1",
      signal,
    );

    expect(restored.restored_from_revision_id).toBe("revision-1");
    expect(protocolFetch).toHaveBeenNthCalledWith(
      1,
      "composition.history",
      { space_id: "space-1", composition_id: "comp-1", limit: 50, offset: 20 },
      undefined,
      { signal },
    );
    expect(protocolFetch).toHaveBeenNthCalledWith(
      2,
      "composition.restore",
      {
        space_id: "space-1",
        composition_id: "tool-1",
        idempotency_key: "restore-attempt-1",
      },
      { source_revision_id: "revision-1", base_revision_id: "revision-2" },
      { signal },
    );
  });

  it("offers Saved SQL only when its kind and argument types are supported", () => {
    const base = {
      id: "sql-1",
      name: "Saved SQL",
      sql: "SELECT 1",
      variables: [],
      created_at: "2026-10-01T00:00:00Z",
      updated_at: "2026-10-02T00:00:00Z",
      revision_id: "sql-revision-1",
    };
    expect(canCreateSavedSqlComposition({ ...base, kind: "user-query" })).toBe(
      true,
    );
    expect(canCreateSavedSqlComposition({ ...base, kind: "search-history" }))
      .toBe(false);
    expect(canCreateSavedSqlComposition({
      ...base,
      kind: "user-query",
      variables: [{ name: "value", type: "decimal", description: "" }],
    })).toBe(false);
  });
});
