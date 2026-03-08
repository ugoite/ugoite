import { A, useNavigate } from "@solidjs/router";
import {
	createEffect,
	createMemo,
	createResource,
	createSignal,
	For,
	Show,
	type JSX,
} from "solid-js";
import { spaceApi } from "~/lib/space-api";

const toMessage = (value: unknown): string => {
	if (value instanceof Error && value.message.trim()) {
		return value.message;
	}
	return "";
};

export default function SpacesIndexRoute() {
	const navigate = useNavigate();
	const [spaces, { refetch: refetchSpaces }] = createResource(async () => {
		return await spaceApi.list();
	});
	const [redirected, setRedirected] = createSignal(false);
	const [showCreateForm, setShowCreateForm] = createSignal(false);
	const [newSpaceName, setNewSpaceName] = createSignal("");
	const [createError, setCreateError] = createSignal<string | null>(null);
	const [isCreating, setIsCreating] = createSignal(false);

	const authHint = createMemo((): JSX.Element | null => {
		const message = toMessage(spaces.error).toLowerCase();
		if (
			message.includes("401") ||
			message.includes("authentication") ||
			message.includes("unauthorized")
		) {
			return (
				<span>
					Authentication required. Re-run <code>mise run dev</code> (or{" "}
					<code>UGOITE_DEV_AUTH_FORCE_LOGIN=true mise run dev</code>) to refresh local login.
				</span>
			);
		}
		if (
			message.includes("403") ||
			message.includes("forbidden") ||
			message.includes("not authorized")
		) {
			return (
				<span>
					Your identity is authenticated, but this space is not shared with your user yet. Ask a
					space admin for an invitation.
				</span>
			);
		}
		return null;
	});

	const hasNoSpaces = createMemo(
		() => !spaces.loading && !spaces.error && (spaces()?.length ?? 0) === 0,
	);

	const openCreateForm = () => {
		setCreateError(null);
		setShowCreateForm(true);
	};

	const closeCreateForm = () => {
		setShowCreateForm(false);
		setNewSpaceName("");
		setCreateError(null);
	};

	const handleCreateSpace = async (event: Event) => {
		event.preventDefault();
		const spaceName = newSpaceName().trim();
		if (!spaceName) {
			setCreateError("Please provide a space name.");
			return;
		}
		setIsCreating(true);
		setCreateError(null);
		try {
			const created = await spaceApi.create(spaceName);
			await refetchSpaces();
			closeCreateForm();
			navigate(`/spaces/${created.id}/dashboard`);
		} catch (error) {
			setCreateError(toMessage(error) || "Failed to create space.");
		} finally {
			setIsCreating(false);
		}
	};

	createEffect(() => {
		if (!spaces.error || redirected()) {
			return;
		}
		const message = toMessage(spaces.error).toLowerCase();
		if (
			message.includes("401") ||
			message.includes("authentication") ||
			message.includes("unauthorized")
		) {
			setRedirected(true);
			navigate("/");
		}
	});

	return (
		<main class="mx-auto max-w-4xl ui-page ui-stack">
			<div class="flex flex-wrap items-center justify-between gap-3">
				<h1 class="ui-page-title">Spaces</h1>
				<div class="flex flex-wrap items-center gap-2">
					<Show when={!spaces.error && !showCreateForm() && !hasNoSpaces()}>
						<button
							type="button"
							class="ui-button ui-button-primary text-sm"
							onClick={openCreateForm}
						>
							Create space
						</button>
					</Show>
					<A href="/" class="ui-muted text-sm">
						Back to Home
					</A>
				</div>
			</div>

			<section class="ui-card ui-stack-sm">
				<h2 class="text-lg font-semibold">Authentication</h2>
				<p class="text-sm ui-muted">
					Localhost and remote mode both require authentication. Use the local login flow via
					<code>mise run dev</code> (or force refresh with
					<code>UGOITE_DEV_AUTH_FORCE_LOGIN=true mise run dev</code>).
				</p>
			</section>

			<section class="ui-card">
				<h2 class="text-lg font-semibold mb-3">Available Spaces</h2>
				<Show when={showCreateForm()}>
					<form class="ui-card ui-stack-sm mb-4" onSubmit={handleCreateSpace}>
						<div>
							<h3 class="text-base font-semibold">Create space</h3>
							<p class="text-sm ui-muted">
								Spaces are never created automatically. Choose a name to create one explicitly.
							</p>
						</div>
						<div class="ui-field">
							<label class="ui-label" for="space-name">
								Space name
							</label>
							<input
								id="space-name"
								type="text"
								class="ui-input"
								value={newSpaceName()}
								onInput={(event) => setNewSpaceName(event.currentTarget.value)}
								placeholder="Enter space name..."
							/>
						</div>
						<Show when={createError()}>
							<p class="ui-alert ui-alert-error text-sm">{createError()}</p>
						</Show>
						<div class="flex flex-wrap justify-end gap-2">
							<button
								type="button"
								class="ui-button ui-button-secondary text-sm"
								onClick={closeCreateForm}
								disabled={isCreating()}
							>
								Cancel
							</button>
							<button
								type="submit"
								class="ui-button ui-button-primary text-sm"
								disabled={!newSpaceName().trim() || isCreating()}
							>
								{isCreating() ? "Creating..." : "Create space"}
							</button>
						</div>
					</form>
				</Show>
				<Show when={spaces.loading}>
					<p class="text-sm ui-muted">Loading spaces...</p>
				</Show>
				<Show when={spaces.error}>
					<p class="ui-alert ui-alert-error text-sm">Failed to load spaces.</p>
					<Show when={authHint()}>
						<p class="text-sm ui-muted mt-2">{authHint()}</p>
					</Show>
				</Show>
				<Show when={hasNoSpaces() && !showCreateForm()}>
					<div class="ui-card ui-card-dashed ui-stack-sm">
						<p class="text-sm ui-muted">No spaces available.</p>
						<div>
							<button
								type="button"
								class="ui-button ui-button-primary text-sm"
								onClick={openCreateForm}
							>
								Create space
							</button>
						</div>
					</div>
				</Show>
				<ul class="ui-stack-sm">
					<For each={spaces() || []}>
						{(space) => (
							<li class="ui-card flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3">
								<div>
									<h3 class="font-medium">{space.name || space.id}</h3>
									<p class="text-xs ui-muted">ID: {space.id}</p>
								</div>
								<div class="flex flex-wrap gap-2">
									<A
										href={`/spaces/${space.id}/settings`}
										class="ui-button ui-button-secondary text-sm"
									>
										Settings
									</A>
									<A
										href={`/spaces/${space.id}/dashboard`}
										class="ui-button ui-button-primary text-sm"
									>
										Open Space
									</A>
								</div>
							</li>
						)}
					</For>
				</ul>
			</section>
		</main>
	);
}
