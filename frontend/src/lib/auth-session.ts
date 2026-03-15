const authCookieName = "ugoite_auth_bearer_token";

const findCookieDescriptor = () => {
	let target: object | null = document;
	while (target) {
		const descriptor = Object.getOwnPropertyDescriptor(target, "cookie");
		if (descriptor) return descriptor;
		target = Object.getPrototypeOf(target);
	}
	return null;
};

const writeDocumentCookie = (value: string) => {
	const descriptor = findCookieDescriptor();
	if (descriptor?.set) {
		descriptor.set.call(document, value);
	}
};

const cookieLifetime = (expiresAt: number | undefined): string => {
	if (typeof expiresAt !== "number" || !Number.isFinite(expiresAt)) {
		return "";
	}
	const expiresDate = new Date(expiresAt * 1000);
	const maxAge = Math.max(0, Math.floor((expiresDate.getTime() - Date.now()) / 1000));
	return `; Max-Age=${maxAge}; Expires=${expiresDate.toUTCString()}`;
};

export const setAuthTokenCookie = (token: string, expiresAt?: number): void => {
	if (typeof document === "undefined") {
		return;
	}
	const secure = window.location.protocol === "https:" ? "; Secure" : "";
	writeDocumentCookie(
		`${authCookieName}=${encodeURIComponent(token)}; Path=/; SameSite=Lax${secure}${cookieLifetime(expiresAt)}`,
	);
};

export const clearAuthTokenCookie = (): void => {
	if (typeof document === "undefined") {
		return;
	}
	writeDocumentCookie(`${authCookieName}=; Path=/; Max-Age=0; SameSite=Lax`);
};
