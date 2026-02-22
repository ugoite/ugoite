import { A } from "@solidjs/router";
import { createMemo, createResource, For, Show } from "solid-js";
import { spaceApi } from "~/lib/space-api";

const toMessage = (value: unknown): string => {
	if (value instanceof Error && value.message.trim()) {
		return value.message;
	}
	return "";
};

export default function SpacesIndexRoute() {
	const [spaces] = createResource(async () => {
		return await spaceApi.list();
	});

	const authHint = createMemo(() => {
		const message = toMessage(spaces.error).toLowerCase();
		if (
			message.includes("401") ||
			message.includes("authentication") ||
			message.includes("unauthorized")
		) {
			return "Sign in by setting UGOITE_AUTH_BEARER_TOKEN or UGOITE_AUTH_API_KEY on the frontend server process.";
		}
		if (
			message.includes("403") ||
			message.includes("forbidden") ||
			message.includes("not authorized")
		) {
			return "Your identity is authenticated, but this space is not shared with your user yet. Ask a space admin for an invitation.";
		}
		return "";
	});

	return (
		<main class="mx-auto max-w-4xl ui-page ui-stack">
			<div class="flex flex-wrap items-center justify-between gap-3">
				<h1 class="ui-page-title">Spaces</h1>
				<A href="/" class="ui-muted text-sm">
					Back to Home
				</A>
			</div>

			<section class="ui-card ui-stack-sm">
				<h2 class="text-lg font-semibold">Authentication</h2>
				<p class="text-sm ui-muted">
					Localhost and remote mode both require authentication. Configure frontend/backend using
					<code>UGOITE_AUTH_BEARER_TOKEN</code> or <code>UGOITE_AUTH_API_KEY</code>.
				</p>
			</section>

			<section class="ui-card">
				<h2 class="text-lg font-semibold mb-3">Available Spaces</h2>
				<Show when={spaces.loading}>
					<p class="text-sm ui-muted">Loading spaces...</p>
				</Show>
				<Show when={spaces.error}>
					<p class="ui-alert ui-alert-error text-sm">Failed to load spaces.</p>
					<Show when={authHint()}>
						<p class="text-sm ui-muted mt-2">{authHint()}</p>
					</Show>
				</Show>
				<Show when={spaces() && spaces()?.length === 0}>
					<p class="text-sm ui-muted">No spaces available.</p>
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
