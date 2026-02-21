import type { APIEvent } from "@solidjs/start/server";

const backendUrl = process.env.BACKEND_URL;

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

const buildTargetUrl = (requestUrl: string, baseUrl: string): URL => {
	const url = new URL(requestUrl);
	const path = url.pathname.replace(/^\/api/, "");
	// When the path is exactly /api, fallback to / so we proxy to the backend root.
	const targetPath = path.length > 0 ? path : "/";
	return new URL(`${targetPath}${url.search}`, baseUrl);
};

const proxyRequest = async (event: APIEvent): Promise<Response> => {
	if (!backendUrl) {
		return new Response("BACKEND_URL is not configured", { status: 500 });
	}

	const request = event.request;
	const targetUrl = buildTargetUrl(request.url, backendUrl);
	const headers = filterRequestHeaders(request.headers);
	if (!headers.has("authorization")) {
		const proxyBearerToken =
			process.env.UGOITE_AUTH_BEARER_TOKEN ?? process.env.UGOITE_FRONTEND_BEARER_TOKEN;
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
	const init: RequestInit = {
		method: request.method,
		headers,
		redirect: "manual",
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
		return new Response(response.body, {
			status: response.status,
			statusText: response.statusText,
			headers: responseHeaders,
		});
	} catch (error) {
		const message =
			`API proxy upstream request failed method=${request.method} target=${targetUrl.toString()} ` +
			`error=${error instanceof Error ? error.message : String(error)}\n`;
		process.stderr.write(message);
		return new Response("Backend service unavailable", { status: 502 });
	}
};

export const GET = proxyRequest;
export const POST = proxyRequest;
export const PUT = proxyRequest;
export const PATCH = proxyRequest;
export const DELETE = proxyRequest;
export const OPTIONS = proxyRequest;
export const HEAD = proxyRequest;
