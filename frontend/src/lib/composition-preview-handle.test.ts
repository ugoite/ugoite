import { describe, expect, it, vi } from "vitest";
import {
  type CompositionPreviewQueryApi,
  type CompositionPreviewResponse,
  createCompositionPreviewHandle,
} from "./composition-preview-handle";
import type { CompositionSourcePage } from "./composition-api";

const source = (id: string) => ({
  kind: "inline_sql" as const,
  source_id: id,
  request: { sql: "SELECT 1", parameters: {}, parameter_types: {}, limit: 20 },
  source_schema_fingerprint: "fp",
  form_bindings: [],
  expected_result: [{ name: "value", type: "integer" as const }],
});

const plan = (fingerprint: string, sources: string[] = ["s1"]) => ({
  draft_fingerprint: fingerprint,
  sources: sources.map(source),
  component_bindings: [],
});

const okResponse = (fingerprint: string): CompositionPreviewResponse => ({
  ok: true,
  draft_fingerprint: fingerprint,
  variable_definitions: [],
  plan: plan(fingerprint),
});

const mockApi = (
  previews: Array<
    (values: Record<string, unknown>) => Promise<CompositionPreviewResponse>
  >,
): CompositionPreviewQueryApi & { calls: string[] } => {
  const calls: string[] = [];
  let index = 0;
  return {
    calls,
    preview: async (
      _spaceId: string,
      yaml: string,
      values: Record<string, unknown>,
    ) => {
      calls.push(yaml);
      const next = previews[Math.min(index++, previews.length - 1)];
      return await next(values);
    },
    querySource: async () => ({
      kind: "sql" as const,
      page: { columns: ["value"], rows: [[1]], has_more: false },
    }),
    countSource: async () => 1,
  };
};

