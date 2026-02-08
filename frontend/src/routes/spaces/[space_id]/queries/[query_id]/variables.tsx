import { useNavigate, useParams } from "@solidjs/router";
import { createMemo, createResource, createSignal, For, Show } from "solid-js";
import { SpaceShell } from "~/components/SpaceShell";
import { sqlSessionApi } from "~/lib/sql-session-api";
import { sqlApi } from "~/lib/sql-api";
import type { SqlVariable } from "~/lib/types";

const VARIABLE_REGEX = /\{\{\s*([A-Za-z_][A-Za-z0-9_]*)\s*\}\}/g;

function formatSqlValue(value: string, variable: SqlVariable | undefined) {
	const trimmed = value.trim();
	if (!trimmed) {
		throw new Error(`Value required for ${variable?.name ?? "variable"}`);
	}
	const type = variable?.type ?? "string";
	if (["number", "double", "float", "integer", "long"].includes(type)) {
		const num = Number(trimmed);
		if (Number.isNaN(num)) {
			throw new Error(`Invalid number for ${variable?.name ?? "variable"}`);
		}
		return String(num);
	}
	if (type === "boolean") {
		const lower = trimmed.toLowerCase();
		if (lower !== "true" && lower !== "false") {
			throw new Error(`Invalid boolean for ${variable?.name ?? "variable"}`);
		}
		return lower;
	}
	const escaped = trimmed.replace(/'/g, "''");
	return `'${escaped}'`;
}

function substituteSql(sql: string, variables: SqlVariable[], values: Record<string, string>) {
	return sql.replace(VARIABLE_REGEX, (_match, name: string) => {
		const variable = variables.find((v) => v.name === name);
		return formatSqlValue(values[name] ?? "", variable);
	});
}

export default function SpaceQueryVariablesRoute() {
	const params = useParams<{ space_id: string; query_id: string }>();
	const navigate = useNavigate();
	const spaceId = () => params.space_id;
	const queryId = () => params.query_id;
	const [values, setValues] = createSignal<Record<string, string>>({});
	const [error, setError] = createSignal<string | null>(null);

	const [entry] = createResource(async () => sqlApi.get(spaceId(), queryId()));

	const variables = createMemo(() => entry()?.variables || []);

	const handleInputChange = (name: string, value: string) => {
		setValues((prev) => ({ ...prev, [name]: value }));
	};

	const handleRun = async () => {
		setError(null);
		const current = entry();
		if (!current) return;
		try {
			const sql = substituteSql(current.sql, current.variables, values());
			const session = await sqlSessionApi.create(spaceId(), sql);
			if (session.status === "failed") {
				setError(session.error || "Query failed.");
				return;
			}
			navigate(`/spaces/${spaceId()}/entries?session=${encodeURIComponent(session.id)}`);
		} catch (err) {
			setError(err instanceof Error ? err.message : "Failed to run query");
		}
	};

	return (
		<SpaceShell spaceId={spaceId()} activeTopTab="search">
			<div class="mx-auto max-w-4xl">
				<h1 class="ui-page-title">Query variables</h1>

				<Show when={entry.loading}>
					<p class="text-sm ui-muted mt-4">Loading query...</p>
				</Show>
				<Show when={entry.error}>
					<p class="text-sm ui-text-danger mt-4">Failed to load query.</p>
				</Show>
				<Show when={entry()}>
					{(data) => (
						<div class="mt-6 ui-stack-sm">
							<p class="text-sm ui-muted">{data().name}</p>
							<div class="ui-stack-sm">
								<For each={variables()}>
									{(variable, index) => {
										const inputId = `query-var-${variable.name}-${index()}`;
										return (
											<div>
												<label class="ui-label" for={inputId}>
													{variable.name}
													<span class="ml-2 text-xs ui-muted">{variable.type}</span>
												</label>
												<input
													id={inputId}
													class="ui-input"
													placeholder={variable.description}
													value={values()[variable.name] ?? ""}
													onInput={(e) => handleInputChange(variable.name, e.currentTarget.value)}
												/>
											</div>
										);
									}}
								</For>
							</div>
							<Show when={error()}>
								<p class="text-sm ui-text-danger">{error()}</p>
							</Show>
							<button type="button" class="ui-button ui-button-primary text-sm" onClick={handleRun}>
								Run
							</button>
						</div>
					)}
				</Show>
			</div>
		</SpaceShell>
	);
}
