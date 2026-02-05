import { createSignal } from "solid-js";
import type { SpaceLink } from "./types";
import { linkApi } from "./link-api";

export function createLinkStore(spaceId: () => string) {
	const [links, setLinks] = createSignal<SpaceLink[]>([]);
	const [loading, setLoading] = createSignal(false);
	const [error, setError] = createSignal<string | null>(null);

	async function loadLinks(): Promise<void> {
		setLoading(true);
		setError(null);
		try {
			const data = await linkApi.list(spaceId());
			setLinks(data);
		} catch (e) {
			setError(e instanceof Error ? e.message : "Failed to load links");
		} finally {
			setLoading(false);
		}
	}

	async function createLink(payload: { source: string; target: string; kind: string }) {
		setError(null);
		const created = await linkApi.create(spaceId(), payload);
		await loadLinks();
		return created;
	}

	async function deleteLink(linkId: string): Promise<void> {
		setError(null);
		await linkApi.delete(spaceId(), linkId);
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
