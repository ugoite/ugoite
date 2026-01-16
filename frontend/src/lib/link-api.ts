import type { WorkspaceLink } from "./types";
import { apiFetch } from "./api";

/** Link API client */
export const linkApi = {
	async create(
		workspaceId: string,
		payload: { source: string; target: string; kind: string },
	): Promise<WorkspaceLink> {
		const res = await apiFetch(`/workspaces/${workspaceId}/links`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(payload),
		});
		if (!res.ok) {
			const error = (await res.json()) as { detail?: string };
			throw new Error(error.detail || `Failed to create link: ${res.statusText}`);
		}
		return (await res.json()) as WorkspaceLink;
	},

	async list(workspaceId: string): Promise<WorkspaceLink[]> {
		const res = await apiFetch(`/workspaces/${workspaceId}/links`);
		if (!res.ok) {
			throw new Error(`Failed to list links: ${res.statusText}`);
		}
		return (await res.json()) as WorkspaceLink[];
	},

	async delete(workspaceId: string, linkId: string): Promise<{ status: string; id: string }> {
		const res = await apiFetch(`/workspaces/${workspaceId}/links/${linkId}`, { method: "DELETE" });
		if (!res.ok) {
			const error = (await res.json()) as { detail?: string };
			throw new Error(error.detail || `Failed to delete link: ${res.statusText}`);
		}
		return (await res.json()) as { status: string; id: string };
	},
};
