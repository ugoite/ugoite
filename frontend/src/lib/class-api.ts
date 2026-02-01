import type { Class, ClassCreatePayload } from "./types";
import { apiFetch } from "./api";

/**
 * Class API client
 */
export const classApi = {
	async listTypes(workspaceId: string): Promise<string[]> {
		const res = await apiFetch(`/workspaces/${workspaceId}/classes/types`);
		if (!res.ok) {
			throw new Error(`Failed to list class types: ${res.statusText}`);
		}
		return (await res.json()) as string[];
	},

	async list(workspaceId: string): Promise<Class[]> {
		const res = await apiFetch(`/workspaces/${workspaceId}/classes`);
		if (!res.ok) throw new Error(`Failed to list classes: ${res.statusText}`);
		return (await res.json()) as Class[];
	},
	async get(workspaceId: string, className: string): Promise<Class> {
		const encodedName = encodeURIComponent(className);
		const res = await apiFetch(`/workspaces/${workspaceId}/classes/${encodedName}`);
		if (!res.ok) throw new Error(`Failed to get class: ${res.statusText}`);
		return (await res.json()) as Class;
	},
	async create(workspaceId: string, payload: ClassCreatePayload): Promise<Class> {
		const res = await apiFetch(`/workspaces/${workspaceId}/classes`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(payload),
		});
		if (!res.ok) throw new Error(`Failed to create class: ${res.statusText}`);
		return (await res.json()) as Class;
	},
};
