import { createSignal } from "solid-js";
import type { Form, FormCreatePayload } from "./types";
import { formApi } from "./form-api";

export function createFormStore(spaceId: () => string) {
	const [forms, setForms] = createSignal<Form[]>([]);
	const [loading, setLoading] = createSignal(false);
	const [error, setError] = createSignal<string | null>(null);

	async function loadForms(): Promise<void> {
		setLoading(true);
		setError(null);
		try {
			const data = await formApi.list(spaceId());
			setForms(data);
		} catch (e) {
			setError(e instanceof Error ? e.message : "Failed to load forms");
		} finally {
			setLoading(false);
		}
	}

	async function createForm(payload: FormCreatePayload): Promise<Form> {
		setError(null);
		const created = await formApi.create(spaceId(), payload);
		await loadForms();
		return created;
	}

	async function getForm(name: string): Promise<Form> {
		setError(null);
		return formApi.get(spaceId(), name);
	}

	async function listTypes(): Promise<string[]> {
		setError(null);
		return formApi.listTypes(spaceId());
	}

	return {
		forms,
		loading,
		error,
		loadForms,
		createForm,
		getForm,
		listTypes,
	};
}

export type FormStore = ReturnType<typeof createFormStore>;
