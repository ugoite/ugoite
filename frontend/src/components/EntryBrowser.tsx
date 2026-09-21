// REQ-FE-004: canonical EntryBrowser display and query controls
// REQ-FE-008: EntryBrowser selection remains separate from mutation
import { createMemo, createSignal, For, Show } from "solid-js";
import type {
  EntryFieldCapability,
  EntryFieldRef,
  EntryFilter,
  EntryFilterOperator,
  EntryProjection,
  EntryQueryCapabilities,
  EntryQueryController,
  EntryQueryResult,
  EntrySort,
} from "~/lib/entry-query";
import { formatDateLabel } from "~/lib/date-format";
import { formatValueForDisplay } from "~/lib/display-value";
import { t } from "~/lib/i18n";
import { LocalBusyIndicator } from "./LocalBusyIndicator";

export type EntryBrowserMode = "browse" | "select_one";

export interface EntryBrowserProps {
  mode?: EntryBrowserMode;
  controller: EntryQueryController;
  capabilities: EntryQueryCapabilities;
  /** Optional human-readable Form labels. UUIDs are never used as labels. */
  formLabels?: Record<string, string>;
  onSelect?: (row: EntryQueryResult) => void;
}

const fieldKey = (field: EntryFieldRef): string => JSON.stringify(field);

const fieldLabel = (field: EntryFieldCapability): string => field.name;

const dateFromMicros = (micros: number): string =>
  formatDateLabel(new Date(micros / 1_000).toISOString());

const displayValue = (value: unknown): string => {
  if (value === null || value === undefined) return "—";
  return formatValueForDisplay(value);
};

const projectedFields = (props: EntryBrowserProps): EntryFieldCapability[] =>
  props.capabilities.fields.filter((field) => field.projectable);

const sortableFields = (props: EntryBrowserProps): EntryFieldCapability[] =>
  props.capabilities.fields.filter((field) => field.sortable);

const filterableFields = (props: EntryBrowserProps): EntryFieldCapability[] =>
  props.capabilities.fields.filter((field) => field.filterable);

const projectionFields = (projection: EntryProjection): EntryFieldRef[] =>
  projection.kind === "fields" ? projection.fields : [];

const MAX_PROJECTION_FIELDS = 64;

