import { useNavigate, useSearchParams } from "@solidjs/router";
import { createEffect, createMemo, createSignal, Show } from "solid-js";
import { ClassTable } from "~/components/ClassTable";
import { CreateClassDialog } from "~/components/create-dialogs";
import { WorkspaceShell } from "~/components/WorkspaceShell";
import { useNotesRouteContext } from "~/lib/notes-route-context";
import { classApi } from "~/lib/class-api";
import type { ClassCreatePayload } from "~/lib/types";

export default function WorkspaceClassesIndexPane() {
	const ctx = useNotesRouteContext();
	const navigate = useNavigate();
	const [searchParams, setSearchParams] = useSearchParams();
	const [showCreateClassDialog, setShowCreateClassDialog] = createSignal(false);
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

	createEffect(() => {
		if (selectedClassName()) return;
		const first = ctx.classes()[0];
		if (first?.name) {
			setSearchParams({ class: first.name }, { replace: true });
		}
	});

	const selectedClass = createMemo(() =>
		ctx.classes().find((entry) => entry.name === selectedClassName()),
	);

	return (
		<WorkspaceShell workspaceId={ctx.workspaceId()} showBottomTabs activeBottomTab="grid">
			<div class="mx-auto max-w-6xl">
				<div class="flex flex-wrap items-center justify-between gap-3">
					<div>
						<h1 class="text-2xl font-semibold text-slate-900">Class Grid</h1>
						<p class="text-sm text-slate-500">Browse class records in a grid.</p>
					</div>
					<div class="flex items-center gap-2">
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
					</div>
				</div>

				<div class="mt-6">
					<Show when={selectedClass()}>
						{(cls) => (
							<ClassTable
								workspaceId={ctx.workspaceId()}
								noteClass={cls()}
								onNoteClick={(noteId) =>
									navigate(`/workspaces/${ctx.workspaceId()}/notes/${encodeURIComponent(noteId)}`)
								}
							/>
						)}
					</Show>
					<Show when={!selectedClass()}>
						<p class="text-sm text-slate-500">Create a class to get started.</p>
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