describe("composition preview handle", () => {
  it("keys execution identity by draft fingerprint", async () => {
    const api = mockApi([async () => okResponse("fp-1")]);
    const handle = createCompositionPreviewHandle(api);
    await handle.preview("space-1", "yaml-1", {});
    expect(handle.state().identity).toEqual({
      spaceId: "space-1",
      fingerprint: "fp-1",
    });
    expect(handle.state().previewing).toBe(false);
    expect(handle.state().sources["s1"]?.status).toBe("ready");
    handle.dispose();
  });

  it("counts a resolved source only for the current draft identity", async () => {
    const api = mockApi([async () => okResponse("fp-1")]);
    const handle = createCompositionPreviewHandle(api);
    await handle.preview("space-1", "yaml-1", {});
    await handle.count("s1");
    expect(handle.state().counts["s1"]).toEqual({
      status: "success",
      value: 1,
    });
    handle.dispose();
  });

  it("drops stale previews when the draft changes", async () => {
    let releaseFirst!: (response: CompositionPreviewResponse) => void;
    const first = new Promise<CompositionPreviewResponse>((resolve) => {
      releaseFirst = resolve;
    });
    const api = mockApi([
      () => first,
      async () => okResponse("fp-2"),
    ]);
    const handle = createCompositionPreviewHandle(api);
    const slow = handle.preview("space-1", "yaml-1", {});
    await handle.preview("space-1", "yaml-2", {});
    releaseFirst(okResponse("fp-1"));
    await slow;
    // The stale first response never overwrites the current preview.
    expect(handle.state().identity?.fingerprint).toBe("fp-2");
    expect(handle.state().yaml).toBe("yaml-2");
    expect(api.calls).toEqual(["yaml-1", "yaml-2"]);
    handle.dispose();
  });

  it("re-previews the stored draft when a variable changes", async () => {
    const seen: Record<string, unknown>[] = [];
    const api = mockApi([
      async (values) => {
        seen.push(values);
        return okResponse("fp-1");
      },
      async (values) => {
        seen.push(values);
        return okResponse("fp-1");
      },
    ]);
    const handle = createCompositionPreviewHandle(api);
    await handle.preview("space-1", "yaml-1", {});
    handle.setVariable("month", "2026-10");
    await vi.waitFor(() => {
      expect(seen.length).toBe(2);
    });
    expect(seen[1]).toEqual({ month: "2026-10" });
    handle.dispose();
  });

  it("surfaces preview errors without stale overwrite", async () => {
    const api = mockApi([
      async () => {
        throw new Error("boom");
      },
    ]);
    const handle = createCompositionPreviewHandle(api);
    await handle.preview("space-1", "yaml-1", {});
    expect(handle.state().previewError).toBeInstanceOf(Error);
    expect(handle.state().previewing).toBe(false);
    handle.dispose();
  });

  it("fetches every plan source without a scope", async () => {
    const requested: string[] = [];
    const api: CompositionPreviewQueryApi = {
      preview: async () => ({
        ok: true,
        draft_fingerprint: "fp-1",
        variable_definitions: [],
        plan: plan("fp-1", ["s1", "s2"]),
      }),
      querySource: async (_spaceId, source) => {
        requested.push(source.source_id);
        return {
          kind: "sql" as const,
          page: { columns: ["value"], rows: [], has_more: false },
        };
      },
      countSource: async () => 0,
    };
    const handle = createCompositionPreviewHandle(api);
    await handle.preview("space-1", "yaml-1", {});
    expect(requested.sort()).toEqual(["s1", "s2"]);
    expect(handle.state().sources["s1"]?.status).toBe("ready");
    expect(handle.state().sources["s2"]?.status).toBe("ready");
    handle.dispose();
  });

  it("fetches only visible-component and selected sources when scoped", async () => {
    const requested: string[] = [];
    const api: CompositionPreviewQueryApi = {
      preview: async () => ({
        ok: true,
        draft_fingerprint: "fp-1",
        variable_definitions: [],
        plan: plan("fp-1", ["s1", "s2", "s3"]),
      }),
      querySource: async (_spaceId, source) => {
        requested.push(source.source_id);
        return {
          kind: "sql" as const,
          page: { columns: ["value"], rows: [], has_more: false },
        };
      },
      countSource: async () => 0,
    };
    const handle = createCompositionPreviewHandle(api);
    await handle.preview("space-1", "yaml-1", {}, {
      visibleSourceIds: ["s1"],
      selectedSourceId: "s2",
    });
    expect(requested.sort()).toEqual(["s1", "s2"]);
    expect(handle.state().sources["s1"]?.status).toBe("ready");
    expect(handle.state().sources["s2"]?.status).toBe("ready");
    // Unfetched sources stay registered for paging but hold no page state.
    expect(handle.state().sources["s3"]).toBeUndefined();

    // Selecting the remaining source pages it through the existing path
    // without re-previewing.
    handle.ensureSource("s3");
    await vi.waitFor(() => {
      expect(handle.state().sources["s3"]?.status).toBe("ready");
    });
    expect(requested.sort()).toEqual(["s1", "s2", "s3"]);

    // Continuation paging still resolves through the registered source.
    handle.ensureSource("s3");
    expect(requested.filter((id) => id === "s3")).toHaveLength(1);
    handle.dispose();
  });

  it("discards stale source pages from the previous draft", async () => {
    const staleRow = ["stale"];
    const latestRow = ["latest"];
    let releaseStale!: (page: CompositionSourcePage) => void;
    const stalePage = new Promise<CompositionSourcePage>((resolve) => {
      releaseStale = resolve;
    });
    const requested: string[] = [];
    const api: CompositionPreviewQueryApi = {
      preview: async (_spaceId, yaml) => okResponse(`fp-${yaml}`),
      querySource: async (_spaceId, source) => {
        requested.push(source.source_id);
        if (requested.length === 1) return await stalePage;
        return {
          kind: "sql" as const,
          page: { columns: ["value"], rows: [latestRow], has_more: false },
        };
      },
      countSource: async () => 0,
    };
    const handle = createCompositionPreviewHandle(api);
    const slow = handle.preview("space-1", "yaml-1", {});
    // Serialize: the first draft's page request is in flight before the
    // second draft preview starts, so the gated call belongs to yaml-1.
    await vi.waitFor(() => {
      expect(requested).toEqual(["s1"]);
    });
    await handle.preview("space-1", "yaml-2", {});
    releaseStale({
      kind: "sql",
      page: { columns: ["value"], rows: [staleRow], has_more: false },
    });
    await slow;
    // The stale first-draft page never overwrites the current preview.
    expect(handle.state().identity?.fingerprint).toBe("fp-yaml-2");
    expect(handle.state().sources["s1"]).toMatchObject({ status: "ready" });
    const page = handle.state().sources["s1"]?.page;
    expect(page?.kind).toBe("sql");
    if (page?.kind === "sql") {
      expect(page.page.rows).toEqual([["latest"]]);
    }
    handle.dispose();
  });
});
