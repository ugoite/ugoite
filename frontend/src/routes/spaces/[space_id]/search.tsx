import { A, useNavigate, useParams } from "@solidjs/router";
import { createMemo, createSignal, For, Index, Show } from "solid-js";
import { SpaceShell } from "~/components/SpaceShell";
import { UiIcon } from "~/components/UiIcon";
import { formatDateLabel } from "~/lib/date-format";
import { formApi } from "~/lib/ugoite-client";
import { searchApi } from "~/lib/ugoite-client";
import { sqlSessionApi } from "~/lib/ugoite-client";
import { sqlApi } from "~/lib/ugoite-client";
import type { EntryRecord, SearchResult, SqlEntry } from "~/lib/types";
import { createResource } from "~/lib/recoverable-resource";
import { t } from "~/lib/i18n";

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
    title: result.title || fallback?.title || t("common.untitled"),
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

function dateInputToUnixSeconds(
  value: string,
  boundary: "start" | "end",
): number | null {
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
  const millis = boundary === "start"
    ? startOfDay.getTime()
    : startOfDay.getTime() + 86_400_000 - 1;
  return Math.floor(millis / 1000);
}

async function enrichKeywordResults(
  spaceId: string,
  results: SearchResult[],
): Promise<SearchResult[]> {
  const idsNeedingEnrichment = [
    ...new Set(
      results.filter((result) => !result.title || !result.updated_at).map((
        result,
      ) => result.id),
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
    throw new Error(
      session.error || t("searchPage.error.enrichFailed"),
    );
  }
  const metadata = await sqlSessionApi.rows(
    spaceId,
    session.id,
    0,
    idsNeedingEnrichment.length,
  );
  const metadataById = new Map(
    metadata.rows.map((entry) => [entry.id, entry] as const),
  );
  return results.map((result) =>
    coerceSearchResult(result, metadataById.get(result.id))
  );
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
      conditions.push(
        `${fieldPath} ILIKE ${escapeSqlLiteral(`%${condition.value}%`)}`,
      );
      continue;
    }
    conditions.push(`${fieldPath} = ${escapeSqlLiteral(condition.value)}`);
  }

  if (conditions.length === 0) {
    return "";
  }

  return `SELECT * FROM entries WHERE ${
    conditions.join(" AND ")
  } ORDER BY updated_at DESC LIMIT ${ADVANCED_SEARCH_LIMIT}`;
}