export function EntryBrowser(props: EntryBrowserProps) {
  const mode = () => props.mode ?? "browse";
  const queryState = createMemo(() => props.controller.query());
  const projectionState = createMemo(() => props.controller.projection());
  const rowsState = createMemo(() => props.controller.rows());
  const loadingState = createMemo(() => props.controller.loading());
  const errorState = createMemo(() => props.controller.error());
  const hasMoreState = createMemo(() => props.controller.hasMore());
  const canGoPreviousState = createMemo(() =>
    props.controller.canGoPrevious()
  );
  const projectionOptions = () => projectedFields(props);
  const sortOptions = () => sortableFields(props);
  const filterOptions = () => filterableFields(props);
  const currentSort = () => queryState().sort;
  const currentFilters = () => queryState().filters;
  const currentText = () => queryState().text ?? "";
  const [draftFilter, setDraftFilter] = createSignal<EntryFilter | null>(null);
  const filterRows = () => {
    const draft = draftFilter();
    return draft ? [...currentFilters(), draft] : currentFilters();
  };

  const isProjected = (field: EntryFieldRef) =>
    projectionFields(projectionState()).some((candidate) =>
      fieldKey(candidate) === fieldKey(field)
    );

  const toggleProjection = (field: EntryFieldRef) => {
    const selected = projectionFields(projectionState());
    const next =
      selected.some((candidate) => fieldKey(candidate) === fieldKey(field))
        ? selected.filter((candidate) =>
          fieldKey(candidate) !== fieldKey(field)
        )
        : [...selected, field];
    if (next.length > 0 && next.length <= MAX_PROJECTION_FIELDS) {
      props.controller.setProjection({ kind: "fields", fields: next });
    }
  };

  const addSort = (field: EntryFieldRef) => {
    if (
      currentSort().some((sort) => fieldKey(sort.field) === fieldKey(field))
    ) {
      return;
    }
    props.controller.setSort([
      ...currentSort(),
      { field, direction: "asc" },
    ]);
  };

  const updateSort = (index: number, sort: EntrySort) => {
    props.controller.setSort(
      currentSort().map((current, currentIndex) =>
        currentIndex === index ? sort : current
      ),
    );
  };

  const removeSort = (index: number) => {
    props.controller.setSort(
      currentSort().filter((_, currentIndex) => currentIndex !== index),
    );
  };

  const filterCapability = (
    index: number,
  ): EntryFieldCapability | undefined => {
    const filter = filterRows()[index];
    return filterOptions().find((candidate) =>
      fieldKey(candidate.field) === fieldKey(filter.field)
    );
  };

  const parseFilterValue = (fieldType: string, value: string): unknown => {
    const trimmed = value.trim();
    if (fieldType === "boolean") {
      if (trimmed.toLowerCase() === "true") return true;
      if (trimmed.toLowerCase() === "false") return false;
      return value;
    }
    if (fieldType === "integer") {
      if (/^[+-]?\d+$/.test(trimmed)) {
        const parsed = Number(trimmed);
        return Number.isSafeInteger(parsed) ? parsed : value;
      }
      return value;
    }
    if (fieldType === "numeric") {
      const parsed = Number(trimmed);
      return trimmed !== "" && Number.isFinite(parsed) ? parsed : value;
    }
    return value;
  };

  const filterValueIsValid = (
    capability: EntryFieldCapability | undefined,
    value: unknown,
  ): boolean => {
    if (!capability) return false;
    switch (capability.field_type) {
      case "boolean":
        return typeof value === "boolean";
      case "integer":
        return typeof value === "number" && Number.isSafeInteger(value);
      case "numeric":
        return typeof value === "number" && Number.isFinite(value);
      case "date":
      case "timestamp":
        return typeof value === "string" && value.trim() !== "";
      default:
        return typeof value === "string";
    }
  };

  const addFilter = () => {
    if (draftFilter()) return;
    const capability = filterOptions()[0];
    const operator = capability?.supported_operators[0];
    if (!capability || !operator) return;
    setDraftFilter({ field: capability.field, operator, value: "" });
  };

  const updateFilter = (index: number, filter: EntryFilter) => {
    if (index >= currentFilters().length) {
      setDraftFilter(filter);
      return;
    }
    props.controller.setFilters(currentFilters().map((current, currentIndex) =>
      currentIndex === index ? filter : current
    ));
  };

  const removeFilter = (index: number) => {
    if (index >= currentFilters().length) {
      setDraftFilter(null);
      return;
    }
    props.controller.setFilters(currentFilters().filter((_, currentIndex) =>
      currentIndex !== index
    ));
  };

  const applyDraftFilter = () => {
    const draft = draftFilter();
    if (!draft || !filterValueIsValid(filterCapability(currentFilters().length), draft.value)) {
      return;
    }
    props.controller.setFilters([...currentFilters(), draft]);
    setDraftFilter(null);
  };

  const rowLabel = (row: EntryQueryResult): string => {
    const firstValue = Object.values(row.properties ?? {})[0];
    return row.preview?.trim() ||
      (firstValue === undefined ? "Entry" : displayValue(firstValue));
  };

  const formLabel = (row: EntryQueryResult): string =>
    props.formLabels?.[row.form_id] || "Form";

  return (
    <section
      class="entry-browser"
      aria-busy={loadingState() || undefined}
    >
      <div
        class="entry-browser-toolbar"
        role="toolbar"
        aria-label={t("entryBrowser.label")}
      >
        <details>
          <summary class="ui-button ui-button-secondary">
            {t("entryBrowser.columns")}
          </summary>
          <fieldset class="entry-browser-popover">
            <legend class="ui-sr-only">{t("entryBrowser.columns")}</legend>
            <For each={projectionOptions()}>
              {(capability) => (
                <label>
                  <input
                    type="checkbox"
                    checked={isProjected(capability.field)}
                    disabled={!isProjected(capability.field) &&
                      projectionFields(projectionState()).length >=
                        MAX_PROJECTION_FIELDS}
                    onChange={() => toggleProjection(capability.field)}
                  />
                  {fieldLabel(capability)}
                </label>
              )}
            </For>
            <label>
              <input
                type="radio"
                name="entry-projection"
                checked={projectionState().kind === "preview"}
                onChange={() =>
                  props.controller.setProjection({ kind: "preview" })}
              />
              {t("entryBrowser.preview")}
            </label>
          </fieldset>
        </details>

        <details>
          <summary class="ui-button ui-button-secondary">
            {t("entryBrowser.filter")}
          </summary>
          <div class="entry-browser-popover">
            <label>
              {t("entryBrowser.textLabel")}
              <input
                type="search"
                class="ui-input"
                value={currentText()}
                placeholder={t("entryBrowser.textPlaceholder")}
                onInput={(event) =>
                  props.controller.setText(event.currentTarget.value)}
              />
            </label>
            <Show
              when={filterOptions().length > 0}
              fallback={
                <p class="ui-muted">{t("entryBrowser.noFilterCapabilities")}</p>
              }
            >
              <div class="ui-stack-sm">
                <For each={filterRows()}>
                  {(filter, index) => {
                    const capability = () => filterCapability(index());
                    return (
                      <div class="flex flex-wrap items-end gap-2">
                        <label>
                          {t("entryBrowser.filterField")}
                          <select
                            class="ui-select"
                            aria-label={`${t("entryBrowser.filterField")} ${
                              index() + 1
                            }`}
                            value={fieldKey(filter.field)}
                            onChange={(event) => {
                              const nextCapability = filterOptions().find((
                                candidate,
                              ) =>
                                fieldKey(candidate.field) ===
                                  event.currentTarget.value
                              );
                              const nextOperator = nextCapability
                                ?.supported_operators[0];
                              if (nextCapability && nextOperator) {
                                updateFilter(index(), {
                                  ...filter,
                                  field: nextCapability.field,
                                  operator: nextOperator,
                                });
                              }
                            }}
                          >
                            <For each={filterOptions()}>
                              {(candidate) => (
                                <option value={fieldKey(candidate.field)}>
                                  {candidate.name}
                                </option>
                              )}
                            </For>
                          </select>
                        </label>
                        <label>
                          {t("entryBrowser.filterOperator")}
                          <select
                            class="ui-select"
                            aria-label={`${t("entryBrowser.filterOperator")} ${
                              index() + 1
                            }`}
                            value={filter.operator}
                            onChange={(event) =>
                              updateFilter(index(), {
                                ...filter,
                                operator: event.currentTarget
                                  .value as EntryFilterOperator,
                              })}
                          >
                            <For each={capability()?.supported_operators ?? []}>
                              {(operator) => (
                                <option value={operator}>{operator}</option>
                              )}
                            </For>
                          </select>
                        </label>
                        <label>
                          {t("entryBrowser.filterValue")}
                          <input
                            class="ui-input"
                            value={String(filter.value ?? "")}
                            onInput={(event) =>
                              updateFilter(index(), {
                                ...filter,
                                value: parseFilterValue(
                                  capability()?.field_type ?? "string",
                                  event.currentTarget.value,
                                ),
                              })}
                          />
                        </label>
                        <button
                          type="button"
                          class="ui-button ui-button-secondary"
                          onClick={() => removeFilter(index())}
                        >
                          {t("entryBrowser.remove")}
                        </button>
                      </div>
                    );
                  }}
                </For>
                <Show when={draftFilter()}>
                  <button
                    type="button"
                    class="ui-button ui-button-primary"
                    disabled={!filterValueIsValid(
                      filterCapability(currentFilters().length),
                      draftFilter()!.value,
                    )}
                    onClick={applyDraftFilter}
                  >
                    {t("entryBrowser.apply")}
                  </button>
                </Show>
                <button
                  type="button"
                  class="ui-button ui-button-secondary"
                  onClick={addFilter}
                  disabled={Boolean(draftFilter()) || filterOptions().every((field) =>
                    currentFilters().some((filter) =>
                      fieldKey(filter.field) === fieldKey(field.field)
                    )
                  )}
                >
                  {t("entryBrowser.addFilter")}
                </button>
              </div>
            </Show>
          </div>
        </details>

        <details>
          <summary class="ui-button ui-button-secondary">
            {t("entryBrowser.sort")}
          </summary>
          <div class="entry-browser-popover ui-stack-sm">
            <For each={currentSort()}>
              {(sort, index) => (
                <div class="flex items-center gap-2">
                  <select
                    class="ui-select"
                    aria-label={`${t("entryBrowser.sortField")} ${index() + 1}`}
                    value={fieldKey(sort.field)}
                    onChange={(event) => {
                      const capability = sortOptions().find((candidate) =>
                        fieldKey(candidate.field) === event.currentTarget.value
                      );
                      if (capability) {
                        updateSort(index(), {
                          ...sort,
                          field: capability.field,
                        });
                      }
                    }}
                  >
                    <For each={sortOptions()}>
                      {(capability) => (
                        <option value={fieldKey(capability.field)}>
                          {fieldLabel(capability)}
                        </option>
                      )}
                    </For>
                  </select>
                  <select
                    class="ui-select"
                    aria-label={`${t("entryBrowser.sortDirection")} ${index() + 1}`}
                    value={sort.direction}
                    onChange={(event) =>
                      updateSort(index(), {
                        ...sort,
                        direction: event.currentTarget.value as "asc" | "desc",
                      })}
                  >
                    <option value="asc">{t("entryBrowser.ascending")}</option>
                    <option value="desc">{t("entryBrowser.descending")}</option>
                  </select>
                  <button
                    type="button"
                    class="ui-button ui-button-secondary"
                    onClick={() => removeSort(index())}
                  >
                    {t("entryBrowser.remove")}
                  </button>
                </div>
              )}
            </For>
            <Show when={sortOptions().length > currentSort().length}>
              <button
                type="button"
                class="ui-button ui-button-secondary"
                onClick={() => {
                  const candidate = sortOptions().find((field) =>
                    !currentSort().some((sort) =>
                      fieldKey(sort.field) === fieldKey(field.field)
                    )
                  );
                  if (candidate) addSort(candidate.field);
                }}
              >
                {t("entryBrowser.addSort")}
              </button>
            </Show>
          </div>
        </details>
      </div>

      <Show when={loadingState()}>
        <LocalBusyIndicator label={t("entryBrowser.loading")} />
      </Show>
      <Show when={errorState()}>
        <p class="ui-text-danger" role="alert">
          {String(errorState())}
        </p>
      </Show>
      <Show
        when={!loadingState() && rowsState().length === 0 && !errorState()}
      >
        <p class="ui-muted">{t("entryBrowser.empty")}</p>
      </Show>

      <div class="entry-browser-table" role="list">
        <For each={rowsState()}>
          {(row) => (
            <button
              type="button"
              role="listitem"
              class="entryRow w-full text-left"
              disabled={loadingState()}
              onClick={() => props.onSelect?.(row)}
            >
              <span class="entryRowMain">
                <span class="entryRowTitle">{rowLabel(row)}</span>
                <Show when={props.capabilities.scope.kind === "all"}>
                  <span class="entryRowForm ui-muted">{formLabel(row)}</span>
                </Show>
                <Show when={row.properties}>
                  <span class="entryRowProperties ui-muted">
                    {Object.entries(row.properties ?? {}).slice(0, 2).map(
                      ([name, value]) => `${name}: ${displayValue(value)}`,
                    ).join(" · ")}
                  </span>
                </Show>
              </span>
              <span class="entryRowDate ui-muted">
                {dateFromMicros(row.updated_at_micros)}
              </span>
              <Show when={mode() === "select_one"}>
                <span class="ui-pill">{t("entryBrowser.confirm")}</span>
              </Show>
            </button>
          )}
        </For>
      </div>

      <nav
        class="entry-browser-pagination"
        aria-label={t("entryBrowser.pagination")}
      >
        <button
          type="button"
          class="ui-button ui-button-secondary"
          disabled={!canGoPreviousState() || loadingState()}
          onClick={() => void props.controller.previous()}
        >
          {t("common.previous")}
        </button>
        <button
          type="button"
          class="ui-button ui-button-secondary"
          disabled={!hasMoreState() || loadingState()}
          onClick={() => void props.controller.next()}
        >
          {t("common.next")}
        </button>
      </nav>
    </section>
  );
}
