import { describe, expect, it } from "vitest";
import { BrowserMcpHost } from "./mcp";

describe("BrowserMcpHost", () => {
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
      },
    ]);
  });
});
