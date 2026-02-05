import type { SpaceLink } from "./types";
import { apiFetch } from "./api";

/** Link API client */
export const linkApi = {
	async create(
		spaceId: string,
		payload: { source: string; target: string; kind: string },
	): Promise<SpaceLink> {
		const res = await apiFetch(`/spaces/${spaceId}/links`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(payload),
		});
		if (!res.ok) {
			const error = (await res.json()) as { detail?: string };
			throw new Error(error.detail || `Failed to create link: ${res.statusText}`);
		}
		return (await res.json()) as SpaceLink;
	},

	async list(spaceId: string): Promise<SpaceLink[]> {
		const res = await apiFetch(`/spaces/${spaceId}/links`);
		if (!res.ok) {
			throw new Error(`Failed to list links: ${res.statusText}`);
		}
		return (await res.json()) as SpaceLink[];
	},

	async delete(spaceId: string, linkId: string): Promise<{ status: string; id: string }> {
		const res = await apiFetch(`/spaces/${spaceId}/links/${linkId}`, { method: "DELETE" });
		if (!res.ok) {
			const error = (await res.json()) as { detail?: string };
			throw new Error(error.detail || `Failed to delete link: ${res.statusText}`);
		}
		return (await res.json()) as { status: string; id: string };
	},
};
