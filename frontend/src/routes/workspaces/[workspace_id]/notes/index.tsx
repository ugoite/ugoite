import { useNavigate, useSearchParams } from "@solidjs/router";
import { createEffect, createMemo, createResource, createSignal, For, Show } from "solid-js";
import { CreateNoteDialog } from "~/components/create-dialogs";
import { WorkspaceShell } from "~/components/WorkspaceShell";
import { ensureClassFrontmatter, replaceFirstH1 } from "~/lib/markdown";
import { useNotesRouteContext } from "~/lib/notes-route-context";
import { searchApi } from "~/lib/search-api";
import type { NoteRecord } from "~/lib/types";

export default function WorkspaceNotesIndexPane() {
	const navigate = useNavigate();
	const [searchParams] = useSearchParams();
	const ctx = useNotesRouteContext();
	const workspaceId = () => ctx.workspaceId();
	const [showCreateNoteDialog, setShowCreateNoteDialog] = createSignal(false);

	const sqlQuery = createMemo(() => (searchParams.sql ? String(searchParams.sql) : ""));

	const [queryResults] = createResource(
		() => {
			const sql = sqlQuery().trim();
			const wsId = workspaceId();
			if (!sql || !wsId) return null;
			return { wsId, sql };
		},
		async ({ wsId, sql }) => searchApi.querySql(wsId, sql),
	);

	createEffect(() => {
		if (workspaceId()) {
			ctx.noteStore.loadNotes();
		}
	});

	const displayNotes = createMemo<NoteRecord[]>(() => {
		if (sqlQuery().trim()) {
			return queryResults() || [];
		}
		return ctx.noteStore.notes() || [];
	});

	const isLoading = createMemo(() =>
		sqlQuery().trim() ? queryResults.loading : ctx.noteStore.loading(),
	);

	const error = createMemo(() => (sqlQuery().trim() ? queryResults.error : ctx.noteStore.error()));

	const errorMessage = createMemo(() => {
		const err = error();
		if (!err) return null;
		return err instanceof Error ? err.message : String(err);
	});

	const handleSelectNote = (noteId: string) => {
		navigate(`/workspaces/${workspaceId()}/notes/${encodeURIComponent(noteId)}`);
	};

	const handleCreateNote = async (title: string, className: string) => {
		if (!className) {
			alert("Please select a class to create a note.");
			return;
		}
		const classDef = ctx.classes().find((s) => s.name === className);
		if (!classDef) {
			alert("Selected class was not found. Please refresh and try again.");
			return;
		}
		const initialContent = ensureClassFrontmatter(
			replaceFirstH1(classDef.template, title),
			className,
		);

		try {
			const result = await ctx.noteStore.createNote(initialContent);
			setShowCreateNoteDialog(false);
			navigate(`/workspaces/${workspaceId()}/notes/${encodeURIComponent(result.id)}`);
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to create note");
		}
	};

	return (
		<WorkspaceShell workspaceId={workspaceId()} showBottomTabs activeBottomTab="object">
			<div class="mx-auto max-w-6xl">
				<div class="flex flex-wrap items-center justify-between gap-3">
					<div>
						<h1 class="text-2xl font-semibold text-slate-900">
							{sqlQuery().trim() ? "Query Results" : "Notes"}
						</h1>
						<Show when={sqlQuery().trim()}>
							<p class="text-sm text-slate-500">Showing results for saved query.</p>
						</Show>
					</div>
					<div class="flex items-center gap-2">
						<Show when={sqlQuery().trim()}>
							<button
								type="button"
								class="px-3 py-1.5 text-sm rounded border border-slate-300 bg-white hover:bg-slate-50"
								onClick={() => navigate(`/workspaces/${workspaceId()}/notes`)}
							>
								Clear query
							</button>
						</Show>
						<button
							type="button"
							class="px-3 py-1.5 text-sm rounded bg-slate-900 text-white hover:bg-slate-800"
							onClick={() => setShowCreateNoteDialog(true)}
						>
							New note
						</button>
					</div>
				</div>

				<div class="mt-6">
					<Show when={isLoading()}>
						<p class="text-sm text-slate-500">Loading notes...</p>
					</Show>
					<Show when={errorMessage()}>
						<p class="text-sm text-red-600">{errorMessage()}</p>
					</Show>
					<Show when={!isLoading() && displayNotes().length === 0 && !errorMessage()}>
						<p class="text-sm text-slate-500">No notes found.</p>
					</Show>
					<div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
						<For each={displayNotes()}>
							{(note) => (
								<button
									type="button"
									class="text-left rounded-2xl border border-slate-200 bg-white p-4 shadow-sm hover:shadow-md transition"
									onClick={() => handleSelectNote(note.id)}
								>
									<div class="flex items-start justify-between gap-2">
										<h2 class="text-base font-semibold text-slate-900">
											{note.title || "Untitled"}
										</h2>
										<Show when={note.class}>
											<span class="text-xs rounded-full bg-slate-100 px-2 py-0.5 text-slate-600">
												{note.class}
											</span>
										</Show>
									</div>
									<p class="mt-2 text-xs text-slate-500">
										Updated {new Date(note.updated_at).toLocaleDateString()}
									</p>
								</button>
							)}
						</For>
					</div>
				</div>
			</div>

			<CreateNoteDialog
				open={showCreateNoteDialog()}
				classes={ctx.classes()}
				defaultClass={ctx.classes()[0]?.name}
				onClose={() => setShowCreateNoteDialog(false)}
				onSubmit={handleCreateNote}
			/>
		</WorkspaceShell>
	);
}