function buildSearchHistoryName(criteria: AdvancedSearchCriteria): string {
  const parts: string[] = [];

  if (criteria.formName) {
    parts.push(t("searchPage.history.form", { value: criteria.formName }));
  }

  for (const tag of criteria.tags) {
    parts.push(t("searchPage.history.tag", { value: tag }));
  }

  if (criteria.updatedFrom) {
    parts.push(
      t("searchPage.history.updatedFrom", { value: criteria.updatedFrom }),
    );
  }

  if (criteria.updatedTo) {
    parts.push(
      t("searchPage.history.updatedTo", { value: criteria.updatedTo }),
    );
  }

  for (const condition of criteria.fieldConditions.slice(0, 2)) {
    const symbol = condition.operator === "contains" ? "~" : "=";
    parts.push(`${condition.field}${symbol}${condition.value}`);
  }

  const extraConditions = criteria.fieldConditions.length - 2;
  if (extraConditions > 0) {
    parts.push(t("searchPage.history.more", { count: extraConditions }));
  }

  if (parts.length === 0) {
    return t("searchPage.advancedSearch");
  }

  const label = `${t("searchPage.advancedSearch")} - ${parts.join(" - ")}`;
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
  const [keywordSearchPerformed, setKeywordSearchPerformed] = createSignal(
    false,
  );
  const [keywordLoading, setKeywordLoading] = createSignal(false);
  const [actionError, setActionError] = createSignal<string | null>(null);
  const [runningSearchId, setRunningSearchId] = createSignal<string | null>(
    null,
  );
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
    [...(forms() || [])].sort((left, right) =>
      left.name.localeCompare(right.name)
    )
  );

  const availableFields = createMemo(() => {
    const formName = advancedFormName().trim();
    if (!formName) return [] as string[];
    const selectedForm = availableForms().find((entryForm) =>
      entryForm.name === formName
    );
    if (!selectedForm?.fields) return [] as string[];
    return Object.keys(selectedForm.fields).sort((left, right) =>
      left.localeCompare(right)
    );
  });

  const searchHistory = createMemo(() =>
    [...(savedSearches() || [])].sort(
      (left, right) =>
        parseTimestamp(right.updated_at) - parseTimestamp(left.updated_at),
    )
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
    return t(
      count === 1 ? "searchBar.results.one" : "searchBar.results.other",
      {
        count,
      },
    );
  });

  const updateFieldCondition = (
    id: string,
    key: "field" | "operator" | "value",
    value: string,
  ) => {
    setFieldConditions((current) =>
      current.map((condition) =>
        condition.id === id
          ? {
            ...condition,
            [key]: value,
          }
          : condition
      )
    );
  };

  const handleKeywordSearch = async () => {
    const query = keywordQuery().trim();
    if (!query) {
      setKeywordSearchPerformed(false);
      setKeywordResults([]);
      setActionError(t("searchPage.error.emptyKeyword"));
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
      setActionError(
        err instanceof Error ? err.message : t("searchPage.error.searchFailed"),
      );
    } finally {
      setKeywordLoading(false);
    }
  };

  const runSavedSearch = async (entry: SqlEntry) => {
    if (entry.variables && entry.variables.length > 0) {
      navigate(
        `/spaces/${spaceId()}/queries/${
          encodeURIComponent(entry.id)
        }/variables`,
      );
      return;
    }

    setActionError(null);
    setRunningSearchId(entry.id);
    try {
      const session = await sqlSessionApi.create(spaceId(), entry.sql);
      if (session.status === "failed") {
        setActionError(session.error || t("searchPage.error.searchFailed"));
        return;
      }
      navigate(
        `/spaces/${spaceId()}/entries?session=${
          encodeURIComponent(session.id)
        }`,
      );
    } catch (err) {
      setActionError(
        err instanceof Error
          ? err.message
          : t("searchPage.error.savedSearchFailed"),
      );
    } finally {
      setRunningSearchId(null);
    }
  };

  const handleAdvancedSearch = async () => {
    const criteria = advancedCriteria();
    const sql = buildAdvancedSearchSql(criteria);
    if (!sql) {
      setActionError(
        t("searchPage.error.advancedFilterRequired"),
      );
      return;
    }

    setMode("advanced");
    setActionError(null);
    setRunningSearchId("advanced-search");
    try {
      const existing = searchHistory().find(
        (entry) =>
          entry.sql.trim() === sql.trim() &&
          (!entry.variables || entry.variables.length === 0),
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
        setActionError(
          session.error || t("searchPage.error.advancedSearchFailed"),
        );
        return;
      }
      navigate(
        `/spaces/${spaceId()}/entries?session=${
          encodeURIComponent(session.id)
        }`,
      );
    } catch (err) {
      setActionError(
        err instanceof Error
          ? err.message
          : t("searchPage.error.advancedSearchFailed"),
      );
    } finally {
      setRunningSearchId(null);
    }
  };

  return (
    <SpaceShell
      spaceId={spaceId()}
      activeNavigation="search"
      title={t("searchPage.title")}
    >
      <div>
        <div class="screenHead">
          <div class="screenTitle">
            <div class="eyebrow">{spaceId()}</div>
            <h1>{t("searchPage.title")}</h1>
          </div>
          <A
            href={`/spaces/${spaceId()}/queries/new`}
            class="ui-button ui-button-secondary inline-flex items-center gap-2 text-sm"
          >
            {t("searchPage.openSqlEditor")}
          </A>
        </div>

        <div class="searchPage">
          <aside class="facet surface">
            <button
              type="button"
              classList={{ active: mode() === "keyword" }}
              onClick={() => setMode("keyword")}
            >
              <UiIcon name="entry" /> {t("searchPage.nav.entries")}
            </button>
            <button
              type="button"
              onClick={() => setMode("advanced")}
              classList={{ active: mode() === "advanced" }}
            >
              <UiIcon name="forms" /> {t("searchPage.nav.forms")}
            </button>
            <A href={`/spaces/${spaceId()}/assets`}>
              <UiIcon name="asset" /> {t("searchPage.nav.assets")}
            </A>
            <A href={`/spaces/${spaceId()}/sql`}>
              <UiIcon name="sql" /> {t("searchPage.nav.savedSql")}
            </A>
          </aside>
          <main>
            <div class="ui-card p-5">
              <div class="flex flex-wrap items-center gap-2">
                <button
                  type="button"
                  class={mode() === "keyword"
                    ? "ui-button ui-button-primary text-sm"
                    : "ui-button ui-button-secondary text-sm"}
                  onClick={() => setMode("keyword")}
                >
                  {t("searchPage.quickSearch")}
                </button>
                <button
                  type="button"
                  class={mode() === "advanced"
                    ? "ui-button ui-button-primary text-sm"
                    : "ui-button ui-button-secondary text-sm"}
                  onClick={() => setMode("advanced")}
                >
                  {t("searchPage.advancedSearch")}
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
                      {t("searchPage.searchKeywords")}
                    </label>
                    <div class="searchBox">
                      <UiIcon name="search" />
                      <input
                        id="search-keywords"
                        type="text"
                        class=""
                        placeholder={t("searchPage.keywordPlaceholder")}
                        value={keywordQuery()}
                        onInput={(event) =>
                          setKeywordQuery(event.currentTarget.value)}
                      />
                    </div>
                  </div>
                  <div class="sm:self-end">
                    <button
                      type="submit"
                      class="ui-button ui-button-primary text-sm"
                      disabled={keywordLoading()}
                    >
                      {keywordLoading()
                        ? t("searchBar.searching")
                        : t("searchPage.searchEntries")}
                    </button>
                  </div>
                </form>
              </Show>

              <Show when={mode() === "advanced"}>
                <div class="mt-5 ui-stack-sm">
                  <div class="grid gap-4 md:grid-cols-2">
                    <div>
                      <label class="ui-label" for="advanced-form">
                        {t("searchPage.form")}
                      </label>
                      <select
                        id="advanced-form"
                        class="ui-input mt-2 w-full"
                        value={advancedFormName()}
                        onChange={(event) =>
                          setAdvancedFormName(event.currentTarget.value)}
                      >
                        <option value="">{t("searchPage.anyForm")}</option>
                        <For each={availableForms()}>
                          {(entryForm) => (
                            <option value={entryForm.name}>
                              {entryForm.name}
                            </option>
                          )}
                        </For>
                      </select>
                    </div>
                    <div>
                      <label class="ui-label" for="advanced-tags">
                        {t("searchPage.tags")}
                      </label>
                      <input
                        id="advanced-tags"
                        type="text"
                        class="ui-input mt-2 w-full"
                        placeholder={t("searchPage.tagsPlaceholder")}
                        value={advancedTagsInput()}
                        onInput={(event) =>
                          setAdvancedTagsInput(event.currentTarget.value)}
                      />
                    </div>
                    <div>
                      <label class="ui-label" for="advanced-updated-from">
                        {t("searchPage.updatedFrom")}
                      </label>
                      <input
                        id="advanced-updated-from"
                        type="date"
                        class="ui-input mt-2 w-full"
                        value={advancedUpdatedFrom()}
                        onInput={(event) =>
                          setAdvancedUpdatedFrom(event.currentTarget.value)}
                      />
                    </div>
                    <div>
                      <label class="ui-label" for="advanced-updated-to">
                        {t("searchPage.updatedTo")}
                      </label>
                      <input
                        id="advanced-updated-to"
                        type="date"
                        class="ui-input mt-2 w-full"
                        value={advancedUpdatedTo()}
                        onInput={(event) =>
                          setAdvancedUpdatedTo(event.currentTarget.value)}
                      />
                    </div>
                  </div>

                  <div class="mt-4 ui-stack-sm">
                    <div class="flex items-center justify-between gap-2">
                      <h2 class="text-base font-semibold">
                        {t("searchPage.fieldConditions")}
                      </h2>
                      <button
                        type="button"
                        class="ui-button ui-button-secondary text-sm"
                        onClick={() =>
                          setFieldConditions((
                            current,
                          ) => [...current, createFieldCondition()])}
                      >
                        {t("searchPage.addFieldCondition")}
                      </button>
                    </div>

                    <Index each={fieldConditions()}>
                      {(condition) => (
                        <div class="ui-card grid gap-3 p-3 md:grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)_minmax(0,1.6fr)_auto]">
                          <div>
                            <label
                              class="ui-label"
                              for={`field-${condition().id}`}
                            >
                              {t("searchPage.field")}
                            </label>
                            <Show
                              when={availableFields().length > 0}
                              fallback={
                                <input
                                  id={`field-${condition().id}`}
                                  type="text"
                                  class="ui-input mt-2 w-full"
                                  placeholder={t("searchPage.fieldPlaceholder")}
                                  value={condition().field}
                                  onInput={(event) =>
                                    updateFieldCondition(
                                      condition().id,
                                      "field",
                                      event.currentTarget.value,
                                    )}
                                />
                              }
                            >
                              <select
                                id={`field-${condition().id}`}
                                class="ui-input mt-2 w-full"
                                value={condition().field}
                                onChange={(event) =>
                                  updateFieldCondition(
                                    condition().id,
                                    "field",
                                    event.currentTarget.value,
                                  )}
                              >
                                <option value="">
                                  {t("searchPage.chooseField")}
                                </option>
                                <For each={availableFields()}>
                                  {(fieldName) => (
                                    <option value={fieldName}>
                                      {fieldName}
                                    </option>
                                  )}
                                </For>
                              </select>
                            </Show>
                          </div>
                          <div>
                            <label
                              class="ui-label"
                              for={`operator-${condition().id}`}
                            >
                              {t("searchPage.match")}
                            </label>
                            <select
                              id={`operator-${condition().id}`}
                              class="ui-input mt-2 w-full"
                              value={condition().operator}
                              onChange={(event) =>
                                updateFieldCondition(
                                  condition().id,
                                  "operator",
                                  event.currentTarget.value,
                                )}
                            >
                              <option value="equals">
                                {t("searchPage.equals")}
                              </option>
                              <option value="contains">
                                {t("searchPage.contains")}
                              </option>
                            </select>
                          </div>
                          <div>
                            <label
                              class="ui-label"
                              for={`value-${condition().id}`}
                            >
                              {t("searchPage.value")}
                            </label>
                            <input
                              id={`value-${condition().id}`}
                              type="text"
                              class="ui-input mt-2 w-full"
                              placeholder={t("searchPage.valuePlaceholder")}
                              value={condition().value}
                              onInput={(event) =>
                                updateFieldCondition(
                                  condition().id,
                                  "value",
                                  event.currentTarget.value,
                                )}
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
                                  return current.filter((item) =>
                                    item.id !== condition().id
                                  );
                                })}
                            >
                              {t("searchPage.remove")}
                            </button>
                          </div>
                        </div>
                      )}
                    </Index>
                  </div>

                  <div class="mt-6 flex flex-wrap items-center justify-between gap-3">
                    <p class="text-sm ui-muted">
                      {t("searchPage.advancedDescription")}
                    </p>
                    <button
                      type="button"
                      class="ui-button ui-button-primary text-sm"
                      disabled={runningSearchId() === "advanced-search"}
                      onClick={() => void handleAdvancedSearch()}
                    >
                      {runningSearchId() === "advanced-search"
                        ? t("searchPage.running")
                        : t("searchPage.runAdvancedSearch")}
                    </button>
                  </div>
                </div>
              </Show>
            </div>

            <div class="mt-6 grid gap-6 lg:grid-cols-[minmax(0,1.5fr)_minmax(18rem,1fr)]">
              <section class="ui-card p-5">
                <div class="flex flex-wrap items-center justify-between gap-3">
                  <div>
                    <h2 class="text-lg font-semibold">
                      {t("searchPage.keywordResults")}
                    </h2>
                    <Show when={keywordSearchPerformed() && !keywordLoading()}>
                      <p class="mt-1 text-sm ui-muted">
                        {keywordResultCountLabel()}
                      </p>
                    </Show>
                  </div>
                </div>

                <div class="mt-4 ui-stack-sm">
                  <Show when={actionError()}>
                    <p class="text-sm ui-text-danger">{actionError()}</p>
                  </Show>
                  <Show when={keywordLoading()}>
                    <p class="text-sm ui-muted">
                      {t("searchPage.searchingEntries")}
                    </p>
                  </Show>
                  <Show
                    when={!keywordLoading() &&
                      keywordSearchPerformed() &&
                      keywordResults().length === 0 &&
                      !actionError()}
                  >
                    <p class="text-sm ui-muted">
                      {t("searchPage.noMatchingEntries")}
                    </p>
                  </Show>
                  <Show
                    when={!keywordSearchPerformed() && !keywordLoading() &&
                      !actionError()}
                  >
                    <p class="text-sm ui-muted">
                      {t("searchPage.initialHelp")}
                    </p>
                  </Show>
                  <div class="grid gap-4 sm:grid-cols-2">
                    <For each={keywordResults()}>
                      {(entry) => (
                        <button
                          type="button"
                          class="ui-card ui-card-interactive text-left"
                          onClick={() =>
                            navigate(
                              `/spaces/${spaceId()}/entries/${
                                encodeURIComponent(entry.id)
                              }`,
                            )}
                        >
                          <div class="flex items-start justify-between gap-2">
                            <h3 class="text-base font-semibold">
                              {entry.title || t("common.untitled")}
                            </h3>
                            <Show when={entry.form}>
                              <span class="ui-pill">{entry.form}</span>
                            </Show>
                          </div>
                          <p class="mt-2 text-xs ui-muted">
                            {t("common.updatedAt", {
                              date: formatDateLabel(entry.updated_at),
                            })}
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
                    <h2 class="text-lg font-semibold">
                      {t("searchPage.searchHistory")}
                    </h2>
                    <p class="mt-1 text-sm ui-muted">
                      {t("searchPage.searchHistoryDescription")}
                    </p>
                  </div>
                  <button
                    type="button"
                    class="ui-button ui-button-secondary text-sm"
                    onClick={() => void refetchSavedSearches()}
                  >
                    {t("searchPage.refreshHistory")}
                  </button>
                </div>

                <div class="mt-4 ui-stack-sm">
                  <Show when={savedSearches.loading}>
                    <p class="text-sm ui-muted">
                      {t("searchPage.loadingHistory")}
                    </p>
                  </Show>
                  <Show when={savedSearches.error}>
                    <p class="text-sm ui-text-danger">
                      {t("searchPage.failedLoadHistory")}
                    </p>
                  </Show>
                  <Show when={forms.loading}>
                    <p class="text-sm ui-muted">
                      {t("searchPage.loadingFormFilters")}
                    </p>
                  </Show>
                  <Show when={forms.error}>
                    <p class="text-sm ui-text-danger">
                      {t("searchPage.failedLoadForms")}
                    </p>
                  </Show>
                  <Show
                    when={!savedSearches.loading &&
                      searchHistory().length === 0}
                  >
                    <p class="text-sm ui-muted">
                      {t("searchPage.noSearchHistory")}
                    </p>
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
                              ? t("searchPage.runningSaved")
                              : entry.variables?.length
                              ? t("searchPage.variables")
                              : t("searchPage.runAgain")}
                          </span>
                        </div>
                        <p class="mt-2 text-xs ui-muted">
                          {t("common.updatedAt", {
                            date: formatDateLabel(entry.updated_at),
                          })}
                        </p>
                      </button>
                    )}
                  </For>
                </div>
              </aside>
            </div>
          </main>
        </div>
      </div>
    </SpaceShell>
  );
}
