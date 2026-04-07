const authCookieName = "ugoite_auth_bearer_token";
export const authSessionChangedEventName = "ugoite-auth-session-changed";
let explicitAuthTokenPresence: boolean | null = null;
let explicitAuthTokenPresenceAtMs = 0;
const explicitAuthTokenPresenceGraceMs = 1_000;

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

const dispatchAuthSessionChanged = () => {
	if (typeof window === "undefined" || typeof window.dispatchEvent !== "function") {
		return;
	}
	window.dispatchEvent(new Event(authSessionChangedEventName));
};

const readAuthTokenCookiePresence = (): boolean =>
	document.cookie
		.split(";")
		.map((part) => part.trim())
		.some((part) => {
			if (!part.startsWith(`${authCookieName}=`)) {
				return false;
			}
			return part.slice(authCookieName.length + 1).trim().length > 0;
		});

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
	explicitAuthTokenPresence = true;
	explicitAuthTokenPresenceAtMs = Date.now();
	dispatchAuthSessionChanged();
};

export const clearAuthTokenCookie = (): void => {
	explicitAuthTokenPresence = false;
	explicitAuthTokenPresenceAtMs = Date.now();
	if (typeof document === "undefined") {
		return;
	}
	writeDocumentCookie(`${authCookieName}=; Path=/; Max-Age=0; SameSite=Lax`);
	dispatchAuthSessionChanged();
};

export const hasAuthTokenCookie = (): boolean => {
	if (typeof document === "undefined") {
		return false;
	}
	const cookiePresence = readAuthTokenCookiePresence();
	if (explicitAuthTokenPresence === null) {
		return cookiePresence;
	}
	if (cookiePresence === explicitAuthTokenPresence) {
		explicitAuthTokenPresence = null;
		return cookiePresence;
	}
	if (Date.now() - explicitAuthTokenPresenceAtMs <= explicitAuthTokenPresenceGraceMs) {
		return explicitAuthTokenPresence;
	}
	explicitAuthTokenPresence = null;
	return cookiePresence;
};
