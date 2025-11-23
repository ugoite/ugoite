export const getBackendBase = (): string => {
	// The frontend requires `VITE_BACKEND_URL` to be set during development and should
	// be used to indicate how frontend code calls the backend:
	// - For local/dev usage set `VITE_BACKEND_URL=/api` and the dev server proxy will
	//   forward requests to the backend.
	// - For production builds set a public absolute URL, e.g. `https://api.example.com`.
	const env = import.meta.env as ImportMetaEnv;
	const url = env.VITE_BACKEND_URL;
	if (!url || url.length === 0) {
		// In dev mode we enforce that `VITE_BACKEND_URL` is set; throw so misconfiguration
		// is caught early. In production, fall back to '/api' for safety if needed.
		if (import.meta.env.DEV) {
			throw new Error(
				"VITE_BACKEND_URL must be set during development. Example: VITE_BACKEND_URL=/api",
			);
		}
		return "/api";
	}
	return url.replace(/\/$/, ""); // remove trailing slash
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
