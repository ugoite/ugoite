import { invokeKonase } from "../ugoite-client/protocol";
import type {
  ModelHost,
  ModelMessage,
  ModelRequest,
  ModelResult,
  ModelTool,
} from "./model";
import {
  type McpHost,
  type McpRequest,
  type McpResult,
  validateMutationResult,
} from "./mcp";

export type CapabilityEffect = "read" | "write";

export type KnowledgeOutcome = "unchanged" | "saved" | "write_failed";

export type Capability = {
  name: string;
  description: string;
  input_schema?: Record<string, unknown>;
  effect?: CapabilityEffect;
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
  knowledge: KnowledgeOutcome;
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
    request: McpRequest;
    preview: WritePreview;
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
  | { kind: "knowledge"; outcome: KnowledgeOutcome }
  | { kind: "undo" };

export type KonaseTurn = {
  outcome: { job_id: string; summary: string; meaningful: boolean };
  workId: string;
  undoAvailable: boolean;
  knowledge: KnowledgeOutcome;
};

export type WritePreview = {
  requestId: string;
  workId: string;
  spaceId: string;
  operation: "ugoite.save" | "ugoite.undo";
  action: "create" | "update" | "undo";
  form?: string;
  entryId?: string;
  summary: string;
};

export class KonaseWriteDeniedError extends Error {
  constructor() {
    super("Konase write was not approved");
    this.name = "KonaseWriteDeniedError";
  }
}

export class KonaseMutationUnconfirmedError extends Error {
  constructor() {
    super("Ugoite could not confirm the mutation result");
    this.name = "KonaseMutationUnconfirmedError";
  }
}

export type KonasePartialWork = {
  workId: string;
  jobId: string;
  knowledge: KnowledgeOutcome;
  undoAvailable: boolean;
};

export class KonaseWorkFailure extends Error {
  constructor(
    readonly reason: unknown,
    readonly partial: KonasePartialWork,
  ) {
    super(reason instanceof Error ? reason.message : "Konase Work failed");
    this.name = "KonaseWorkFailure";
  }
}

export type KonaseHostOptions = {
  model: ModelHost;
  mcp: McpHost;
  spaceId: string;
  protocol?: KonaseProtocol;
  onProgress?: (progress: KonaseProgress) => void;
  onConfirmationRequired?: (preview: WritePreview) => void;
  onConfirmationCancelled?: (requestId: string) => void;
};

const defaultProtocol: KonaseProtocol = {
  newState: () => invokeKonase<KonaseState>("konase.new"),
  step: (state, event) =>
    invokeKonase<StepResult>("konase.step", { state, event }),
};

const CONFIRMATION_TIMEOUT_MS = 5 * 60 * 1_000;

/** Browser host for one disposable Konase Work. */
export class KonaseHost {
  private readonly model: ModelHost;
  private readonly mcp: McpHost;
  private readonly protocol: KonaseProtocol;
  private readonly spaceId: string;
  private readonly onProgress?: (progress: KonaseProgress) => void;
  private readonly onConfirmationRequired?: (preview: WritePreview) => void;
  private readonly onConfirmationCancelled?: (requestId: string) => void;
  private readonly progressListeners = new Set<
    (progress: KonaseProgress) => void
  >();
  private running = false;
  private disposed = false;
  private generation = 0;
  private partialWork?: KonasePartialWork;
  private pendingConfirmation?: {
    requestId: string;
    resolve: (approved: boolean) => void;
    timer: ReturnType<typeof setTimeout>;
    expiresAt: number;
  };

  constructor(options: KonaseHostOptions) {
    this.model = options.model;
    this.mcp = options.mcp;
    this.protocol = options.protocol ?? defaultProtocol;
    this.spaceId = options.spaceId;
    this.onProgress = options.onProgress;
    this.onConfirmationRequired = options.onConfirmationRequired;
    this.onConfirmationCancelled = options.onConfirmationCancelled;
  }

  resolveConfirmation(requestId: string, approved: boolean): boolean {
    const pending = this.pendingConfirmation;
    if (!pending || pending.requestId !== requestId || this.disposed) {
      return false;
    }
    clearTimeout(pending.timer);
    this.pendingConfirmation = undefined;
    const isExpired = Date.now() >= pending.expiresAt;
    pending.resolve(approved && !isExpired);
    if (isExpired) this.onConfirmationCancelled?.(pending.requestId);
    return !isExpired;
  }

  cancelPending(): void {
    this.generation += 1;
    const pending = this.pendingConfirmation;
    if (!pending) return;
    clearTimeout(pending.timer);
    this.pendingConfirmation = undefined;
    pending.resolve(false);
    this.onConfirmationCancelled?.(pending.requestId);
  }

  dispose(): void {
    if (this.disposed) return;
    this.cancelPending();
    this.disposed = true;
  }

  subscribeProgress(listener: (progress: KonaseProgress) => void): () => void {
    this.progressListeners.add(listener);
    return () => this.progressListeners.delete(listener);
  }

  async submit(prompt: string): Promise<KonaseTurn> {
    if (!prompt.trim()) throw new Error("Konase prompt must not be empty");
    if (this.running) throw new Error("Konase is already running a Work");
    if (this.disposed) throw new Error("Konase Host has been disposed");
    this.running = true;
    try {
      return await this.run(prompt);
    } catch (cause) {
      if (this.partialWork?.undoAvailable) {
        throw new KonaseWorkFailure(cause, this.partialWork);
      }
      throw cause;
    } finally {
      this.running = false;
    }
  }

  async undo(workId: string): Promise<McpResult> {
    if (!workId.trim()) throw new Error("Konase Work ID is required");
    const generation = this.generation;
    const request: McpRequest = {
      request_id: `undo-${newId()}`,
      server: "ugoite",
      operation: "ugoite.undo",
      arguments: {},
      effect: "write",
    };
    this.assertCurrent(generation);
    let rawResult: McpResult;
    try {
      rawResult = await this.mcp.callMcp(request, workId);
    } catch {
      throw new KonaseMutationUnconfirmedError();
    }
    let result: McpResult;
    try {
      result = validateMutationResult(request, workId, rawResult);
    } catch {
      throw new KonaseMutationUnconfirmedError();
    }
    this.assertCurrent(generation);
    if (!result.success) {
      throw new Error(result.error ?? "Konase Work undo failed");
    }
    this.emitProgress({ kind: "undo" });
    return result;
  }

  private async run(prompt: string): Promise<KonaseTurn> {
    const generation = this.generation;
    this.assertCurrent(generation);
    const workId = `work-${newId()}`;
    const jobId = `job-${newId()}`;
    this.partialWork = {
      workId,
      jobId,
      knowledge: "unchanged",
      undoAvailable: false,
    };
    const capabilities = await this.mcp.capabilities();
    this.assertCurrent(generation);
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

    const runtime = new BrowserAgentRuntime(this.spaceId);
    let action = runtime.start(start.job, start.context, workId);
    state = await this.progress(state, jobId, action);
    let undoAvailable = false;

    while (true) {
      if (action.kind === "call_model") {
        this.emitProgress({ kind: "model" });
        try {
          const response = await this.model.callModel(action.request);
          this.assertCurrent(generation);
          action = runtime.resumeModel(response);
        } catch (cause) {
          if (this.isCurrent(generation)) {
            await this.hostFailed(
              state,
              action.request.request_id,
              "model_request_failed",
              cause,
            );
          }
          throw cause;
        }
      } else if (action.kind === "call_mcp") {
        this.assertCurrent(generation);
        let dispatchStarted = false;
        let receiptValidated = false;
        try {
          runtime.authorizeDispatch(
            action.request,
            workId,
            this.spaceId,
            generation,
          );
          this.emitProgress({
            kind: "mcp",
            operation: action.request.operation,
          });
          dispatchStarted = true;
          const rawResult = await this.mcp.callMcp(action.request, workId);
          const mcpResult = validateMutationResult(
            action.request,
            workId,
            rawResult,
          );
          receiptValidated = true;
          this.assertCurrent(generation);
          undoAvailable = action.request.operation === "ugoite.save"
            ? mcpResult.success || undoAvailable
            : action.request.operation === "ugoite.undo" && mcpResult.success
            ? false
            : undoAvailable;
          const mcpStep = await this.protocol.step(state, {
            mcp_completed: mcpResult,
          });
          state = requireState(mcpStep);
          this.partialWork = {
            workId,
            jobId,
            knowledge: state.knowledge,
            undoAvailable,
          };
          action = runtime.resumeMcp(mcpResult);
        } catch (cause) {
          if (this.isCurrent(generation)) {
            await this.hostFailed(
              state,
              action.request.request_id,
              "mcp_request_failed",
              cause,
            );
          }
          if (
            dispatchStarted && !receiptValidated &&
            action.request.effect === "write"
          ) {
            throw new KonaseMutationUnconfirmedError();
          }
          throw cause;
        }
      } else if (action.kind === "ask_confirmation") {
        const wasApproved = await this.requestApproval(
          action.preview,
          generation,
        );
        const approved = wasApproved && this.isCurrent(generation);
        const confirmationStep = await this.protocol.step(state, {
          confirmation_completed: {
            request_id: action.request_id,
            approved,
          },
        });
        state = requireState(confirmationStep);
        if (!approved) {
          throw new KonaseWriteDeniedError();
        }
        action = runtime.resumeConfirmation(
          {
            request_id: action.request_id,
            approved: true,
          },
          workId,
          this.spaceId,
          generation,
        );
      } else if (action.kind === "complete") {
        state = await this.progress(state, jobId, action);
        this.emitProgress({ kind: "complete", summary: action.summary });
        this.emitProgress({ kind: "knowledge", outcome: state.knowledge });
        return {
          outcome: {
            job_id: action.job_id,
            summary: action.summary,
            meaningful: action.meaningful,
          },
          workId,
          undoAvailable,
          knowledge: state.knowledge,
        };
      } else {
        throw new Error("Unsupported Konase action");
      }
      state = await this.progress(state, jobId, action);
      if (action.kind === "complete") {
        this.emitProgress({ kind: "complete", summary: action.summary });
        this.emitProgress({ kind: "knowledge", outcome: state.knowledge });
        return {
          outcome: {
            job_id: action.job_id,
            summary: action.summary,
            meaningful: action.meaningful,
          },
          workId,
          undoAvailable,
          knowledge: state.knowledge,
        };
      }
    }
  }

  private async requestApproval(
    preview: WritePreview,
    generation: number,
  ): Promise<boolean> {
    this.assertCurrent(generation);
    if (!this.onConfirmationRequired) return false;
    return await new Promise<boolean>((resolve) => {
      const expiresAt = Date.now() + CONFIRMATION_TIMEOUT_MS;
      const timer = setTimeout(() => {
        if (this.pendingConfirmation?.requestId !== preview.requestId) return;
        this.pendingConfirmation = undefined;
        resolve(false);
        this.onConfirmationCancelled?.(preview.requestId);
      }, CONFIRMATION_TIMEOUT_MS);
      this.pendingConfirmation = {
        requestId: preview.requestId,
        resolve,
        timer,
        expiresAt,
      };
      try {
        this.onConfirmationRequired?.(preview);
      } catch {
        this.resolveConfirmation(preview.requestId, false);
      }
    });
  }

  private async hostFailed(
    state: KonaseState,
    requestId: string,
    kind: string,
    cause: unknown,
  ): Promise<KonaseState> {
    const message = cause instanceof Error
      ? cause.message
      : "Host request failed";
    const result = await this.protocol.step(state, {
      host_failed: { kind, message, request_id: requestId },
    });
    return requireState(result);
  }

  private isCurrent(generation: number): boolean {
    return !this.disposed && this.generation === generation;
  }

  private assertCurrent(generation: number): void {
    if (!this.isCurrent(generation)) {
      throw new Error("Konase Host is no longer active for this Space");
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
  private workId = "";

  constructor(private readonly spaceId: string) {}

  private jobId = "";
  private tools: ModelTool[] = [];
  private history: ModelMessage[] = [];
  private pending:
    | { kind: "model"; requestId: string }
    | { kind: "mcp"; requestId: string; callId: string; name: string }
    | {
      kind: "confirmation";
      requestId: string;
      callId: string;
      name: string;
      request: McpRequest;
      preview: WritePreview;
    }
    | undefined;
  private approvedDispatch?: {
    request: McpRequest;
    workId: string;
    spaceId: string;
    generation: number;
  };
  private modelTurn = 0;
  private mcpSequence = 0;

  start(
    job: JobSpec,
    context: ContextCapsule,
    workId: string,
  ): AgentAction {
    this.jobId = job.id;
    this.workId = workId;
    this.tools = context.available_capabilities.flatMap((capability) => {
      if (!capability.input_schema) return [];
      return [{
        name: capability.name,
        description: capability.description,
        input_schema: capability.input_schema,
        effect: capability.effect,
      }];
    });
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
    const capability = this.tools.find((tool) => tool.name === call.name);
    if (!capability) {
      throw new Error(`Model requested unlisted MCP capability ${call.name}`);
    }
    const effect = effectiveEffect(call.name, capability.effect);
    const request: McpRequest = freezeRequest({
      request_id: requestId,
      server: "ugoite",
      operation: call.name,
      arguments: call.arguments,
      effect,
    });
    if (effect === "write") {
      const preview = createWritePreview(request, this.workId, this.spaceId);
      this.pending = {
        kind: "confirmation",
        requestId,
        callId: call.id,
        name: call.name,
        request,
        preview,
      };
      return {
        kind: "ask_confirmation",
        request_id: requestId,
        operation: call.name,
        reason: preview.summary,
        request,
        preview,
      };
    }
    this.pending = { kind: "mcp", requestId, callId: call.id, name: call.name };
    return {
      kind: "call_mcp",
      request,
    };
  }

  resumeConfirmation(
    result: { request_id: string; approved: boolean },
    workId: string,
    spaceId: string,
    generation: number,
  ): AgentAction {
    const pending = this.pending;
    if (
      !pending || pending.kind !== "confirmation" ||
      pending.requestId !== result.request_id
    ) {
      throw new Error(
        "confirmation does not match the pending browser request",
      );
    }
    if (!result.approved) {
      this.pending = undefined;
      throw new Error("Konase write was not approved");
    }
    this.pending = {
      kind: "mcp",
      requestId: pending.request.request_id,
      callId: pending.callId,
      name: pending.name,
    };
    this.approvedDispatch = {
      request: pending.request,
      workId,
      spaceId: safePreviewLabel(spaceId),
      generation,
    };
    return { kind: "call_mcp", request: pending.request };
  }

  authorizeDispatch(
    request: McpRequest,
    workId: string,
    spaceId: string,
    generation: number,
  ): void {
    const expectedEffect = effectiveEffect(request.operation, request.effect);
    if (expectedEffect === "write") {
      const approved = this.approvedDispatch;
      if (
        !approved || approved.request !== request ||
        approved.workId !== workId ||
        approved.spaceId !== spaceId || approved.generation !== generation
      ) {
        throw new Error("MCP write is missing its matching one-shot approval");
      }
      this.approvedDispatch = undefined;
      return;
    }
    if (request.effect !== "read") {
      throw new Error("MCP capability effect is unknown");
    }
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
      return {
        kind: action.kind,
        request_id: action.request_id,
        operation: action.operation,
        reason: action.reason,
      };
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

function effectiveEffect(
  name: string,
  declared: CapabilityEffect | undefined,
): CapabilityEffect {
  switch (name) {
    case "resources/read":
      return "read";
    case "ugoite.search":
      if (declared === "read") return "read";
      throw new Error(
        declared === "write"
          ? "The listed search capability is not read-only"
          : "The listed search capability has no trusted effect metadata",
      );
    case "ugoite.save":
    case "ugoite.undo":
      return "write";
    default:
      throw new Error(`Model requested unlisted MCP capability ${name}`);
  }
}

function createWritePreview(
  request: McpRequest,
  workId: string,
  spaceId: string,
): WritePreview {
  if (request.operation === "ugoite.undo") {
    return {
      requestId: request.request_id,
      workId,
      spaceId: safePreviewLabel(spaceId),
      operation: "ugoite.undo",
      action: "undo",
      summary: `Undo changes from Work ${safePreviewLabel(workId)} in Space ${
        safePreviewLabel(spaceId)
      }.`,
    };
  }
  const id = request.arguments.id;
  const form = request.arguments.form;
  const fields = request.arguments.fields;
  if (
    (id !== undefined && (typeof id !== "string" || !id.trim())) ||
    (form !== undefined && (typeof form !== "string" || !form.trim())) ||
    !isRecord(fields)
  ) {
    throw new Error("MCP save arguments cannot be safely previewed");
  }
  const entryId = typeof id === "string" ? id : undefined;
  const formName = typeof form === "string" ? form : undefined;
  if (!entryId && !formName) {
    throw new Error("New MCP save has no Form target");
  }
  const fieldSummary = Object.entries(fields).map(([name, value]) =>
    `${safePreviewLabel(name)}: ${
      isSensitiveField(name) ? "[hidden]" : valueSummary(value)
    }`
  ).join(", ");
  const summaryParts = [
    `${entryId ? "Update" : "Create"} in Space ${
      safePreviewLabel(spaceId)
    }; Form ${safePreviewLabel(formName ?? "from existing Entry")}${
      entryId ? `; Entry ${safePreviewLabel(entryId)}` : ""
    }.`,
    `Fields (${
      entryId ? "replace the complete structured field map" : "new Entry values"
    }): ${fieldSummary}.`,
  ];
  if (Array.isArray(request.arguments.tags) && request.arguments.tags.length) {
    summaryParts.push(`Tags: ${request.arguments.tags.length} tag(s).`);
  }
  if (
    isRecord(request.arguments.extra_attributes) &&
    Object.keys(request.arguments.extra_attributes).length
  ) {
    summaryParts.push("Extra attributes: present.");
  }
  let summary = summaryParts.join(" ");
  const summaryCharacters = [...summary];
  if (summaryCharacters.length > 1_200) {
    summary = `${summaryCharacters.slice(0, 1_189).join("")}… [omitted]`;
  }
  return {
    requestId: request.request_id,
    workId,
    spaceId,
    operation: "ugoite.save",
    action: entryId ? "update" : "create",
    form: formName ? safePreviewLabel(formName) : undefined,
    entryId: entryId ? safePreviewLabel(entryId) : undefined,
    summary,
  };
}

function freezeRequest(request: McpRequest): McpRequest {
  const copy = structuredClone(request);
  freezeNested(copy.arguments);
  return Object.freeze(copy);
}

function freezeNested(value: unknown): void {
  if (!value || typeof value !== "object" || Object.isFrozen(value)) return;
  for (const child of Object.values(value)) freezeNested(child);
  Object.freeze(value);
}

function safePreviewLabel(value: string): string {
  const characters = [...value];
  const shortened = characters.length > 96;
  return characters.slice(0, shortened ? 84 : 96).map((character) =>
    isControlCharacter(character) ? " " : character
  ).join("") + (shortened ? "… [omitted]" : "");
}

function isControlCharacter(character: string): boolean {
  const code = character.codePointAt(0) ?? 0;
  return code < 0x20 || (code >= 0x7f && code <= 0x9f);
}

function isSensitiveField(name: string): boolean {
  const normalized = name.toLowerCase();
  return ["api_key", "api key", "token", "credential", "password", "secret"]
    .some((needle) => normalized.includes(needle));
}

function valueSummary(value: unknown): string {
  if (value === null) return "empty";
  if (typeof value === "string") return `text (${[...value].length} chars)`;
  if (typeof value === "boolean") return "boolean";
  if (typeof value === "number") return "number";
  if (Array.isArray(value)) return `list (${value.length} items)`;
  if (isRecord(value)) return `object (${Object.keys(value).length} fields)`;
  return "value";
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

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
