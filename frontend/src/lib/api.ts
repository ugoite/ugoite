import { loadingState } from "./loading";

export const getBackendBase = (): string => {
	// In test environment, use absolute URL for MSW to intercept
	if (typeof process !== "undefined" && process.env?.NODE_ENV === "test") {
		return "http://localhost:3000/api";
	}
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

export const apiFetch = async (path = "/", options?: ApiFetchOptions) => {
	const base = getBackendBase();
	let url: string;
	if (/^https?:\/\//.test(base)) {
		url = `${base.replace(/\/$/, "")}${path.startsWith("/") ? path : `/${path}`}`;
	} else {
		// relative path; base probably like '/api'
		url = `${base}${path.startsWith("/") ? path : `/${path}`}`;
	}
	const shouldTrackLoading = options?.trackLoading ?? true;
	if (shouldTrackLoading) {
		loadingState.start();
	}
	const { trackLoading: _trackLoading, ...requestInit } = options ?? {};
	try {
		return await fetch(url, requestInit);
	} finally {
		if (shouldTrackLoading) {
			loadingState.stop();
		}
	}
};
