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
import {
  EntryBrowserDisplayDialog,
  type EntryBrowserDisplayMode,
} from "./EntryBrowserDisplayDialog";
import { RowListChevron } from "./RowList";
import { PagedResultTable, type ResultColumn } from "./PagedResultTable";
import { UiIcon } from "./UiIcon";

export type EntryBrowserMode = "browse" | "select_one";
export type EntryBrowserFormLabelsState = "loading" | "ready" | "error";

export interface EntryBrowserProps {
  mode?: EntryBrowserMode;
  controller: EntryQueryController;
  capabilities: EntryQueryCapabilities;
  /** Optional human-readable Form labels. UUIDs are never used as labels. */
  formLabels?: Record<string, string>;
  /** State of the optional Space-scoped Form label metadata. */
  formLabelsState?: EntryBrowserFormLabelsState;
  onSelect?: (row: EntryQueryResult) => void;
}

const fieldKey = (field: EntryFieldRef): string => JSON.stringify(field);
const filterOperatorLabel = (operator: EntryFilterOperator): string =>
  t(`entryBrowser.operator.${operator}`);

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
const projectionFields = (projection: EntryProjection): EntryFieldRef[] =>
  projection.kind === "fields" ? projection.fields : [];

export function EntryBrowser(props: EntryBrowserProps) {
  const [selectedEntryId, setSelectedEntryId] = createSignal<string>();
  const [dialogMode, setDialogMode] = createSignal<EntryBrowserDisplayMode>();
  const [dialogTrigger, setDialogTrigger] = createSignal<HTMLElement>();
  const mode = () => props.mode ?? "browse";
  const queryState = createMemo(() => props.controller.query());
  const projectionState = createMemo(() => props.controller.projection());
  const [previewSystemFields, setPreviewSystemFields] = createSignal<
    EntryFieldRef[]
  >(
    projectionState().kind === "fields"
      ? projectionFields(projectionState()).filter((field) =>
        field.kind === "created_at" || field.kind === "updated_at"
      )
      : props.capabilities.fields.filter((field) =>
        field.field.kind === "created_at" || field.field.kind === "updated_at"
      ).map((field) => field.field),
  );
  const rowsState = createMemo(() => props.controller.rows());
  const loadingState = createMemo(() => props.controller.loading());
  const errorState = createMemo(() => props.controller.error());
  const hasMoreState = createMemo(() => props.controller.hasMore());
  const canGoPreviousState = createMemo(() => props.controller.canGoPrevious());
  const currentSort = () => queryState().sort;
  const currentFilters = () => queryState().filters;
  const currentText = () => queryState().text ?? "";
  const appliedCount = (mode: EntryBrowserDisplayMode): number =>
    mode === "filter"
      ? currentFilters().length
      : mode === "sort"
      ? currentSort().length
      : 0;

  const removeFilter = (index: number) =>
    props.controller.setFilters(
      currentFilters().filter((_, currentIndex) => currentIndex !== index),
    );
  const removeSort = (index: number) =>
    props.controller.setSort(
      currentSort().filter((_, currentIndex) => currentIndex !== index),
    );
  const displayValueForField = (field: EntryFieldRef) =>
    props.capabilities.fields.find((candidate) =>
      fieldKey(candidate.field) === fieldKey(field)
    )?.name ?? field.kind;
  const applyDisplayDraft = (draft: {
    projection?: EntryProjection;
    previewSystemFields?: EntryFieldRef[];
    filters?: EntryFilter[];
    sort?: EntrySort[];
  }) => {
    if (draft.previewSystemFields) {
      setPreviewSystemFields(draft.previewSystemFields);
    }
    if (draft.projection) props.controller.setProjection(draft.projection);
    if (draft.filters) props.controller.setFilters(draft.filters);
    if (draft.sort) props.controller.setSort(draft.sort);
    setDialogMode(undefined);
    dialogTrigger()?.focus();
  };
  const closeDisplayDialog = () => {
    setDialogMode(undefined);
    dialogTrigger()?.focus();
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
      if (
        capability && previewSystemFields().some((field) => field.kind === kind)
      ) {
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
      const regularColumns: VisibleColumn[] = [];
      const timestampColumns = new Map<string, VisibleColumn>();
      for (const field of projection.fields) {
        const capability = capabilityForField(props, field);
        if (capability) {
          const column: VisibleColumn = {
            kind: "field",
            key: fieldKey(field),
            label: capability.name,
            field,
          };
          if (field.kind === "created_at" || field.kind === "updated_at") {
            timestampColumns.set(field.kind, column);
          } else {
            regularColumns.push(column);
          }
        }
      }
      const columns = [
        ...regularColumns,
        ...(["created_at", "updated_at"] as const).flatMap((kind) => {
          const column = timestampColumns.get(kind);
          return column ? [column] : [];
        }),
      ];
      if (columns.length > 0) return columns;
    }
    return previewColumns();
  };

  const tableColumns = createMemo((): ResultColumn<EntryQueryResult>[] =>
    visibleColumns().map((column) => ({
      key: column.key,
      label: column.label,
      // Keep the cell expression reactive to label metadata that arrives
      // after the query rows. Its value is display-only and must not change
      // the table's page identity or cause another EntryQuery.
      cell: (row) => (
        <span title={cellText(row, column)}>{cellText(row, column)}</span>
      ),
    }))
  );

  const cellText = (row: EntryQueryResult, column: VisibleColumn): string => {
    if (column.kind === "preview") return row.preview?.trim() || "—";
    if (column.field.kind === "form") {
      if (
        props.formLabelsState === "loading" ||
        props.formLabelsState === "error"
      ) return "—";
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
        <label class="entry-browser-search">
          <span class="ui-sr-only">{t("entryBrowser.textLabel")}</span>
          <input
            type="search"
            class="ui-input"
            value={currentText()}
            placeholder={t("entryBrowser.textPlaceholder")}
            onInput={(event) =>
              props.controller.setText(event.currentTarget.value)}
          />
        </label>
        <div class="entry-browser-display-actions">
          <For
            each={[
              {
                mode: "columns" as const,
                icon: "columns" as const,
                key: "entryBrowser.columns",
              },
              {
                mode: "filter" as const,
                icon: "filter" as const,
                key: "entryBrowser.filter",
              },
              {
                mode: "sort" as const,
                icon: "sort" as const,
                key: "entryBrowser.sort",
              },
            ]}
          >
            {(action) => (
              <button
                type="button"
                class="ui-button ui-button-secondary entry-browser-display-button"
                aria-label={`${t(action.key)}${
                  appliedCount(action.mode) > 0
                    ? `, ${
                      t("entryBrowser.appliedCount", {
                        count: appliedCount(action.mode),
                      })
                    }`
                    : ""
                }`}
                title={t(action.key)}
                aria-haspopup="dialog"
                aria-expanded={dialogMode() === action.mode}
                onClick={(event) => {
                  setDialogTrigger(event.currentTarget);
                  setDialogMode(action.mode);
                }}
              >
                <UiIcon name={action.icon} />
                <Show when={appliedCount(action.mode) > 0}>
                  <span class="entry-browser-count-badge" aria-hidden="true">
                    {appliedCount(action.mode)}
                  </span>
                </Show>
              </button>
            )}
          </For>
        </div>
      </div>

      <Show when={currentFilters().length > 0 || currentSort().length > 0}>
        <div
          class="entry-browser-query-chips"
          role="group"
          aria-label={t("entryBrowser.appliedConditions")}
        >
          <For each={currentFilters()}>
            {(filter, index) => (
              <button
                type="button"
                class="entry-browser-query-chip"
                aria-label={`${t("entryBrowser.remove")} ${
                  displayValueForField(filter.field)
                } ${filterOperatorLabel(filter.operator)} ${
                  String(filter.value ?? "")
                }`}
                onClick={() => removeFilter(index())}
              >
                <span>
                  {displayValueForField(filter.field)}{" "}
                  {filterOperatorLabel(filter.operator)}{" "}
                  {String(filter.value ?? "")}
                </span>
                <span aria-hidden="true">×</span>
              </button>
            )}
          </For>
          <For each={currentSort()}>
            {(sort, index) => (
              <button
                type="button"
                class="entry-browser-query-chip"
                aria-label={`${t("entryBrowser.remove")} ${
                  displayValueForField(sort.field)
                } ${
                  t(
                    sort.direction === "asc"
                      ? "entryBrowser.ascending"
                      : "entryBrowser.descending",
                  )
                }`}
                onClick={() => removeSort(index())}
              >
                <span>
                  {displayValueForField(sort.field)} · {t(
                    sort.direction === "asc"
                      ? "entryBrowser.ascending"
                      : "entryBrowser.descending",
                  )}
                </span>
                <span aria-hidden="true">×</span>
              </button>
            )}
          </For>
        </div>
      </Show>

      <Show when={dialogMode()}>
        {(mode) => (
          <EntryBrowserDisplayDialog
            mode={mode()}
            returnFocus={dialogTrigger()}
            fields={props.capabilities.fields.filter((field) =>
              props.capabilities.scope.kind !== "all" ||
              field.field.kind !== "property"
            )}
            projection={projectionState()}
            previewSystemFields={previewSystemFields()}
            filters={currentFilters()}
            sort={currentSort()}
            onApply={applyDisplayDraft}
            onClose={closeDisplayDialog}
          />
        )}
      </Show>

      <PagedResultTable
        columns={tableColumns()}
        rows={rowsState()}
        rowKey={(row) => row.id}
        pageIdentity={JSON.stringify({
          cursor: props.controller.currentStart(),
          projection: projectionState(),
        })}
        loading={loadingState()}
        loadingLabel={t("entryBrowser.loading")}
        error={errorState() ? String(errorState()) : null}
        emptyLabel={t("entryBrowser.empty")}
        retryLabel={t("common.retry")}
        onRetry={() => void props.controller.retry()}
        canPrevious={canGoPreviousState()}
        canNext={hasMoreState()}
        previousLabel={t("common.previous")}
        nextLabel={t("common.next")}
        onPrevious={() => void props.controller.previous()}
        onNext={() => void props.controller.next()}
        selectedRowKey={selectedEntryId()}
        onRowSelect={(row) => setSelectedEntryId(row.id)}
        renderTrailingAction={mode() === "select_one"
          ? (row) => (
            <button
              type="button"
              class="ui-button ui-button-secondary"
              disabled={loadingState()}
              onClick={(event) => {
                event.stopPropagation();
                props.onSelect?.(row);
              }}
            >
              {t("entryBrowser.confirm")}
            </button>
          )
          : (row) => (
            <button
              type="button"
              class="entry-browser-open"
              aria-label={t("entryBrowser.openEntry")}
              title={t("entryBrowser.openEntry")}
              disabled={loadingState()}
              onClick={(event) => {
                event.stopPropagation();
                props.onSelect?.(row);
              }}
            >
              <RowListChevron />
            </button>
          )}
        trailingActionLabel={mode() === "select_one"
          ? t("entryBrowser.confirm")
          : t("entryBrowser.openEntry")}
        entryDataId={(row) => row.id}
        trailingActionClassName="entry-browser-trailing-cell"
        trailingHeaderClassName="entry-browser-trailing-header"
        paginationLabel={t("entryBrowser.pagination")}
        classNames={{
          table: "entry-browser-table",
          scroll: "entry-browser-table-scroll",
        }}
      />
    </section>
  );
}
