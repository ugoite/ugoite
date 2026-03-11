import { loadingState } from "./loading";
import { getRequestEvent } from "solid-js/web";

export const getBackendBase = (): string => {
	// In test environment, use absolute URL for MSW to intercept
	/* v8 ignore start */
	if (typeof process !== "undefined" && process.env?.NODE_ENV === "test") {
		return "http://localhost:3000/api";
	}
	/* v8 ignore stop */
	/* v8 ignore start */
	// In SSR, Node's fetch requires an absolute URL.
	// Default to the frontend dev server origin used in e2e/dev.
	if (typeof window === "undefined") {
		const env = process.env ?? {};
		const origin = env.FRONTEND_ORIGIN || env.ORIGIN || "http://localhost:3000";
		return `${origin.replace(/\/$/, "")}/api`;
	}
	// Always use /api which is proxied to the backend in development
	// and should be served by the backend or a reverse proxy in production.
	return "/api";
	/* v8 ignore stop */
};

export const joinUrl = (base: string, path = "/"): string => {
	if (!base) return path;
	const b = base.replace(/\/$/, "");
	const p = path.replace(/^\//, "");
	return `${b}/${p}`;
};

export type ApiFetchOptions = RequestInit & {
	trackLoading?: boolean;
};

const applyServerRequestAuth = (requestInit: RequestInit): RequestInit => {
	if (typeof window !== "undefined") {
		return requestInit;
	}

	const event = getRequestEvent();
	if (!event) {
		return requestInit;
	}

	const headers = new Headers(requestInit.headers);
	const cookieHeader = event.request.headers.get("cookie");
	if (cookieHeader && !headers.has("cookie")) {
		headers.set("cookie", cookieHeader);
	}
	const authorizationHeader = event.request.headers.get("authorization");
	if (authorizationHeader && !headers.has("authorization")) {
		headers.set("authorization", authorizationHeader);
	}
	return {
		...requestInit,
		headers,
	};
};

export const apiFetch = async (path = "/", options?: ApiFetchOptions) => {
	const base = getBackendBase();
	let url: string;
	/* v8 ignore start */
	if (/^https?:\/\//.test(base)) {
		url = `${base.replace(/\/$/, "")}${path.startsWith("/") ? path : `/${path}`}`;
	} else {
		// relative path; base probably like '/api'
		url = `${base}${path.startsWith("/") ? path : `/${path}`}`;
	}
	/* v8 ignore stop */
	const shouldTrackLoading = options?.trackLoading ?? true;
	if (shouldTrackLoading) {
		loadingState.start();
	}
	const { trackLoading: _trackLoading, ...requestInit } = options ?? {};
	const serverAwareRequestInit = applyServerRequestAuth(requestInit);
	try {
		return await fetch(url, serverAwareRequestInit);
	} finally {
		if (shouldTrackLoading) {
			loadingState.stop();
		}
	}
};
