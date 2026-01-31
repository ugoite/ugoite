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

const filterHeaders = (headers: Headers): Headers => {
	const filtered = new Headers(headers);
	for (const header of hopByHopHeaders) {
		filtered.delete(header);
	}
	return filtered;
};

const buildTargetUrl = (requestUrl: string, baseUrl: string): URL => {
	const url = new URL(requestUrl);
	const path = url.pathname.replace(/^\/api/, "");
	const targetPath = path.length > 0 ? path : "/";
	return new URL(`${targetPath}${url.search}`, baseUrl);
};

const proxyRequest = async (event: APIEvent): Promise<Response> => {
	if (!backendUrl) {
		return new Response("BACKEND_URL is not configured", { status: 500 });
	}

	const request = event.request;
	const targetUrl = buildTargetUrl(request.url, backendUrl);
	const headers = filterHeaders(request.headers);
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

	const response = await fetch(targetUrl, init);
	const responseHeaders = filterHeaders(response.headers);
	return new Response(response.body, {
		status: response.status,
		statusText: response.statusText,
		headers: responseHeaders,
	});
};

export const GET = proxyRequest;
export const POST = proxyRequest;
export const PUT = proxyRequest;
export const PATCH = proxyRequest;
export const DELETE = proxyRequest;
export const OPTIONS = proxyRequest;
export const HEAD = proxyRequest;
