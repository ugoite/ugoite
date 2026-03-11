const authCookieName = "ugoite_auth_bearer_token";

export const setAuthTokenCookie = (token: string): void => {
	if (typeof document === "undefined") {
		return;
	}
	const secure = window.location.protocol === "https:" ? "; Secure" : "";
	document.cookie = `${authCookieName}=${encodeURIComponent(token)}; Path=/; SameSite=Lax${secure}`;
};

export const clearAuthTokenCookie = (): void => {
	if (typeof document === "undefined") {
		return;
	}
	document.cookie = `${authCookieName}=; Path=/; Max-Age=0; SameSite=Lax`;
};
