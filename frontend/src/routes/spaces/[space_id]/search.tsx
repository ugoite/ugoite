import { A, useNavigate, useParams } from "@solidjs/router";
import { createMemo, createResource, createSignal, For, Show } from "solid-js";
import { SpaceShell } from "~/components/SpaceShell";
import { formatDateLabel } from "~/lib/date-format";
import { formApi } from "~/lib/form-api";
import { searchApi } from "~/lib/search-api";
import { sqlSessionApi } from "~/lib/sql-session-api";
import { sqlApi } from "~/lib/sql-api";
import type { EntryRecord, SearchResult, SqlEntry } from "~/lib/types";

type SearchMode = "keyword" | "advanced";
type FieldMatchOperator = "equals" | "contains";

type FieldCondition = {
	id: string;
	field: string;
	operator: FieldMatchOperator;
	value: string;
};

type AdvancedSearchCriteria = {
	formName: string;
	tags: string[];
	updatedFrom: string;
	updatedTo: string;
	fieldConditions: Array<{
		field: string;
		operator: FieldMatchOperator;
		value: string;
	}>;
};

const ADVANCED_SEARCH_LIMIT = 50;

function escapeSqlLiteral(value: string): string {
	return `'${value.replaceAll("'", "''")}'`;
}

function quoteSqlIdentifier(value: string): string {
	return `"${value.replaceAll('"', '""')}"`;
}

function parseTimestamp(value: string | number | null | undefined): number {
	if (typeof value === "number") return value;
	if (typeof value !== "string") return 0;
	const timestamp = Date.parse(value);
	return Number.isNaN(timestamp) ? 0 : timestamp;
}

function coerceSearchResult(
	result: Partial<SearchResult> & { id: string },
	fallback?: EntryRecord,
): SearchResult {
	return {
		id: result.id,
		title: result.title || fallback?.title || "Untitled",
		form: result.form ?? fallback?.form,
		updated_at: result.updated_at || fallback?.updated_at || "",
		properties: result.properties ?? fallback?.properties ?? {},
		tags: result.tags ?? fallback?.tags ?? [],
		links: result.links ?? fallback?.links ?? [],
		canvas_position: result.canvas_position ?? fallback?.canvas_position,
		checksum: result.checksum ?? fallback?.checksum,
		assets: result.assets ?? fallback?.assets,
	};
}

function buildKeywordMetadataSql(entryIds: string[]): string {
	const ids = entryIds.map(escapeSqlLiteral).join(", ");
	return `SELECT * FROM entries WHERE id IN (${ids}) LIMIT ${entryIds.length}`;
}

function dateInputToUnixSeconds(value: string, boundary: "start" | "end"): number | null {
	if (!value) return null;
	const [yearText, monthText, dayText] = value.split("-");
	const year = Number(yearText);
	const month = Number(monthText);
	const day = Number(dayText);
	if (![year, month, day].every(Number.isInteger)) {
		return null;
	}
	const startOfDay = new Date(Date.UTC(year, month - 1, day));
	if (
		startOfDay.getUTCFullYear() !== year ||
		startOfDay.getUTCMonth() !== month - 1 ||
		startOfDay.getUTCDate() !== day
	) {
		return null;
	}
	const millis =
		boundary === "start" ? startOfDay.getTime() : startOfDay.getTime() + 86_400_000 - 1;
	return Math.floor(millis / 1000);
}

async function enrichKeywordResults(
	spaceId: string,
	results: SearchResult[],
): Promise<SearchResult[]> {
	const idsNeedingEnrichment = [
		...new Set(
			results.filter((result) => !result.title || !result.updated_at).map((result) => result.id),
		),
	];
	if (idsNeedingEnrichment.length === 0) {
		return results;
	}

	const session = await sqlSessionApi.create(
		spaceId,
		buildKeywordMetadataSql(idsNeedingEnrichment),
	);
	if (session.status === "failed") {
		throw new Error(session.error || "Failed to enrich keyword search results.");
	}
	const metadata = await sqlSessionApi.rows(spaceId, session.id, 0, idsNeedingEnrichment.length);
	const metadataById = new Map(metadata.rows.map((entry) => [entry.id, entry] as const));
	return results.map((result) => coerceSearchResult(result, metadataById.get(result.id)));
}

