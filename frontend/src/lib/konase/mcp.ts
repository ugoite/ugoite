import type {
  Capability,
  Observation,
  ResourceContent,
  ResourceReference,
} from "./host";

export type McpRequest = {
  request_id: string;
  server: string;
  operation: string;
  arguments: Record<string, unknown>;
};

export type McpResult = {
  request_id: string;
  operation: string;
  success: boolean;
  observation?: Observation;
  resources: ResourceReference[];
  resource_contents: ResourceContent[];
  error?: string;
};

export interface McpHost {
  callMcp(request: McpRequest, workId: string): Promise<McpResult>;
  capabilities(): Promise<Capability[]>;
}

export type BrowserMcpHostOptions = {
  accessToken: string;
  endpoint?: string;
  fetcher?: typeof fetch;
};

type RpcResponse = {
  result?: Record<string, unknown>;
  error?: { message?: string; data?: unknown };
};

const MCP_VERSION = "2026-07-28";

/** Stateless Streamable HTTP MCP host for the current browser Work. */
export class BrowserMcpHost implements McpHost {
  private readonly accessToken: string;
  private readonly endpoint: string;
  private readonly fetcher: typeof fetch;
  private requestSequence = 0;
  private capabilitiesPromise?: Promise<Capability[]>;

  constructor(options: BrowserMcpHostOptions) {
    if (!options.accessToken.trim()) {
      throw new Error("MCP access token is required");
    }
    this.accessToken = options.accessToken;
    this.endpoint = options.endpoint ?? "/api/mcp";
    this.fetcher = options.fetcher ?? fetch;
  }

  capabilities(): Promise<Capability[]> {
    this.capabilitiesPromise ??= this.listCapabilities();
    return this.capabilitiesPromise;
  }

  async callMcp(request: McpRequest, workId: string): Promise<McpResult> {
    if (request.operation === "resources/read") {
      const uri = request.arguments.uri;
      if (typeof uri !== "string" || !uri.trim()) {
        throw new Error("resources/read requires a string uri");
      }
      const response = await this.rpc(
        "resources/read",
        { uri, _meta: this.meta(workId) },
        uri,
      );
      const contents = Array.isArray(response.contents)
        ? response.contents
        : [];
      return {
        request_id: request.request_id,
        operation: request.operation,
        success: true,
        resources: [],
        resource_contents: contents.flatMap((value) => {
          if (!isRecord(value) || value.type !== "text") return [];
          const contentUri = value.uri;
          const text = value.text;
          return typeof contentUri === "string" && typeof text === "string"
            ? [{ uri: contentUri, content: text }]
            : [];
        }),
      };
    }

    if (!matchesCapability(request.operation)) {
      throw new Error(`unsupported MCP operation ${request.operation}`);
    }
    const response = await this.rpc(
      "tools/call",
      {
        name: request.operation,
        arguments: request.arguments,
        _meta: this.meta(workId),
      },
      request.operation,
    );
    const content = readContent(response.content);
    const success = response.isError !== true;
    if (request.operation !== "ugoite.search") {
      return {
        request_id: request.request_id,
        operation: request.operation,
        success,
        resources: [],
        resource_contents: [],
        error: success ? undefined : content,
      };
    }
    const resources = readResourceLinks(response.content);
    const observation: Observation = {
      id: `browser-observation-${request.request_id}`,
      kind: "mcp",
      summary: content,
      facts: {},
      resource_references: resources,
    };
    return {
      request_id: request.request_id,
      operation: request.operation,
      success,
      observation,
      resources,
      resource_contents: [],
      error: success ? undefined : content,
    };
  }

  private async listCapabilities(): Promise<Capability[]> {
    const response = await this.rpc(
      "tools/list",
      { _meta: this.meta() },
    );
    const tools = Array.isArray(response.tools) ? response.tools : [];
    const capabilities = tools.flatMap((value) => {
      if (!isRecord(value) || typeof value.name !== "string") return [];
      if (!matchesCapability(value.name)) return [];
      return [{
        name: value.name,
        description: typeof value.description === "string"
          ? value.description
          : "",
        input_schema: isRecord(value.inputSchema)
          ? value.inputSchema
          : undefined,
      }];
    });
    capabilities.push({
      name: "resources/read",
      description: "Read the full content of an opaque Ugoite resource URI",
    });
    return capabilities;
  }

  private meta(workId?: string): Record<string, unknown> {
    return {
      "io.modelcontextprotocol/protocolVersion": MCP_VERSION,
      "io.modelcontextprotocol/clientCapabilities": {},
      ...(workId ? { "ugoite/runId": workId } : {}),
    };
  }

  private async rpc(
    method: string,
    params: Record<string, unknown>,
    name?: string,
  ): Promise<Record<string, unknown>> {
    const id = ++this.requestSequence;
    const headers: Record<string, string> = {
      accept: "application/json, text/event-stream",
      authorization: `Bearer ${this.accessToken}`,
      "content-type": "application/json",
      "mcp-method": method,
      "mcp-protocol-version": MCP_VERSION,
    };
    if (name) headers["mcp-name"] = name;
    const response = await this.fetcher(this.endpoint, {
      method: "POST",
      headers,
      credentials: "omit",
      body: JSON.stringify({ jsonrpc: "2.0", id, method, params }),
    });
    const raw = await response.text();
    let body: RpcResponse;
    try {
      body = JSON.parse(raw) as RpcResponse;
    } catch {
      throw new Error(`MCP returned invalid JSON (${response.status})`);
    }
    if (!response.ok || body.error) {
      throw new Error(
        body.error?.message ?? `MCP request failed with ${response.status}`,
      );
    }
    return body.result ?? {};
  }
}

const matchesCapability = (name: string) =>
  name === "ugoite.search" || name === "ugoite.save" || name === "ugoite.undo";

const readContent = (value: unknown): string => {
  if (!Array.isArray(value)) return "";
  return value.flatMap((item) => {
    if (!isRecord(item) || item.type !== "text") return [];
    return typeof item.text === "string" ? [item.text] : [];
  }).join("\n");
};

const readResourceLinks = (value: unknown): ResourceReference[] => {
  if (!Array.isArray(value)) return [];
  return value.flatMap((item) => {
    if (!isRecord(item) || item.type !== "resource_link") return [];
    if (typeof item.uri !== "string") return [];
    return [{
      uri: item.uri,
      label: typeof item.name === "string" ? item.name : undefined,
    }];
  });
};

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);
