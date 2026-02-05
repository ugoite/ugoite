import { A, useNavigate, useParams } from "@solidjs/router";
import { createMemo, createResource, createSignal, Show } from "solid-js";
import { linkApi } from "~/lib/link-api";

export default function SpaceLinkDetailRoute() {
	const navigate = useNavigate();
	const params = useParams<{ space_id: string; link_id: string }>();
	const spaceId = () => params.space_id;
	const linkId = () => params.link_id;
	const [deleteError, setDeleteError] = createSignal<string | null>(null);
	const [isDeleting, setIsDeleting] = createSignal(false);

	const [links] = createResource(async () => {
		return await linkApi.list(spaceId());
	});

	const link = createMemo(() => {
		return links()?.find((item) => item.id === linkId()) || null;
	});

	const handleDelete = async () => {
		setDeleteError(null);
		setIsDeleting(true);
		try {
			await linkApi.delete(spaceId(), linkId());
			navigate(`/spaces/${spaceId()}/links`);
		} catch (err) {
			setDeleteError(err instanceof Error ? err.message : "Failed to delete link");
		} finally {
			setIsDeleting(false);
		}
	};

	return (
		<main class="min-h-screen bg-gray-50">
			<div class="max-w-3xl mx-auto p-6">
				<div class="flex items-center justify-between mb-6">
					<h1 class="text-2xl font-bold text-gray-900">Link</h1>
					<A href={`/spaces/${spaceId()}/links`} class="text-sm text-sky-700 hover:underline">
						Back to Links
					</A>
				</div>

				<Show when={links.loading}>
					<p class="text-sm text-gray-500">Loading link...</p>
				</Show>
				<Show when={links.error}>
					<p class="text-sm text-red-600">Failed to load link.</p>
				</Show>
				<Show when={link()}>
					{(item) => (
						<div class="bg-white border rounded-lg p-4">
							<p class="text-sm text-gray-700">ID: {item().id}</p>
							<p class="text-sm text-gray-700">Kind: {item().kind}</p>
							<p class="text-sm text-gray-500">
								{item().source} → {item().target}
							</p>
							<button
								type="button"
								class="mt-4 px-4 py-2 bg-red-600 text-white rounded hover:bg-red-700 disabled:opacity-50"
								onClick={handleDelete}
								disabled={isDeleting()}
							>
								{isDeleting() ? "Deleting..." : "Delete Link"}
							</button>
							<Show when={deleteError()}>
								<p class="text-sm text-red-600 mt-2">{deleteError()}</p>
							</Show>
						</div>
					)}
				</Show>
			</div>
		</main>
	);
}
