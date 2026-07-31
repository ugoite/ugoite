import { A, useNavigate, useParams } from "@solidjs/router";
import { createMemo, createSignal, For, Index, Show } from "solid-js";
import { SpaceShell } from "~/components/SpaceShell";
import { UiIcon } from "~/components/UiIcon";
import { formatDateLabel } from "~/lib/date-format";
import { formApi } from "~/lib/ugoite-client";
import { searchApi } from "~/lib/ugoite-client";
import { sqlSessionApi } from "~/lib/ugoite-client";
import { sqlApi } from "~/lib/ugoite-client";
import type { SearchResult, SqlEntry } from "~/lib/types";
import { createResource } from "~/lib/recoverable-resource";

type SearchMode = "keyword" | "advanced";
type FieldMatchOperator = "equals" | "contains" | "lt" | "lte" | "gt" | "gte";

type FieldCondition = {
  id: string;
  field: string;
  operator: FieldMatchOperator;
  value: string;
};

type AdvancedSearchCriteria = {
  formName: string;
  sqlRelation: string;
  updatedFrom: string;
  updatedTo: string;
  fieldConditions: Array<{
    field: string;
    type: SearchFieldType;
    operator: FieldMatchOperator;
    value: string;
  }>;
};

type SearchFieldType = "string" | "boolean" | "integer" | "float" |
  "date" | "timestamp" | "unsupported";

type SearchParameterValue = string | number | boolean | null;

type AdvancedSearchQuery = {
  sql: string;
  historySql: string;
  parameters: Record<string, SearchParameterValue>;
  parameterTypes: Record<string, string>;
};

