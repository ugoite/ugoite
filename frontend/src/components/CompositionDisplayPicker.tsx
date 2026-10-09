import {
  createEffect,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
} from "solid-js";
import { Portal } from "solid-js/web";
import { RowList, RowListButton, RowListItem } from "~/components/RowList";
import { UiIcon } from "~/components/UiIcon";
import type { CompositionResultType } from "~/lib/composition-api";
import type {
  DraftMetricValueField,
  DraftSource,
} from "~/lib/composition-draft";
import { MAX_ENTRY_PROJECTION_FIELDS } from "~/lib/composition-draft";
import { t } from "~/lib/i18n";

export type CompositionDisplaySeed =
  | { kind: "table"; sourceDraftId: string; label?: string }
  | {
    kind: "metric";
    sourceDraftId: string;
    valueField: DraftMetricValueField;
    label?: string;
  };

interface CompositionDisplayPickerProps {
  sources: readonly DraftSource[];
  fieldNames?: (formId: string, fieldId: number) => string | undefined;
  fieldProjectable?: (formId: string, fieldId: number) => boolean | undefined;
  fieldNamesLoading?: boolean;
  initialKind?: "table" | "metric";
  initialSourceDraftId?: string | null;
  autoAddSingleCandidate?: boolean;
  onAdd: (seed: CompositionDisplaySeed) => void;
  onChooseSource?: (kind: "table" | "metric") => void;
  onClose: () => void;
}

export interface DisplayScalarCandidate {
  id: string;
  name: string;
  valueField: DraftMetricValueField;
}

/**
 * Scalar candidates for a Saved SQL source. Mirrors
 * `CompositionResultFieldType::supports_metric`: every declared result type
 * except json stays selectable. The shared evaluator stays authoritative.
 */
export const savedSqlScalarCandidates = (
  expectedResult: readonly { name: string; type: CompositionResultType }[],
): DisplayScalarCandidate[] =>
  expectedResult
    .filter((column) => column.type !== "json")
    .map((column) => ({
      id: column.name,
      name: column.name,
      valueField: { column: column.name },
    }));

/**
 * Structural pre-filter for EntryQuery field schema. Field types that can
 * never hold an exact scalar stay unselectable; the shared evaluator
 * (`composition_metric_result_type`) stays authoritative and reports the
 * rest through resolve diagnostics.
 */
const nonScalarFieldTypes = new Set([
  "list",
  "object_list",
  "row_reference",
  "asset_reference",
]);

export const entryQueryScalarCandidates = (
  fieldSchema: readonly { field_id: number; field_type: string }[],
  formId: string,
  fieldNames?: (formId: string, fieldId: number) => string | undefined,
  fieldProjectable?: (formId: string, fieldId: number) => boolean | undefined,
): DisplayScalarCandidate[] => {
  const resolvedNames = new Map(
    fieldSchema.map((entry) => [
      entry.field_id,
      fieldNames?.(formId, entry.field_id),
    ]),
  );
  const candidates: DisplayScalarCandidate[] = [];

  fieldSchema.forEach((entry) => {
    if (nonScalarFieldTypes.has(entry.field_type)) return;
    if (fieldProjectable?.(formId, entry.field_id) === false) return;
    const formName = resolvedNames.get(entry.field_id);
    // A schema ordinal is stable but not a human name. If the live Form
    // definition cannot name a field, keep it out of the picker rather than
    // teaching users an index they cannot identify later.
    if (!formName) return;
    candidates.push({
      id: String(entry.field_id),
      name: formName,
      valueField: { fieldId: entry.field_id },
    });
  });

  return candidates;
};

export const displayScalarCandidates = (
  source: DraftSource,
  fieldNames?: (formId: string, fieldId: number) => string | undefined,
  fieldProjectable?: (formId: string, fieldId: number) => boolean | undefined,
): DisplayScalarCandidate[] => {
  if (source.kind === "saved_sql") {
    return savedSqlScalarCandidates(source.expectedResult);
  }
  const candidates = entryQueryScalarCandidates(
    source.fieldSchema,
    source.formId,
    fieldNames,
    fieldProjectable,
  );
  if (source.query.projection.kind !== "fields") return candidates;
  const projectedFields = new Set(source.query.projection.fields);
  let additionalCapacity = Math.max(
    0,
    MAX_ENTRY_PROJECTION_FIELDS - projectedFields.size,
  );
  return candidates.filter((candidate) => {
    if (!("fieldId" in candidate.valueField)) return false;
    if (projectedFields.has(candidate.valueField.fieldId)) return true;
    if (additionalCapacity === 0) return false;
    additionalCapacity -= 1;
    return true;
  });
};

const sourceKindLabel = (source: DraftSource): string =>
  source.kind === "saved_sql"
    ? t("spaceShell.title.savedSql")
    : t("common.form");

