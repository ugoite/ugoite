import { invokeKonase } from "../ugoite-client/protocol";
import type {
  ModelHost,
  ModelMessage,
  ModelRequest,
  ModelResult,
  ModelTool,
} from "./model";
import type { McpHost, McpRequest, McpResult } from "./mcp";

export type Capability = {
  name: string;
  description: string;
  input_schema?: Record<string, unknown>;
};

export type ResourceReference = { uri: string; label?: string };
export type ResourceContent = { uri: string; content: string };

export type Observation = {
  id: string;
  kind: "user" | "model" | "mcp" | "host";
  summary: string;
  facts: Record<string, string>;
  resource_references: ResourceReference[];
};

export type KonaseState = {
  status: "idle" | "working" | "completed" | "failed";
  work?: Record<string, unknown>;
  job?: Record<string, unknown>;
  observations: Observation[];
  pending_effect?: Record<string, unknown>;
  last_output?: Record<string, unknown>;
};

type JobSpec = {
  id: string;
  work_id: string;
  goal: string;
  expected_response_schema?: Record<string, unknown>;
};

type ContextCapsule = {
  work_goal: string;
  job_goal: string;
  current_strategy_summary?: string;
  relevant_observations: Observation[];
  available_capabilities: Capability[];
  selected_resource_contents: ResourceContent[];
  safety_hints: string[];
  expected_response_schema?: Record<string, unknown>;
};

type JobRequest = { job: JobSpec; context: ContextCapsule };

type AgentAction =
  | { kind: "call_model"; request: ModelRequest }
  | { kind: "call_mcp"; request: McpRequest }
  | {
    kind: "ask_confirmation";
    request_id: string;
    reason: string;
    operation: string;
  }
  | {
    kind: "complete";
    job_id: string;
    summary: string;
    meaningful: boolean;
  };

type KonaseEffect =
  | { start_job: JobRequest }
  | { call_model: ModelRequest }
  | { call_mcp: McpRequest }
  | { ask_confirmation: AgentAction & { kind: "ask_confirmation" } }
  | { emit: Record<string, unknown> };

type KonaseError = { kind: string; message: string };

type StepResult = {
  state: KonaseState;
  effects: KonaseEffect[];
  error?: KonaseError;
};

type UserRequest = {
  work_id: string;
  job_id: string;
  goal: string;
  available_capabilities: Capability[];
  safety_hints: string[];
};

export type KonaseProtocol = {
  newState(): Promise<KonaseState>;
  step(state: KonaseState, event: unknown): Promise<StepResult>;
};

export type KonaseProgress =
  | { kind: "model" }
  | { kind: "mcp"; operation: string }
  | { kind: "complete"; summary: string }
  | { kind: "undo" };

export type KonaseTurn = {
  outcome: { job_id: string; summary: string; meaningful: boolean };
  workId: string;
  undoAvailable: boolean;
};

export type KonaseHostOptions = {
  model: ModelHost;
  mcp: McpHost;
  protocol?: KonaseProtocol;
  onProgress?: (progress: KonaseProgress) => void;
};

const defaultProtocol: KonaseProtocol = {
  newState: () => invokeKonase<KonaseState>("konase.new"),
  step: (state, event) =>
    invokeKonase<StepResult>("konase.step", { state, event }),
};

/** Browser host for one disposable Konase Work. */
export class KonaseHost {
  private readonly model: ModelHost;
  private readonly mcp: McpHost;
  private readonly protocol: KonaseProtocol;
  private readonly onProgress?: (progress: KonaseProgress) => void;
  private readonly progressListeners = new Set<
    (progress: KonaseProgress) => void
  >();
  private running = false;

  constructor(options: KonaseHostOptions) {
    this.model = options.model;
    this.mcp = options.mcp;
    this.protocol = options.protocol ?? defaultProtocol;
    this.onProgress = options.onProgress;
  }

  subscribeProgress(listener: (progress: KonaseProgress) => void): () => void {
    this.progressListeners.add(listener);
    return () => this.progressListeners.delete(listener);
  }

  async submit(prompt: string): Promise<KonaseTurn> {
    if (!prompt.trim()) throw new Error("Konase prompt must not be empty");
    if (this.running) throw new Error("Konase is already running a Work");
    this.running = true;
    try {
      return await this.run(prompt);
    } finally {
      this.running = false;
    }
  }

  async undo(workId: string): Promise<McpResult> {
    if (!workId.trim()) throw new Error("Konase Work ID is required");
    const result = await this.mcp.callMcp({
      request_id: `undo-${newId()}`,
      server: "ugoite",
      operation: "ugoite.undo",
      arguments: {},
    }, workId);
    if (!result.success) {
      throw new Error(result.error ?? "Konase Work undo failed");
    }
    this.emitProgress({ kind: "undo" });
    return result;
  }

