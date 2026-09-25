import { describe, expect, it } from "vitest";
import {
  type Capability,
  KonaseHost,
  KonaseWorkFailure,
  type WritePreview,
} from "./host";
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
  readonly requests: McpRequest[] = [];

  constructor(
    private readonly failSave = false,
    private readonly omitSaveReceipt = false,
    private readonly searchEffect: Capability["effect"] = "read",
    private readonly schemaReadResult?: {
      success: boolean;
      resource_contents: McpResult["resource_contents"];
    },
    private readonly schemaReadBarrier?: () => Promise<void>,
    private readonly onSchemaRead?: () => void,
  ) {}

  async capabilities(): Promise<Capability[]> {
    return [
      {
        name: "ugoite.search",
        description: "Search Ugoite entries",
        input_schema: {
          type: "object",
          properties: { q: { type: "string" } },
          required: ["q"],
        },
        effect: this.searchEffect,
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
        effect: "read",
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
        effect: "write",
      },
      {
        name: "ugoite.undo",
        description: "Undo Work changes",
        input_schema: {
          type: "object",
          additionalProperties: false,
        },
        effect: "write",
      },
    ];
  }

  async callMcp(request: McpRequest, workId: string): Promise<McpResult> {
    this.operations.push(request.operation);
    this.calls.push({ operation: request.operation, workId });
    this.requests.push(structuredClone(request));
    const search = request.operation === "ugoite.search";
    const schemaRead = request.operation === "resources/read" &&
      typeof request.arguments.uri === "string" &&
      request.arguments.uri.endsWith("/schema");
    if (schemaRead) {
      this.onSchemaRead?.();
      await this.schemaReadBarrier?.();
    }
    const success = !(this.failSave && request.operation === "ugoite.save");
    return {
      request_id: request.request_id,
      operation: request.operation,
      success: schemaRead ? this.schemaReadResult?.success ?? true : success,
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
      resource_contents: schemaRead
        ? this.schemaReadResult?.resource_contents ?? [{
          uri: String(request.arguments.uri),
          content: JSON.stringify({
            id: "form-7",
            name: "Note",
            fields: {},
            _untrusted_content: true,
          }),
        }]
        : search
        ? []
        : [{ uri: "ugoite://entry/1", content: "WebAssembly memo body" }],
      structured_content: request.operation === "ugoite.save"
        ? this.omitSaveReceipt ? undefined : {
          id: "entry-1",
          uri: "ugoite://entry/entry-1",
          status: "created",
          _untrusted_content: true,
        }
        : request.operation === "ugoite.undo"
        ? {
          run_id: workId,
          reverted_change_count: 1,
          _untrusted_content: true,
        }
        : undefined,
      error: success ? undefined : "save failed",
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
      spaceId: "space-a",
      onProgress: (event) => {
        progress.push(
          event.kind === "mcp"
            ? `${event.kind}:${event.operation}`
            : event.kind,
        );
      },
    });

    const turn = await host.submit("Find and save the WebAssembly memo");

    expect(turn.outcome.summary).toBe("WebAssembly memo confirmed");
    expect(turn.knowledge).toBe("unchanged");
    expect(turn.undoAvailable).toBe(false);
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
      "knowledge",
    ]);
  });

  it("makes a successful save undoable and reuses its Work ID for undo", async () => {
    const model = new ScriptedModel([
      {
        request_id: "",
        tool_calls: [{
          id: "save-call",
          name: "ugoite.save",
          arguments: {
            form: "Entry",
            fields: { title: "Saved" },
          },
        }],
      },
      { request_id: "", text: "Entry saved", tool_calls: [] },
    ]);
    const mcp = new ScriptedMcp();
    const progress: string[] = [];
    const previews: WritePreview[] = [];
    const host = new KonaseHost({
      model,
      mcp,
      spaceId: "space-a",
      onConfirmationRequired: (preview) => {
        previews.push(preview);
        host.resolveConfirmation(preview.requestId, true);
      },
      onProgress: (event) => progress.push(event.kind),
    });

    const turn = await host.submit("Save this Entry");
    const undo = await host.undo(turn.workId);

    expect(turn.undoAvailable).toBe(true);
    expect(turn.knowledge).toBe("saved");
    expect(previews[0].action).toBe("create");
    expect(undo.success).toBe(true);
    expect(mcp.calls).toEqual([
      { operation: "ugoite.save", workId: turn.workId },
      { operation: "ugoite.undo", workId: turn.workId },
    ]);
    expect(progress).toContain("undo");
  });

  it("reports a failed save without offering undo", async () => {
    const model = new ScriptedModel([
      {
        request_id: "",
        tool_calls: [{
          id: "save-call",
          name: "ugoite.save",
          arguments: {
            form: "Entry",
            fields: { title: "Failed" },
          },
        }],
      },
      { request_id: "", text: "Save attempted", tool_calls: [] },
    ]);
    const host = new KonaseHost({
      model,
      mcp: new ScriptedMcp(true),
      spaceId: "space-a",
      onConfirmationRequired: (preview) => {
        queueMicrotask(() => host.resolveConfirmation(preview.requestId, true));
      },
    });

    const turn = await host.submit("Save this Entry");

    expect(turn.knowledge).toBe("write_failed");
    expect(turn.undoAvailable).toBe(false);
  });

  it("shows a safe per-call preview and dispatches the original arguments only after approval", async () => {
    const argumentsValue = {
      form: "Note",
      fields: { title: "private note body", api_token: "secret-token-value" },
      tags: ["work"],
      extra_attributes: { source: "Konase" },
    };
    const model = new ScriptedModel([
      {
        request_id: "",
        tool_calls: [{
          id: "save-call",
          name: "ugoite.save",
          arguments: argumentsValue,
        }],
      },
      { request_id: "", text: "Entry saved", tool_calls: [] },
    ]);
    const mcp = new ScriptedMcp();
    let resolvePreview!: (preview: WritePreview) => void;
    const previewReady = new Promise<WritePreview>((resolve) => {
      resolvePreview = resolve;
    });
    const host = new KonaseHost({
      model,
      mcp,
      spaceId: "space-a",
      onConfirmationRequired: resolvePreview,
    });

    const turnPromise = host.submit("Save the note");
    const preview = await previewReady;
    expect(mcp.calls).toHaveLength(0);
    expect(preview).toMatchObject({
      workId: expect.stringMatching(/^work-/),
      spaceId: "space-a",
      operation: "ugoite.save",
      action: "create",
      form: "Note",
    });
    expect(preview.summary).toContain("title: text (17 chars)");
    expect(preview.summary).toContain("api_token: [hidden]");
    expect(preview.summary).not.toContain("private note body");
    expect(preview.summary).not.toContain("secret-token-value");

    expect(host.resolveConfirmation(preview.requestId, true)).toBe(true);
    const turn = await turnPromise;
    expect(turn.knowledge).toBe("saved");
    expect(turn.undoAvailable).toBe(true);
    expect(mcp.calls).toHaveLength(1);
    expect(mcp.requests[0]).toEqual({
      request_id: preview.requestId,
      server: "ugoite",
      operation: "ugoite.save",
      arguments: argumentsValue,
      effect: "write",
    });
    expect(host.resolveConfirmation(preview.requestId, true)).toBe(false);
  });

  it("denies a write without dispatch and consumes each approval once", async () => {
    const model = new ScriptedModel([
      {
        request_id: "",
        tool_calls: [{
          id: "save-call",
          name: "ugoite.save",
          arguments: { form: "Note", fields: { title: "x" } },
        }],
      },
      { request_id: "", text: "Entry saved", tool_calls: [] },
    ]);
    const mcp = new ScriptedMcp();
    let resolvePreview!: (preview: WritePreview) => void;
    const previewReady = new Promise<WritePreview>((resolve) => {
      resolvePreview = resolve;
    });
    const host = new KonaseHost({
      model,
      mcp,
      spaceId: "space-a",
      onConfirmationRequired: resolvePreview,
    });

    const turnPromise = host.submit("Save the note");
    const preview = await previewReady;
    expect(mcp.calls).toHaveLength(0);
    expect(host.resolveConfirmation("stale-request", true)).toBe(false);
    expect(host.resolveConfirmation(preview.requestId, false)).toBe(true);
    await expect(turnPromise).rejects.toThrow(/not approved/);
    expect(host.resolveConfirmation(preview.requestId, true)).toBe(false);
    expect(mcp.calls).toHaveLength(0);
  });

  it("denies a write when no confirmation UI is registered", async () => {
    const model = new ScriptedModel([{
      request_id: "",
      tool_calls: [{
        id: "save-call",
        name: "ugoite.save",
        arguments: { form: "Note", fields: { title: "x" } },
      }],
    }]);
    const mcp = new ScriptedMcp();
    const host = new KonaseHost({ model, mcp, spaceId: "space-a" });

    await expect(host.submit("Save the note")).rejects.toThrow(/not approved/);
    expect(mcp.calls).toHaveLength(0);
  });

  it("cancels a pending approval on disposal and ignores a late approve", async () => {
    const model = new ScriptedModel([
      {
        request_id: "",
        tool_calls: [{
          id: "save-call",
          name: "ugoite.save",
          arguments: { form: "Note", fields: { title: "x" } },
        }],
      },
    ]);
    const mcp = new ScriptedMcp();
    let resolvePreview!: (preview: WritePreview) => void;
    const previewReady = new Promise<WritePreview>((resolve) => {
      resolvePreview = resolve;
    });
    const host = new KonaseHost({
      model,
      mcp,
      spaceId: "space-a",
      onConfirmationRequired: resolvePreview,
    });
    const turnPromise = host.submit("Save the note");
    const preview = await previewReady;

    host.dispose();
    await expect(turnPromise).rejects.toThrow();
    expect(host.resolveConfirmation(preview.requestId, true)).toBe(false);
    expect(mcp.calls).toHaveLength(0);
  });

  it("fails closed for an unlisted model-requested capability", async () => {
    const model = new ScriptedModel([{
      request_id: "",
      tool_calls: [{ id: "other", name: "ugoite.unknown", arguments: {} }],
    }]);
    const mcp = new ScriptedMcp();
    const host = new KonaseHost({ model, mcp, spaceId: "space-a" });

    await expect(host.submit("Run an unknown tool")).rejects.toThrow(
      /unlisted/,
    );
    expect(mcp.calls).toHaveLength(0);
  });

  it("fails closed when tools/list does not declare search read-only", async () => {
    const model = new ScriptedModel([{
      request_id: "",
      tool_calls: [{
        id: "search",
        name: "ugoite.search",
        arguments: { q: "x" },
      }],
    }]);
    const mcp = new ScriptedMcp(false, false, "write");
    const host = new KonaseHost({ model, mcp, spaceId: "space-a" });

    await expect(host.submit("Search")).rejects.toThrow(/not read-only/);
    expect(mcp.calls).toHaveLength(0);
  });

  it("resolves an omitted Form name before approving a complete field-map replacement", async () => {
    const argumentsValue = {
      id: "entry-7",
      fields: { title: "Replacement title" },
    };
    const model = new ScriptedModel([
      {
        request_id: "",
        tool_calls: [{
          id: "update-call",
          name: "ugoite.save",
          arguments: argumentsValue,
        }],
      },
      { request_id: "", text: "Entry updated", tool_calls: [] },
    ]);
    const mcp = new ScriptedMcp();
    const previews: WritePreview[] = [];
    const host = new KonaseHost({
      model,
      mcp,
      spaceId: "space-a",
      onConfirmationRequired: (preview) => {
        previews.push(preview);
        host.resolveConfirmation(preview.requestId, true);
      },
    });

    const turn = await host.submit("Update this Entry");

    expect(previews[0]).toMatchObject({
      workId: turn.workId,
      action: "update",
      form: "Note",
      entryId: "entry-7",
    });
    expect(previews[0].summary).toContain("Form Note");
    expect(previews[0].summary).toContain(
      "replace the complete structured field map",
    );
    expect(mcp.operations).toEqual(["resources/read", "ugoite.save"]);
    expect(mcp.requests[0]).toMatchObject({
      operation: "resources/read",
      arguments: { uri: "ugoite://entry/entry-7/schema" },
      effect: "read",
    });
    expect(mcp.requests[1].arguments).toEqual(argumentsValue);
  });

  it("does not request approval or save when the existing Form cannot be resolved", async () => {
    const model = new ScriptedModel([{
      request_id: "",
      tool_calls: [{
        id: "update-call",
        name: "ugoite.save",
        arguments: { id: "entry-7", fields: { title: "Replacement" } },
      }],
    }]);
    const uri = "ugoite://entry/entry-7/schema";
    const mcp = new ScriptedMcp(false, false, "read", {
      success: false,
      resource_contents: [{ uri, content: "{}" }],
    });
    let approvalCount = 0;
    const host = new KonaseHost({
      model,
      mcp,
      spaceId: "space-a",
      onConfirmationRequired: () => approvalCount++,
    });

    await expect(host.submit("Update Entry")).rejects.toThrow(
      /could not be safely resolved/,
    );
    expect(approvalCount).toBe(0);
    expect(mcp.operations).toEqual(["resources/read"]);
    expect(model.requests).toHaveLength(1);
  });

  it("rejects malformed schema projections before approval or save", async () => {
    const uri = "ugoite://entry/entry-7/schema";
    const malformedContents = [
      { uri, content: "{" },
      {
        uri,
        content: JSON.stringify({
          id: "form-7",
          name: "Note",
          _untrusted_content: true,
        }),
      },
      {
        uri,
        content: JSON.stringify({
          id: "form-7",
          name: "Note",
          fields: [],
          _untrusted_content: true,
        }),
      },
      {
        uri: "ugoite://entry/other/schema",
        content: JSON.stringify({
          id: "form-7",
          name: "Note",
          fields: {},
          _untrusted_content: true,
        }),
      },
    ];

    for (const content of malformedContents) {
      const model = new ScriptedModel([{
        request_id: "",
        tool_calls: [{
          id: "update-call",
          name: "ugoite.save",
          arguments: { id: "entry-7", fields: { title: "Replacement" } },
        }],
      }]);
      const mcp = new ScriptedMcp(false, false, "read", {
        success: true,
        resource_contents: [content],
      });
      let approvalCount = 0;
      const host = new KonaseHost({
        model,
        mcp,
        spaceId: "space-a",
        onConfirmationRequired: () => approvalCount++,
      });

      await expect(host.submit("Update Entry")).rejects.toThrow(
        /could not be safely resolved/,
      );
      expect(approvalCount).toBe(0);
      expect(mcp.operations).toEqual(["resources/read"]);
    }
  });

  it("discards a schema read that returns after the Host is disposed", async () => {
    let finishRead!: () => void;
    let markReadStarted!: () => void;
    const readStarted = new Promise<void>((resolve) => {
      markReadStarted = resolve;
    });
    const readBarrier = new Promise<void>((resolve) => {
      finishRead = resolve;
    });
    const model = new ScriptedModel([{
      request_id: "",
      tool_calls: [{
        id: "update-call",
        name: "ugoite.save",
        arguments: { id: "entry-7", fields: { title: "Replacement" } },
      }],
    }]);
    const mcp = new ScriptedMcp(
      false,
      false,
      "read",
      undefined,
      () => readBarrier,
      markReadStarted,
    );
    let approvalCount = 0;
    const host = new KonaseHost({
      model,
      mcp,
      spaceId: "space-a",
      onConfirmationRequired: () => approvalCount++,
    });
    const turn = host.submit("Update Entry");

    await readStarted;
    host.dispose();
    finishRead();
    await expect(turn).rejects.toThrow(/no longer active/);
    expect(approvalCount).toBe(0);
    expect(mcp.operations).toEqual(["resources/read"]);
  });

  it("preserves a confirmed save and its Undo when a later write is denied", async () => {
    const model = new ScriptedModel([
      {
        request_id: "",
        tool_calls: [{
          id: "save-a",
          name: "ugoite.save",
          arguments: { form: "Note", fields: { title: "A" } },
        }],
      },
      {
        request_id: "",
        tool_calls: [{
          id: "save-b",
          name: "ugoite.save",
          arguments: { form: "Note", fields: { title: "B" } },
        }],
      },
      { request_id: "", text: "Done", tool_calls: [] },
    ]);
    const mcp = new ScriptedMcp();
    let resolveSecondPreview!: (preview: WritePreview) => void;
    const secondPreviewReady = new Promise<WritePreview>((resolve) => {
      resolveSecondPreview = resolve;
    });
    let confirmationCount = 0;
    const host = new KonaseHost({
      model,
      mcp,
      spaceId: "space-a",
      onConfirmationRequired: (preview) => {
        confirmationCount += 1;
        if (confirmationCount === 1) {
          host.resolveConfirmation(preview.requestId, true);
        } else {
          resolveSecondPreview(preview);
        }
      },
    });
    const turnPromise = host.submit("Save two notes");
    const secondPreview = await secondPreviewReady;
    expect(mcp.calls.filter((call) => call.operation === "ugoite.save"))
      .toHaveLength(1);
    expect(mcp.calls[0].workId).toBe(secondPreview.workId);
    expect(host.resolveConfirmation(secondPreview.requestId, false)).toBe(true);
    const failure = await turnPromise.catch((cause) => cause);
    expect(failure).toBeInstanceOf(KonaseWorkFailure);
    expect(failure).toMatchObject({
      partial: {
        workId: secondPreview.workId,
        knowledge: "saved",
        undoAvailable: true,
      },
      reason: expect.objectContaining({
        message: "Konase write was not approved",
      }),
    });
    expect(mcp.calls.filter((call) => call.operation === "ugoite.save"))
      .toHaveLength(1);
  });

  it("does not report saved or undoable when a save receipt is missing", async () => {
    const model = new ScriptedModel([
      {
        request_id: "",
        tool_calls: [{
          id: "save-call",
          name: "ugoite.save",
          arguments: { form: "Note", fields: { title: "x" } },
        }],
      },
      { request_id: "", text: "Save attempted", tool_calls: [] },
    ]);
    const mcp = new ScriptedMcp(false, true);
    const host = new KonaseHost({
      model,
      mcp,
      spaceId: "space-a",
      onConfirmationRequired: (preview) => {
        host.resolveConfirmation(preview.requestId, true);
      },
    });

    await expect(host.submit("Save the note")).rejects.toThrow(/confirm/);
    expect(mcp.calls.filter((call) => call.operation === "ugoite.save"))
      .toHaveLength(1);
  });

  it("requires separate approval for model Undo and clears Undo availability", async () => {
    const model = new ScriptedModel([
      {
        request_id: "",
        tool_calls: [{
          id: "save-call",
          name: "ugoite.save",
          arguments: { form: "Note", fields: { title: "x" } },
        }],
      },
      {
        request_id: "",
        tool_calls: [{ id: "undo-call", name: "ugoite.undo", arguments: {} }],
      },
      { request_id: "", text: "Change undone", tool_calls: [] },
    ]);
    const mcp = new ScriptedMcp();
    const previews: WritePreview[] = [];
    const host = new KonaseHost({
      model,
      mcp,
      spaceId: "space-a",
      onConfirmationRequired: (preview) => {
        previews.push(preview);
        host.resolveConfirmation(preview.requestId, true);
      },
    });

    const turn = await host.submit("Save then undo");
    expect(previews.map((preview) => preview.operation)).toEqual([
      "ugoite.save",
      "ugoite.undo",
    ]);
    expect(previews[1].workId).toBe(turn.workId);
    expect(previews[1].summary).toContain(turn.workId);
    expect(turn.knowledge).toBe("unchanged");
    expect(turn.undoAvailable).toBe(false);
    expect(mcp.calls.map((call) => call.operation)).toEqual([
      "ugoite.save",
      "ugoite.undo",
    ]);
  });
});
