import initializeWasm from "../generated/ugoite_wasm.wasm?init";
import { apiFetch, type ApiFetchOptions } from "../api";

export const UGOITE_API_OPERATIONS = [
  "auth.get_config",
  "auth.get_session",
  "auth.clear_session",
  "auth.accept_invitation",
  "auth.list_sessions",
  "auth.revoke_session",
  "preferences.get",
  "preferences.patch",
  "space.list",
  "space.create",
  "space.get",
  "space.patch",
  "space.test_connection",
  "space.members.list",
  "space.members.invite",
  "space.members.update_role",
  "space.members.revoke",
  "form.list_types",
  "form.list",
  "form.get",
  "form.upsert",
  "entry.list",
  "entry.get",
  "entry.create",
  "entry.update",
  "entry.delete",
  "entry.history",
  "entry.revision",
  "entry.restore",
  "entry.options",
  "search.keyword",
  "search.query",
  "sql.list",
  "sql.get",
  "sql.create",
  "sql.update",
  "sql.delete",
  "sql_session.create",
  "sql_session.get",
  "sql_session.count",
  "sql_session.rows",
  "agent.list",
  "agent.create",
  "agent.revoke",
  "access.get",
  "access.put",
  "asset.list",
  "asset.upload",
  "asset.delete",
] as const;

export type UgoiteApiOperation = (typeof UGOITE_API_OPERATIONS)[number];

type ProtocolHeader = {
  name: string;
  value: string;
};

type PreparedRequest = {
  operation: UgoiteApiOperation;
  method: "GET" | "POST" | "PUT" | "PATCH" | "DELETE";
  path: string;
  headers: ProtocolHeader[];
  body?: string;
  body_kind: "none" | "json" | "multipart";
};

type ProtocolErrorPayload = {
  kind: string;
  message: string;
  operation?: string;
  status?: number;
  detail?: unknown;
  payload?: unknown;
};

type ProtocolEnvelope<T> =
  | { ok: true; value: T }
  | { ok: false; error: ProtocolErrorPayload };

type UgoiteWasmExports = WebAssembly.Exports & {
  memory: WebAssembly.Memory;
  ugoite_protocol_version(): number;
  ugoite_alloc(length: number): number;
  ugoite_dealloc(pointer: number, length: number): void;
  ugoite_protocol_invoke(pointer: number, length: number): number;
  ugoite_protocol_result_pointer(): number;
  ugoite_protocol_result_length(): number;
  ugoite_protocol_clear_result(): void;
};

export const UGOITE_WASM_PROTOCOL_VERSION = 1;

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();
let exportsPromise: Promise<UgoiteWasmExports> | undefined;

const loadExports = async (): Promise<UgoiteWasmExports> => {
  exportsPromise ??= initializeWasm().then((instance) => {
    const exports = instance.exports as UgoiteWasmExports;
    const protocolVersion = exports.ugoite_protocol_version();
    if (protocolVersion !== UGOITE_WASM_PROTOCOL_VERSION) {
      throw new Error(
        `Unsupported Ugoite WASM protocol version: ${protocolVersion}`,
      );
    }
    return exports;
  });
  return await exportsPromise;
};

const invokeProtocol = async <T>(command: unknown): Promise<T> => {
  const exports = await loadExports();
  const input = textEncoder.encode(JSON.stringify(command));
  const inputPointer = exports.ugoite_alloc(input.byteLength);

  try {
    new Uint8Array(
      exports.memory.buffer,
      inputPointer,
      input.byteLength,
    ).set(input);
    const status = exports.ugoite_protocol_invoke(
      inputPointer,
      input.byteLength,
    );
    if (status !== 0) {
      throw new Error(`Ugoite WASM protocol invocation failed: ${status}`);
    }

    const outputPointer = exports.ugoite_protocol_result_pointer();
    const outputLength = exports.ugoite_protocol_result_length();
    const output = textDecoder.decode(
      new Uint8Array(
        exports.memory.buffer,
        outputPointer,
        outputLength,
      ).slice(),
    );
    const envelope = JSON.parse(output) as ProtocolEnvelope<T>;
    if (!envelope.ok) {
      throw new UgoiteApiError(envelope.error);
    }
    return envelope.value;
  } finally {
    exports.ugoite_dealloc(inputPointer, input.byteLength);
    exports.ugoite_protocol_clear_result();
  }
};

export const getWasmSupportedOperations = async (): Promise<
  UgoiteApiOperation[]
> => await invokeProtocol<UgoiteApiOperation[]>({ action: "operations" });

export class UgoiteApiError extends Error {
  readonly kind: string;
  readonly operation?: string;
  readonly status?: number;
  readonly detail?: unknown;
  readonly payload?: unknown;

  constructor(error: ProtocolErrorPayload) {
    super(error.message);
    this.name = "UgoiteApiError";
    this.kind = error.kind;
    this.operation = error.operation;
    this.status = error.status;
    this.detail = error.detail;
    this.payload = error.payload;
  }
}

export const prepareApiRequest = async (
  operation: UgoiteApiOperation,
  argumentsValue: Record<string, unknown> = {},
  body?: unknown,
): Promise<PreparedRequest> =>
  await invokeProtocol<PreparedRequest>({
    action: "prepare",
    operation,
    arguments: argumentsValue,
    body,
  });

const decodeApiResponse = async <T>(
  operation: UgoiteApiOperation,
  response: Response,
): Promise<T> => {
  const body = await response.text();
  return await invokeProtocol<T>({
    action: "decode",
    operation,
    response: {
      status: response.status,
      status_text: response.statusText,
      headers: [...response.headers.entries()].map(([name, value]) => ({
        name,
        value,
      })),
      body,
    },
  });
};

export type ProtocolFetchOptions =
  & Omit<
    ApiFetchOptions,
    "method" | "body"
  >
  & {
    body?: BodyInit | null;
  };

/**
 * Execute a named Ugoite operation.
 *
 * Rust/WASM owns HTTP method, encoded path/query, JSON serialization, and
 * response/error decoding. TypeScript owns the environment-specific fetch,
 * SSR auth forwarding, cookies, loading state, AbortSignal, and multipart
 * objects.
 */
export const protocolFetch = async <T>(
  operation: UgoiteApiOperation,
  argumentsValue: Record<string, unknown> = {},
  body?: unknown,
  options: ProtocolFetchOptions = {},
): Promise<T> => {
  const prepared = await prepareApiRequest(operation, argumentsValue, body);
  const headers = new Headers();
  for (const header of prepared.headers) {
    headers.set(header.name, header.value);
  }
  const optionHeaders = new Headers(options.headers);
  optionHeaders.forEach((value, name) => headers.set(name, value));

  const requestBody = prepared.body_kind === "json"
    ? prepared.body
    : options.body;
  const response = await apiFetch(prepared.path, {
    ...options,
    method: prepared.method,
    headers,
    body: requestBody,
  });
  return await decodeApiResponse<T>(operation, response);
};
