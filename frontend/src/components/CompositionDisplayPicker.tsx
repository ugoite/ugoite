import { createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { RowList, RowListButton, RowListItem } from "~/components/RowList";
import type { CompositionResultType } from "~/lib/composition-api";
import type {
  DraftMetricValueField,
  DraftSource,
} from "~/lib/composition-draft";
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
  onAdd: (seed: CompositionDisplaySeed) => void;
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
  "binary",
  "list",
  "object_list",
  "asset_reference",
]);

export const entryQueryScalarCandidates = (
  fieldSchema: readonly { field_id: number; field_type: string }[],
): DisplayScalarCandidate[] =>
  fieldSchema
    .filter((entry) => !nonScalarFieldTypes.has(entry.field_type))
    .map((entry) => ({
      id: String(entry.field_id),
      name: `#${entry.field_id}`,
      valueField: { fieldId: entry.field_id },
    }));

export const displayScalarCandidates = (
  source: DraftSource,
): DisplayScalarCandidate[] =>
  source.kind === "saved_sql"
    ? savedSqlScalarCandidates(source.expectedResult)
    : entryQueryScalarCandidates(source.fieldSchema);

const sourceKindLabel = (source: DraftSource): string =>
  source.kind === "saved_sql"
    ? t("spaceShell.title.savedSql")
    : t("common.form");

/**
 * Display picker for the Composition Studio. Chooses a table or metric kind,
 * then a draft source (metric sources without scalar candidates stay
 * disabled with the unavailable reason), then a scalar value and an
 * optional label. Value identity stays `{column}` for Saved SQL and
 * `{fieldId}` for EntryQuery; the Browser never aggregates or infers.
 */
export function CompositionDisplayPicker(
  props: CompositionDisplayPickerProps,
) {
  const titleId = "composition-display-picker-title";
  const valueSelectId = "composition-display-value";
  const labelInputId = "composition-display-label";
  const [kind, setKind] = createSignal<"table" | "metric" | null>(null);
  const [sourceDraftId, setSourceDraftId] = createSignal<string | null>(null);
  const [valueId, setValueId] = createSignal<string | null>(null);
  const [label, setLabel] = createSignal("");
  let dialog: HTMLDivElement | undefined;
  let opener: HTMLElement | null = null;

  onMount(() => {
    opener = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    const appRoot = document.getElementById("app");
    appRoot?.setAttribute("inert", "");
    queueMicrotask(() => dialog?.querySelector("button")?.focus());
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
    return source ? displayScalarCandidates(source) : [];
  };

  const selectKind = (next: "table" | "metric") => {
    setKind(next);
    setSourceDraftId(null);
    setValueId(null);
  };

  const selectSource = (source: DraftSource) => {
    setSourceDraftId(source.draftId);
    const scalar = displayScalarCandidates(source);
    setValueId(scalar.length > 0 ? scalar[0].id : null);
  };

  const canAdd = (): boolean => {
    const selectedKind = kind();
    const source = selectedSource();
    if (!selectedKind || !source) return false;
    if (selectedKind === "table") return true;
    return valueId() !== null &&
      candidates().some((candidate) => candidate.id === valueId());
  };

  const handleAdd = () => {
    const selectedKind = kind();
    const source = selectedSource();
    if (!selectedKind || !source || !canAdd()) return;
    const trimmed = label().trim();
    const maybeLabel = trimmed ? { label: trimmed } : {};
    if (selectedKind === "table") {
      props.onAdd({
        kind: "table",
        sourceDraftId: source.draftId,
        ...maybeLabel,
      });
      return;
    }
    const candidate = candidates().find((entry) => entry.id === valueId());
    if (!candidate) return;
    props.onAdd({
      kind: "metric",
      sourceDraftId: source.draftId,
      valueField: candidate.valueField,
      ...maybeLabel,
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
          "button:not([disabled]), input, select",
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
          class="ui-dialog"
          role="dialog"
          aria-modal="true"
          aria-labelledby={titleId}
          onKeyDown={handleKeyDown}
        >
          <h2 id={titleId} class="ui-dialog-title">
            {t("composition.studioAddDisplay")}
          </h2>
          <div class="ui-stack-sm">
            <RowList label={t("composition.studioDisplay")}>
              <RowListItem
                main={
                  <RowListButton
                    ariaLabel={t("composition.studioTable")}
                    primary={<span>{t("composition.studioTable")}</span>}
                    onActivate={() => selectKind("table")}
                  />
                }
              />
              <RowListItem
                main={
                  <RowListButton
                    ariaLabel={t("composition.studioMetric")}
                    primary={<span>{t("composition.studioMetric")}</span>}
                    onActivate={() => selectKind("metric")}
                  />
                }
              />
            </RowList>
            <Show when={kind()}>
              {(selectedKind) => (
                <>
                  <h3 class="ui-label">{t("composition.studioSource")}</h3>
                  <RowList label={t("composition.studioSource")}>
                    <For each={props.sources}>
                      {(source) => {
                        const scalar = () => displayScalarCandidates(source);
                        const unavailable = () =>
                          selectedKind() === "metric" &&
                          scalar().length === 0;
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
                                  >
                                    <span class="rowListText">
                                      <span class="rowListPrimary">
                                        <span>{source.name}</span>
                                      </span>
                                      <span class="rowListSecondary">
                                        {t(
                                          "composition.studioNoCandidates",
                                        )}
                                      </span>
                                    </span>
                                  </button>
                                }
                              >
                                <RowListButton
                                  ariaLabel={source.name}
                                  primary={<span>{source.name}</span>}
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
                  <Show
                    when={selectedKind() === "metric" && selectedSource()}
                  >
                    <label class="ui-label" for={valueSelectId}>
                      {t("composition.studioValue")}
                    </label>
                    <select
                      id={valueSelectId}
                      class="ui-input"
                      value={valueId() ?? ""}
                      onChange={(event) =>
                        setValueId(event.currentTarget.value || null)}
                    >
                      <For each={candidates()}>
                        {(candidate) => (
                          <option value={candidate.id}>
                            {candidate.name}
                          </option>
                        )}
                      </For>
                    </select>
                  </Show>
                  <Show when={selectedSource()}>
                    <label class="ui-label" for={labelInputId}>
                      {t("composition.studioLabel")}
                    </label>
                    <input
                      id={labelInputId}
                      class="ui-input"
                      value={label()}
                      onInput={(event) => setLabel(event.currentTarget.value)}
                    />
                    <button
                      class="ui-button ui-button-secondary"
                      type="button"
                      disabled={!canAdd()}
                      onClick={handleAdd}
                    >
                      {t("composition.studioAddDisplay")}
                    </button>
                  </Show>
                </>
              )}
            </Show>
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
