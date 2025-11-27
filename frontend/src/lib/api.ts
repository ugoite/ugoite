export const getBackendBase = (): string => {
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

export const apiFetch = async (path = "/", options?: RequestInit) => {
	const base = getBackendBase();
	let url: string;
	if (/^https?:\/\//.test(base)) {
		url = `${base.replace(/\/$/, "")}${path.startsWith("/") ? path : `/${path}`}`;
	} else {
		// relative path; base probably like '/api'
		url = `${base}${path.startsWith("/") ? path : `/${path}`}`;
	}
	console.log(`apiFetch: ${url}`);
	return fetch(url, options);
};
