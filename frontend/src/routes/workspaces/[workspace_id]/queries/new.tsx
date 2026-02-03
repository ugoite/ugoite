import { useNavigate, useParams } from "@solidjs/router";
import type { Diagnostic } from "@codemirror/lint";
import { createResource, createSignal, For, Show } from "solid-js";
import { WorkspaceShell } from "~/components/WorkspaceShell";
import { SqlQueryEditor } from "~/components";
import { classApi } from "~/lib/class-api";
import { buildSqlSchema } from "~/lib/sql";
import { sqlApi } from "~/lib/sql-api";
import type { Class, SqlVariable } from "~/lib/types";

const VARIABLE_REGEX = /\{\{\s*([A-Za-z_][A-Za-z0-9_]*)\s*\}\}/g;

function extractVariables(sql: string): SqlVariable[] {
	const names = new Set<string>();
	VARIABLE_REGEX.lastIndex = 0;
	let match = VARIABLE_REGEX.exec(sql);
	while (match !== null) {
		names.add(match[1]);
		match = VARIABLE_REGEX.exec(sql);
	}
	return Array.from(names).map((name) => ({
		type: "string",
		name,
		description: `Variable ${name}`,
	}));
}

export default function WorkspaceQueryCreateRoute() {
	const params = useParams<{ workspace_id: string }>();
	const navigate = useNavigate();
	const workspaceId = () => params.workspace_id;
	const [queryName, setQueryName] = createSignal("");
	const [sqlInput, setSqlInput] = createSignal("SELECT * FROM notes LIMIT 50");
	const [diagnostics, setDiagnostics] = createSignal<Diagnostic[]>([]);
	const [error, setError] = createSignal<string | null>(null);
	const [isSaving, setIsSaving] = createSignal(false);

	const [classes] = createResource(async () => {
		return await classApi.list(workspaceId());
	});

	const schema = () => buildSqlSchema((classes() || []) as Class[]);

	const handleSave = async () => {
		setError(null);
		const name = queryName().trim() || "Untitled query";
		const sql = sqlInput().trim();
		if (!sql) {
			setError("SQL is required.");
			return;
		}

		setIsSaving(true);
		try {
			await sqlApi.create(workspaceId(), {
				name,
				sql,
				variables: extractVariables(sql),
			});
			navigate(`/workspaces/${workspaceId()}/search`);
		} catch (err) {
			setError(err instanceof Error ? err.message : "Failed to save query");
		} finally {
			setIsSaving(false);
		}
	};

	return (
		<WorkspaceShell workspaceId={workspaceId()} activeTopTab="search">
			<div class="mx-auto max-w-4xl">
				<h1 class="text-2xl font-semibold text-slate-900">Create query</h1>

				<div class="mt-6 space-y-4">
					<label class="block text-sm font-medium text-slate-700" for="query-title">
						Query name
					</label>
					<input
						id="query-title"
						class="w-full rounded-lg border border-slate-300 px-3 py-2"
						placeholder="Untitled query"
						value={queryName()}
						onInput={(e) => setQueryName(e.currentTarget.value)}
					/>

					<div>
						<label class="block text-sm font-medium text-slate-700 mb-2" for="query-sql">
							SQL
						</label>
						<SqlQueryEditor
							id="query-sql"
							value={sqlInput()}
							onChange={setSqlInput}
							schema={schema()}
							onDiagnostics={setDiagnostics}
							disabled={isSaving()}
						/>
					</div>

					<Show when={diagnostics().length > 0}>
						<ul class="text-sm text-amber-700 space-y-1">
							<For each={diagnostics()}>{(diag) => <li>{diag.message}</li>}</For>
						</ul>
					</Show>
					<Show when={error()}>
						<p class="text-sm text-red-600">{error()}</p>
					</Show>

					<button
						type="button"
						class="rounded-lg bg-slate-900 px-4 py-2 text-sm text-white hover:bg-slate-800 disabled:opacity-50"
						onClick={handleSave}
						disabled={isSaving()}
					>
						{isSaving() ? "Saving..." : "Save"}
					</button>
				</div>
			</div>
		</WorkspaceShell>
	);
}
