import { createSignal } from "solid-js";
import type { WorkspaceLink } from "./types";
import { linksApi } from "./client";

export function createLinkStore(workspaceId: () => string) {
	const [links, setLinks] = createSignal<WorkspaceLink[]>([]);
	const [loading, setLoading] = createSignal(false);
	const [error, setError] = createSignal<string | null>(null);

	async function loadLinks(): Promise<void> {
		setLoading(true);
		setError(null);
		try {
			const data = await linksApi.list(workspaceId());
			setLinks(data);
		} catch (e) {
			setError(e instanceof Error ? e.message : "Failed to load links");
		} finally {
			setLoading(false);
		}
	}

	async function createLink(payload: { source: string; target: string; kind: string }) {
		setError(null);
		const created = await linksApi.create(workspaceId(), payload);
		await loadLinks();
		return created;
	}

	async function deleteLink(linkId: string): Promise<void> {
		setError(null);
		await linksApi.delete(workspaceId(), linkId);
		await loadLinks();
	}

	return {
		links,
		loading,
		error,
		loadLinks,
		createLink,
		deleteLink,
	};
}

export type LinkStore = ReturnType<typeof createLinkStore>;
