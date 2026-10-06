import { For, Show } from "solid-js";
import { EntryResultTable } from "~/components/EntryResultTable";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { ResultPagination } from "~/components/ResultPagination";
import {
  entryQueryDisplayColumns,
} from "~/components/CompositionEntryQueryTable";
import type {
  CompositionResolveDiagnostic,
  CompositionResolvedSource,
} from "~/lib/composition-api";
import type {
  EntryQueryCompositionFilter,
  EntryQueryCompositionProjection,
  EntryQueryCompositionSort,
} from "~/lib/entry-query-composition";
import type { DraftSource } from "~/lib/composition-draft";
import {
  entryQueryFilterOperators,
  entryQuerySortDirections,
} from "~/lib/composition-draft";
import type { CompositionSourcePageState } from "~/lib/composition-query-handle";
import { t } from "~/lib/i18n";

export type EntryQuerySource = Extract<DraftSource, { kind: "entry_query" }>;

const filterValueText = (value: unknown): string => {
  if (value !== null && typeof value === "object") {
    const record = value as Record<string, unknown>;
    if (typeof record.parameter === "string") {
      return `{{${record.parameter}}}`;
    }
  }
  if (value === null || value === undefined) return "";
  return String(value);
};

interface EntryQuerySourceEditorProps {
  source: EntryQuerySource;
  /** Narrow draft updaters; each returns false when the edit is rejected. */
  onFilters: (filters: EntryQueryCompositionFilter[]) => boolean;
  onSort: (sort: EntryQueryCompositionSort[]) => boolean;
  onProjection: (projection: EntryQueryCompositionProjection) => boolean;
  planSources: readonly CompositionResolvedSource[];
  sourceStates: Record<string, CompositionSourcePageState>;
  diagnostics: readonly CompositionResolveDiagnostic[];
  onNext: (sourceId: string) => void;
  onPrevious: (sourceId: string) => void;
  onRetry: (sourceId: string) => void;
}

/**
 * EntryQuery source editor for the Data workspace. Fields, filter, sort,
 * and result map onto the draft's `source.query` through the existing
 * EntryQuery vocabulary (field IDs, six filter operators, two sort
 * directions, fields/preview projection). The result preview reuses the
 * shared per-source preview page; no new query implementation.
 */
