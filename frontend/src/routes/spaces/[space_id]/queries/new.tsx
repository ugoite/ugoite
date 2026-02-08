import { useNavigate, useParams } from "@solidjs/router";
import type { Diagnostic } from "@codemirror/lint";
import { createResource, createSignal, For, Show } from "solid-js";
import { SpaceShell } from "~/components/SpaceShell";
import { SqlQueryEditor } from "~/components";
import { formApi } from "~/lib/form-api";
import { buildSqlSchema } from "~/lib/sql";
import { sqlApi } from "~/lib/sql-api";
import type { Form, SqlVariable } from "~/lib/types";

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

export default function SpaceQueryCreateRoute() {
	const params = useParams<{ space_id: string }>();
	const navigate = useNavigate();
	const spaceId = () => params.space_id;
	const [queryName, setQueryName] = createSignal("");
	const [sqlInput, setSqlInput] = createSignal("SELECT * FROM entries LIMIT 50");
	const [diagnostics, setDiagnostics] = createSignal<Diagnostic[]>([]);
	const [error, setError] = createSignal<string | null>(null);
	const [isSaving, setIsSaving] = createSignal(false);

	const [forms] = createResource(async () => {
		return await formApi.list(spaceId());
	});

	const schema = () => buildSqlSchema((forms() || []) as Form[]);

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
			await sqlApi.create(spaceId(), {
				name,
				sql,
				variables: extractVariables(sql),
			});
			navigate(`/spaces/${spaceId()}/search`);
		} catch (err) {
			setError(err instanceof Error ? err.message : "Failed to save query");
		} finally {
			setIsSaving(false);
		}
	};

	return (
		<SpaceShell spaceId={spaceId()} activeTopTab="search">
			<div class="mx-auto max-w-4xl">
				<h1 class="ui-page-title">Create query</h1>

				<div class="mt-6 ui-stack-sm">
					<label class="ui-label" for="query-title">
						Query name
					</label>
					<input
						id="query-title"
						class="ui-input"
						placeholder="Untitled query"
						value={queryName()}
						onInput={(e) => setQueryName(e.currentTarget.value)}
					/>

					<div>
						<label class="ui-label mb-2" for="query-sql">
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
						<ul class="text-sm ui-text-warning ui-stack-sm">
							<For each={diagnostics()}>{(diag) => <li>{diag.message}</li>}</For>
						</ul>
					</Show>
					<Show when={error()}>
						<p class="text-sm ui-text-danger">{error()}</p>
					</Show>

					<button
						type="button"
						class="ui-button ui-button-primary text-sm"
						onClick={handleSave}
						disabled={isSaving()}
					>
						{isSaving() ? "Saving..." : "Save"}
					</button>
				</div>
			</div>
		</SpaceShell>
	);
}
