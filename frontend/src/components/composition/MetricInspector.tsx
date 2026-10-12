import { For, Show } from "solid-js";
import type { JSX } from "solid-js";
import {
  type CompositionInspectorDataJump,
  type InspectorApply,
  InspectorDataJumpButton,
  InspectorLabelField,
  InspectorShell,
  InspectorSourceField,
} from "~/components/composition/InspectorFields";
import { savedSqlScalarCandidates } from "~/components/CompositionDisplayPicker";
import {
  type CompositionDraft,
  type DraftDisplay,
  type DraftMetricPresentation,
  metricColumnIsNumeric,
  setMetricColumn,
  setMetricPresentation,
  setMetricSource,
} from "~/lib/composition-draft";
import { t } from "~/lib/i18n";

/** Metric edits select one Saved SQL column and presentation settings only. */
export function MetricInspector(props: {
  draft: CompositionDraft;
  display: DraftDisplay;
  apply: InspectorApply;
  onDataJump: (jump: CompositionInspectorDataJump) => void;
  actions?: JSX.Element;
}) {
  const metric = () =>
    props.display.kind === "metric" ? props.display : undefined;
  const sourceId = () => metric()?.sourceDraftId ?? "";
  const source = () =>
    props.draft.sources.find((entry) => entry.draftId === sourceId());
  const columnValue = () => metric()?.column ?? "";
  const sqlCandidates = () => {
    const currentSource = source();
    return currentSource?.kind === "saved_sql"
      ? savedSqlScalarCandidates(currentSource.expectedResult)
      : [];
  };
  const supportsNumberPresentation = () => {
    const currentSource = source();
    const column = metric()?.column;
    return currentSource !== undefined &&
      metricColumnIsNumeric(currentSource, column);
  };

  const updatePresentation = (
    values: Omit<DraftMetricPresentation, "kind">,
  ) => {
    const current = metric();
    if (!current) return;
    props.apply(
      setMetricPresentation(props.draft, current.draftId, {
        kind: "number",
        ...(values.unit === undefined ? {} : { unit: values.unit }),
        ...(values.fractionDigits === undefined
          ? {}
          : { fractionDigits: values.fractionDigits }),
      }),
    );
  };

  const pickUnit = (unit: string) => {
    const fractionDigits = metric()?.presentation?.fractionDigits;
    updatePresentation({
      unit,
      ...(fractionDigits === undefined ? {} : { fractionDigits }),
    });
  };

  const pickFractionDigits = (value: string) => {
    const unit = metric()?.presentation?.unit;
    updatePresentation({
      ...(unit === undefined ? {} : { unit }),
      ...(value === "" ? {} : { fractionDigits: Number(value) }),
    });
  };

  const pickSource = (nextSourceDraftId: string) => {
    const current = metric();
    if (!current || nextSourceDraftId === current.sourceDraftId) return;
    props.apply(
      setMetricSource(props.draft, current.draftId, nextSourceDraftId),
    );
  };

  const pickColumn = (column: string) => {
    const current = metric();
    if (!current) return;
    props.apply(
      setMetricColumn(props.draft, current.draftId, column || undefined),
    );
  };

  return (
    <InspectorShell
      icon="canvas-metric"
      title={t("composition.studioMetric")}
      actions={props.actions}
    >
      <Show when={metric()}>
        {(entry) => (
          <>
            <InspectorLabelField
              display={entry()}
              draft={props.draft}
              apply={props.apply}
            />
            <InspectorSourceField
              draft={props.draft}
              sourceDraftId={sourceId()}
              sourceKind="saved_sql"
              fieldId={`studio-inspector-source-${entry().draftId}`}
              onPick={pickSource}
            />
            <Show when={source()?.kind === "saved_sql"}>
              <div class="ui-field">
                <label
                  class="ui-label"
                  for={`studio-inspector-column-${entry().draftId}`}
                >
                  {t("composition.studioColumn")}
                </label>
                <select
                  id={`studio-inspector-column-${entry().draftId}`}
                  class="ui-input"
                  value={columnValue()}
                  onChange={(event) => pickColumn(event.currentTarget.value)}
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
            </Show>
            <Show when={supportsNumberPresentation()}>
              <div class="ui-field">
                <label
                  class="ui-label"
                  for={`studio-inspector-metric-unit-${entry().draftId}`}
                >
                  {t("composition.studioMetricUnit")}
                </label>
                <input
                  id={`studio-inspector-metric-unit-${entry().draftId}`}
                  class="ui-input"
                  value={entry().presentation?.unit ?? ""}
                  onInput={(event) => pickUnit(event.currentTarget.value)}
                  onBlur={(event) => pickUnit(event.currentTarget.value.trim())}
                />
              </div>
              <div class="ui-field">
                <label
                  class="ui-label"
                  for={`studio-inspector-metric-fraction-digits-${entry().draftId}`}
                >
                  {t("composition.studioMetricFractionDigits")}
                </label>
                <input
                  id={`studio-inspector-metric-fraction-digits-${entry().draftId}`}
                  class="ui-input"
                  type="number"
                  min="0"
                  max="255"
                  step="1"
                  value={entry().presentation?.fractionDigits?.toString() ?? ""}
                  onInput={(event) =>
                    pickFractionDigits(event.currentTarget.value)}
                />
              </div>
            </Show>
            <InspectorDataJumpButton
              sourceDraftId={sourceId()}
              onDataJump={props.onDataJump}
            />
          </>
        )}
      </Show>
    </InspectorShell>
  );
}
