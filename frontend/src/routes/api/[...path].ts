import type { APIEvent } from "@solidjs/start/server";
import { buildServerAuthCookie, readAuthCookie } from "~/lib/auth-cookie";

const backendUrl = process.env.BACKEND_URL;
const defaultProxyTimeoutMs = 15_000;
const devAuthProxyToken = process.env.UGOITE_DEV_AUTH_PROXY_TOKEN;
const devAuthProxyTokenHeader = "x-ugoite-dev-auth-proxy-token";
const devPasskeyContext = process.env.UGOITE_DEV_PASSKEY_CONTEXT;
const devPasskeyContextHeader = "x-ugoite-dev-passkey-context";
const proxyTokenAuthPaths = new Set(["/auth/config", "/auth/login", "/auth/mock-oauth"]);
const passkeyContextAuthPaths = new Set(["/auth/login"]);
const authCookieProxyPaths = new Set(["/auth/login", "/auth/mock-oauth"]);

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
	"content-type",
	"if-match",
	"if-none-match",
	"prefer",
	"x-request-id",
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
	const existingRequestId =
		headers.get("x-request-id") ?? headers.get("x-correlation-id") ?? headers.get("x-trace-id");
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

const buildTargetUrl = (requestUrl: string, baseUrl: string): URL => {
	const url = new URL(requestUrl);
	const path = url.pathname.replace(/^\/api/, "");
	const targetPath = path.length > 0 ? path : "/";
	return new URL(`${targetPath}${url.search}`, baseUrl);
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

const requestUsesHttps = (request: Request): boolean => {
	const forwardedProto = request.headers.get("x-forwarded-proto");
	const effectiveProto = forwardedProto?.split(",", 1)[0]?.trim().toLowerCase();
	if (effectiveProto) {
		return effectiveProto === "https";
	}
	return new URL(request.url).protocol === "https:";
};

const applyProxyCredentials = (headers: Headers, cookieHeader: string | null): void => {
	if (!headers.has("authorization")) {
		const proxyBearerToken = readAuthCookie(cookieHeader);
		if (proxyBearerToken) {
			headers.set("authorization", `Bearer ${proxyBearerToken}`);
		}
	}
	if (!headers.has("x-api-key")) {
		const proxyApiKey = process.env.UGOITE_AUTH_API_KEY;
		if (proxyApiKey) {
			headers.set("x-api-key", proxyApiKey);
		}
	}
};

const applyDevAuthProxyToken = (headers: Headers, pathname: string): void => {
	if (!proxyTokenAuthPaths.has(pathname)) {
		return;
	}
	if (!devAuthProxyToken?.trim()) {
		return;
	}
	headers.set(devAuthProxyTokenHeader, devAuthProxyToken);
};

const applyDevPasskeyContext = (headers: Headers, pathname: string): void => {
	if (!passkeyContextAuthPaths.has(pathname)) {
		return;
	}
	if (!devPasskeyContext?.trim()) {
		return;
	}
	headers.set(devPasskeyContextHeader, devPasskeyContext);
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

const isRecord = (value: unknown): value is Record<string, unknown> =>
	typeof value === "object" && value !== null;

const invalidAuthProxyResponse = (requestId: string): Response =>
	new Response(
		JSON.stringify({ detail: "Frontend auth proxy received an invalid login response." }),
		{
			status: 502,
			headers: {
				"content-type": "application/json",
				"x-request-id": requestId,
			},
		},
	);

const rewriteAuthProxyResponse = async (
	response: Response,
	requestId: string,
	secure: boolean,
): Promise<Response> => {
	const payload = (await response.json()) as unknown;
	if (!isRecord(payload)) {
		return invalidAuthProxyResponse(requestId);
	}
	const bearerToken = payload.bearer_token;
	if (typeof bearerToken !== "string") {
		return invalidAuthProxyResponse(requestId);
	}
	const userId = payload.user_id;
	if (typeof userId !== "string") {
		return invalidAuthProxyResponse(requestId);
	}
	const expiresAt = payload.expires_at;
	if (typeof expiresAt !== "number") {
		return invalidAuthProxyResponse(requestId);
	}
	const responseHeaders = filterResponseHeaders(response.headers);
	responseHeaders.delete("content-length");
	responseHeaders.delete("set-cookie");
	if (!responseHeaders.has("x-request-id")) {
		responseHeaders.set("x-request-id", requestId);
	}
	responseHeaders.append("set-cookie", buildServerAuthCookie(bearerToken, expiresAt, { secure }));
	const redactedPayload = { ...payload };
	delete redactedPayload.bearer_token;
	return new Response(JSON.stringify(redactedPayload), {
		status: response.status,
		statusText: response.statusText,
		headers: responseHeaders,
	});
};

const proxyRequest = async (event: APIEvent): Promise<Response> => {
	if (!backendUrl) {
		return new Response("BACKEND_URL is not configured", { status: 500 });
	}

	const request = event.request;
	const targetUrl = buildTargetUrl(request.url, backendUrl);
	const headers = filterRequestHeaders(request.headers);
	const requestId = ensureRequestId(headers);
	applyProxyCredentials(headers, request.headers.get("cookie"));
	applyDevAuthProxyToken(headers, targetUrl.pathname);
	applyDevPasskeyContext(headers, targetUrl.pathname);

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
		if (response.ok && authCookieProxyPaths.has(targetUrl.pathname)) {
			return await rewriteAuthProxyResponse(response, requestId, requestUsesHttps(request));
		}
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
