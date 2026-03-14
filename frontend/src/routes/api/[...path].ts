import type { APIEvent } from "@solidjs/start/server";

const backendUrl = process.env.BACKEND_URL;
const defaultProxyTimeoutMs = 15_000;
const authCookieName = "ugoite_auth_bearer_token";
const devAuthProxyToken = process.env.UGOITE_DEV_AUTH_PROXY_TOKEN;
const devAuthProxyTokenHeader = "x-ugoite-dev-auth-proxy-token";

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

const readAuthCookie = (cookieHeader: string | null): string | null => {
	if (!cookieHeader) {
		return null;
	}
	for (const segment of cookieHeader.split(";")) {
		const [rawName, ...rest] = segment.split("=");
		if (rawName?.trim() !== authCookieName) {
			continue;
		}
		const rawValue = rest.join("=").trim();
		if (!rawValue) {
			return null;
		}
		try {
			return decodeURIComponent(rawValue);
		} catch {
			return rawValue;
		}
	}
	return null;
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
	if (!pathname.startsWith("/auth/dev/")) {
		return;
	}
	if (!devAuthProxyToken?.trim()) {
		return;
	}
	headers.set(devAuthProxyTokenHeader, devAuthProxyToken);
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
