import { A, useNavigate, useParams } from "@solidjs/router";
import { createMemo, createSignal, For, Index, Show } from "solid-js";
import { ButtonSpinner } from "~/components/ButtonSpinner";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { UiIcon } from "~/components/UiIcon";
import { formatDateLabel } from "~/lib/date-format";
import { formApi } from "~/lib/ugoite-client";
import { searchApi } from "~/lib/ugoite-client";
import { localInputToRfc3339Instant } from "~/lib/search-date";
import type { EntryRecord, KeywordSearchResult } from "~/lib/types";
import { createResource } from "~/lib/recoverable-resource";
import { t, type TranslationKey } from "~/lib/i18n";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { spaceRoute } from "~/lib/space-shell-route";
import { pageFromArray } from "~/lib/pagination";

export const route = spaceRoute({ navigation: "search" });

type SearchMode = "keyword" | "advanced";
type FieldMatchOperator = "equals" | "contains" | "lt" | "lte" | "gt" | "gte";

type FieldCondition = {
  id: string;
  field: string;
  operator: FieldMatchOperator;
  value: string;
};

type SearchFieldType =
  | "string"
  | "boolean"
  | "integer"
  | "float"
  | "date"
  | "timestamp"
  | "timestamp_tz"
  | "unsupported";

type AvailableField = {
  name: string;
  type: SearchFieldType;
  supported: boolean;
};

type AdvancedFieldCondition = {
  field: string;
  type: SearchFieldType;
  operator: FieldMatchOperator;
  value: string;
  supported: boolean;
};

type AdvancedSearchCriteria = {
  formName: string;
  updatedFrom: string;
  updatedTo: string;
  fieldConditions: AdvancedFieldCondition[];
};

const SEARCH_PAGE_SIZE = 50;

function normalizeFieldType(type: string): SearchFieldType {
  switch (type) {
    case "string":
    case "markdown":
      return "string";
    case "boolean":
      return "boolean";
    case "integer":
      return "integer";
    case "number":
    case "float":
    case "double":
      return "float";
    case "date":
      return "date";
    case "timestamp":
      return "timestamp";
    case "timestamp_tz":
    case "timestamp_tz_ns":
      return "timestamp_tz";
    default:
      return "unsupported";
  }
}

function operatorsForFieldType(type: SearchFieldType): FieldMatchOperator[] {
  if (type === "string") return ["equals", "contains"];
  if (type === "boolean") return ["equals"];
  if (
    type === "integer" || type === "float" || type === "date" ||
    type === "timestamp" || type === "timestamp_tz"
  ) {
    return ["equals", "lt", "lte", "gt", "gte"];
  }
  return [];
}

function operatorLabel(operator: FieldMatchOperator): string {
  switch (operator) {
    case "equals":
      return t("searchPage.equals");
    case "contains":
      return t("searchPage.contains");
    case "lt":
      return t("searchPage.lessThan");
    case "lte":
      return t("searchPage.lessThanOrEqual");
    case "gt":
      return t("searchPage.greaterThan");
    case "gte":
      return t("searchPage.greaterThanOrEqual");
  }
}

function fieldInputType(type: SearchFieldType):
  | "text"
  | "number"
  | "date"
  | "datetime-local" {
  if (type === "integer" || type === "float") return "number";
  if (type === "date") return "date";
  if (type === "timestamp" || type === "timestamp_tz") {
    return "datetime-local";
  }
  return "text";
}

function fieldInputPlaceholder(type: SearchFieldType): TranslationKey {
  switch (type) {
    case "boolean":
      return "searchPage.booleanPlaceholder";
    case "integer":
      return "searchPage.integerPlaceholder";
    case "float":
      return "searchPage.numberPlaceholder";
    case "date":
      return "searchPage.datePlaceholder";
    case "timestamp":
    case "timestamp_tz":
      return "searchPage.timestampPlaceholder";
    default:
      return "searchPage.valuePlaceholder";
  }
}

function fieldInputStep(type: SearchFieldType): string | undefined {
  if (type === "integer") return "1";
  if (type === "float") return "any";
  return undefined;
}

