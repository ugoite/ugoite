import { createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { RowList, RowListButton, RowListItem } from "~/components/RowList";
import { UiIcon } from "~/components/UiIcon";
import type { CompositionResultType } from "~/lib/composition-api";
import type { CompositionDraft, DraftSource } from "~/lib/composition-draft";
import { metricColumnReady } from "~/lib/composition-draft";
import { t } from "~/lib/i18n";

export type CompositionDisplaySeed =
  | { kind: "table"; sourceDraftId: string; label?: string }
  | {
    kind: "metric";
    sourceDraftId: string;
    column: string;
    label?: string;
  };

export interface MetricPickerPreview {
  ok: boolean;
  value?: unknown;
  error?: string;
}

interface CompositionDisplayPickerProps {
  draft: CompositionDraft;
  initialKind?: "table" | "metric";
  initialSourceDraftId?: string | null;
  onAdd: (seed: CompositionDisplaySeed) => void;
  onPreview?: (seed: { sourceDraftId: string; column: string }) => Promise<MetricPickerPreview>;
  onChooseSource?: (kind: "table" | "metric") => void;
  onClose: () => void;
}

export interface DisplayScalarCandidate {
  id: string;
  name: string;
}

/** Saved SQL result columns that can be selected as a Metric scalar. */
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
    .map((column) => ({ id: column.name, name: column.name }));
};

const sourceKindLabel = (source: DraftSource): string =>
  source.kind === "saved_sql"
    ? t("spaceShell.title.savedSql")
    : t("common.form");

/** Add a Table or a SQL-backed Metric to the Composition canvas. */
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
  const [column, setColumn] = createSignal<string>();
  const [preview, setPreview] = createSignal<MetricPickerPreview>();
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
  const metricSources = () =>
    props.draft.sources.filter((source) => source.kind === "saved_sql");
  const sqlCandidates = () => {
    const source = selectedSource();
    return source?.kind === "saved_sql"
      ? savedSqlScalarCandidates(source.expectedResult)
      : [];
  };
  const metricReady = () =>
    metricColumnReady(props.draft, sourceDraftId() ?? "", column());

  const addTable = (source: DraftSource) => {
    props.onAdd({ kind: "table", sourceDraftId: source.draftId });
  };

  const selectSource = (source: DraftSource) => {
    if (kind() === "table") {
      addTable(source);
      return;
    }
    if (source.kind !== "saved_sql") return;
    invalidatePreview();
    setSourceDraftId(source.draftId);
    setColumn(undefined);
  };

  const selectKind = (nextKind: "table" | "metric") => {
    if (kind() === nextKind) return;
    setKind(nextKind);
    setSourceDraftId(null);
    setColumn(undefined);
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

  const updateColumn = (nextColumn: string) => {
    invalidatePreview();
    setColumn(nextColumn || undefined);
  };

  const previewMetric = async () => {
    const source = selectedSource();
    const selectedColumn = column();
    if (
      source?.kind !== "saved_sql" || !selectedColumn || !metricReady() ||
      !props.onPreview
    ) return;
    const request = ++previewRequest;
    setPreviewing(true);
    setPreview(undefined);
    try {
      const result = await props.onPreview({
        sourceDraftId: source.draftId,
        column: selectedColumn,
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
    const selectedColumn = column();
    if (
      source?.kind !== "saved_sql" || !selectedColumn || !metricReady() ||
      !preview()?.ok
    ) return;
    props.onAdd({
      kind: "metric",
      sourceDraftId: source.draftId,
      column: selectedColumn,
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
                <For each={kind() === "metric" ? metricSources() : props.draft.sources}>
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
              <Show when={kind() === "metric" && selectedSource()?.kind === "saved_sql"}>
                <div class="ui-stack-sm compositionMetricSetup">
                  <div class="ui-field">
                    <label class="ui-label" for="composition-metric-column">
                      {t("composition.studioColumn")}
                    </label>
                    <select
                      id="composition-metric-column"
                      class="ui-input"
                      value={column() ?? ""}
                      onChange={(event) =>
                        updateColumn(event.currentTarget.value)}
                    >
                      <option value="" disabled>
                        {t("composition.studioChooseColumn")}
                      </option>
                      <For each={sqlCandidates()}>
                        {(candidate) => (
                          <option value={candidate.id}>{candidate.name}</option>
                        )}
                      </For>
                    </select>
                  </div>
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
                      disabled={!metricReady() || !props.onPreview || previewing()}
                      onClick={() => void previewMetric()}
                    >
                      {previewing()
                        ? t("composition.queryLoading")
                        : t("composition.studioPreviewMetric")}
                    </button>
                    <button
                      type="button"
                      class="ui-button ui-button-primary"
                      disabled={!metricReady() || !preview()?.ok}
                      onClick={addSelectedMetric}
                    >
                      <UiIcon name="plus" />
                      <span>{t("composition.studioInsert")}</span>
                    </button>
                  </div>
                </div>
              </Show>
              <button
                class="ui-button ui-button-secondary composition-source-browse"
                type="button"
                onClick={() => props.onChooseSource?.(kind())}
              >
                <UiIcon name="plus" />
                <span>{kind() === "metric"
                  ? t("spaceShell.title.savedSql")
                  : t("composition.studioChooseFormOrSql")}</span>
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
