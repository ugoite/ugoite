/** Browser-side model boundary for Konase.
 *
 * The browser owns provider credentials and transport. These types mirror the
 * provider-neutral Rust boundary; provider-specific request/response types do
 * not cross into the Konase WASM protocol.
 */

export type ModelTool = {
  name: string;
  description: string;
  input_schema: Record<string, unknown>;
  effect?: "read" | "write";
};

export type ModelToolCall = {
  id: string;
  name: string;
  arguments: Record<string, unknown>;
};

export type ModelMessage =
  | { role: "system"; content: string }
  | { role: "user"; content: string }
  | {
    role: "assistant";
    content: string;
    tool_calls?: ModelToolCall[];
  }
  | { role: "tool"; call_id: string; name: string; content: string };

export type ModelRequest = {
  request_id: string;
  prompt: string;
  history: ModelMessage[];
  tools: ModelTool[];
};

export type ModelResult = {
  request_id: string;
  text?: string;
  tool_calls: ModelToolCall[];
};

export interface ModelHost {
  callModel(request: ModelRequest): Promise<ModelResult>;
}

export type OpenAiModelHostOptions = {
  apiKey: string;
  baseUrl?: string;
  model?: string;
  fetcher?: typeof fetch;
};

type ChatMessage = {
  role: string;
  content?: string | null;
  tool_calls?: Array<{
    id: string;
    type: "function";
    function: { name: string; arguments: string };
  }>;
  tool_call_id?: string;
};

type ChatResponse = {
  choices?: Array<{
    finish_reason?: string | null;
    message?: {
      content?: string | null;
      tool_calls?: Array<{
        id: string;
        function?: { name?: string; arguments?: string };
      }>;
    };
  }>;
  error?: { message?: string };
};

/** One deliberately small OpenAI-compatible provider adapter for the MVP. */
export class OpenAiModelHost implements ModelHost {
  private readonly apiKey: string;
  private readonly baseUrl: string;
  private readonly model: string;
  private readonly fetcher: typeof fetch;

  constructor(options: OpenAiModelHostOptions) {
    if (!options.apiKey.trim()) throw new Error("model API key is required");
    this.apiKey = options.apiKey;
    this.baseUrl = options.baseUrl ?? "https://api.openai.com/v1";
    this.model = options.model ?? "gpt-4o-mini";
    this.fetcher = options.fetcher ?? fetch;
  }

  async callModel(request: ModelRequest): Promise<ModelResult> {
    const messages: ChatMessage[] = request.history.map(toChatMessage);
    messages.push({ role: "user", content: request.prompt });
    const response = await this.fetcher(
      `${this.baseUrl.replace(/\/$/, "")}/chat/completions`,
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
          authorization: `Bearer ${this.apiKey}`,
        },
        body: JSON.stringify({
          model: this.model,
          messages,
          tools: request.tools.map((tool) => ({
            type: "function",
            function: {
              name: tool.name,
              description: tool.description,
              parameters: tool.input_schema,
            },
          })),
          tool_choice: "auto",
        }),
      },
    );
    const text = await response.text();
    let body: ChatResponse;
    try {
      body = JSON.parse(text) as ChatResponse;
    } catch {
      throw new Error(
        `model provider returned invalid JSON (${response.status})`,
      );
    }
    if (!response.ok) {
      throw new Error(
        body.error?.message ?? `model provider returned ${response.status}`,
      );
    }
    const choice = body.choices?.[0];
    const message = choice?.message;
    if (!message) throw new Error("model provider returned no choices");
    const toolCalls = (message.tool_calls ?? []).map((call) => {
      const name = call.function?.name;
      const rawArguments = call.function?.arguments;
      if (!name || rawArguments === undefined) {
        throw new Error("model provider returned an invalid tool call");
      }
      let argumentsValue: unknown;
      try {
        argumentsValue = JSON.parse(rawArguments);
      } catch {
        throw new Error(`invalid model tool arguments for ${name}`);
      }
      if (!isRecord(argumentsValue)) {
        throw new Error(`model tool arguments for ${name} must be an object`);
      }
      return { id: call.id, name, arguments: argumentsValue };
    });
    const messageText = message.content?.trim() ? message.content : undefined;
    if (!messageText && toolCalls.length === 0) {
      throw new Error(
        choice?.finish_reason === "length"
          ? "model provider reached the output limit without producing an answer; retry with a shorter request"
          : "model provider returned an empty completion; retry the request",
      );
    }
    return {
      request_id: request.request_id,
      text: messageText,
      tool_calls: toolCalls,
    };
  }
}

const toChatMessage = (message: ModelMessage): ChatMessage => {
  switch (message.role) {
    case "system":
    case "user":
      return { role: message.role, content: message.content };
    case "assistant":
      return {
        role: "assistant",
        content: message.content || null,
        tool_calls: message.tool_calls?.map((call) => ({
          id: call.id,
          type: "function",
          function: {
            name: call.name,
            arguments: JSON.stringify(call.arguments),
          },
        })),
      };
    case "tool":
      return {
        role: "tool",
        content: message.content,
        tool_call_id: message.call_id,
      };
  }
};

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);
