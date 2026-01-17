export const getBackendBase = (): string => {
	// In test environment, use absolute URL for MSW to intercept
	if (typeof process !== "undefined" && process.env?.NODE_ENV === "test") {
		return "http://localhost:3000";
	}
	// In SSR, Node's fetch requires an absolute URL.
	// Default to the frontend dev server origin used in e2e/dev.
	if (typeof window === "undefined") {
		const env = process.env ?? {};
		const origin = env.FRONTEND_ORIGIN || env.ORIGIN || "http://localhost:3000";
		return origin.replace(/\/$/, "");
	}
	// Use same-origin paths in the browser; dev server proxies /workspaces.
	return "";
};

export const joinUrl = (base: string, path = "/"): string => {
	if (!base) return path;
	const b = base.replace(/\/$/, "");
	const p = path.replace(/^\//, "");
	return `${b}/${p}`;
};

export const apiFetch = async (path = "/", options?: RequestInit) => {
	const base = getBackendBase();
	let url: string;
	if (/^https?:\/\//.test(base)) {
		url = `${base.replace(/\/$/, "")}${path.startsWith("/") ? path : `/${path}`}`;
	} else {
		// relative path; base is empty for same-origin requests
		url = `${base}${path.startsWith("/") ? path : `/${path}`}`;
	}
	return fetch(url, options);
};