function buildAdvancedSearchSql(criteria: AdvancedSearchCriteria): string {
	const conditions: string[] = [];

	if (criteria.formName) {
		conditions.push(`form = ${escapeSqlLiteral(criteria.formName)}`);
	}

	for (const tag of criteria.tags) {
		conditions.push(`tags = ${escapeSqlLiteral(tag)}`);
	}

	const updatedFrom = dateInputToUnixSeconds(criteria.updatedFrom, "start");
	if (updatedFrom !== null) {
		conditions.push(`updated_at >= ${updatedFrom}`);
	}

	const updatedTo = dateInputToUnixSeconds(criteria.updatedTo, "end");
	if (updatedTo !== null) {
		conditions.push(`updated_at <= ${updatedTo}`);
	}

	for (const condition of criteria.fieldConditions) {
		const fieldPath = `properties.${quoteSqlIdentifier(condition.field)}`;
		if (condition.operator === "contains") {
			conditions.push(`${fieldPath} ILIKE ${escapeSqlLiteral(`%${condition.value}%`)}`);
			continue;
		}
		conditions.push(`${fieldPath} = ${escapeSqlLiteral(condition.value)}`);
	}

	if (conditions.length === 0) {
		return "";
	}

	return `SELECT * FROM entries WHERE ${conditions.join(" AND ")} ORDER BY updated_at DESC LIMIT ${ADVANCED_SEARCH_LIMIT}`;
}

function buildSearchHistoryName(criteria: AdvancedSearchCriteria): string {
	const parts: string[] = [];

	if (criteria.formName) {
		parts.push(`form: ${criteria.formName}`);
	}

	for (const tag of criteria.tags) {
		parts.push(`tag: ${tag}`);
	}

	if (criteria.updatedFrom) {
		parts.push(`updated-from: ${criteria.updatedFrom}`);
	}

	if (criteria.updatedTo) {
		parts.push(`updated-to: ${criteria.updatedTo}`);
	}

	for (const condition of criteria.fieldConditions.slice(0, 2)) {
		const symbol = condition.operator === "contains" ? "~" : "=";
		parts.push(`${condition.field}${symbol}${condition.value}`);
	}

	const extraConditions = criteria.fieldConditions.length - 2;
	if (extraConditions > 0) {
		parts.push(`+${extraConditions} more`);
	}

	if (parts.length === 0) {
		return "Advanced search";
	}

	const label = `Advanced search - ${parts.join(" - ")}`;
	return label.length > 120 ? `${label.slice(0, 117)}...` : label;
}

