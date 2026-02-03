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
import { CreateNoteDialog } from "~/components/create-dialogs";
import { WorkspaceShell } from "~/components/WorkspaceShell";
import { ensureClassFrontmatter, replaceFirstH1 } from "~/lib/markdown";
import { useNotesRouteContext } from "~/lib/notes-route-context";
import { sqlSessionApi } from "~/lib/sql-session-api";
import type { NoteRecord } from "~/lib/types";

export default function WorkspaceNotesIndexPane() {
	const navigate = useNavigate();
	const [searchParams] = useSearchParams();
	const ctx = useNotesRouteContext();
	const workspaceId = () => ctx.workspaceId();
	const [showCreateNoteDialog, setShowCreateNoteDialog] = createSignal(false);

	const sessionId = createMemo(() => (searchParams.session ? String(searchParams.session) : ""));
	const [page, setPage] = createSignal(1);
	const [pageSize] = createSignal(24);

	const [session, { refetch: refetchSession }] = createResource(
		() => sessionId().trim() || null,
		async (id) => sqlSessionApi.get(workspaceId(), id),
	);

	const [sessionRows] = createResource(
		() => {
			const id = sessionId().trim();
			if (!id || session()?.status !== "completed") return null;
			return { id, offset: (page() - 1) * pageSize(), limit: pageSize() };
		},
		async ({ id, offset, limit }) => sqlSessionApi.rows(workspaceId(), id, offset, limit),
	);

	createEffect(() => {
		if (workspaceId() && !sessionId().trim()) {
			ctx.noteStore.loadNotes();
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

	const displayNotes = createMemo<NoteRecord[]>(() => {
		if (sessionId().trim()) {
			return sessionRows()?.rows || [];
		}
		return ctx.noteStore.notes() || [];
	});

	const totalCount = createMemo(
		() => sessionRows()?.totalCount ?? session()?.row_count ?? displayNotes().length,
	);

	const totalPages = createMemo(() => Math.max(1, Math.ceil(totalCount() / pageSize())));

	const isLoading = createMemo(() => {
		if (sessionId().trim()) {
			return session.loading || sessionRows.loading;
		}
		return ctx.noteStore.loading();
	});

	const error = createMemo(() => {
		if (sessionId().trim()) {
			return session.error || sessionRows.error;
		}
		return ctx.noteStore.error();
	});

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
		<WorkspaceShell
			workspaceId={workspaceId()}
			showBottomTabs
			activeBottomTab="object"
			bottomTabHrefSuffix={sessionId().trim() ? `?session=${encodeURIComponent(sessionId())}` : ""}
		>
			<div class="mx-auto max-w-6xl">
				<div class="flex flex-wrap items-center justify-between gap-3">
					<div>
						<h1 class="text-2xl font-semibold text-slate-900">
							{sessionId().trim() ? "Query Results" : "Notes"}
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
