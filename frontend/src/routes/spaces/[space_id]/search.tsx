import { A, useNavigate, useParams } from "@solidjs/router";
import { createMemo, createResource, createSignal, For, Show } from "solid-js";
import { SpaceShell } from "~/components/SpaceShell";
import { sqlSessionApi } from "~/lib/sql-session-api";
import { sqlApi } from "~/lib/sql-api";
import type { SqlEntry } from "~/lib/types";

export default function SpaceSearchRoute() {
	const params = useParams<{ space_id: string }>();
	const navigate = useNavigate();
	const spaceId = () => params.space_id;
	const [query, setQuery] = createSignal("");
	const [showFilters, setShowFilters] = createSignal(false);
	const [error, setError] = createSignal<string | null>(null);
	const [runningId, setRunningId] = createSignal<string | null>(null);

	const [queries, { refetch }] = createResource(async () => sqlApi.list(spaceId()));

	const filteredQueries = createMemo(() => {
		const q = query().trim().toLowerCase();
		if (!q) return queries() || [];
		return (queries() || []).filter((entry) => entry.name.toLowerCase().includes(q));
	});

	const handleSelect = async (entry: SqlEntry) => {
		if (entry.variables && entry.variables.length > 0) {
			navigate(`/spaces/${spaceId()}/queries/${encodeURIComponent(entry.id)}/variables`);
			return;
		}
		setError(null);
		setRunningId(entry.id);
		try {
			const session = await sqlSessionApi.create(spaceId(), entry.sql);
			if (session.status === "failed") {
				setError(session.error || "Query failed.");
				return;
			}
			navigate(`/spaces/${spaceId()}/entries?session=${encodeURIComponent(session.id)}`);
		} catch (err) {
			setError(err instanceof Error ? err.message : "Failed to run query");
		} finally {
			setRunningId(null);
		}
	};

	return (
		<SpaceShell spaceId={spaceId()} activeTopTab="search">
			<div class="mx-auto max-w-4xl">
				<div class="flex flex-wrap items-center justify-between gap-3">
					<h1 class="ui-page-title">Queries</h1>
					<A
						href={`/spaces/${spaceId()}/queries/new`}
						class="ui-button ui-button-primary inline-flex items-center gap-2 text-sm"
					>
						<span class="text-lg">+</span>
						Create query
					</A>
				</div>

				<div class="mt-6 flex flex-col gap-3 sm:flex-row sm:items-center">
					<input
						type="text"
						class="ui-input flex-1"
						placeholder="Search queries"
						value={query()}
						onInput={(e) => setQuery(e.currentTarget.value)}
					/>
					<button
						type="button"
						class="ui-button ui-button-secondary text-sm"
						onClick={() => setShowFilters(true)}
					>
						Filter
					</button>
					<button
						type="button"
						class="ui-button ui-button-secondary text-sm"
						onClick={() => refetch()}
					>
						Refresh
					</button>
				</div>

				<div class="mt-6 ui-stack-sm">
					<Show when={queries.loading}>
						<p class="text-sm ui-muted">Loading queries...</p>
					</Show>
					<Show when={error()}>
						<p class="text-sm ui-text-danger">{error()}</p>
					</Show>
					<Show when={queries.error}>
						<p class="text-sm ui-text-danger">Failed to load queries.</p>
					</Show>
					<Show when={!queries.loading && filteredQueries().length === 0}>
						<p class="text-sm ui-muted">No queries yet.</p>
					</Show>
					<For each={filteredQueries()}>
						{(entry) => (
							<button
								type="button"
								class="ui-card ui-card-interactive w-full text-left"
								onClick={() => handleSelect(entry)}
							>
								<div class="flex items-center justify-between gap-2">
									<h2 class="text-base font-semibold">{entry.name}</h2>
									<span class="text-xs ui-muted">
										{runningId() === entry.id
											? "Running"
											: entry.variables?.length
												? "Variables"
												: "Ready"}
									</span>
								</div>
								<p class="mt-2 text-xs ui-muted">Updated {entry.updated_at}</p>
							</button>
						)}
					</For>
				</div>
			</div>

			<Show when={showFilters()}>
				<div class="fixed inset-0 z-50 flex items-center justify-center ui-backdrop p-4">
					<div class="ui-dialog w-full max-w-md">
						<h2 class="text-lg font-semibold">Filters</h2>
						<div class="mt-4 ui-stack-sm">
							<input type="text" class="ui-input" placeholder="Form" />
							<input type="text" class="ui-input" placeholder="Tags" />
							<input type="text" class="ui-input" placeholder="Updated range" />
						</div>
						<div class="mt-6 flex justify-end gap-2">
							<button
								type="button"
								class="ui-button ui-button-secondary text-sm"
								onClick={() => setShowFilters(false)}
							>
								Close
							</button>
						</div>
					</div>
				</div>
			</Show>
		</SpaceShell>
	);
}
