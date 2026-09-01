import { OpenAiModelHost } from "./model";

const request = {
  request_id: "request-1",
  prompt: "answer",
  history: [],
  tools: [],
};

describe("OpenAI-compatible model host", () => {
  it("treats an empty length completion as a retryable failure", async () => {
    const host = new OpenAiModelHost({
      apiKey: "test-key",
      fetcher: async () =>
        new Response(
          JSON.stringify({
            choices: [{
              finish_reason: "length",
              message: { content: "   " },
            }],
          }),
          { status: 200 },
        ),
    });

    await expect(host.callModel(request)).rejects.toThrow(
      "output limit without producing an answer",
    );
  });

  it("does not reject a tool-only completion", async () => {
    const host = new OpenAiModelHost({
      apiKey: "test-key",
      fetcher: async () =>
        new Response(
          JSON.stringify({
            choices: [{
              finish_reason: "tool_calls",
              message: {
                content: null,
                tool_calls: [{
                  id: "call-1",
                  function: { name: "ugoite.search", arguments: "{}" },
                }],
              },
            }],
          }),
          { status: 200 },
        ),
    });

    await expect(host.callModel(request)).resolves.toMatchObject({
      request_id: "request-1",
      text: undefined,
      tool_calls: [{ name: "ugoite.search" }],
    });
  });
});
