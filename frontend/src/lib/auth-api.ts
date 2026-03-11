import { apiFetch } from "./api";

export type DevAuthConfig = {
	mode: "manual-totp" | "mock-oauth";
	username_hint: string;
	supports_manual_totp: boolean;
	supports_mock_oauth: boolean;
};

export type DevAuthLoginResponse = {
	bearer_token: string;
	user_id: string;
	expires_at: number;
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

export const authApi = {
	async getDevConfig(): Promise<DevAuthConfig> {
		const response = await apiFetch("/auth/dev/config", { trackLoading: false });
		if (!response.ok) {
			throw new Error(`Failed to load local auth config: ${response.statusText}`);
		}
		return (await response.json()) as DevAuthConfig;
	},

	async loginWithTotp(username: string, totpCode: string): Promise<DevAuthLoginResponse> {
		const response = await apiFetch("/auth/dev/login", {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify({ username, totp_code: totpCode }),
		});
		if (!response.ok) {
			throw new Error(await formatAuthError(response, `Failed to log in: ${response.statusText}`));
		}
		return (await response.json()) as DevAuthLoginResponse;
	},

	async loginWithMockOAuth(): Promise<DevAuthLoginResponse> {
		const response = await apiFetch("/auth/dev/mock-oauth", {
			method: "POST",
		});
		if (!response.ok) {
			throw new Error(
				await formatAuthError(response, `Failed to start mock OAuth login: ${response.statusText}`),
			);
		}
		return (await response.json()) as DevAuthLoginResponse;
	},
};
