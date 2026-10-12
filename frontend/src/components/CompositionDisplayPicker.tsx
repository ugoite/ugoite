import { createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { RowList, RowListButton, RowListItem } from "~/components/RowList";
import { UiIcon } from "~/components/UiIcon";
import type { CompositionResultType } from "~/lib/composition-api";
import type {
  CompositionDraft,
  DraftAggregateOperation,
  DraftMetricDefinition,
  DraftSource,
} from "~/lib/composition-draft";
import { metricDefinitionReady } from "~/lib/composition-draft";
import { supportedCompositionAggregateFieldTypes } from "~/lib/composition-metric-capability";
import { t } from "~/lib/i18n";

export type CompositionDisplaySeed =
  | { kind: "table"; sourceDraftId: string; label?: string }
  | {
    kind: "metric";
    sourceDraftId: string;
    definition: DraftMetricDefinition;
    label?: string;
  };

export interface MetricPickerPreview {
  ok: boolean;
  value?: unknown;
  error?: string;
}

interface CompositionDisplayPickerProps {
  draft: CompositionDraft;
  fieldNames?: (formId: string, fieldId: number) => string | undefined;
  fieldNamesLoading?: boolean;
  initialKind?: "table" | "metric";
  initialSourceDraftId?: string | null;
  onAdd: (seed: CompositionDisplaySeed) => void;
  onPreview?: (seed: {
    sourceDraftId: string;
    definition: DraftMetricDefinition;
  }) => Promise<MetricPickerPreview>;
  onChooseSource?: (kind: "table" | "metric") => void;
  onClose: () => void;
}

interface AggregateFieldCapabilities {
  sourceDraftId: string;
  operation: Exclude<DraftAggregateOperation, "count_rows">;
  supportedFieldTypes: ReadonlySet<string>;
}

export interface DisplayScalarCandidate {
  id: string;
  name: string;
  definition: DraftMetricDefinition;
}

/**
 * Scalar candidates for a Saved SQL source. Mirrors
 * `CompositionResultFieldType::supports_metric`: every declared result type
 * except json stays selectable. The shared evaluator stays authoritative.
 */
export const savedSqlScalarCandidates = (
  expectedResult: readonly { name: string; type: CompositionResultType }[],
): DisplayScalarCandidate[] => {
  const nameCounts = new Map<string, number>();
  for (const column of expectedResult) {
    nameCounts.set(column.name, (nameCounts.get(column.name) ?? 0) + 1);
  }
  return expectedResult
    .filter((column) =>
      column.type !== "json" && nameCounts.get(column.name) === 1
    )
    .map((column) => ({
      id: column.name,
      name: column.name,
      definition: { kind: "sql_scalar", column: column.name },
    }));
};

export const entryQueryAggregateCandidates = (
  fieldSchema: readonly { field_id: number; field_type: string }[],
  formId: string,
  operation: Exclude<DraftAggregateOperation, "count_rows">,
  supportedFieldTypes: ReadonlySet<string>,
  fieldNames?: (formId: string, fieldId: number) => string | undefined,
): DisplayScalarCandidate[] => {
  const resolvedNames = new Map(
    fieldSchema.map((entry) => [
      entry.field_id,
      fieldNames?.(formId, entry.field_id),
    ]),
  );
  const candidates: DisplayScalarCandidate[] = [];

  fieldSchema.forEach((entry) => {
    if (!supportedFieldTypes.has(entry.field_type)) return;
    const formName = resolvedNames.get(entry.field_id);
    // A schema ordinal is stable but not a human name. If the live Form
    // definition cannot name a field, keep it out of the picker rather than
    // teaching users an index they cannot identify later.
    if (!formName) return;
    candidates.push({
      id: String(entry.field_id),
      name: formName,
      definition: {
        kind: "aggregate",
        operation,
        fieldId: entry.field_id,
      },
    });
  });

  return candidates;
};

export const metricFieldCandidates = (
  source: DraftSource,
  operation: Exclude<DraftAggregateOperation, "count_rows">,
  supportedFieldTypes: ReadonlySet<string>,
  fieldNames?: (formId: string, fieldId: number) => string | undefined,
): DisplayScalarCandidate[] => {
  if (source.kind === "saved_sql") return [];
  return entryQueryAggregateCandidates(
    source.fieldSchema,
    source.formId,
    operation,
    supportedFieldTypes,
    fieldNames,
  );
};

const sourceKindLabel = (source: DraftSource): string =>
  source.kind === "saved_sql"
    ? t("spaceShell.title.savedSql")
    : t("common.form");

const aggregateOperations: readonly DraftAggregateOperation[] = [
  "count_rows",
  "count_values",
  "sum",
  "avg",
  "min",
  "max",
];

const aggregateOperationLabel = (operation: DraftAggregateOperation) =>
  t(
    `composition.metricOperation.${operation}` as
      | "composition.metricOperation.count_rows"
      | "composition.metricOperation.count_values"
      | "composition.metricOperation.sum"
      | "composition.metricOperation.avg"
      | "composition.metricOperation.min"
      | "composition.metricOperation.max",
  );

/**
 * Display picker for the Composition Studio. Form metrics are configured as
 * typed aggregates; Saved SQL metrics bind one declared scalar column.
 * Preview happens before the draft is inserted on the canvas.
 */
export function CompositionDisplayPicker(
  props: CompositionDisplayPickerProps,
) {
  const titleId = "composition-display-picker-title";
  const [kind, setKind] = createSignal<"table" | "metric">(
    props.initialKind ?? "table",
  );
  const [sourceDraftId, setSourceDraftId] = createSignal<string | null>(
    props.initialSourceDraftId ?? null,
  );
  const initialMetricSource = props.draft.sources.find((source) =>
    source.draftId === props.initialSourceDraftId
  );
  const [metricDefinition, setMetricDefinition] = createSignal<
    DraftMetricDefinition | undefined
  >(
    initialMetricSource?.kind === "entry_query"
      ? { kind: "aggregate", operation: "count_rows" }
      : initialMetricSource?.kind === "saved_sql"
      ? { kind: "sql_scalar" }
      : undefined,
  );
  const [preview, setPreview] = createSignal<MetricPickerPreview | undefined>();
  const [previewing, setPreviewing] = createSignal(false);
  let previewRequest = 0;
  let dialog: HTMLDivElement | undefined;
  let opener: HTMLElement | null = null;

  const invalidatePreview = () => {
    previewRequest += 1;
    setPreview(undefined);
    setPreviewing(false);
  };

  onCleanup(() => {
    previewRequest += 1;
  });

  onMount(() => {
    opener = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    const appRoot = document.getElementById("app");
    appRoot?.setAttribute("inert", "");
    queueMicrotask(() =>
      dialog?.querySelector<HTMLElement>("button:not([disabled])")?.focus()
    );
    onCleanup(() => {
      appRoot?.removeAttribute("inert");
      const target = opener;
      opener = null;
      queueMicrotask(() => {
        if (target?.isConnected) target.focus();
      });
    });
  });

  const selectedSource = (): DraftSource | undefined =>
    props.draft.sources.find((source) => source.draftId === sourceDraftId());

  const aggregateOperation = (): DraftAggregateOperation | undefined => {
    const definition = metricDefinition();
    return definition?.kind === "aggregate" ? definition.operation : undefined;
  };

  const [fieldCapabilities, setFieldCapabilities] = createSignal<
    AggregateFieldCapabilities | undefined
  >();
  const [fieldCapabilitiesLoading, setFieldCapabilitiesLoading] = createSignal(
    false,
  );
  let fieldCapabilityRequest = 0;

  const currentSupportedFieldTypes = (): ReadonlySet<string> => {
    const result = fieldCapabilities();
    return result?.sourceDraftId === sourceDraftId() &&
        result.operation === aggregateOperation()
      ? result.supportedFieldTypes
      : new Set<string>();
  };

  const invalidateFieldCapabilities = () => {
    fieldCapabilityRequest += 1;
    setFieldCapabilities(undefined);
    setFieldCapabilitiesLoading(false);
  };

  const loadFieldCapabilities = async (
    source: Extract<DraftSource, { kind: "entry_query" }>,
    operation: Exclude<DraftAggregateOperation, "count_rows">,
  ) => {
    const request = ++fieldCapabilityRequest;
    setFieldCapabilities(undefined);
    setFieldCapabilitiesLoading(true);
    const supportedFieldTypes = await supportedCompositionAggregateFieldTypes(
      operation,
      source.fieldSchema.map((entry) => entry.field_type),
    );
    if (request !== fieldCapabilityRequest) return;
    setFieldCapabilities({
      sourceDraftId: source.draftId,
      operation,
      supportedFieldTypes,
    });
    setFieldCapabilitiesLoading(false);
  };

  const fieldCandidates = (): DisplayScalarCandidate[] => {
    const source = selectedSource();
    const operation = aggregateOperation();
    return source?.kind === "entry_query" && operation &&
        operation !== "count_rows"
      ? metricFieldCandidates(
        source,
        operation,
        currentSupportedFieldTypes(),
        props.fieldNames,
      )
      : [];
  };

  const sqlCandidates = (): DisplayScalarCandidate[] => {
    const source = selectedSource();
    return source?.kind === "saved_sql"
      ? savedSqlScalarCandidates(source.expectedResult)
      : [];
  };

  const selectedFieldId = () => {
    const definition = metricDefinition();
    return definition?.kind === "aggregate" &&
        definition.fieldId !== undefined
      ? String(definition.fieldId)
      : "";
  };

  const selectedSqlColumn = () => {
    const definition = metricDefinition();
    return definition?.kind === "sql_scalar" ? definition.column ?? "" : "";
  };

  const hasReadyDefinition = () => {
    const definition = metricDefinition();
    return Boolean(
      definition &&
        metricDefinitionReady(props.draft, sourceDraftId() ?? "", definition),
    );
  };

  const addTable = (source: DraftSource) => {
    props.onAdd({ kind: "table", sourceDraftId: source.draftId });
  };

  const previewMetric = async () => {
    const selected = selectedSource();
    const definition = metricDefinition();
    if (!selected || !definition || !hasReadyDefinition() || !props.onPreview) {
      return;
    }
    const request = ++previewRequest;
    setPreviewing(true);
    setPreview(undefined);
    try {
      const result = await props.onPreview({
        sourceDraftId: selected.draftId,
        definition,
      });
      if (request === previewRequest) setPreview(result);
    } catch {
      if (request === previewRequest) {
        setPreview({ ok: false, error: t("composition.queryFailed") });
      }
    } finally {
      if (request === previewRequest) setPreviewing(false);
    }
  };

  const addSelectedMetric = () => {
    const source = selectedSource();
    const definition = metricDefinition();
    if (!source || !definition || !hasReadyDefinition() || !preview()?.ok) {
      return;
    }
    props.onAdd({
      kind: "metric",
      sourceDraftId: source.draftId,
      definition,
    });
  };

  const selectKind = (nextKind: "table" | "metric") => {
    if (kind() === nextKind) return;
    invalidateFieldCapabilities();
    setKind(nextKind);
    setSourceDraftId(null);
    setMetricDefinition(undefined);
    invalidatePreview();
  };

  const handleKindTabKeyDown = (event: KeyboardEvent) => {
    if (
      event.key !== "ArrowLeft" && event.key !== "ArrowRight" &&
      event.key !== "Home" && event.key !== "End"
    ) return;
    event.preventDefault();
    const currentKind = kind();
    const nextKind = event.key === "Home"
      ? "table"
      : event.key === "End"
      ? "metric"
      : event.key === "ArrowRight"
      ? currentKind === "table" ? "metric" : "table"
      : currentKind === "metric"
      ? "table"
      : "metric";
    selectKind(nextKind);
    dialog?.querySelector<HTMLButtonElement>(
      `[data-display-kind="${nextKind}"]`,
    )?.focus();
  };

  const selectSource = (source: DraftSource) => {
    if (kind() === "table") {
      addTable(source);
      return;
    }
    invalidateFieldCapabilities();
    invalidatePreview();
    setSourceDraftId(source.draftId);
    setMetricDefinition(
      source.kind === "entry_query"
        ? { kind: "aggregate", operation: "count_rows" }
        : { kind: "sql_scalar" },
    );
  };

  const updateOperation = (operation: DraftAggregateOperation) => {
    invalidatePreview();
    setMetricDefinition({ kind: "aggregate", operation });
    const source = selectedSource();
    if (source?.kind === "entry_query" && operation !== "count_rows") {
      void loadFieldCapabilities(source, operation).catch(() => {
        invalidateFieldCapabilities();
      });
    } else {
      invalidateFieldCapabilities();
    }
  };

  const updateField = (fieldId: string) => {
    const definition = metricDefinition();
    if (
      definition?.kind !== "aggregate" ||
      definition.operation === "count_rows"
    ) return;
    invalidatePreview();
    setMetricDefinition({
      kind: "aggregate",
      operation: definition.operation,
      ...(fieldId ? { fieldId: Number(fieldId) } : {}),
    });
  };

  const updateSqlColumn = (column: string) => {
    invalidatePreview();
    setMetricDefinition({
      kind: "sql_scalar",
      ...(column ? { column } : {}),
    });
  };

  const handleKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      props.onClose();
      return;
    }
    if (event.key === "Tab") {
      const container = event.currentTarget as HTMLElement;
      const focusable = Array.from(
        container.querySelectorAll<HTMLElement>(
          'button:not([disabled]):not([tabindex="-1"]), input:not([disabled]), select:not([disabled])',
        ),
      );
      if (focusable.length === 0) {
        event.preventDefault();
        return;
      }
      const currentIndex = focusable.indexOf(
        document.activeElement as HTMLElement,
      );
      const nextIndex = event.shiftKey
        ? currentIndex <= 0 ? focusable.length - 1 : currentIndex - 1
        : currentIndex < 0 || currentIndex === focusable.length - 1
        ? 0
        : currentIndex + 1;
      event.preventDefault();
      focusable[nextIndex].focus();
    }
  };

  return (
    <Portal>
      <div
        class="ui-backdrop"
        onClick={(event) => {
          if (event.target === event.currentTarget) props.onClose();
        }}
      >
        <div
          ref={dialog}
          class="ui-dialog composition-display-picker"
          role="dialog"
          aria-modal="true"
          aria-labelledby={titleId}
          onKeyDown={handleKeyDown}
        >
          <h2 id={titleId} class="ui-dialog-title">
            {t("composition.studioAddDataComponent")}
          </h2>
          <div class="ui-stack-sm">
            <div
              class="tabs compositionDisplayKind"
              role="tablist"
              aria-label={t("composition.studioDataComponentType")}
              onKeyDown={handleKindTabKeyDown}
            >
              <button
                type="button"
                id="composition-display-table-tab"
                data-display-kind="table"
                role="tab"
                aria-selected={kind() === "table"}
                aria-controls="composition-display-options"
                tabIndex={kind() === "table" ? 0 : -1}
                class="tab"
                classList={{ active: kind() === "table" }}
                onClick={() => selectKind("table")}
              >
                <UiIcon name="canvas-table" />
                <span>{t("composition.studioTable")}</span>
              </button>
              <button
                type="button"
                id="composition-display-metric-tab"
                data-display-kind="metric"
                role="tab"
                aria-selected={kind() === "metric"}
                aria-controls="composition-display-options"
                tabIndex={kind() === "metric" ? 0 : -1}
                class="tab"
                classList={{ active: kind() === "metric" }}
                onClick={() => selectKind("metric")}
              >
                <UiIcon name="canvas-metric" />
                <span>{t("composition.studioMetric")}</span>
              </button>
            </div>
            <section
              id="composition-display-options"
              role="tabpanel"
              aria-labelledby={kind() === "table"
                ? "composition-display-table-tab"
                : "composition-display-metric-tab"}
            >
              <RowList label={t("composition.studioSources")}>
                <For each={props.draft.sources}>
                  {(source) => {
                    const unavailable = () =>
                      kind() === "metric" && source.kind === "saved_sql" &&
                      savedSqlScalarCandidates(source.expectedResult).length ===
                        0;
                    return (
                      <RowListItem
                        main={
                          <RowListButton
                            ariaLabel={source.name}
                            title={source.name}
                            selected={sourceDraftId() === source.draftId}
                            disabled={unavailable()}
                            primary={
                              <span class="rowListName">{source.name}</span>
                            }
                            secondary={sourceKindLabel(source)}
                            onActivate={() => selectSource(source)}
                          />
                        }
                      />
                    );
                  }}
                </For>
              </RowList>
              <Show when={kind() === "metric" && selectedSource()}>
                {(source) => (
                  <div class="ui-stack-sm compositionMetricSetup">
                    <Show when={source().kind === "entry_query"}>
                      <div class="ui-field">
                        <label
                          class="ui-label"
                          for="composition-metric-operation"
                        >
                          {t("composition.studioOperation")}
                        </label>
                        <select
                          id="composition-metric-operation"
                          class="ui-input"
                          value={aggregateOperation() ?? "count_rows"}
                          onChange={(event) =>
                            updateOperation(
                              event.currentTarget
                                .value as DraftAggregateOperation,
                            )}
                        >
                          <For each={aggregateOperations}>
                            {(operation) => (
                              <option value={operation}>
                                {aggregateOperationLabel(operation)}
                              </option>
                            )}
                          </For>
                        </select>
                      </div>
                      <Show
                        when={aggregateOperation() !== undefined &&
                          aggregateOperation() !== "count_rows"}
                      >
                        <div class="ui-field">
                          <label
                            class="ui-label"
                            for="composition-metric-field"
                          >
                            {t("composition.studioField")}
                          </label>
                          <select
                            id="composition-metric-field"
                            class="ui-input"
                            value={selectedFieldId()}
                            disabled={props.fieldNamesLoading ||
                              fieldCapabilitiesLoading()}
                            onChange={(event) =>
                              updateField(event.currentTarget.value)}
                          >
                            <option value="" disabled>
                              {t("composition.studioChooseField")}
                            </option>
                            <For each={fieldCandidates()}>
                              {(candidate) => (
                                <option value={candidate.id}>
                                  {candidate.name}
                                </option>
                              )}
                            </For>
                          </select>
                        </div>
                      </Show>
                    </Show>
                    <Show when={source().kind === "saved_sql"}>
                      <div class="ui-field">
                        <label
                          class="ui-label"
                          for="composition-metric-column"
                        >
                          {t("composition.studioColumn")}
                        </label>
                        <select
                          id="composition-metric-column"
                          class="ui-input"
                          value={selectedSqlColumn()}
                          onChange={(event) =>
                            updateSqlColumn(event.currentTarget.value)}
                        >
                          <option value="" disabled>
                            {t("composition.studioChooseColumn")}
                          </option>
                          <For each={sqlCandidates()}>
                            {(candidate) => (
                              <option value={candidate.id}>
                                {candidate.name}
                              </option>
                            )}
                          </For>
                        </select>
                      </div>
                    </Show>
                    <Show when={preview()?.ok}>
                      <output
                        class="compositionMetricPreview"
                        aria-label={preview()?.value === null
                          ? t("composition.metricNoData")
                          : t("composition.studioMetricPreview")}
                      >
                        {preview()?.value === null
                          ? "—"
                          : typeof preview()?.value === "string" ||
                              typeof preview()?.value === "number" ||
                              typeof preview()?.value === "boolean"
                          ? String(preview()?.value)
                          : t("composition.metricUnavailable")}
                      </output>
                    </Show>
                    <Show when={preview() && !preview()?.ok}>
                      <p class="ui-text-danger" role="alert">
                        {preview()?.error ?? t("composition.metricUnavailable")}
                      </p>
                    </Show>
                    <div class="ui-dialog-actions">
                      <button
                        type="button"
                        class="ui-button ui-button-secondary"
                        disabled={!hasReadyDefinition() || !props.onPreview ||
                          previewing()}
                        onClick={() => void previewMetric()}
                      >
                        {previewing()
                          ? t("composition.queryLoading")
                          : t("composition.studioPreviewMetric")}
                      </button>
                      <button
                        type="button"
                        class="ui-button ui-button-primary"
                        disabled={!hasReadyDefinition() || !preview()?.ok}
                        onClick={addSelectedMetric}
                      >
                        <UiIcon name="plus" />
                        <span>{t("composition.studioInsert")}</span>
                      </button>
                    </div>
                  </div>
                )}
              </Show>
              <button
                class="ui-button ui-button-secondary composition-source-browse"
                type="button"
                onClick={() => props.onChooseSource?.(kind())}
              >
                <UiIcon name="plus" />
                <span>{t("composition.studioChooseFormOrSql")}</span>
              </button>
            </section>
          </div>
          <div class="ui-dialog-actions">
            <button
              type="button"
              class="ui-button ui-button-secondary"
              onClick={() => props.onClose()}
            >
              {t("common.cancel")}
            </button>
          </div>
        </div>
      </div>
    </Portal>
  );
}
