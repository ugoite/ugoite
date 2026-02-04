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
import { ClassTable } from "~/components/ClassTable";
import { CreateClassDialog } from "~/components/create-dialogs";
import { WorkspaceShell } from "~/components/WorkspaceShell";
import { useNotesRouteContext } from "~/lib/notes-route-context";
import { classApi } from "~/lib/class-api";
import { sqlSessionApi } from "~/lib/sql-session-api";
import type { ClassCreatePayload } from "~/lib/types";

export default function WorkspaceClassesIndexPane() {
	const ctx = useNotesRouteContext();
	const navigate = useNavigate();
	const [searchParams, setSearchParams] = useSearchParams();
	const [showCreateClassDialog, setShowCreateClassDialog] = createSignal(false);
	const sessionId = createMemo(() => (searchParams.session ? String(searchParams.session) : ""));
	const [page, setPage] = createSignal(1);
	const [pageSize] = createSignal(25);
	const handleCreateClass = async (payload: ClassCreatePayload) => {
		try {
			await classApi.create(ctx.workspaceId(), payload);
			setShowCreateClassDialog(false);
			ctx.refetchClasses();
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to create class");
		}
	};

	const selectedClassName = createMemo(() =>
		searchParams.class ? String(searchParams.class) : "",
	);

	const [session, { refetch: refetchSession }] = createResource(
		() => sessionId().trim() || null,
		async (id) => sqlSessionApi.get(ctx.workspaceId(), id),
	);

	const [sessionRows] = createResource(
		() => {
			const id = sessionId().trim();
			if (!id || session()?.status !== "completed") return null;
			return { id, offset: (page() - 1) * pageSize(), limit: pageSize() };
		},
		async ({ id, offset, limit }) => sqlSessionApi.rows(ctx.workspaceId(), id, offset, limit),
	);

	createEffect(() => {
		if (sessionId().trim()) {
			setPage(1);
			return;
		}
		if (selectedClassName()) return;
		const first = ctx.classes()[0];
		if (first?.name) {
			setSearchParams({ class: first.name }, { replace: true });
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

	const selectedClass = createMemo(() =>
		ctx.classes().find((entry) => entry.name === selectedClassName()),
	);

	const sessionNotes = createMemo(() => sessionRows()?.rows || []);
	const sessionFields = createMemo(() => {
		const fields = new Set<string>();
		for (const note of sessionNotes()) {
			const props = note.properties || {};
			for (const key of Object.keys(props)) {
				fields.add(key);
			}
		}
		return Array.from(fields);
	});

	const totalCount = createMemo(
		() => sessionRows()?.totalCount ?? session()?.row_count ?? sessionNotes().length,
	);
	const totalPages = createMemo(() => Math.max(1, Math.ceil(totalCount() / pageSize())));

	return (
		<WorkspaceShell
			workspaceId={ctx.workspaceId()}
			showBottomTabs
			activeBottomTab="grid"
			bottomTabHrefSuffix={sessionId().trim() ? `?session=${encodeURIComponent(sessionId())}` : ""}
		>
			<div class="mx-auto max-w-6xl">
				<div class="flex flex-wrap items-center justify-between gap-3">
					<div>
						<h1 class="text-2xl font-semibold text-slate-900">
							{sessionId().trim() ? "Query Results" : "Class Grid"}
						</h1>
						<p class="text-sm text-slate-500">
							{sessionId().trim()
								? "Viewing query results in a grid."
								: "Browse class records in a grid."}
						</p>
					</div>
					<div class="flex items-center gap-2">
						<Show when={!sessionId().trim()}>
							<select
								class="rounded-lg border border-slate-300 bg-white px-3 py-1.5 text-sm"
								value={selectedClassName()}
								onChange={(e) => setSearchParams({ class: e.currentTarget.value })}
							>
								<option value="" disabled>
									Select class
								</option>
								{ctx.classes().map((entry) => (
									<option value={entry.name}>{entry.name}</option>
								))}
							</select>
							<button
								type="button"
								class="rounded-lg bg-slate-900 px-3 py-1.5 text-sm text-white hover:bg-slate-800"
								onClick={() => setShowCreateClassDialog(true)}
							>
								New class
							</button>
						</Show>
						<Show when={sessionId().trim()}>
							<button
								type="button"
								class="rounded-lg border border-slate-300 bg-white px-3 py-1.5 text-sm"
								onClick={() => navigate(`/workspaces/${ctx.workspaceId()}/classes`)}
							>
								Clear query
							</button>
						</Show>
					</div>
				</div>

				<div class="mt-6">
					<Show when={sessionId().trim()}>
						<div class="space-y-4">
							<Show when={session()?.status === "running"}>
								<p class="text-sm text-slate-500">Running query...</p>
							</Show>
							<Show when={session()?.status === "failed"}>
								<p class="text-sm text-red-600">{session()?.error || "Query failed."}</p>
							</Show>
							<Show when={sessionRows.loading}>
								<p class="text-sm text-slate-500">Loading results...</p>
							</Show>
							<Show when={!sessionRows.loading && sessionNotes().length === 0}>
								<p class="text-sm text-slate-500">No results found.</p>
							</Show>
							<Show when={sessionNotes().length > 0}>
								<div class="overflow-x-auto rounded-2xl border border-slate-200 bg-white">
									<table class="min-w-full text-sm">
										<thead class="bg-slate-50 text-slate-600">
											<tr>
												<th class="px-4 py-2 text-left font-medium">Title</th>
												<th class="px-4 py-2 text-left font-medium">Class</th>
												<th class="px-4 py-2 text-left font-medium">Updated</th>
												<For each={sessionFields()}>
													{(field) => <th class="px-4 py-2 text-left font-medium">{field}</th>}
												</For>
											</tr>
										</thead>
										<tbody>
											<For each={sessionNotes()}>
												{(note) => (
													<tr class="border-t border-slate-100">
														<td class="px-4 py-2">
															<button
																type="button"
																class="text-left text-slate-900 hover:underline"
																onClick={() =>
																	navigate(
																		`/workspaces/${ctx.workspaceId()}/notes/${encodeURIComponent(note.id)}`,
																	)
																}
															>
																{note.title || "Untitled"}
															</button>
														</td>
														<td class="px-4 py-2 text-slate-600">{note.class || "-"}</td>
														<td class="px-4 py-2 text-slate-500">
															{new Date(note.updated_at).toLocaleDateString()}
														</td>
														<For each={sessionFields()}>
															{(field) => (
																<td class="px-4 py-2 text-slate-600">
																	{String(note.properties?.[field] ?? "")}
																</td>
															)}
														</For>
													</tr>
												)}
											</For>
										</tbody>
									</table>
								</div>
							</Show>
							<Show when={totalCount() > 0}>
								<div class="flex flex-wrap items-center justify-between gap-3 text-sm text-slate-600">
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
					</Show>
					<Show when={!sessionId().trim()}>
						<Show when={selectedClass()}>
							<ClassTable
								workspaceId={ctx.workspaceId()}
								noteClass={selectedClass()}
								onNoteClick={(noteId) =>
									navigate(`/workspaces/${ctx.workspaceId()}/notes/${encodeURIComponent(noteId)}`)
								}
							/>
						</Show>
						<Show when={!selectedClass()}>
							<p class="text-sm text-slate-500">Create a class to get started.</p>
						</Show>
					</Show>
				</div>
			</div>

			<CreateClassDialog
				open={showCreateClassDialog()}
				columnTypes={ctx.columnTypes()}
				onClose={() => setShowCreateClassDialog(false)}
				onSubmit={handleCreateClass}
			/>
		</WorkspaceShell>
	);
}
