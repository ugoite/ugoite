import type { Form, FormCreatePayload } from "./types";
import { apiFetch } from "./api";

/**
 * Form API client
 */
export const formApi = {
	async listTypes(spaceId: string): Promise<string[]> {
		const res = await apiFetch(`/spaces/${spaceId}/forms/types`);
		if (!res.ok) {
			throw new Error(`Failed to list form types: ${res.statusText}`);
		}
		return (await res.json()) as string[];
	},

	async list(spaceId: string): Promise<Form[]> {
		const res = await apiFetch(`/spaces/${spaceId}/forms`);
		if (!res.ok) throw new Error(`Failed to list forms: ${res.statusText}`);
		return (await res.json()) as Form[];
	},
	async get(spaceId: string, formName: string): Promise<Form> {
		const encodedName = encodeURIComponent(formName);
		const res = await apiFetch(`/spaces/${spaceId}/forms/${encodedName}`);
		if (!res.ok) throw new Error(`Failed to get form: ${res.statusText}`);
		return (await res.json()) as Form;
	},
	async create(spaceId: string, payload: FormCreatePayload): Promise<Form> {
		const res = await apiFetch(`/spaces/${spaceId}/forms`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(payload),
		});
		if (!res.ok) throw new Error(`Failed to create form: ${res.statusText}`);
		return (await res.json()) as Form;
	},
};
