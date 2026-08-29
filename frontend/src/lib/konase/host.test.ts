import { describe, expect, it } from "vitest";
import { KonaseHost } from "./host";
import type { McpHost, McpRequest, McpResult } from "./mcp";
import type { ModelHost, ModelRequest, ModelResult } from "./model";

class ScriptedModel implements ModelHost {
  readonly requests: ModelRequest[] = [];

  constructor(private readonly responses: ModelResult[]) {}

  async callModel(request: ModelRequest): Promise<ModelResult> {
    this.requests.push(request);
    const response = this.responses.shift();
    if (!response) throw new Error("scripted model ran out of responses");
    return { ...response, request_id: request.request_id };
  }
}

class ScriptedMcp implements McpHost {
  readonly operations: string[] = [];
  readonly calls: Array<{ operation: string; workId: string }> = [];

  async capabilities() {
    return [
      {
        name: "ugoite.search",
        description: "Search Ugoite entries",
        input_schema: {
          type: "object",
          properties: { q: { type: "string" } },
          required: ["q"],
        },
      },
      {
        name: "resources/read",
        description: "Read an opaque resource",
        input_schema: {
          type: "object",
          properties: { uri: { type: "string" } },
          required: ["uri"],
          additionalProperties: false,
        },
      },
      {
        name: "ugoite.save",
        description: "Save an Entry",
        input_schema: {
          type: "object",
          properties: { content: { type: "string" } },
          required: ["content"],
          additionalProperties: false,
        },
      },
      {
        name: "ugoite.undo",
        description: "Undo Work changes",
        input_schema: {
          type: "object",
          additionalProperties: false,
        },
      },
    ];
  }

  async callMcp(request: McpRequest, workId: string): Promise<McpResult> {
    this.operations.push(request.operation);
    this.calls.push({ operation: request.operation, workId });
    const search = request.operation === "ugoite.search";
    return {
      request_id: request.request_id,
      operation: request.operation,
      success: true,
      observation: search
        ? {
          id: "search-1",
          kind: "mcp",
          summary: "WebAssembly memo",
          facts: {},
          resource_references: [{
            uri: "ugoite://entry/1",
            label: "WebAssembly",
          }],
        }
        : undefined,
      resources: [],
      resource_contents: search
        ? []
        : [{ uri: "ugoite://entry/1", content: "WebAssembly memo body" }],
    };
  }
}

describe("Konase browser host", () => {
  it("completes the same search → resource read → answer path as the CLI", async () => {
    const progress: string[] = [];
    const model = new ScriptedModel([
      {
        request_id: "",
        tool_calls: [{
          id: "search-call",
          name: "ugoite.search",
          arguments: { q: "WebAssembly" },
        }],
      },
      {
        request_id: "",
        text: "1 memo found",
        tool_calls: [{
          id: "read-call",
          name: "resources/read",
          arguments: { uri: "ugoite://entry/1" },
        }],
      },
      { request_id: "", text: "WebAssembly memo confirmed", tool_calls: [] },
    ]);
    const mcp = new ScriptedMcp();
    const host = new KonaseHost({
      model,
      mcp,
      onProgress: (event) => {
        progress.push(
          event.kind === "mcp"
            ? `${event.kind}:${event.operation}`
            : event.kind,
        );
      },
    });

    const turn = await host.submit("Find the WebAssembly memo");

    expect(turn.outcome.summary).toBe("WebAssembly memo confirmed");
    expect(mcp.operations).toEqual(["ugoite.search", "resources/read"]);
    expect(
      model.requests[0].tools.find((tool) => tool.name === "ugoite.search")
        ?.input_schema,
    ).toEqual({
      type: "object",
      properties: { q: { type: "string" } },
      required: ["q"],
    });
    expect(
      model.requests[0].tools.find((tool) => tool.name === "resources/read")
        ?.input_schema,
    ).toEqual({
      type: "object",
      properties: { uri: { type: "string" } },
      required: ["uri"],
      additionalProperties: false,
    });
    expect(progress).toEqual([
      "model",
      "mcp:ugoite.search",
      "model",
      "mcp:resources/read",
      "model",
      "complete",
    ]);
  });

  it("makes a successful save undoable and reuses its Work ID for undo", async () => {
    const model = new ScriptedModel([
      {
        request_id: "",
        tool_calls: [{
          id: "save-call",
          name: "ugoite.save",
          arguments: { content: "---\nform: Entry\n---\n# Saved" },
        }],
      },
      { request_id: "", text: "Entry saved", tool_calls: [] },
    ]);
    const mcp = new ScriptedMcp();
    const progress: string[] = [];
    const host = new KonaseHost({
      model,
      mcp,
      onProgress: (event) => progress.push(event.kind),
    });

    const turn = await host.submit("Save this Entry");
    const undo = await host.undo(turn.workId);

    expect(turn.undoAvailable).toBe(true);
    expect(undo.success).toBe(true);
    expect(mcp.calls).toEqual([
      { operation: "ugoite.save", workId: turn.workId },
      { operation: "ugoite.undo", workId: turn.workId },
    ]);
    expect(progress).toContain("undo");
  });
});
