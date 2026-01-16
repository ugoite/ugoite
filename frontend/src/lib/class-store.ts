import { createSignal } from "solid-js";
import type { Class, ClassCreatePayload } from "./types";
import { classApi } from "./class-api";

export function createClassStore(workspaceId: () => string) {
	const [classes, setClasses] = createSignal<Class[]>([]);
	const [loading, setLoading] = createSignal(false);
	const [error, setError] = createSignal<string | null>(null);

	async function loadClasses(): Promise<void> {
		setLoading(true);
		setError(null);
		try {
			const data = await classApi.list(workspaceId());
			setClasses(data);
		} catch (e) {
			setError(e instanceof Error ? e.message : "Failed to load classes");
		} finally {
			setLoading(false);
		}
	}

	async function createClass(payload: ClassCreatePayload): Promise<Class> {
		setError(null);
		const created = await classApi.create(workspaceId(), payload);
		await loadClasses();
		return created;
	}

	async function getClass(name: string): Promise<Class> {
		setError(null);
		return classApi.get(workspaceId(), name);
	}

	async function listTypes(): Promise<string[]> {
		setError(null);
		return classApi.listTypes(workspaceId());
	}

	return {
		classes,
		loading,
		error,
		loadClasses,
		createClass,
		getClass,
		listTypes,
	};
}

export type ClassStore = ReturnType<typeof createClassStore>;
