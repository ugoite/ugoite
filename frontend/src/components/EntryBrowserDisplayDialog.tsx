import { createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { createStore } from "solid-js/store";
import type {
  EntryFieldCapability,
  EntryFieldRef,
  EntryFilter,
  EntryFilterOperator,
  EntryProjection,
  EntrySort,
} from "~/lib/entry-query";
import { t } from "~/lib/i18n";
import { UiIcon } from "./UiIcon";

export type EntryBrowserDisplayMode = "columns" | "filter" | "sort";

export interface EntryBrowserDisplayDialogProps {
  mode: EntryBrowserDisplayMode;
  returnFocus?: HTMLElement;
  fields: EntryFieldCapability[];
  projection: EntryProjection;
  previewSystemFields: EntryFieldRef[];
  filters: EntryFilter[];
  sort: EntrySort[];
  onApply: (draft: {
    projection?: EntryProjection;
    previewSystemFields?: EntryFieldRef[];
    filters?: EntryFilter[];
    sort?: EntrySort[];
  }) => void;
  onClose: () => void;
}

const MAX_PROJECTION_FIELDS = 64;
const MAX_ENTRY_FILTERS = 32;
const MAX_ENTRY_SORTS = 8;
const fieldKey = (field: EntryFieldRef): string => JSON.stringify(field);

interface FilterDraft extends EntryFilter {
  draftId: number;
  sourceValue: unknown;
  valueDirty: boolean;
}

const pad2 = (value: number): string => String(value).padStart(2, "0");

const localDateTime = (value: string): string => {
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) return value;
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${
    pad2(date.getDate())
  }T${pad2(date.getHours())}:${pad2(date.getMinutes())}:${
    pad2(date.getSeconds())
  }`;
};

const filterValueText = (fieldType: string, value: unknown): string => {
  if (value === null || value === undefined) return "";
  if (fieldType === "timestamp_tz" || fieldType === "timestamp_tz_ns") {
    return typeof value === "string" ? localDateTime(value) : String(value);
  }
  return String(value);
};

const parseFilterValue = (
  fieldType: string,
  value: string,
): { value: unknown; valid: boolean } => {
  const trimmed = value.trim();
  if (trimmed === "") return { value, valid: false };
  if (fieldType === "boolean") {
    if (trimmed === "true") return { value: true, valid: true };
    if (trimmed === "false") return { value: false, valid: true };
    return { value, valid: false };
  }
  if (fieldType === "integer" || fieldType === "long") {
    if (!/^[+-]?\d+$/.test(trimmed)) return { value, valid: false };
    const parsed = Number(trimmed);
    return { value: parsed, valid: Number.isSafeInteger(parsed) };
  }
  if (["numeric", "float", "double"].includes(fieldType)) {
    if (!/^[+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?$/.test(trimmed)) {
      return { value, valid: false };
    }
    const parsed = Number(trimmed);
    return { value: parsed, valid: Number.isFinite(parsed) };
  }
  if (fieldType === "date") {
    return {
      value: trimmed,
      valid: /^\d{4}-\d{2}-\d{2}$/.test(trimmed) &&
        Number.isFinite(Date.parse(`${trimmed}T00:00:00Z`)),
    };
  }
  if (fieldType.startsWith("timestamp")) {
    const timezoneAware = fieldType === "timestamp_tz" ||
      fieldType === "timestamp_tz_ns";
    const normalized = trimmed.length === 16 ? `${trimmed}:00` : trimmed;
    const parsed = new Date(normalized);
    return {
      value: timezoneAware && Number.isFinite(parsed.getTime())
        ? parsed.toISOString()
        : normalized,
      valid: Number.isFinite(parsed.getTime()),
    };
  }
  return { value, valid: true };
};

const operatorLabel = (operator: EntryFilterOperator): string =>
  t(`entryBrowser.operator.${operator}`);

export function EntryBrowserDisplayDialog(
  props: EntryBrowserDisplayDialogProps,
) {
  let dialog: HTMLDivElement | undefined;
  let returnFocus: HTMLElement | null = null;

  const [projectionKind, setProjectionKind] = createSignal(
    props.projection.kind,
  );
  const [selectedFields, setSelectedFields] = createSignal<EntryFieldRef[]>(
    props.projection.kind === "fields"
      ? props.projection.fields.filter((field) =>
        field.kind !== "created_at" && field.kind !== "updated_at"
      )
      : [],
  );
  const [selectedSystemFields, setSelectedSystemFields] = createSignal<
    EntryFieldRef[]
  >(
    props.projection.kind === "fields"
      ? props.projection.fields.filter((field) =>
        field.kind === "created_at" || field.kind === "updated_at"
      )
      : [...props.previewSystemFields],
  );
  const [filters, setFilters] = createStore<FilterDraft[]>(
    props.filters.map((filter, draftId) => ({
      ...filter,
      draftId,
      sourceValue: filter.value,
      valueDirty: false,
      value: filterValueText(
        props.fields.find((field) =>
          fieldKey(field.field) === fieldKey(filter.field)
        )
          ?.field_type ?? "string",
        filter.value,
      ),
    })),
  );
  let nextFilterDraftId = filters.length;
  const [sort, setSort] = createSignal(
    props.sort.map((entry) => ({ ...entry })),
  );

  const heading = () => t(`entryBrowser.${props.mode}`);
  const filterCapability = (filter: EntryFilter) =>
    props.fields.find((field) =>
      fieldKey(field.field) === fieldKey(filter.field)
    );
  const filterDraftValid = () =>
    filters.length <= MAX_ENTRY_FILTERS &&
    filters.every((filter) => {
      const capability = filterCapability(filter);
      return !!capability?.filterable &&
        capability.supported_operators.includes(filter.operator) &&
        (!filter.valueDirty ||
          parseFilterValue(capability.field_type, String(filter.value ?? ""))
            .valid);
    });
  const sortDraftValid = () =>
    sort().length <= MAX_ENTRY_SORTS &&
    new Set(sort().map((item) => fieldKey(item.field))).size ===
      sort().length &&
    sort().every((item) =>
      props.fields.some((field) =>
        field.sortable && fieldKey(field.field) === fieldKey(item.field)
      )
    );
  const toggleField = (field: EntryFieldRef) => {
    const current = selectedFields();
    const exists = current.some((candidate) =>
      fieldKey(candidate) === fieldKey(field)
    );
    const next = exists
      ? current.filter((candidate) => fieldKey(candidate) !== fieldKey(field))
      : [...current, field];
    if (
      next.length + selectedSystemFields().length > 0 &&
      next.length + selectedSystemFields().length <= MAX_PROJECTION_FIELDS
    ) {
      setSelectedFields(next);
      setProjectionKind("fields");
    }
  };
  const regularColumnOptions = () => {
    const projectable = props.fields.filter((field) =>
      field.projectable && field.field.kind !== "created_at" &&
      field.field.kind !== "updated_at"
    );
    const selected = selectedFields().flatMap((field) => {
      const capability = projectable.find((item) =>
        fieldKey(item.field) === fieldKey(field)
      );
      return capability ? [capability] : [];
    });
    const selectedKeys = new Set(
      selected.map((field) => fieldKey(field.field)),
    );
    return [
      ...selected,
      ...projectable.filter((field) =>
        !selectedKeys.has(fieldKey(field.field))
      ),
    ];
  };
  const moveField = (index: number, offset: -1 | 1) => {
    const next = [...selectedFields()];
    const target = index + offset;
    if (target < 0 || target >= next.length) return;
    [next[index], next[target]] = [next[target], next[index]];
    setSelectedFields(next);
  };
  const toggleSystemField = (field: EntryFieldRef) => {
    const current = selectedSystemFields();
    const exists = current.some((candidate) =>
      fieldKey(candidate) === fieldKey(field)
    );
    const next = exists
      ? current.filter((candidate) => fieldKey(candidate) !== fieldKey(field))
      : [...current, field];
    const total = next.length + selectedFields().length;
    if (
      total <= MAX_PROJECTION_FIELDS &&
      (projectionKind() === "preview" || total > 0)
    ) setSelectedSystemFields(next);
  };
  const addFilter = () => {
    if (filters.length >= MAX_ENTRY_FILTERS) return;
    const availableField = (field: EntryFieldCapability) =>
      field.filterable && field.supported_operators.length > 0 &&
      !filters.some((filter) =>
        fieldKey(filter.field) === fieldKey(field.field)
      );
    const available =
      props.fields.find((field) =>
        field.field.kind === "property" && availableField(field)
      ) ?? props.fields.find(availableField);
    if (!available) return;
    const operator = available.supported_operators[0];
    setFilters(filters.length, {
      draftId: nextFilterDraftId++,
      field: available.field,
      operator,
      value: available.field_type === "boolean" ? "true" : "",
      sourceValue: undefined,
      valueDirty: true,
    });
  };
  const updateFilter = (index: number, next: EntryFilter) =>
    setFilters(index, {
      sourceValue: filters[index].sourceValue,
      valueDirty: filters[index].valueDirty ||
        fieldKey(next.field) !== fieldKey(filters[index].field) ||
        next.value !== filters[index].value,
      field: next.field,
      operator: next.operator,
      value: next.value,
    });
  const addSort = () => {
    if (sort().length >= MAX_ENTRY_SORTS) return;
    const available = props.fields.find((field) =>
      field.sortable &&
      !sort().some((item) => fieldKey(item.field) === fieldKey(field.field))
    );
    if (available) {
      setSort([...sort(), { field: available.field, direction: "asc" }]);
    }
  };
  const moveSort = (index: number, offset: -1 | 1) => {
    const next = [...sort()];
    const target = index + offset;
    if (target < 0 || target >= next.length) return;
    [next[index], next[target]] = [next[target], next[index]];
    setSort(next);
  };
  const apply = () => {
    if (props.mode === "columns") {
      if (projectionKind() === "preview") {
        props.onApply({
          projection: { kind: "preview" },
          previewSystemFields: selectedSystemFields(),
        });
      } else if (selectedFields().length + selectedSystemFields().length > 0) {
        props.onApply({
          projection: {
            kind: "fields",
            fields: [
              ...selectedFields(),
              ...(["created_at", "updated_at"] as const).flatMap((kind) =>
                selectedSystemFields().filter((field) => field.kind === kind)
              ),
            ],
          },
          previewSystemFields: selectedSystemFields(),
        });
      }
      return;
    }
    if (props.mode === "filter" && filterDraftValid()) {
      props.onApply({
        filters: filters.map((filter) => {
          const capability = filterCapability(filter)!;
          return {
            field: filter.field,
            operator: filter.operator,
            value: filter.valueDirty
              ? parseFilterValue(
                capability.field_type,
                String(filter.value ?? ""),
              ).value
              : filter.sourceValue,
          };
        }),
      });
      return;
    }
    if (props.mode === "sort" && sortDraftValid()) {
      props.onApply({ sort: sort() });
    }
  };
  const canApply = () =>
    props.mode === "columns"
      ? projectionKind() === "preview" ||
        selectedFields().length + selectedSystemFields().length > 0
      : props.mode === "filter"
      ? filterDraftValid()
      : sortDraftValid();

  onMount(() => {
    returnFocus = props.returnFocus ??
      (document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null);
    dialog?.querySelector<HTMLElement>("button, input, select")?.focus();
    const handleFocus = () => returnFocus?.focus();
    onCleanup(handleFocus);
  });

  const handleKeyDown = (event: KeyboardEvent) => {
    if (!dialog) return;
    if (event.key === "Escape") {
      event.preventDefault();
      props.onClose();
      return;
    }
    if (event.key !== "Tab") return;
    const items = [...dialog.querySelectorAll<HTMLElement>(
      'button:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex="0"]',
    )];
    if (items.length === 0) return;
    const first = items[0];
    const last = items.at(-1)!;
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  return (
    <div
      class="ui-backdrop entry-browser-dialog-backdrop"
      onClick={(event) => {
        if (event.target === event.currentTarget) props.onClose();
      }}
    >
      <div
        ref={dialog}
        class="ui-dialog entry-browser-display-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="entry-browser-dialog-title"
        tabIndex={-1}
        onKeyDown={handleKeyDown}
      >
        <header class="ui-dialog-header">
          <h2 id="entry-browser-dialog-title" class="ui-dialog-title">
            {heading()}
          </h2>
          <button
            type="button"
            class="ui-button ui-button-secondary entry-browser-dialog-close"
            aria-label={t("entryBrowser.closeDialog")}
            title={t("entryBrowser.closeDialog")}
            onClick={props.onClose}
          >
            <UiIcon name="close" />
          </button>
        </header>

        <Show when={props.mode === "columns"}>
          <div class="entry-browser-dialog-content ui-stack-sm">
            <fieldset class="ui-stack-sm">
              <legend>{t("entryBrowser.columns")}</legend>
              <label class="entry-browser-dialog-option">
                <input
                  type="radio"
                  name="entry-projection"
                  checked={projectionKind() === "preview"}
                  onChange={() => setProjectionKind("preview")}
                />
                {t("entryBrowser.preview")}
              </label>
              <label class="entry-browser-dialog-option">
                <input
                  type="radio"
                  name="entry-projection"
                  checked={projectionKind() === "fields"}
                  onChange={() => setProjectionKind("fields")}
                />
                {t("entryBrowser.selectedFields")}
              </label>
              <p class="ui-muted">{t("entryBrowser.fields")}</p>
              <For
                each={regularColumnOptions()}
              >
                {(capability) => {
                  const index = () =>
                    selectedFields().findIndex((field) =>
                      fieldKey(field) === fieldKey(capability.field)
                    );
                  const checked = () => index() >= 0;
                  return (
                    <div class="entry-browser-column-row">
                      <label class="entry-browser-dialog-option">
                        <input
                          type="checkbox"
                          checked={checked()}
                          disabled={!checked() &&
                            selectedFields().length +
                                  selectedSystemFields().length >=
                              MAX_PROJECTION_FIELDS}
                          onChange={() => toggleField(capability.field)}
                        />
                        {capability.name}
                      </label>
                      <div class="entry-browser-column-order">
                        <button
                          type="button"
                          class="ui-button ui-button-secondary"
                          aria-label={`${
                            t("entryBrowser.moveUp")
                          } ${capability.name}`}
                          disabled={!checked() || index() === 0}
                          onClick={() => moveField(index(), -1)}
                        >
                          ↑
                        </button>
                        <button
                          type="button"
                          class="ui-button ui-button-secondary"
                          aria-label={`${
                            t("entryBrowser.moveDown")
                          } ${capability.name}`}
                          disabled={!checked() ||
                            index() === selectedFields().length - 1}
                          onClick={() => moveField(index(), 1)}
                        >
                          ↓
                        </button>
                      </div>
                    </div>
                  );
                }}
              </For>
              <p class="ui-muted">{t("entryBrowser.timestampsFixed")}</p>
              <For
                each={props.fields.filter((field) =>
                  field.projectable &&
                  (field.field.kind === "created_at" ||
                    field.field.kind === "updated_at")
                )}
              >
                {(capability) => (
                  <label class="entry-browser-dialog-option">
                    <input
                      type="checkbox"
                      checked={selectedSystemFields().some((field) =>
                        fieldKey(field) === fieldKey(capability.field)
                      )}
                      disabled={selectedFields().length +
                                  selectedSystemFields().length <= 1 &&
                          selectedSystemFields().some((field) =>
                            fieldKey(field) === fieldKey(capability.field)
                          ) ||
                        !selectedSystemFields().some((field) =>
                            fieldKey(field) === fieldKey(capability.field)
                          ) &&
                          selectedFields().length +
                                selectedSystemFields().length >=
                            MAX_PROJECTION_FIELDS}
                      onChange={() => toggleSystemField(capability.field)}
                    />
                    {capability.name}
                  </label>
                )}
              </For>
            </fieldset>
          </div>
        </Show>

        <Show when={props.mode === "filter"}>
          <div class="entry-browser-dialog-content ui-stack-sm">
            <Show
              when={props.fields.some((field) =>
                field.filterable && field.supported_operators.length > 0
              )}
              fallback={
                <p class="ui-muted">{t("entryBrowser.noFilterCapabilities")}</p>
              }
            >
              <For each={filters}>
                {(filter, index) => {
                  const capability = () => filterCapability(filter);
                  const type = () => capability()?.field_type ?? "string";
                  const parsed = () =>
                    parseFilterValue(type(), String(filter.value ?? ""));
                  const updateField = (key: string) => {
                    const next = props.fields.find((field) =>
                      fieldKey(field.field) === key && field.filterable &&
                      field.supported_operators.length > 0
                    );
                    const operator = next?.supported_operators[0];
                    if (next && operator) {
                      updateFilter(index(), {
                        field: next.field,
                        operator,
                        value: "",
                      });
                    }
                  };
                  return (
                    <div class="entry-browser-filter-row">
                      <label>
                        {t("entryBrowser.filterField")}
                        <select
                          class="ui-select"
                          aria-label={`${t("entryBrowser.filterField")} ${
                            index() + 1
                          }`}
                          value={fieldKey(filter.field)}
                          onChange={(event) =>
                            updateField(event.currentTarget.value)}
                        >
                          <For
                            each={props.fields.filter((field) =>
                              field.filterable &&
                              field.supported_operators.length > 0
                            )}
                          >
                            {(field) => (
                              <option value={fieldKey(field.field)}>
                                {field.name}
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
                              <option value={operator}>
                                {operatorLabel(operator)}
                              </option>
                            )}
                          </For>
                        </select>
                      </label>
                      <label>
                        {t("entryBrowser.filterValue")}
                        <Show
                          when={type() === "boolean"}
                          fallback={
                            <input
                              class="ui-input"
                              type={type() === "integer" || type() === "long" ||
                                  type() === "numeric" || type() === "float" ||
                                  type() === "double"
                                ? "number"
                                : type() === "date"
                                ? "date"
                                : type().startsWith("timestamp")
                                ? "datetime-local"
                                : "text"}
                              step={type() === "numeric" ||
                                  type() === "float" || type() === "double"
                                ? "any"
                                : type().startsWith("timestamp")
                                ? "1"
                                : undefined}
                              value={String(filter.value ?? "")}
                              aria-invalid={!parsed().valid || undefined}
                              onInput={(event) =>
                                updateFilter(index(), {
                                  ...filter,
                                  value: event.currentTarget.value,
                                })}
                            />
                          }
                        >
                          <select
                            class="ui-select"
                            value={String(filter.value ?? "true")}
                            onChange={(event) =>
                              updateFilter(index(), {
                                ...filter,
                                value: event.currentTarget.value,
                              })}
                          >
                            <option value="true">
                              {t("entryBrowser.booleanTrue")}
                            </option>
                            <option value="false">
                              {t("entryBrowser.booleanFalse")}
                            </option>
                          </select>
                        </Show>
                      </label>
                      <button
                        type="button"
                        class="ui-button ui-button-secondary"
                        onClick={() =>
                          setFilters((current) =>
                            current.filter((_, i) => i !== index())
                          )}
                      >
                        {t("entryBrowser.remove")}
                      </button>
                      <Show when={!parsed().valid}>
                        <p class="ui-alert ui-alert-error" role="alert">
                          {t("entryBrowser.invalidFilterValue")}
                        </p>
                      </Show>
                    </div>
                  );
                }}
              </For>
              <button
                type="button"
                class="ui-button ui-button-secondary"
                onClick={addFilter}
                disabled={filters.length >= MAX_ENTRY_FILTERS ||
                  !props.fields.some((field) =>
                    field.filterable && field.supported_operators.length > 0 &&
                    !filters.some((filter) =>
                      fieldKey(filter.field) === fieldKey(field.field)
                    )
                  )}
              >
                {t("entryBrowser.addFilter")}
              </button>
            </Show>
          </div>
        </Show>

        <Show when={props.mode === "sort"}>
          <div class="entry-browser-dialog-content ui-stack-sm">
            <Show
              when={props.fields.some((field) => field.sortable)}
              fallback={
                <p class="ui-muted">{t("entryBrowser.noSortCapabilities")}</p>
              }
            >
              <For each={sort()}>
                {(item, index) => (
                  <div class="entry-browser-sort-row">
                    <span class="entry-browser-sort-priority">
                      {index() + 1}
                    </span>
                    <select
                      class="ui-select"
                      aria-label={`${t("entryBrowser.sortField")} ${
                        index() + 1
                      }`}
                      value={fieldKey(item.field)}
                      onChange={(event) => {
                        const next = props.fields.find((field) =>
                          field.sortable &&
                          fieldKey(field.field) === event.currentTarget.value
                        );
                        if (
                          next && !sort().some((other, i) =>
                            i !== index() &&
                            fieldKey(other.field) === fieldKey(next.field)
                          )
                        ) {
                          setSort(
                            sort().map((current, i) => i === index()
                              ? { ...current, field: next.field }
                              : current
                            ),
                          );
                        }
                      }}
                    >
                      <For
                        each={props.fields.filter((field) =>
                          field.sortable &&
                          !sort().some((other, i) => i !== index() &&
                            fieldKey(other.field) === fieldKey(field.field)
                          )
                        )}
                      >
                        {(field) => (
                          <option value={fieldKey(field.field)}>
                            {field.name}
                          </option>
                        )}
                      </For>
                    </select>
                    <label>
                      {t("entryBrowser.sortDirection")}
                      <select
                        class="ui-select"
                        aria-label={`${t("entryBrowser.sortDirection")} ${
                          index() + 1
                        }`}
                        value={item.direction}
                        onChange={(event) =>
                          setSort(
                            sort().map((current, i) =>
                              i === index()
                                ? {
                                  ...current,
                                  direction: event.currentTarget.value as
                                    | "asc"
                                    | "desc",
                                }
                                : current
                            ),
                          )}
                      >
                        <option value="asc">
                          {t("entryBrowser.ascending")}
                        </option>
                        <option value="desc">
                          {t("entryBrowser.descending")}
                        </option>
                      </select>
                    </label>
                    <div class="entry-browser-sort-order">
                      <button
                        type="button"
                        class="ui-button ui-button-secondary"
                        aria-label={`${
                          t("entryBrowser.moveUp")
                        } ${item.field.kind}`}
                        disabled={index() === 0}
                        onClick={() => moveSort(index(), -1)}
                      >
                        ↑
                      </button>
                      <button
                        type="button"
                        class="ui-button ui-button-secondary"
                        aria-label={`${
                          t("entryBrowser.moveDown")
                        } ${item.field.kind}`}
                        disabled={index() === sort().length - 1}
                        onClick={() => moveSort(index(), 1)}
                      >
                        ↓
                      </button>
                    </div>
                    <button
                      type="button"
                      class="ui-button ui-button-secondary"
                      onClick={() =>
                        setSort(sort().filter((_, i) => i !== index()))}
                    >
                      {t("entryBrowser.remove")}
                    </button>
                  </div>
                )}
              </For>
              <button
                type="button"
                class="ui-button ui-button-secondary"
                onClick={addSort}
                disabled={sort().length >= MAX_ENTRY_SORTS ||
                  !props.fields.some((field) =>
                    field.sortable &&
                    !sort().some((item) =>
                      fieldKey(item.field) === fieldKey(field.field)
                    )
                  )}
              >
                {t("entryBrowser.addSort")}
              </button>
            </Show>
          </div>
        </Show>

        <footer class="ui-dialog-actions">
          <button
            type="button"
            class="ui-button ui-button-secondary"
            onClick={props.onClose}
          >
            {t("common.cancel")}
          </button>
          <button
            type="button"
            class="ui-button ui-button-primary"
            disabled={!canApply()}
            onClick={apply}
          >
            {t("entryBrowser.apply")}
          </button>
        </footer>
      </div>
    </div>
  );
}
