import {
  createEffect,
  createMemo,
  createSignal,
  For,
  onCleanup,
  Show,
} from "solid-js";
import { EntryResultTable } from "~/components/EntryResultTable";
import {
  type EntryTableColumnOption,
  EntryTableColumnPicker,
} from "~/components/EntryTableColumnPicker";
import { EntryBrowserDisplayDialog } from "~/components/EntryBrowserDisplayDialog";
import type {
  EntryBrowserDisplayMode,
} from "~/components/EntryBrowserDisplayDialog";
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
  EntryQueryCompositionSystemField,
} from "~/lib/entry-query-composition";
import {
  type DraftSource,
  MAX_ENTRY_PROJECTION_FIELDS,
} from "~/lib/composition-draft";
import type {
  EntryFieldCapability,
  EntryFilter,
  EntrySort,
} from "~/lib/entry-query";
import { systemEntryCapabilities } from "~/lib/entry-query";
import {
  fetchStudioFormDefinition,
  studioBindingName,
  studioBindingText,
  studioCapabilitiesFromForm,
  studioEntryFilterToComposition,
  studioEntrySortToComposition,
  studioFallbackCapabilities,
  studioFieldNames,
  studioFilterToEntryFilter,
  studioSortToEntrySort,
} from "~/lib/entry-query-studio-capabilities";
import type { CompositionSourcePageState } from "~/lib/composition-query-handle";
import type { Form } from "~/lib/types";
import { t } from "~/lib/i18n";

export type EntryQuerySource = Extract<DraftSource, { kind: "entry_query" }>;

const filterValueText = (value: unknown): string => {
  const binding = studioBindingName(value);
  if (binding !== undefined) return studioBindingText(binding);
  if (value === null || value === undefined) return "";
  return String(value);
};

const operatorLabel = (operator: EntryQueryCompositionFilter["operator"]) =>
  t(
    `entryBrowser.operator.${operator}` as
      | "entryBrowser.operator.equals"
      | "entryBrowser.operator.contains"
      | "entryBrowser.operator.lt"
      | "entryBrowser.operator.lte"
      | "entryBrowser.operator.gt"
      | "entryBrowser.operator.gte",
  );

type EditorDefinitionLoad =
  | { status: "loading" }
  | { status: "ready"; form: Form | undefined }
  | { status: "error" };

interface EntryQuerySourceEditorProps {
  spaceId: string;
  source: EntryQuerySource;
  requiredMetricFieldIds?: readonly number[];
  /** Narrow draft updaters; each returns false when the edit is rejected. */
  onFilters: (filters: EntryQueryCompositionFilter[]) => boolean;
  onSort: (sort: EntryQueryCompositionSort[]) => boolean;
  onProjection: (projection: EntryQueryCompositionProjection) => boolean;
  onDisplaySystemFields: (
    fields: EntryQueryCompositionSystemField[],
  ) => boolean;
  planSources: readonly CompositionResolvedSource[];
  sourceStates: Record<string, CompositionSourcePageState>;
  diagnostics: readonly CompositionResolveDiagnostic[];
  /** True while the shared debounced preview is in flight. The result
   * spinner renders only while active; settled without a page renders
   * nothing. */
  previewActive: boolean;
  onNext: (sourceId: string) => void;
  onPrevious: (sourceId: string) => void;
  onRetry: (sourceId: string) => void;
}

/**
 * EntryQuery source editor for the Data workspace. Filter and sort edits
 * route through the shared EntryBrowserDisplayDialog with capabilities and
 * human names from the transient Form definition; the draft Knowledge
 * keeps only field IDs. Projection stays inline: the dialog's column
 * contract (EntryFieldRef refs with system fields) is not identical to the
 * Composition numeric-IDs-only projection. The result preview reuses the
 * shared per-source preview page; no new query implementation.
 */
