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
		<main class="ui-shell">
			<div class="ui-page max-w-3xl mx-auto">
				<div class="flex items-center justify-between mb-6">
					<h1 class="ui-page-title">Link</h1>
					<A href={`/spaces/${spaceId()}/links`} class="text-sm ui-link">
						Back to Links
					</A>
				</div>

				<Show when={links.loading}>
					<p class="text-sm ui-muted">Loading link...</p>
				</Show>
				<Show when={links.error}>
					<p class="text-sm ui-text-danger">Failed to load link.</p>
				</Show>
				<Show when={link()}>
					{(item) => (
						<div class="ui-card">
							<p class="text-sm ui-muted">ID: {item().id}</p>
							<p class="text-sm ui-muted">Kind: {item().kind}</p>
							<p class="text-sm ui-muted">
								{item().source} → {item().target}
							</p>
							<button
								type="button"
								class="mt-4 ui-button ui-button-danger"
								onClick={handleDelete}
								disabled={isDeleting()}
							>
								{isDeleting() ? "Deleting..." : "Delete Link"}
							</button>
							<Show when={deleteError()}>
								<p class="text-sm ui-text-danger mt-2">{deleteError()}</p>
							</Show>
						</div>
					)}
				</Show>
			</div>
		</main>
	);
}
