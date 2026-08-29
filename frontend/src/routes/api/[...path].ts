import type { APIEvent } from "@solidjs/start/server";

const backendUrl = process.env.BACKEND_URL;
const defaultProxyTimeoutMs = 15_000;
const invalidProxyPathMessage = "Invalid API proxy path";
const invalidBackendUrlMessage = "BACKEND_URL is invalid";

const hopByHopHeaders = new Set([
  "connection",
  "keep-alive",
  "proxy-authenticate",
  "proxy-authorization",
  "te",
  "trailer",
  "transfer-encoding",
  "upgrade",
  "host",
  "content-length",
]);

const requestHeaderAllowlist = new Set([
  "accept",
  "accept-language",
  "authorization",
  "cookie",
  "dpop",
  "content-type",
  "mcp-method",
  "mcp-name",
  "mcp-protocol-version",
  "if-match",
  "if-none-match",
  "prefer",
  "x-request-id",
  "x-ugoite-human-approval",
  "x-correlation-id",
  "x-trace-id",
  "x-b3-traceid",
  "x-b3-spanid",
  "traceparent",
  "tracestate",
]);

const filterResponseHeaders = (headers: Headers): Headers => {
  const filtered = new Headers(headers);
  for (const header of hopByHopHeaders) {
    filtered.delete(header);
  }
  return filtered;
};

const filterRequestHeaders = (headers: Headers): Headers => {
  const filtered = new Headers();
  for (const [name, value] of headers.entries()) {
    const key = name.toLowerCase();
    if (requestHeaderAllowlist.has(key)) {
      filtered.set(key, value);
    }
  }
  return filtered;
};

const resolveRequestId = (headers: Headers): string => {
  const existingRequestId = headers.get("x-request-id") ??
    headers.get("x-correlation-id") ?? headers.get("x-trace-id");
  if (existingRequestId && existingRequestId.trim().length > 0) {
    return existingRequestId;
  }
  return crypto.randomUUID();
};

const ensureRequestId = (headers: Headers): string => {
  const requestId = resolveRequestId(headers);
  headers.set("x-request-id", requestId);
  if (!headers.has("x-correlation-id")) {
    headers.set("x-correlation-id", requestId);
  }
  return requestId;
};

const buildTargetUrl = (requestUrl: string, baseUrl: URL): URL => {
  const url = new URL(requestUrl);
  const path = url.pathname.replace(/^\/api/, "");
  const targetPath = path.length > 0 ? path : "/";
  const targetUrl = new URL(`${targetPath}${url.search}`, baseUrl);
  if (targetUrl.origin !== baseUrl.origin) {
    throw new Error("cross-origin proxy target is not allowed");
  }
  return targetUrl;
};

const resolveProxyTimeoutMs = (): number => {
  const rawTimeout = process.env.UGOITE_PROXY_TIMEOUT_MS;
  if (!rawTimeout) {
    return defaultProxyTimeoutMs;
  }
  const parsed = Number.parseInt(rawTimeout, 10);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    return defaultProxyTimeoutMs;
  }
  return parsed;
};

const handleProxyError = (
  error: unknown,
  requestMethod: string,
  targetUrl: URL,
  timeoutMs: number,
): Response => {
  if (error instanceof Error && error.name === "AbortError") {
    process.stderr.write(
      `API proxy upstream timeout method=${requestMethod} target=${targetUrl.toString()} timeout_ms=${timeoutMs}\n`,
    );
    return new Response("Backend request timed out", { status: 504 });
  }
  const message =
    `API proxy upstream request failed method=${requestMethod} target=${targetUrl.toString()} ` +
    `error=${error instanceof Error ? error.message : String(error)}\n`;
  process.stderr.write(message);
  return new Response("Backend service unavailable", { status: 502 });
};

const handleInvalidTargetPath = (
  error: unknown,
  requestMethod: string,
  requestUrl: string,
): Response => {
  const message =
    `API proxy rejected invalid target method=${requestMethod} request=${requestUrl} ` +
    `error=${error instanceof Error ? error.message : String(error)}\n`;
  process.stderr.write(message);
  return new Response(invalidProxyPathMessage, { status: 400 });
};

const handleInvalidBackendUrl = (error: unknown): Response => {
  process.stderr.write(
    `API proxy backend misconfiguration error=${
      error instanceof Error ? error.message : String(error)
    }\n`,
  );
  return new Response(invalidBackendUrlMessage, { status: 500 });
};

const proxyRequest = async (event: APIEvent): Promise<Response> => {
  if (!backendUrl) {
    return new Response("BACKEND_URL is not configured", { status: 500 });
  }

  const request = event.request;
  let backendBaseUrl: URL;
  try {
    backendBaseUrl = new URL(backendUrl);
  } catch (error) {
    return handleInvalidBackendUrl(error);
  }
  let targetUrl: URL;
  try {
    targetUrl = buildTargetUrl(request.url, backendBaseUrl);
  } catch (error) {
    return handleInvalidTargetPath(error, request.method, request.url);
  }
  const headers = filterRequestHeaders(request.headers);
  const requestId = ensureRequestId(headers);
  const timeoutMs = resolveProxyTimeoutMs();
  const controller = new AbortController();
  const timeoutHandle = setTimeout(() => controller.abort(), timeoutMs);
  const init: RequestInit = {
    method: request.method,
    headers,
    redirect: "manual",
    signal: controller.signal,
  };

  if (request.method !== "GET" && request.method !== "HEAD") {
    const body = await request.arrayBuffer();
    if (body.byteLength > 0) {
      init.body = body;
    }
  }

  try {
    const response = await fetch(targetUrl, init);
    const responseHeaders = filterResponseHeaders(response.headers);
    if (!responseHeaders.has("x-request-id")) {
      responseHeaders.set("x-request-id", requestId);
    }
    return new Response(response.body, {
      status: response.status,
      statusText: response.statusText,
      headers: responseHeaders,
    });
  } catch (error) {
    return handleProxyError(error, request.method, targetUrl, timeoutMs);
  } finally {
    clearTimeout(timeoutHandle);
  }
};

export const GET = proxyRequest;
export const POST = proxyRequest;
export const PUT = proxyRequest;
export const PATCH = proxyRequest;
export const DELETE = proxyRequest;
export const OPTIONS = proxyRequest;
export const HEAD = proxyRequest;