export function EntryQuerySourceEditor(props: EntryQuerySourceEditorProps) {
  // Transient Work only: the referenced Form definition supplies names and
  // capabilities, cached per source. Loading and error states are explicit;
  // any failure degrades to the schema snapshot, never blocks editing, and
  // is never stored. The fetch follows the manual signal pattern used by the
  // source viewer: explicit resolve/reject handlers, no unhandled rejection.
  const [definitionLoad, setDefinitionLoad] = createSignal<
    EditorDefinitionLoad
  >(
    { status: "loading" },
  );
  let definitionGeneration = 0;
  let loadedDefinitionKey: string | undefined;
  const loadDefinition = (spaceId: string, formId: string) => {
    const generation = ++definitionGeneration;
    setDefinitionLoad({ status: "loading" });
    fetchStudioFormDefinition(spaceId, formId).then(
      (form) => {
        if (definitionGeneration === generation) {
          setDefinitionLoad({ status: "ready", form });
        }
      },
      () => {
        if (definitionGeneration === generation) {
          setDefinitionLoad({ status: "error" });
        }
      },
    );
  };
  createEffect(() => {
    // Rerun the transient load only when the referenced source changes.
    // The source prop is a fresh object on every draft update, so guard on
    // the stable key instead of reloading (and flashing) on each edit.
    const spaceId = props.spaceId;
    const formId = props.source.formId;
    const key = `${spaceId}/${formId}`;
    if (key === loadedDefinitionKey) return;
    loadedDefinitionKey = key;
    loadDefinition(spaceId, formId);
  });
  onCleanup(() => {
    definitionGeneration += 1;
  });
  const definitionForm = (): Form | undefined => {
    const load = definitionLoad();
    return load.status === "ready" ? load.form : undefined;
  };
  const names = createMemo(() => studioFieldNames(definitionForm()));
  const fallbackFieldName = (index: number): string =>
    t("entryBrowser.fieldOrdinal", { number: index + 1 });
  const fallbackFieldLabels = createMemo(() => {
    const labels = new Map<number, string>();
    const usedLabels = new Set(names().values());
    const assignFallback = (fieldId: number, startIndex: number) => {
      if (labels.has(fieldId) || names().has(fieldId)) return;
      let index = startIndex;
      let label = fallbackFieldName(index);
      while (usedLabels.has(label)) {
        index += 1;
        label = fallbackFieldName(index);
      }
      labels.set(fieldId, label);
      usedLabels.add(label);
    };

    props.source.fieldSchema.forEach((entry, index) => {
      assignFallback(entry.field_id, index);
    });

    const schemaFieldIds = new Set(
      props.source.fieldSchema.map((entry) => entry.field_id),
    );
    const referencedFieldIds = new Set([
      ...props.source.query.filters.map((filter) => filter.field_id),
      ...props.source.query.sort.map((clause) => clause.field_id),
      ...(props.source.query.projection.kind === "fields"
        ? props.source.query.projection.fields
        : []),
    ]);
    const staleFieldIds = [...referencedFieldIds].filter((fieldId) =>
      !schemaFieldIds.has(fieldId) && !names().has(fieldId)
    ).sort((left, right) => left - right);
    staleFieldIds.forEach((fieldId, index) => {
      assignFallback(fieldId, props.source.fieldSchema.length + index);
    });
    return labels;
  });
  const fieldName = (fieldId: number): string => {
    const knownName = names().get(fieldId);
    if (knownName) return knownName;
    // Allocate fallbacks around live Form names and earlier fallbacks so
    // unknown fields remain distinguishable even when a Form uses an
    // ordinal-looking name such as "Field 3".
    return fallbackFieldLabels().get(fieldId) ??
      fallbackFieldName(props.source.fieldSchema.length);
  };
  const capabilities = createMemo((): EntryFieldCapability[] => {
    const form = definitionForm();
    if (!form) {
      return studioFallbackCapabilities(
        props.source.fieldSchema,
        (_entry, index) => fallbackFieldName(index),
      );
    }
    const snapshottedFieldIds = new Set(
      props.source.fieldSchema.map((entry) => entry.field_id),
    );
    return studioCapabilitiesFromForm(form).filter((field) =>
      field.field.kind === "property" &&
      snapshottedFieldIds.has(field.field.field_id)
    );
  });

  const [dialogMode, setDialogMode] = createSignal<
    EntryBrowserDisplayMode | null
  >(
    null,
  );
  const [dialogTrigger, setDialogTrigger] = createSignal<
    HTMLElement | undefined
  >(undefined);
  const openDialog = (mode: "filter" | "sort", trigger: HTMLElement) => {
    setDialogTrigger(trigger);
    setDialogMode(mode);
  };
  const closeDialog = () => setDialogMode(null);

  const applyDialogFilters = (filters: EntryFilter[]) => {
    const next: EntryQueryCompositionFilter[] = [];
    for (const filter of filters) {
      const converted = studioEntryFilterToComposition(filter);
      // No draft grammar for the ref: fail closed and keep the dialog open.
      if (!converted) return;
      next.push(converted);
    }
    if (props.onFilters(next)) closeDialog();
  };

  const applyDialogSort = (sort: EntrySort[]) => {
    const next: EntryQueryCompositionSort[] = [];
    for (const clause of sort) {
      const converted = studioEntrySortToComposition(clause);
      if (!converted) return;
      next.push(converted);
    }
    if (props.onSort(next)) closeDialog();
  };

  // Genuinely incapable fields never reach the dialog's own dropdowns. With
  // zero capable fields the Add control disables with the reason in its
  // accessible name and title, mirroring the Add-parameter gate; the reason
  // reuses existing vocabulary and renders no visible prose.
  const filterCapable = () =>
    capabilities().some((field) =>
      field.filterable && field.supported_operators.length > 0
    );
  const sortCapable = () => capabilities().some((field) => field.sortable);
  const addFilterLabel = () =>
    filterCapable()
      ? t("entryBrowser.addFilter")
      : `${t("entryBrowser.addFilter")}: ${
        t("entryBrowser.noFilterCapabilities")
      }`;
  const addSortLabel = () =>
    sortCapable()
      ? t("entryBrowser.addSort")
      : `${t("entryBrowser.addSort")}: ${t("entryBrowser.noSortCapabilities")}`;

  const schemaFieldIds = () => {
    const snapshotFieldIds = props.source.fieldSchema.map((entry) =>
      entry.field_id
    );
    const form = definitionForm();
    if (!form) return snapshotFieldIds;
    const projectableFieldIds = new Set(
      studioCapabilitiesFromForm(form).flatMap((field) =>
        field.field.kind === "property" && field.projectable
          ? [field.field.field_id]
          : []
      ),
    );
    const selectedFieldIds = new Set(
      props.source.query.projection.kind === "fields"
        ? props.source.query.projection.fields
        : [],
    );
    return snapshotFieldIds.filter((fieldId) =>
      projectableFieldIds.has(fieldId) || selectedFieldIds.has(fieldId)
    );
  };
  const previewSchemaIsComplete = () => {
    if (props.source.query.projection.kind === "preview") return true;
    const form = definitionForm();
    if (!form) return false;
    const schemaById = new Map(
      props.source.fieldSchema.map((entry) => [entry.field_id, entry]),
    );
    const fields = studioCapabilitiesFromForm(form);
    return schemaById.size === props.source.fieldSchema.length &&
      fields.length === schemaById.size &&
      fields.every((field) =>
        field.field.kind === "property" &&
        schemaById.get(field.field.field_id)?.field_type === field.field_type
      );
  };
  const requiredMetricFields =
    () => [...new Set(props.requiredMetricFieldIds ?? [])];
  const initialProjectionFieldIds = () => {
    const required = requiredMetricFields();
    const remaining = schemaFieldIds().filter((fieldId) =>
      !required.includes(fieldId)
    );
    return [
      ...required,
      ...remaining.slice(
        0,
        Math.max(0, MAX_ENTRY_PROJECTION_FIELDS - required.length),
      ),
    ];
  };
  const projectionRecoveryBlocked = () => {
    if (requiredMetricFields().length > MAX_ENTRY_PROJECTION_FIELDS) {
      return true;
    }
    const projection = props.source.query.projection;
    const projected = projection.kind === "fields" ? projection.fields : [];
    const missingMetricFields =
      requiredMetricFields().filter((fieldId) => !projected.includes(fieldId))
        .length;
    // Individual removals can only recover the draft when at most one slot
    // is missing. Larger deficits require the metric bindings to be edited.
    return projected.length + missingMetricFields -
        MAX_ENTRY_PROJECTION_FIELDS > 1;
  };

  const propertyColumnKey = (fieldId: number) => `field:${fieldId}`;
  const systemColumnKey = (field: EntryQueryCompositionSystemField) =>
    `system:${field}`;
  const systemFieldLabel = (field: EntryQueryCompositionSystemField) =>
    systemEntryCapabilities({ kind: "form", form_id: props.source.formId })
      .fields.find((capability) => capability.field.kind === field)?.name ??
      field;
  const columnOptions = createMemo((): EntryTableColumnOption[] => {
    const projection = props.source.query.projection;
    const projected = projection.kind === "fields"
      ? new Set(projection.fields)
      : new Set<number>();
    const systemFields = new Set(
      props.source.query.display_system_fields ?? [],
    );
    const propertyOptions = projection.kind === "fields"
      ? schemaFieldIds().map((fieldId) => {
        const selected = projected.has(fieldId);
        const requiredByMetric = requiredMetricFields().includes(fieldId);
        return {
          key: propertyColumnKey(fieldId),
          label: fieldName(fieldId),
          selected,
          disabled: projectionRecoveryBlocked() ||
            (selected && requiredByMetric),
          disabledReason: projectionRecoveryBlocked()
            ? t("composition.studioProjectionRecoveryRequired")
            : selected && requiredByMetric
            ? t("composition.studioMetricProjectionRequired")
            : undefined,
        };
      })
      : [];
    const systemOptions: EntryTableColumnOption[] = ([
      "created_at",
      "updated_at",
    ] as const).map((field) => ({
      key: systemColumnKey(field),
      label: systemFieldLabel(field),
      selected: systemFields.has(field),
      countsTowardSelectionLimit: false,
    }));
    return [...propertyOptions, ...systemOptions];
  });
  const projectionFieldCount = (selectedKeys: readonly string[]) => {
    const selected = new Set(
      selectedKeys.flatMap((key) => {
        const match = /^field:(\d+)$/.exec(key);
        return match ? [Number(match[1])] : [];
      }),
    );
    const missingMetricFields =
      requiredMetricFields().filter((fieldId) => !selected.has(fieldId)).length;
    return selected.size + missingMetricFields;
  };
  const applyColumns = (selectedKeys: string[]) => {
    const fieldIds = selectedKeys.flatMap((key) => {
      const match = /^field:(\d+)$/.exec(key);
      return match ? [Number(match[1])] : [];
    });
    if (props.source.query.projection.kind === "fields") {
      const nextProjection = fieldIds.length > 0
        ? { kind: "fields" as const, fields: fieldIds }
        : { kind: "preview" as const };
      if (!props.onProjection(nextProjection)) return;
    }
    props.onDisplaySystemFields(
      (["created_at", "updated_at"] as const).filter((field) =>
        selectedKeys.includes(systemColumnKey(field))
      ),
    );
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
        <Show when={definitionLoad().status === "loading"}>
          <LocalBusyIndicator label={t("composition.studioSourcesLoading")} />
        </Show>
        <Show when={definitionLoad().status === "error"}>
          <p class="ui-text-danger" role="alert">
            {t("composition.studioSourcesFailed")}
          </p>
          <button
            class="ui-button ui-button-secondary"
            type="button"
            onClick={() => loadDefinition(props.spaceId, props.source.formId)}
          >
            {t("composition.retry")}
          </button>
        </Show>
        <Show
          when={props.source.fieldSchema.length > 0}
          fallback={<p class="ui-muted">{t("composition.queryEmpty")}</p>}
        >
          <ul class="ui-stack-sm">
            <For each={props.source.fieldSchema}>
              {(entry) => (
                <li>
                  <span class="pill">
                    <span>{fieldName(entry.field_id)}</span>
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
            disabled={!filterCapable()}
            aria-label={addFilterLabel()}
            title={addFilterLabel()}
            onClick={(event) => openDialog("filter", event.currentTarget)}
          >
            {t("entryBrowser.addFilter")}
          </button>
        </div>
        <Show when={props.source.query.filters.length > 0}>
          <ul class="ui-stack-sm">
            <For each={props.source.query.filters}>
              {(filter, index) => (
                <li class="flex flex-wrap items-center gap-2">
                  <span>
                    {fieldName(filter.field_id)}{" "}
                    {operatorLabel(filter.operator)}{" "}
                    {filterValueText(filter.value)}
                  </span>
                  <button
                    class="ui-button ui-button-secondary"
                    type="button"
                    aria-label={t("composition.studioEdit")}
                    onClick={(event) =>
                      openDialog("filter", event.currentTarget)}
                  >
                    {t("composition.studioEdit")}
                  </button>
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
            disabled={!sortCapable()}
            aria-label={addSortLabel()}
            title={addSortLabel()}
            onClick={(event) => openDialog("sort", event.currentTarget)}
          >
            {t("entryBrowser.addSort")}
          </button>
        </div>
        <Show when={props.source.query.sort.length > 0}>
          <ul class="ui-stack-sm">
            <For each={props.source.query.sort}>
              {(clause, index) => (
                <li class="flex flex-wrap items-center gap-2">
                  <span>
                    {fieldName(clause.field_id)} · {clause.direction ===
                        "asc"
                      ? t("entryBrowser.ascending")
                      : t("entryBrowser.descending")}
                  </span>
                  <button
                    class="ui-button ui-button-secondary"
                    type="button"
                    aria-label={t("composition.studioEdit")}
                    onClick={(event) => openDialog("sort", event.currentTarget)}
                  >
                    {t("composition.studioEdit")}
                  </button>
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
              disabled={(props.requiredMetricFieldIds?.length ?? 0) > 0 ||
                !previewSchemaIsComplete()}
              aria-describedby={(props.requiredMetricFieldIds?.length ?? 0) >
                  0
                ? `metric-projection-lock-${props.source.draftId}`
                : !previewSchemaIsComplete()
                ? `preview-schema-lock-${props.source.draftId}`
                : undefined}
              title={(props.requiredMetricFieldIds?.length ?? 0) > 0
                ? t("composition.studioMetricProjectionRequired")
                : !previewSchemaIsComplete()
                ? t("composition.studioPreviewRequiresSchema")
                : undefined}
              onChange={() => props.onProjection({ kind: "preview" })}
            />
            {t("entryBrowser.preview")}
          </label>
          <Show when={(props.requiredMetricFieldIds?.length ?? 0) > 0}>
            <span
              class="ui-sr-only"
              id={`metric-projection-lock-${props.source.draftId}`}
            >
              {t("composition.studioMetricProjectionRequired")}
            </span>
          </Show>
          <Show when={!previewSchemaIsComplete()}>
            <span
              class="ui-sr-only"
              id={`preview-schema-lock-${props.source.draftId}`}
            >
              {t("composition.studioPreviewRequiresSchema")}
            </span>
          </Show>
          <label>
            <input
              type="radio"
              name={`projection-${props.source.draftId}`}
              checked={props.source.query.projection.kind === "fields"}
              disabled={requiredMetricFields().length >
                MAX_ENTRY_PROJECTION_FIELDS}
              aria-describedby={requiredMetricFields().length >
                  MAX_ENTRY_PROJECTION_FIELDS
                ? `metric-projection-recovery-${props.source.draftId}`
                : undefined}
              title={requiredMetricFields().length >
                  MAX_ENTRY_PROJECTION_FIELDS
                ? t("composition.studioProjectionRecoveryRequired")
                : undefined}
              onChange={() => {
                const current = props.source.query.projection;
                props.onProjection(
                  current.kind === "fields" ? current : {
                    kind: "fields",
                    fields: initialProjectionFieldIds(),
                  },
                );
              }}
            />
            {t("entryBrowser.selectedFields")}
          </label>
          <Show when={projectionRecoveryBlocked()}>
            <span
              class="ui-sr-only"
              id={`metric-projection-recovery-${props.source.draftId}`}
            >
              {t("composition.studioProjectionRecoveryRequired")}
            </span>
          </Show>
        </div>
        <EntryTableColumnPicker
          options={columnOptions}
          selectionLimit={{
            maximum: MAX_ENTRY_PROJECTION_FIELDS,
            reason: t("composition.studioProjectionLimitReached"),
            countSelected: projectionFieldCount,
          }}
          canApply={(keys) => !projectionRecoveryBlocked() &&
            projectionFieldCount(keys) <= MAX_ENTRY_PROJECTION_FIELDS}
          onApply={applyColumns}
        />
        <EntryQuerySourceResult
          source={props.source}
          planSource={planSource()}
          sourceState={sourceState()}
          unavailable={unavailable()}
          previewActive={props.previewActive}
          onNext={() => props.onNext(props.source.draftId)}
          onPrevious={() => props.onPrevious(props.source.draftId)}
          onRetry={() => props.onRetry(props.source.draftId)}
        />
      </section>

      <Show when={dialogMode()}>
        {(mode) => (
          <EntryBrowserDisplayDialog
            mode={mode()}
            returnFocus={dialogTrigger()}
            fields={capabilities()}
            projection={{ kind: "preview" }}
            previewSystemFields={[]}
            filters={props.source.query.filters.map(studioFilterToEntryFilter)}
            sort={props.source.query.sort.map(studioSortToEntrySort)}
            onApply={(draft) => {
              if (mode() === "filter" && draft.filters) {
                applyDialogFilters(draft.filters);
              } else if (mode() === "sort" && draft.sort) {
                applyDialogSort(draft.sort);
              }
            }}
            onClose={closeDialog}
          />
        )}
      </Show>
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
  previewActive: boolean;
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
  // The spinner renders only while the shared preview is in flight. Settled
  // without a page for this source renders nothing here; the diagnostics
  // strip already covers failures.
  const showSpinner = () =>
    !props.unavailable && props.previewActive && loading();

  return (
    <div>
      <Show when={props.unavailable}>
        <p class="ui-text-danger" role="alert">
          {t("composition.diagnostic.source_unavailable")}
        </p>
      </Show>
      <Show when={showSpinner()}>
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
