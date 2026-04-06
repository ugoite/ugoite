import { A, useNavigate, useParams } from "@solidjs/router";
import { createMemo, createResource, createSignal, For, Match, Show, Switch } from "solid-js";
import { SqlQueryEditor } from "~/components";
import { SpaceShell } from "~/components/SpaceShell";
import { formatDateLabel } from "~/lib/date-format";
import { buildSqlSchema } from "~/lib/sql";
import { sqlApi } from "~/lib/sql-api";
import { sqlSessionApi } from "~/lib/sql-session-api";

const READ_ONLY_SQL_SCHEMA = buildSqlSchema([]);

export default function SpaceSqlDetailRoute() {
	const params = useParams<{ space_id: string; sql_id: string }>();
	const navigate = useNavigate();
	const spaceId = () => params.space_id;
	const sqlId = () => params.sql_id;
	const [runError, setRunError] = createSignal<string | null>(null);
	const [running, setRunning] = createSignal(false);

	const [entry] = createResource(async () => sqlApi.get(spaceId(), sqlId()));
	const variableCount = createMemo(() => entry()?.variables.length ?? 0);
	const queryVariablesHref = () =>
		`/spaces/${spaceId()}/queries/${encodeURIComponent(sqlId())}/variables`;

	const handleRun = async () => {
		const current = entry();
		if (!current || variableCount() > 0 || running()) {
			return;
		}

		setRunError(null);
		setRunning(true);
		try {
			const session = await sqlSessionApi.create(spaceId(), current.sql);
			if (session.status === "failed") {
				setRunError(session.error || "Query failed.");
				return;
			}
			navigate(`/spaces/${spaceId()}/entries?session=${encodeURIComponent(session.id)}`);
		} catch (err) {
			setRunError(err instanceof Error ? err.message : "Failed to run query");
		} finally {
			setRunning(false);
		}
	};

	return (
		<SpaceShell spaceId={spaceId()} activeTopTab="search">
			<div class="mx-auto max-w-4xl ui-stack">
				<section class="ui-card ui-stack">
					<div class="ui-stack-sm">
						<p class="text-sm ui-muted">Space: {spaceId()}</p>
						<p class="text-sm ui-muted">Saved query ID: {sqlId()}</p>
						<Show when={entry()} fallback={<h1 class="ui-page-title">Saved SQL detail</h1>}>
							{(data) => (
								<>
									<h1 class="ui-page-title">{data().name}</h1>
									<p class="ui-page-subtitle max-w-2xl">
										Review the saved query text, confirm whether it needs variables, then use the
										supported run flow that already ships today.
									</p>
								</>
							)}
						</Show>
					</div>

					<Switch>
						<Match when={entry.loading}>
							<p class="text-sm ui-muted">Loading saved query...</p>
						</Match>
						<Match when={entry.error}>
							<div class="ui-stack-sm">
								<p class="text-sm ui-text-danger">Failed to load this saved query.</p>
								<p class="text-sm ui-muted">
									Return to Search or the saved-SQL overview to keep working in supported query
									flows.
								</p>
							</div>
						</Match>
						<Match when={entry()}>
							{(data) => (
								<>
									<dl class="grid gap-4 text-sm sm:grid-cols-3">
										<div class="ui-stack-sm">
											<dt class="font-semibold">Updated</dt>
											<dd class="ui-muted">{formatDateLabel(data().updated_at)}</dd>
										</div>
										<div class="ui-stack-sm">
											<dt class="font-semibold">Created</dt>
											<dd class="ui-muted">{formatDateLabel(data().created_at)}</dd>
										</div>
										<div class="ui-stack-sm">
											<dt class="font-semibold">Variables</dt>
											<dd class="ui-muted">
												{variableCount() === 0
													? "No variables"
													: `${variableCount()} variable${variableCount() === 1 ? "" : "s"}`}
											</dd>
										</div>
									</dl>

									<div class="ui-stack-sm">
										<h2 class="text-lg font-semibold">SQL</h2>
										<SqlQueryEditor
											value={data().sql}
											onChange={() => undefined}
											schema={READ_ONLY_SQL_SCHEMA}
											disabled
										/>
									</div>

									<div class="ui-stack-sm">
										<h2 class="text-lg font-semibold">Variables</h2>
										<Show
											when={variableCount() > 0}
											fallback={
												<p class="text-sm ui-muted">
													This saved query has no template variables, so you can run it immediately
													from this page.
												</p>
											}
										>
											<ul class="list-disc space-y-2 pl-5 text-sm ui-muted">
												<For each={data().variables}>
													{(variable) => (
														<li>
															<span class="font-medium">{variable.name}</span>
															<span class="ml-2 text-xs">{variable.type}</span>
															<Show when={variable.description}>
																<span class="ml-2">{variable.description}</span>
															</Show>
														</li>
													)}
												</For>
											</ul>
										</Show>
									</div>

									<Show when={runError()}>
										<p class="text-sm ui-text-danger">{runError()}</p>
									</Show>
								</>
							)}
						</Match>
					</Switch>

					<div class="flex flex-wrap gap-3">
						<Show when={entry() && variableCount() === 0}>
							<button
								type="button"
								class="ui-button ui-button-primary"
								onClick={handleRun}
								disabled={running()}
							>
								{running() ? "Running..." : "Run Query"}
							</button>
						</Show>
						<Show when={entry() && variableCount() > 0}>
							<A href={queryVariablesHref()} class="ui-button ui-button-primary">
								Open Variables
							</A>
						</Show>
						<A href={`/spaces/${spaceId()}/sql`} class="ui-button ui-button-secondary">
							Back to Saved SQL
						</A>
						<A href={`/spaces/${spaceId()}/search`} class="ui-button ui-button-secondary">
							Open Search
						</A>
						<A href={`/spaces/${spaceId()}/dashboard`} class="ui-button ui-button-secondary">
							Back to Dashboard
						</A>
					</div>
				</section>
			</div>
		</SpaceShell>
	);
}