type AvailableField = {
  name: string;
  type: SearchFieldType;
  supported: boolean;
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

function normalizeFieldType(type: string): SearchFieldType {
  switch (type) {
    case "string":
    case "markdown":
      return "string";
    case "boolean":
      return "boolean";
    case "integer":
    case "long":
      return "integer";
    case "number":
    case "float":
    case "double":
      return "float";
    case "date":
      return "date";
    case "timestamp":
    case "timestamp_tz":
    case "timestamp_ns":
    case "timestamp_tz_ns":
      return "timestamp";
    default:
      return "unsupported";
  }
}

function operatorsForFieldType(type: SearchFieldType): FieldMatchOperator[] {
  if (type === "string") return ["equals", "contains"];
  if (type === "boolean") return ["equals"];
  if (type === "integer" || type === "float" || type === "date" ||
    type === "timestamp") {
    return ["equals", "lt", "lte", "gt", "gte"];
  }
  return [];
}

function operatorLabel(operator: FieldMatchOperator): string {
  return {
    equals: "equals",
    contains: "contains",
    lt: "less than",
    lte: "less than or equal",
    gt: "greater than",
    gte: "greater than or equal",
  }[operator];
}

function fieldInputType(type: SearchFieldType): "text" | "number" | "date" | "datetime-local" {
  if (type === "integer" || type === "float") return "number";
  if (type === "date") return "date";
  if (type === "timestamp") return "datetime-local";
  return "text";
}

function escapeLikePattern(value: string): string {
  return value.replaceAll("\\", "\\\\").replaceAll("%", "\\%").replaceAll(
    "_",
    "\\_",
  );
}

function dateInput(
  value: string,
  boundary: "start" | "end",
): { parameter: string; literal: string } | null {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return null;
  const [yearText, monthText, dayText] = value.split("-");
  const year = Number(yearText);
  const month = Number(monthText);
  const day = Number(dayText);
  const date = new Date(Date.UTC(year, month - 1, day));
  if (
    ![year, month, day].every(Number.isInteger) ||
    date.getUTCFullYear() !== year ||
    date.getUTCMonth() !== month - 1 ||
    date.getUTCDate() !== day
  ) return null;
  if (boundary === "end") date.setUTCDate(date.getUTCDate() + 1);
  const dateText = date.toISOString().slice(0, 10);
  return {
    parameter: `${dateText}T00:00:00.000Z`,
    literal: `TIMESTAMP '${dateText} 00:00:00Z'`,
  };
}

function sqlLiteral(value: SearchParameterValue, type: string): string {
  if (value === null) return "NULL";
  if (type === "string") return escapeSqlLiteral(String(value));
  if (type === "boolean") return value ? "TRUE" : "FALSE";
  if (type === "integer" || type === "float") return String(value);
  if (type === "date") return `DATE '${String(value)}'`;
  if (type === "timestamp") {
    return `TIMESTAMP '${String(value).replace("T", " ")}'`;
  }
  return escapeSqlLiteral(String(value));
}

function buildAdvancedSearchQuery(
  criteria: AdvancedSearchCriteria,
): AdvancedSearchQuery | null {
  if (!criteria.formName || !criteria.sqlRelation) return null;

  const sessionConditions: string[] = [];
  const historyConditions: string[] = [];
  const parameters: Record<string, SearchParameterValue> = {};
  const parameterTypes: Record<string, string> = {};
  let parameterIndex = 0;
  const bind = (
    value: SearchParameterValue,
    type: string,
    literal: string,
  ) => {
    const name = `search_${parameterIndex++}`;
    parameters[name] = value;
    parameterTypes[name] = type;
    return { parameter: `$${name}`, literal };
  };

  const addDateCondition = (value: string, operator: ">=" | "<") => {
    const converted = dateInput(value, operator === ">=" ? "start" : "end");
    if (!converted) throw new Error(`Invalid date: ${value}`);
    const bound = bind(converted.parameter, "timestamp", converted.literal);
    sessionConditions.push(`_ugoite_updated_at ${operator} ${bound.parameter}`);
    historyConditions.push(`_ugoite_updated_at ${operator} ${bound.literal}`);
  };

  if (criteria.updatedFrom) addDateCondition(criteria.updatedFrom, ">=");
  if (criteria.updatedTo) addDateCondition(criteria.updatedTo, "<");

  for (const condition of criteria.fieldConditions) {
    const fieldPath = quoteSqlIdentifier(condition.field.toLowerCase());
    const operator = condition.operator === "equals"
      ? "="
      : condition.operator === "contains"
      ? "ILIKE"
      : condition.operator === "lt"
      ? "<"
      : condition.operator === "lte"
      ? "<="
      : condition.operator === "gt"
      ? ">"
      : ">=";
    let value: SearchParameterValue;
    let type: string;
    let literalValue: string;
    if (condition.type === "string") {
      type = "string";
      value = condition.operator === "contains"
        ? `%${escapeLikePattern(condition.value)}%`
        : condition.value;
      literalValue = escapeSqlLiteral(String(value));
    } else if (condition.type === "boolean") {
      if (condition.value !== "true" && condition.value !== "false") {
        throw new Error(`${condition.field} requires true or false.`);
      }
      type = "boolean";
      value = condition.value === "true";
      literalValue = value ? "TRUE" : "FALSE";
    } else if (condition.type === "integer") {
      if (!/^-?\d+$/.test(condition.value) ||
        !Number.isSafeInteger(Number(condition.value))) {
        throw new Error(`${condition.field} requires an integer.`);
      }
      type = "integer";
      value = Number(condition.value);
      literalValue = String(value);
    } else if (condition.type === "float") {
      const number = Number(condition.value);
      if (!Number.isFinite(number)) {
        throw new Error(`${condition.field} requires a number.`);
      }
      type = "float";
      value = number;
      literalValue = String(number);
    } else if (condition.type === "date") {
      const converted = dateInput(condition.value, "start");
      if (!converted) throw new Error(`${condition.field} requires YYYY-MM-DD.`);
      type = "date";
      value = converted.parameter.slice(0, 10);
      literalValue = sqlLiteral(value, type);
    } else if (condition.type === "timestamp") {
      const timestamp = new Date(condition.value);
      if (Number.isNaN(timestamp.getTime())) {
        throw new Error(`${condition.field} requires an RFC3339 timestamp.`);
      }
      type = "timestamp";
      value = timestamp.toISOString();
      literalValue = sqlLiteral(value, type);
    } else {
      continue;
    }
    const bound = bind(value, type, literalValue);
    const escape = condition.operator === "contains" ? " ESCAPE '\\'" : "";
    sessionConditions.push(`${fieldPath} ${operator} ${bound.parameter}${escape}`);
    historyConditions.push(`${fieldPath} ${operator} ${bound.literal}${escape}`);
  }

  const sessionWhere = sessionConditions.length > 0
    ? ` WHERE ${sessionConditions.join(" AND ")}`
    : "";
  const historyWhere = historyConditions.length > 0
    ? ` WHERE ${historyConditions.join(" AND ")}`
    : "";
  const render = (where: string) =>
    `SELECT * FROM ${quoteSqlIdentifier(criteria.sqlRelation)}${where} ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT ${ADVANCED_SEARCH_LIMIT}`;
  return {
    sql: render(sessionWhere),
    historySql: render(historyWhere),
    parameters,
    parameterTypes,
  };
}

function buildSearchHistoryName(criteria: AdvancedSearchCriteria): string {
  const parts: string[] = [];

  if (criteria.formName) {
    parts.push(`form: ${criteria.formName}`);
  }

  if (criteria.updatedFrom) {
    parts.push(`updated-from: ${criteria.updatedFrom}`);
  }

  if (criteria.updatedTo) {
    parts.push(`updated-to: ${criteria.updatedTo}`);
  }

  for (const condition of criteria.fieldConditions.slice(0, 2)) {
    const symbol = { equals: "=", contains: "~", lt: "<", lte: "<=", gt: ">", gte: ">=" }[condition.operator];
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
  const [keywordSearchPerformed, setKeywordSearchPerformed] = createSignal(
    false,
  );
  const [keywordLoading, setKeywordLoading] = createSignal(false);
  const [actionError, setActionError] = createSignal<string | null>(null);
  const [runningSearchId, setRunningSearchId] = createSignal<string | null>(
    null,
  );
  const [advancedFormName, setAdvancedFormName] = createSignal("");
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

  const selectedForm = createMemo(() => availableForms().find((entryForm) =>
    entryForm.name === advancedFormName().trim()
  ));

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

  const searchHistory = createMemo(() =>
    [...(savedSearches() || [])].sort(
      (left, right) =>
        parseTimestamp(right.updated_at) - parseTimestamp(left.updated_at),
    )
  );

  const advancedCriteria = createMemo<AdvancedSearchCriteria>(() => ({
    formName: advancedFormName().trim(),
    sqlRelation: selectedForm()?.sql_relation ?? "",
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
      .filter((condition) =>
        condition.field && condition.value && condition.supported
      )
      .map(({ supported: _supported, ...condition }) => condition),
  }));

  const keywordResultCountLabel = createMemo(() => {
    const count = keywordResults().length;
    return count === 1 ? "1 result" : `${count} results`;
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
              const field = availableFields().find((item) => item.name === value);
              const operators = operatorsForFieldType(field?.type ?? "unsupported");
              if (!operators.includes(next.operator)) next.operator = "equals";
            }
            return next;
          })()
          : condition
      )
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
      setKeywordResults(results);
    } catch (err) {
      setKeywordResults([]);
      setActionError(
        err instanceof Error ? err.message : "Failed to search entries.",
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
        setActionError(session.error || "Search failed.");
        return;
      }
      navigate(
        `/spaces/${spaceId()}/entries?session=${
          encodeURIComponent(session.id)
        }`,
      );
    } catch (err) {
      setActionError(
        err instanceof Error ? err.message : "Failed to run saved search.",
      );
    } finally {
      setRunningSearchId(null);
    }
  };

  const handleAdvancedSearch = async () => {
    const criteria = advancedCriteria();
    let query: AdvancedSearchQuery | null;
    try {
      query = buildAdvancedSearchQuery(criteria);
    } catch (err) {
      setActionError(err instanceof Error ? err.message : "Invalid search condition.");
      return;
    }
    if (!query) {
      setActionError(
        "Choose a Form with a backend SQL relation before running an advanced search.",
      );
      return;
    }

    setMode("advanced");
    setActionError(null);
    setRunningSearchId("advanced-search");
    try {
      const existing = searchHistory().find(
        (entry) =>
          entry.sql.trim() === query.historySql.trim() &&
          (!entry.variables || entry.variables.length === 0),
      );
      const session = await sqlSessionApi.create(
        spaceId(),
        query.sql,
        query.parameters,
        query.parameterTypes,
      );
      if (session.status === "failed") {
        setActionError(session.error || "Advanced search failed.");
        return;
      }
      if (!existing) {
        await sqlApi.create(spaceId(), {
          name: buildSearchHistoryName(criteria),
          sql: query.historySql,
          variables: [],
        });
        await refetchSavedSearches();
      }
      navigate(
        `/spaces/${spaceId()}/entries?session=${
          encodeURIComponent(session.id)
        }`,
      );
    } catch (err) {
      setActionError(
        err instanceof Error ? err.message : "Failed to run advanced search.",
      );
    } finally {
      setRunningSearchId(null);
    }
  };

  return (
    <SpaceShell spaceId={spaceId()} activeNavigation="search" title="Search">
      <div>
        <div class="screenHead">
          <div class="screenTitle">
            <div class="eyebrow">{spaceId()}</div>
            <h1>Search</h1>
          </div>
          <A
            href={`/spaces/${spaceId()}/queries/new`}
            class="ui-button ui-button-secondary inline-flex items-center gap-2 text-sm"
          >
            Open SQL editor
          </A>
        </div>

        <div class="searchPage">
          <aside class="facet surface">
            <button type="button" classList={{ active: mode() === "keyword" }} onClick={() => setMode("keyword")}><UiIcon name="entry" /> Entries</button>
            <button type="button" onClick={() => setMode("advanced")} classList={{ active: mode() === "advanced" }}><UiIcon name="forms" /> Forms</button>
            <A href={`/spaces/${spaceId()}/assets`}>
              <UiIcon name="asset" /> Assets
            </A>
            <A href={`/spaces/${spaceId()}/sql`}><UiIcon name="sql" /> Saved SQL</A>
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
              Quick search
            </button>
            <button
              type="button"
              class={mode() === "advanced"
                ? "ui-button ui-button-primary text-sm"
                : "ui-button ui-button-secondary text-sm"}
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
                <div class="searchBox">
                  <UiIcon name="search" />
                <input
                  id="search-keywords"
                  type="text"
                  class=""
                  placeholder="Search entries by title, fields, tags, or content"
                  value={keywordQuery()}
                  onInput={(event) =>
                    setKeywordQuery(event.currentTarget.value)}
                /></div>
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
                    onChange={(event) =>
                      setAdvancedFormName(event.currentTarget.value)}
                  >
                    <option value="">Choose a Form</option>
                    <For each={availableForms()}>
                      {(entryForm) => (
                        <option value={entryForm.name}>{entryForm.name}</option>
                      )}
                    </For>
                  </select>
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
                    onInput={(event) =>
                      setAdvancedUpdatedFrom(event.currentTarget.value)}
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
                    onInput={(event) =>
                      setAdvancedUpdatedTo(event.currentTarget.value)}
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
                      setFieldConditions((
                        current,
                      ) => [...current, createFieldCondition()])}
                  >
                    Add field condition
                  </button>
                </div>

                <Index each={fieldConditions()}>
                  {(condition) => (
                    <div class="ui-card grid gap-3 p-3 md:grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)_minmax(0,1.6fr)_auto]">
                      <div>
                        <label class="ui-label" for={`field-${condition().id}`}>
                          Field
                        </label>
                        <Show
                          when={availableFields().length > 0}
                          fallback={
                            <input
                              id={`field-${condition().id}`}
                              type="text"
                              class="ui-input mt-2 w-full"
                              placeholder="Owner"
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
                            <option value="">Choose a field</option>
                            <For each={availableFields()}>
                              {(field) => (
                                <option value={field.name} disabled={!field.supported}>
                                  {field.name}{field.supported ? "" : " (unsupported)"}
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
                          Match
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
                          <For each={operatorsForFieldType(availableFields().find((field) => field.name === condition().field)?.type ?? "unsupported")}>
                            {(operator) => (
                              <option value={operator}>{operatorLabel(operator)}</option>
                            )}
                          </For>
                        </select>
                      </div>
                      <div>
                        <label class="ui-label" for={`value-${condition().id}`}>
                          Value
                        </label>
                        <input
                          id={`value-${condition().id}`}
                          type={fieldInputType(availableFields().find((field) => field.name === condition().field)?.type ?? "string")}
                          class="ui-input mt-2 w-full"
                          placeholder={availableFields().find((field) => field.name === condition().field)?.type === "boolean" ? "true or false" : "alice"}
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
                          Remove
                        </button>
                      </div>
                    </div>
                  )}
                </Index>
              </div>

              <div class="mt-6 flex flex-wrap items-center justify-between gap-3">
                <p class="text-sm ui-muted">
                  Advanced searches compile to reusable SQL-backed history and
                  open the shared query results view.
                </p>
                <button
                  type="button"
                  class="ui-button ui-button-primary text-sm"
                  disabled={runningSearchId() === "advanced-search"}
                  onClick={() => void handleAdvancedSearch()}
                >
                  {runningSearchId() === "advanced-search"
                    ? "Running..."
                    : "Run advanced search"}
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
                <p class="text-sm ui-muted">Searching entries...</p>
              </Show>
              <Show
                when={!keywordLoading() &&
                  keywordSearchPerformed() &&
                  keywordResults().length === 0 &&
                  !actionError()}
              >
                <p class="text-sm ui-muted">No matching entries found.</p>
              </Show>
              <Show
                when={!keywordSearchPerformed() && !keywordLoading() &&
                  !actionError()}
              >
                <p class="text-sm ui-muted">
                  Search results stay here so you can refine your query without
                  leaving the page.
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
                          {entry.title || "Untitled"}
                        </h3>
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
                <p class="text-sm ui-text-danger">
                  Failed to load search history.
                </p>
              </Show>
              <Show when={forms.loading}>
                <p class="text-sm ui-muted">Loading form filters...</p>
              </Show>
              <Show when={forms.error}>
                <p class="text-sm ui-text-danger">
                  Failed to load forms for advanced search.
                </p>
              </Show>
              <Show
                when={!savedSearches.loading && searchHistory().length === 0}
              >
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
                    <p class="mt-2 text-xs ui-muted">
                      Updated {formatDateLabel(entry.updated_at)}
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
