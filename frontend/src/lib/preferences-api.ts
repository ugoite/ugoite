import { apiFetch } from "./api";
import type { UserPreferences, UserPreferencesPatchPayload } from "./types";

const formatApiError = async (res: Response, fallback: string): Promise<string> => {
	try {
		const payload = (await res.json()) as { detail?: unknown };
		if (typeof payload.detail === "string" && payload.detail.trim()) {
			return payload.detail;
		}
		if (payload.detail) {
			return JSON.stringify(payload.detail);
		}
	} catch {
		/* v8 ignore start */
		return fallback;
		/* v8 ignore stop */
	}
	return fallback;
};

export const preferencesApi = {
	async getMe(): Promise<UserPreferences> {
		const res = await apiFetch("/preferences/me");
		if (!res.ok) {
			throw new Error(await formatApiError(res, `Failed to load preferences: ${res.statusText}`));
		}
		return (await res.json()) as UserPreferences;
	},

	async patchMe(payload: UserPreferencesPatchPayload): Promise<UserPreferences> {
		const res = await apiFetch("/preferences/me", {
			method: "PATCH",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(payload),
		});
		if (!res.ok) {
			throw new Error(await formatApiError(res, `Failed to update preferences: ${res.statusText}`));
		}
		return (await res.json()) as UserPreferences;
	},
};
