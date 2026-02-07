import { useNavigate, useSearchParams } from "@solidjs/router";
import {
	createEffect,
	createMemo,
	createResource,
	createSignal,
	For,
	Show,
	onCleanup,
} from "solid-js";
import { CreateEntryDialog } from "~/components/create-dialogs";
import { SpaceShell } from "~/components/SpaceShell";
import { ensureFormFrontmatter, replaceFirstH1 } from "~/lib/markdown";
import { useEntriesRouteContext } from "~/lib/entries-route-context";
import { sqlSessionApi } from "~/lib/sql-session-api";
import type { EntryRecord } from "~/lib/types";

export default function SpaceEntriesIndexPane() {
	const navigate = useNavigate();
	const [searchParams] = useSearchParams();
	const ctx = useEntriesRouteContext();
	const spaceId = () => ctx.spaceId();
	const [showCreateEntryDialog, setShowCreateEntryDialog] = createSignal(false);

	const sessionId = createMemo(() => (searchParams.session ? String(searchParams.session) : ""));
	const [page, setPage] = createSignal(1);
	const [pageSize] = createSignal(24);

	const [session, { refetch: refetchSession }] = createResource(
		() => sessionId().trim() || null,
		async (id) => sqlSessionApi.get(spaceId(), id),
	);

	const [sessionRows] = createResource(
		() => {
			const id = sessionId().trim();
			if (!id || session()?.status !== "completed") return null;
			return { id, offset: (page() - 1) * pageSize(), limit: pageSize() };
		},
		async ({ id, offset, limit }) => sqlSessionApi.rows(spaceId(), id, offset, limit),
	);

	createEffect(() => {
		if (spaceId() && !sessionId().trim()) {
			ctx.entryStore.loadEntries();
		}
	});

	createEffect(() => {
		const id = sessionId().trim();
		if (!id) return;
		const interval = setInterval(() => {
			if (session()?.status === "running") {
				refetchSession();
			}
		}, 1000);
		onCleanup(() => clearInterval(interval));
	});

	createEffect(() => {
		if (sessionId().trim()) {
			setPage(1);
		}
	});

	const displayEntries = createMemo<EntryRecord[]>(() => {
		if (sessionId().trim()) {
			return sessionRows()?.rows || [];
		}
		return ctx.entryStore.entries() || [];
	});

	const totalCount = createMemo(
		() => sessionRows()?.totalCount ?? session()?.row_count ?? displayEntries().length,
	);

	const totalPages = createMemo(() => Math.max(1, Math.ceil(totalCount() / pageSize())));

	const isLoading = createMemo(() => {
		if (sessionId().trim()) {
			return session.loading || sessionRows.loading;
		}
		return ctx.entryStore.loading();
	});

	const error = createMemo(() => {
		if (sessionId().trim()) {
			return session.error || sessionRows.error;
		}
		return ctx.entryStore.error();
	});

	const errorMessage = createMemo(() => {
		const err = error();
		if (!err) return null;
		return err instanceof Error ? err.message : String(err);
	});

	const handleSelectEntry = (entryId: string) => {
		navigate(`/spaces/${spaceId()}/entries/${encodeURIComponent(entryId)}`);
	};

	const handleCreateEntry = async (title: string, formName: string) => {
		if (!formName) {
			alert("Please select a form to create a entry.");
			return;
		}
		const formDef = ctx.forms().find((s) => s.name === formName);
		if (!formDef) {
			alert("Selected form was not found. Please refresh and try again.");
			return;
		}
		const initialContent = ensureFormFrontmatter(replaceFirstH1(formDef.template, title), formName);

		try {
			const result = await ctx.entryStore.createEntry(initialContent);
			setShowCreateEntryDialog(false);
			navigate(`/spaces/${spaceId()}/entries/${encodeURIComponent(result.id)}`);
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to create entry");
		}
	};

	return (
		<SpaceShell
			spaceId={spaceId()}
			showBottomTabs
			activeBottomTab="object"
			bottomTabHrefSuffix={sessionId().trim() ? `?session=${encodeURIComponent(sessionId())}` : ""}
		>
			<div class="mx-auto max-w-6xl">
				<div class="flex flex-wrap items-center justify-between gap-3">
					<div>
						<h1 class="text-2xl font-semibold text-slate-900">
							{sessionId().trim() ? "Query Results" : "Entries"}
						</h1>
						<Show when={sessionId().trim()}>
							<p class="text-sm text-slate-500">Showing results for this query session.</p>
						</Show>
					</div>
					<div class="flex items-center gap-2">
						<Show when={sessionId().trim()}>
							<button
								type="button"
								class="px-3 py-1.5 text-sm rounded border border-slate-300 bg-white hover:bg-slate-50"
								onClick={() => navigate(`/spaces/${spaceId()}/entries`)}
							>
								Clear query
							</button>
						</Show>
						<button
							type="button"
							class="px-3 py-1.5 text-sm rounded bg-accent-strong text-white hover:bg-accent"
							onClick={() => setShowCreateEntryDialog(true)}
						>
							New entry
						</button>
					</div>
				</div>

				<div class="mt-6">
					<Show when={sessionId().trim() && session()?.status === "running"}>
						<p class="text-sm text-slate-500">
							Running query...
							<Show when={session()?.progress}>
								{(progress) => (
									<span class="ml-2 text-xs text-slate-400">
										{progress().processed} / {progress().total ?? "?"}
									</span>
								)}
							</Show>
						</p>
					</Show>
					<Show when={session()?.status === "failed"}>
						<p class="text-sm text-red-600">{session()?.error || "Query failed."}</p>
					</Show>
					<Show when={isLoading()}>
						<p class="text-sm text-slate-500">Loading entries...</p>
					</Show>
					<Show when={errorMessage()}>
						<p class="text-sm text-red-600">{errorMessage()}</p>
					</Show>
					<Show when={!isLoading() && displayEntries().length === 0 && !errorMessage()}>
						<p class="text-sm text-slate-500">No entries found.</p>
					</Show>
					<div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
						<For each={displayEntries()}>
							{(entry) => (
								<button
									type="button"
									class="text-left rounded-2xl border border-slate-200 bg-white p-4 shadow-sm hover:shadow-md transition"
									onClick={() => handleSelectEntry(entry.id)}
								>
									<div class="flex items-start justify-between gap-2">
										<h2 class="text-base font-semibold text-slate-900">
											{entry.title || "Untitled"}
										</h2>
										<Show when={entry.form}>
											<span class="text-xs rounded-full bg-slate-100 px-2 py-0.5 text-slate-600">
												{entry.form}
											</span>
										</Show>
									</div>
									<p class="mt-2 text-xs text-slate-500">
										Updated {new Date(entry.updated_at).toLocaleDateString()}
									</p>
								</button>
							)}
						</For>
					</div>
					<Show when={sessionId().trim() && totalCount() > 0}>
						<div class="mt-6 flex flex-wrap items-center justify-between gap-3 text-sm text-slate-600">
							<div>
								Page {page()} of {totalPages()} · {totalCount()} results
							</div>
							<div class="flex items-center gap-2">
								<button
									type="button"
									class="px-3 py-1.5 rounded border border-slate-300 bg-white hover:bg-slate-50 disabled:opacity-50"
									disabled={page() <= 1}
									onClick={() => setPage((prev) => Math.max(1, prev - 1))}
								>
									Previous
								</button>
								<button
									type="button"
									class="px-3 py-1.5 rounded border border-slate-300 bg-white hover:bg-slate-50 disabled:opacity-50"
									disabled={page() >= totalPages()}
									onClick={() => setPage((prev) => Math.min(totalPages(), prev + 1))}
								>
									Next
								</button>
							</div>
						</div>
					</Show>
				</div>
			</div>

			<CreateEntryDialog
				open={showCreateEntryDialog()}
				forms={ctx.forms()}
				defaultForm={ctx.forms()[0]?.name}
				onClose={() => setShowCreateEntryDialog(false)}
				onSubmit={handleCreateEntry}
			/>
		</SpaceShell>
	);
}
