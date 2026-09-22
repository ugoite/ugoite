import { createMemo, For, Show } from "solid-js";
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

type VisibleColumn =
  | { kind: "field"; key: string; label: string; field: EntryFieldRef }
  | { kind: "preview"; key: string; label: string };

const capabilityForField = (
  props: EntryBrowserProps,
  field: EntryFieldRef,
): EntryFieldCapability | undefined =>
  props.capabilities.fields.find((candidate) =>
    fieldKey(candidate.field) === fieldKey(field)
  );

const capabilityForKind = (
  props: EntryBrowserProps,
  kind: EntryFieldRef["kind"],
): EntryFieldCapability | undefined =>
  props.capabilities.fields.find((candidate) => candidate.field.kind === kind);

/**
 * Selectable columns. All-Forms scope offers system-level columns only
 * (Form, Created, Updated): heterogeneous Form properties are never unioned
 * into common columns, and the backend rejects property fields for the All
 * scope.
 */
const columnOptions = (props: EntryBrowserProps): EntryFieldCapability[] =>
  props.capabilities.fields.filter((field) =>
    field.projectable &&
    (props.capabilities.scope.kind !== "all" ||
      field.field.kind !== "property")
  );

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
  const canGoPreviousState = createMemo(() => props.controller.canGoPrevious());
  const projectionOptions = () => columnOptions(props);
  const sortOptions = () => sortableFields(props);
  const filterOptions = () => filterableFields(props);
  const currentSort = () => queryState().sort;
  const currentFilters = () => queryState().filters;
  const currentText = () => queryState().text ?? "";

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
    const filter = currentFilters()[index];
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

  const addFilter = () => {
    const capability = filterOptions()[0];
    const operator = capability?.supported_operators[0];
    if (!capability || !operator) return;
    props.controller.setFilters([
      ...currentFilters(),
      { field: capability.field, operator, value: "" },
    ]);
  };

  const updateFilter = (index: number, filter: EntryFilter) => {
    props.controller.setFilters(
      currentFilters().map((current, currentIndex) =>
        currentIndex === index ? filter : current
      ),
    );
  };

  const removeFilter = (index: number) => {
    props.controller.setFilters(
      currentFilters().filter((_, currentIndex) => currentIndex !== index),
    );
  };

  /**
   * Preview-mode columns. Timestamps and the Form label are row identity,
   * always returned by the backend outside the projection payload, so they
   * render alongside the preview. Form-scoped: Preview + Created + Updated.
   * All-Forms: Form + Preview + Created + Updated. Raw ids are never shown:
   * the Form column renders the human-readable form label.
   */
  const previewColumns = (): VisibleColumn[] => {
    const columns: VisibleColumn[] = [];
    if (props.capabilities.scope.kind === "all") {
      const formCapability = capabilityForKind(props, "form");
      if (formCapability) {
        columns.push({
          kind: "field",
          key: fieldKey(formCapability.field),
          label: formCapability.name,
          field: formCapability.field,
        });
      }
    }
    columns.push({
      kind: "preview",
      key: "preview",
      label: t("entryBrowser.preview"),
    });
    for (const kind of ["created_at", "updated_at"] as const) {
      const capability = capabilityForKind(props, kind);
      if (capability) {
        columns.push({
          kind: "field",
          key: fieldKey(capability.field),
          label: capability.name,
          field: capability.field,
        });
      }
    }
    return columns;
  };

  /**
   * Visible table columns, derived from the same EntryProjection state that
   * drives the query. Fields mode renders exactly the projected fields in
   * projection order, so the Columns selection and the table can never
   * disagree. Preview mode renders the preview pseudo-column plus the
   * always-available system columns.
   */
  const visibleColumns = (): VisibleColumn[] => {
    const projection = projectionState();
    if (projection.kind === "fields") {
      const columns: VisibleColumn[] = [];
      for (const field of projection.fields) {
        const capability = capabilityForField(props, field);
        if (capability) {
          columns.push({
            kind: "field",
            key: fieldKey(field),
            label: capability.name,
            field,
          });
        }
      }
      if (columns.length > 0) return columns;
    }
    return previewColumns();
  };

  const cellText = (row: EntryQueryResult, column: VisibleColumn): string => {
    if (column.kind === "preview") return row.preview?.trim() || "—";
    if (column.field.kind === "form") {
      return props.formLabels?.[row.form_id] ??
        t("entryBrowser.unknownForm");
    }
    if (column.field.kind === "created_at") {
      return dateFromMicros(row.created_at_micros);
    }
    if (column.field.kind === "updated_at") {
      return dateFromMicros(row.updated_at_micros);
    }
    const capability = capabilityForField(props, column.field);
    const value = capability ? row.properties?.[capability.name] : undefined;
    if (value === null || value === undefined) return "—";
    return displayValue(value);
  };

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
                <For each={currentFilters()}>
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
                <button
                  type="button"
                  class="ui-button ui-button-secondary"
                  onClick={addFilter}
                  disabled={filterOptions().every((field) =>
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
                    aria-label={`${t("entryBrowser.sortDirection")} ${
                      index() + 1
                    }`}
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
      <Show when={!loadingState() && rowsState().length === 0 && !errorState()}>
        <p class="ui-muted">{t("entryBrowser.empty")}</p>
      </Show>

      <Show when={rowsState().length > 0}>
        <div class="entry-browser-table-scroll">
          <table class="entry-browser-table">
            <thead>
              <tr>
                <For each={visibleColumns()}>
                  {(column) => (
                    <th scope="col" class="entry-browser-header-cell">
                      {column.label}
                    </th>
                  )}
                </For>
                <Show when={mode() === "select_one"}>
                  <th scope="col" class="entry-browser-header-cell">
                    <span class="ui-sr-only">
                      {t("entryBrowser.confirm")}
                    </span>
                  </th>
                </Show>
              </tr>
            </thead>
            <tbody>
              <For each={rowsState()}>
                {(row) => (
                  <tr
                    class="entry-browser-row"
                    data-entry-id={row.id}
                  >
                    <For each={visibleColumns()}>
                      {(column, columnIndex) => (
                        <td
                          class="entry-browser-cell"
                          data-column-key={column.key}
                        >
                          <Show
                            when={columnIndex() === 0}
                            fallback={cellText(row, column)}
                          >
                            <button
                              type="button"
                              class="entry-browser-primary"
                              disabled={loadingState()}
                              onClick={() => props.onSelect?.(row)}
                            >
                              {cellText(row, column)}
                            </button>
                          </Show>
                        </td>
                      )}
                    </For>
                    <Show when={mode() === "select_one"}>
                      <td class="entry-browser-cell">
                        <button
                          type="button"
                          class="ui-button ui-button-secondary"
                          disabled={loadingState()}
                          onClick={() => props.onSelect?.(row)}
                        >
                          {t("entryBrowser.confirm")}
                        </button>
                      </td>
                    </Show>
                  </tr>
                )}
              </For>
            </tbody>
          </table>
        </div>
      </Show>

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
