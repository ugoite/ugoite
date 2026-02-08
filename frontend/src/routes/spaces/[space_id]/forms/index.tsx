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
	let formSelectEl: HTMLSelectElement | undefined;
	const [showCreateFormDialog, setShowCreateFormDialog] = createSignal(false);
	const sessionId = createMemo(() => (searchParams.session ? String(searchParams.session) : ""));
	const [page, setPage] = createSignal(1);
	const [pageSize] = createSignal(25);
	const selectedFormName = createMemo(() => (searchParams.form ? String(searchParams.form) : ""));
	const handleCreateForm = async (payload: FormCreatePayload) => {
		try {
			await formApi.create(ctx.spaceId(), payload);
			setShowCreateFormDialog(false);
			ctx.refetchForms();
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to create form");
		}
	};

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

	const selectedFormValue = createMemo(() => selectedFormName().trim());

	const handleFormSelection = (value: string) => {
		if (!value) return;
		setSearchParams({ form: value });
	};

	createEffect(() => {
		const select = formSelectEl;
		if (!select) return;
		const interval = setInterval(() => {
			const selected = select.value.trim();
			if (!selected) return;
			if (selected !== selectedFormName().trim()) {
				setSearchParams({ form: selected });
			}
		}, 200);
		onCleanup(() => clearInterval(interval));
	});

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
						<h1 class="ui-page-title">{sessionId().trim() ? "Query Results" : "Form Grid"}</h1>
						<p class="text-sm ui-muted">
							{sessionId().trim()
								? "Viewing query results in a grid."
								: "Browse form records in a grid."}
						</p>
					</div>
					<div class="flex items-center gap-2">
						<Show when={!sessionId().trim()}>
							<select
								class="ui-input"
								ref={formSelectEl}
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
								class="ui-button ui-button-primary text-sm"
								onClick={() => setShowCreateFormDialog(true)}
							>
								New form
							</button>
						</Show>
						<Show when={sessionId().trim()}>
							<button
								type="button"
								class="ui-button ui-button-secondary text-sm"
								onClick={() => navigate(`/spaces/${ctx.spaceId()}/forms`)}
							>
								Clear query
							</button>
						</Show>
					</div>
				</div>

				<div class="mt-6">
					<Show when={sessionId().trim()}>
						<div class="ui-stack-sm">
							<Show when={session()?.status === "running"}>
								<p class="text-sm ui-muted">Running query...</p>
							</Show>
							<Show when={session()?.status === "failed"}>
								<p class="text-sm ui-text-danger">{session()?.error || "Query failed."}</p>
							</Show>
							<Show when={sessionRows.loading}>
								<p class="text-sm ui-muted">Loading results...</p>
							</Show>
							<Show when={!sessionRows.loading && sessionEntries().length === 0}>
								<p class="text-sm ui-muted">No results found.</p>
							</Show>
							<Show when={sessionEntries().length > 0}>
								<div class="ui-card overflow-x-auto">
									<table class="ui-table text-sm min-w-full">
										<thead>
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
													<tr>
														<td class="px-4 py-2">
															<button
																type="button"
																class="text-left hover:underline"
																onClick={() =>
																	navigate(
																		`/spaces/${ctx.spaceId()}/entries/${encodeURIComponent(entry.id)}`,
																	)
																}
															>
																{entry.title || "Untitled"}
															</button>
														</td>
														<td class="px-4 py-2 ui-muted">{entry.form || "-"}</td>
														<td class="px-4 py-2 ui-muted">
															{new Date(entry.updated_at).toLocaleDateString()}
														</td>
														<For each={sessionFields()}>
															{(field) => (
																<td class="px-4 py-2 ui-muted">
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
								<div class="flex flex-wrap items-center justify-between gap-3 text-sm ui-muted">
									<div>
										Page {page()} of {totalPages()} · {totalCount()} results
									</div>
									<div class="flex items-center gap-2">
										<button
											type="button"
											class="ui-button ui-button-secondary text-sm"
											disabled={page() <= 1}
											onClick={() => setPage((prev) => Math.max(1, prev - 1))}
										>
											Previous
										</button>
										<button
											type="button"
											class="ui-button ui-button-secondary text-sm"
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
						<Show
							when={selectedForm()}
							fallback={<p class="text-sm ui-muted">Create a form to get started.</p>}
						>
							{(form) => (
								<>
									<div class="mb-4">
										<h2 class="text-xl font-semibold">{form().name}</h2>
										<p class="text-sm ui-muted">Query results for the selected form.</p>
									</div>
									<FormTable
										spaceId={ctx.spaceId()}
										entryForm={form()}
										onEntryClick={(entryId) =>
											navigate(`/spaces/${ctx.spaceId()}/entries/${encodeURIComponent(entryId)}`)
										}
									/>
								</>
							)}
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
