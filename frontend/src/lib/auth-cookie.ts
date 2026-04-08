export const authCookieName = "ugoite_auth_bearer_token";

export const readAuthCookie = (cookieHeader: string | null): string | null => {
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

export const buildServerAuthCookie = (
	token: string,
	expiresAt: number | undefined,
	options: {
		secure: boolean;
		nowMs?: number;
	},
): string => {
	const segments = [
		`${authCookieName}=${encodeURIComponent(token)}`,
		"Path=/",
		"HttpOnly",
		"SameSite=Lax",
	];
	if (options.secure) {
		segments.push("Secure");
	}
	if (typeof expiresAt === "number" && Number.isFinite(expiresAt)) {
		const expiresDate = new Date(expiresAt * 1000);
		const maxAge = Math.max(
			0,
			Math.floor((expiresDate.getTime() - (options.nowMs ?? Date.now())) / 1000),
		);
		segments.push(`Max-Age=${maxAge}`);
		segments.push(`Expires=${expiresDate.toUTCString()}`);
	}
	return segments.join("; ");
};

export const buildClearedAuthCookie = (options: { secure: boolean }): string => {
	const segments = [
		`${authCookieName}=`,
		"Path=/",
		"HttpOnly",
		"SameSite=Lax",
		"Max-Age=0",
		"Expires=Thu, 01 Jan 1970 00:00:00 GMT",
	];
	if (options.secure) {
		segments.push("Secure");
	}
	return segments.join("; ");
};
