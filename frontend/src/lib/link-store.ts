import { createSignal } from "solid-js";

const linksRemovedMessage = "Links API has been removed. Use row_reference fields instead.";

export function createLinkStore(_spaceId: () => string) {
	const [links, setLinks] = createSignal<unknown[]>([]);
	const [loading, setLoading] = createSignal(false);
	const [error, setError] = createSignal<string | null>(linksRemovedMessage);

	async function loadLinks(): Promise<void> {
		setLoading(true);
		setError(linksRemovedMessage);
		setLinks([]);
		setLoading(false);
	}

	async function createLink(_payload: { source: string; target: string; kind: string }) {
		setError(linksRemovedMessage);
		throw new Error(linksRemovedMessage);
	}

	async function deleteLink(_linkId: string): Promise<void> {
		setError(linksRemovedMessage);
		throw new Error(linksRemovedMessage);
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
