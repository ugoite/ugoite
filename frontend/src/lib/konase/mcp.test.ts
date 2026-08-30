import { describe, expect, it } from "vitest";
import { BrowserMcpHost } from "./mcp";

const MCP_VERSION = "2026-07-28";

type CapturedRpc = {
  headers: Headers;
  body: {
    jsonrpc: string;
    id: number;
    method: string;
    params: Record<string, unknown>;
  };
};

const scriptedRpc = (results: Record<string, unknown>[]) => {
  const calls: CapturedRpc[] = [];
  const fetcher: typeof fetch = async (_input, init) => {
    const body = JSON.parse(String(init?.body)) as CapturedRpc["body"];
    calls.push({ headers: new Headers(init?.headers), body });
    const result = results.shift();
    if (!result) throw new Error("scripted MCP ran out of responses");
    return new Response(JSON.stringify({ result }), { status: 200 });
  };
  return { calls, fetcher };
};

const mcpRequest = (
  requestId: string,
  operation: string,
  argumentsValue: Record<string, unknown>,
) => ({
  request_id: requestId,
  server: "ugoite",
  operation,
  arguments: argumentsValue,
});

describe("BrowserMcpHost", () => {
  it("uses the canonical root MCP endpoint when no endpoint override is supplied", async () => {
    const fetcher: typeof fetch = async (input) => {
      expect(String(input)).toBe("/mcp");
      return new Response(JSON.stringify({ result: { tools: [] } }), {
        status: 200,
      });
    };
    const host = new BrowserMcpHost({
      accessToken: "test-token",
      fetcher,
    });

    await expect(host.capabilities()).resolves.toEqual([
      {
        name: "resources/read",
        description: "Read the full content of an opaque Ugoite resource URI",
        input_schema: {
          type: "object",
          properties: { uri: { type: "string" } },
          required: ["uri"],
          additionalProperties: false,
        },
        effect: "read",
      },
    ]);
  });

  it("preserves listed schemas and gives resources/read an explicit schema", async () => {
    const host = new BrowserMcpHost({
      accessToken: "test-token",
      fetcher: (async () =>
        new Response(
          JSON.stringify({
            result: {
              tools: [
                {
                  name: "ugoite.search",
                  description: "Search entries",
                  inputSchema: {
                    type: "object",
                    properties: { q: { type: "string" } },
                    required: ["q"],
                    additionalProperties: false,
                  },
                  annotations: { readOnlyHint: true },
                },
                {
                  name: "ugoite.save",
                  description: "Malformed tool without a schema",
                },
              ],
            },
          }),
          { status: 200 },
        )) as typeof fetch,
    });

    await expect(host.capabilities()).resolves.toEqual([
      {
        name: "ugoite.search",
        description: "Search entries",
        input_schema: {
          type: "object",
          properties: { q: { type: "string" } },
          required: ["q"],
          additionalProperties: false,
        },
        effect: "read",
      },
      {
        name: "resources/read",
        description: "Read the full content of an opaque Ugoite resource URI",
        input_schema: {
          type: "object",
          properties: { uri: { type: "string" } },
          required: ["uri"],
          additionalProperties: false,
        },
        effect: "read",
      },
    ]);
  });

  it("sends MCP wire metadata and scopes save and undo to the same Work", async () => {
    const workId = "work-2063";
    const { calls, fetcher } = scriptedRpc([
      {
        tools: [
          {
            name: "ugoite.search",
            description: "Search entries",
            inputSchema: { type: "object" },
            annotations: { readOnlyHint: true },
          },
          {
            name: "ugoite.save",
            description: "Save an entry",
            inputSchema: { type: "object" },
            annotations: { readOnlyHint: false },
          },
          {
            name: "ugoite.undo",
            description: "Undo Work changes",
            inputSchema: { type: "object" },
            annotations: { readOnlyHint: false },
          },
        ],
      },
      {
        content: [{ type: "text", text: "search result" }],
      },
      {
        contents: [{
          type: "text",
          uri: "ugoite://entry/entry-1",
          text: "entry content",
        }],
      },
      {
        content: [{ type: "text", text: "saved" }],
        structuredContent: { status: "created", id: "entry-2" },
      },
      {
        content: [{ type: "text", text: "undone" }],
        structuredContent: { run_id: workId, reverted_change_count: 1 },
      },
    ]);
    const host = new BrowserMcpHost({
      accessToken: "test-token",
      fetcher,
    });

    await expect(host.capabilities()).resolves.toEqual([
      expect.objectContaining({ name: "ugoite.search", effect: "read" }),
      expect.objectContaining({ name: "ugoite.save", effect: "write" }),
      expect.objectContaining({ name: "ugoite.undo", effect: "write" }),
      expect.objectContaining({ name: "resources/read", effect: "read" }),
    ]);
    await host.callMcp(
      mcpRequest("search-1", "ugoite.search", { q: "entry" }),
      workId,
    );
    await host.callMcp(
      mcpRequest("read-1", "resources/read", {
        uri: "ugoite://entry/entry-1",
      }),
      workId,
    );
    const save = await host.callMcp(
      mcpRequest("save-1", "ugoite.save", { content: "new entry" }),
      workId,
    );
    const undo = await host.callMcp(
      mcpRequest("undo-1", "ugoite.undo", {}),
      workId,
    );

    expect(save.success).toBe(true);
    expect(undo.success).toBe(true);
    expect(calls).toHaveLength(5);

    for (const call of calls) {
      expect(call.body.jsonrpc).toBe("2.0");
      expect(call.headers.get("accept")).toBe(
        "application/json, text/event-stream",
      );
      expect(call.headers.get("authorization")).toBe("Bearer test-token");
      expect(call.headers.get("content-type")).toBe("application/json");
      expect(call.headers.get("mcp-protocol-version")).toBe(MCP_VERSION);
      expect(call.body.params._meta).toEqual(
        expect.objectContaining({
          "io.modelcontextprotocol/protocolVersion": MCP_VERSION,
          "io.modelcontextprotocol/clientCapabilities": {},
        }),
      );
    }

    expect(calls[0].body).toMatchObject({
      method: "tools/list",
      params: { _meta: expect.any(Object) },
    });
    expect(calls[0].headers.get("mcp-method")).toBe("tools/list");
    expect(calls[0].headers.get("mcp-name")).toBeNull();

    expect(calls[1].body).toMatchObject({
      method: "tools/call",
      params: {
        name: "ugoite.search",
        arguments: { q: "entry" },
      },
    });
    expect(calls[1].headers.get("mcp-method")).toBe("tools/call");
    expect(calls[1].headers.get("mcp-name")).toBe("ugoite.search");

    expect(calls[2].body).toMatchObject({
      method: "resources/read",
      params: { uri: "ugoite://entry/entry-1" },
    });
    expect(calls[2].headers.get("mcp-method")).toBe("resources/read");
    expect(calls[2].headers.get("mcp-name")).toBe("ugoite://entry/entry-1");

    expect(calls[3].body.params._meta).toEqual(
      expect.objectContaining({ "ugoite/runId": workId }),
    );
    expect(calls[3].headers.get("mcp-method")).toBe("tools/call");
    expect(calls[3].headers.get("mcp-name")).toBe("ugoite.save");

    expect(calls[4].body.params._meta).toEqual(
      expect.objectContaining({ "ugoite/runId": workId }),
    );
    expect(calls[4].headers.get("mcp-method")).toBe("tools/call");
    expect(calls[4].headers.get("mcp-name")).toBe("ugoite.undo");
    expect(calls[4].body.params._meta).toMatchObject(
      calls[3].body.params._meta,
    );
  });
});
