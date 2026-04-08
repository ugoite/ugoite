import { apiFetch } from "./api";

export type AuthConfig = {
	mode: "passkey-totp" | "mock-oauth";
	usernameHint: string;
	supportsPasskeyTotp: boolean;
	supportsMockOauth: boolean;
};

export type AuthLoginResponse = {
	userId: string;
	expiresAt: number;
};

const formatAuthError = async (response: Response, fallback: string): Promise<string> => {
	try {
		const payload = (await response.json()) as { detail?: unknown };
		if (typeof payload.detail === "string" && payload.detail.trim()) {
			return payload.detail;
		}
		if (payload.detail && typeof payload.detail === "object") {
			return JSON.stringify(payload.detail);
		}
	} catch {
		// ignore parse errors and fall back to status text
	}
	return fallback;
};

const readString = (payload: Record<string, unknown>, key: string): string => {
	const value = payload[key];
	if (typeof value !== "string") {
		throw new Error(`Invalid auth response: ${key} must be a string.`);
	}
	return value;
};

const readBoolean = (payload: Record<string, unknown>, key: string): boolean => {
	const value = payload[key];
	if (typeof value !== "boolean") {
		throw new Error(`Invalid auth response: ${key} must be a boolean.`);
	}
	return value;
};

const readNumber = (payload: Record<string, unknown>, key: string): number => {
	const value = payload[key];
	if (typeof value !== "number") {
		throw new Error(`Invalid auth response: ${key} must be a number.`);
	}
	return value;
};

export const authApi = {
	async getConfig(): Promise<AuthConfig> {
		const response = await apiFetch("/auth/config", { trackLoading: false });
		if (!response.ok) {
			throw new Error(
				await formatAuthError(response, `Failed to load auth config: ${response.statusText}`),
			);
		}
		const payload = (await response.json()) as Record<string, unknown>;
		return {
			mode: readString(payload, "mode") as AuthConfig["mode"],
			usernameHint: readString(payload, "username_hint"),
			supportsPasskeyTotp: readBoolean(payload, "supports_passkey_totp"),
			supportsMockOauth: readBoolean(payload, "supports_mock_oauth"),
		};
	},

	async loginWithPasskeyTotp(username: string, totpCode: string): Promise<AuthLoginResponse> {
		const loginPayload = Object.fromEntries([
			["username", username],
			["totp_code", totpCode],
		]);
		const response = await apiFetch("/auth/login", {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(loginPayload),
		});
		if (!response.ok) {
			throw new Error(await formatAuthError(response, `Failed to log in: ${response.statusText}`));
		}
		const payload = (await response.json()) as Record<string, unknown>;
		return {
			userId: readString(payload, "user_id"),
			expiresAt: readNumber(payload, "expires_at"),
		};
	},

	async loginWithMockOauth(): Promise<AuthLoginResponse> {
		const response = await apiFetch("/auth/mock-oauth", {
			method: "POST",
		});
		if (!response.ok) {
			throw new Error(
				await formatAuthError(response, `Failed to start mock OAuth login: ${response.statusText}`),
			);
		}
		const payload = (await response.json()) as Record<string, unknown>;
		return {
			userId: readString(payload, "user_id"),
			expiresAt: readNumber(payload, "expires_at"),
		};
	},
};
