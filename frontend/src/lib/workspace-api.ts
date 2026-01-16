import type { TestConnectionPayload, Workspace, WorkspacePatchPayload } from "./types";
import { apiFetch } from "./api";

/**
 * Workspace API client
 */
export const workspaceApi = {
	/** List all workspaces */
	async list(): Promise<Workspace[]> {
		const res = await apiFetch("/workspaces");
		if (!res.ok) {
			throw new Error(`Failed to list workspaces: ${res.statusText}`);
		}
		return (await res.json()) as Workspace[];
	},

	/** Create a new workspace */
	async create(name: string): Promise<{ id: string; name: string }> {
		const res = await apiFetch("/workspaces", {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify({ name }),
		});
		if (!res.ok) {
			const error = (await res.json()) as { detail?: string };
			throw new Error(error.detail || `Failed to create workspace: ${res.statusText}`);
		}
		return (await res.json()) as { id: string; name: string };
	},

	/** Get workspace by ID */
	async get(id: string): Promise<Workspace> {
		const res = await apiFetch(`/workspaces/${id}`);
		if (!res.ok) {
			throw new Error(`Failed to get workspace: ${res.statusText}`);
		}
		return (await res.json()) as Workspace;
	},

	/** Patch workspace metadata/settings */
	async patch(id: string, payload: WorkspacePatchPayload): Promise<Workspace> {
		const res = await apiFetch(`/workspaces/${id}`, {
			method: "PATCH",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(payload),
		});
		if (!res.ok) {
			const error = (await res.json()) as { detail?: string };
			throw new Error(error.detail || `Failed to patch workspace: ${res.statusText}`);
		}
		return (await res.json()) as Workspace;
	},

	/** Test storage connection */
	async testConnection(id: string, payload: TestConnectionPayload): Promise<{ status: string }> {
		const res = await apiFetch(`/workspaces/${id}/test-connection`, {
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