export function EntryQuerySourceEditor(props: EntryQuerySourceEditorProps) {
  const schemaFieldIds = () =>
    props.source.fieldSchema.map((entry) => entry.field_id);

  const addFilter = () => {
    const first = schemaFieldIds()[0];
    if (first === undefined) return;
    props.onFilters([
      ...props.source.query.filters,
      { field_id: first, operator: "equals", value: "" },
    ]);
  };

  const addSort = () => {
    const first = schemaFieldIds()[0];
    if (first === undefined) return;
    props.onSort([
      ...props.source.query.sort,
      { field_id: first, direction: "asc" },
    ]);
  };

  const planSource = () =>
    props.planSources.find((source) =>
      source.kind === "entry_query" && source.source_id === props.source.draftId
    );
  const sourceState = () => props.sourceStates[props.source.draftId];
  const unavailable = () =>
    !planSource() &&
    props.diagnostics.some((diagnostic) =>
      diagnostic.code === "source_unavailable"
    );

  return (
    <div class="ui-stack">
      <section aria-label={t("entryBrowser.fields")}>
        <h3 class="ui-label">{t("entryBrowser.fields")}</h3>
        <Show
          when={props.source.fieldSchema.length > 0}
          fallback={<p class="ui-muted">{t("composition.queryEmpty")}</p>}
        >
          <ul class="ui-stack-sm">
            <For each={props.source.fieldSchema}>
              {(entry) => (
                <li>
                  <span class="pill">
                    <span>{entry.field_id}</span>
                    <span class="ui-muted">{entry.field_type}</span>
                  </span>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </section>

      <section aria-label={t("entryBrowser.filter")}>
        <div class="flex flex-wrap items-center justify-between gap-2">
          <h3 class="ui-label">{t("entryBrowser.filter")}</h3>
          <button
            class="ui-button ui-button-secondary"
            type="button"
            disabled={schemaFieldIds().length === 0}
            onClick={addFilter}
          >
            {t("entryBrowser.addFilter")}
          </button>
        </div>
        <Show
          when={props.source.query.filters.length > 0}
          fallback={
            <p class="ui-muted">{t("entryBrowser.noFilterCapabilities")}</p>
          }
        >
          <ul class="ui-stack-sm">
            <For each={props.source.query.filters}>
              {(filter, index) => (
                <li class="flex flex-wrap items-center gap-2">
                  <label>
                    <span class="ui-sr-only">
                      {t("entryBrowser.filterField")}
                    </span>
                    <select
                      class="ui-input"
                      value={filter.field_id}
                      onChange={(event) => {
                        const next = [...props.source.query.filters];
                        next[index()] = {
                          ...filter,
                          field_id: Number(event.currentTarget.value),
                        };
                        props.onFilters(next);
                      }}
                    >
                      <For each={schemaFieldIds()}>
                        {(fieldId) => <option value={fieldId}>{fieldId}
                        </option>}
                      </For>
                    </select>
                  </label>
                  <label>
                    <span class="ui-sr-only">
                      {t("entryBrowser.filterOperator")}
                    </span>
                    <select
                      class="ui-input"
                      value={filter.operator}
                      onChange={(event) => {
                        const next = [...props.source.query.filters];
                        next[index()] = {
                          ...filter,
                          operator: event.currentTarget
                            .value as EntryQueryCompositionFilter["operator"],
                        };
                        props.onFilters(next);
                      }}
                    >
                      <For each={[...entryQueryFilterOperators]}>
                        {(operator) => (
                          <option value={operator}>
                            {t(
                              `entryBrowser.operator.${operator}` as
                                | "entryBrowser.operator.equals"
                                | "entryBrowser.operator.contains"
                                | "entryBrowser.operator.lt"
                                | "entryBrowser.operator.lte"
                                | "entryBrowser.operator.gt"
                                | "entryBrowser.operator.gte",
                            )}
                          </option>
                        )}
                      </For>
                    </select>
                  </label>
                  <label>
                    <span class="ui-sr-only">
                      {t("entryBrowser.filterValue")}
                    </span>
                    <input
                      class="ui-input"
                      value={filterValueText(filter.value)}
                      onChange={(event) => {
                        const next = [...props.source.query.filters];
                        next[index()] = {
                          ...filter,
                          value: event.currentTarget.value,
                        };
                        props.onFilters(next);
                      }}
                    />
                  </label>
                  <button
                    class="ui-button ui-button-secondary"
                    type="button"
                    aria-label={t("entryBrowser.remove")}
                    onClick={() =>
                      props.onFilters(
                        props.source.query.filters.filter((_, at) =>
                          at !== index()
                        ),
                      )}
                  >
                    {t("entryBrowser.remove")}
                  </button>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </section>

      <section aria-label={t("entryBrowser.sort")}>
        <div class="flex flex-wrap items-center justify-between gap-2">
          <h3 class="ui-label">{t("entryBrowser.sort")}</h3>
          <button
            class="ui-button ui-button-secondary"
            type="button"
            disabled={schemaFieldIds().length === 0}
            onClick={addSort}
          >
            {t("entryBrowser.addSort")}
          </button>
        </div>
        <Show
          when={props.source.query.sort.length > 0}
          fallback={
            <p class="ui-muted">{t("entryBrowser.noSortCapabilities")}</p>
          }
        >
          <ul class="ui-stack-sm">
            <For each={props.source.query.sort}>
              {(clause, index) => (
                <li class="flex flex-wrap items-center gap-2">
                  <label>
                    <span class="ui-sr-only">
                      {t("entryBrowser.sortField")}
                    </span>
                    <select
                      class="ui-input"
                      value={clause.field_id}
                      onChange={(event) => {
                        const next = [...props.source.query.sort];
                        next[index()] = {
                          ...clause,
                          field_id: Number(event.currentTarget.value),
                        };
                        props.onSort(next);
                      }}
                    >
                      <For each={schemaFieldIds()}>
                        {(fieldId) => <option value={fieldId}>{fieldId}
                        </option>}
                      </For>
                    </select>
                  </label>
                  <label>
                    <span class="ui-sr-only">
                      {t("entryBrowser.sortDirection")}
                    </span>
                    <select
                      class="ui-input"
                      value={clause.direction}
                      onChange={(event) => {
                        const next = [...props.source.query.sort];
                        next[index()] = {
                          ...clause,
                          direction: event.currentTarget
                            .value as EntryQueryCompositionSort["direction"],
                        };
                        props.onSort(next);
                      }}
                    >
                      <For each={[...entryQuerySortDirections]}>
                        {(direction) => (
                          <option value={direction}>
                            {direction === "asc"
                              ? t("entryBrowser.ascending")
                              : t("entryBrowser.descending")}
                          </option>
                        )}
                      </For>
                    </select>
                  </label>
                  <button
                    class="ui-button ui-button-secondary"
                    type="button"
                    aria-label={t("entryBrowser.remove")}
                    onClick={() =>
                      props.onSort(
                        props.source.query.sort.filter((_, at) =>
                          at !== index()
                        ),
                      )}
                  >
                    {t("entryBrowser.remove")}
                  </button>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </section>

      <section aria-label={t("composition.resultPages")}>
        <h3 class="ui-label">{t("composition.resultPages")}</h3>
        <div role="radiogroup" aria-label={t("entryBrowser.selectedFields")}>
          <label>
            <input
              type="radio"
              name={`projection-${props.source.draftId}`}
              checked={props.source.query.projection.kind === "preview"}
              onChange={() => props.onProjection({ kind: "preview" })}
            />
            {t("entryBrowser.preview")}
          </label>
          <label>
            <input
              type="radio"
              name={`projection-${props.source.draftId}`}
              checked={props.source.query.projection.kind === "fields"}
              onChange={() => {
                const current = props.source.query.projection;
                props.onProjection(
                  current.kind === "fields"
                    ? current
                    : { kind: "fields", fields: [...schemaFieldIds()] },
                );
              }}
            />
            {t("entryBrowser.selectedFields")}
          </label>
        </div>
        <Show when={props.source.query.projection.kind === "fields"}>
          <ul class="ui-stack-sm">
            <For each={schemaFieldIds()}>
              {(fieldId) => {
                const selected = () => {
                  const projection = props.source.query.projection;
                  return projection.kind === "fields" &&
                    projection.fields.includes(fieldId);
                };
                return (
                  <li>
                    <label class="pill">
                      <input
                        type="checkbox"
                        checked={selected()}
                        onChange={(event) => {
                          const projection = props.source.query.projection;
                          const fields = projection.kind === "fields"
                            ? [...projection.fields]
                            : [];
                          if (event.currentTarget.checked) {
                            if (!fields.includes(fieldId)) {
                              fields.push(fieldId);
                            }
                          } else {
                            const at = fields.indexOf(fieldId);
                            if (at >= 0) fields.splice(at, 1);
                          }
                          props.onProjection({ kind: "fields", fields });
                        }}
                      />
                      <span>{fieldId}</span>
                    </label>
                  </li>
                );
              }}
            </For>
          </ul>
        </Show>
        <EntryQuerySourceResult
          source={props.source}
          planSource={planSource()}
          sourceState={sourceState()}
          unavailable={unavailable()}
          onNext={() => props.onNext(props.source.draftId)}
          onPrevious={() => props.onPrevious(props.source.draftId)}
          onRetry={() => props.onRetry(props.source.draftId)}
        />
      </section>

      <details class="ui-stack-sm">
        <summary>{props.source.name}</summary>
        <p class="ui-muted">
          {t("composition.studioFormDetail", { form: props.source.formId })}
        </p>
      </details>
    </div>
  );
}

function EntryQuerySourceResult(props: {
  source: EntryQuerySource;
  planSource:
    | Extract<CompositionResolvedSource, { kind: "entry_query" }>
    | undefined;
  sourceState: CompositionSourcePageState | undefined;
  unavailable: boolean;
  onNext: () => void;
  onPrevious: () => void;
  onRetry: () => void;
}) {
  const page = () => {
    const state = props.sourceState?.page;
    return state?.kind === "entry_query" ? state.page : undefined;
  };
  const rows = () => page()?.rows ?? [];
  const status = () => props.sourceState?.status;
  const loading = () => !props.sourceState || status() === "loading";

  return (
    <div>
      <Show when={props.unavailable}>
        <p class="ui-text-danger" role="alert">
          {t("composition.diagnostic.source_unavailable")}
        </p>
      </Show>
      <Show when={!props.unavailable && loading()}>
        <LocalBusyIndicator label={t("composition.queryLoading")} />
      </Show>
      <Show when={!props.unavailable && status() === "error"}>
        <p class="ui-text-danger" role="alert">
          {t("composition.queryFailed")}
        </p>
        <button
          class="ui-button ui-button-secondary"
          type="button"
          onClick={props.onRetry}
        >
          {t("composition.retry")}
        </button>
      </Show>
      <Show
        when={!props.unavailable && status() === "ready" &&
          rows().length === 0}
      >
        <p class="ui-muted">{t("composition.queryEmpty")}</p>
      </Show>
      <Show
        when={!props.unavailable && status() === "ready" &&
          rows().length > 0 && props.planSource}
      >
        {(resolved) => (
          <>
            <EntryResultTable
              columns={entryQueryDisplayColumns(resolved(), rows()).map((
                column,
              ) => ({
                key: column.key,
                label: column.label,
                cell: (row) => (
                  <span title={column.text(row)}>{column.text(row)}</span>
                ),
              }))}
              rows={rows()}
              pageIdentity={`${props.source.draftId}:${
                props.sourceState?.cursor ?? "first"
              }`}
              tableLabel={t("composition.resultPages")}
            />
            <ResultPagination
              canPrevious={(props.sourceState?.cursorStack.length ?? 0) > 1}
              canNext={!!page()?.has_more && !!page()?.next}
              busy={status() !== "ready"}
              previousLabel={t("composition.previous")}
              nextLabel={t("composition.next")}
              ariaLabel={t("composition.resultPages")}
              onPrevious={props.onPrevious}
              onNext={props.onNext}
            />
          </>
        )}
      </Show>
    </div>
  );
}