  private async run(prompt: string): Promise<KonaseTurn> {
    const workId = `work-${newId()}`;
    const jobId = `job-${newId()}`;
    const capabilities = await this.mcp.capabilities();
    let state = await this.protocol.newState();
    const result = await this.protocol.step(state, {
      user_submitted: {
        work_id: workId,
        job_id: jobId,
        goal: prompt,
        available_capabilities: capabilities,
        safety_hints: [
          "Use Ugoite MCP for requested reads and writes; the Host binds writes to this Work and supports undo",
        ],
      } satisfies UserRequest,
    });
    state = requireState(result);
    const start = result.effects.find(isStartJob)?.start_job;
    if (!start) throw new Error("Konase did not start a Job");

    const runtime = new BrowserAgentRuntime();
    let action = runtime.start(start.job, start.context, capabilities);
    state = await this.progress(state, jobId, action);
    let undoAvailable = false;

    while (true) {
      if (action.kind === "call_model") {
        this.emitProgress({ kind: "model" });
        const response = await this.model.callModel(action.request);
        action = runtime.resumeModel(response);
      } else if (action.kind === "call_mcp") {
        this.emitProgress({ kind: "mcp", operation: action.request.operation });
        const mcpResult = await this.mcp.callMcp(action.request, workId);
        undoAvailable ||= action.request.operation === "ugoite.save" &&
          mcpResult.success;
        const mcpStep = await this.protocol.step(state, {
          mcp_completed: mcpResult,
        });
        state = requireState(mcpStep);
        action = runtime.resumeMcp(mcpResult);
      } else if (action.kind === "complete") {
        this.emitProgress({ kind: "complete", summary: action.summary });
        await this.progress(state, jobId, action);
        return {
          outcome: {
            job_id: action.job_id,
            summary: action.summary,
            meaningful: action.meaningful,
          },
          workId,
          undoAvailable,
        };
      } else {
        throw new Error(
          "Konase confirmation is not available in the browser MVP",
        );
      }
      state = await this.progress(state, jobId, action);
      if (action.kind === "complete") {
        this.emitProgress({ kind: "complete", summary: action.summary });
        return {
          outcome: {
            job_id: action.job_id,
            summary: action.summary,
            meaningful: action.meaningful,
          },
          workId,
          undoAvailable,
        };
      }
    }
  }

  private emitProgress(progress: KonaseProgress) {
    this.onProgress?.(progress);
    for (const listener of this.progressListeners) listener(progress);
  }

  private async progress(
    state: KonaseState,
    jobId: string,
    action: AgentAction,
  ): Promise<KonaseState> {
    return requireState(
      await this.protocol.step(state, {
        agent_progress: {
          job_id: jobId,
          action: actionToProtocol(action),
        },
      }),
    );
  }
}

class BrowserAgentRuntime {
  private jobId = "";
  private tools: ModelTool[] = [];
  private history: ModelMessage[] = [];
  private pending:
    | { kind: "model"; requestId: string }
    | { kind: "mcp"; requestId: string; callId: string; name: string }
    | undefined;
  private modelTurn = 0;
  private mcpSequence = 0;

  start(
    job: JobSpec,
    context: ContextCapsule,
    capabilities: Capability[],
  ): AgentAction {
    this.jobId = job.id;
    this.tools = capabilities.map((capability) => ({
      name: capability.name,
      description: capability.description,
      input_schema: capability.input_schema ?? { type: "object" },
    }));
    return this.nextModel(
      `Job goal: ${job.goal}\nContext: ${JSON.stringify(context)}`,
    );
  }

  resumeModel(result: ModelResult): AgentAction {
    const pending = this.pending;
    if (
      !pending || pending.kind !== "model" ||
      pending.requestId !== result.request_id
    ) {
      throw new Error(
        "model result does not match the pending browser request",
      );
    }
    this.pending = undefined;
    if (result.tool_calls.length > 1) {
      throw new Error("Konase hosts one MCP effect at a time");
    }
    this.history.push({
      role: "assistant",
      content: result.text ?? "",
      tool_calls: result.tool_calls,
    });
    const call = result.tool_calls[0];
    if (!call) {
      const summary = result.text?.trim();
      if (!summary) {
        throw new Error("model result must contain text or a tool call");
      }
      return {
        kind: "complete",
        job_id: this.jobId,
        summary,
        meaningful: true,
      };
    }
    this.mcpSequence += 1;
    const requestId = `${this.jobId}:mcp:${this.mcpSequence}`;
    this.pending = {
      kind: "mcp",
      requestId,
      callId: call.id,
      name: call.name,
    };
    return {
      kind: "call_mcp",
      request: {
        request_id: requestId,
        server: "ugoite",
        operation: call.name,
        arguments: call.arguments,
      },
    };
  }

  resumeMcp(result: McpResult): AgentAction {
    const pending = this.pending;
    if (
      !pending || pending.kind !== "mcp" ||
      pending.requestId !== result.request_id
    ) {
      throw new Error("MCP result does not match the pending browser request");
    }
    this.pending = undefined;
    this.history.push({
      role: "tool",
      call_id: pending.callId,
      name: pending.name,
      content: JSON.stringify(result),
    });
    return this.nextModel("Continue the Job using the latest Ugoite result.");
  }

  private nextModel(prompt: string): AgentAction {
    this.modelTurn += 1;
    const requestId = `${this.jobId}:model:${this.modelTurn}`;
    this.pending = { kind: "model", requestId };
    return {
      kind: "call_model",
      request: {
        request_id: requestId,
        prompt,
        history: [...this.history],
        tools: [...this.tools],
      },
    };
  }
}

const actionToProtocol = (action: AgentAction): Record<string, unknown> => {
  switch (action.kind) {
    case "call_model":
      return { kind: action.kind, ...action.request };
    case "call_mcp":
      return { kind: action.kind, ...action.request };
    case "complete":
      return action;
    case "ask_confirmation":
      return action;
  }
};

const isStartJob = (
  effect: KonaseEffect,
): effect is { start_job: JobRequest } => "start_job" in effect;

const requireState = (result: StepResult): KonaseState => {
  if (result.error) throw new Error(result.error.message);
  return result.state;
};

const newId = (): string => globalThis.crypto.randomUUID();

export type {
  BrowserMcpHostOptions,
  McpHost,
  McpRequest,
  McpResult,
} from "./mcp";
export type {
  ModelHost,
  ModelMessage,
  ModelRequest,
  ModelResult,
  ModelTool,
  ModelToolCall,
  OpenAiModelHostOptions,
} from "./model";