type StructuredTransportCriteria = {
  form: string;
  updated_from?: string;
  updated_to?: string;
  conditions: Array<{
    field: string;
    operator: FieldMatchOperator;
    value: string;
  }>;
  limit: number;
  offset?: number;
};

/**
 * Build logical transport criteria from UI state. No SQL is produced here:
 * relation/column resolution, literal escaping, and type mapping stay in
 * the trusted Rust layer behind search.query.
 */
function buildStructuredSearchCriteria(
  criteria: AdvancedSearchCriteria,
): StructuredTransportCriteria | null {
  if (!criteria.formName) return null;
  for (const condition of criteria.fieldConditions) {
    if (!condition.field || !condition.value) {
      throw new Error(t("searchPage.error.fieldValueRequired"));
    }
    if (!condition.supported) {
      throw new Error(
        t("searchPage.error.unsupportedField", { value: condition.field }),
      );
    }
  }
  if (criteria.fieldConditions.length === 0) {
    throw new Error(t("searchPage.error.advancedFilterRequired"));
  }
  return {
    form: criteria.formName,
    ...(criteria.updatedFrom
      ? {
        updated_from: localInputToRfc3339Instant(
          criteria.updatedFrom,
          "start",
        ),
      }
      : {}),
    ...(criteria.updatedTo
      ? {
        updated_to: localInputToRfc3339Instant(criteria.updatedTo, "end"),
      }
      : {}),
    conditions: criteria.fieldConditions.map((condition) => ({
      field: condition.field,
      operator: condition.operator,
      value: condition.type === "timestamp_tz"
        ? localInputToRfc3339Instant(condition.value)
        : condition.value,
    })),
    limit: SEARCH_PAGE_SIZE,
  };
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
  const [keywordSearchQuery, setKeywordSearchQuery] = createSignal("");
  const [keywordResults, setKeywordResults] = createSignal<
    KeywordSearchResult[]
  >([]);
  const [keywordSearchPerformed, setKeywordSearchPerformed] = createSignal(
    false,
  );
  const [keywordLoading, setKeywordLoading] = createSignal(false);
  const [keywordHasMore, setKeywordHasMore] = createSignal(false);
  const [actionError, setActionError] = createSignal<string | null>(null);
  const [advancedFormName, setAdvancedFormName] = createSignal("");
  const [advancedUpdatedFrom, setAdvancedUpdatedFrom] = createSignal("");
  const [advancedUpdatedTo, setAdvancedUpdatedTo] = createSignal("");
  const [fieldConditions, setFieldConditions] = createSignal<FieldCondition[]>([
    createFieldCondition(),
  ]);
  const [advancedResults, setAdvancedResults] = createSignal<EntryRecord[]>(
    [],
  );
  const [advancedSearchPerformed, setAdvancedSearchPerformed] = createSignal(
    false,
  );
  const [advancedLoading, setAdvancedLoading] = createSignal(false);
  const [advancedHasMore, setAdvancedHasMore] = createSignal(false);
  const [activeAdvancedCriteria, setActiveAdvancedCriteria] = createSignal<
    StructuredTransportCriteria | null
  >(null);

  const [forms] = createResource(
    () => spaceId(),
    async (id) => formApi.list(id),
  );

  const availableForms = createMemo(() =>
    [...(forms() || [])].sort((left, right) =>
      left.name.localeCompare(right.name)
    )
  );

  const selectedForm = createMemo(() =>
    availableForms().find((entryForm) =>
      entryForm.name === advancedFormName().trim()
    )
  );

  const availableFields = createMemo(() => {
    if (!selectedForm()?.fields) return [] as AvailableField[];
    return Object.entries(selectedForm()?.fields ?? {})
      .map(([name, field]) => {
        const type = normalizeFieldType(field.type);
        return {
          name,
          type,
          supported: operatorsForFieldType(type).length > 0,
        };
      })
      .sort((left, right) => left.name.localeCompare(right.name));
  });

  const advancedCriteria = createMemo<AdvancedSearchCriteria>(() => ({
    formName: advancedFormName().trim(),
    updatedFrom: advancedUpdatedFrom().trim(),
    updatedTo: advancedUpdatedTo().trim(),
    fieldConditions: fieldConditions()
      .map((condition) => {
        const field = availableFields().find((item) =>
          item.name === condition.field.trim()
        );
        return {
          field: condition.field.trim(),
          type: field?.type ?? "unsupported",
          operator: condition.operator,
          value: condition.value.trim(),
          supported: field?.supported ?? false,
        };
      })
      .filter((condition) => condition.field || condition.value),
  }));

  const keywordResultCountLabel = createMemo(() => {
    const count = keywordResults().length;
    if (keywordHasMore()) {
      return t("searchPage.resultsAtLeast", { count });
    }
    return t(
      count === 1 ? "searchBar.results.one" : "searchBar.results.other",
      {
        count,
      },
    );
  });

  const advancedResultCountLabel = createMemo(() => {
    const count = advancedResults().length;
    if (advancedHasMore()) {
      return t("searchPage.resultsAtLeast", { count });
    }
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
          ? (() => {
            const next = { ...condition, [key]: value };
            if (key === "field") {
              const field = availableFields().find((item) =>
                item.name === value
              );
              const operators = operatorsForFieldType(
                field?.type ?? "unsupported",
              );
              if (!operators.includes(next.operator)) next.operator = "equals";
            }
            return next;
          })()
          : condition
      )
    );
  };

  const handleAdvancedFormChange = (value: string) => {
    setAdvancedFormName(value);
    setFieldConditions([createFieldCondition()]);
  };

  const handleKeywordSearch = async () => {
    if (keywordLoading()) return;
    const query = keywordQuery().trim();
    if (!query) {
      setKeywordSearchPerformed(false);
      setKeywordResults([]);
      setKeywordSearchQuery("");
      setKeywordHasMore(false);
      setActionError(t("searchPage.error.emptyKeyword"));
      return;
    }

    setMode("keyword");
    setKeywordSearchPerformed(true);
    setKeywordSearchQuery(query);
    setKeywordHasMore(false);
    setActionError(null);
    setKeywordLoading(true);
    try {
      const results = await searchApi.keyword(
        spaceId(),
        query,
        SEARCH_PAGE_SIZE + 1,
      );
      const page = pageFromArray(results, SEARCH_PAGE_SIZE);
      setKeywordResults(page.items);
      setKeywordHasMore(page.hasMore);
    } catch (err) {
      setKeywordResults([]);
      setActionError(
        formatUserFacingError(err, "searchPage.error.searchFailed"),
      );
    } finally {
      setKeywordLoading(false);
    }
  };

  const loadMoreKeywordResults = async () => {
    const query = keywordSearchQuery();
    if (!query || keywordLoading() || !keywordHasMore()) return;
    setActionError(null);
    setKeywordLoading(true);
    try {
      const results = await searchApi.keyword(
        spaceId(),
        query,
        SEARCH_PAGE_SIZE + 1,
        keywordResults().length,
      );
      const page = pageFromArray(results, SEARCH_PAGE_SIZE);
      setKeywordResults((current) => [...current, ...page.items]);
      setKeywordHasMore(page.hasMore);
    } catch (err) {
      setActionError(
        formatUserFacingError(err, "searchPage.error.searchFailed"),
      );
    } finally {
      setKeywordLoading(false);
    }
  };

  const handleAdvancedSearch = async () => {
    if (advancedLoading()) return;
    const criteria = advancedCriteria();
    let transport: StructuredTransportCriteria | null;
    try {
      transport = buildStructuredSearchCriteria(criteria);
    } catch (error) {
      setAdvancedSearchPerformed(false);
      setAdvancedResults([]);
      setActiveAdvancedCriteria(null);
      setAdvancedHasMore(false);
      setActionError(
        error instanceof Error
          ? error.message
          : t("searchPage.error.advancedSearchFailed"),
      );
      return;
    }
    if (!transport) {
      setAdvancedSearchPerformed(false);
      setAdvancedResults([]);
      setActiveAdvancedCriteria(null);
      setAdvancedHasMore(false);
      setActionError(t("searchPage.error.chooseForm"));
      return;
    }

    setMode("advanced");
    setAdvancedSearchPerformed(true);
    const firstPageCriteria: StructuredTransportCriteria = {
      ...transport,
      limit: SEARCH_PAGE_SIZE + 1,
    };
    setActiveAdvancedCriteria(firstPageCriteria);
    setAdvancedHasMore(false);
    setActionError(null);
    setAdvancedLoading(true);
    try {
      const results = await searchApi.queryStructured(
        spaceId(),
        firstPageCriteria,
      );
      const page = pageFromArray(results, SEARCH_PAGE_SIZE);
      setAdvancedResults(page.items);
      setAdvancedHasMore(page.hasMore);
    } catch (err) {
      setAdvancedResults([]);
      setActionError(
        formatUserFacingError(err, "searchPage.error.advancedSearchFailed"),
      );
    } finally {
      setAdvancedLoading(false);
    }
  };

  const loadMoreAdvancedResults = async () => {
    const criteria = activeAdvancedCriteria();
    if (!criteria || advancedLoading() || !advancedHasMore()) return;
    setActionError(null);
    setAdvancedLoading(true);
    try {
      const results = await searchApi.queryStructured(spaceId(), {
        ...criteria,
        limit: SEARCH_PAGE_SIZE + 1,
        offset: advancedResults().length,
      });
      const page = pageFromArray(results, SEARCH_PAGE_SIZE);
      setAdvancedResults((current) => [...current, ...page.items]);
      setAdvancedHasMore(page.hasMore);
    } catch (err) {
      setActionError(
        formatUserFacingError(err, "searchPage.error.advancedSearchFailed"),
      );
    } finally {
      setAdvancedLoading(false);
    }
  };

  return (
    <>
      <div class="searchWorkspace">
        <h1 class="ui-sr-only" id="search-page-title">
          {t("searchPage.title")}
        </h1>
        <nav
          class="searchModeNav"
          aria-label={t("searchPage.title")}
          aria-labelledby="search-page-title"
        >
          <button
            type="button"
            aria-label={t("searchPage.quickSearch")}
            aria-pressed={mode() === "keyword"}
            classList={{ active: mode() === "keyword" }}
            onClick={() => setMode("keyword")}
          >
            {t("searchPage.mode.quick")}
          </button>
          <button
            type="button"
            aria-label={t("searchPage.advancedSearch")}
            aria-pressed={mode() === "advanced"}
            classList={{ active: mode() === "advanced" }}
            onClick={() => setMode("advanced")}
          >
            {t("searchPage.mode.advanced")}
          </button>
          <A href={`/spaces/${encodeURIComponent(spaceId())}/assets`}>
            {t("searchPage.nav.files")}
          </A>
          <A href={`/spaces/${encodeURIComponent(spaceId())}/sql`}>
            {t("searchPage.nav.saved")}
          </A>
        </nav>

        <div class="searchPage">
          <main>
            <section class="searchControls" aria-labelledby="search-page-title">
              <Show when={mode() === "keyword"}>
                <form
                  class="mt-5 flex flex-col gap-3 sm:flex-row sm:items-center"
                  onSubmit={(event) => {
                    event.preventDefault();
                    void handleKeywordSearch();
                  }}
                >
                  <div class="flex-1">
                    <label class="ui-sr-only" for="search-keywords">
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
                  <div class="sm:self-end queryLane">
                    {/* Query-lane spinner: previous results stay visible. */}
                    <Show when={keywordLoading()}>
                      <LocalBusyIndicator
                        size="sm"
                        label={t("searchPage.searchingEntries")}
                      />
                    </Show>
                    <button
                      type="submit"
                      class="ui-button ui-button-primary text-sm"
                      disabled={keywordLoading()}
                      aria-busy={keywordLoading() || undefined}
                    >
                      <Show when={keywordLoading()}>
                        <ButtonSpinner />
                      </Show>
                      {t("searchPage.searchEntries")}
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
                          handleAdvancedFormChange(event.currentTarget.value)}
                      >
                        <option value="">{t("searchPage.selectForm")}</option>
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
                        <div class="searchCondition grid gap-3 p-3 md:grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)_minmax(0,1.6fr)_auto]">
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
                                  {(field) => (
                                    <option
                                      value={field.name}
                                      disabled={!field.supported}
                                    >
                                      {field.name}
                                      {field.supported
                                        ? ""
                                        : ` (${t("searchPage.unsupported")})`}
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
                              <For
                                each={operatorsForFieldType(
                                  availableFields().find((field) =>
                                    field.name === condition().field
                                  )?.type ?? "unsupported",
                                )}
                              >
                                {(operator) => (
                                  <option value={operator}>
                                    {operatorLabel(operator)}
                                  </option>
                                )}
                              </For>
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
                              type={fieldInputType(
                                availableFields().find((field) =>
                                  field.name === condition().field
                                )?.type ?? "unsupported",
                              )}
                              class="ui-input mt-2 w-full"
                              step={fieldInputStep(
                                availableFields().find((field) =>
                                  field.name === condition().field
                                )?.type ?? "unsupported",
                              )}
                              placeholder={t(fieldInputPlaceholder(
                                availableFields().find((field) =>
                                  field.name === condition().field
                                )?.type ?? "unsupported",
                              ))}
                              value={condition().value}
                              onInput={(event) =>
                                updateFieldCondition(
                                  condition().id,
                                  "value",
                                  event.currentTarget.value,
                                )}
                            />
                            <Show
                              when={(() => {
                                const field = availableFields().find((
                                  item,
                                ) => item.name === condition().field);
                                return condition().field &&
                                  field &&
                                  !field.supported;
                              })()}
                            >
                              <p class="mt-2 text-xs ui-text-danger">
                                {t("searchPage.error.unsupportedField", {
                                  value: condition().field,
                                })}
                              </p>
                            </Show>
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

                  <div class="mt-6 flex justify-end queryLane">
                    {/* Query-lane spinner: previous results stay visible. */}
                    <Show when={advancedLoading()}>
                      <LocalBusyIndicator
                        size="sm"
                        label={t("searchPage.searchingEntries")}
                      />
                    </Show>
                    <button
                      type="button"
                      class="ui-button ui-button-primary text-sm"
                      disabled={advancedLoading()}
                      aria-busy={advancedLoading() || undefined}
                      onClick={() => void handleAdvancedSearch()}
                    >
                      <Show when={advancedLoading()}>
                        <ButtonSpinner />
                      </Show>
                      {t("searchPage.runAdvancedSearch")}
                    </button>
                  </div>
                </div>
              </Show>
            </section>

            <section
              class="searchResults"
              aria-labelledby="search-results-title"
              aria-busy={(keywordLoading() || advancedLoading()) || undefined}
            >
              <div class="searchResultsHead flex flex-wrap items-center justify-between gap-3">
                <div>
                  <h2
                    class="ui-sr-only"
                    id="search-results-title"
                  >
                    {mode() === "advanced"
                      ? t("searchPage.advancedResults")
                      : t("searchPage.keywordResults")}
                  </h2>
                  <Show
                    when={mode() === "keyword" && keywordSearchPerformed()}
                  >
                    <p class="mt-1 text-sm ui-muted">
                      {keywordResultCountLabel()}
                    </p>
                  </Show>
                  <Show
                    when={mode() === "advanced" &&
                      advancedSearchPerformed()}
                  >
                    <p class="mt-1 text-sm ui-muted">
                      {advancedResultCountLabel()}
                    </p>
                  </Show>
                </div>
                {/* Result-header spinner: previous results stay visible. */}
                <Show
                  when={keywordLoading() ||
                    (mode() === "advanced" && advancedLoading())}
                >
                  <LocalBusyIndicator
                    size="sm"
                    label={t("searchPage.searchingEntries")}
                  />
                </Show>
              </div>

              <div class="mt-4 ui-stack-sm">
                <Show when={actionError()}>
                  <p class="text-sm ui-text-danger">{actionError()}</p>
                </Show>
                <Show
                  when={mode() === "keyword" && !keywordLoading() &&
                    keywordSearchPerformed() &&
                    keywordResults().length === 0 &&
                    !actionError()}
                >
                  <p class="text-sm ui-muted">
                    {t("searchPage.noMatchingEntries")}
                  </p>
                </Show>
                <Show
                  when={mode() === "advanced" && !advancedLoading() &&
                    advancedSearchPerformed() &&
                    advancedResults().length === 0 &&
                    !actionError()}
                >
                  <p class="text-sm ui-muted">
                    {t("searchPage.noMatchingEntries")}
                  </p>
                </Show>
                <Show
                  when={mode() === "keyword" && !keywordSearchPerformed() &&
                    !keywordLoading() &&
                    !actionError()}
                >
                  <p class="text-sm ui-muted">
                    {t("searchPage.initialHelp")}
                  </p>
                </Show>
                <Show
                  when={mode() === "advanced" && !advancedSearchPerformed() &&
                    !advancedLoading() &&
                    !actionError()}
                >
                  <p class="text-sm ui-muted">
                    {t("searchPage.initialHelp")}
                  </p>
                </Show>
                <div class="searchResultList">
                  <Show when={mode() === "keyword"}>
                    <For each={keywordResults()}>
                      {(entry) => (
                        <button
                          type="button"
                          class="searchResultRow"
                          onClick={() =>
                            navigate(
                              `/spaces/${encodeURIComponent(spaceId())}/entries/${
                                encodeURIComponent(entry.id)
                              }`,
                            )}
                        >
                          <div class="searchResultContent">
                            <h3 class="text-base font-semibold">
                              {entry.title || t("common.untitled")}
                            </h3>
                            <Show when={entry.form}>
                              <span class="ui-pill">{entry.form}</span>
                            </Show>
                            <p class="mt-2 text-xs ui-muted">
                              {t("common.updatedAt", {
                                date: formatDateLabel(entry.updated_at),
                              })}
                            </p>
                          </div>
                          <span class="searchResultChevron" aria-hidden="true">
                            ›
                          </span>
                        </button>
                      )}
                    </For>
                  </Show>
                  <Show when={mode() === "advanced"}>
                    <For each={advancedResults()}>
                      {(entry) => (
                        <button
                          type="button"
                          class="searchResultRow"
                          onClick={() =>
                            navigate(
                              `/spaces/${encodeURIComponent(spaceId())}/entries/${
                                encodeURIComponent(entry.id)
                              }`,
                            )}
                        >
                          <div class="searchResultContent">
                            <h3 class="text-base font-semibold">
                              {entry.title || t("common.untitled")}
                            </h3>
                            <Show when={entry.form}>
                              <span class="ui-pill">{entry.form}</span>
                            </Show>
                            <p class="mt-2 text-xs ui-muted">
                              {t("common.updatedAt", {
                                date: formatDateLabel(entry.updated_at),
                              })}
                            </p>
                          </div>
                          <span class="searchResultChevron" aria-hidden="true">
                            ›
                          </span>
                        </button>
                      )}
                    </For>
                  </Show>
                </div>
                <Show
                  when={mode() === "keyword" && keywordHasMore() ||
                    mode() === "advanced" && advancedHasMore()}
                >
                  <div class="flex justify-center pt-2">
                    <button
                      type="button"
                      class="ui-button ui-button-secondary text-sm"
                      disabled={keywordLoading() || advancedLoading()}
                      onClick={() => {
                        if (mode() === "keyword") {
                          void loadMoreKeywordResults();
                        } else {
                          void loadMoreAdvancedResults();
                        }
                      }}
                    >
                      {t("searchPage.loadMore")}
                    </button>
                  </div>
                </Show>
              </div>
            </section>
          </main>
        </div>
      </div>
    </>
  );
}