export default function SpaceSearchRoute() {
	const params = useParams<{ space_id: string }>();
	const navigate = useNavigate();
	const spaceId = () => params.space_id;
	let nextFieldConditionId = 1;

	const createFieldCondition = (): FieldCondition => ({
		id: `condition-${nextFieldConditionId++}`,
		field: "",
		operator: "equals",
		value: "",
	});

	const [mode, setMode] = createSignal<SearchMode>("keyword");
	const [keywordQuery, setKeywordQuery] = createSignal("");
	const [keywordResults, setKeywordResults] = createSignal<SearchResult[]>([]);
	const [keywordSearchPerformed, setKeywordSearchPerformed] = createSignal(false);
	const [keywordLoading, setKeywordLoading] = createSignal(false);
	const [actionError, setActionError] = createSignal<string | null>(null);
	const [runningSearchId, setRunningSearchId] = createSignal<string | null>(null);
	const [advancedFormName, setAdvancedFormName] = createSignal("");
	const [advancedTagsInput, setAdvancedTagsInput] = createSignal("");
	const [advancedUpdatedFrom, setAdvancedUpdatedFrom] = createSignal("");
	const [advancedUpdatedTo, setAdvancedUpdatedTo] = createSignal("");
	const [fieldConditions, setFieldConditions] = createSignal<FieldCondition[]>([
		createFieldCondition(),
	]);

	const [savedSearches, { refetch: refetchSavedSearches }] = createResource(
		() => spaceId(),
		async (id) => sqlApi.list(id),
	);
	const [forms] = createResource(
		() => spaceId(),
		async (id) => formApi.list(id),
	);

	const availableForms = createMemo(() =>
		[...(forms() || [])].sort((left, right) => left.name.localeCompare(right.name)),
	);

	const availableFields = createMemo(() => {
		const formName = advancedFormName().trim();
		if (!formName) return [] as string[];
		const selectedForm = availableForms().find((entryForm) => entryForm.name === formName);
		if (!selectedForm?.fields) return [] as string[];
		return Object.keys(selectedForm.fields).sort((left, right) => left.localeCompare(right));
	});

	const searchHistory = createMemo(() =>
		[...(savedSearches() || [])].sort(
			(left, right) => parseTimestamp(right.updated_at) - parseTimestamp(left.updated_at),
		),
	);

	const advancedCriteria = createMemo<AdvancedSearchCriteria>(() => ({
		formName: advancedFormName().trim(),
		tags: advancedTagsInput()
			.split(",")
			.map((part) => part.trim())
			.filter(Boolean),
		updatedFrom: advancedUpdatedFrom().trim(),
		updatedTo: advancedUpdatedTo().trim(),
		fieldConditions: fieldConditions()
			.map((condition) => ({
				field: condition.field.trim(),
				operator: condition.operator,
				value: condition.value.trim(),
			}))
			.filter((condition) => condition.field && condition.value),
	}));

	const keywordResultCountLabel = createMemo(() => {
		const count = keywordResults().length;
		return count === 1 ? "1 result" : `${count} results`;
	});

	const updateFieldCondition = (id: string, key: "field" | "operator" | "value", value: string) => {
		setFieldConditions((current) =>
			current.map((condition) =>
				condition.id === id
					? {
							...condition,
							[key]: value,
						}
					: condition,
			),
		);
	};

	const handleKeywordSearch = async () => {
		const query = keywordQuery().trim();
		if (!query) {
			setKeywordSearchPerformed(false);
			setKeywordResults([]);
			setActionError("Enter at least one keyword to search your entries.");
			return;
		}

		setMode("keyword");
		setKeywordSearchPerformed(true);
		setActionError(null);
		setKeywordLoading(true);
		try {
			const results = await searchApi.keyword(spaceId(), query);
			setKeywordResults(await enrichKeywordResults(spaceId(), results));
		} catch (err) {
			setKeywordResults([]);
			setActionError(err instanceof Error ? err.message : "Failed to search entries.");
		} finally {
			setKeywordLoading(false);
		}
	};

	const runSavedSearch = async (entry: SqlEntry) => {
		if (entry.variables && entry.variables.length > 0) {
			navigate(`/spaces/${spaceId()}/queries/${encodeURIComponent(entry.id)}/variables`);
			return;
		}

		setActionError(null);
		setRunningSearchId(entry.id);
		try {
			const session = await sqlSessionApi.create(spaceId(), entry.sql);
			if (session.status === "failed") {
				setActionError(session.error || "Search failed.");
				return;
			}
			navigate(`/spaces/${spaceId()}/entries?session=${encodeURIComponent(session.id)}`);
		} catch (err) {
			setActionError(err instanceof Error ? err.message : "Failed to run saved search.");
		} finally {
			setRunningSearchId(null);
		}
	};

	const handleAdvancedSearch = async () => {
		const criteria = advancedCriteria();
		const sql = buildAdvancedSearchSql(criteria);
		if (!sql) {
			setActionError("Add at least one structured filter before running an advanced search.");
			return;
		}

		setMode("advanced");
		setActionError(null);
		setRunningSearchId("advanced-search");
		try {
			const existing = searchHistory().find(
				(entry) =>
					entry.sql.trim() === sql.trim() && (!entry.variables || entry.variables.length === 0),
			);
			if (!existing) {
				await sqlApi.create(spaceId(), {
					name: buildSearchHistoryName(criteria),
					sql,
					variables: [],
				});
				await refetchSavedSearches();
			}

			const session = await sqlSessionApi.create(spaceId(), sql);
			if (session.status === "failed") {
				setActionError(session.error || "Advanced search failed.");
				return;
			}
			navigate(`/spaces/${spaceId()}/entries?session=${encodeURIComponent(session.id)}`);
		} catch (err) {
			setActionError(err instanceof Error ? err.message : "Failed to run advanced search.");
		} finally {
			setRunningSearchId(null);
		}
	};

	return (
		<SpaceShell spaceId={spaceId()} activeTopTab="search">
			<div class="mx-auto max-w-5xl">
				<div class="flex flex-wrap items-center justify-between gap-3">
					<div>
						<h1 class="ui-page-title">Search</h1>
						<p class="mt-2 text-sm ui-muted">
							Start with keywords, then switch to structured filters only when you need them.
						</p>
					</div>
					<A
						href={`/spaces/${spaceId()}/queries/new`}
						class="ui-button ui-button-secondary inline-flex items-center gap-2 text-sm"
					>
						Open SQL editor
					</A>
				</div>

				<div class="mt-6 ui-card p-5">
					<div class="flex flex-wrap items-center gap-2">
						<button
							type="button"
							class={
								mode() === "keyword"
									? "ui-button ui-button-primary text-sm"
									: "ui-button ui-button-secondary text-sm"
							}
							onClick={() => setMode("keyword")}
						>
							Quick search
						</button>
						<button
							type="button"
							class={
								mode() === "advanced"
									? "ui-button ui-button-primary text-sm"
									: "ui-button ui-button-secondary text-sm"
							}
							onClick={() => setMode("advanced")}
						>
							Advanced search
						</button>
					</div>

					<Show when={mode() === "keyword"}>
						<form
							class="mt-5 flex flex-col gap-3 sm:flex-row sm:items-center"
							onSubmit={(event) => {
								event.preventDefault();
								void handleKeywordSearch();
							}}
						>
							<div class="flex-1">
								<label class="ui-label" for="search-keywords">
									Search keywords
								</label>
								<input
									id="search-keywords"
									type="text"
									class="ui-input mt-2 w-full"
									placeholder="Search entries by title, fields, tags, or content"
									value={keywordQuery()}
									onInput={(event) => setKeywordQuery(event.currentTarget.value)}
								/>
							</div>
							<div class="sm:self-end">
								<button
									type="submit"
									class="ui-button ui-button-primary text-sm"
									disabled={keywordLoading()}
								>
									{keywordLoading() ? "Searching..." : "Search entries"}
								</button>
							</div>
						</form>
					</Show>

					<Show when={mode() === "advanced"}>
						<div class="mt-5 ui-stack-sm">
							<div class="grid gap-4 md:grid-cols-2">
								<div>
									<label class="ui-label" for="advanced-form">
										Form
									</label>
									<select
										id="advanced-form"
										class="ui-input mt-2 w-full"
										value={advancedFormName()}
										onChange={(event) => setAdvancedFormName(event.currentTarget.value)}
									>
										<option value="">Any form</option>
										<For each={availableForms()}>
											{(entryForm) => <option value={entryForm.name}>{entryForm.name}</option>}
										</For>
									</select>
								</div>
								<div>
									<label class="ui-label" for="advanced-tags">
										Tags (comma-separated)
									</label>
									<input
										id="advanced-tags"
										type="text"
										class="ui-input mt-2 w-full"
										placeholder="project, weekly-review"
										value={advancedTagsInput()}
										onInput={(event) => setAdvancedTagsInput(event.currentTarget.value)}
									/>
								</div>
								<div>
									<label class="ui-label" for="advanced-updated-from">
										Updated from
									</label>
									<input
										id="advanced-updated-from"
										type="date"
										class="ui-input mt-2 w-full"
										value={advancedUpdatedFrom()}
										onInput={(event) => setAdvancedUpdatedFrom(event.currentTarget.value)}
									/>
								</div>
								<div>
									<label class="ui-label" for="advanced-updated-to">
										Updated to
									</label>
									<input
										id="advanced-updated-to"
										type="date"
										class="ui-input mt-2 w-full"
										value={advancedUpdatedTo()}
										onInput={(event) => setAdvancedUpdatedTo(event.currentTarget.value)}
									/>
								</div>
							</div>

							<div class="mt-4 ui-stack-sm">
								<div class="flex items-center justify-between gap-2">
									<h2 class="text-base font-semibold">Field conditions</h2>
									<button
										type="button"
										class="ui-button ui-button-secondary text-sm"
										onClick={() =>
											setFieldConditions((current) => [...current, createFieldCondition()])
										}
									>
										Add field condition
									</button>
								</div>

								<For each={fieldConditions()}>
									{(condition) => (
										<div class="ui-card grid gap-3 p-3 md:grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)_minmax(0,1.6fr)_auto]">
											<div>
												<label class="ui-label" for={`field-${condition.id}`}>
													Field
												</label>
												<Show
													when={availableFields().length > 0}
													fallback={
														<input
															id={`field-${condition.id}`}
															type="text"
															class="ui-input mt-2 w-full"
															placeholder="Owner"
															value={condition.field}
															onInput={(event) =>
																updateFieldCondition(
																	condition.id,
																	"field",
																	event.currentTarget.value,
																)
															}
														/>
													}
												>
													<select
														id={`field-${condition.id}`}
														class="ui-input mt-2 w-full"
														value={condition.field}
														onChange={(event) =>
															updateFieldCondition(condition.id, "field", event.currentTarget.value)
														}
													>
														<option value="">Choose a field</option>
														<For each={availableFields()}>
															{(fieldName) => <option value={fieldName}>{fieldName}</option>}
														</For>
													</select>
												</Show>
											</div>
											<div>
												<label class="ui-label" for={`operator-${condition.id}`}>
													Match
												</label>
												<select
													id={`operator-${condition.id}`}
													class="ui-input mt-2 w-full"
													value={condition.operator}
													onChange={(event) =>
														updateFieldCondition(
															condition.id,
															"operator",
															event.currentTarget.value,
														)
													}
												>
													<option value="equals">equals</option>
													<option value="contains">contains</option>
												</select>
											</div>
											<div>
												<label class="ui-label" for={`value-${condition.id}`}>
													Value
												</label>
												<input
													id={`value-${condition.id}`}
													type="text"
													class="ui-input mt-2 w-full"
													placeholder="alice"
													value={condition.value}
													onInput={(event) =>
														updateFieldCondition(condition.id, "value", event.currentTarget.value)
													}
												/>
											</div>
											<div class="md:self-end">
												<button
													type="button"
													class="ui-button ui-button-secondary text-sm"
													onClick={() =>
														setFieldConditions((current) => {
															if (current.length === 1) {
																return [createFieldCondition()];
															}
															return current.filter((item) => item.id !== condition.id);
														})
													}
												>
													Remove
												</button>
											</div>
										</div>
									)}
								</For>
							</div>

							<div class="mt-6 flex flex-wrap items-center justify-between gap-3">
								<p class="text-sm ui-muted">
									Advanced searches compile to reusable SQL-backed history and open the shared query
									results view.
								</p>
								<button
									type="button"
									class="ui-button ui-button-primary text-sm"
									disabled={runningSearchId() === "advanced-search"}
									onClick={() => void handleAdvancedSearch()}
								>
									{runningSearchId() === "advanced-search" ? "Running..." : "Run advanced search"}
								</button>
							</div>
						</div>
					</Show>
				</div>

				<div class="mt-6 grid gap-6 lg:grid-cols-[minmax(0,1.5fr)_minmax(18rem,1fr)]">
					<section class="ui-card p-5">
						<div class="flex flex-wrap items-center justify-between gap-3">
							<div>
								<h2 class="text-lg font-semibold">Keyword results</h2>
								<Show when={keywordSearchPerformed() && !keywordLoading()}>
									<p class="mt-1 text-sm ui-muted">{keywordResultCountLabel()}</p>
								</Show>
							</div>
						</div>

						<div class="mt-4 ui-stack-sm">
							<Show when={actionError()}>
								<p class="text-sm ui-text-danger">{actionError()}</p>
							</Show>
							<Show when={keywordLoading()}>
								<p class="text-sm ui-muted">Searching entries...</p>
							</Show>
							<Show
								when={
									!keywordLoading() &&
									keywordSearchPerformed() &&
									keywordResults().length === 0 &&
									!actionError()
								}
							>
								<p class="text-sm ui-muted">No matching entries found.</p>
							</Show>
							<Show when={!keywordSearchPerformed() && !keywordLoading() && !actionError()}>
								<p class="text-sm ui-muted">
									Search results stay here so you can refine your query without leaving the page.
								</p>
							</Show>
							<div class="grid gap-4 sm:grid-cols-2">
								<For each={keywordResults()}>
									{(entry) => (
										<button
											type="button"
											class="ui-card ui-card-interactive text-left"
											onClick={() =>
												navigate(`/spaces/${spaceId()}/entries/${encodeURIComponent(entry.id)}`)
											}
										>
											<div class="flex items-start justify-between gap-2">
												<h3 class="text-base font-semibold">{entry.title || "Untitled"}</h3>
												<Show when={entry.form}>
													<span class="ui-pill">{entry.form}</span>
												</Show>
											</div>
											<p class="mt-2 text-xs ui-muted">
												Updated {formatDateLabel(entry.updated_at)}
											</p>
										</button>
									)}
								</For>
							</div>
						</div>
					</section>

					<aside class="ui-card p-5">
						<div class="flex flex-wrap items-center justify-between gap-3">
							<div>
								<h2 class="text-lg font-semibold">Search history</h2>
								<p class="mt-1 text-sm ui-muted">
									Saved searches and power-user SQL queries stay reusable here.
								</p>
							</div>
							<button
								type="button"
								class="ui-button ui-button-secondary text-sm"
								onClick={() => void refetchSavedSearches()}
							>
								Refresh history
							</button>
						</div>

						<div class="mt-4 ui-stack-sm">
							<Show when={savedSearches.loading}>
								<p class="text-sm ui-muted">Loading search history...</p>
							</Show>
							<Show when={savedSearches.error}>
								<p class="text-sm ui-text-danger">Failed to load search history.</p>
							</Show>
							<Show when={forms.loading}>
								<p class="text-sm ui-muted">Loading form filters...</p>
							</Show>
							<Show when={forms.error}>
								<p class="text-sm ui-text-danger">Failed to load forms for advanced search.</p>
							</Show>
							<Show when={!savedSearches.loading && searchHistory().length === 0}>
								<p class="text-sm ui-muted">No search history yet.</p>
							</Show>
							<For each={searchHistory()}>
								{(entry) => (
									<button
										type="button"
										class="ui-card ui-card-interactive w-full text-left"
										onClick={() => void runSavedSearch(entry)}
									>
										<div class="flex items-center justify-between gap-2">
											<h3 class="text-sm font-semibold">{entry.name}</h3>
											<span class="text-xs ui-muted">
												{runningSearchId() === entry.id
													? "Running"
													: entry.variables?.length
														? "Variables"
														: "Run again"}
											</span>
										</div>
										<p class="mt-2 text-xs ui-muted">Updated {formatDateLabel(entry.updated_at)}</p>
									</button>
								)}
							</For>
						</div>
					</aside>
				</div>
			</div>
		</SpaceShell>
	);
}
