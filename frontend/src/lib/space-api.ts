import type { TestConnectionPayload, Space, SpacePatchPayload } from "./types";
import { apiFetch } from "./api";

/**
 * Space API client
 */
export const spaceApi = {
	/** List all spaces */
	async list(): Promise<Space[]> {
		const res = await apiFetch("/spaces");
		if (!res.ok) {
			throw new Error(`Failed to list spaces: ${res.statusText}`);
		}
		return (await res.json()) as Space[];
	},

	/** Create a new space */
	async create(name: string): Promise<{ id: string; name: string }> {
		const res = await apiFetch("/spaces", {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify({ name }),
		});
		if (!res.ok) {
			const error = (await res.json()) as { detail?: string };
			throw new Error(error.detail || `Failed to create space: ${res.statusText}`);
		}
		return (await res.json()) as { id: string; name: string };
	},

	/** Get space by ID */
	async get(id: string): Promise<Space> {
		const res = await apiFetch(`/spaces/${id}`);
		if (!res.ok) {
			throw new Error(`Failed to get space: ${res.statusText}`);
		}
		return (await res.json()) as Space;
	},

	/** Patch space metadata/settings */
	async patch(id: string, payload: SpacePatchPayload): Promise<Space> {
		const res = await apiFetch(`/spaces/${id}`, {
			method: "PATCH",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(payload),
		});
		if (!res.ok) {
			const error = (await res.json()) as { detail?: string };
			throw new Error(error.detail || `Failed to patch space: ${res.statusText}`);
		}
		return (await res.json()) as Space;
	},

	/** Test storage connection */
	async testConnection(id: string, payload: TestConnectionPayload): Promise<{ status: string }> {
		const res = await apiFetch(`/spaces/${id}/test-connection`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(payload),
		});
		if (!res.ok) {
			const error = (await res.json()) as { detail?: string };
			throw new Error(error.detail || `Failed to test connection: ${res.statusText}`);
		}
		return (await res.json()) as { status: string };
	},
};