/**
 * Display picker for the Composition Studio. The user chooses Table or
 * Metric in one tablist, then chooses a draft source. Metric
 * sources without scalar candidates stay disabled with the unavailable
 * reason. Value identity stays `{column}` for Saved SQL and `{fieldId}` for
 * EntryQuery; the Browser never aggregates or infers.
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
  let dialog: HTMLDivElement | undefined;
  let opener: HTMLElement | null = null;

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
    props.sources.find((source) => source.draftId === sourceDraftId());

  const candidates = (): DisplayScalarCandidate[] => {
    const source = selectedSource();
    return source
      ? displayScalarCandidates(
        source,
        props.fieldNames,
        props.fieldProjectable,
      )
      : [];
  };

  const hasScalarSchemaFields = (source: DraftSource): boolean =>
    source.kind === "saved_sql"
      ? source.expectedResult.some((column) => column.type !== "json")
      : source.fieldSchema.some((field) =>
        !nonScalarFieldTypes.has(field.field_type)
      );

  const hasNamedScalarFields = (source: DraftSource): boolean =>
    source.kind === "entry_query" &&
    source.fieldSchema.some((field) =>
      !nonScalarFieldTypes.has(field.field_type) &&
      Boolean(props.fieldNames?.(source.formId, field.field_id))
    );

  const addTable = (source: DraftSource) => {
    props.onAdd({ kind: "table", sourceDraftId: source.draftId });
  };

  const addMetric = (
    source: DraftSource,
    candidate: DisplayScalarCandidate,
  ) => {
    props.onAdd({
      kind: "metric",
      sourceDraftId: source.draftId,
      valueField: candidate.valueField,
    });
  };

  let addedInitialMetric = false;
  createEffect(() => {
    if (
      !props.autoAddSingleCandidate || addedInitialMetric ||
      kind() !== "metric" || props.fieldNamesLoading
    ) return;
    const source = selectedSource();
    const scalar = candidates();
    if (!source || scalar.length !== 1) return;
    addedInitialMetric = true;
    addMetric(source, scalar[0]);
  });

  const selectKind = (nextKind: "table" | "metric") => {
    if (kind() === nextKind) return;
    setKind(nextKind);
    setSourceDraftId(null);
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
    const scalar = displayScalarCandidates(
      source,
      props.fieldNames,
      props.fieldProjectable,
    );
    if (scalar.length === 1) {
      addMetric(source, scalar[0]);
      return;
    }
    setSourceDraftId(source.draftId);
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
                <For each={props.sources}>
                  {(source) => {
                    const scalar = () =>
                      displayScalarCandidates(
                        source,
                        props.fieldNames,
                        props.fieldProjectable,
                      );
                    const waitingForNames = () =>
                      kind() === "metric" && source.kind === "entry_query" &&
                      props.fieldNamesLoading;
                    const unavailable = () =>
                      kind() === "metric" && scalar().length === 0 &&
                      !waitingForNames();
                    return (
                      <RowListItem
                        main={
                          <Show
                            when={!unavailable()}
                            fallback={
                              <button
                                type="button"
                                class="rowListMain"
                                disabled
                                aria-label={source.name}
                                title={source.name}
                              >
                                <span class="rowListText">
                                  <span class="rowListPrimary">
                                    <span class="rowListName">
                                      {source.name}
                                    </span>
                                  </span>
                                  <span class="rowListSecondary">
                                    {waitingForNames()
                                      ? t("composition.studioSourcesLoading")
                                      : hasScalarSchemaFields(source) &&
                                          !hasNamedScalarFields(source)
                                      ? t(
                                        "composition.studioMetricFieldsUnavailable",
                                      )
                                      : source.kind === "entry_query"
                                      ? t("composition.studioNoMetricFields")
                                      : t("composition.studioNoCandidates")}
                                  </span>
                                </span>
                              </button>
                            }
                          >
                            <RowListButton
                              ariaLabel={source.name}
                              title={source.name}
                              selected={sourceDraftId() === source.draftId}
                              primary={
                                <span class="rowListName">{source.name}</span>
                              }
                              secondary={sourceKindLabel(source)}
                              onActivate={() => selectSource(source)}
                            />
                          </Show>
                        }
                      />
                    );
                  }}
                </For>
              </RowList>
              <Show when={kind() === "metric" && selectedSource()}>
                {(source) => (
                  <Show
                    when={!props.fieldNamesLoading}
                    fallback={
                      <p class="ui-muted">
                        {t("composition.studioSourcesLoading")}
                      </p>
                    }
                  >
                    <RowList label={source().name}>
                      <For each={candidates()}>
                        {(candidate) => (
                          <RowListItem
                            main={
                              <RowListButton
                                ariaLabel={`${candidate.name}, ${source().name}`}
                                title={candidate.name}
                                primary={
                                  <span class="rowListName">
                                    {candidate.name}
                                  </span>
                                }
                                secondary={source().name}
                                onActivate={() =>
                                  addMetric(source(), candidate)}
                              />
                            }
                          />
                        )}
                      </For>
                    </RowList>
                  </Show>
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
