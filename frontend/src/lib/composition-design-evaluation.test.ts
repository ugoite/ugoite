import { createRoot, createSignal } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createCompositionDesignEvaluation } from "./composition-design-evaluation";
import {
  type CompositionGraphDraft,
  createCompositionGraphDraft,
} from "./composition-variable-binding";

const { evaluate, canonicalize, parse } = vi.hoisted(() => ({
  evaluate: vi.fn(),
  canonicalize: vi.fn(),
  parse: vi.fn(),
}));
vi.mock(
  "./composition-api",
  () => ({
    compositionApi: {
      evaluate: (...args: unknown[]) => evaluate(...args),
      canonicalizeDocument: (...args: unknown[]) => canonicalize(...args),
    },
  }),
);
vi.mock(
  "./ugoite-client",
  () => ({ parseVariableGraphInput: (...args: unknown[]) => parse(...args) }),
);
const draft = (): CompositionGraphDraft => ({
  ...createCompositionGraphDraft("Report"),
  variables: [
    {
      id: "minimum",
      kind: "input",
      value_type: { scalar_type: "integer", nullable: false },
      required: true,
      default: { kind: "integer", value: 1 },
    },
    {
      id: "rows",
      kind: "query",
      returns: {
        kind: "scalar",
        value_type: { scalar_type: "integer", nullable: false },
      },
      query: {
        kind: "inline_sql",
        sql: "SELECT $minimum",
        arguments: [{ name: "minimum", variable: "minimum" }],
      },
    },
    {
      id: "stable",
      kind: "query",
      returns: {
        kind: "scalar",
        value_type: { scalar_type: "integer", nullable: false },
      },
      query: { kind: "inline_sql", sql: "SELECT 10", arguments: [] },
    },
  ],
});
const response = (
  request: {
    evaluation_id: string;
    requested_queries?: string[];
    variables?: Record<string, unknown>;
  },
  auth = "auth",
) => ({
  evaluation_id: request.evaluation_id,
  ok: true,
  publication_fingerprint: "pub",
  authorization_fingerprint: auth,
  computed_query_ids: request.requested_queries ?? ["rows", "stable"],
  variable_values: {},
  queries: [],
  component_bindings: [],
  dependencies: {},
  query_results: Object.fromEntries(
    (request.requested_queries ?? ["rows", "stable"]).map((
      id,
    ) => [id, {
      value: {
        kind: "scalar",
        value: {
          kind: "integer",
          value: id === "stable" ? 10 : request.variables?.minimum ?? 1,
        },
      },
      has_more: false,
    }]),
  ),
});
let dispose: () => void;
const setup = (base?: { entryId: string; revisionId: string }) => {
  let state!: ReturnType<typeof createCompositionDesignEvaluation>;
  let changeScope!: (value: string) => void;
  let changeDraft!: (value: CompositionGraphDraft) => void;
  createRoot((cleanup) => {
    dispose = cleanup;
    const [scope, setScope] = createSignal("one");
    const [graph, setGraph] = createSignal(draft());
    changeScope = setScope;
    changeDraft = setGraph;
    state = createCompositionDesignEvaluation({
      draft: graph,
      scope,
      spaceId: scope,
      base: () => base,
    });
  });
  return { state, changeScope, changeDraft };
};
describe("Composition Design Work evaluation", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    canonicalize.mockImplementation(async (value) => ({
      canonical_yaml: JSON.stringify(value),
    }));
    evaluate.mockImplementation(async (_space, request) => response(request));
    parse.mockImplementation(async (_type, raw) =>
      raw === "bad"
        ? { status: "invalid" }
        : { status: "valid", value: Number(raw) }
    );
  });
  afterEach(() => {
    dispose?.();
    vi.useRealTimers();
  });
  it("automatically evaluates values and updates only dependent queries without persisting Work", async () => {
    const { state } = setup();
    await vi.advanceTimersByTimeAsync(250);
    expect(state.evaluation()?.query_results.stable.value).toEqual({
      kind: "scalar",
      value: { kind: "integer", value: 10 },
    });
    state.onInput("minimum", "2");
    expect(state.blocked().has("rows")).toBe(true);
    await vi.advanceTimersByTimeAsync(250);
    expect(evaluate.mock.calls[1][1].requested_queries).toEqual(["rows"]);
    expect(evaluate.mock.calls[1][1].variables).toEqual({ minimum: 2 });
    expect(state.evaluation()?.query_results.stable.value).toEqual({
      kind: "scalar",
      value: { kind: "integer", value: 10 },
    });
    expect(canonicalize.mock.calls[1][0].spec).not.toHaveProperty("rawInputs");
    expect(canonicalize.mock.calls[1][0].spec).not.toHaveProperty(
      "query_results",
    );
    state.onInput("minimum", "bad");
    await vi.advanceTimersByTimeAsync(300);
    expect(evaluate).toHaveBeenCalledTimes(2);
    expect(state.inputStates().minimum).toBe("invalid");
    expect(state.blocked().has("rows")).toBe(true);
  });
  it("discards all results and pages after an authorization failure", async () => {
    const { state } = setup();
    await vi.advanceTimersByTimeAsync(250);
    evaluate.mockRejectedValue(new Error("forbidden"));
    state.onInput("minimum", "2");
    await vi.advanceTimersByTimeAsync(250);
    expect(state.evaluation()).toBeUndefined();
    expect(state.blocked()).toEqual(new Set(["rows", "stable"]));
    expect(state.previousIds().size).toBe(0);
    expect(state.error()).toBeDefined();
  });

  it("keeps existing Composition identity bindings and authorization on draft evaluation", async () => {
    setup({ entryId: "composition", revisionId: "revision" });
    await vi.advanceTimersByTimeAsync(250);
    expect(evaluate.mock.calls[0][1]).toMatchObject({
      base_entry_id: "composition",
      base_revision_id: "revision",
    });
  });

  it("does not let paging cancel a pending input re-evaluation", async () => {
    const { state } = setup();
    await vi.advanceTimersByTimeAsync(250);
    state.onInput("minimum", "2");
    state.onNext("stable", "old-cursor");
    await vi.advanceTimersByTimeAsync(250);
    expect(evaluate).toHaveBeenCalledTimes(2);
    expect(evaluate.mock.calls[1][1]).not.toHaveProperty("page");
    expect(evaluate.mock.calls[1][1].variables).toEqual({ minimum: 2 });
    expect(state.blocked().size).toBe(0);
  });

  it("does not rerun queries for layout-only changes", async () => {
    const { changeDraft } = setup();
    await vi.advanceTimersByTimeAsync(250);
    const next = draft();
    next.layout.rows = [{ id: "different", items: [] }];
    changeDraft(next);
    await vi.advanceTimersByTimeAsync(300);
    expect(evaluate).toHaveBeenCalledOnce();
  });
  it("rejects late parse and evaluation replies from another Space", async () => {
    const { state, changeScope } = setup();
    await vi.advanceTimersByTimeAsync(250);
    let finishParse!: (value: unknown) => void;
    parse.mockImplementationOnce(() =>
      new Promise((resolve) => {
        finishParse = resolve;
      })
    );
    state.onInput("minimum", "20");
    changeScope("two");
    await vi.advanceTimersByTimeAsync(250);
    finishParse({ status: "valid", value: 20 });
    await vi.advanceTimersByTimeAsync(300);
    expect(evaluate).toHaveBeenCalledTimes(2);
    expect(evaluate.mock.calls[1][0]).toBe("two");
    expect(evaluate.mock.calls[1][1].variables).toEqual({});
    expect(state.rawInputs()).toEqual({});
  });
  it("discards a pending parse when the input definition changes before any value was committed", async () => {
    const { state, changeDraft } = setup();
    await vi.advanceTimersByTimeAsync(250);
    let finishParse!: (value: unknown) => void;
    parse.mockImplementationOnce(() =>
      new Promise((resolve) => {
        finishParse = resolve;
      })
    );
    state.onInput("minimum", "2");
    const next = draft();
    next.variables = next.variables.map((node) =>
      node.id === "minimum" && node.kind === "input"
        ? {
          ...node,
          value_type: { scalar_type: "string", nullable: false },
          default: { kind: "string", value: "new default" },
        }
        : node
    );
    changeDraft(next);
    finishParse({ status: "valid", value: 2 });
    await vi.advanceTimersByTimeAsync(250);
    expect(evaluate.mock.calls.at(-1)?.[1].variables).toEqual({});
    expect(state.rawInputs()).toEqual({});
    expect(state.inputStates()).toEqual({});
  });

  it("re-evaluates the full graph after authorization changes", async () => {
    const { state } = setup();
    await vi.advanceTimersByTimeAsync(250);
    evaluate.mockImplementation(async (_space, request) =>
      response(request, "new-auth")
    );
    state.onInput("minimum", "2");
    await vi.advanceTimersByTimeAsync(250);
    expect(evaluate).toHaveBeenCalledTimes(3);
    expect(evaluate.mock.calls[2][1]).not.toHaveProperty("requested_queries");
    expect(state.evaluation()?.authorization_fingerprint).toBe("new-auth");
    expect(state.evaluation()?.query_results.stable).toBeDefined();
    expect(state.blocked().size).toBe(0);
  });
});
