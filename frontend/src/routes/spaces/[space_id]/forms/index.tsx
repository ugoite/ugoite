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
import { FormTable } from "~/components/FormTable";
import { CreateFormDialog } from "~/components/create-dialogs";
import { SpaceShell } from "~/components/SpaceShell";
import { useEntriesRouteContext } from "~/lib/entries-route-context";
import { formApi } from "~/lib/form-api";
import { sqlSessionApi } from "~/lib/sql-session-api";
import type { FormCreatePayload } from "~/lib/types";

export default function SpaceFormsIndexPane() {
	const ctx = useEntriesRouteContext();
	const navigate = useNavigate();
	const [searchParams, setSearchParams] = useSearchParams();
	const [showCreateFormDialog, setShowCreateFormDialog] = createSignal(false);
	const sessionId = createMemo(() => (searchParams.session ? String(searchParams.session) : ""));
	const [page, setPage] = createSignal(1);
	const [pageSize] = createSignal(25);
	const [selectedFormLabel, setSelectedFormLabel] = createSignal("");
	const handleCreateForm = async (payload: FormCreatePayload) => {
		try {
			await formApi.create(ctx.spaceId(), payload);
			setShowCreateFormDialog(false);
			ctx.refetchForms();
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to create form");
		}
	};

	const selectedFormName = createMemo(() => (searchParams.form ? String(searchParams.form) : ""));

	const [session, { refetch: refetchSession }] = createResource(
		() => sessionId().trim() || null,
		async (id) => sqlSessionApi.get(ctx.spaceId(), id),
	);

	const [sessionRows] = createResource(
		() => {
			const id = sessionId().trim();
			if (!id || session()?.status !== "completed") return null;
			return { id, offset: (page() - 1) * pageSize(), limit: pageSize() };
		},
		async ({ id, offset, limit }) => sqlSessionApi.rows(ctx.spaceId(), id, offset, limit),
	);

	createEffect(() => {
		const name = selectedFormName().trim();
		if (!name) return;
		const label = selectedFormLabel().trim();
		if (!label || label === name) {
			if (selectedFormLabel() !== name) {
				setSelectedFormLabel(name);
			}
		}
	});

	const selectedFormValue = createMemo(() => {
		const label = selectedFormLabel().trim();
		if (label) return label;
		return selectedFormName().trim();
	});

	const handleFormSelection = (value: string) => {
		if (!value) return;
		if (value === selectedFormValue()) return;
		setSelectedFormLabel(value);
		setSearchParams({ form: value });
	};

	createEffect(() => {
		if (sessionId().trim()) {
			setPage(1);
			return;
		}
		if (selectedFormValue().trim()) return;
		const first = ctx.forms()[0];
		if (first?.name) {
			setSearchParams({ form: first.name }, { replace: true });
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

	const selectedForm = createMemo(() =>
		ctx.forms().find((entry) => entry.name === selectedFormValue()),
	);

	const selectedHeading = createMemo(() => {
		if (selectedFormValue().trim()) return selectedFormValue();
		return selectedForm()?.name || "";
	});

	const sessionEntries = createMemo(() => sessionRows()?.rows || []);
	const sessionFields = createMemo(() => {
		const fields = new Set<string>();
		for (const entry of sessionEntries()) {
			const props = entry.properties || {};
			for (const key of Object.keys(props)) {
				fields.add(key);
			}
		}
		return Array.from(fields);
	});

	const totalCount = createMemo(
		() => sessionRows()?.totalCount ?? session()?.row_count ?? sessionEntries().length,
	);
	const totalPages = createMemo(() => Math.max(1, Math.ceil(totalCount() / pageSize())));

	return (
		<SpaceShell
			spaceId={ctx.spaceId()}
			showBottomTabs
			activeBottomTab="grid"
			bottomTabHrefSuffix={sessionId().trim() ? `?session=${encodeURIComponent(sessionId())}` : ""}
		>
			<div class="mx-auto max-w-6xl">
				<div class="flex flex-wrap items-center justify-between gap-3">
					<div>
						<h1 class="text-2xl font-semibold text-slate-900">
							{sessionId().trim() ? "Query Results" : "Form Grid"}
						</h1>
						<p class="text-sm text-slate-500">
							{sessionId().trim()
								? "Viewing query results in a grid."
								: "Browse form records in a grid."}
						</p>
					</div>
					<div class="flex items-center gap-2">
						<Show when={!sessionId().trim()}>
							<select
								class="rounded-lg border border-slate-300 bg-white px-3 py-1.5 text-sm"
								value={selectedFormValue()}
								onInput={(e) => handleFormSelection(e.currentTarget.value)}
								onChange={(e) => handleFormSelection(e.currentTarget.value)}
							>
								<option value="" disabled>
									Select form
								</option>
								{ctx.forms().map((entry) => (
									<option value={entry.name}>{entry.name}</option>
								))}
							</select>
							<button
								type="button"
								class="rounded-lg bg-slate-900 px-3 py-1.5 text-sm text-white hover:bg-slate-800"
								onClick={() => setShowCreateFormDialog(true)}
							>
								New form
							</button>
						</Show>
						<Show when={sessionId().trim()}>
							<button
								type="button"
								class="rounded-lg border border-slate-300 bg-white px-3 py-1.5 text-sm"
								onClick={() => navigate(`/spaces/${ctx.spaceId()}/forms`)}
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
							<Show when={!sessionRows.loading && sessionEntries().length === 0}>
								<p class="text-sm text-slate-500">No results found.</p>
							</Show>
							<Show when={sessionEntries().length > 0}>
								<div class="overflow-x-auto rounded-2xl border border-slate-200 bg-white">
									<table class="min-w-full text-sm">
										<thead class="bg-slate-50 text-slate-600">
											<tr>
												<th class="px-4 py-2 text-left font-medium">Title</th>
												<th class="px-4 py-2 text-left font-medium">Form</th>
												<th class="px-4 py-2 text-left font-medium">Updated</th>
												<For each={sessionFields()}>
													{(field) => <th class="px-4 py-2 text-left font-medium">{field}</th>}
												</For>
											</tr>
										</thead>
										<tbody>
											<For each={sessionEntries()}>
												{(entry) => (
													<tr class="border-t border-slate-100">
														<td class="px-4 py-2">
															<button
																type="button"
																class="text-left text-slate-900 hover:underline"
																onClick={() =>
																	navigate(
																		`/spaces/${ctx.spaceId()}/entries/${encodeURIComponent(entry.id)}`,
																	)
																}
															>
																{entry.title || "Untitled"}
															</button>
														</td>
														<td class="px-4 py-2 text-slate-600">{entry.form || "-"}</td>
														<td class="px-4 py-2 text-slate-500">
															{new Date(entry.updated_at).toLocaleDateString()}
														</td>
														<For each={sessionFields()}>
															{(field) => (
																<td class="px-4 py-2 text-slate-600">
																	{String(entry.properties?.[field] ?? "")}
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
						<Show when={selectedHeading()}>
							<div class="mb-4">
								<h2 class="text-xl font-semibold text-slate-900">{selectedHeading()}</h2>
								<p class="text-sm text-slate-500">Query results for the selected form.</p>
							</div>
						</Show>
						<Show when={selectedForm()}>
							<FormTable
								spaceId={ctx.spaceId()}
								entryForm={selectedForm()}
								onEntryClick={(entryId) =>
									navigate(`/spaces/${ctx.spaceId()}/entries/${encodeURIComponent(entryId)}`)
								}
							/>
						</Show>
						<Show when={!selectedForm()}>
							<p class="text-sm text-slate-500">Create a form to get started.</p>
						</Show>
					</Show>
				</div>
			</div>

			<CreateFormDialog
				open={showCreateFormDialog()}
				columnTypes={ctx.columnTypes()}
				onClose={() => setShowCreateFormDialog(false)}
				onSubmit={handleCreateForm}
			/>
		</SpaceShell>
	);
}
