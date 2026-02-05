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
					<h1 class="text-2xl font-semibold text-slate-900">Queries</h1>
					<A
						href={`/spaces/${spaceId()}/queries/new`}
						class="inline-flex items-center gap-2 rounded-full bg-slate-900 px-4 py-2 text-sm text-white hover:bg-slate-800"
					>
						<span class="text-lg">+</span>
						Create query
					</A>
				</div>

				<div class="mt-6 flex flex-col gap-3 sm:flex-row sm:items-center">
					<input
						type="text"
						class="flex-1 rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm"
						placeholder="Search queries"
						value={query()}
						onInput={(e) => setQuery(e.currentTarget.value)}
					/>
					<button
						type="button"
						class="rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm"
						onClick={() => setShowFilters(true)}
					>
						Filter
					</button>
					<button
						type="button"
						class="rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm"
						onClick={() => refetch()}
					>
						Refresh
					</button>
				</div>

				<div class="mt-6 space-y-3">
					<Show when={queries.loading}>
						<p class="text-sm text-slate-500">Loading queries...</p>
					</Show>
					<Show when={error()}>
						<p class="text-sm text-red-600">{error()}</p>
					</Show>
					<Show when={queries.error}>
						<p class="text-sm text-red-600">Failed to load queries.</p>
					</Show>
					<Show when={!queries.loading && filteredQueries().length === 0}>
						<p class="text-sm text-slate-500">No queries yet.</p>
					</Show>
					<For each={filteredQueries()}>
						{(entry) => (
							<button
								type="button"
								class="w-full rounded-2xl border border-slate-200 bg-white p-4 text-left shadow-sm hover:shadow-md"
								onClick={() => handleSelect(entry)}
							>
								<div class="flex items-center justify-between gap-2">
									<h2 class="text-base font-semibold text-slate-900">{entry.name}</h2>
									<span class="text-xs text-slate-500">
										{runningId() === entry.id
											? "Running"
											: entry.variables?.length
												? "Variables"
												: "Ready"}
									</span>
								</div>
								<p class="mt-2 text-xs text-slate-500">Updated {entry.updated_at}</p>
							</button>
						)}
					</For>
				</div>
			</div>

			<Show when={showFilters()}>
				<div class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
					<div class="w-full max-w-md rounded-2xl bg-white p-6 shadow-xl">
						<h2 class="text-lg font-semibold text-slate-900">Filters</h2>
						<div class="mt-4 space-y-3">
							<input
								type="text"
								class="w-full rounded-lg border border-slate-300 px-3 py-2 text-sm"
								placeholder="Form"
							/>
							<input
								type="text"
								class="w-full rounded-lg border border-slate-300 px-3 py-2 text-sm"
								placeholder="Tags"
							/>
							<input
								type="text"
								class="w-full rounded-lg border border-slate-300 px-3 py-2 text-sm"
								placeholder="Updated range"
							/>
						</div>
						<div class="mt-6 flex justify-end gap-2">
							<button
								type="button"
								class="rounded-lg border border-slate-300 px-3 py-1.5 text-sm"
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
