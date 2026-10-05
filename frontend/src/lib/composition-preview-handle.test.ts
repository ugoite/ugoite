import { describe, expect, it, vi } from "vitest";
import {
  type CompositionPreviewQueryApi,
  type CompositionPreviewResponse,
  createCompositionPreviewHandle,
} from "./composition-preview-handle";

const source = (id: string) => ({
  source_id: id,
  request: {},
  source_schema_fingerprint: "fp",
  kind: "entry_query",
});

const plan = (fingerprint: string, sources: string[] = ["s1"]) => ({
  draft_fingerprint: fingerprint,
  sources: sources.map(source),
  component_bindings: [],
});

const okResponse = (fingerprint: string): CompositionPreviewResponse => ({
  ok: true,
  draft_fingerprint: fingerprint,
  parameter_definitions: [],
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
      kind: "entry_query" as const,
      page: { rows: [], has_more: false },
    }),
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

  it("re-previews the stored draft when a parameter changes", async () => {
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
    handle.setParameter("month", "2026-10");
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
});
